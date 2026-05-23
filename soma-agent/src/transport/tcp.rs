//! TCP transport — optional, controlled by NetworkConfig.tcp_listen_addr.
//!
//! If tls_cert_path + tls_key_path are both set in config, TLS is applied.
//! Every connection must open with CompositorMessage::Auth { token }; the token is
//! SHA-256-hashed and compared against NetworkConfig.accepted_tokens.
//! Connections that fail auth are immediately dropped.

use log::{error, info, warn};
use sha2::{Digest, Sha256};
use soma_common::CompositorMessage;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::config::NetworkConfig;
use crate::ipc::{handle_connection, SharedIpcState};

/// Build a rustls `ServerConfig` from PEM cert + key paths.
/// Returns `None` if either path is missing or loading fails.
fn build_tls_config(cfg: &NetworkConfig) -> Option<Arc<rustls::ServerConfig>> {
    let cert_path = cfg.tls_cert_path.as_deref()?;
    let key_path  = cfg.tls_key_path.as_deref()?;

    let cert_bytes = std::fs::read(cert_path)
        .map_err(|e| warn!("TLS: cannot read cert {}: {}", cert_path, e))
        .ok()?;
    let key_bytes = std::fs::read(key_path)
        .map_err(|e| warn!("TLS: cannot read key {}: {}", key_path, e))
        .ok()?;

    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_bytes.as_slice())
            .filter_map(|r| r.ok())
            .collect();

    let key = match rustls_pemfile::private_key(&mut key_bytes.as_slice()) {
        Ok(Some(k)) => k,
        _ => { warn!("TLS: no private key found in {}", key_path); return None; }
    };

    let server_cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| warn!("TLS: config error: {}", e))
        .ok()?;

    Some(Arc::new(server_cfg))
}

/// Validate a raw bearer token against the SHA-256 whitelist.
fn token_valid(token: &str, accepted_hashes: &[String]) -> bool {
    if accepted_hashes.is_empty() {
        return true; // no auth configured → accept all
    }
    let hash = format!("{:x}", Sha256::new().chain_update(token.as_bytes()).finalize());
    accepted_hashes.iter().any(|h| h == &hash)
}

pub async fn run_tcp_listener(
    addr: String,
    network_cfg: NetworkConfig,
    shared: SharedIpcState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(&addr).await?;
    let tls_acceptor = build_tls_config(&network_cfg)
        .map(|cfg| tokio_rustls::TlsAcceptor::from(cfg));

    if tls_acceptor.is_some() {
        info!("Agent TCP listener on {} (TLS)", addr);
    } else {
        info!("Agent TCP listener on {} (plain TCP)", addr);
    }

    let accepted_tokens = Arc::new(network_cfg.accepted_tokens.clone());

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                info!("New TCP client: {}", peer);
                let shared = shared.clone();
                let tokens = accepted_tokens.clone();
                let tls_acceptor = tls_acceptor.clone();

                tokio::spawn(async move {
                    if let Some(acceptor) = tls_acceptor {
                        match acceptor.accept(stream).await {
                            Ok(tls_stream) => {
                                let (reader, writer) = tokio::io::split(tls_stream);
                                handle_tcp_connection(reader, writer, tokens, shared).await;
                            }
                            Err(e) => warn!("TLS handshake failed from {}: {}", peer, e),
                        }
                    } else {
                        let (reader, writer) = stream.into_split();
                        handle_tcp_connection(reader, writer, tokens, shared).await;
                    }
                });
            }
            Err(e) => {
                error!("TCP accept error: {}", e);
            }
        }
    }
}

/// Perform auth handshake then hand off to the main connection handler.
async fn handle_tcp_connection<R, W>(
    reader: R,
    mut writer: W,
    accepted_tokens: Arc<Vec<String>>,
    shared: SharedIpcState,
) where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
    W: tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    // First message must be Auth
    match buf_reader.read_line(&mut line).await {
        Ok(0) | Err(_) => return, // connection closed or error before auth
        Ok(_) => {}
    }

    let trimmed = line.trim();
    let auth_ok = match serde_json::from_str::<CompositorMessage>(trimmed) {
        Ok(CompositorMessage::Auth { token }) => token_valid(&token, &accepted_tokens),
        _ => {
            warn!("TCP: first message was not Auth — dropping connection");
            false
        }
    };

    if !auth_ok {
        warn!("TCP: auth failed — dropping connection");
        let _ = writer.write_all(b"{\"type\":\"Error\",\"id\":\"\",\"message\":\"auth failed\"}\n").await;
        return;
    }

    info!("TCP: auth OK");
    handle_connection(buf_reader, Box::new(writer), shared).await;
}
