use log::info;
use serde_json::json;
use soma_common::{SessionScope, SessionStatus};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct SessionStep {
    pub capability: String,
    pub action: String,
    pub success: bool,
}

pub struct Session {
    pub id: String,
    pub task: String,
    pub started_at: std::time::SystemTime,
    pub steps: Vec<SessionStep>,
    pub affected_resources: Vec<String>,
    pub scope: Option<SessionScope>,
}

impl Session {
    pub fn new(task: &str, scope: Option<SessionScope>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            task: task.to_string(),
            started_at: std::time::SystemTime::now(),
            steps: Vec::new(),
            affected_resources: Vec::new(),
            scope,
        }
    }

    pub fn to_status(&self) -> SessionStatus {
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

    pub fn record_step(&mut self, capability: &str, action: &str, success: bool) {
        self.steps.push(SessionStep { capability: capability.to_string(), action: action.to_string(), success });
    }

    pub fn record_resource(&mut self, path: &str) {
        if !self.affected_resources.contains(&path.to_string()) {
            self.affected_resources.push(path.to_string());
        }
    }

    pub fn persist(&self) {
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

/// Global registry of all active agent sessions. Shared across IPC connections
/// so that multiple clients (e.g. parallel CLI invocations) share session state.
pub struct SessionManager {
    sessions: HashMap<String, Session>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self { sessions: HashMap::new() }
    }

    /// Create a new session, returning its generated ID.
    pub fn create(&mut self, task: &str, scope: Option<SessionScope>) -> String {
        let session = Session::new(task, scope);
        let id = session.id.clone();
        info!("Session created: {} ({})", id, task);
        self.sessions.insert(id.clone(), session);
        id
    }

    pub fn get(&self, id: &str) -> Option<&Session> {
        self.sessions.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    /// End and persist a session, removing it from the active map.
    pub fn end(&mut self, id: &str) {
        if let Some(session) = self.sessions.remove(id) {
            let sid = session.id.clone();
            session.persist();
            info!("Session ended and persisted: {}", sid);
        }
    }

    pub fn list_statuses(&self) -> Vec<SessionStatus> {
        self.sessions.values().map(|s| s.to_status()).collect()
    }

    /// Check capability whitelist + path whitelist for a session.
    /// Returns Err with a human-readable message if the step is out of scope.
    pub fn check_scope(
        &self,
        session_id: &str,
        capability: &str,
        path: Option<&str>,
    ) -> Result<(), String> {
        let session = match self.sessions.get(session_id) {
            Some(s) => s,
            None => return Ok(()), // No active session = no scope restriction
        };
        let scope = match &session.scope {
            Some(s) => s,
            None => return Ok(()), // Session has no scope = unrestricted
        };

        if let Some(ref whitelist) = scope.capability_whitelist {
            if !whitelist.contains(&capability.to_string()) {
                return Err(format!(
                    "Capability '{}' not in session scope whitelist (allowed: {})",
                    capability,
                    whitelist.join(", ")
                ));
            }
        }

        if let Some(ref path_whitelist) = scope.path_whitelist {
            if let Some(p) = path {
                if !p.is_empty() && !path_whitelist.iter().any(|prefix| p.starts_with(prefix.as_str())) {
                    return Err(format!(
                        "Path '{}' not in session path whitelist (allowed prefixes: {})",
                        p,
                        path_whitelist.join(", ")
                    ));
                }
            }
        }

        Ok(())
    }
}

pub type SharedSessionManager = Arc<Mutex<SessionManager>>;
