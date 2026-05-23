use soma_common::{CapabilityResult, CommandResult, TaskStep};
use std::process::Command;
use std::sync::Mutex;
use soma_substrate::StateReflector;

use crate::capabilities::CapabilityRegistry;

/// Execute task steps through the capability registry
pub struct Executor {
    reflector: Mutex<StateReflector>,
}

impl Executor {
    pub fn new() -> Self {
        Self {
            reflector: Mutex::new(StateReflector::new()),
        }
    }

    /// Execute a capability-based task step
    pub fn execute_step(&self, step: &TaskStep, registry: &CapabilityRegistry) -> CapabilityResult {
        let mut reflector = self.reflector.lock().unwrap();
        
        // 1. Capture state before execution
        let before_snapshot = reflector.capture_snapshot();
        
        // 2. Execute capability
        let mut result = registry.execute(&step.capability, &step.action, &step.params);
        
        // 3. Capture state after execution
        let after_snapshot = reflector.capture_snapshot();
        
        // 4. Compute state delta
        let delta = reflector.compute_delta(before_snapshot, after_snapshot, &step.capability, &step.action);
        
        // 5. Attach delta to execution result
        result.state_delta = Some(delta);
        
        result
    }

    /// Execute a raw shell command (for the embedded terminal)
    pub fn execute_raw(&self, command: &str) -> CommandResult {
        match Command::new("sh").args(["-c", command]).output() {
            Ok(output) => CommandResult {
                success: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code(),
            },
            Err(e) => CommandResult {
                success: false,
                stdout: String::new(),
                stderr: format!("Failed to execute: {}", e),
                exit_code: None,
            },
        }
    }
}

