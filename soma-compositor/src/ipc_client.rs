use soma_common::{AgentMessage, CompositorMessage, AGENT_SOCKET_PATH};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, Mutex};

/// Channel for sending messages from the compositor to the agent
pub type AgentSender = mpsc::UnboundedSender<CompositorMessage>;
/// Channel for receiving messages from the agent
pub type AgentReceiver = mpsc::UnboundedReceiver<AgentMessage>;

/// Connect to the soma-agent daemon and return send/receive channels
pub async fn connect_to_agent() -> Result<(AgentSender, AgentReceiver), String> {
    let stream = UnixStream::connect(AGENT_SOCKET_PATH)
        .await
        .map_err(|e| format!("Cannot connect to agent at {}: {}", AGENT_SOCKET_PATH, e))?;

    let (reader, writer) = stream.into_split();
    let writer = Arc::new(Mutex::new(writer));

    // Channel: compositor → agent
    let (tx_to_agent, mut rx_from_compositor) = mpsc::unbounded_channel::<CompositorMessage>();

    // Channel: agent → compositor
    let (tx_to_compositor, rx_from_agent) = mpsc::unbounded_channel::<AgentMessage>();

    // Writer task: forwards compositor messages to the agent socket
    let writer_clone = Arc::clone(&writer);
    tokio::spawn(async move {
        while let Some(msg) = rx_from_compositor.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                let mut w = writer_clone.lock().await;
                let line = format!("{}\n", json);
                if w.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
        }
    });

    // Reader task: forwards agent messages to the compositor channel
    tokio::spawn(async move {
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // Connection closed
                Ok(_) => {
                    if let Ok(msg) = serde_json::from_str::<AgentMessage>(line.trim()) {
                        if tx_to_compositor.send(msg).is_err() {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });

    Ok((tx_to_agent, rx_from_agent))
}
