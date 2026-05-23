pub mod tcp;
pub mod unix;

use std::sync::Arc;
use tokio::sync::Mutex;

/// Type-erased write half shared across connection handlers.
pub type IpcWriter = Arc<Mutex<Box<dyn tokio::io::AsyncWrite + Send + Unpin>>>;
