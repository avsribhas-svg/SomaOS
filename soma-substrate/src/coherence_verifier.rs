use soma_common::{CoherenceReport, ArchitecturalContradiction, SystemMode, ActionTier, Scaffold, ScaffoldType, ScaffoldState, BehavioralHistory, ConsequenceRecord};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct CoherenceVerifier;

impl CoherenceVerifier {
    pub fn verify(
        current_mode: SystemMode,
        current_tier: ActionTier,
        scaffolds: &[Scaffold],
        history: &BehavioralHistory,
        recent_consequences: &[ConsequenceRecord],
    ) -> CoherenceReport {
        let mut contradictions = Vec::new();

        // Contradiction 1: Stressed system but allowed high tier actions
        if current_mode == SystemMode::Stressed && current_tier >= ActionTier::Operate {
            contradictions.push(ArchitecturalContradiction {
                property_a: "SystemMode".to_string(),
                property_b: "ActionTier".to_string(),
                description: format!("System mode is Stressed, but current action tier allows privileged actions ({:?})", current_tier),
                severity: 0.9,
            });
        }

        // Contradiction 2: Scaffolding dissolved but consistency is low
        let hitl_dissolved = scaffolds.iter().any(|s| s.scaffold_type == ScaffoldType::HumanApproval && s.state == ScaffoldState::Dissolved);
        if let Some(recent_score) = history.consistency_scores.last() {
            if hitl_dissolved && recent_score.score < 0.7 {
                contradictions.push(ArchitecturalContradiction {
                    property_a: "Scaffolding".to_string(),
                    property_b: "BehavioralHistory".to_string(),
                    description: format!("Safety scaffolds have dissolved, but agent's behavioral consistency is critically low ({:.2})", recent_score.score),
                    severity: 0.8,
                });
            }
        }

        // Contradiction 3: System thrashing (cascade effects present) but mode is Idle/Active
        let thrashing_cascades = recent_consequences.iter()
            .any(|c| c.cascading_effects.len() >= 2);
        if thrashing_cascades && (current_mode == SystemMode::Idle || current_mode == SystemMode::Active) {
            contradictions.push(ArchitecturalContradiction {
                property_a: "ConsequenceObserver".to_string(),
                property_b: "SystemMode".to_string(),
                description: format!("Consequence logs show multiple cascading resource effects, but mode engine reports {:?} mode", current_mode),
                severity: 0.6,
            });
        }

        let overall_coherence = if contradictions.is_empty() {
            1.0
        } else {
            let total_severity: f64 = contradictions.iter().map(|c| c.severity).sum();
            (1.0 - (total_severity / contradictions.len() as f64) * 0.5).max(0.0)
        };

        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        CoherenceReport {
            timestamp_ms,
            overall_coherence,
            contradictions,
        }
    }
}
