//! Ordinary Chat and scoped Memory inference. Host Memory is explicit; no Runtime tools or approvals.
mod cache;
mod client;
pub mod memory;
pub mod sync;

pub use cache::{Attachment, Cache, Conversation, Message, OutboxEntry};
use caidex_auth::{ConfirmedSession, Trust};
use caidex_cloud_core::Id;
use caidex_credentials::Id as CredentialOwner;
pub use client::{Chat, LocalModel, Send};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const MAX_TEXT: usize = 1024 * 1024;
pub const MAX_ATTACHMENT: usize = 8 * 1024 * 1024;
pub const MAX_HISTORY: usize = 128;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Chat storage operation failed")]
    Storage,
    #[error("Chat sync unavailable; original queue retained")]
    SyncOffline,
    #[error("Chat sync consent, authentication or permission refused")]
    SyncDenied,
    #[error("Chat sync conflict; original data and operation retained")]
    SyncConflict,
    #[error("Chat storage path, permissions, owner or schema refused")]
    UnsafeStorage,
    #[error("Chat request or response is invalid or exceeds a limit")]
    Invalid,
    #[error("Chat entity is unavailable in the active scope")]
    NotFound,
    #[error("Chat operation was already submitted; inference is never replayed")]
    AlreadySubmitted,
    #[error("Chat model is not configured for this local namespace")]
    ModelUnavailable,
    #[error("Chat credential belongs to another endpoint or account")]
    WrongCredentialOwner,
    #[error("Host offline; no inference was submitted")]
    HostOffline,
    #[error("Host Memory authorization or pinned model refused")]
    HostDenied,
    #[error("Host inference outcome unknown; query the original operation, never replay")]
    HostUnknown,
    #[error("Chat account changed or the turn was cancelled")]
    Cancelled,
}
impl From<caidex_cloud_core::Invalid> for Error {
    fn from(_: caidex_cloud_core::Invalid) -> Self {
        Self::Invalid
    }
}
impl From<rusqlite::Error> for Error {
    fn from(_: rusqlite::Error) -> Self {
        Self::Storage
    }
}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::Storage
    }
}
pub type Result<T> = std::result::Result<T, Error>;

/// Local partition identity is not a cloud authorization token.
#[derive(Clone)]
pub struct Namespace(
    String,
    Option<Id>,
    Option<std::sync::Arc<caidex_credentials::Redactor>>,
);
impl PartialEq for Namespace {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 && self.1 == other.1
    }
}
impl Eq for Namespace {}
impl Namespace {
    pub fn guest() -> Self {
        Self("guest".into(), None, None)
    }
    pub fn account(session: &ConfirmedSession) -> Self {
        let mut namespace = Self::key(session.issuer(), session.user_id(), session.is_test());
        namespace.2 = Some(session.credential_guard());
        namespace
    }
    /// Explicit local offline selection; never resumes login or grants upload permission.
    pub fn offline_account(trust: &Trust, user: &Id) -> Self {
        Self::key(trust.issuer(), &String::from(user.clone()), trust.is_test())
    }
    fn key(issuer: &str, user: &str, test: bool) -> Self {
        Self(
            format!("{:x}", Sha256::digest(format!("{test}\0{issuer}\0{user}"))),
            Id::try_from(user.to_owned()).ok(),
            None,
        )
    }
    pub fn owner_user_id(&self) -> Option<&Id> {
        self.1.as_ref()
    }
    pub fn credential_owner(&self) -> CredentialOwner {
        CredentialOwner::new(format!("chat-{}", &self.0[..self.0.len().min(59)])).unwrap()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub text: String,
    pub attachments: Vec<Uuid>,
}
impl Content {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            attachments: vec![],
        }
    }
    fn validate(&self) -> Result<()> {
        if serde_json::to_vec(self).map_err(|_| Error::Invalid)?.len() > MAX_TEXT
            || self.text.len() > MAX_TEXT
            || self.attachments.len() > 4
            || self
                .attachments
                .iter()
                .enumerate()
                .any(|(i, id)| self.attachments[..i].contains(id))
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    Complete,
    Interrupted,
    Incomplete,
    Failed,
    Cancelled,
    Truncated,
    Invalid,
}
impl End {
    pub fn state(self) -> &'static str {
        if self == Self::Complete {
            "complete"
        } else {
            "interrupted"
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Interrupted => "interrupted",
            Self::Incomplete => "incomplete",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Truncated => "truncated",
            Self::Invalid => "invalid",
        }
    }
    fn parse(value: &str) -> Result<Self> {
        match value {
            "complete" => Ok(Self::Complete),
            "interrupted" => Ok(Self::Interrupted),
            "incomplete" => Ok(Self::Incomplete),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "truncated" => Ok(Self::Truncated),
            "invalid" => Ok(Self::Invalid),
            _ => Err(Error::UnsafeStorage),
        }
    }
}
