use log::{error, info, warn};
use soma_common::{AgentMessage, CompositorMessage, TaskPlan, AGENT_SOCKET_PATH};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;

use crate::capabilities::CapabilityRegistry;
use crate::config::SomaConfig;
use crate::executor::Executor;
use crate::intent::IntentParser;

/// Pending plans awaiting user approval
type PendingPlans = Arc<Mutex<HashMap<String, TaskPlan>>>;

/// Conversation context — stores recent exchanges for follow-up resolution
const MAX_CONTEXT_ENTRIES: usize = 5;

pub async fn run_ipc_server() -> Result<(), Box<dyn std::error::Error>> {
    // Clean up old socket
    let _ = std::fs::remove_file(AGENT_SOCKET_PATH);

    let listener = UnixListener::bind(AGENT_SOCKET_PATH)?;
    info!("Agent daemon listening on {}", AGENT_SOCKET_PATH);

    let registry = Arc::new(CapabilityRegistry::new());
    let config = SomaConfig::load();
    info!("Loaded config: provider={} model={}", config.model.provider, config.model.model);

    let parser = Arc::new(Mutex::new(IntentParser::new(&config)));
    let executor = Arc::new(Executor::new());
    let pending: PendingPlans = Arc::new(Mutex::new(HashMap::new()));

    info!("Registered capabilities:");
    for cap in registry.list() {
        info!(
            "  • {} — {} ({} actions)",
            cap.name,
            cap.description,
            cap.actions.len()
        );
    }

    loop {
        let (stream, _) = listener.accept().await?;
        info!("New client connection");

        let registry = Arc::clone(&registry);
        let parser = Arc::clone(&parser);
        let executor = Arc::clone(&executor);
        let pending = Arc::clone(&pending);

        tokio::spawn(async move {
            let (reader, writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let writer = Arc::new(Mutex::new(writer));
            let mut line = String::new();

            // Per-client conversation context (pairs of user/agent strings)
            let mut context: Vec<(String, String)> = Vec::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        info!("Client disconnected");
                        break;
                    }
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }

                        match serde_json::from_str::<CompositorMessage>(trimmed) {
                            Ok(msg) => {
                                handle_message(
                                    msg,
                                    &parser,
                                    &executor,
                                    &registry,
                                    &pending,
                                    &writer,
                                    &mut context,
                                )
                                .await;
                            }
                            Err(e) => {
                                warn!("Failed to parse message: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Read error: {}", e);
                        break;
                    }
                }
            }
        });
    }
}

async fn handle_message(
    msg: CompositorMessage,
    parser: &Arc<Mutex<IntentParser>>,
    executor: &Executor,
    registry: &CapabilityRegistry,
    pending: &PendingPlans,
    writer: &Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    context: &mut Vec<(String, String)>,
) {
    let response = match msg {
        CompositorMessage::NaturalLanguageInput { text } => {
            let id = uuid::Uuid::new_v4().to_string();
            info!("Parsing NL input [{}]: {}", id, text);

            match parser.lock().await.parse(&text, "", None, context, registry).await {
                Ok(plan) => {
                    let summary = format!(
                        "Planned: {} ({} steps)",
                        plan.intent,
                        plan.steps.len()
                    );
                    context.push((text, summary));
                    if context.len() > MAX_CONTEXT_ENTRIES {
                        context.remove(0);
                    }
                    pending.lock().await.insert(id.clone(), plan.clone());
                    AgentMessage::TaskPlanReady { id, plan }
                }
                Err(e) => {
                    error!("Intent parse error: {}", e);
                    AgentMessage::Error { id, message: e }
                }
            }
        }

        CompositorMessage::ParseIntent { id, input } => {
            info!("Parsing intent [{}]: {}", id, input);
            match parser.lock().await.parse(&input, "", None, context, registry).await {
                Ok(plan) => {
                    let summary = format!(
                        "Planned: {} ({} steps)",
                        plan.intent,
                        plan.steps.len()
                    );
                    context.push((input, summary));
                    if context.len() > MAX_CONTEXT_ENTRIES {
                        context.remove(0);
                    }
                    pending.lock().await.insert(id.clone(), plan.clone());
                    AgentMessage::TaskPlanReady { id, plan }
                }
                Err(e) => {
                    error!("Intent parse error: {}", e);
                    AgentMessage::Error { id, message: e }
                }
            }
        }

        CompositorMessage::Approve { id } => {
            if let Some(plan) = pending.lock().await.remove(&id) {
                info!("Executing approved plan: {}", plan.intent);
                let mut results = Vec::new();

                for (i, step) in plan.steps.iter().enumerate() {
                    let result = executor.execute_step(step, registry);
                    info!(
                        "  Step {}: {}.{} → {}",
                        i,
                        step.capability,
                        step.action,
                        if result.success { "ok" } else { "error" }
                    );

                    let step_msg = AgentMessage::StepResult {
                        id: id.clone(),
                        step_index: i,
                        result: result.clone(),
                    };
                    send_message(&step_msg, writer).await;

                    // If a browser action produced a screenshot, push a BrowserUpdate
                    if step.capability == "browser" && result.success {
                        if result.data.get("screenshot_base64").is_some() {
                            let browser_msg = AgentMessage::BrowserUpdate {
                                url: result.data["url"]
                                    .as_str().unwrap_or("").to_string(),
                                title: result.data["title"]
                                    .as_str().unwrap_or("").to_string(),
                                screenshot_base64: result.data["screenshot_base64"]
                                    .as_str().map(|s| s.to_string()),
                            };
                            send_message(&browser_msg, writer).await;
                        }
                    }

                    results.push(result);
                }

                let ok = results.iter().filter(|r| r.success).count();
                if let Some(last) = context.last_mut() {
                    last.1 = format!(
                        "Executed: {} ({}/{} steps succeeded)",
                        plan.intent, ok, results.len()
                    );
                }

                AgentMessage::ExecutionComplete { id, results }
            } else {
                AgentMessage::Error {
                    id,
                    message: "No pending plan found for this ID".to_string(),
                }
            }
        }

        CompositorMessage::Reject { id } => {
            pending.lock().await.remove(&id);
            info!("Plan rejected: {}", id);
            return;
        }

        CompositorMessage::DirectExec { id, command } => {
            info!("Direct exec: {}", command);
            let result = executor.execute_raw(&command);
            AgentMessage::DirectOutput { id, result }
        }

        CompositorMessage::ListCapabilities => {
            let capabilities = registry.list();
            AgentMessage::Capabilities { capabilities }
        }

        CompositorMessage::UpdateConfig { provider, model, api_key, api_url } => {
            let id = uuid::Uuid::new_v4().to_string();
            let mut config = SomaConfig::load();
            config.model.provider = provider.clone();
            config.model.model    = model.clone();
            config.model.api_key  = api_key;
            config.model.api_url  = api_url;

            // Convert save error to String before any await to keep future Send
            let save_result = config.save().map_err(|e| e.to_string());
            match save_result {
                Ok(_) => {
                    parser.lock().await.set_provider(&config);
                    info!("Config updated: provider={} model={}", provider, model);
                    AgentMessage::ConfigUpdated { provider, model }
                }
                Err(e) => {
                    error!("Failed to save config: {}", e);
                    AgentMessage::Error { id, message: format!("Failed to save config: {}", e) }
                }
            }
        }

        CompositorMessage::Ping => AgentMessage::Pong,
    };

    send_message(&response, writer).await;
}

async fn send_message(
    msg: &AgentMessage,
    writer: &Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
) {
    if let Ok(json) = serde_json::to_string(msg) {
        let mut w = writer.lock().await;
        let line = format!("{}\n", json);
        if let Err(e) = w.write_all(line.as_bytes()).await {
            error!("Failed to send message: {}", e);
        }
    }
}
