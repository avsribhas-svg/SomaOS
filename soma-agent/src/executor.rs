use soma_common::CommandResult;
use std::process::Command;

/// Whitelist of commands allowed to be executed
const ALLOWED_COMMANDS: &[&str] = &[
    "ls", "mkdir", "open", "cat", "echo", "pwd", "rm", "cp", "mv", "touch",
    "head", "tail", "wc", "find", "grep", "which", "whoami", "date", "uname",
];

pub struct Executor;

impl Executor {
    pub fn new() -> Self {
        Self
    }

    /// Execute a whitelisted command, returning the result
    pub fn execute(&self, command: &str, args: &[String]) -> Result<CommandResult, String> {
        if !ALLOWED_COMMANDS.contains(&command) {
            return Err(format!(
                "Command '{}' is not allowed. Whitelist: {:?}",
                command, ALLOWED_COMMANDS
            ));
        }

        match Command::new(command).args(args).output() {
            Ok(output) => Ok(CommandResult {
                success: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code(),
            }),
            Err(e) => Err(format!("Failed to execute '{}': {}", command, e)),
        }
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
