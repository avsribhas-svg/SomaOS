use soma_common::{AgentMessage, CompositorMessage, AGENT_SOCKET_PATH};
use std::io::{self, Write};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔══════════════════════════════════════╗");
    println!("║       SomaOS Agent CLI v0.2.0        ║");
    println!("╚══════════════════════════════════════╝");
    println!();
    println!("Connecting to agent at {}...", AGENT_SOCKET_PATH);

    let stream = UnixStream::connect(AGENT_SOCKET_PATH).await?;
    let (reader, writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let writer = std::sync::Arc::new(tokio::sync::Mutex::new(writer));

    println!("Connected! Type a command in natural language, or:");
    println!("  /exec <cmd>   — run a shell command directly");
    println!("  /caps         — list available capabilities");
    println!("  /quit         — exit");
    println!();

    // Spawn a task to read agent responses
    let response_writer = writer.clone();
    let response_handle = tokio::spawn(async move {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    println!("\n[Agent disconnected]");
                    break;
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<AgentMessage>(trimmed) {
                        Ok(msg) => handle_agent_message(msg, &response_writer).await,
                        Err(e) => eprintln!("[Parse error: {}]", e),
                    }
                }
                Err(e) => {
                    eprintln!("[Read error: {}]", e);
                    break;
                }
            }
        }
    });

    // Main input loop
    loop {
        print!("soma> ");
        io::stdout().flush()?;

        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            break; // EOF
        }
        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        let msg = if input == "/quit" {
            break;
        } else if input == "/caps" {
            CompositorMessage::ListCapabilities
        } else if let Some(cmd) = input.strip_prefix("/exec ") {
            CompositorMessage::DirectExec {
                id: uuid_v4(),
                command: cmd.to_string(),
            }
        } else {
            CompositorMessage::NaturalLanguageInput {
                text: input.to_string(),
            }
        };

        let json = serde_json::to_string(&msg)?;
        let mut w = writer.lock().await;
        w.write_all(format!("{}\n", json).as_bytes()).await?;
    }

    response_handle.abort();
    println!("Goodbye!");
    Ok(())
}

async fn handle_agent_message(
    msg: AgentMessage,
    writer: &std::sync::Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
) {
    match msg {
        AgentMessage::TaskPlanReady { id, plan } => {
            println!();
            println!("┌─── Task Plan ───────────────────────────┐");
            println!("│ Intent: {}", plan.intent);
            println!("│ Description: {}", plan.description);
            println!("│ Risk: {}", plan.risk_level);
            println!("│ Steps:");
            for (i, step) in plan.steps.iter().enumerate() {
                println!("│   {}. {}.{} — {}", i + 1, step.capability, step.action, step.description);
                println!("│      params: {}", step.params);
            }
            println!("└──────────────────────────────────────────┘");
            println!();

            // Ask for approval
            print!("[Approve? y/n] ");
            io::stdout().flush().unwrap();

            let mut response = String::new();
            io::stdin().read_line(&mut response).unwrap();
            let response = response.trim().to_lowercase();

            let msg = if response == "y" || response == "yes" {
                CompositorMessage::Approve { id }
            } else {
                CompositorMessage::Reject { id }
            };

            let json = serde_json::to_string(&msg).unwrap();
            let mut w = writer.lock().await;
            let _ = w.write_all(format!("{}\n", json).as_bytes()).await;
        }

        AgentMessage::StepResult {
            step_index, result, ..
        } => {
            if result.success {
                println!("  ✓ Step {} completed:", step_index + 1);
                // Pretty-print the data
                if let Ok(pretty) = serde_json::to_string_pretty(&result.data) {
                    for line in pretty.lines() {
                        println!("    {}", line);
                    }
                }
            } else {
                println!(
                    "  ✗ Step {} failed: {}",
                    step_index + 1,
                    result.error.unwrap_or_default()
                );
            }
        }

        AgentMessage::ExecutionComplete { results, .. } => {
            let ok_count = results.iter().filter(|r| r.success).count();
            println!();
            println!("[Execution complete: {}/{} steps succeeded]", ok_count, results.len());
            println!();
        }

        AgentMessage::Error { message, .. } => {
            println!();
            println!("[Error] {}", message);
            println!();
        }

        AgentMessage::Capabilities { capabilities } => {
            println!();
            println!("┌─── Available Capabilities ──────────────┐");
            for cap in &capabilities {
                println!("│ {} — {}", cap.name, cap.description);
                for action in &cap.actions {
                    println!("│   • {}: {}", action.name, action.description);
                }
            }
            println!("└──────────────────────────────────────────┘");
            println!();
        }

        AgentMessage::DirectOutput { result, .. } => {
            if !result.stdout.is_empty() {
                print!("{}", result.stdout);
            }
            if !result.stderr.is_empty() {
                eprint!("{}", result.stderr);
            }
        }

        AgentMessage::Pong => {
            println!("[Pong]");
        }
    }
}

fn uuid_v4() -> String {
    // Simple UUID v4 without external dependency
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:032x}", t)
}
