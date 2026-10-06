use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex as AsyncMutex, Notify, mpsc, oneshot},
    task::JoinHandle,
};

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(untagged)]
pub enum RequestId {
    Integer(i64),
    String(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum Error {
    #[error("transport I/O failed: {0}")]
    Io(String),
    #[error("invalid app-server message: {0}")]
    Protocol(String),
    #[error("app-server connection closed")]
    Closed,
    #[error("app-server event queue overflowed; reconnect and reconcile runtime state")]
    EventOverflow,
    #[error("request timed out; its runtime outcome is unknown")]
    Timeout,
    #[error("request was cancelled; its runtime outcome is unknown")]
    Cancelled,
    #[error("runtime method is unavailable on this connection: {0}")]
    Unavailable(String),
    #[error("server request is no longer pending")]
    NotPending,
    #[error("app-server error {0}: {1}")]
    Rpc(i64, String, Option<Value>),
}

/// Original envelopes are retained, including unknown fields and methods.
#[derive(Clone, Debug)]
pub struct ServerEvent {
    pub method: String,
    pub request_id: Option<RequestId>,
    pub raw: Value,
}

type Pending = HashMap<RequestId, oneshot::Sender<Result<Value, Error>>>;

struct State {
    next_id: i64,
    pending: Pending,
    closed: Option<Error>,
    closed_signal: Arc<Notify>,
}

#[derive(Clone)]
pub struct RpcClient {
    input: Arc<AsyncMutex<ChildStdin>>,
    state: Arc<Mutex<State>>,
}

struct CancellationGuard {
    state: Arc<Mutex<State>>,
    active: bool,
}

impl Drop for CancellationGuard {
    fn drop(&mut self) {
        if self.active {
            close(&self.state, Error::Cancelled);
        }
    }
}

impl RpcClient {
    pub(crate) fn fail(&self, error: Error) {
        close(&self.state, error);
    }

    pub(crate) fn check_open(&self) -> Result<(), Error> {
        match &self.state.lock().expect("runtime state poisoned").closed {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    pub async fn request(
        &self,
        method: &str,
        params: Value,
        deadline: Duration,
    ) -> Result<Value, Error> {
        self.request_optional(method, Some(params), deadline).await
    }

    pub(crate) async fn request_optional(
        &self,
        method: &str,
        params: Option<Value>,
        deadline: Duration,
    ) -> Result<Value, Error> {
        let (send, receive) = oneshot::channel();
        let id = {
            let mut state = self.state.lock().expect("runtime state poisoned");
            if let Some(error) = &state.closed {
                return Err(error.clone());
            }
            let id = RequestId::Integer(state.next_id);
            state.next_id = state
                .next_id
                .checked_add(1)
                .ok_or_else(|| Error::Protocol("request ID space exhausted".into()))?;
            state.pending.insert(id.clone(), send);
            id
        };
        let mut guard = CancellationGuard {
            state: self.state.clone(),
            active: true,
        };
        let operation = async {
            let mut message = json!({"id": id, "method": method});
            if let Some(params) = params {
                message["params"] = params;
            }
            self.write(message).await?;
            receive.await.map_err(|_| Error::Closed)?
        };
        let result = match tokio::time::timeout(deadline, operation).await {
            Ok(result) => result,
            Err(_) => {
                // A timed-out write could have left a partial JSONL frame.
                close(&self.state, Error::Timeout);
                Err(Error::Timeout)
            }
        };
        guard.active = false;
        self.state
            .lock()
            .expect("runtime state poisoned")
            .pending
            .remove(&id);
        result
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), Error> {
        self.write(json!({"method": method, "params": params}))
            .await
    }

    /// Reply using the original server ID; this layer never decides approvals.
    pub async fn respond(
        &self,
        id: RequestId,
        result: Result<Value, RpcError>,
    ) -> Result<(), Error> {
        let message = match result {
            Ok(value) => json!({"id": id, "result": value}),
            Err(error) => json!({"id": id, "error": error}),
        };
        self.write(message).await
    }

    async fn write(&self, message: Value) -> Result<(), Error> {
        let mut bytes =
            serde_json::to_vec(&message).map_err(|error| Error::Protocol(error.to_string()))?;
        bytes.push(b'\n');
        self.check_open()?;
        let mut guard = CancellationGuard {
            state: self.state.clone(),
            active: true,
        };
        let mut input = self.input.lock().await;
        if let Some(error) = &self.state.lock().expect("runtime state poisoned").closed {
            return Err(error.clone());
        }
        let result = async {
            input.write_all(&bytes).await?;
            input.flush().await
        }
        .await;
        guard.active = false;
        if let Err(error) = result {
            let error = Error::Io(error.to_string());
            close(&self.state, error.clone());
            return Err(error);
        }
        Ok(())
    }
}

pub struct AppServer {
    child: Child,
    client: RpcClient,
    events: mpsc::Receiver<ServerEvent>,
    reader: JoinHandle<()>,
}

impl AppServer {
    pub fn spawn(mut command: Command, event_capacity: usize) -> Result<Self, Error> {
        if event_capacity == 0 {
            return Err(Error::Protocol("event capacity must be positive".into()));
        }
        let mut child = command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| Error::Io(error.to_string()))?;
        let input = child.stdin.take().expect("piped stdin");
        let output = child.stdout.take().expect("piped stdout");
        let closed_signal = Arc::new(Notify::new());
        let state = Arc::new(Mutex::new(State {
            next_id: 1,
            pending: HashMap::new(),
            closed: None,
            closed_signal: closed_signal.clone(),
        }));
        let client = RpcClient {
            input: Arc::new(AsyncMutex::new(input)),
            state: state.clone(),
        };
        let (send, events) = mpsc::channel(event_capacity);
        let reader = tokio::spawn(async move {
            let mut lines = BufReader::new(output).lines();
            let outcome = async {
                loop {
                    let line = tokio::select! {
                        _ = closed_signal.notified() => break,
                        line = lines.next_line() => line.map_err(|e| Error::Io(e.to_string()))?,
                    };
                    let Some(line) = line else { break };
                    let raw: Value = serde_json::from_str(&line)
                        .map_err(|_| Error::Protocol("invalid JSON".into()))?;
                    route(raw, &state, &send).await?;
                }
                Err::<(), Error>(Error::Closed)
            }
            .await;
            close(&state, outcome.unwrap_err());
        });
        Ok(Self {
            child,
            client,
            events,
            reader,
        })
    }

    pub fn client(&self) -> RpcClient {
        self.client.clone()
    }

    pub async fn next_event(&mut self) -> Option<ServerEvent> {
        self.events.recv().await
    }

    pub(crate) fn take_events(&mut self) -> mpsc::Receiver<ServerEvent> {
        let (_, replacement) = mpsc::channel(1);
        std::mem::replace(&mut self.events, replacement)
    }

    /// Terminates only this explicitly owned child, not a persistent shared Host.
    pub async fn shutdown(&mut self) -> Result<(), Error> {
        close(&self.client.state, Error::Closed);
        self.reader.abort();
        self.child
            .kill()
            .await
            .map_err(|e| Error::Io(e.to_string()))
    }
}

impl Drop for AppServer {
    fn drop(&mut self) {
        self.reader.abort();
        close(&self.client.state, Error::Closed);
    }
}

fn close(state: &Mutex<State>, reason: Error) {
    let mut state = state.lock().expect("runtime state poisoned");
    let reason = state.closed.get_or_insert(reason).clone();
    state.closed_signal.notify_one();
    for (_, request) in state.pending.drain() {
        let _ = request.send(Err(reason.clone()));
    }
}

async fn route(
    raw: Value,
    state: &Mutex<State>,
    events: &mpsc::Sender<ServerEvent>,
) -> Result<(), Error> {
    if !raw.is_object() {
        return Err(Error::Protocol("expected an object".into()));
    }
    let id = raw
        .get("id")
        .map(|id| serde_json::from_value::<RequestId>(id.clone()))
        .transpose()
        .map_err(|_| Error::Protocol("invalid request ID".into()))?;
    if let Some(method) = raw.get("method") {
        let method = method
            .as_str()
            .ok_or_else(|| Error::Protocol("invalid method".into()))?
            .to_owned();
        if raw.get("result").is_some() || raw.get("error").is_some() {
            return Err(Error::Protocol("event contains a response payload".into()));
        }
        events
            .try_send(ServerEvent {
                method,
                request_id: id,
                raw,
            })
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => Error::EventOverflow,
                mpsc::error::TrySendError::Closed(_) => Error::Closed,
            })?;
        return Ok(());
    }
    let id = id.ok_or_else(|| Error::Protocol("response has no ID".into()))?;
    let result = match (raw.get("result"), raw.get("error")) {
        (Some(value), None) => Ok(value.clone()),
        (None, Some(value)) => {
            let error: RpcError = serde_json::from_value(value.clone())
                .map_err(|_| Error::Protocol("invalid RPC error".into()))?;
            Err(Error::Rpc(error.code, error.message, error.data))
        }
        _ => {
            return Err(Error::Protocol(
                "expected exactly one of result or error".into(),
            ));
        }
    };
    if let Some(request) = state
        .lock()
        .expect("runtime state poisoned")
        .pending
        .remove(&id)
    {
        let _ = request.send(result);
    }
    Ok(())
}
