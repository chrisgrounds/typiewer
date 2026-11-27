use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, LspError>;

#[derive(Debug, Error)]
pub enum LspError {
  #[error("failed to spawn language server '{program}': {source}")]
  Spawn {
    program: String,
    #[source]
    source: std::io::Error,
  },

  #[error("language server stdio pipe unavailable: {0}")]
  MissingPipe(&'static str),

  #[error("I/O error communicating with language server: {0}")]
  Io(#[from] std::io::Error),

  #[error("JSON serialization/deserialization error: {0}")]
  Json(#[from] serde_json::Error),

  #[error("protocol error: {0}")]
  Protocol(String),

  #[error("LSP server error ({code}): {message}")]
  Server {
    code: i64,
    message: String,
    data: Option<serde_json::Value>,
  },

  #[error("language server closed the connection unexpectedly")]
  Disconnected,

  #[error("invalid file path for URI conversion: {0}")]
  InvalidPath(PathBuf),

  #[error("timed out waiting for language server")]
  Timeout,
}
