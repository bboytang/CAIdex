//! Execution-side credential storage. Remote/UI protocols carry only references
//! and configuration status; secret resolution stays inside the executing process.

mod environment;
#[cfg(unix)]
mod protected_file;
mod redaction;
#[cfg(any(windows, all(target_os = "linux", feature = "linux-secret-service")))]
mod system;

use serde::{Deserialize, Serialize};
use std::{fmt, sync::Mutex};
use thiserror::Error;
use zeroize::Zeroizing;

pub use environment::EnvironmentStore;
#[cfg(unix)]
pub use protected_file::ProtectedFileStore;
pub use redaction::Redactor;
#[cfg(any(windows, all(target_os = "linux", feature = "linux-secret-service")))]
pub use system::SystemStore;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("invalid credential identifier")]
    InvalidIdentifier,
    #[error("invalid secret")]
    InvalidSecret,
    #[error("credential belongs to another executing endpoint")]
    WrongOwner,
    #[error("credential backend unavailable")]
    BackendUnavailable,
    #[error("credential storage is locked or access is denied")]
    StorageAccessDenied,
    #[error("secret exceeds the backend limit of {0} stored bytes")]
    SecretTooLong(u32),
    #[error("credential storage permissions or path are unsafe")]
    UnsafeStorage,
    #[error("credential storage must be outside Git worktrees")]
    GitWorktree,
    #[error("credential source is read-only")]
    ReadOnly,
    #[error("credential I/O failed ({0:?})")]
    Io(std::io::ErrorKind),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Non-secret persistent lowercase identifier; display labels are separate.
/// Windows credential targets are case-insensitive, so mixed case is rejected.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id(String);

impl Id {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        {
            return Err(Error::InvalidIdentifier);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for Id {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        Self::new(value)
    }
}
impl From<Id> for String {
    fn from(value: Id) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretKind {
    ApiKey,
    AccessToken,
    RefreshToken,
    ClientSecret,
}

impl SecretKind {
    fn name(self) -> &'static str {
        match self {
            Self::ApiKey => "api-key",
            Self::AccessToken => "access-token",
            Self::RefreshToken => "refresh-token",
            Self::ClientSecret => "client-secret",
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CredentialRef {
    pub owner: Id,
    pub provider: Id,
    pub profile: Id,
    pub kind: SecretKind,
}

impl CredentialRef {
    pub(crate) fn storage_key(&self) -> String {
        format!(
            "{}.{}.{}.{}",
            self.owner.as_str(),
            self.provider.as_str(),
            self.profile.as_str(),
            self.kind.name()
        )
    }
}

/// Intentionally neither Serialize, Clone nor Display. Providers explicitly expose
/// a borrowed value only when constructing authentication inside the executor.
///
/// ```compile_fail
/// use caidex_credentials::Secret;
/// let value = Secret::new("synthetic-example".into()).unwrap();
/// serde_json::to_string(&value).unwrap(); // Secret must not implement Serialize.
/// ```
pub struct Secret(Zeroizing<String>);
impl Secret {
    pub fn new(value: String) -> Result<Self> {
        let value = Zeroizing::new(value);
        if value.is_empty() || value.len() > 16 * 1024 || value.contains('\0') {
            return Err(Error::InvalidSecret);
        }
        Ok(Self(value))
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

/// Native Keychain implementations can implement this same executor-side contract.
/// Backend errors must use safe enums, never a third-party error carrying secrets.
pub trait SecretStore: Send + Sync {
    fn get(&self, reference: &CredentialRef) -> Result<Option<Secret>>;
    fn set(&self, reference: &CredentialRef, value: &Secret) -> Result<()>;
    fn remove(&self, reference: &CredentialRef) -> Result<bool>;
    fn is_read_only(&self) -> bool {
        false
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStatus {
    pub reference: CredentialRef,
    pub configured: bool,
    pub read_only: bool,
}

pub struct Broker<S: SecretStore> {
    owner: Id,
    store: S,
    operations: Mutex<()>,
    redactor: Redactor,
}

impl<S: SecretStore> Broker<S> {
    pub fn new(owner: Id, store: S) -> Self {
        Self {
            owner,
            store,
            operations: Mutex::new(()),
            redactor: Redactor::default(),
        }
    }
    pub fn redactor(&self) -> &Redactor {
        &self.redactor
    }
    fn check_owner(&self, reference: &CredentialRef) -> Result<()> {
        if reference.owner != self.owner {
            return Err(Error::WrongOwner);
        }
        Ok(())
    }
    pub fn status(&self, reference: &CredentialRef) -> Result<CredentialStatus> {
        let configured = self.resolve(reference)?.is_some();
        Ok(CredentialStatus {
            reference: reference.clone(),
            configured,
            read_only: self.store.is_read_only(),
        })
    }
    /// Only provider adapters in this executor use resolve. Never export through
    /// a Host/UI RPC; Host authorization is a separate boundary.
    pub fn resolve(&self, reference: &CredentialRef) -> Result<Option<Secret>> {
        self.check_owner(reference)?;
        let _operation = self
            .operations
            .lock()
            .map_err(|_| Error::BackendUnavailable)?;
        let value = self.store.get(reference)?;
        if let Some(secret) = &value {
            self.redactor.register(secret)?;
        }
        Ok(value)
    }
    pub fn set(&self, reference: &CredentialRef, value: Secret) -> Result<()> {
        self.check_owner(reference)?;
        let _operation = self
            .operations
            .lock()
            .map_err(|_| Error::BackendUnavailable)?;
        self.redactor.register(&value)?;
        self.store.set(reference, &value)
    }
    pub fn remove(&self, reference: &CredentialRef) -> Result<bool> {
        self.check_owner(reference)?;
        let _operation = self
            .operations
            .lock()
            .map_err(|_| Error::BackendUnavailable)?;
        self.store.remove(reference)
    }
}
