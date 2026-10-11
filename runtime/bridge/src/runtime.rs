use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::{AppServer, Error, RequestId, RpcClient, RpcError, ServerEvent, protocol_surface};

pub struct ClientOptions {
    pub name: String,
    pub title: Option<String>,
    pub version: String,
    /// All pinned initialize capabilities can be supplied, including extensions.
    /// Experimental methods are enabled only by an explicit boolean true.
    pub capabilities: Value,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            name: "caidex".into(),
            title: Some("CAIdex".into()),
            version: env!("CARGO_PKG_VERSION").into(),
            capabilities: json!({"experimentalApi": false}),
        }
    }
}

pub struct RuntimeInfo {
    /// Original initialize result; the server does not return a feature bitmap.
    pub initialize_result: Value,
    pub experimental_api_requested: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InteractionKind {
    CommandApproval,
    FileApproval,
    PermissionsApproval,
    UserInput,
    McpElicitation,
    ToolCall,
    AuthRefresh,
    Attestation,
    CurrentTime,
    LegacyApproval,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Interaction {
    pub id: RequestId,
    pub kind: InteractionKind,
    pub event: ServerEvent,
}

impl Interaction {
    pub fn thread_id(&self) -> Option<&str> {
        self.event
            .raw
            .pointer("/params/threadId")
            .or_else(|| self.event.raw.pointer("/params/conversationId"))
            .and_then(Value::as_str)
    }

    pub fn turn_id(&self) -> Option<&str> {
        self.event
            .raw
            .pointer("/params/turnId")
            .and_then(Value::as_str)
    }
}

#[derive(Clone, Debug)]
pub enum RuntimeEvent {
    Interaction(Interaction),
    Notification(ServerEvent),
}

/// Common approval decisions. Policy amendments use the explicit raw reply path.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalDecision {
    Accept,
    AcceptForSession,
    Decline,
    Cancel,
}

type PendingInteractions = Arc<Mutex<HashMap<RequestId, Interaction>>>;

#[derive(Clone)]
pub struct RuntimeClient {
    rpc: RpcClient,
    experimental_api: bool,
    pending: PendingInteractions,
}

impl RuntimeClient {
    /// Full pinned protocol access, including optional-param methods. The upstream
    /// validates payloads and field-level experimental gates. No actions are retried.
    pub async fn call(
        &self,
        method: &str,
        params: Option<Value>,
        deadline: Duration,
    ) -> Result<Value, Error> {
        self.rpc.check_open()?;
        let spec = protocol_surface()
            .client_requests
            .get(method)
            .ok_or_else(|| Error::Unavailable(method.into()))?;
        if method == "initialize" || (spec.experimental && !self.experimental_api) {
            return Err(Error::Unavailable(method.into()));
        }
        if spec.params_required && params.is_none() {
            return Err(Error::Protocol(format!("{method} requires params")));
        }
        self.rpc.request_optional(method, params, deadline).await
    }

    pub async fn start_thread(&self, options: Value, deadline: Duration) -> Result<Value, Error> {
        self.call("thread/start", Some(options), deadline).await
    }

    pub async fn resume_thread(
        &self,
        id: &str,
        options: Value,
        deadline: Duration,
    ) -> Result<Value, Error> {
        self.thread_options("thread/resume", id, options, deadline)
            .await
    }

    pub async fn fork_thread(
        &self,
        id: &str,
        options: Value,
        deadline: Duration,
    ) -> Result<Value, Error> {
        self.thread_options("thread/fork", id, options, deadline)
            .await
    }

    pub async fn read_thread(
        &self,
        id: &str,
        include_turns: bool,
        deadline: Duration,
    ) -> Result<Value, Error> {
        self.call(
            "thread/read",
            Some(json!({"threadId": id, "includeTurns": include_turns})),
            deadline,
        )
        .await
    }

    pub async fn start_turn(
        &self,
        thread: &str,
        input: Vec<Value>,
        options: Value,
        deadline: Duration,
    ) -> Result<Value, Error> {
        let mut params = object(options)?;
        params.insert("threadId".into(), thread.into());
        params.insert("input".into(), input.into());
        self.call("turn/start", Some(params.into()), deadline).await
    }

    pub async fn steer_turn(
        &self,
        thread: &str,
        expected_turn: &str,
        input: Vec<Value>,
        deadline: Duration,
    ) -> Result<Value, Error> {
        self.call(
            "turn/steer",
            Some(json!({"threadId": thread, "expectedTurnId": expected_turn, "input": input})),
            deadline,
        )
        .await
    }

    pub async fn interrupt_turn(
        &self,
        thread: &str,
        turn: &str,
        deadline: Duration,
    ) -> Result<Value, Error> {
        self.call(
            "turn/interrupt",
            Some(json!({"threadId": thread, "turnId": turn})),
            deadline,
        )
        .await
    }

    async fn thread_options(
        &self,
        method: &str,
        id: &str,
        options: Value,
        deadline: Duration,
    ) -> Result<Value, Error> {
        let mut params = object(options)?;
        params.insert("threadId".into(), id.into());
        self.call(method, Some(params.into()), deadline).await
    }

    /// Exact request IDs are the authority, never item IDs or approvalId aliases.
    /// Removing before writing allows only one local response attempt. Write failure
    /// has unknown outcome and must be reconciled by the future persistent Host.
    pub async fn reply(
        &self,
        id: &RequestId,
        response: Result<Value, RpcError>,
    ) -> Result<(), Error> {
        self.rpc.check_open()?;
        self.pending
            .lock()
            .expect("interaction state poisoned")
            .remove(id)
            .ok_or(Error::NotPending)?;
        self.rpc.respond(id.clone(), response).await
    }

    pub async fn decide_approval(
        &self,
        id: &RequestId,
        decision: ApprovalDecision,
    ) -> Result<(), Error> {
        let value = json!({"decision": decision});
        {
            let pending = self.pending.lock().expect("interaction state poisoned");
            let request = pending.get(id).ok_or(Error::NotPending)?;
            if !matches!(
                request.kind,
                InteractionKind::CommandApproval | InteractionKind::FileApproval
            ) {
                return Err(Error::Protocol(
                    "decision does not match this request kind".into(),
                ));
            }
            if let Some(allowed) = request
                .event
                .raw
                .pointer("/params/availableDecisions")
                .and_then(Value::as_array)
                && !allowed.contains(&value["decision"])
            {
                return Err(Error::Protocol(
                    "decision is not offered by this request".into(),
                ));
            }
        }
        self.reply(id, Ok(value)).await
    }

    pub async fn answer_questions(
        &self,
        id: &RequestId,
        answers: HashMap<String, Vec<String>>,
    ) -> Result<(), Error> {
        {
            let pending = self.pending.lock().expect("interaction state poisoned");
            let request = pending.get(id).ok_or(Error::NotPending)?;
            if request.kind != InteractionKind::UserInput {
                return Err(Error::Protocol(
                    "answers do not match this request kind".into(),
                ));
            }
            let questions = request
                .event
                .raw
                .pointer("/params/questions")
                .and_then(Value::as_array)
                .ok_or_else(|| Error::Protocol("user input request has no questions".into()))?;
            if answers
                .keys()
                .any(|id| !questions.iter().any(|q| q["id"].as_str() == Some(id)))
            {
                return Err(Error::Protocol(
                    "answer contains an unknown question ID".into(),
                ));
            }
        }
        let answers: serde_json::Map<String, Value> = answers
            .into_iter()
            .map(|(id, answers)| (id, json!({"answers": answers})))
            .collect();
        self.reply(id, Ok(json!({"answers": answers}))).await
    }

    pub fn pending_interactions(&self) -> Vec<Interaction> {
        self.pending
            .lock()
            .expect("interaction state poisoned")
            .values()
            .cloned()
            .collect()
    }
}

pub struct Runtime {
    server: AppServer,
    client: RuntimeClient,
    info: RuntimeInfo,
    events: mpsc::Receiver<RuntimeEvent>,
    dispatcher: JoinHandle<()>,
}

impl Runtime {
    /// Owns this app-server child. Persistent shared Host ownership is a separate layer.
    pub async fn connect(
        mut server: AppServer,
        options: ClientOptions,
        deadline: Duration,
        event_capacity: usize,
    ) -> Result<Self, Error> {
        if event_capacity == 0 {
            return Err(Error::Protocol("event capacity must be positive".into()));
        }
        if !options.capabilities.is_object() {
            return Err(Error::Protocol(
                "initialize capabilities must be an object".into(),
            ));
        }
        let experimental_api =
            options.capabilities.get("experimentalApi") == Some(&Value::Bool(true));
        let rpc = server.client();
        let initialize_result = rpc.request("initialize", json!({
            "clientInfo": {"name": options.name, "title": options.title, "version": options.version},
            "capabilities": options.capabilities
        }), deadline).await?;
        rpc.notify("initialized", json!({})).await?;
        let pending: PendingInteractions = Arc::new(Mutex::new(HashMap::new()));
        let client = RuntimeClient {
            rpc: rpc.clone(),
            experimental_api,
            pending: pending.clone(),
        };
        let mut incoming = server.take_events();
        let (send, events) = mpsc::channel(event_capacity);
        let dispatcher = tokio::spawn(async move {
            while let Some(event) = incoming.recv().await {
                let mapped = if let Some(id) = event.request_id.clone() {
                    let request = Interaction {
                        id: id.clone(),
                        kind: interaction_kind(&event.method),
                        event,
                    };
                    let duplicate = pending
                        .lock()
                        .expect("interaction state poisoned")
                        .insert(id, request.clone())
                        .is_some();
                    if duplicate {
                        rpc.fail(Error::Protocol(
                            "duplicate pending server request ID".into(),
                        ));
                        break;
                    }
                    RuntimeEvent::Interaction(request)
                } else {
                    if event.method == "serverRequest/resolved" {
                        let id = event
                            .raw
                            .pointer("/params/requestId")
                            .and_then(|id| serde_json::from_value::<RequestId>(id.clone()).ok());
                        let Some(id) = id else {
                            rpc.fail(Error::Protocol("resolved request has no valid ID".into()));
                            break;
                        };
                        pending
                            .lock()
                            .expect("interaction state poisoned")
                            .remove(&id);
                    }
                    if matches!(
                        event.method.as_str(),
                        "turn/completed" | "thread/closed" | "thread/deleted"
                    ) {
                        // Pinned upstream aborts per-thread callbacks on turn completion
                        // and shutdown without necessarily emitting request/resolved.
                        let Some(thread) = event
                            .raw
                            .pointer("/params/threadId")
                            .and_then(Value::as_str)
                        else {
                            rpc.fail(Error::Protocol(
                                "thread lifecycle notification has no thread ID".into(),
                            ));
                            break;
                        };
                        pending
                            .lock()
                            .expect("interaction state poisoned")
                            .retain(|_, request| request.thread_id() != Some(thread));
                    }
                    RuntimeEvent::Notification(event)
                };
                if let Err(error) = send.try_send(mapped) {
                    rpc.fail(match error {
                        mpsc::error::TrySendError::Full(_) => Error::EventOverflow,
                        mpsc::error::TrySendError::Closed(_) => Error::Closed,
                    });
                    break;
                }
            }
            pending.lock().expect("interaction state poisoned").clear();
        });
        Ok(Self {
            server,
            client,
            info: RuntimeInfo {
                initialize_result,
                experimental_api_requested: experimental_api,
            },
            events,
            dispatcher,
        })
    }

    pub fn client(&self) -> RuntimeClient {
        self.client.clone()
    }
    pub fn info(&self) -> &RuntimeInfo {
        &self.info
    }
    pub async fn next_event(&mut self) -> Option<RuntimeEvent> {
        self.events.recv().await
    }

    pub async fn shutdown(&mut self) -> Result<(), Error> {
        self.dispatcher.abort();
        self.client
            .pending
            .lock()
            .expect("interaction state poisoned")
            .clear();
        self.server.shutdown().await
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.dispatcher.abort();
        self.client
            .pending
            .lock()
            .expect("interaction state poisoned")
            .clear();
    }
}

fn object(value: Value) -> Result<serde_json::Map<String, Value>, Error> {
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(Error::Protocol("runtime options must be an object".into())),
    }
}

fn interaction_kind(method: &str) -> InteractionKind {
    match method {
        "item/commandExecution/requestApproval" => InteractionKind::CommandApproval,
        "item/fileChange/requestApproval" => InteractionKind::FileApproval,
        "item/permissions/requestApproval" => InteractionKind::PermissionsApproval,
        "item/tool/requestUserInput" => InteractionKind::UserInput,
        "mcpServer/elicitation/request" => InteractionKind::McpElicitation,
        "item/tool/call" => InteractionKind::ToolCall,
        "account/chatgptAuthTokens/refresh" => InteractionKind::AuthRefresh,
        "attestation/generate" => InteractionKind::Attestation,
        "currentTime/read" => InteractionKind::CurrentTime,
        "applyPatchApproval" | "execCommandApproval" => InteractionKind::LegacyApproval,
        _ => InteractionKind::Unknown,
    }
}
