use std::{collections::BTreeMap, time::Duration};

use caidex_runtime::{ApprovalDecision, Interaction, InteractionKind, RequestId, RuntimeClient};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{sync::mpsc, task::JoinSet};

use crate::{Error, Event, Journal, Result, random_id};

const DEADLINE: Duration = Duration::from_secs(15);
const MAX_TASKS: usize = 1024;
const MAX_OPERATIONS: usize = 4096;
const MAX_PENDING: usize = 32;
const TASK_LIFETIME: u64 = 300;
const APPROVAL_LIFETIME: u64 = 120;

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Submission {
    pub prompt: String,
    pub model: String,
    pub provider: String,
    #[serde(default)]
    pub parent_task_id: Option<String>,
    #[serde(default)]
    pub continue_thread: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub review: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Task {
    pub task_id: String,
    pub operation_id: String,
    pub submission: Submission,
    pub status: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub actual: Value,
    pub pending: BTreeMap<String, Value>,
    pub artifacts: BTreeMap<String, Value>,
    pub last_seq: i64,
    pub stream: i64,
    pub expires_at: u64,
}

impl Task {
    pub fn terminal(&self) -> bool {
        matches!(self.status.as_str(), "completed" | "failed" | "cancelled")
    }

    pub(crate) fn invalidate(&mut self) {
        if !self.terminal() {
            self.status = "unknown".into();
        }
        for pending in self.pending.values_mut() {
            if matches!(pending["status"].as_str(), Some("pending" | "sending")) {
                pending["status"] = json!("unavailable");
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Operation {
    pub operation_id: String,
    pub payload_hash: String,
    pub task_id: String,
    pub action: String,
    pub outcome: String,
    pub last_seq: i64,
}

/// Host-selected profiles only; submission cannot change execution policy or cwd.
#[derive(Clone)]
pub struct HostPolicy {
    pub models: BTreeMap<String, Vec<String>>,
}

impl HostPolicy {
    pub fn probe_only() -> Self {
        Self {
            models: BTreeMap::new(),
        }
    }

    pub fn offline() -> Self {
        Self {
            models: ["caidex_h2_a", "caidex_h2_b"]
                .into_iter()
                .map(|provider| (provider.into(), vec!["gpt-5.5".into(), "gpt-5.4".into()]))
                .collect(),
        }
    }
}

pub(crate) enum Command {
    Submit {
        operation_id: String,
        submission: Submission,
    },
    Status {
        task_id: Option<String>,
        operation_id: Option<String>,
    },
    List {
        after: Option<String>,
        limit: usize,
    },
    Cancel {
        task_id: String,
        operation_id: String,
    },
    Revoke {
        task_id: String,
        operation_id: String,
        request_id: RequestId,
    },
    Approval {
        task_id: String,
        operation_id: String,
        request_id: RequestId,
        decision: ApprovalDecision,
    },
}

#[derive(Clone, Copy)]
enum Stage {
    Thread,
    Turn,
    Cancel,
    Approval,
}

pub(crate) struct Completion {
    task_id: String,
    operation_id: String,
    stage: Stage,
    request_id: Option<RequestId>,
    result: std::result::Result<Value, caidex_runtime::Error>,
}

pub(crate) struct Tasks {
    pub jobs: JoinSet<()>,
    send: mpsc::Sender<Completion>,
    client: RuntimeClient,
    policy: HostPolicy,
    project: std::path::PathBuf,
    task_started: BTreeMap<String, tokio::time::Instant>,
    approval_started: BTreeMap<(String, String), tokio::time::Instant>,
    cancel_started: BTreeMap<String, tokio::time::Instant>,
}

impl Tasks {
    pub fn new(
        send: mpsc::Sender<Completion>,
        client: RuntimeClient,
        policy: HostPolicy,
        project: std::path::PathBuf,
    ) -> Self {
        Self {
            jobs: JoinSet::new(),
            send,
            client,
            policy,
            project,
            task_started: BTreeMap::new(),
            approval_started: BTreeMap::new(),
            cancel_started: BTreeMap::new(),
        }
    }

    fn task_expired(&self, task: &Task, timestamp: u64) -> bool {
        if task.status == "cancel-requested" {
            return task.expires_at <= timestamp
                || self
                    .cancel_started
                    .get(&task.task_id)
                    .is_some_and(|start| start.elapsed() >= DEADLINE);
        }
        task.expires_at <= timestamp
            || self
                .task_started
                .get(&task.task_id)
                .is_some_and(|start| start.elapsed() >= Duration::from_secs(TASK_LIFETIME))
    }

    fn request_expired(&self, task: &Task, key: &str, timestamp: u64) -> bool {
        task.pending[key]["expires_at"]
            .as_u64()
            .is_none_or(|expiry| expiry <= timestamp)
            || self
                .approval_started
                .get(&(task.task_id.clone(), key.into()))
                .is_some_and(|start| start.elapsed() >= Duration::from_secs(APPROVAL_LIFETIME))
    }

    fn launch(
        &mut self,
        task: &Task,
        operation_id: String,
        stage: Stage,
        request: Option<(RequestId, ApprovalDecision)>,
    ) {
        let task = task.clone();
        let client = self.client.clone();
        let send = self.send.clone();
        let project = self.project.clone();
        self.jobs.spawn(async move {
            let request_id = request.as_ref().map(|(id, _)| id.clone());
            let result = match stage {
                Stage::Thread => client.start_thread(json!({"cwd": project, "model": task.submission.model, "modelProvider": task.submission.provider, "approvalPolicy": "on-request", "approvalsReviewer": "user", "sandbox": "read-only"}), DEADLINE).await,
                Stage::Turn if task.submission.review => client.call("review/start", Some(json!({"threadId": task.thread_id, "target": {"type": "uncommittedChanges"}, "delivery": "inline"})), DEADLINE).await,
                Stage::Turn => client.start_turn(task.thread_id.as_deref().expect("confirmed thread"), vec![json!({"type": "text", "text": task.submission.prompt})], json!({"model": task.submission.model}), DEADLINE).await,
                Stage::Cancel => client.interrupt_turn(task.thread_id.as_deref().expect("known thread"), task.turn_id.as_deref().expect("known turn"), DEADLINE).await,
                Stage::Approval => {
                    let (id, decision) = request.expect("validated decision");
                    tokio::time::timeout(DEADLINE, client.decide_approval(&id, decision)).await.unwrap_or(Err(caidex_runtime::Error::Timeout)).map(|()| json!({"sent": true}))
                }
            };
            let _ = send.send(Completion { task_id: task.task_id, operation_id, stage, request_id, result }).await;
        });
    }

    fn operation(
        journal: &Journal,
        id: &str,
        task: &str,
        action: &str,
        payload: &Value,
    ) -> Result<std::result::Result<Operation, Value>> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        {
            return Err(Error::Refused("invalid operation ID"));
        }
        let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(payload)?));
        let snapshot = journal.snapshot();
        if let Some(previous) = snapshot.operations.get(id) {
            if previous.payload_hash != hash || previous.action != action {
                return Err(Error::Refused("operation ID payload conflict"));
            }
            return Ok(Err(
                json!({"operation": previous, "task": snapshot.tasks.get(&previous.task_id)}),
            ));
        }
        if snapshot.operations.len() >= MAX_OPERATIONS {
            return Err(Error::Refused("Host operation capacity reached"));
        }
        Ok(Ok(Operation {
            operation_id: id.into(),
            payload_hash: hash,
            task_id: task.into(),
            action: action.into(),
            outcome: "accepted".into(),
            last_seq: 0,
        }))
    }

    pub fn command(
        &mut self,
        journal: &mut Journal,
        events: &tokio::sync::broadcast::Sender<Event>,
        command: Command,
        client_id: &str,
    ) -> Result<Value> {
        let snapshot = journal.snapshot();
        match command {
            Command::Status { task_id, operation_id } => {
                match (task_id, operation_id) {
                    (Some(id), None) => snapshot.tasks.get(&id).map(|task| json!({"task": task})).ok_or(Error::Refused("task not found")),
                    (None, Some(id)) => snapshot.operations.get(&id).map(|operation| json!({"operation": operation, "task": snapshot.tasks.get(&operation.task_id)})).ok_or(Error::Refused("operation not found")),
                    _ => Err(Error::Refused("provide task ID or operation ID, exclusively")),
                }
            }
            Command::List { after, limit } => {
                if !(1..=100).contains(&limit) { return Err(Error::Refused("list limit outside 1..100")); }
                let values: Vec<_> = snapshot.tasks.values().filter(|task| after.as_ref().is_none_or(|id| task.task_id > *id)).take(limit + 1).collect();
                let next = (values.len() > limit).then(|| values[limit - 1].task_id.clone());
                Ok(json!({"tasks": values.iter().take(limit).collect::<Vec<_>>(), "next": next}))
            }
            Command::Submit { operation_id, submission } => {
                let payload = json!(submission);
                let id = random_id()?;
                let operation = match Self::operation(journal, &operation_id, &id, "submit", &payload)? { Ok(value) => value, Err(previous) => return Ok(previous) };
                if submission.prompt.trim().is_empty() || submission.prompt.len() > 32 * 1024 || !self.policy.models.get(&submission.provider).is_some_and(|models| models.contains(&submission.model)) {
                    return Err(Error::Refused("prompt/model/provider outside Host policy"));
                }
                if submission.review && submission.continue_thread { return Err(Error::Refused("review requires an explicit new thread")); }
                if snapshot.tasks.len() >= MAX_TASKS || snapshot.tasks.values().any(|task| !task.terminal() && task.status != "unknown") {
                    return Err(Error::Refused("Host task capacity/busy"));
                }
                let mut thread_id = None;
                let mut actual = Value::Null;
                if let Some(parent) = &submission.parent_task_id {
                    let parent = snapshot.tasks.get(parent).ok_or(Error::Refused("parent task not found"))?;
                    if !parent.terminal() { return Err(Error::Refused("model change requires confirmed turn boundary")); }
                    if submission.continue_thread {
                        if parent.submission.provider != submission.provider || parent.stream != snapshot.stream {
                            return Err(Error::Refused("continuation requires same provider and live stream; use explicit new thread"));
                        }
                        let id = parent.thread_id.clone().ok_or(Error::Refused("parent has no thread"))?;
                        if snapshot.threads.get(&id).is_none_or(|thread| thread["runtime_state"] != "loaded") { return Err(Error::Refused("parent thread is no longer loaded")); }
                        thread_id = Some(id);
                        actual = parent.actual.clone();
                    }
                } else if submission.continue_thread { return Err(Error::Refused("continuation requires parent task")); }
                let task = Task { task_id: id, operation_id: operation_id.clone(), submission, status: "submitted".into(), thread_id, turn_id: None, actual, pending: BTreeMap::new(), artifacts: BTreeMap::new(), last_seq: 0, stream: snapshot.stream, expires_at: now()?.checked_add(TASK_LIFETIME).ok_or(Error::Refused("task deadline exhausted"))? };
                persist(journal, events, &task, Some(&operation))?;
                self.task_started.insert(task.task_id.clone(), tokio::time::Instant::now());
                self.launch(&task, operation_id.clone(), if task.submission.continue_thread { Stage::Turn } else { Stage::Thread }, None);
                Ok(json!({"operation": journal.snapshot().operations[&operation_id], "task": journal.snapshot().tasks[&task.task_id], "accepted": true}))
            }
            Command::Cancel { task_id, operation_id } => {
                let operation = match Self::operation(journal, &operation_id, &task_id, "cancel", &json!({"task_id": task_id}))? { Ok(value) => value, Err(previous) => return Ok(previous) };
                let mut task = snapshot.tasks.get(&task_id).cloned().ok_or(Error::Refused("task not found"))?;
                let mut operation = operation;
                let first_cancel = !task.terminal() && !matches!(task.status.as_str(), "unknown" | "cancel-requested");
                if task.terminal() { operation.outcome = "confirmed".into(); }
                else if task.status == "unknown" { operation.outcome = "unknown".into(); }
                else if first_cancel {
                    task.status = "cancel-requested".into();
                    task.expires_at = now()?.checked_add(DEADLINE.as_secs()).ok_or(Error::Refused("cancel deadline exhausted"))?;
                }
                for pending in task.pending.values_mut() { if matches!(pending["status"].as_str(), Some("pending" | "sending")) { pending["status"] = json!("unavailable"); } }
                persist(journal, events, &task, Some(&operation))?;
                if first_cancel { self.cancel_started.insert(task_id.clone(), tokio::time::Instant::now()); }
                self.after_event(journal, events)?;
                Ok(json!({"operation": journal.snapshot().operations[&operation_id], "task": journal.snapshot().tasks[&task_id]}))
            }
            Command::Revoke { task_id, operation_id, request_id } => {
                let operation = match Self::operation(journal, &operation_id, &task_id, "approval-revoke", &json!({"task_id": task_id, "request_id": request_id}))? { Ok(value) => value, Err(previous) => return Ok(previous) };
                let mut task = snapshot.tasks.get(&task_id).cloned().ok_or(Error::Refused("task not found"))?;
                let key = serde_json::to_string(&request_id)?;
                let request = task.pending.get(&key).ok_or(Error::Refused("request not found"))?;
                if task.stream != snapshot.stream || task.status != "blocked" || request["status"] != "pending" { return Err(Error::Refused("request handled or unavailable")); }
                let timestamp = now()?;
                if self.task_expired(&task, timestamp) || self.request_expired(&task, &key, timestamp) { return Err(Error::Refused("request expired")); }
                let request = task.pending.get_mut(&key).expect("checked request");
                request["status"] = json!("revoked");
                request["client_id"] = json!(client_id);
                request["operation_id"] = json!(operation_id);
                let mut operation = operation;
                operation.outcome = "revoked".into();
                persist(journal, events, &task, Some(&operation))?;
                self.command(journal, events, Command::Cancel { task_id: task_id.clone(), operation_id: format!("host-revoke-{:x}", Sha256::digest(operation_id.as_bytes())) }, crate::access::OWNER)?;
                Ok(json!({"operation": journal.snapshot().operations[&operation_id], "task": journal.snapshot().tasks[&task_id]}))
            }
            Command::Approval { task_id, operation_id, request_id, decision } => {
                let key = serde_json::to_string(&request_id)?;
                let payload = json!({"task_id": task_id, "request_id": request_id, "decision": decision});
                let operation = match Self::operation(journal, &operation_id, &task_id, "approval", &payload)? { Ok(value) => value, Err(previous) => return Ok(previous) };
                let mut task = snapshot.tasks.get(&task_id).cloned().ok_or(Error::Refused("task not found"))?;
                if task.stream != snapshot.stream || task.status != "blocked" { return Err(Error::Refused("approval is unavailable")); }
                let request = task.pending.get(&key).ok_or(Error::Refused("request not found"))?;
                if request["status"] != "pending" { return Err(Error::Refused("request handled or unavailable")); }
                let timestamp = now()?;
                if self.task_expired(&task, timestamp) || self.request_expired(&task, &key, timestamp) { return Err(Error::Refused("request expired")); }
                let request = task.pending.get_mut(&key).expect("checked request");
                if let Some(allowed) = request.pointer("/raw/params/availableDecisions").and_then(Value::as_array) && !allowed.contains(&json!(decision)) { return Err(Error::Refused("decision not offered by Runtime")); }
                request["status"] = json!("sending");
                request["client_id"] = json!(client_id);
                request["operation_id"] = json!(operation_id);
                request["decision"] = json!(decision);
                let mut operation = operation;
                operation.outcome = "sending".into();
                persist(journal, events, &task, Some(&operation))?;
                self.launch(&task, operation_id.clone(), Stage::Approval, Some((request_id, decision)));
                Ok(json!({"operation": journal.snapshot().operations[&operation_id], "task": journal.snapshot().tasks[&task_id]}))
            }
        }
    }

    pub fn interaction(
        &mut self,
        journal: &mut Journal,
        events: &tokio::sync::broadcast::Sender<Event>,
        request: Interaction,
    ) -> Result<()> {
        if request.thread_id().is_none() || request.turn_id().is_none() {
            return Err(Error::Refused(
                "interaction requires explicit thread and turn IDs",
            ));
        }
        let snapshot = journal.snapshot();
        let mut task = snapshot
            .tasks
            .values()
            .find(|task| {
                !task.terminal()
                    && task.status != "unknown"
                    && task.thread_id.as_deref() == request.thread_id()
                    && task.turn_id.as_deref() == request.turn_id()
            })
            .cloned()
            .ok_or(Error::Refused("interaction has no live task/turn"))?;
        if !matches!(
            request.kind,
            InteractionKind::CommandApproval | InteractionKind::FileApproval
        ) || task.pending.len() >= MAX_PENDING
        {
            return Err(Error::Refused(
                "interaction unsupported or pending capacity reached; no automatic answer",
            ));
        }
        let key = serde_json::to_string(&request.id)?;
        if task.pending.contains_key(&key) {
            return Err(Error::Refused("duplicate Runtime request ID"));
        }
        task.pending.insert(
            key.clone(),
            json!({"request_id": request.id, "status": if task.status == "cancel-requested" { "unavailable" } else { "pending" }, "raw": request.event.raw, "stream": task.stream, "thread_id": task.thread_id, "turn_id": task.turn_id, "expires_at": task.expires_at.min(now()?.checked_add(APPROVAL_LIFETIME).ok_or(Error::Refused("approval deadline exhausted"))?)}),
        );
        if task.status != "cancel-requested" {
            task.status = "blocked".into();
        }
        persist(journal, events, &task, None)?;
        self.approval_started
            .insert((task.task_id.clone(), key), tokio::time::Instant::now());
        Ok(())
    }

    pub fn complete(
        &mut self,
        journal: &mut Journal,
        events: &tokio::sync::broadcast::Sender<Event>,
        completion: Completion,
    ) -> Result<()> {
        let snapshot = journal.snapshot();
        let mut task = snapshot
            .tasks
            .get(&completion.task_id)
            .cloned()
            .ok_or(Error::Refused("completion has no task"))?;
        let mut operation = snapshot.operations[&completion.operation_id].clone();
        match completion.result {
            Ok(value) => match completion.stage {
                Stage::Thread => {
                    let thread = value
                        .pointer("/thread/id")
                        .and_then(Value::as_str)
                        .ok_or(Error::Refused("Runtime returned no thread ID"))?;
                    task.thread_id = Some(thread.into());
                    task.actual = json!({"model": value["model"], "provider": value["modelProvider"], "policy": value["approvalPolicy"], "reviewer": value["approvalsReviewer"], "sandbox": value["sandbox"], "cwd": value["cwd"]});
                    persist(journal, events, &task, None)?;
                    if value["approvalPolicy"] != "on-request"
                        || value["approvalsReviewer"] != "user"
                        || value.pointer("/sandbox/type").and_then(Value::as_str)
                            != Some("readOnly")
                        || value["model"] != task.submission.model
                        || value["modelProvider"] != task.submission.provider
                    {
                        return Err(Error::Refused(
                            "Runtime actual configuration violates Host policy",
                        ));
                    }
                    if task.status == "cancel-requested" {
                        task.status = "cancelled".into();
                        persist(journal, events, &task, None)?;
                        self.after_event(journal, events)?;
                    } else {
                        self.launch(&task, task.operation_id.clone(), Stage::Turn, None);
                    }
                    return Ok(());
                }
                Stage::Turn => {
                    let turn = value
                        .pointer("/turn/id")
                        .and_then(Value::as_str)
                        .ok_or(Error::Refused("Runtime returned no turn ID"))?;
                    if let Some(known) = &task.turn_id
                        && known != turn
                    {
                        return Err(Error::Refused("Runtime turn identity mismatch"));
                    }
                    task.turn_id = Some(turn.into());
                    task.actual["turn_model_request"] = json!(task.submission.model);
                    if task.status == "submitted" {
                        task.status = "running".into();
                    }
                }
                Stage::Cancel => {
                    operation.outcome = if task.terminal() {
                        "confirmed"
                    } else {
                        "requested"
                    }
                    .into();
                }
                Stage::Approval => {
                    operation.outcome = "sent".into();
                    let key = serde_json::to_string(
                        &completion.request_id.expect("approval completion request"),
                    )?;
                    if let Some(pending) = task.pending.get_mut(&key)
                        && pending["status"] == "sending"
                    {
                        pending["status"] = json!("sent");
                    }
                    if task.status == "blocked"
                        && !task.pending.values().any(|pending| {
                            matches!(pending["status"].as_str(), Some("pending" | "sending"))
                        })
                    {
                        task.status = "running".into();
                    }
                }
            },
            Err(caidex_runtime::Error::NotPending)
                if matches!(completion.stage, Stage::Approval) =>
            {
                operation.outcome = "invalid".into();
                if let Some(request) = task.pending.get_mut(&serde_json::to_string(
                    &completion.request_id.expect("approval request"),
                )?) && request["status"] == "sending"
                {
                    request["status"] = json!("unavailable");
                }
            }
            Err(caidex_runtime::Error::Rpc(_, _, _)) => {
                operation.outcome = "rejected".into();
                if matches!(completion.stage, Stage::Thread | Stage::Turn) && !task.terminal() {
                    task.status = "failed".into();
                }
            }
            Err(_) => {
                operation.outcome = "unknown".into();
                task.invalidate();
                persist(journal, events, &task, Some(&operation))?;
                return Err(Error::Refused(
                    "Runtime action outcome unknown; never automatically retry",
                ));
            }
        }
        persist(journal, events, &task, Some(&operation))?;
        self.after_event(journal, events)?;
        Ok(())
    }

    pub fn expire(
        &mut self,
        journal: &mut Journal,
        events: &tokio::sync::broadcast::Sender<Event>,
        timestamp: u64,
    ) -> Result<()> {
        let snapshot = journal.snapshot();
        let due: Vec<_> = snapshot
            .tasks
            .values()
            .filter(|task| {
                !task.terminal()
                    && task.status != "unknown"
                    && (self.task_expired(task, timestamp)
                        || task.pending.iter().any(|(key, request)| {
                            request["status"] == "pending"
                                && self.request_expired(task, key, timestamp)
                        }))
            })
            .cloned()
            .collect();
        for mut task in due {
            if task.status == "cancel-requested" {
                task.invalidate();
                persist(journal, events, &task, None)?;
                return Err(Error::Refused(
                    "cancel terminal confirmation expired; result unknown, stop Runtime",
                ));
            }
            let expired: Vec<_> = task
                .pending
                .iter()
                .filter(|(key, request)| {
                    request["status"] == "pending" && self.request_expired(&task, key, timestamp)
                })
                .map(|(key, _)| key.clone())
                .collect();
            for key in expired {
                task.pending.get_mut(&key).expect("known request")["status"] = json!("expired");
            }
            persist(journal, events, &task, None)?;
            self.command(
                journal,
                events,
                Command::Cancel {
                    task_id: task.task_id.clone(),
                    operation_id: format!("host-expiry-{}", task.task_id),
                },
                crate::access::OWNER,
            )?;
        }
        Ok(())
    }

    pub fn after_event(
        &mut self,
        journal: &mut Journal,
        events: &tokio::sync::broadcast::Sender<Event>,
    ) -> Result<()> {
        let snapshot = journal.snapshot();
        self.cancel_started.retain(|id, _| {
            snapshot
                .tasks
                .get(id)
                .is_some_and(|task| task.status == "cancel-requested")
        });
        self.task_started.retain(|id, _| {
            snapshot
                .tasks
                .get(id)
                .is_some_and(|task| !task.terminal() && task.status != "unknown")
        });
        self.approval_started.retain(|(id, key), _| {
            snapshot.tasks.get(id).is_some_and(|task| {
                task.pending
                    .get(key)
                    .is_some_and(|request| request["status"] == "pending")
            })
        });
        for task in snapshot
            .tasks
            .values()
            .filter(|task| !task.terminal() && task.status != "unknown" && !task.actual.is_null())
        {
            if task.actual["policy"] != "on-request"
                || task.actual["reviewer"] != "user"
                || task.actual.pointer("/sandbox/type").and_then(Value::as_str) != Some("readOnly")
                || task.actual["provider"] != task.submission.provider
            {
                return Err(Error::Refused(
                    "Runtime changed execution policy; no further actions",
                ));
            }
        }
        for mut operation in snapshot
            .operations
            .values()
            .filter(|operation| operation.action == "cancel" && operation.outcome == "accepted")
            .cloned()
        {
            let task = &snapshot.tasks[&operation.task_id];
            if task.status == "unknown" {
                operation.outcome = "unknown".into();
                persist(journal, events, task, Some(&operation))?;
            } else if task.terminal() {
                operation.outcome = "confirmed".into();
                persist(journal, events, task, Some(&operation))?;
            } else if task.turn_id.is_some() {
                operation.outcome = "sending".into();
                persist(journal, events, task, Some(&operation))?;
                self.launch(task, operation.operation_id, Stage::Cancel, None);
            }
        }
        Ok(())
    }
}

fn persist(
    journal: &mut Journal,
    events: &tokio::sync::broadcast::Sender<Event>,
    task: &Task,
    operation: Option<&Operation>,
) -> Result<()> {
    let event = journal.append("host/task", json!({"task": task, "operation": operation}))?;
    let _ = events.send(event);
    Ok(())
}

pub(crate) fn now() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .map_err(|_| Error::Refused("Host clock before epoch"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reply_completion_is_per_request_and_deadline_interrupts_once_without_approval() {
        let directory = std::env::temp_dir().join(format!(
            "caidex-h2-expiry-{}-{}",
            std::process::id(),
            random_id().unwrap()
        ));
        let mut journal = Journal::open(&directory).unwrap();
        journal.append("host/started", json!({})).unwrap();
        let python = std::env::var_os("CAIDEX_TEST_PYTHON")
            .unwrap_or_else(|| if cfg!(windows) { "python" } else { "python3" }.into());
        let mut command = tokio::process::Command::new(python);
        command
            .arg(
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/peer.py"),
            )
            .arg("task-normal")
            .arg(directory.join("marker"));
        let mut runtime = caidex_runtime::Runtime::connect(
            caidex_runtime::AppServer::spawn(command, 32).unwrap(),
            caidex_runtime::ClientOptions::default(),
            DEADLINE,
            32,
        )
        .await
        .unwrap();
        let (send, mut completions) = mpsc::channel(4);
        let (events, _) = tokio::sync::broadcast::channel(4);
        let mut tasks = Tasks::new(
            send,
            runtime.client(),
            HostPolicy::offline(),
            directory.clone(),
        );
        let task = Task {
            task_id: "task".into(),
            operation_id: "submit".into(),
            submission: Submission {
                prompt: "fixture".into(),
                model: "gpt-5.5".into(),
                provider: "caidex_h2_a".into(),
                parent_task_id: None,
                continue_thread: false,
                review: false,
            },
            status: "blocked".into(),
            thread_id: Some("task-thread".into()),
            turn_id: Some("task-turn".into()),
            actual: Value::Null,
            pending: [("77".into(), json!({"status": "pending", "expires_at": 100}))].into(),
            artifacts: BTreeMap::new(),
            last_seq: 0,
            stream: 1,
            expires_at: 100,
        };
        let operation = Operation {
            operation_id: "submit".into(),
            payload_hash: "hash".into(),
            task_id: "task".into(),
            action: "submit".into(),
            outcome: "accepted".into(),
            last_seq: 0,
        };
        persist(&mut journal, &events, &task, Some(&operation)).unwrap();
        let mut concurrent = task.clone();
        concurrent.pending = [
            ("77".into(), json!({"status": "sending"})),
            ("78".into(), json!({"status": "sending"})),
        ]
        .into();
        let mut approval = operation.clone();
        approval.operation_id = "approval".into();
        approval.action = "approval".into();
        persist(&mut journal, &events, &concurrent, Some(&approval)).unwrap();
        tasks
            .complete(
                &mut journal,
                &events,
                Completion {
                    task_id: "task".into(),
                    operation_id: "approval".into(),
                    stage: Stage::Approval,
                    request_id: Some(RequestId::Integer(77)),
                    result: Ok(json!({"sent": true})),
                },
            )
            .unwrap();
        assert_eq!(
            journal.snapshot().tasks["task"].pending["77"]["status"],
            "sent"
        );
        assert_eq!(
            journal.snapshot().tasks["task"].pending["78"]["status"],
            "sending"
        );
        assert_eq!(journal.snapshot().tasks["task"].status, "blocked");
        persist(&mut journal, &events, &task, None).unwrap();
        tasks.expire(&mut journal, &events, 99).unwrap();
        assert_eq!(journal.snapshot().tasks["task"].status, "blocked");
        tasks.expire(&mut journal, &events, 100).unwrap();
        tasks.expire(&mut journal, &events, 101).unwrap();
        let completion = tokio::time::timeout(DEADLINE, completions.recv())
            .await
            .unwrap()
            .unwrap();
        tasks.complete(&mut journal, &events, completion).unwrap();
        assert_eq!(journal.snapshot().tasks["task"].status, "cancel-requested");
        assert_eq!(
            journal.snapshot().tasks["task"].pending["77"]["status"],
            "expired"
        );
        assert_eq!(
            std::fs::read_to_string(directory.join("marker")).unwrap(),
            "turn/interrupt\n"
        );
        let mut timed = task.clone();
        timed.task_id = "monotonic".into();
        timed.operation_id = "monotonic-submit".into();
        timed.expires_at = now().unwrap() + 300;
        timed.pending.get_mut("77").unwrap()["expires_at"] = json!(now().unwrap() + 120);
        let mut op = operation.clone();
        op.task_id = timed.task_id.clone();
        op.operation_id = timed.operation_id.clone();
        persist(&mut journal, &events, &timed, Some(&op)).unwrap();
        tasks.approval_started.insert(
            (timed.task_id.clone(), "77".into()),
            tokio::time::Instant::now() - Duration::from_secs(APPROVAL_LIFETIME),
        );
        let seq = journal.snapshot().seq;
        assert!(
            tasks
                .command(
                    &mut journal,
                    &events,
                    Command::Approval {
                        task_id: timed.task_id.clone(),
                        operation_id: "too-late".into(),
                        request_id: RequestId::Integer(77),
                        decision: ApprovalDecision::Accept
                    },
                    crate::access::OWNER
                )
                .is_err()
        );
        assert_eq!(
            journal.snapshot().seq,
            seq,
            "expired claim cannot persist or launch"
        );
        tasks
            .expire(&mut journal, &events, now().unwrap() - 60)
            .unwrap();
        assert_eq!(
            journal.snapshot().tasks["monotonic"].pending["77"]["status"],
            "expired"
        );
        let completion = tokio::time::timeout(DEADLINE, completions.recv())
            .await
            .unwrap()
            .unwrap();
        tasks.complete(&mut journal, &events, completion).unwrap();
        assert_eq!(
            std::fs::read_to_string(directory.join("marker")).unwrap(),
            "turn/interrupt\nturn/interrupt\n"
        );
        timed.task_id = "monotonic-task".into();
        timed.operation_id = "monotonic-task-submit".into();
        timed.pending.clear();
        timed.status = "running".into();
        op.task_id = timed.task_id.clone();
        op.operation_id = timed.operation_id.clone();
        persist(&mut journal, &events, &timed, Some(&op)).unwrap();
        tasks.task_started.insert(
            timed.task_id.clone(),
            tokio::time::Instant::now() - Duration::from_secs(TASK_LIFETIME),
        );
        tasks
            .expire(&mut journal, &events, now().unwrap() - 60)
            .unwrap();
        assert_eq!(
            journal.snapshot().tasks["monotonic-task"].status,
            "cancel-requested"
        );
        let completion = tokio::time::timeout(DEADLINE, completions.recv())
            .await
            .unwrap()
            .unwrap();
        tasks.complete(&mut journal, &events, completion).unwrap();
        assert_eq!(
            std::fs::read_to_string(directory.join("marker")).unwrap(),
            "turn/interrupt\nturn/interrupt\nturn/interrupt\n"
        );
        assert!(
            tasks
                .expire(
                    &mut journal,
                    &events,
                    now().unwrap() + DEADLINE.as_secs() + 1
                )
                .is_err(),
            "acknowledged cancel without terminal must expire, not remain busy forever"
        );
        assert!(
            journal
                .snapshot()
                .tasks
                .values()
                .any(|task| task.status == "unknown")
        );
        let deadline = journal.snapshot().tasks["task"].expires_at;
        let started = tasks.cancel_started["task"];
        tasks
            .command(
                &mut journal,
                &events,
                Command::Cancel {
                    task_id: "task".into(),
                    operation_id: "repeat-cancel".into(),
                },
                crate::access::OWNER,
            )
            .unwrap();
        assert_eq!(
            journal.snapshot().tasks["task"].expires_at,
            deadline,
            "new cancel operation cannot extend the bound"
        );
        assert_eq!(tasks.cancel_started["task"], started);
        let completion = tokio::time::timeout(DEADLINE, completions.recv())
            .await
            .unwrap()
            .unwrap();
        tasks.complete(&mut journal, &events, completion).unwrap();
        tasks
            .cancel_started
            .insert("task".into(), tokio::time::Instant::now() - DEADLINE);
        assert!(
            tasks
                .expire(&mut journal, &events, now().unwrap() - 60)
                .is_err(),
            "UTC rollback cannot extend cancel confirmation"
        );
        assert_eq!(journal.snapshot().tasks["task"].status, "unknown");
        assert_eq!(
            std::fs::read_to_string(directory.join("marker")).unwrap(),
            "turn/interrupt\nturn/interrupt\nturn/interrupt\nturn/interrupt\n",
            "no automatic cancel resend or approval after timeout"
        );
        runtime.shutdown().await.unwrap();
        drop(tasks);
        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
