use crate::{
    Cache, Content, Conversation, End, Error, MAX_TEXT, Message, Namespace, OutboxEntry, Result,
};
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
struct State {
    cache: Cache,
    model: Option<LocalModel>,
    generation: u64,
    pending: BTreeMap<Uuid, CancellationToken>,
}
/// Shared client state makes cancellation/account changes invalidate late replies.
pub struct Chat {
    root: PathBuf,
    state: Mutex<State>,
}
impl Chat {
    pub fn open(root: &Path, namespace: Namespace) -> Result<Self> {
        Ok(Self {
            root: root.to_owned(),
            state: Mutex::new(State {
                cache: Cache::open(root, namespace)?,
                model: None,
                generation: 0,
                pending: BTreeMap::new(),
            }),
        })
    }
    fn state(&self) -> Result<std::sync::MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| Error::Storage)
    }
    pub fn configure(&self, model: LocalModel) -> Result<()> {
        let mut state = self.state()?;
        validate_model(&model, state.cache.namespace())?;
        if !state.pending.is_empty() {
            return Err(Error::Invalid);
        }
        state.model = Some(model);
        Ok(())
    }
    /// Unsynced data remains in its original partition. Guest data is never adopted.
    pub fn switch(&self, namespace: Namespace) -> Result<()> {
        let mut state = self.state()?;
        if state.cache.namespace() == &namespace {
            return Ok(());
        }
        let next = Cache::open(&self.root, namespace)?;
        for token in state.pending.values() {
            token.cancel();
        }
        state.cache.interrupt_pending(End::Cancelled)?;
        state.pending.clear();
        state.model = None;
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
    /// Callers choose a parent explicitly; concurrent sends produce sibling branches.
    pub async fn send(&self, send: Send) -> Result<Message> {
        let (provider, request, cancel, generation) = {
            let mut state = self.state()?;
            let model = state.model.as_ref().ok_or(Error::ModelUnavailable)?;
            validate_model(model, state.cache.namespace())?;
            let provider = model.provider.clone();
            let model = model.model.clone();
            let input =
                state
                    .cache
                    .begin(send.conversation, send.parent, send.content, send.operation)?;
            let request=CanonicalRequest::new(json!({"model":model,"input":input,"stream":true,"store":false,"tools":[],"tool_choice":"none"}),ResponsesDialect::Classic).map_err(|_|Error::Invalid)?;
            let cancel = CancellationToken::new();
            state.pending.insert(send.operation, cancel.clone());
            (provider, request, cancel, state.generation)
        };
        let context = RequestContext {
            cancellation: cancel.clone(),
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
        state.cache.checkpoint(operation, text)
    }
}
impl Drop for Chat {
    fn drop(&mut self) {
        if let Ok(state) = self.state.get_mut() {
            for token in state.pending.values() {
                token.cancel();
            }
            // On write failure, pending rows remain recoverable at the next open.
            let _ = state.cache.interrupt_pending(End::Interrupted);
        }
    }
}
fn validate_model(model: &LocalModel, namespace: &Namespace) -> Result<()> {
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
fn visible_text(response: &CanonicalResponse) -> Result<String> {
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
