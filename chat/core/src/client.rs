use crate::{
    Cache, Content, Conversation, End, Error, MAX_TEXT, Message, Namespace, OutboxEntry, Result,
};
use caidex_auth::ConfirmedSession;
use caidex_cloud_core::Id;
use caidex_model_core::{
    CancellationToken, CanonicalRequest, CanonicalResponse, CredentialRequirement, ModelProvider,
    ProviderStreamEvent, RequestContext, ResponsesDialect, StreamState,
};
use futures_util::StreamExt;
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

/// The adapter is configured locally with this partition's Broker, never via Host RPC.
#[derive(Clone)]
pub struct LocalModel {
    pub provider: Arc<dyn ModelProvider>,
    pub model: String,
}
pub struct Send {
    pub operation: Uuid,
    pub conversation: Uuid,
    pub parent: Option<Uuid>,
    pub content: Content,
}
pub(crate) struct State {
    pub(crate) cache: Cache,
    pub(crate) model: Option<LocalModel>,
    pub(crate) generation: u64,
    pub(crate) memory: crate::memory::Session,
    pub(crate) sync_session: Option<Arc<ConfirmedSession>>,
    pending: BTreeMap<Uuid, CancellationToken>,
}
/// Shared client state makes cancellation/account changes invalidate late replies.
pub struct Chat {
    root: PathBuf,
    state: Mutex<State>,
}
impl Chat {
    pub fn open(root: &Path, namespace: Namespace) -> Result<Self> {
        let memory = crate::memory::Session::for_namespace(&namespace);
        Ok(Self {
            root: root.to_owned(),
            state: Mutex::new(State {
                cache: Cache::open(root, namespace)?,
                model: None,
                generation: 0,
                sync_session: None,
                pending: BTreeMap::new(),
                memory,
            }),
        })
    }
    pub(crate) fn state(&self) -> Result<std::sync::MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| Error::Storage)
    }
    pub fn configure(&self, model: LocalModel) -> Result<()> {
        let mut state = self.state()?;
        validate_model(&model, state.cache.namespace())?;
        if !state.pending.is_empty() {
            return Err(Error::Invalid);
        }
        if let Some(guard) = model.provider.memory_guard(&model.model) {
            state.memory.guards.push(guard);
        }
        state.model = Some(model);
        Ok(())
    }
    /// Unsynced data remains in its original partition. Guest data is never adopted.
    pub fn switch(&self, namespace: Namespace) -> Result<()> {
        let mut state = self.state()?;
        if state.cache.namespace() == &namespace {
            if let Some(guard) = namespace.2 {
                state.memory.cancel_all();
                state.memory.guards.push(guard);
            }
            return Ok(());
        }
        let next = Cache::open(&self.root, namespace)?;
        for token in state.pending.values() {
            token.cancel();
        }
        state.cache.interrupt_pending(End::Cancelled)?;
        state.memory.cancel_all();
        state.memory = crate::memory::Session::for_namespace(next.namespace());
        state.pending.clear();
        state.model = None;
        state.sync_session = None;
        state.generation += 1;
        state.cache = next;
        Ok(())
    }
    /// Logout locks the account view by selecting the separate guest partition.
    pub fn logout(&self) -> Result<()> {
        self.switch(Namespace::guest())
    }
    pub fn cancel(&self, operation: Uuid) -> Result<()> {
        let mut state = self.state()?;
        if let Some(token) = state.pending.get(&operation) {
            token.cancel();
        }
        state.cache.finish(operation, End::Cancelled)?;
        state.pending.remove(&operation);
        Ok(())
    }
    pub fn create_conversation(&self, project: Option<Id>, title: String) -> Result<Uuid> {
        self.state()?.cache.create_conversation(project, title)
    }
    pub fn conversations(&self, after: Option<Uuid>, limit: usize) -> Result<Vec<Conversation>> {
        self.state()?.cache.conversations(after, limit)
    }
    pub fn attach(
        &self,
        conversation: Uuid,
        name: String,
        media_type: String,
        bytes: Vec<u8>,
    ) -> Result<Uuid> {
        self.state()?
            .cache
            .attach(conversation, name, media_type, bytes)
    }
    pub fn attachment(&self, conversation: Uuid, id: Uuid) -> Result<crate::Attachment> {
        self.state()?.cache.attachment(conversation, id)
    }
    pub fn messages(&self, conversation: Uuid, after: i64, limit: usize) -> Result<Vec<Message>> {
        self.state()?.cache.messages(conversation, after, limit)
    }
    pub fn outbox(&self, after: i64, limit: usize) -> Result<Vec<OutboxEntry>> {
        self.state()?.cache.outbox(after, limit)
    }
    /// Read only this partition's in-progress output; never resumes inference.
    pub fn draft(&self, operation: Uuid) -> Result<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .state()?
            .cache
            .db
            .query_row(
                "SELECT draft FROM turns WHERE operation=?1 AND status='pending'",
                [operation.to_string()],
                |row| row.get(0),
            )
            .optional()?)
    }
    /// A portable local snapshot of one explicitly selected Chat, never credentials or sync queues.
    pub fn export_conversation(&self, conversation: Uuid) -> Result<String> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let state = self.state()?;
        let selected = state.cache.conversation_detail(conversation)?;
        if !state.memory.safe(&selected.title) {
            return Err(Error::Invalid);
        }
        let mut messages = Vec::new();
        let mut after = 0;
        let mut bytes = selected.title.len();
        loop {
            let page = state.cache.messages(conversation, after, 128)?;
            if page.is_empty() {
                break;
            }
            after = page.last().ok_or(Error::Storage)?.sequence;
            for message in page {
                bytes += message.content.text.len();
                if bytes > 16 * 1024 * 1024
                    || messages.len() >= 4096
                    || !state.memory.safe(&message.content.text)
                {
                    return Err(Error::Invalid);
                }
                messages.push(message);
            }
        }
        let mut drafts = Vec::new();
        let mut pending = state.cache.db.prepare("SELECT operation,user_message,assistant_message,draft FROM turns WHERE conversation=?1 AND status='pending' ORDER BY operation LIMIT 17")?;
        let rows = pending.query_map([conversation.to_string()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (operation, user, assistant, draft) = row?;
            bytes += draft.len();
            if bytes > 16 * 1024 * 1024 || drafts.len() >= 16 || !state.memory.safe(&draft) {
                return Err(Error::Invalid);
            }
            drafts.push(json!({"operation":operation,"user_message":user,"assistant_message":assistant,"draft":draft,"status":"pending"}));
        }
        let mut attachments = Vec::new();
        let mut selected_attachments = state
            .cache
            .db
            .prepare("SELECT id FROM attachments WHERE conversation=?1 ORDER BY id LIMIT 65")?;
        let ids = selected_attachments
            .query_map([conversation.to_string()], |r| r.get::<_, String>(0))?;
        for id in ids {
            let id = Uuid::parse_str(&id?).map_err(|_| Error::UnsafeStorage)?;
            let attachment = state.cache.attachment(conversation, id)?;
            bytes += attachment.bytes.len().div_ceil(3) * 4;
            if bytes > 16 * 1024 * 1024 || attachments.len() >= 64 {
                return Err(Error::Invalid);
            }
            if !state.memory.safe(&attachment.name)
                || !state.memory.safe(&attachment.media_type)
                || !state
                    .memory
                    .safe(&String::from_utf8_lossy(&attachment.bytes))
            {
                return Err(Error::Invalid);
            }
            attachments.push(json!({"id":id,"name":attachment.name,"media_type":attachment.media_type,"base64":STANDARD.encode(attachment.bytes)}));
        }
        let output = json!({"format":"caidex-chat-export","version":1,"conversation":selected,"messages":messages,"pending_turns":drafts,"attachments":attachments}).to_string();
        if output.len() > 16 * 1024 * 1024 || !state.memory.safe(&output) {
            return Err(Error::Invalid);
        }
        Ok(output)
    }
    /// Callers choose a parent explicitly; concurrent sends produce sibling branches.
    pub async fn send(&self, send: Send) -> Result<Message> {
        let (provider, request, cancel, generation, has_memory) = {
            let mut state = self.state()?;
            if state.pending.len() >= 16 {
                return Err(Error::Invalid);
            }
            let model = state.model.as_ref().ok_or(Error::ModelUnavailable)?;
            validate_model(model, state.cache.namespace())?;
            let provider = model.provider.clone();
            let model = model.model.clone();
            if let Some(guard) = provider.memory_guard(&model) {
                state.memory.guards.push(guard);
            }
            let project: Option<String> = state.cache.db.query_row(
                "SELECT project_id FROM conversations WHERE id=?1",
                [send.conversation.to_string()],
                |r| r.get(0),
            )?;
            let project = project
                .map(Id::try_from)
                .transpose()
                .map_err(|_| Error::UnsafeStorage)?;
            let injection = crate::memory::injection(&state, project, &provider, &model)?;
            let has_memory = injection.is_some();
            let mut input =
                state
                    .cache
                    .begin(send.conversation, send.parent, send.content, send.operation)?;
            if let Some(memory) = injection {
                input
                    .as_array_mut()
                    .ok_or(Error::Invalid)?
                    .insert(0, memory);
            }
            let request=CanonicalRequest::new(json!({"model":model,"input":input,"stream":true,"store":false,"tools":[],"tool_choice":"none"}),ResponsesDialect::Classic).map_err(|_|Error::Invalid)?;
            let cancel = CancellationToken::new();
            state.pending.insert(send.operation, cancel.clone());
            if has_memory {
                state.memory.pending.insert(send.operation, cancel.clone());
            }
            (provider, request, cancel, state.generation, has_memory)
        };
        let context = RequestContext {
            cancellation: cancel.clone(),
            memory_request: has_memory,
            deadline: Some(Instant::now() + Duration::from_secs(120)),
            ..RequestContext::default()
        };
        let work = self.receive(provider, request, context, send.operation, generation);
        let end = tokio::select! {
            biased;
            _=cancel.cancelled()=>End::Cancelled,
            _=tokio::time::sleep(Duration::from_secs(120))=>End::Interrupted,
            result=work=>match result { Ok(end)=>end, Err(Error::Invalid)=>End::Invalid, Err(error)=>return Err(error) },
        };
        cancel.cancel();
        let mut state = self.state()?;
        if state.generation != generation || !state.pending.contains_key(&send.operation) {
            return Err(Error::Cancelled);
        }
        let message = state.cache.finish(send.operation, end)?;
        state.pending.remove(&send.operation);
        state.memory.pending.remove(&send.operation);
        if end == End::Complete {
            crate::memory::automatic_chat_sources(&mut state, &message)?;
        }
        Ok(message)
    }
    async fn receive(
        &self,
        provider: Arc<dyn ModelProvider>,
        request: CanonicalRequest,
        context: RequestContext,
        operation: Uuid,
        generation: u64,
    ) -> Result<End> {
        let mut stream = match provider.stream_response(request, context).await {
            Ok(s) => s.events,
            Err(_) => return Ok(End::Failed),
        };
        let mut text = String::new();
        while let Some(event) = stream.next().await {
            let event = match event {
                Ok(ProviderStreamEvent::Model(e)) => e.response,
                Ok(ProviderStreamEvent::Heartbeat) => continue,
                Err(_) => return Ok(End::Failed),
            };
            if let Some(delta) = event.text_delta() {
                if text.len().saturating_add(delta.len()) > MAX_TEXT {
                    return Ok(End::Invalid);
                }
                text.push_str(delta);
                self.checkpoint(operation, generation, &text)?;
            }
            if event.item().is_err() {
                return Ok(End::Invalid);
            }
            if let Ok(Some(item)) = event.item()
                && !matches!(item.kind(), "message" | "reasoning")
            {
                return Ok(End::Invalid);
            }
            if let Some(end) = event.terminal() {
                if matches!(
                    end,
                    StreamState::Completed | StreamState::Incomplete | StreamState::Interrupted
                ) {
                    let wire = event.wire()["response"].clone();
                    if wire.get("output").is_some() {
                        let response = match CanonicalResponse::new(wire) {
                            Ok(r) => r,
                            Err(_) => return Ok(End::Invalid),
                        };
                        let final_text = match visible_text(&response) {
                            Ok(t) => t,
                            Err(_) => return Ok(End::Invalid),
                        };
                        self.checkpoint(operation, generation, &final_text)?;
                    } else if end == StreamState::Completed {
                        return Ok(End::Invalid);
                    }
                }
                return Ok(match end {
                    StreamState::Completed => End::Complete,
                    StreamState::Incomplete => End::Incomplete,
                    StreamState::Interrupted => End::Interrupted,
                    _ => End::Failed,
                });
            }
        }
        Ok(End::Truncated)
    }
    fn checkpoint(&self, operation: Uuid, generation: u64, text: &str) -> Result<()> {
        let mut state = self.state()?;
        if state.generation != generation || !state.pending.contains_key(&operation) {
            return Err(Error::Cancelled);
        }
        // Provider output cannot persist credentials registered by this local Broker.
        if state
            .memory
            .guards
            .iter()
            .any(|guard| guard.contains_registered_secret(text))
        {
            return Err(Error::Invalid);
        }
        state.cache.checkpoint(operation, text)
    }
}
impl Drop for Chat {
    fn drop(&mut self) {
        if let Ok(state) = self.state.get_mut() {
            state.memory.cancel_all();
            for token in state.pending.values() {
                token.cancel();
            }
            // On write failure, pending rows remain recoverable at the next open.
            let _ = state.cache.interrupt_pending(End::Interrupted);
        }
    }
}
pub(crate) fn validate_model(model: &LocalModel, namespace: &Namespace) -> Result<()> {
    if model.model.is_empty() || model.model.len() > 256 {
        return Err(Error::Invalid);
    }
    let metadata = model
        .provider
        .metadata(&model.model)
        .map_err(|_| Error::ModelUnavailable)?;
    if !metadata.dialects.contains(&ResponsesDialect::Classic) {
        return Err(Error::ModelUnavailable);
    }
    match model
        .provider
        .credential_requirements(&model.model)
        .map_err(|_| Error::ModelUnavailable)?
    {
        CredentialRequirement::None => (),
        CredentialRequirement::Bearer { reference }
        | CredentialRequirement::ApiKey { reference } => {
            if reference.owner != namespace.credential_owner() {
                return Err(Error::WrongCredentialOwner);
            }
        }
    }
    Ok(())
}
pub(crate) fn visible_text(response: &CanonicalResponse) -> Result<String> {
    for item in response.output() {
        match item["type"].as_str() {
            Some("reasoning") => (),
            Some("message") => {
                if item["role"] != "assistant" {
                    return Err(Error::Invalid);
                }
                for content in item["content"].as_array().ok_or(Error::Invalid)? {
                    if !matches!(content["type"].as_str(), Some("output_text" | "refusal")) {
                        return Err(Error::Invalid);
                    }
                }
            }
            _ => return Err(Error::Invalid),
        }
    }
    let mut text = String::new();
    for item in response.output().iter().filter(|v| v["type"] == "message") {
        for content in item["content"].as_array().ok_or(Error::Invalid)? {
            let value = if content["type"] == "refusal" {
                &content["refusal"]
            } else {
                &content["text"]
            };
            let value = value.as_str().ok_or(Error::Invalid)?;
            if text.len().saturating_add(value.len()) > MAX_TEXT {
                return Err(Error::Invalid);
            }
            text.push_str(value);
        }
    }
    Ok(text)
}
