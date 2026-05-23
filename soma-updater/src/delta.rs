//! Delta patch application via xdelta3.
//!
//! Downloads the patch file from `url`, verifies its SHA-256, then runs:
//!   xdelta3 -d -s <base_binary> <patch_file> <output_binary>

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Download a delta patch, verify its hash, and apply it to `base_binary`.
/// Writes the patched binary to `output_path`.
pub async fn download_and_apply(
    url: &str,
    expected_sha256: &str,
    base_binary: &Path,
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    log::info!("Downloading patch from {}", url);
    let bytes = reqwest::get(url).await?.bytes().await?;

    // Verify hash
    let actual_hash = format!("{:x}", Sha256::digest(&bytes));
    if actual_hash != expected_sha256 {
        return Err(format!(
            "Patch SHA-256 mismatch: expected {}, got {}",
            expected_sha256, actual_hash
        ).into());
    }

    // Write patch to temp file
    let patch_path = output_path.with_extension("xdelta3");
    std::fs::write(&patch_path, &bytes)?;

    // Apply via xdelta3
    apply_patch(&patch_path, base_binary, output_path)?;
    std::fs::remove_file(&patch_path)?;

    log::info!("Patch applied successfully to {:?}", output_path);
    Ok(())
}

fn apply_patch(
    patch: &Path,
    base: &Path,
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("xdelta3")
        .args([
            "-d",
            "-s", base.to_str().ok_or("invalid base path")?,
            patch.to_str().ok_or("invalid patch path")?,
            output.to_str().ok_or("invalid output path")?,
        ])
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("xdelta3 exited with status: {}", status).into())
    }
}

/// Verify a file's SHA-256 hash.
pub fn verify_sha256(path: &Path, expected: &str) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read(path)?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual == expected {
        Ok(())
    } else {
        Err(format!("SHA-256 mismatch for {:?}: expected {}, got {}", path, expected, actual).into())
    }
}
