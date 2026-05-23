mod atomic;
mod delta;
mod manifest;

use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("help");

    match cmd {
        "check" => cmd_check().await?,
        "apply" => {
            let version = args.get(2).ok_or("Usage: soma-updater apply <version>")?;
            cmd_apply(version).await?;
        }
        "rollback" => cmd_rollback()?,
        _ => {
            eprintln!("soma-updater — SomaOS OTA update client");
            eprintln!();
            eprintln!("Usage:");
            eprintln!("  soma-updater check              Check for available updates");
            eprintln!("  soma-updater apply <version>    Download and apply an update");
            eprintln!("  soma-updater rollback           Restore the previous binary");
        }
    }

    Ok(())
}

fn soma_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".soma")
}

fn current_binary() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/usr/bin/soma-agent"))
}

/// Check the configured manifest URL for available updates.
async fn cmd_check() -> Result<(), Box<dyn std::error::Error>> {
    let updates_dir = soma_dir().join("updates");
    let manifest_url_path = updates_dir.join("manifest_url.txt");

    let manifest_url = std::fs::read_to_string(&manifest_url_path)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "https://updates.somaos.io/stable/manifest.toml".to_string());

    log::info!("Checking for updates at {}", manifest_url);
    let toml_str = manifest::UpdateManifest::fetch(&manifest_url).await?;
    let parsed   = manifest::UpdateManifest::parse_and_verify(&toml_str)?;

    if !parsed.verified {
        log::warn!("Manifest signature verification FAILED — treating as untrusted");
    }

    let m = &parsed.inner.manifest;
    println!("Available: v{} ({} channel) — published {}", m.version, m.channel, m.published_at);

    let target = std::env::consts::ARCH;
    for artifact in &parsed.inner.artifacts {
        if artifact.target.contains(target) {
            println!(
                "  Artifact for {} — {:.1} MB, based on v{}",
                artifact.target,
                artifact.size_bytes as f64 / 1_048_576.0,
                artifact.base_version,
            );
        }
    }

    // Cache manifest for later apply
    std::fs::create_dir_all(&updates_dir)?;
    std::fs::write(updates_dir.join("latest_manifest.toml"), &toml_str)?;

    Ok(())
}

/// Download and apply an update for `version`.
async fn cmd_apply(version: &str) -> Result<(), Box<dyn std::error::Error>> {
    log::info!("Applying update to v{}", version);

    let updates_dir  = soma_dir().join("updates");
    let manifest_path = updates_dir.join("latest_manifest.toml");

    let toml_str = std::fs::read_to_string(&manifest_path)
        .map_err(|_| "Run 'soma-updater check' first to fetch the manifest")?;

    let parsed = manifest::UpdateManifest::parse_and_verify(&toml_str)?;

    if !parsed.verified {
        return Err("Manifest signature invalid — refusing to apply update".into());
    }

    if parsed.inner.manifest.version != version {
        return Err(format!(
            "Manifest is for v{}, not v{}",
            parsed.inner.manifest.version, version
        ).into());
    }

    let arch = format!("{}-unknown-linux-musl", std::env::consts::ARCH);
    let artifact = parsed.inner.artifacts.iter()
        .find(|a| a.target == arch)
        .ok_or(format!("No artifact for target {}", arch))?;

    let base_binary  = current_binary();
    let output_path  = updates_dir.join(format!("soma-agent-{}.new", version));

    delta::download_and_apply(
        &artifact.url,
        &artifact.sha256,
        &base_binary,
        &output_path,
    ).await?;

    let swap = atomic::AtomicSwap::new(&base_binary);
    swap.apply(&output_path)?;

    std::fs::remove_file(&output_path).ok();
    println!("Update to v{} applied. Restart soma-agent to use the new version.", version);

    Ok(())
}

fn cmd_rollback() -> Result<(), Box<dyn std::error::Error>> {
    let binary = current_binary();
    let swap   = atomic::AtomicSwap::new(&binary);

    if !swap.has_backup() {
        return Err("No backup found — cannot rollback".into());
    }

    swap.rollback()?;
    println!("Rollback complete.");
    Ok(())
}
