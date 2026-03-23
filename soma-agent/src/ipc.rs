use log::{error, info, warn};
use serde_json::json;
use soma_common::{AgentMessage, AppState, CompositorMessage, SessionScope, SessionStatus, TaskPlan, AGENT_SOCKET_PATH};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;

use crate::capabilities::CapabilityRegistry;
use crate::config::SomaConfig;
use crate::executor::Executor;
use crate::intent::IntentParser;
use crate::observer::DesktopObserver;

/// Pending plans awaiting user approval
type PendingPlans = Arc<Mutex<HashMap<String, TaskPlan>>>;

/// Shared cache of per-window app state pushed by the compositor
pub type AppStateCache = Arc<std::sync::Mutex<HashMap<u32, AppState>>>;

/// Conversation context — stores recent exchanges for follow-up resolution
const MAX_CONTEXT_ENTRIES: usize = 5;
/// Reject IPC messages larger than this to prevent memory exhaustion
const MAX_MESSAGE_BYTES: usize = 1_048_576; // 1 MB

// ── Session tracking ──────────────────────────────────────────────────────────

struct SessionStep {
    capability: String,
    action: String,
    success: bool,
}

struct Session {
    id: String,
    task: String,
    started_at: std::time::SystemTime,
    steps: Vec<SessionStep>,
    affected_resources: Vec<String>,
    scope: Option<SessionScope>,
}

impl Session {
    fn new(task: &str, scope: Option<SessionScope>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            task: task.to_string(),
            started_at: std::time::SystemTime::now(),
            steps: Vec::new(),
            affected_resources: Vec::new(),
            scope,
        }
    }

    fn to_status(&self) -> SessionStatus {
        let started_at_unix = self.started_at
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        SessionStatus {
            session_id: self.id.clone(),
            task: self.task.clone(),
            started_at_unix,
            step_count: self.steps.len(),
            scope: self.scope.clone(),
            affected_resources: self.affected_resources.clone(),
        }
    }

    fn persist(&self) {
        let soma_dir = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".soma/sessions");
        let _ = std::fs::create_dir_all(&soma_dir);
        let elapsed = self.started_at.elapsed().unwrap_or_default().as_secs();
        let steps_json: Vec<serde_json::Value> = self.steps.iter().map(|s| json!({
            "capability": s.capability,
            "action": s.action,
            "success": s.success,
        })).collect();
        let doc = json!({
            "id": self.id,
            "task": self.task,
            "duration_secs": elapsed,
            "steps": steps_json,
            "affected_resources": self.affected_resources,
        });
        let path = soma_dir.join(format!("{}.json", self.id));
        if let Ok(text) = serde_json::to_string_pretty(&doc) {
            let _ = std::fs::write(path, text);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────

pub async fn run_ipc_server() -> Result<(), Box<dyn std::error::Error>> {
    // Clean up old socket
    let _ = std::fs::remove_file(AGENT_SOCKET_PATH);

    let listener = UnixListener::bind(AGENT_SOCKET_PATH)?;

    // Restrict socket to owner only — prevents other users on the system from
    // connecting and issuing arbitrary capability commands.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            AGENT_SOCKET_PATH,
            std::fs::Permissions::from_mode(0o600),
        )?;
    }

    info!("Agent daemon listening on {}", AGENT_SOCKET_PATH);

    // App-state cache shared across connections and with SheetsCapability
    let app_state_cache: AppStateCache = Arc::new(std::sync::Mutex::new(HashMap::new()));

    let registry = Arc::new(CapabilityRegistry::new_with_cache(app_state_cache.clone()));
    let config = SomaConfig::load();
    info!("Loaded config: provider={} model={}", config.model.provider, config.model.model);

    let parser = Arc::new(Mutex::new(IntentParser::new(&config)));
    let executor = Arc::new(Executor::new());
    let pending: PendingPlans = Arc::new(Mutex::new(HashMap::new()));
    let observer = Arc::new(Mutex::new(DesktopObserver::new()));

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
        let observer = Arc::clone(&observer);
        let app_state_cache = Arc::clone(&app_state_cache);

        tokio::spawn(async move {
            let (reader, writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let writer = Arc::new(Mutex::new(writer));
            let mut line = String::new();

            // Per-client conversation context (pairs of user/agent strings)
            let mut context: Vec<(String, String)> = Vec::new();
            // Per-client active session (started when agent mode begins)
            let mut active_session: Option<Session> = None;

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
                        if trimmed.len() > MAX_MESSAGE_BYTES {
                            warn!("Dropping oversized IPC message ({} bytes)", trimmed.len());
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
                                    &observer,
                                    &writer,
                                    &mut context,
                                    &app_state_cache,
                                    &mut active_session,
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
    observer: &Arc<Mutex<DesktopObserver>>,
    writer: &Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    context: &mut Vec<(String, String)>,
    app_state_cache: &AppStateCache,
    active_session: &mut Option<Session>,
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
                    // Scope enforcement — check capability and path whitelists
                    if let Some(ref session) = active_session {
                        if let Some(ref scope) = session.scope {
                            if let Some(ref whitelist) = scope.capability_whitelist {
                                if !whitelist.contains(&step.capability) {
                                    let scope_err = soma_common::CapabilityResult {
                                        success: false,
                                        data: serde_json::Value::Null,
                                        error: Some(soma_common::CapabilityError::new(
                                            soma_common::ErrorReason::PermissionDenied,
                                            format!("Capability '{}' not in session scope whitelist", step.capability),
                                        )),
                                    };
                                    let step_msg = AgentMessage::StepResult {
                                        id: id.clone(),
                                        step_index: i,
                                        result: scope_err.clone(),
                                    };
                                    send_message(&step_msg, writer).await;
                                    results.push(scope_err);
                                    continue;
                                }
                            }
                            if let Some(ref path_whitelist) = scope.path_whitelist {
                                if ["filesystem", "process", "script"].contains(&step.capability.as_str()) {
                                    let path = step.params.get("path")
                                        .or_else(|| step.params.get("from"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    if !path.is_empty() && !path_whitelist.iter().any(|prefix| path.starts_with(prefix.as_str())) {
                                        let scope_err = soma_common::CapabilityResult {
                                            success: false,
                                            data: serde_json::Value::Null,
                                            error: Some(soma_common::CapabilityError::new(
                                                soma_common::ErrorReason::PermissionDenied,
                                                format!("Path '{}' not in session path whitelist", path),
                                            )),
                                        };
                                        let step_msg = AgentMessage::StepResult {
                                            id: id.clone(),
                                            step_index: i,
                                            result: scope_err.clone(),
                                        };
                                        send_message(&step_msg, writer).await;
                                        results.push(scope_err);
                                        continue;
                                    }
                                }
                            }
                        }
                    }

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

                    // Forward desktop_agent / sheets / docs / media results as IPC messages
                    if (step.capability == "desktop_agent" || step.capability == "sheets"
                        || step.capability == "docs" || step.capability == "media") && result.success {
                        // Session lifecycle via ipc_message type
                        let msg_type = result.data["ipc_message"].as_str().unwrap_or("");
                        if msg_type == "AgentModeStarted" {
                            let task = result.data["task"].as_str().unwrap_or("").to_string();
                            // Parse optional scope from result data
                            let scope: Option<SessionScope> = result.data.get("scope")
                                .and_then(|s| serde_json::from_value(s.clone()).ok());
                            *active_session = Some(Session::new(&task, scope));
                            info!("Session started: {}", task);
                        } else if msg_type == "AgentModeEnded" {
                            if let Some(session) = active_session.take() {
                                let sid = session.id.clone();
                                session.persist();
                                info!("Session persisted: {}", sid);
                            }
                        }
                        forward_ipc_command(&result, observer, writer, active_session).await;
                    }

                    // Session step recording
                    if let Some(ref mut session) = active_session {
                        session.steps.push(SessionStep {
                            capability: step.capability.clone(),
                            action: step.action.clone(),
                            success: result.success,
                        });
                        // Record filesystem paths touched
                        if step.capability == "filesystem" {
                            if let Some(path) = result.data.get("path").and_then(|p| p.as_str()) {
                                session.affected_resources.push(path.to_string());
                            }
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

        // ── New v1.0 desktop messages ─────────────────────────────────────

        CompositorMessage::DesktopEvent { event_type, window_title, timestamp } => {
            observer.lock().await.observe(event_type.clone(), window_title.clone(), timestamp);
            info!("Desktop event: {} — {}", event_type, window_title);
            return; // No response needed
        }

        CompositorMessage::AnnotateWorkflow { name } => {
            observer.lock().await.annotate_workflow(&name);
            info!("Annotated workflow: {}", name);
            return;
        }

        CompositorMessage::PrivateModeChanged { active } => {
            observer.lock().await.set_active(!active);
            info!("Private mode: {}", active);
            return;
        }

        CompositorMessage::DynamicAppAction { app_id, action_id, window_id } => {
            info!("DynamicApp action: app={} action={} win={}", app_id, action_id, window_id);
            // Future: route to app-specific handler
            return;
        }

        CompositorMessage::AppStateChanged { window_id, state } => {
            info!("AppStateChanged: window={}", window_id);
            app_state_cache.lock().unwrap().insert(window_id, state);
            return; // No response needed
        }

        CompositorMessage::ReloadCapabilities => {
            // The registry is Arc<CapabilityRegistry> (immutable shared ref in this handler).
            // Full mutable hot-reload requires a RwLock refactor deferred to v1.3.
            // For now, return the current count so the compositor registry tab can refresh.
            let count = registry.list().len();
            info!("ReloadCapabilities received — {} capabilities registered (reload takes effect on restart)", count);
            AgentMessage::CapabilitiesReloaded { count }
        }

        CompositorMessage::QueryCapabilities => {
            let capabilities = registry.list();
            AgentMessage::Capabilities { capabilities }
        }

        CompositorMessage::Ping => AgentMessage::Pong,

        CompositorMessage::GetSessionStatus => {
            let status = active_session.as_ref().map(|s| s.to_status());
            AgentMessage::SessionStatusResponse { status }
        }
    };

    send_message(&response, writer).await;
}

/// Forward a capability result that carries an `ipc_message` key as a real IPC message
/// to the compositor. Handles desktop_agent, sheets, docs, and media command results.
async fn forward_ipc_command(
    result: &soma_common::CapabilityResult,
    observer: &Arc<Mutex<DesktopObserver>>,
    writer: &Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    active_session: &Option<Session>,
) {
    let msg_type = result.data["ipc_message"].as_str().unwrap_or("");
    let agent_msg: Option<AgentMessage> = match msg_type {
        "AgentModeStarted" => {
            let task = result.data["task"].as_str().unwrap_or("").to_string();
            let scope: Option<SessionScope> = result.data.get("scope")
                .and_then(|s| serde_json::from_value(s.clone()).ok());
            Some(AgentMessage::AgentModeStarted { task, scope })
        }
        "AgentModeEnded" => Some(AgentMessage::AgentModeEnded),
        "GetSessionStatus" => {
            let status = active_session.as_ref().map(|s| s.to_status());
            Some(AgentMessage::SessionStatusResponse { status })
        }
        "SpawnApp" => {
            let title = result.data["title"].as_str().unwrap_or("App").to_string();
            let app_id = result.data["app_id"].as_str().unwrap_or("app").to_string();
            let description = result.data["description"].as_str().unwrap_or("").to_string();
            let widgets_json = result.data["widgets_json"].as_str().unwrap_or("[]").to_string();
            Some(AgentMessage::SpawnApp { title, app_id, description, widgets_json })
        }
        "DesktopAction" => {
            let action = result.data["action"].as_str().unwrap_or("").to_string();
            Some(AgentMessage::DesktopAction { action })
        }
        "GetWorkflowHistory" => {
            let history = observer.lock().await.get_history();
            info!("Workflow history: {}", &history[..history.len().min(200)]);
            None
        }
        "ActivityUpdate" => {
            let text = result.data["text"].as_str().unwrap_or("").to_string();
            Some(AgentMessage::ActivityUpdate { text })
        }
        "AppAction" => {
            let window_id = result.data["window_id"].as_u64().unwrap_or(0) as u32;
            let action = result.data["action"].as_str().unwrap_or("").to_string();
            let params = result.data["params"].clone();
            Some(AgentMessage::AppAction { window_id, action, params })
        }
        _ => None,
    };

    if let Some(msg) = agent_msg {
        send_message(&msg, writer).await;
    }
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

