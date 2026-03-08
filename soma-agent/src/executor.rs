use soma_common::{CapabilityResult, CommandResult, TaskStep};
use std::process::Command;

use crate::capabilities::CapabilityRegistry;

/// Execute task steps through the capability registry
pub struct Executor;

impl Executor {
    pub fn new() -> Self {
        Self
    }

    /// Execute a capability-based task step
    pub fn execute_step(&self, step: &TaskStep, registry: &CapabilityRegistry) -> CapabilityResult {
        registry.execute(&step.capability, &step.action, &step.params)
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
