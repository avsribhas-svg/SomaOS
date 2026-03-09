use soma_common::{AgentMessage, CompositorMessage, AGENT_SOCKET_PATH};
use std::io::{self, BufRead, Write};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

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

    // Channel for stdin lines (single reader, avoids race condition)
    let (stdin_tx, mut stdin_rx) = mpsc::unbounded_channel::<String>();

    // Spawn a dedicated stdin reader thread
    std::thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(l) => {
                    if stdin_tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Main event loop
    let mut pending_approval: Option<String> = None;
    let mut agent_line = String::new();

    print!("soma> ");
    io::stdout().flush()?;

    loop {
        tokio::select! {
            // Read from agent socket
            result = reader.read_line(&mut agent_line) => {
                match result {
                    Ok(0) => {
                        println!("\n[Agent disconnected]");
                        break;
                    }
                    Ok(_) => {
                        let trimmed = agent_line.trim();
                        if !trimmed.is_empty() {
                            if let Ok(msg) = serde_json::from_str::<AgentMessage>(trimmed) {
                                pending_approval = handle_agent_message(msg);
                                if pending_approval.is_none() {
                                    print!("soma> ");
                                    io::stdout().flush()?;
                                }
                            }
                        }
                        agent_line.clear();
                    }
                    Err(e) => {
                        eprintln!("[Read error: {}]", e);
                        break;
                    }
                }
            }

            // Read from stdin
            Some(input) = stdin_rx.recv() => {
                let input = input.trim().to_string();
                if input.is_empty() {
                    if pending_approval.is_some() {
                        print!("[Approve? y/n] ");
                    } else {
                        print!("soma> ");
                    }
                    io::stdout().flush()?;
                    continue;
                }

                // Handle approval prompt
                if let Some(ref plan_id) = pending_approval {
                    let msg = if input == "y" || input == "yes" {
                        CompositorMessage::Approve { id: plan_id.clone() }
                    } else {
                        println!("[Plan rejected]");
                        CompositorMessage::Reject { id: plan_id.clone() }
                    };
                    pending_approval = None;

                    let json = serde_json::to_string(&msg)?;
                    let mut w = writer.lock().await;
                    w.write_all(format!("{}\n", json).as_bytes()).await?;
                    continue;
                }

                // Handle CLI commands
                let msg = if input == "/quit" {
                    break;
                } else if input == "/caps" {
                    CompositorMessage::ListCapabilities
                } else if let Some(cmd) = input.strip_prefix("/exec ") {
                    CompositorMessage::DirectExec {
                        id: simple_id(),
                        command: cmd.to_string(),
                    }
                } else {
                    // Natural language input
                    CompositorMessage::NaturalLanguageInput {
                        text: input.to_string(),
                    }
                };

                let json = serde_json::to_string(&msg)?;
                let mut w = writer.lock().await;
                w.write_all(format!("{}\n", json).as_bytes()).await?;
            }
        }
    }

    println!("Goodbye!");
    Ok(())
}

/// Handle an agent message, returns Some(id) if approval is needed
fn handle_agent_message(msg: AgentMessage) -> Option<String> {
    match msg {
        AgentMessage::TaskPlanReady { id, plan } => {
            println!();
            println!("┌─── Task Plan ───────────────────────────┐");
            println!("│ Intent: {}", plan.intent);
            if !plan.description.is_empty() {
                println!("│ Description: {}", plan.description);
            }
            println!("│ Risk: {}", plan.risk_level);
            if !plan.steps.is_empty() {
                println!("│ Steps:");
                for (i, step) in plan.steps.iter().enumerate() {
                    let desc = if step.description.is_empty() {
                        format!("{}.{}", step.capability, step.action)
                    } else {
                        step.description.clone()
                    };
                    println!("│   {}. {} — {}", i + 1, format!("{}.{}", step.capability, step.action), desc);
                    if !step.params.is_null() && step.params != serde_json::json!({}) {
                        println!("│      params: {}", step.params);
                    }
                }
            }
            println!("└──────────────────────────────────────────┘");
            println!();
            print!("[Approve? y/n] ");
            io::stdout().flush().ok();
            Some(id)
        }

        AgentMessage::StepResult {
            step_index, result, ..
        } => {
            if result.success {
                println!("  ✓ Step {}:", step_index + 1);
                if let Ok(pretty) = serde_json::to_string_pretty(&result.data) {
                    // Limit output to avoid flooding the terminal
                    let lines: Vec<&str> = pretty.lines().collect();
                    let max_lines = 30;
                    for line in lines.iter().take(max_lines) {
                        println!("    {}", line);
                    }
                    if lines.len() > max_lines {
                        println!("    ... ({} more lines)", lines.len() - max_lines);
                    }
                }
            } else {
                println!(
                    "  ✗ Step {} failed: {}",
                    step_index + 1,
                    result.error.unwrap_or_default()
                );
            }
            None
        }

        AgentMessage::ExecutionComplete { results, .. } => {
            let ok_count = results.iter().filter(|r| r.success).count();
            println!();
            println!("[Done: {}/{} steps succeeded]", ok_count, results.len());
            println!();
            None
        }

        AgentMessage::Error { message, .. } => {
            println!();
            println!("[Error] {}", message);
            println!();
            None
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
            None
        }

        AgentMessage::DirectOutput { result, .. } => {
            if !result.stdout.is_empty() {
                print!("{}", result.stdout);
            }
            if !result.stderr.is_empty() {
                eprint!("{}", result.stderr);
            }
            None
        }

        AgentMessage::Pong => {
            println!("[Pong]");
            None
        }
    }
}

fn simple_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:032x}", t)
}
