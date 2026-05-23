use log::{error, info, warn};
use soma_common::{
    AgentMessage, AppState, CompositorMessage, SessionScope, TaskPlan,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use crate::capabilities::CapabilityRegistry;
use crate::config::SomaConfig;
use crate::executor::Executor;
use crate::intent::IntentParser;
use crate::observer::DesktopObserver;
use crate::session_manager::{SessionManager, SharedSessionManager};
use crate::transport::IpcWriter;

use soma_substrate::{
    StateReflector, TierGate, ModeEngine, ConsequenceObserver, ScaffoldManager,
    CoherenceVerifier, BehavioralHistoryManager
};

/// Pending plans awaiting user approval — keyed by plan ID
type PendingPlans = Arc<Mutex<HashMap<String, PendingPlan>>>;

/// Shared cache of per-window app state pushed by the compositor
pub type AppStateCache = Arc<std::sync::Mutex<HashMap<u32, AppState>>>;

pub(crate) struct PendingPlan {
    plan: TaskPlan,
    /// Session ID of the session that initiated this plan (if any)
    session_id: Option<String>,
}

/// Conversation context — stores recent exchanges for follow-up resolution
const MAX_CONTEXT_ENTRIES: usize = 5;
/// Reject IPC messages larger than this to prevent memory exhaustion
const MAX_MESSAGE_BYTES: usize = 1_048_576; // 1 MB

// ── Shared state passed to every connection handler ───────────────────────────

#[derive(Clone)]
pub struct SharedIpcState {
    pub registry: Arc<CapabilityRegistry>,
    pub parser: Arc<Mutex<IntentParser>>,
    pub executor: Arc<Executor>,
    pub pending: PendingPlans,
    pub observer: Arc<Mutex<DesktopObserver>>,
    pub app_state_cache: AppStateCache,
    pub session_manager: SharedSessionManager,
    // V2 Substrate extensions
    pub active_writers: Arc<Mutex<Vec<IpcWriter>>>,
    pub substrate_reflector: Arc<Mutex<StateReflector>>,
    pub substrate_history: Arc<Mutex<BehavioralHistoryManager>>,
    pub substrate_tier_gate: Arc<Mutex<TierGate>>,
    pub substrate_mode_engine: Arc<Mutex<ModeEngine>>,
    pub substrate_consequence_observer: Arc<Mutex<ConsequenceObserver>>,
    pub substrate_scaffold_manager: Arc<Mutex<ScaffoldManager>>,
}

// Helper to broadcast IPC message to all connected compositors
pub async fn broadcast_message(msg: &AgentMessage, state: &SharedIpcState) {
    let json_str = match serde_json::to_string(msg) {
        Ok(s) => format!("{}\n", s),
        Err(_) => return,
    };
    let mut writers = state.active_writers.lock().await;
    let mut to_remove = Vec::new();
    for (i, w) in writers.iter_mut().enumerate() {
        let mut lock = w.lock().await;
        if let Err(e) = lock.write_all(json_str.as_bytes()).await {
            log::error!("Failed to broadcast to client: {}", e);
            to_remove.push(i);
        }
    }
    for &idx in to_remove.iter().rev() {
        writers.remove(idx);
    }
}

// ─────────────────────────────────────────────────────────────────────────────

pub async fn run_ipc_server() -> Result<(), Box<dyn std::error::Error>> {
    let app_state_cache: AppStateCache = Arc::new(std::sync::Mutex::new(HashMap::new()));
    let registry = Arc::new(CapabilityRegistry::new_with_cache(app_state_cache.clone()));
    let config = SomaConfig::load();
    info!("Loaded config: provider={} model={}", config.model.provider, config.model.model);

    let shared = SharedIpcState {
        registry,
        parser: Arc::new(Mutex::new(IntentParser::new(&config))),
        executor: Arc::new(Executor::new()),
        pending: Arc::new(Mutex::new(HashMap::new())),
        observer: Arc::new(Mutex::new(DesktopObserver::new())),
        app_state_cache,
        session_manager: Arc::new(std::sync::Mutex::new(SessionManager::new())),
        active_writers: Arc::new(Mutex::new(Vec::new())),
        substrate_reflector: Arc::new(Mutex::new(StateReflector::new())),
        substrate_history: Arc::new(Mutex::new(BehavioralHistoryManager::new())),
        substrate_tier_gate: Arc::new(Mutex::new(TierGate::new())),
        substrate_mode_engine: Arc::new(Mutex::new(ModeEngine::new())),
        substrate_consequence_observer: Arc::new(Mutex::new(ConsequenceObserver::new())),
        substrate_scaffold_manager: Arc::new(Mutex::new(ScaffoldManager::new())),
    };

    // V2 Background tick loop
    let tick_shared = shared.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            
            let current_state = {
                let mut reflector = tick_shared.substrate_reflector.lock().await;
                reflector.capture_snapshot()
            };
            
            // 1. Evaluate Mode
            let new_mode = {
                let mut mode_engine = tick_shared.substrate_mode_engine.lock().await;
                let old_mode = mode_engine.current_mode;
                let mode = mode_engine.evaluate(&current_state);
                if mode != old_mode {
                    log::info!("SystemMode changed: {:?} -> {:?}", old_mode, mode);
                    broadcast_message(&AgentMessage::SystemModeChanged { mode }, &tick_shared).await;
                }
                mode
            };

            // 2. Tick Consequence Observer
            let completed_consequences = {
                let mut observer = tick_shared.substrate_consequence_observer.lock().await;
                let reflector = tick_shared.substrate_reflector.lock().await;
                observer.tick(&current_state, &reflector)
            };

            if !completed_consequences.is_empty() {
                let mut history_mgr = tick_shared.substrate_history.lock().await;
                let tier = tick_shared.substrate_tier_gate.lock().await.current_tier;
                for record in completed_consequences {
                    log::info!("Consequence record finalized: {}.{}", record.action_capability, record.action_name);
                    
                    let timestamp_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
                    history_mgr.record_event(soma_common::ActionEvent {
                        timestamp_ms,
                        action_capability: record.action_capability.clone(),
                        action_name: record.action_name.clone(),
                        tier_at_time: tier,
                        mode_at_time: new_mode,
                        consequence: record,
                        was_characteristic: true,
                    });
                }
            }

            // 3. Evaluate Action Tiers
            let mut tier_gate = tick_shared.substrate_tier_gate.lock().await;
            let mut history_mgr = tick_shared.substrate_history.lock().await;
            if let Some(transition) = tier_gate.evaluate_transitions(&history_mgr.history, new_mode) {
                log::info!("ActionTier transition: {:?}", transition);
                history_mgr.record_transition(transition.from, transition.to, transition.reason.clone());
                broadcast_message(&AgentMessage::TierTransitioned { transition }, &tick_shared).await;
            }

            // 4. Evaluate Scaffolding
            let mut scaffolds = tick_shared.substrate_scaffold_manager.lock().await;
            let old_scaffolds = scaffolds.scaffolds.clone();
            scaffolds.evaluate_scaffolds(&history_mgr.history, tier_gate.current_tier, new_mode);
            for (old, new) in old_scaffolds.iter().zip(scaffolds.scaffolds.iter()) {
                if old.state != new.state || (old.activation_level - new.activation_level).abs() > 0.05 {
                    log::info!("Scaffold status updated: {:?}", new);
                    broadcast_message(&AgentMessage::ScaffoldChanged {
                        scaffold_type: new.scaffold_type,
                        state: new.state,
                        activation_level: new.activation_level,
                    }, &tick_shared).await;
                }
            }

            // 5. Coherence Verification
            let report = soma_substrate::CoherenceVerifier::verify(
                new_mode,
                tier_gate.current_tier,
                &scaffolds.scaffolds,
                &history_mgr.history,
                &Vec::new(),
            );
            if report.overall_coherence < 0.8 {
                log::warn!("Architectural incoherence detected! Coherence Score: {:.2}", report.overall_coherence);
                for contradiction in &report.contradictions {
                    log::warn!("Contradiction: {} <-> {}: {}", contradiction.property_a, contradiction.property_b, contradiction.description);
                    
                    if contradiction.property_a == "SystemMode" && contradiction.property_b == "ActionTier" {
                        let old_t = tier_gate.current_tier;
                        tier_gate.current_tier = soma_common::ActionTier::Touch;
                        history_mgr.record_transition(old_t, soma_common::ActionTier::Touch, "Coherence self-correction".to_string());
                        broadcast_message(&AgentMessage::TierTransitioned {
                            transition: soma_common::TierTransition {
                                from: old_t,
                                to: soma_common::ActionTier::Touch,
                                reason: "Coherence self-correction forced".to_string(),
                                timestamp_ms: report.timestamp_ms,
                            }
                        }, &tick_shared).await;
                    }
                }
            }

            // 6. Broadcast periodic developmental status
            broadcast_message(&AgentMessage::BehavioralReport {
                maturity_score: history_mgr.maturity_score(),
                consistency_trend: history_mgr.consistency_trend(),
            }, &tick_shared).await;
        }
    });

    info!("Registered capabilities:");
    for cap in shared.registry.list() {
        info!(
            "  • {} — {} ({} actions)",
            cap.name,
            cap.description,
            cap.actions.len()
        );
    }

    let network = config.network.clone();

    if let Some(addr) = network.tcp_listen_addr.clone() {
        // Run Unix + TCP listeners concurrently; both run forever so join! is correct.
        let unix_shared = shared.clone();
        let tcp_shared  = shared.clone();
        let (unix_res, tcp_res) = tokio::join!(
            crate::transport::unix::run_unix_listener(unix_shared),
            crate::transport::tcp::run_tcp_listener(addr, network, tcp_shared),
        );
        unix_res?;
        tcp_res.map_err(|e| format!("{}", e))?;
    } else {
        crate::transport::unix::run_unix_listener(shared).await?;
    }

    Ok(())
}

pub(crate) async fn handle_connection(
    raw_reader: impl tokio::io::AsyncRead + Send + Unpin + 'static,
    raw_writer: Box<dyn tokio::io::AsyncWrite + Send + Unpin>,
    shared: SharedIpcState,
) {
    let mut reader = BufReader::new(raw_reader);
    let writer: IpcWriter = Arc::new(Mutex::new(raw_writer));
    let mut line = String::new();

    // Register active writer
    shared.active_writers.lock().await.push(writer.clone());

    // Per-connection conversation context (pairs of user/agent strings)
    let mut context: Vec<(String, String)> = Vec::new();
    // Per-connection current session ID (set when agent mode starts on this connection)
    let mut conn_session_id: Option<String> = None;

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
                            &shared,
                            &writer,
                            &mut context,
                            &mut conn_session_id,
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

    // Unregister active writer
    {
        let mut writers = shared.active_writers.lock().await;
        if let Some(pos) = writers.iter().position(|w| Arc::ptr_eq(w, &writer)) {
            writers.remove(pos);
        }
    }
}

async fn handle_message(
    msg: CompositorMessage,
    shared: &SharedIpcState,
    writer: &IpcWriter,
    context: &mut Vec<(String, String)>,
    conn_session_id: &mut Option<String>,
) {
    let response = match msg {
        CompositorMessage::NaturalLanguageInput { text, session_id } => {
            let id = uuid::Uuid::new_v4().to_string();
            let sid = session_id.or_else(|| conn_session_id.clone());
            info!("Parsing NL input [{}]: {}", id, text);

            match shared.parser.lock().await.parse(&text, "", None, context, &shared.registry).await {
                Ok(plan) => {
                    let summary = format!("Planned: {} ({} steps)", plan.intent, plan.steps.len());
                    context.push((text, summary));
                    if context.len() > MAX_CONTEXT_ENTRIES { context.remove(0); }
                    shared.pending.lock().await.insert(id.clone(), PendingPlan { plan: plan.clone(), session_id: sid.clone() });
                    AgentMessage::TaskPlanReady { id, plan, session_id: sid }
                }
                Err(e) => {
                    error!("Intent parse error: {}", e);
                    AgentMessage::Error { id, message: e }
                }
            }
        }

        CompositorMessage::ParseIntent { id, input, session_id } => {
            let sid = session_id.or_else(|| conn_session_id.clone());
            info!("Parsing intent [{}]: {}", id, input);
            match shared.parser.lock().await.parse(&input, "", None, context, &shared.registry).await {
                Ok(plan) => {
                    let summary = format!("Planned: {} ({} steps)", plan.intent, plan.steps.len());
                    context.push((input, summary));
                    if context.len() > MAX_CONTEXT_ENTRIES { context.remove(0); }
                    shared.pending.lock().await.insert(id.clone(), PendingPlan { plan: plan.clone(), session_id: sid.clone() });
                    AgentMessage::TaskPlanReady { id, plan, session_id: sid }
                }
                Err(e) => {
                    error!("Intent parse error: {}", e);
                    AgentMessage::Error { id, message: e }
                }
            }
        }

        CompositorMessage::Approve { id, session_id } => {
            let pending_plan = shared.pending.lock().await.remove(&id);
            if let Some(pp) = pending_plan {
                let sid = session_id.or(pp.session_id).or_else(|| conn_session_id.clone());
                let plan = pp.plan;
                info!("Executing approved plan: {}", plan.intent);
                let mut results = Vec::new();

                for (i, step) in plan.steps.iter().enumerate() {
                    // Check ActionTier permission gate
                    {
                        let gate = shared.substrate_tier_gate.lock().await;
                        if !gate.check_action_permitted(&step.capability, &step.action) {
                            let error_res = soma_common::CapabilityResult {
                                success: false,
                                data: serde_json::Value::Null,
                                error: Some(soma_common::CapabilityError::new(
                                    soma_common::ErrorReason::PermissionDenied,
                                    format!("Action exceeds current developmental ActionTier ({:?})", gate.current_tier),
                                )),
                                state_delta: None,
                            };
                            send_message(&AgentMessage::StepResult {
                                id: id.clone(),
                                step_index: i,
                                result: error_res.clone(),
                                session_id: sid.clone(),
                            }, writer).await;
                            results.push(error_res);
                            continue;
                        }
                    }

                    // Scope enforcement via SessionManager
                    if let Some(ref session_id) = sid {
                        let path = step.params.get("path")
                            .or_else(|| step.params.get("from"))
                            .and_then(|v| v.as_str());
                        let path_to_check = if ["filesystem", "process", "script"].contains(&step.capability.as_str()) {
                            path
                        } else {
                            None
                        };

                        let scope_result = shared.session_manager
                            .lock().unwrap()
                            .check_scope(session_id, &step.capability, path_to_check);

                        if let Err(msg) = scope_result {
                            let scope_err = soma_common::CapabilityResult {
                                success: false,
                                data: serde_json::Value::Null,
                                error: Some(soma_common::CapabilityError::new(
                                    soma_common::ErrorReason::PermissionDenied,
                                    msg,
                                )),
                                state_delta: None,
                            };
                            send_message(&AgentMessage::StepResult {
                                id: id.clone(),
                                step_index: i,
                                result: scope_err.clone(),
                                session_id: sid.clone(),
                            }, writer).await;
                            results.push(scope_err);
                            continue;
                        }
                    }

                    let result = shared.executor.execute_step(step, &shared.registry);
                    info!(
                        "  Step {}: {}.{} → {}",
                        i, step.capability, step.action,
                        if result.success { "ok" } else { "error" }
                    );

                    send_message(&AgentMessage::StepResult {
                        id: id.clone(),
                        step_index: i,
                        result: result.clone(),
                        session_id: sid.clone(),
                    }, writer).await;

                    // V2 Consequence Tracking & Prediction Accuracy Validation
                    if let Some(ref delta) = result.state_delta {
                        shared.substrate_consequence_observer.lock().await.register_action(
                            step.capability.clone(),
                            step.action.clone(),
                            delta.before.clone(),
                            delta.clone(),
                            step.predicted_delta.clone(),
                        );

                        if let Some(ref predicted) = step.predicted_delta {
                            let matches = delta.delta_summary.to_lowercase().contains(&predicted.to_lowercase())
                                || (predicted.contains("Prediction required") && delta.delta_summary.contains("No significant"));
                            shared.substrate_history.lock().await.record_prediction(
                                i,
                                step.capability.clone(),
                                step.action.clone(),
                                predicted.clone(),
                                delta.delta_summary.clone(),
                                matches,
                            );
                        }
                    }

                    // Push browser screenshots
                    if step.capability == "browser" && result.success {
                        if result.data.get("screenshot_base64").is_some() {
                            send_message(&AgentMessage::BrowserUpdate {
                                url: result.data["url"].as_str().unwrap_or("").to_string(),
                                title: result.data["title"].as_str().unwrap_or("").to_string(),
                                screenshot_base64: result.data["screenshot_base64"].as_str().map(|s| s.to_string()),
                            }, writer).await;
                        }
                    }

                    // Session lifecycle + IPC command forwarding
                    if (step.capability == "desktop_agent" || step.capability == "sheets"
                        || step.capability == "docs" || step.capability == "media"
                        || step.capability == "delegate" || step.capability == "update") && result.success
                    {
                        let msg_type = result.data["ipc_message"].as_str().unwrap_or("");
                        if msg_type == "AgentModeStarted" {
                            let task = result.data["task"].as_str().unwrap_or("").to_string();
                            let scope: Option<SessionScope> = result.data.get("scope")
                                .and_then(|s| serde_json::from_value(s.clone()).ok());
                            let new_sid = shared.session_manager.lock().unwrap().create(&task, scope.clone());
                            *conn_session_id = Some(new_sid.clone());
                            info!("Session started: {} ({})", new_sid, task);
                        } else if msg_type == "AgentModeEnded" {
                            if let Some(ref sid_val) = sid.as_ref().or(conn_session_id.as_ref()) {
                                shared.session_manager.lock().unwrap().end(sid_val);
                            }
                            *conn_session_id = None;
                        }
                        forward_ipc_command(&result, &shared.observer, writer, sid.as_deref(), &shared.session_manager).await;
                    }

                    // Record step + resources in session
                    if let Some(ref session_id) = sid {
                        let mut sm = shared.session_manager.lock().unwrap();
                        if let Some(session) = sm.get_mut(session_id) {
                            session.record_step(&step.capability, &step.action, result.success);
                            if step.capability == "filesystem" {
                                if let Some(path) = result.data.get("path").and_then(|p| p.as_str()) {
                                    session.record_resource(path);
                                }
                            }
                        }
                    }

                    results.push(result);
                }

                let ok = results.iter().filter(|r| r.success).count();
                if let Some(last) = context.last_mut() {
                    last.1 = format!("Executed: {} ({}/{} steps succeeded)", plan.intent, ok, results.len());
                }

                AgentMessage::ExecutionComplete { id, results, session_id: sid }
            } else {
                AgentMessage::Error { id, message: "No pending plan found for this ID".to_string() }
            }
        }

        CompositorMessage::Reject { id, .. } => {
            shared.pending.lock().await.remove(&id);
            info!("Plan rejected: {}", id);
            return;
        }

        CompositorMessage::DirectExec { id, command } => {
            info!("Direct exec: {}", command);
            let result = shared.executor.execute_raw(&command);
            AgentMessage::DirectOutput { id, result }
        }

        CompositorMessage::ListCapabilities => {
            AgentMessage::Capabilities { capabilities: shared.registry.list() }
        }

        CompositorMessage::UpdateConfig { provider, model, api_key, api_url } => {
            let id = uuid::Uuid::new_v4().to_string();
            let mut config = SomaConfig::load();
            config.model.provider = provider.clone();
            config.model.model    = model.clone();
            config.model.api_key  = api_key;
            config.model.api_url  = api_url;
            let save_result = config.save().map_err(|e| e.to_string());
            match save_result {
                Ok(_) => {
                    shared.parser.lock().await.set_provider(&config);
                    info!("Config updated: provider={} model={}", provider, model);
                    AgentMessage::ConfigUpdated { provider, model }
                }
                Err(e) => {
                    error!("Failed to save config: {}", e);
                    AgentMessage::Error { id, message: format!("Failed to save config: {}", e) }
                }
            }
        }

        CompositorMessage::DesktopEvent { event_type, window_title, timestamp } => {
            shared.observer.lock().await.observe(event_type.clone(), window_title.clone(), timestamp);
            info!("Desktop event: {} — {}", event_type, window_title);
            return;
        }

        CompositorMessage::AnnotateWorkflow { name } => {
            shared.observer.lock().await.annotate_workflow(&name);
            info!("Annotated workflow: {}", name);
            return;
        }

        CompositorMessage::PrivateModeChanged { active } => {
            shared.observer.lock().await.set_active(!active);
            info!("Private mode: {}", active);
            return;
        }

        CompositorMessage::DynamicAppAction { app_id, action_id, window_id } => {
            info!("DynamicApp action: app={} action={} win={}", app_id, action_id, window_id);
            return;
        }

        CompositorMessage::AppStateChanged { window_id, state } => {
            info!("AppStateChanged: window={}", window_id);
            shared.app_state_cache.lock().unwrap().insert(window_id, state);
            return;
        }

        CompositorMessage::ReloadCapabilities => {
            let count = shared.registry.list().len();
            info!("ReloadCapabilities received — {} capabilities registered (reload takes effect on restart)", count);
            AgentMessage::CapabilitiesReloaded { count }
        }

        CompositorMessage::QueryCapabilities => {
            AgentMessage::Capabilities { capabilities: shared.registry.list() }
        }

        CompositorMessage::Ping => AgentMessage::Pong,

        CompositorMessage::GetSessionStatus { session_id } => {
            let sid = session_id.or_else(|| conn_session_id.clone());
            let status = sid.as_ref().and_then(|id| {
                shared.session_manager.lock().unwrap().get(id).map(|s| s.to_status())
            });
            AgentMessage::SessionStatusResponse { status }
        }

        CompositorMessage::ListSessions => {
            let sessions = shared.session_manager.lock().unwrap().list_statuses();
            AgentMessage::SessionList { sessions }
        }

        CompositorMessage::InterruptSession { session_id } => {
            shared.session_manager.lock().unwrap().end(&session_id);
            if conn_session_id.as_deref() == Some(session_id.as_str()) {
                *conn_session_id = None;
            }
            info!("Session interrupted: {}", session_id);
            AgentMessage::SessionInterrupted { session_id }
        }

        // TCP-only auth handshake — on Unix socket connections this is a no-op
        CompositorMessage::Auth { .. } => {
            return;
        }

        // OTA approval — handled by compositor; agent receives this as confirmation
        CompositorMessage::ApproveUpdate { version } => {
            info!("OTA update approved for version: {}", version);
            return;
        }

        // ApplyLayout is a compositor-internal directive forwarded from the sidebar;
        // the agent does not need to act on it.
        CompositorMessage::ApplyLayout { .. } => {
            return;
        }
    };

    send_message(&response, writer).await;
}

/// Forward a capability result that carries an `ipc_message` key as a real IPC message.
async fn forward_ipc_command(
    result: &soma_common::CapabilityResult,
    observer: &Arc<Mutex<DesktopObserver>>,
    writer: &IpcWriter,
    session_id: Option<&str>,
    session_manager: &SharedSessionManager,
) {
    let msg_type = result.data["ipc_message"].as_str().unwrap_or("");
    let agent_msg: Option<AgentMessage> = match msg_type {
        "AgentModeStarted" => {
            let task = result.data["task"].as_str().unwrap_or("").to_string();
            let scope: Option<SessionScope> = result.data.get("scope")
                .and_then(|s| serde_json::from_value(s.clone()).ok());
            let sid = session_id.map(|s| s.to_string());
            Some(AgentMessage::AgentModeStarted { task, scope, session_id: sid })
        }
        "AgentModeEnded" => {
            Some(AgentMessage::AgentModeEnded { session_id: session_id.map(|s| s.to_string()) })
        }
        "GetSessionStatus" => {
            let status = session_id.and_then(|id| {
                session_manager.lock().unwrap().get(id).map(|s| s.to_status())
            });
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
        "LayoutProposal" => {
            let layout: soma_common::LayoutSpec = serde_json::from_value(result.data["layout"].clone())
                .unwrap_or_else(|_| soma_common::LayoutSpec {
                    name: "custom".to_string(),
                    description: String::new(),
                    windows: Vec::new(),
                    preset: None,
                });
            Some(AgentMessage::LayoutProposal { layout })
        }
        "SaveLayout" => {
            // Layout snapshot is taken by the compositor when it receives this message
            let name = result.data["name"].as_str().unwrap_or("layout").to_string();
            let description = result.data["description"].as_str().unwrap_or("").to_string();
            Some(AgentMessage::LayoutProposal {
                layout: soma_common::LayoutSpec {
                    name,
                    description,
                    windows: Vec::new(), // compositor fills in current geometry
                    preset: Some("snapshot".to_string()),
                },
            })
        }
        "UpdateAvailable" => {
            let version   = result.data["version"].as_str().unwrap_or("").to_string();
            let size_bytes = result.data["size_bytes"].as_u64().unwrap_or(0);
            Some(AgentMessage::UpdateAvailable { version, size_bytes })
        }
        "ApplyUpdate" => {
            // Requires compositor HITL — we surface as an ActivityUpdate with a
            // special prefix so the compositor can show the approve button.
            let version = result.data["version"].as_str().unwrap_or("").to_string();
            Some(AgentMessage::ActivityUpdate {
                text: format!("UPDATE_PENDING:{}", version),
            })
        }
        "DelegateRun" => {
            let node_name = result.data["node"].as_str().unwrap_or("").to_string();
            let task      = result.data["task"].as_str().unwrap_or("").to_string();
            // Fire-and-forget: spawn a task that connects to the peer and relays results.
            let writer_clone = writer.clone();
            let session_id_owned = session_id.map(|s| s.to_string());
            tokio::spawn(delegate_to_peer(node_name, task, session_id_owned, writer_clone));
            None
        }
        _ => None,
    };

    if let Some(msg) = agent_msg {
        send_message(&msg, writer).await;
    }
}

/// Connect to a named peer node over TCP, send a task, and stream results back
/// as ActivityUpdate messages to the originating compositor connection.
async fn delegate_to_peer(
    node_name: String,
    task: String,
    session_id: Option<String>,
    writer: IpcWriter,
) {
    use crate::config::SomaConfig;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    let config = SomaConfig::load();
    let peer = match config.network.peers.iter().find(|p| p.name == node_name) {
        Some(p) => p.clone(),
        None => {
            let msg = AgentMessage::ActivityUpdate {
                text: format!("delegate: unknown node '{}'", node_name),
            };
            send_message(&msg, &writer).await;
            return;
        }
    };

    let stream = match TcpStream::connect(&peer.addr).await {
        Ok(s) => s,
        Err(e) => {
            let msg = AgentMessage::ActivityUpdate {
                text: format!("delegate: cannot connect to {}: {}", peer.addr, e),
            };
            send_message(&msg, &writer).await;
            return;
        }
    };

    let (reader, mut write_half) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);

    // Auth handshake
    if let Some(token) = &peer.token {
        let auth = serde_json::json!({"type": "Auth", "token": token});
        let _ = write_half.write_all(format!("{}\n", auth).as_bytes()).await;
    }

    // Send NaturalLanguageInput
    let nl_msg = CompositorMessage::NaturalLanguageInput {
        text: task.clone(),
        session_id: session_id.clone(),
    };
    if let Ok(json) = serde_json::to_string(&nl_msg) {
        let _ = write_half.write_all(format!("{}\n", json).as_bytes()).await;
    }

    send_message(&AgentMessage::ActivityUpdate {
        text: format!("delegate → {}: {}", node_name, &task[..task.len().min(40)]),
    }, &writer).await;

    // Read responses until ExecutionComplete or connection closes
    let mut line = String::new();
    let mut plan_id: Option<String> = None;

    loop {
        line.clear();
        match buf_reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }

        match serde_json::from_str::<AgentMessage>(trimmed) {
            Ok(AgentMessage::TaskPlanReady { id, session_id: sid, .. }) => {
                plan_id = Some(id.clone());
                // Auto-approve the delegated plan
                let approve = CompositorMessage::Approve { id, session_id: sid };
                if let Ok(json) = serde_json::to_string(&approve) {
                    let _ = write_half.write_all(format!("{}\n", json).as_bytes()).await;
                }
            }
            Ok(AgentMessage::StepResult { step_index, result, .. }) => {
                let status = if result.success { "ok" } else { "err" };
                send_message(&AgentMessage::ActivityUpdate {
                    text: format!("[{}] step {} — {}", node_name, step_index, status),
                }, &writer).await;
            }
            Ok(AgentMessage::ExecutionComplete { results, .. }) => {
                let ok = results.iter().filter(|r| r.success).count();
                send_message(&AgentMessage::ActivityUpdate {
                    text: format!("[{}] done ({}/{} steps ok)", node_name, ok, results.len()),
                }, &writer).await;
                break;
            }
            Ok(AgentMessage::Error { message, .. }) => {
                send_message(&AgentMessage::ActivityUpdate {
                    text: format!("[{}] error: {}", node_name, &message[..message.len().min(60)]),
                }, &writer).await;
                break;
            }
            _ => {}
        }
    }

    let _ = plan_id; // used above
}

pub(crate) async fn send_message(msg: &AgentMessage, writer: &IpcWriter) {
    if let Ok(json) = serde_json::to_string(msg) {
        let mut w = writer.lock().await;
        let line = format!("{}\n", json);
        if let Err(e) = w.write_all(line.as_bytes()).await {
            error!("Failed to send message: {}", e);
        }
    }
}
