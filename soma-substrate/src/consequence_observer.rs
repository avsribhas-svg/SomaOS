use soma_common::{SystemStateSnapshot, StateDelta, ConsequenceRecord, CascadeEffect, StateChannel};
use crate::state_reflection::StateReflector;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct PendingObservation {
    pub timestamp_ms: u64,
    pub action_capability: String,
    pub action_name: String,
    pub before_state: SystemStateSnapshot,
    pub immediate_delta: StateDelta,
    pub predicted_delta: Option<String>,
    pub short_term_delta: Option<StateDelta>,
    pub medium_term_delta: Option<StateDelta>,
}

pub struct ConsequenceObserver {
    pub pending: Vec<PendingObservation>,
}

impl ConsequenceObserver {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    pub fn register_action(&mut self, capability: String, action: String, before: SystemStateSnapshot, immediate: StateDelta, predicted: Option<String>) {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.pending.push(PendingObservation {
            timestamp_ms,
            action_capability: capability,
            action_name: action,
            before_state: before,
            immediate_delta: immediate,
            predicted_delta: predicted,
            short_term_delta: None,
            medium_term_delta: None,
        });
    }

    /// Evaluates any pending observations against the current state and returns fully constructed ConsequenceRecords.
    pub fn tick(&mut self, current_state: &SystemStateSnapshot, reflector: &StateReflector) -> Vec<ConsequenceRecord> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut completed_records = Vec::new();
        let mut still_pending = Vec::new();

        for mut obs in self.pending.drain(..) {
            let elapsed = now_ms - obs.timestamp_ms;

            // Capture short-term delta around 5 seconds
            if obs.short_term_delta.is_none() && elapsed >= 5000 {
                obs.short_term_delta = Some(reflector.compute_delta(obs.before_state.clone(), current_state.clone(), &obs.action_capability, &obs.action_name));
            }

            // Capture medium-term delta around 60 seconds (or if the session is ending, we force close it)
            if elapsed >= 60000 {
                obs.medium_term_delta = Some(reflector.compute_delta(obs.before_state.clone(), current_state.clone(), &obs.action_capability, &obs.action_name));

                // Construct full ConsequenceRecord
                let mut cascading_effects = Vec::new();

                // Evaluate cascades
                let before = &obs.before_state;
                let after = current_state;

                // CPU cascade
                let before_cpu = if before.cpu_load.is_empty() { 0.0 } else { before.cpu_load.iter().sum::<f64>() / before.cpu_load.len() as f64 };
                let after_cpu = if after.cpu_load.is_empty() { 0.0 } else { after.cpu_load.iter().sum::<f64>() / after.cpu_load.len() as f64 };
                let cpu_diff = (after_cpu - before_cpu).abs();
                if cpu_diff > 10.0 {
                    let expected = obs.predicted_delta.as_ref()
                        .map(|p| p.to_lowercase().contains("cpu") || p.to_lowercase().contains("processor") || p.to_lowercase().contains("load"))
                        .unwrap_or(false);
                    cascading_effects.push(CascadeEffect {
                        affected_subsystem: StateChannel::Cpu,
                        magnitude: (cpu_diff / 100.0).min(1.0),
                        delay_ms: elapsed,
                        expected,
                    });
                }

                // Memory cascade
                let mem_diff = (after.memory_used_kb as i64 - before.memory_used_kb as i64).abs() as f64 / before.memory_total_kb.max(1) as f64;
                if mem_diff > 0.05 { // change > 5% memory
                    let expected = obs.predicted_delta.as_ref()
                        .map(|p| p.to_lowercase().contains("mem") || p.to_lowercase().contains("ram") || p.to_lowercase().contains("memory"))
                        .unwrap_or(false);
                    cascading_effects.push(CascadeEffect {
                        affected_subsystem: StateChannel::Memory,
                        magnitude: mem_diff,
                        delay_ms: elapsed,
                        expected,
                    });
                }

                // Process count cascade
                let proc_diff = (after.process_count as i32 - before.process_count as i32).abs() as f64;
                if proc_diff > 3.0 {
                    let expected = obs.predicted_delta.as_ref()
                        .map(|p| p.to_lowercase().contains("proc") || p.to_lowercase().contains("process") || p.to_lowercase().contains("kill") || p.to_lowercase().contains("start"))
                        .unwrap_or(false);
                    cascading_effects.push(CascadeEffect {
                        affected_subsystem: StateChannel::Process,
                        magnitude: (proc_diff / 50.0).min(1.0),
                        delay_ms: elapsed,
                        expected,
                    });
                }

                completed_records.push(ConsequenceRecord {
                    action_capability: obs.action_capability,
                    action_name: obs.action_name,
                    immediate_delta: obs.immediate_delta,
                    short_term_delta: obs.short_term_delta,
                    medium_term_delta: obs.medium_term_delta,
                    cascading_effects,
                });
            } else {
                still_pending.push(obs);
            }
        }

        self.pending = still_pending;
        completed_records
    }
}
