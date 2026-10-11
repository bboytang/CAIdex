//! Account-local Memory. Sources are data, never Runtime instructions or approvals.
mod host;
mod inference;
mod jobs;
use host::Executor;
pub use host::{HostConnection, HostModel, HostModelInfo, host_target};
pub use inference::{Candidate, InferenceOutput, InferenceRequest, infer};
mod store;
pub mod sync;
pub(crate) use jobs::{automatic_chat_sources, injection};

use crate::{Chat, Error, LocalModel, Result};
use caidex_cloud_core::Id;
use caidex_credentials::Redactor;
use caidex_model_core::{CancellationToken, ProviderTarget};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};
use uuid::Uuid;

pub use caidex_cloud_core::memory::{Draft, Kind, Origin, Privacy, Scope, Source, SourceType};
/// Trusted application boundary: must check real file/Git/Host ACL and return current evidence.
/// It cannot confer execution authority. An unavailable/offline Host returns None.
pub trait EvidenceResolver: Send + Sync {
    fn read(&self, scope: &Scope, origin: &Origin) -> Option<Vec<u8>>;
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Revision {
    pub id: Uuid,
    pub memory: Uuid,
    pub parent: Option<Uuid>,
    pub created_millis: i64,
    pub conflict: bool,
    pub source_current: bool,
    pub draft: Draft,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Memory {
    pub id: Uuid,
    pub owner_user_id: Option<Id>,
    pub head: Uuid,
    pub state: String,
    pub revisions: Vec<Revision>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Chat,
    Integration,
    Embedding,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permission {
    pub target: ProviderTarget,
    pub purpose: Purpose,
    pub scope: Scope,
    pub kinds: Vec<Kind>,
    pub source_types: Vec<SourceType>,
    pub allow_local_only: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Distance {
    Cosine,
    L2,
    InnerProduct,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Space {
    pub target: ProviderTarget,
    pub model_revision: String,
    pub dimensions: usize,
    pub preprocessing_version: String,
    pub distance: Distance,
}
impl Space {
    pub fn id(&self) -> Result<String> {
        if !(1..=16000).contains(&self.dimensions)
            || self.model_revision.is_empty()
            || self.model_revision.len() > 256
            || self.preprocessing_version.is_empty()
            || self.preprocessing_version.len() > 256
        {
            return Err(Error::Invalid);
        }
        Ok(digest(
            &serde_json::to_vec(self).map_err(|_| Error::Invalid)?,
        ))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub scope: Scope,
    pub kind: String,
    pub state: String,
}
#[derive(Default)]
pub(crate) struct Session {
    pub integration: Option<Executor>,
    pub embedding: Option<(Executor, Space)>,
    pub resolver: Option<Arc<dyn EvidenceResolver>>,
    pub guards: Vec<Arc<Redactor>>,
    pub pending: BTreeMap<Uuid, CancellationToken>,
    pub epoch: u64,
}
impl Session {
    pub(crate) fn for_namespace(namespace: &crate::Namespace) -> Self {
        Self {
            guards: namespace.2.iter().cloned().collect(),
            ..Self::default()
        }
    }
    pub(crate) fn cancel_all(&mut self) {
        for token in self.pending.values() {
            token.cancel();
        }
        self.pending.clear();
        self.epoch += 1;
    }
    pub(crate) fn safe(&self, text: &str) -> bool {
        !Redactor::default().contains_secret(text)
            && !self.guards.iter().any(|g| g.contains_secret(text))
    }
}
pub struct MemoryEngine<'a> {
    chat: &'a Chat,
}
impl Chat {
    pub fn memory(&self) -> MemoryEngine<'_> {
        MemoryEngine { chat: self }
    }
}
impl MemoryEngine<'_> {
    pub fn set_resolver(&self, resolver: Arc<dyn EvidenceResolver>) -> Result<()> {
        let mut s = self.chat.state()?;
        s.memory.cancel_all();
        s.memory.resolver = Some(resolver);
        Ok(())
    }
    /// Supply the account/session credential guard without storing or exporting its secrets.
    pub fn protect_credentials(&self, guard: Arc<Redactor>) -> Result<()> {
        let mut s = self.chat.state()?;
        s.memory.cancel_all();
        s.memory.guards.push(guard);
        Ok(())
    }
    pub fn configure_integration(&self, model: LocalModel) -> Result<()> {
        let mut s = self.chat.state()?;
        crate::client::validate_model(&model, s.cache.namespace())?;
        model
            .provider
            .memory_target(&model.model, false)
            .map_err(|_| Error::ModelUnavailable)?;
        if let Some(g) = model.provider.memory_guard(&model.model) {
            s.memory.guards.push(g);
        }
        s.memory.cancel_all();
        s.memory.integration = Some(Executor::Local(model));
        Ok(())
    }
    pub fn configure_embedding(&self, model: LocalModel, space: Space) -> Result<()> {
        space.id()?;
        let mut s = self.chat.state()?;
        crate::client::validate_model(&model, s.cache.namespace())?;
        if model
            .provider
            .memory_target(&model.model, true)
            .map_err(|_| Error::ModelUnavailable)?
            != space.target
        {
            return Err(Error::Invalid);
        }
        if let Some(g) = model.provider.memory_guard(&model.model) {
            s.memory.guards.push(g);
        }
        s.memory.cancel_all();
        s.memory.embedding = Some((Executor::Local(model), space));
        Ok(())
    }
}
pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn now() -> Result<i64> {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::Storage)?
            .as_millis(),
    )
    .map_err(|_| Error::Storage)
}
pub(crate) fn decode<T: serde::de::DeserializeOwned>(text: &str) -> Result<T> {
    serde_json::from_str(text).map_err(|_| Error::UnsafeStorage)
}
pub(crate) fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| Error::Invalid)
}
