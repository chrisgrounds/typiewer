use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{LspError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
  Number(u64),
  String(String),
}

#[derive(Debug, Serialize)]
pub struct OutgoingRequest<'a> {
  pub jsonrpc: &'static str,
  pub id: u64,
  pub method: &'a str,
  pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct OutgoingNotification<'a> {
  pub jsonrpc: &'static str,
  pub method: &'a str,
  pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct OutgoingResponse {
  pub jsonrpc: &'static str,
  pub id: RequestId,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub result: Option<Value>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
  pub code: i64,
  pub message: String,
  #[serde(default)]
  pub data: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct RawMessage {
  #[serde(default)]
  pub id: Option<RequestId>,
  #[serde(default)]
  pub method: Option<String>,
  #[serde(default)]
  pub params: Option<Value>,
  #[serde(default)]
  pub result: Option<Value>,
  #[serde(default)]
  pub error: Option<RpcError>,
}

pub enum IncomingMessage {
  Response {
    id: RequestId,
    result: Option<Value>,
    error: Option<RpcError>,
  },
  Request {
    id: RequestId,
    method: String,
    params: Value,
  },
  Notification {
    method: String,
    params: Value,
  },
}

impl TryFrom<RawMessage> for IncomingMessage {
  type Error = LspError;

  fn try_from(raw: RawMessage) -> Result<Self> {
    match (raw.id, raw.method) {
      (Some(id), Some(method)) => Ok(Self::Request {
        id,
        method,
        params: raw.params.unwrap_or(Value::Null),
      }),
      (Some(id), None) => Ok(Self::Response {
        id,
        result: raw.result,
        error: raw.error,
      }),
      (None, Some(method)) => Ok(Self::Notification {
        method,
        params: raw.params.unwrap_or(Value::Null),
      }),
      (None, None) => Err(LspError::Protocol(
        "received JSON-RPC message without id or method".to_string(),
      )),
    }
  }
}

pub fn write_message<W: Write, T: Serialize>(writer: &mut W, msg: &T) -> Result<()> {
  let body = serde_json::to_vec(msg)?;
  write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
  writer.write_all(&body)?;
  writer.flush()?;
  Ok(())
}

pub fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<IncomingMessage>> {
  let mut content_length: Option<usize> = None;
  let mut header_line = String::new();

  loop {
    header_line.clear();
    let bytes_read = reader.read_line(&mut header_line)?;
    if bytes_read == 0 {
      return Ok(None);
    }

    let trimmed = header_line.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
      break;
    }

    if let Some((name, value)) = trimmed.split_once(':')
      && name.eq_ignore_ascii_case("Content-Length")
    {
      let len = value.trim().parse::<usize>().map_err(|err| {
        LspError::Protocol(format!("invalid Content-Length header '{value}': {err}"))
      })?;
      content_length = Some(len);
    }
  }

  let length = content_length
    .ok_or_else(|| LspError::Protocol("missing Content-Length header".to_string()))?;

  let mut body = vec![0u8; length];
  reader.read_exact(&mut body)?;

  let raw: RawMessage = serde_json::from_slice(&body)?;
  Ok(Some(IncomingMessage::try_from(raw)?))
}
