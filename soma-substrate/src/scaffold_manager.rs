use soma_common::{Scaffold, ScaffoldType, ScaffoldState, ActionTier, BehavioralHistory, SystemMode};

pub struct ScaffoldManager {
    pub scaffolds: Vec<Scaffold>,
}

impl ScaffoldManager {
    pub fn new() -> Self {
        Self {
            scaffolds: vec![
                Scaffold {
                    scaffold_type: ScaffoldType::HumanApproval,
                    state: ScaffoldState::Active,
                    activation_level: 1.0,
                },
                Scaffold {
                    scaffold_type: ScaffoldType::ActionSpaceRestriction,
                    state: ScaffoldState::Active,
                    activation_level: 1.0,
                },
                Scaffold {
                    scaffold_type: ScaffoldType::ModeProtection,
                    state: ScaffoldState::Active,
                    activation_level: 1.0,
                },
                Scaffold {
                    scaffold_type: ScaffoldType::PredictionRequirement,
                    state: ScaffoldState::Active,
                    activation_level: 1.0,
                },
                Scaffold {
                    scaffold_type: ScaffoldType::ExplanationRequirement,
                    state: ScaffoldState::Active,
                    activation_level: 1.0,
                },
            ]
        }
    }

    pub fn evaluate_scaffolds(&mut self, history: &BehavioralHistory, current_tier: ActionTier, current_mode: SystemMode) {
        // If there's a recent anomaly or system is Stressed, reactivate all scaffolds fully
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        let recent_anomaly = history.anomalies.iter().any(|a| now_ms - a.timestamp_ms < 180_000);

        if recent_anomaly || current_mode == SystemMode::Stressed {
            // Reactivate fully
            for s in &mut self.scaffolds {
                s.state = ScaffoldState::Active;
                s.activation_level = 1.0;
            }
            return;
        }

        let action_count = history.action_log.len();
        
        for s in &mut self.scaffolds {
            match s.scaffold_type {
                ScaffoldType::HumanApproval => {
                    // HITL degrades as tier advances:
                    match current_tier {
                        ActionTier::Observe => {
                            s.activation_level = 0.0;
                            s.state = ScaffoldState::Latent;
                        }
                        ActionTier::Touch => {
                            if action_count > 10 {
                                s.activation_level = 0.0;
                                s.state = ScaffoldState::Latent;
                            } else {
                                s.activation_level = 0.5;
                                s.state = ScaffoldState::Active;
                            }
                        }
                        ActionTier::Operate => {
                            if action_count > 25 {
                                s.activation_level = 0.1;
                                s.state = ScaffoldState::Active;
                            } else {
                                s.activation_level = 0.7;
                                s.state = ScaffoldState::Active;
                            }
                        }
                        ActionTier::Control | ActionTier::Autonomous => {
                            if action_count > 50 {
                                s.activation_level = 0.2;
                                s.state = ScaffoldState::Active;
                            } else {
                                s.activation_level = 0.9;
                                s.state = ScaffoldState::Active;
                            }
                        }
                    }
                }
                ScaffoldType::ActionSpaceRestriction => {
                    // Action restrictions dissolve at Autonomous tier
                    if current_tier == ActionTier::Autonomous {
                        s.activation_level = 0.0;
                        s.state = ScaffoldState::Dissolved;
                    } else {
                        s.activation_level = 1.0 - (current_tier as usize as f64 / 4.0);
                        s.state = ScaffoldState::Active;
                    }
                }
                ScaffoldType::ModeProtection => {
                    // Keeps active in normal/underload modes, latent when perfectly idle
                    if current_mode == SystemMode::Idle {
                        s.activation_level = 0.0;
                        s.state = ScaffoldState::Latent;
                    } else {
                        s.activation_level = 0.5;
                        s.state = ScaffoldState::Active;
                    }
                }
                ScaffoldType::PredictionRequirement => {
                    if action_count > 40 {
                        s.activation_level = 0.0;
                        s.state = ScaffoldState::Latent;
                    } else {
                        s.activation_level = 0.8;
                        s.state = ScaffoldState::Active;
                    }
                }
                ScaffoldType::ExplanationRequirement => {
                    if action_count > 20 {
                        s.activation_level = 0.0;
                        s.state = ScaffoldState::Latent;
                    } else {
                        s.activation_level = 0.5;
                        s.state = ScaffoldState::Active;
                    }
                }
            }
        }
    }

    pub fn check_hitl_required(&self, action_tier: ActionTier) -> bool {
        if let Some(hitl) = self.scaffolds.iter().find(|s| s.scaffold_type == ScaffoldType::HumanApproval) {
            if hitl.state == ScaffoldState::Active {
                if action_tier >= ActionTier::Operate {
                    return hitl.activation_level > 0.15;
                }
                return hitl.activation_level > 0.4;
            }
        }
        false
    }
}
