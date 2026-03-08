use log::{error, info, warn};
use soma_common::{AgentMessage, CompositorMessage, TaskPlan, AGENT_SOCKET_PATH};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;

use crate::capabilities::CapabilityRegistry;
use crate::executor::Executor;
use crate::intent::{self, IntentParser};

/// Pending plans awaiting user approval
type PendingPlans = Arc<Mutex<HashMap<String, TaskPlan>>>;

pub async fn run_ipc_server() -> Result<(), Box<dyn std::error::Error>> {
    // Clean up old socket
    let _ = std::fs::remove_file(AGENT_SOCKET_PATH);

    let listener = UnixListener::bind(AGENT_SOCKET_PATH)?;
    info!("Agent daemon listening on {}", AGENT_SOCKET_PATH);

    let registry = Arc::new(CapabilityRegistry::new());
    let system_prompt = Arc::new(intent::build_system_prompt(&registry));
    let parser = Arc::new(IntentParser::new());
    let executor = Arc::new(Executor::new());
    let pending: PendingPlans = Arc::new(Mutex::new(HashMap::new()));

    info!("Registered capabilities:");
    for cap in registry.list() {
        info!("  • {} — {} ({} actions)", cap.name, cap.description, cap.actions.len());
    }

    loop {
        let (stream, _) = listener.accept().await?;
        info!("New client connection");

        let registry = Arc::clone(&registry);
        let system_prompt = Arc::clone(&system_prompt);
        let parser = Arc::clone(&parser);
        let executor = Arc::clone(&executor);
        let pending = Arc::clone(&pending);

        tokio::spawn(async move {
            let (reader, writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let writer = Arc::new(Mutex::new(writer));
            let mut line = String::new();

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
                                    &system_prompt,
                                    &pending,
                                    &writer,
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
    parser: &IntentParser,
    executor: &Executor,
    registry: &CapabilityRegistry,
    system_prompt: &str,
    pending: &PendingPlans,
    writer: &Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
) {
    let response = match msg {
        CompositorMessage::NaturalLanguageInput { text } => {
            // Generate a unique ID and parse
            let id = uuid::Uuid::new_v4().to_string();
            info!("Parsing NL input [{}]: {}", id, text);
            match parser.parse(&text, system_prompt).await {
                Ok(plan) => {
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
            match parser.parse(&input, system_prompt).await {
                Ok(plan) => {
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

                    // Send incremental step results
                    let step_msg = AgentMessage::StepResult {
                        id: id.clone(),
                        step_index: i,
                        result: result.clone(),
                    };
                    send_message(&step_msg, writer).await;
                    results.push(result);
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
            return; // No response needed
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
