use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use log::{info, error};
use soma_common::{BehavioralHistory, ActionEvent, ConsistencyScore, PredictionRecord, TierTransition, AnomalyRecord, ActionTier, SystemMode};

pub struct BehavioralHistoryManager {
    pub history: BehavioralHistory,
    path: PathBuf,
}

impl BehavioralHistoryManager {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let path = PathBuf::from(home).join(".soma").join("behavioral_history.json");
        
        let history = if path.exists() {
            match fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(h) => h,
                    Err(e) => {
                        error!("Failed to parse behavioral history, starting fresh: {}", e);
                        Self::default_history()
                    }
                },
                Err(e) => {
                    error!("Failed to read behavioral history file: {}", e);
                    Self::default_history()
                }
            }
        } else {
            Self::default_history()
        };

        Self { history, path }
    }

    fn default_history() -> BehavioralHistory {
        BehavioralHistory {
            action_log: Vec::new(),
            consistency_scores: Vec::new(),
            prediction_accuracy: Vec::new(),
            tier_trajectory: Vec::new(),
            anomalies: Vec::new(),
        }
    }

    pub fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(&self.history) {
            Ok(content) => {
                if let Err(e) = fs::write(&self.path, content) {
                    error!("Failed to write behavioral history to {:?}: {}", self.path, e);
                }
            }
            Err(e) => {
                error!("Failed to serialize behavioral history: {}", e);
            }
        }
    }

    pub fn record_event(&mut self, mut event: ActionEvent) {
        // Evaluate consistency before pushing
        event.was_characteristic = self.is_characteristic(&event.action_capability, &event.action_name, &event.mode_at_time);
        
        self.history.action_log.push(event);
        
        // Recalculate consistency score for recent window (e.g. last 1 hour)
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let window_ms = 3600_000; // 1 hour
        let recent_actions = self.history.action_log.iter()
            .filter(|e| now_ms - e.timestamp_ms < window_ms)
            .count() as u64;

        let recent_anomalies = self.history.anomalies.iter()
            .filter(|a| now_ms - a.timestamp_ms < window_ms)
            .count() as u64;

        let consistency_score = if recent_actions == 0 {
            1.0 // default perfectly consistent if no actions
        } else {
            let characteristic_actions = self.history.action_log.iter()
                .filter(|e| now_ms - e.timestamp_ms < window_ms && e.was_characteristic)
                .count() as f64;
            
            let action_ratio = characteristic_actions / recent_actions as f64;
            let anomaly_penalty = (recent_anomalies as f64 * 0.2).min(0.5); // penalty of 20% per anomaly up to 50%
            (action_ratio - anomaly_penalty).max(0.0)
        };

        self.history.consistency_scores.push(ConsistencyScore {
            window_ms,
            score: consistency_score,
            action_count: recent_actions,
            anomaly_count: recent_anomalies,
        });

        // Limit sizes
        if self.history.action_log.len() > 1000 {
            self.history.action_log.remove(0);
        }
        if self.history.consistency_scores.len() > 100 {
            self.history.consistency_scores.remove(0);
        }

        self.save();
    }

    pub fn is_characteristic(&self, capability: &str, action: &str, context: &SystemMode) -> bool {
        // If there is very little history, everything is characteristic
        if self.history.action_log.len() < 5 {
            return true;
        }

        // An action is characteristic if it or its capability has been used under this system mode
        // or if it doesn't represent a massive spike in critical execution actions.
        let same_mode_actions = self.history.action_log.iter()
            .filter(|e| e.mode_at_time == *context)
            .count();

        if same_mode_actions == 0 {
            // New system state mode context — allow general capabilities
            return true;
        }

        let matching_actions = self.history.action_log.iter()
            .filter(|e| e.mode_at_time == *context && e.action_capability == capability)
            .count();

        matching_actions as f64 / same_mode_actions as f64 >= 0.05 // at least 5% historical presence, or it's a mild anomaly
    }

    pub fn record_prediction(&mut self, step_index: usize, capability: String, action: String, predicted_delta: String, actual_delta: String, matches: bool) {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.history.prediction_accuracy.push(PredictionRecord {
            step_index,
            capability,
            action,
            predicted_delta,
            actual_delta,
            matches,
            timestamp_ms,
        });

        if self.history.prediction_accuracy.len() > 100 {
            self.history.prediction_accuracy.remove(0);
        }

        self.save();
    }

    pub fn record_anomaly(&mut self, description: String, severity: f64) {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.history.anomalies.push(AnomalyRecord {
            timestamp_ms,
            description,
            severity,
        });

        if self.history.anomalies.len() > 100 {
            self.history.anomalies.remove(0);
        }

        self.save();
    }

    pub fn record_transition(&mut self, from: ActionTier, to: ActionTier, reason: String) {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.history.tier_trajectory.push(TierTransition {
            from,
            to,
            reason,
            timestamp_ms,
        });

        self.save();
    }

    pub fn maturity_score(&self) -> f64 {
        // Base maturity score from 0.0 to 1.0
        // Increases with action count and prediction accuracy
        // Decreases with anomalies
        let action_factor = (self.history.action_log.len() as f64 / 100.0).min(1.0); // full marks for 100 actions
        
        let prediction_factor = if self.history.prediction_accuracy.is_empty() {
            0.5 // neutral
        } else {
            let correct = self.history.prediction_accuracy.iter().filter(|p| p.matches).count() as f64;
            correct / self.history.prediction_accuracy.len() as f64
        };

        let anomaly_penalty = if self.history.anomalies.is_empty() {
            0.0
        } else {
            let sum_severity: f64 = self.history.anomalies.iter().map(|a| a.severity).sum();
            (sum_severity / 5.0).min(0.8) // severe penalty up to 0.8
        };

        let base_maturity = (action_factor * 0.4) + (prediction_factor * 0.6);
        (base_maturity - anomaly_penalty).max(0.0).min(1.0)
    }

    pub fn consistency_trend(&self) -> f64 {
        if self.history.consistency_scores.len() < 2 {
            return 0.0; // neutral trend
        }
        // Slope of last 5 consistency scores
        let len = self.history.consistency_scores.len();
        let start = len.saturating_sub(5);
        let recent_scores: Vec<f64> = self.history.consistency_scores[start..len].iter().map(|c| c.score).collect();
        
        let mut diff_sum = 0.0;
        for i in 1..recent_scores.len() {
            diff_sum += recent_scores[i] - recent_scores[i - 1];
        }
        diff_sum / (recent_scores.len() - 1) as f64
    }
}
