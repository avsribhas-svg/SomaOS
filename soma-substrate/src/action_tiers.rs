use soma_common::{ActionTier, TierTransition, BehavioralHistory, SystemMode};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TierGate {
    pub current_tier: ActionTier,
}

impl TierGate {
    pub fn new() -> Self {
        Self {
            current_tier: ActionTier::Observe, // Start at Observe tier by default
        }
    }

    /// Classification of capability action into architectural tiers
    pub fn classify_action(capability: &str, action: &str) -> ActionTier {
        match capability {
            "filesystem" => match action {
                "read_file" | "list_dir" | "file_exists" | "file_info" | "search" | "find_files" => ActionTier::Observe,
                "write_file" | "create_dir" | "copy_file" | "move_file" | "append_file" => ActionTier::Touch,
                "delete_file" | "delete_dir" => ActionTier::Control,
                _ => ActionTier::Touch,
            },
            "process" => match action {
                "list_processes" | "process_info" | "get_process_by_name" => ActionTier::Observe,
                "start_process" | "restart_service" | "stop_service" => ActionTier::Operate,
                "kill_process" => ActionTier::Control,
                _ => ActionTier::Operate,
            },
            "system" => ActionTier::Observe, // all info reads
            "network" => match action {
                "ping" | "dns_lookup" | "http_get" => ActionTier::Observe,
                "http_post" | "http_put" | "download_file" => ActionTier::Operate,
                _ => ActionTier::Operate,
            },
            "package" => match action {
                "list_installed" | "search_packages" => ActionTier::Observe,
                "install_package" | "remove_package" | "update_package" => ActionTier::Operate,
                _ => ActionTier::Operate,
            },
            "browser" => match action {
                "navigate" | "screenshot" | "get_url" | "search" => ActionTier::Observe,
                "click" | "type_text" | "submit_form" => ActionTier::Touch,
                _ => ActionTier::Touch,
            },
            "sheets" | "docs" => match action {
                "read_sheet" | "read_cell" | "read_range" | "get_sheet_info" | "read_doc" => ActionTier::Observe,
                "write_cell" | "write_range" | "add_row" | "edit_doc" | "create_doc" => ActionTier::Touch,
                _ => ActionTier::Touch,
            },
            "desktop_agent" | "delegate" | "update" => ActionTier::Autonomous,
            _ => ActionTier::Touch, // default fallback
        }
    }

    pub fn check_action_permitted(&self, capability: &str, action: &str) -> bool {
        let action_tier = Self::classify_action(capability, action);
        // Permitted if action tier <= current tier
        action_tier <= self.current_tier
    }

    pub fn evaluate_transitions(&mut self, history: &BehavioralHistory, current_mode: SystemMode) -> Option<TierTransition> {
        let old_tier = self.current_tier;

        // PROTECTION CONTRACTION: If system is in stressed or degraded mode, contract tiers to Observe or Touch
        if (current_mode == SystemMode::Stressed || current_mode == SystemMode::Degraded) && self.current_tier > ActionTier::Touch {
            self.current_tier = ActionTier::Touch;
            return Some(TierTransition {
                from: old_tier,
                to: ActionTier::Touch,
                reason: format!("System safety contraction due to Stressed/Degraded mode ({:?})", current_mode),
                timestamp_ms: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
            });
        }

        // ANOMALY CONTRACTION: If an anomaly was detected recently (last 3 minutes), contract tier
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        let recent_anomaly = history.anomalies.iter()
            .any(|a| now_ms - a.timestamp_ms < 180_000); // 3 minutes

        if recent_anomaly && self.current_tier > ActionTier::Observe {
            self.current_tier = ActionTier::Observe;
            return Some(TierTransition {
                from: old_tier,
                to: ActionTier::Observe,
                reason: "Automated contraction to Observe tier due to recent behavioral anomaly".to_string(),
                timestamp_ms: now_ms,
            });
        }

        // BASELINE EVALUATION based on consistency score
        if let Some(recent_score) = history.consistency_scores.last() {
            if recent_score.action_count >= 5 {
                if recent_score.score >= 0.85 {
                    // Opportunity for advancement
                    let advanced = match self.current_tier {
                        ActionTier::Observe => Some(ActionTier::Touch),
                        ActionTier::Touch => Some(ActionTier::Operate),
                        ActionTier::Operate => Some(ActionTier::Control),
                        ActionTier::Control => Some(ActionTier::Autonomous),
                        ActionTier::Autonomous => None,
                    };
                    if let Some(new_tier) = advanced {
                        self.current_tier = new_tier;
                        return Some(TierTransition {
                            from: old_tier,
                            to: new_tier,
                            reason: format!("Advancement based on high consistency score ({:.2}) over {} actions", recent_score.score, recent_score.action_count),
                            timestamp_ms: now_ms,
                        });
                    }
                } else if recent_score.score < 0.6 {
                    // Contraction due to poor consistency
                    let contracted = match self.current_tier {
                        ActionTier::Autonomous => Some(ActionTier::Control),
                        ActionTier::Control => Some(ActionTier::Operate),
                        ActionTier::Operate => Some(ActionTier::Touch),
                        ActionTier::Touch => Some(ActionTier::Observe),
                        ActionTier::Observe => None,
                    };
                    if let Some(new_tier) = contracted {
                        self.current_tier = new_tier;
                        return Some(TierTransition {
                            from: old_tier,
                            to: new_tier,
                            reason: format!("Contraction based on low consistency score ({:.2})", recent_score.score),
                            timestamp_ms: now_ms,
                        });
                    }
                }
            }
        }

        None
    }
}
