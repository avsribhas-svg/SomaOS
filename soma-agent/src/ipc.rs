use log::{error, info, warn};
use soma_common::{AgentMessage, CompositorMessage, TaskPlan, AGENT_SOCKET_PATH};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;

use crate::executor::Executor;
use crate::intent::IntentParser;

/// Pending plans awaiting user approval
type PendingPlans = Arc<Mutex<HashMap<String, TaskPlan>>>;

pub async fn run_ipc_server() -> Result<(), Box<dyn std::error::Error>> {
    // Clean up old socket
    let _ = std::fs::remove_file(AGENT_SOCKET_PATH);

    let listener = UnixListener::bind(AGENT_SOCKET_PATH)?;
    info!("Agent daemon listening on {}", AGENT_SOCKET_PATH);

    let parser = Arc::new(IntentParser::new());
    let executor = Arc::new(Executor::new());
    let pending: PendingPlans = Arc::new(Mutex::new(HashMap::new()));

    loop {
        let (stream, _) = listener.accept().await?;
        info!("New compositor connection");

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
                        info!("Compositor disconnected");
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
    pending: &PendingPlans,
    writer: &Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
) {
    let response = match msg {
        CompositorMessage::ParseIntent { id, input } => {
            info!("Parsing intent: {}", input);
            match parser.parse(&input).await {
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
                    match executor.execute(&step.command, &step.args) {
                        Ok(result) => {
                            // Send incremental step results
                            let step_msg = AgentMessage::StepResult {
                                id: id.clone(),
                                step_index: i,
                                result: result.clone(),
                            };
                            send_message(&step_msg, writer).await;
                            results.push(result);
                        }
                        Err(e) => {
                            let err_msg = AgentMessage::Error {
                                id: id.clone(),
                                message: e,
                            };
                            send_message(&err_msg, writer).await;
                            return;
                        }
                    }
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

        CompositorMessage::ReadClipboard { id } => {
            // Basic clipboard read attempt
            let result = executor.execute_raw("pbpaste 2>/dev/null || xclip -selection clipboard -o 2>/dev/null || echo 'Clipboard unavailable'");
            AgentMessage::ClipboardContent {
                id,
                content: result.stdout,
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
