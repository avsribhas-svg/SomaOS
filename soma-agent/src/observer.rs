//! Desktop observer — records window focus/open/close events and allows
//! the user or agent to annotate named workflows from the event stream.
//!
//! Observation is suspended while private mode is active.

use log::info;
use serde::{Deserialize, Serialize};

const MAX_EVENTS: usize = 200;

/// A single observed desktop event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservedEvent {
    pub event_type: String,
    pub window_title: String,
    pub timestamp: u64,
}

/// A named workflow — a snapshot of recent events tagged by the user or agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPattern {
    pub name: String,
    pub events: Vec<ObservedEvent>,
    pub created_at: u64,
}

/// Passive desktop observer that records window events.
pub struct DesktopObserver {
    active: bool,
    events: Vec<ObservedEvent>,
    workflows: Vec<WorkflowPattern>,
}

impl DesktopObserver {
    pub fn new() -> Self {
        let mut obs = Self {
            active: true,
            events: Vec::new(),
            workflows: Vec::new(),
        };
        obs.load();
        obs
    }

    /// Record a desktop event (only when active).
    pub fn observe(&mut self, event_type: String, window_title: String, timestamp: u64) {
        if !self.active {
            return;
        }
        self.events.push(ObservedEvent {
            event_type,
            window_title,
            timestamp,
        });
        // Cap the event buffer
        if self.events.len() > MAX_EVENTS {
            self.events.drain(..self.events.len() - MAX_EVENTS);
        }
    }

    /// Toggle observation (disabled during private mode).
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
        info!("Desktop observer active={}", active);
    }

    /// Snapshot the most recent events (up to 20) as a named workflow.
    pub fn annotate_workflow(&mut self, name: &str) {
        let snapshot_count = 20.min(self.events.len());
        let events: Vec<ObservedEvent> = self
            .events
            .iter()
            .rev()
            .take(snapshot_count)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let pattern = WorkflowPattern {
            name: name.to_string(),
            events,
            created_at: ts,
        };
        info!("Annotated workflow '{}' ({} events)", name, pattern.events.len());
        self.workflows.push(pattern);
        self.save();
    }

    /// Serialise workflow history for LLM consumption.
    pub fn get_history(&self) -> String {
        match serde_json::to_string_pretty(&self.workflows) {
            Ok(json) => json,
            Err(_) => "[]".to_string(),
        }
    }

    /// Persist workflows to `~/.soma/workflows.json`.
    pub fn save(&self) {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let dir = std::path::PathBuf::from(&home).join(".soma");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("workflows.json");
        if let Ok(json) = serde_json::to_string_pretty(&self.workflows) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// Load workflows from `~/.soma/workflows.json`.
    fn load(&mut self) {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let path = std::path::PathBuf::from(&home)
            .join(".soma")
            .join("workflows.json");
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(workflows) = serde_json::from_str::<Vec<WorkflowPattern>>(&data) {
                info!("Loaded {} workflow patterns", workflows.len());
                self.workflows = workflows;
            }
        }
    }
}
