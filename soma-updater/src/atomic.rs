//! Atomic binary swap with rollback support.
//!
//! Sequence:
//!   soma-agent.new  (downloaded / patched binary)
//!   soma-agent.old  ← rename(soma-agent, soma-agent.old)
//!   soma-agent      ← rename(soma-agent.new, soma-agent)
//!
//! On rollback:
//!   soma-agent      ← rename(soma-agent.old, soma-agent)

use std::path::{Path, PathBuf};

pub struct AtomicSwap {
    target: PathBuf,
    backup: PathBuf,
}

impl AtomicSwap {
    pub fn new(target: impl AsRef<Path>) -> Self {
        let target = target.as_ref().to_path_buf();
        let mut backup = target.clone();
        let name = backup
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("binary")
            .to_string();
        backup.set_file_name(format!("{}.old", name));
        Self { target, backup }
    }

    /// Swap `new_binary` into place. Creates `.old` as rollback point.
    /// Returns `Err` if the swap fails (rollback is automatic).
    pub fn apply(&self, new_binary: &Path) -> Result<(), Box<dyn std::error::Error>> {
        // Step 1: backup current binary
        if self.target.exists() {
            std::fs::rename(&self.target, &self.backup)?;
        }

        // Step 2: place new binary
        match std::fs::rename(new_binary, &self.target) {
            Ok(_) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&self.target, std::fs::Permissions::from_mode(0o755))?;
                }
                log::info!("Swap complete: {:?} → {:?}", new_binary, self.target);
                Ok(())
            }
            Err(e) => {
                // Restore backup
                if self.backup.exists() {
                    let _ = std::fs::rename(&self.backup, &self.target);
                }
                Err(e.into())
            }
        }
    }

    /// Restore the `.old` backup over the current binary.
    pub fn rollback(&self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.backup.exists() {
            return Err("No backup found to rollback from".into());
        }
        std::fs::rename(&self.backup, &self.target)?;
        log::info!("Rollback: restored {:?} from {:?}", self.target, self.backup);
        Ok(())
    }

    pub fn has_backup(&self) -> bool {
        self.backup.exists()
    }
}
