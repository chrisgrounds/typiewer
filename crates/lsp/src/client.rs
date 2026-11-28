use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use url::Url;

use crate::error::{LspError, Result};
use crate::protocol::{
  IncomingMessage, OutgoingNotification, OutgoingRequest, OutgoingResponse, RequestId,
  read_message, write_message,
};
use crate::types::{GotoDefinitionResponse, Location, Position, ServerStatusNotification};

type PendingMap = HashMap<u64, mpsc::Sender<Result<Value>>>;

#[derive(Debug)]
struct ReadyState {
  quiescent: Option<bool>,
  active_progress: HashSet<String>,
  saw_progress: bool,
  last_activity: Instant,
  disconnected: bool,
}

impl ReadyState {
  fn new() -> Self {
    Self {
      quiescent: None,
      active_progress: HashSet::new(),
      saw_progress: false,
      last_activity: Instant::now(),
      disconnected: false,
    }
  }
}

/// A synchronous JSON-RPC 2.0 client for talking to a language server
/// (such as `rust-analyzer`) over stdin/stdout.
pub struct LspClient {
  child: Mutex<Child>,
  writer: Arc<Mutex<BufWriter<ChildStdin>>>,
  pending: Arc<Mutex<PendingMap>>,
  ready: Arc<(Mutex<ReadyState>, Condvar)>,
  next_id: AtomicU64,
  shut_down: AtomicBool,
  reader_thread: Option<JoinHandle<()>>,
}

impl LspClient {
  /// Spawns `rust-analyzer` for the given workspace root and completes the
  /// LSP `initialize` / `initialized` handshake.
  pub fn start_rust_analyzer(workspace_root: impl AsRef<Path>) -> Result<Self> {
    Self::start_with_command("rust-analyzer", &[], workspace_root)
  }

  /// Spawns an arbitrary LSP server binary and completes the handshake.
  pub fn start_with_command(
    program: &str,
    args: &[&str],
    workspace_root: impl AsRef<Path>,
  ) -> Result<Self> {
    let root_path = canonicalize_or_keep(workspace_root.as_ref());
    let root_uri = path_to_uri(&root_path)?;

    let mut child = Command::new(program)
      .args(args)
      .current_dir(&root_path)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::null())
      .spawn()
      .map_err(|source| LspError::Spawn {
        program: program.to_owned(),
        source,
      })?;

    let stdin = child.stdin.take().ok_or(LspError::MissingPipe("stdin"))?;
    let stdout = child.stdout.take().ok_or(LspError::MissingPipe("stdout"))?;

    let writer = Arc::new(Mutex::new(BufWriter::new(stdin)));
    let pending: Arc<Mutex<PendingMap>> = Arc::new(Mutex::new(HashMap::new()));
    let ready = Arc::new((Mutex::new(ReadyState::new()), Condvar::new()));

    let reader_writer = Arc::clone(&writer);
    let reader_pending = Arc::clone(&pending);
    let reader_ready = Arc::clone(&ready);

    let reader_thread = thread::Builder::new()
      .name("lsp-reader".to_string())
      .spawn(move || {
        reader_loop(
          BufReader::new(stdout),
          reader_writer,
          reader_pending,
          reader_ready,
        );
      })?;

    let client = Self {
      child: Mutex::new(child),
      writer,
      pending,
      ready,
      next_id: AtomicU64::new(1),
      shut_down: AtomicBool::new(false),
      reader_thread: Some(reader_thread),
    };

    let workspace_name = root_path
      .file_name()
      .and_then(|n| n.to_str())
      .unwrap_or("workspace");

    let init_params = json!({
      "processId": std::process::id(),
      "rootUri": root_uri.as_str(),
      "workspaceFolders": [{
        "uri": root_uri.as_str(),
        "name": workspace_name,
      }],
      "capabilities": {
        "window": {
          "workDoneProgress": true,
        },
        "textDocument": {
          "definition": { "linkSupport": true },
          "typeDefinition": { "linkSupport": true },
        },
        "experimental": {
          "serverStatusNotification": true,
        },
      },
    });

    let _: Value = client.request("initialize", init_params)?;
    client.notify("initialized", json!({}))?;

    Ok(client)
  }

  /// Blocks until the language server finishes initial indexing (e.g.
  /// `rust-analyzer` reports `quiescent: true`, or `$/progress` settles),
  /// or until `timeout` elapses.
  pub fn wait_until_ready(&self, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let quiet_window = Duration::from_millis(400);
    let initial_grace = Duration::from_millis(800);
    let started = Instant::now();

    let (lock, cvar) = &*self.ready;
    let mut state = lock
      .lock()
      .map_err(|_| LspError::Protocol("ready lock poisoned".to_string()))?;

    loop {
      if state.disconnected {
        return Err(LspError::Disconnected);
      }

      if state.quiescent == Some(true) {
        return Ok(());
      }

      if state.quiescent.is_none() && state.active_progress.is_empty() {
        let idle = state.last_activity.elapsed();
        let ready_by_progress = state.saw_progress && idle >= quiet_window;
        let ready_by_grace = !state.saw_progress && started.elapsed() >= initial_grace;
        if ready_by_progress || ready_by_grace {
          return Ok(());
        }
      }

      let now = Instant::now();
      if now >= deadline {
        return Err(LspError::Timeout);
      }

      let remaining = deadline.saturating_duration_since(now);
      let wait_step = remaining.min(Duration::from_millis(100));
      let (guard, _) = cvar
        .wait_timeout(state, wait_step)
        .map_err(|_| LspError::Protocol("ready condvar poisoned".to_string()))?;
      state = guard;
    }
  }

  /// Reads a Rust source file from disk and sends `textDocument/didOpen`.
  pub fn open_file(&self, path: impl AsRef<Path>) -> Result<Url> {
    let canonical = canonicalize_or_keep(path.as_ref());
    let text = fs::read_to_string(&canonical)?;
    self.did_open(&canonical, &text)
  }

  /// Sends `textDocument/didOpen` with the provided document contents.
  pub fn did_open(&self, path: impl AsRef<Path>, text: &str) -> Result<Url> {
    let uri = path_to_uri(&canonicalize_or_keep(path.as_ref()))?;
    self.notify(
      "textDocument/didOpen",
      json!({
        "textDocument": {
          "uri": uri.as_str(),
          "languageId": "rust",
          "version": 1,
          "text": text,
        }
      }),
    )?;
    Ok(uri)
  }

  /// Sends `textDocument/definition` and normalizes the response to `Vec<Location>`.
  pub fn definition(&self, path: impl AsRef<Path>, position: Position) -> Result<Vec<Location>> {
    self.goto_request("textDocument/definition", path, position)
  }

  /// Sends `textDocument/typeDefinition` and normalizes the response to `Vec<Location>`.
  pub fn type_definition(
    &self,
    path: impl AsRef<Path>,
    position: Position,
  ) -> Result<Vec<Location>> {
    self.goto_request("textDocument/typeDefinition", path, position)
  }

  /// Sends an arbitrary LSP request and deserializes the JSON result.
  pub fn request<P: Serialize, R: DeserializeOwned>(&self, method: &str, params: P) -> Result<R> {
    let id = self.next_id.fetch_add(1, Ordering::Relaxed);
    let params_value = serde_json::to_value(params)?;
    let (tx, rx) = mpsc::channel();

    {
      let mut pending = self
        .pending
        .lock()
        .map_err(|_| LspError::Protocol("pending map lock poisoned".to_string()))?;
      pending.insert(id, tx);
    }

    let req = OutgoingRequest {
      jsonrpc: "2.0",
      id,
      method,
      params: params_value,
    };

    if let Err(err) = self.write_locked(&req) {
      if let Ok(mut pending) = self.pending.lock() {
        pending.remove(&id);
      }
      return Err(err);
    }

    let raw_value = rx.recv().map_err(|_| LspError::Disconnected)??;
    Ok(serde_json::from_value(raw_value)?)
  }

  /// Sends an arbitrary LSP notification (no response expected).
  pub fn notify<P: Serialize>(&self, method: &str, params: P) -> Result<()> {
    let params_value = serde_json::to_value(params)?;
    let notif = OutgoingNotification {
      jsonrpc: "2.0",
      method,
      params: params_value,
    };
    self.write_locked(&notif)
  }

  /// Gracefully shuts down the language server (`shutdown` request + `exit` notification).
  pub fn shutdown(mut self) -> Result<()> {
    let result = self.shutdown_inner();
    if let Some(handle) = self.reader_thread.take() {
      let _ = handle.join();
    }
    result
  }

  fn goto_request(
    &self,
    method: &str,
    path: impl AsRef<Path>,
    position: Position,
  ) -> Result<Vec<Location>> {
    let uri = path_to_uri(&canonicalize_or_keep(path.as_ref()))?;
    let response: Option<GotoDefinitionResponse> = self.request(
      method,
      json!({
        "textDocument": { "uri": uri.as_str() },
        "position": position,
      }),
    )?;
    Ok(
      response
        .map(GotoDefinitionResponse::into_locations)
        .unwrap_or_default(),
    )
  }

  fn write_locked<T: Serialize>(&self, msg: &T) -> Result<()> {
    let mut writer = self
      .writer
      .lock()
      .map_err(|_| LspError::Protocol("stdin writer lock poisoned".to_string()))?;
    write_message(&mut *writer, msg)
  }

  fn shutdown_inner(&self) -> Result<()> {
    if self.shut_down.swap(true, Ordering::SeqCst) {
      return Ok(());
    }

    let shutdown_res: Result<Value> = self.request("shutdown", Value::Null);
    let _ = self.notify("exit", Value::Null);

    if let Ok(mut child) = self.child.lock() {
      let deadline = Instant::now() + Duration::from_secs(2);
      loop {
        match child.try_wait() {
          Ok(Some(_)) => break,
          Ok(None) if Instant::now() < deadline => {
            thread::sleep(Duration::from_millis(20));
          }
          _ => {
            let _ = child.kill();
            let _ = child.wait();
            break;
          }
        }
      }
    }

    shutdown_res.map(|_| ())
  }
}

impl Drop for LspClient {
  fn drop(&mut self) {
    let _ = self.shutdown_inner();
    if let Some(handle) = self.reader_thread.take() {
      let _ = handle.join();
    }
  }
}

fn reader_loop(
  mut reader: BufReader<std::process::ChildStdout>,
  writer: Arc<Mutex<BufWriter<ChildStdin>>>,
  pending: Arc<Mutex<PendingMap>>,
  ready: Arc<(Mutex<ReadyState>, Condvar)>,
) {
  while let Ok(Some(msg)) = read_message(&mut reader) {
    match msg {
      IncomingMessage::Response { id, result, error } => {
        if let RequestId::Number(num_id) = id {
          let sender = pending.lock().ok().and_then(|mut map| map.remove(&num_id));
          if let Some(tx) = sender {
            let outcome = match error {
              Some(err) => Err(LspError::Server {
                code: err.code,
                message: err.message,
                data: err.data,
              }),
              None => Ok(result.unwrap_or(Value::Null)),
            };
            let _ = tx.send(outcome);
          }
        }
      }
      IncomingMessage::Request { id, method, params } => {
        let reply_value = match method.as_str() {
          "workspace/configuration" => {
            let count = params
              .get("items")
              .and_then(Value::as_array)
              .map_or(0, Vec::len);
            Value::Array(vec![Value::Null; count])
          }
          _ => Value::Null,
        };
        let response = OutgoingResponse {
          jsonrpc: "2.0",
          id,
          result: Some(reply_value),
          error: None,
        };
        if let Ok(mut w) = writer.lock() {
          let _ = write_message(&mut *w, &response);
        }
      }
      IncomingMessage::Notification { method, params } => {
        handle_notification(&method, &params, &ready);
      }
    }
  }

  // Server closed stdout: wake all pending callers and ready waiters.
  if let Ok(mut map) = pending.lock() {
    for (_, tx) in map.drain() {
      let _ = tx.send(Err(LspError::Disconnected));
    }
  }

  let (lock, cvar) = &*ready;
  if let Ok(mut state) = lock.lock() {
    state.disconnected = true;
    cvar.notify_all();
  }
}

fn handle_notification(method: &str, params: &Value, ready: &(Mutex<ReadyState>, Condvar)) {
  let (lock, cvar) = ready;
  match method {
    "experimental/serverStatus" => {
      if let Ok(status) = serde_json::from_value::<ServerStatusNotification>(params.clone())
        && let Ok(mut state) = lock.lock()
      {
        state.quiescent = Some(status.quiescent);
        state.last_activity = Instant::now();
        cvar.notify_all();
      }
    }
    "$/progress" => {
      let token = params
        .get("token")
        .map(|t| match t {
          Value::String(s) => s.clone(),
          other => other.to_string(),
        })
        .unwrap_or_default();
      let kind = params
        .get("value")
        .and_then(|v| v.get("kind"))
        .and_then(Value::as_str);

      if let Ok(mut state) = lock.lock() {
        state.saw_progress = true;
        state.last_activity = Instant::now();
        match kind {
          Some("begin") => {
            state.active_progress.insert(token);
          }
          Some("end") => {
            state.active_progress.remove(&token);
          }
          _ => {}
        }
        cvar.notify_all();
      }
    }
    _ => {}
  }
}

fn canonicalize_or_keep(path: &Path) -> std::path::PathBuf {
  fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn path_to_uri(path: &Path) -> Result<Url> {
  Url::from_file_path(path).map_err(|()| LspError::InvalidPath(path.to_path_buf()))
}
