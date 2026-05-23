use soma_common::{AgentMessage, CompositorMessage, AGENT_SOCKET_PATH};
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, Semaphore};

// ─────────────────────────────────────────────────────────────────────────────
//  Entry point — dispatch to interactive or test mode
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    if let Some(pos) = args.iter().position(|a| a == "--test") {
        let path = args.get(pos + 1).expect("Usage: soma-cli --test <scenarios.json> [--concurrency N] [--filter <prefix>]");
        let concurrency = args.iter().position(|a| a == "--concurrency")
            .and_then(|p| args.get(p + 1))
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(1); // default: sequential — Ollama handles one inference at a time
        // --filter <prefix> — only run scenarios whose id starts with <prefix>
        let filter = args.iter().position(|a| a == "--filter")
            .and_then(|p| args.get(p + 1))
            .cloned();
        run_test_suite(path, concurrency, filter.as_deref()).await?;
    } else {
        run_interactive().await?;
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Test mode
// ─────────────────────────────────────────────────────────────────────────────

#[derive(serde::Deserialize, Debug, Clone)]
struct Scenario {
    id: String,
    prompt: String,
    /// If set, the plan must contain at least one step from this capability.
    expect_capability: Option<String>,
    /// If set, the plan must contain at least one step with this action name.
    expect_action: Option<String>,
    /// Auto-approve the HITL plan (set false to just parse and reject).
    #[serde(default = "default_true")]
    auto_approve: bool,
    /// Per-scenario timeout in seconds (default 30).
    #[serde(default = "default_timeout")]
    timeout_secs: u64,
}

fn default_true() -> bool { true }
fn default_timeout() -> u64 { 30 }

#[derive(Debug)]
struct ScenarioResult {
    id: String,
    passed: bool,
    duration_ms: u128,
    message: String,
}

async fn run_test_suite(path: &str, concurrency: usize, filter: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {}: {}", path, e))?;
    let all_scenarios: Vec<Scenario> = serde_json::from_str(&raw)
        .map_err(|e| format!("Invalid JSON in {}: {}", path, e))?;

    // Apply --filter prefix if given
    let scenarios: Vec<Scenario> = if let Some(prefix) = filter {
        let filtered: Vec<_> = all_scenarios.into_iter()
            .filter(|s| s.id.starts_with(prefix))
            .collect();
        if filtered.is_empty() {
            eprintln!("[soma-test] No scenarios match filter '{}'", prefix);
            std::process::exit(1);
        }
        filtered
    } else {
        all_scenarios
    };

    println!("╔══════════════════════════════════════════╗");
    println!("║       SomaOS Agent Test Runner           ║");
    println!("╚══════════════════════════════════════════╝");
    println!();
    println!("[soma-test] {} scenarios, concurrency={}", scenarios.len(), concurrency);
    if let Some(f) = filter {
        println!("[soma-test] Filter: '{}'", f);
    }
    if concurrency == 1 {
        println!("[soma-test] Running sequentially (Ollama handles one inference at a time).");
        println!("[soma-test] Use --concurrency N for faster providers (Anthropic/OpenAI).");
    }
    println!();

    let sem = Arc::new(Semaphore::new(concurrency));

    // Spawn all scenarios — semaphore limits how many run at once
    let mut handles = Vec::new();
    for scenario in scenarios {
        let sem = sem.clone();
        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            run_scenario(scenario).await
        });
        handles.push(handle);
    }

    // Collect results
    let mut results: Vec<ScenarioResult> = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(r) => results.push(r),
            Err(e) => results.push(ScenarioResult {
                id: "unknown".to_string(),
                passed: false,
                duration_ms: 0,
                message: format!("Task panicked: {}", e),
            }),
        }
    }

    // Sort by id for stable output, then print
    results.sort_by(|a, b| a.id.cmp(&b.id));

    let passed = results.iter().filter(|r| r.passed).count();
    let total = results.len();

    for r in &results {
        let icon = if r.passed { "✓" } else { "✗" };
        println!("  {} {:35} ({:>5}ms) {}", icon, r.id, r.duration_ms, r.message);
    }

    let failed_msgs: Vec<String> = results.iter()
        .filter(|r| !r.passed)
        .map(|r| format!("  ✗ {}: {}", r.id, r.message))
        .collect();

    println!();
    if !failed_msgs.is_empty() {
        println!("─── FAILURES ────────────────────────────────────────────────────");
        for msg in &failed_msgs { println!("{}", msg); }
        println!();
    }

    if passed == total {
        println!("[soma-test] {}/{} passed ✓", passed, total);
    } else {
        println!("[soma-test] {}/{} passed — {} FAILED", passed, total, total - passed);
        std::process::exit(1);
    }

    Ok(())
}

async fn run_scenario(s: Scenario) -> ScenarioResult {
    let start = std::time::Instant::now();
    let id = s.id.clone();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(s.timeout_secs),
        execute_scenario(s),
    )
    .await;

    let duration_ms = start.elapsed().as_millis();

    match result {
        Ok(Ok(msg)) => ScenarioResult { id, passed: true,  duration_ms, message: msg },
        Ok(Err(msg)) => ScenarioResult { id, passed: false, duration_ms, message: msg },
        Err(_) => ScenarioResult {
            id,
            passed: false,
            duration_ms,
            message: format!("TIMEOUT after {}s", duration_ms / 1000),
        },
    }
}

/// Connects to the agent, sends the prompt, waits for plan + execution, returns Ok/Err.
async fn execute_scenario(s: Scenario) -> Result<String, String> {
    let stream = UnixStream::connect(AGENT_SOCKET_PATH)
        .await
        .map_err(|e| format!("Cannot connect to agent socket: {}", e))?;

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Send natural language prompt
    let msg = serde_json::to_string(&CompositorMessage::NaturalLanguageInput {
        text: s.prompt.clone(),
        session_id: None,
    })
    .unwrap();
    writer.write_all(format!("{}\n", msg).as_bytes()).await
        .map_err(|e| format!("Write error: {}", e))?;

    let mut plan_id: Option<String> = None;
    let mut plan_steps: Vec<(String, String)> = Vec::new(); // (capability, action)
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await
            .map_err(|e| format!("Read error: {}", e))?;
        if n == 0 {
            return Err("Agent closed connection".to_string());
        }

        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }

        let agent_msg: AgentMessage = serde_json::from_str(trimmed)
            .map_err(|e| format!("Parse error: {} on: {}", e, &trimmed[..trimmed.len().min(80)]))?;

        match agent_msg {
            AgentMessage::TaskPlanReady { id, plan, .. } => {
                // Validate expectations before approving
                plan_steps = plan.steps.iter()
                    .map(|step| (step.capability.clone(), step.action.clone()))
                    .collect();

                if let Some(ref exp_cap) = s.expect_capability {
                    let found = plan_steps.iter().any(|(cap, _)| cap == exp_cap);
                    if !found {
                        // Reject plan and fail
                        let reject = serde_json::to_string(&CompositorMessage::Reject { id: id.clone(), session_id: None }).unwrap();
                        let _ = writer.write_all(format!("{}\n", reject).as_bytes()).await;
                        return Err(format!(
                            "Expected capability '{}', plan used: {}",
                            exp_cap,
                            plan_steps.iter().map(|(c, a)| format!("{}.{}", c, a)).collect::<Vec<_>>().join(", ")
                        ));
                    }
                }

                if let Some(ref exp_act) = s.expect_action {
                    let found = plan_steps.iter().any(|(_, act)| act == exp_act);
                    if !found {
                        let reject = serde_json::to_string(&CompositorMessage::Reject { id: id.clone(), session_id: None }).unwrap();
                        let _ = writer.write_all(format!("{}\n", reject).as_bytes()).await;
                        return Err(format!(
                            "Expected action '{}', plan used: {}",
                            exp_act,
                            plan_steps.iter().map(|(c, a)| format!("{}.{}", c, a)).collect::<Vec<_>>().join(", ")
                        ));
                    }
                }

                plan_id = Some(id.clone());

                let response = if s.auto_approve {
                    CompositorMessage::Approve { id, session_id: None }
                } else {
                    CompositorMessage::Reject { id, session_id: None }
                };
                let json = serde_json::to_string(&response).unwrap();
                writer.write_all(format!("{}\n", json).as_bytes()).await
                    .map_err(|e| format!("Write error: {}", e))?;

                if !s.auto_approve {
                    let steps_str = plan_steps.iter()
                        .map(|(c, a)| format!("{}.{}", c, a))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Ok(format!("plan parsed ok (not approved): {}", steps_str));
                }
            }

            AgentMessage::ExecutionComplete { results, .. } => {
                if plan_id.is_none() {
                    return Err("ExecutionComplete before TaskPlanReady".to_string());
                }
                let ok = results.iter().filter(|r| r.success).count();
                let total = results.len();
                let steps_str = plan_steps.iter()
                    .map(|(c, a)| format!("{}.{}", c, a))
                    .collect::<Vec<_>>()
                    .join(", ");
                if ok == total {
                    return Ok(format!("{} — {}/{} steps ok", steps_str, ok, total));
                } else {
                    let first_err = results.iter()
                        .find(|r| !r.success)
                        .and_then(|r| r.error.as_ref())
                        .map(|e| format!("{:?}", e))
                        .unwrap_or_else(|| "unknown error".to_string());
                    return Err(format!(
                        "{}/{} steps ok — {}: {}",
                        ok, total, steps_str, first_err
                    ));
                }
            }

            AgentMessage::Error { message, .. } => {
                return Err(format!("Agent error: {}", message));
            }

            // Ignore other messages
            _ => {}
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Interactive mode (original soma-cli behaviour)
// ─────────────────────────────────────────────────────────────────────────────

async fn run_interactive() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔══════════════════════════════════════╗");
    println!("║       SomaOS Agent CLI v0.3.0        ║");
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
    println!("  --test <file> — run scenario file (pass as CLI arg instead)");
    println!("  /quit         — exit");
    println!();

    let (stdin_tx, mut stdin_rx) = mpsc::unbounded_channel::<String>();

    std::thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(l) => { if stdin_tx.send(l).is_err() { break; } }
                Err(_) => break,
            }
        }
    });

    let mut pending_approval: Option<String> = None;
    let mut agent_line = String::new();

    print!("soma> ");
    io::stdout().flush()?;

    loop {
        tokio::select! {
            result = reader.read_line(&mut agent_line) => {
                match result {
                    Ok(0) => { println!("\n[Agent disconnected]"); break; }
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
                    Err(e) => { eprintln!("[Read error: {}]", e); break; }
                }
            }

            Some(input) = stdin_rx.recv() => {
                let input = input.trim().to_string();
                if input.is_empty() {
                    print!("{}", if pending_approval.is_some() { "[Approve? y/n] " } else { "soma> " });
                    io::stdout().flush()?;
                    continue;
                }

                if let Some(ref plan_id) = pending_approval {
                    let msg = if input == "y" || input == "yes" {
                        CompositorMessage::Approve { id: plan_id.clone(), session_id: None }
                    } else {
                        println!("[Plan rejected]");
                        CompositorMessage::Reject { id: plan_id.clone(), session_id: None }
                    };
                    pending_approval = None;
                    let json = serde_json::to_string(&msg)?;
                    let mut w = writer.lock().await;
                    w.write_all(format!("{}\n", json).as_bytes()).await?;
                    continue;
                }

                let msg = if input == "/quit" {
                    break;
                } else if input == "/caps" {
                    CompositorMessage::ListCapabilities
                } else if let Some(cmd) = input.strip_prefix("/exec ") {
                    CompositorMessage::DirectExec { id: simple_id(), command: cmd.to_string() }
                } else {
                    CompositorMessage::NaturalLanguageInput { text: input, session_id: None }
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

fn handle_agent_message(msg: AgentMessage) -> Option<String> {
    match msg {
        AgentMessage::TaskPlanReady { id, plan, .. } => {
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
                    println!("│   {}. {}.{}", i + 1, step.capability, step.action);
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
        AgentMessage::StepResult { step_index, result, .. } => {
            if result.success {
                println!("  ✓ Step {}:", step_index + 1);
                let pretty = serde_json::to_string_pretty(&result.data).unwrap_or_default();
                let lines: Vec<&str> = pretty.lines().collect();
                for line in lines.iter().take(30) { println!("    {}", line); }
                if lines.len() > 30 { println!("    ... ({} more lines)", lines.len() - 30); }
            } else {
                println!("  ✗ Step {} failed: {:?}", step_index + 1, result.error);
            }
            None
        }
        AgentMessage::ExecutionComplete { results, .. } => {
            let ok = results.iter().filter(|r| r.success).count();
            println!();
            println!("[Done: {}/{} steps succeeded]", ok, results.len());
            println!();
            None
        }
        AgentMessage::Error { message, .. } => {
            println!("\n[Error] {}\n", message);
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
            if !result.stdout.is_empty() { print!("{}", result.stdout); }
            if !result.stderr.is_empty() { eprint!("{}", result.stderr); }
            None
        }
        AgentMessage::BrowserUpdate { url, title, .. } => {
            println!("[Browser] {} — {}", title, url);
            None
        }
        AgentMessage::Pong => { println!("[Pong]"); None }
        _ => None,
    }
}

fn simple_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    format!("{:032x}", t)
}
