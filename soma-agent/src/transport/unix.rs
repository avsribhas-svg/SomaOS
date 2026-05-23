//! Unix socket transport — always active, owner-only permissions (0o600).

use log::{error, info};
use soma_common::AGENT_SOCKET_PATH;
use tokio::net::UnixListener;

use crate::ipc::{handle_connection, SharedIpcState};

pub async fn run_unix_listener(shared: SharedIpcState) -> Result<(), Box<dyn std::error::Error>> {
    let _ = std::fs::remove_file(AGENT_SOCKET_PATH);
    let listener = UnixListener::bind(AGENT_SOCKET_PATH)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            AGENT_SOCKET_PATH,
            std::fs::Permissions::from_mode(0o600),
        )?;
    }

    info!("Agent daemon listening on {} (Unix)", AGENT_SOCKET_PATH);

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                info!("New Unix client connection");
                let (reader, writer) = stream.into_split();
                let shared = shared.clone();
                tokio::spawn(handle_connection(
                    reader,
                    Box::new(writer),
                    shared,
                ));
            }
            Err(e) => {
                error!("Unix accept error: {}", e);
            }
        }
    }
}
