mod capabilities;
mod config;
mod executor;
mod intent;
mod ipc;
mod observer;
mod providers;

use log::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("╔══════════════════════════════════════╗");
    info!("║       Soma Agent Daemon v0.2.0       ║");
    info!("╚══════════════════════════════════════╝");

    ipc::run_ipc_server().await
}
