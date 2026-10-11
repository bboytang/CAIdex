//! Shared Memory payloads and bounded, versioned synchronization contracts.
use crate::Id;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const PROTOCOL: u32 = 1;
pub const MAX_WIRE: usize = 1024 * 1024;
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    Personal,
    Project { project_id: Id },
}
impl Scope {
    pub fn key(&self) -> String {
        match self {
            Self::Personal => "personal".into(),
            Self::Project { project_id } => format!("project:{}", String::from(project_id.clone())),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    UserDecision,
    Preference,
    FileEvidence,
    ExecutionResult,
    AiInference,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Privacy {
    LocalOnly,
    Shareable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Origin {
    User,
    Chat { conversation: Uuid, message: Uuid },
    File { reference: String },
    Git { reference: String },
    Execution { reference: String },
    External { reference: String },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    User,
    Chat,
    File,
    Git,
    Execution,
    External,
}
impl Origin {
    pub fn source_type(&self) -> SourceType {
        match self {
            Self::User => SourceType::User,
            Self::Chat { .. } => SourceType::Chat,
            Self::File { .. } => SourceType::File,
            Self::Git { .. } => SourceType::Git,
            Self::Execution { .. } => SourceType::Execution,
            Self::External { .. } => SourceType::External,
        }
    }
}
/// Expected digest covers the complete authorized source, not only the excerpt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub scope: Scope,
    pub origin: Origin,
    pub sha256: String,
    pub excerpt: String,
}
impl Source {
    pub fn user(scope: Scope, text: String) -> Self {
        Self {
            scope,
            origin: Origin::User,
            sha256: digest(text.as_bytes()),
            excerpt: text,
        }
    }
    pub fn evidence(scope: Scope, origin: Origin, bytes: &[u8], excerpt: String) -> Self {
        Self {
            scope,
            origin,
            sha256: digest(bytes),
            excerpt,
        }
    }
    pub fn fingerprint(&self) -> Result<String, crate::Invalid> {
        Ok(digest(
            &serde_json::to_vec(&(&self.scope, &self.origin, &self.sha256))
                .map_err(|_| crate::Invalid)?,
        ))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub scope: Scope,
    pub text: String,
    pub kind: Kind,
    pub privacy: Privacy,
    pub tags: Vec<String>,
    pub sources: Vec<Source>,
    pub valid_until_millis: Option<i64>,
}

impl Draft {
    pub fn valid(&self) -> bool {
        !self.text.trim().is_empty()
            && self.text.len() <= 16384
            && self.tags.len() <= 16
            && self.tags.iter().all(|t| !t.is_empty() && t.len() <= 128)
            && !self.sources.is_empty()
            && self.sources.len() <= 8
            && self.valid_until_millis.is_none_or(|t| t >= 0)
            && serde_json::to_vec(self).is_ok_and(|v| v.len() <= 65536)
            && self.sources.iter().all(|s| {
                s.scope == self.scope
                    && !s.excerpt.is_empty()
                    && s.excerpt.len() <= 16384
                    && s.sha256.len() == 64
                    && s.sha256
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                    && match &s.origin {
                        Origin::File { reference }
                        | Origin::Git { reference }
                        | Origin::Execution { reference }
                        | Origin::External { reference } => {
                            !reference.is_empty() && reference.len() <= 1024
                        }
                        _ => true,
                    }
            })
            && (self.kind != Kind::FileEvidence
                || self
                    .sources
                    .iter()
                    .any(|s| matches!(s.origin, Origin::File { .. } | Origin::Git { .. })))
            && (self.kind != Kind::ExecutionResult
                || self
                    .sources
                    .iter()
                    .any(|s| matches!(s.origin, Origin::Execution { .. })))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub enabled: bool,
    pub version: i64,
    pub epoch: i64,
    pub scope_version: i64,
    pub scopes: Vec<Scope>,
    pub delete_pending: bool,
}
impl Control {
    pub fn valid(&self) -> bool {
        self.version > 0
            && self.epoch > 0
            && self.scope_version > 0
            && self.scopes.len() <= 64
            && (!self.delete_pending || !self.enabled)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Enable,
    Keep,
    Delete,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetControl {
    pub protocol: u32,
    pub operation: Uuid,
    pub base_version: i64,
    pub mode: Mode,
    pub scopes: Vec<Scope>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permit {
    pub enabled: bool,
    pub delete_pending: bool,
    pub owner: Uuid,
    pub project: Option<Uuid>,
    pub version: i64,
    pub epoch: i64,
    pub scope_version: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    Upsert {
        id: Uuid,
        revision: Uuid,
        base: Option<Uuid>,
        created: i64,
        draft: Draft,
        recreate: bool,
        candidate: bool,
    },
    Select {
        id: Uuid,
        revision: Uuid,
        base: Uuid,
        generation: i64,
    },
    Delete {
        id: Uuid,
        sources: Vec<String>,
    },
}
impl Change {
    pub fn id(&self) -> Uuid {
        match self {
            Self::Upsert { id, .. } | Self::Select { id, .. } | Self::Delete { id, .. } => *id,
        }
    }
    pub fn valid(&self) -> bool {
        match self {
            Self::Upsert {
                revision,
                base,
                created,
                draft,
                ..
            } => {
                *created >= 0
                    && base.as_ref() != Some(revision)
                    && draft.valid()
                    && draft.privacy == Privacy::Shareable
            }
            Self::Select { generation, .. } => *generation > 0,
            Self::Delete { sources, .. } => {
                sources.len() <= 8192
                    && sources.iter().all(|h| {
                        h.len() == 64
                            && h.bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    })
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Push {
    pub protocol: u32,
    pub operation: Uuid,
    pub version: i64,
    pub permit: Permit,
    pub change: Change,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pull {
    pub protocol: u32,
    pub version: i64,
    pub permit: Permit,
    pub after: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub operation: Uuid,
    pub hash: String,
    pub sequence: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub after: i64,
    pub next: i64,
    pub change: Option<Change>,
    pub head: Option<Uuid>,
    pub state: Option<String>,
    pub generation: i64,
}
