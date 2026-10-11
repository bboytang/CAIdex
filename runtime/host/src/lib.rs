//! Local Host lifecycle and persistent tasks. Execution and approvals remain in Codex.
mod journal;
mod security;
mod service;
mod task;

pub use journal::{Event, Journal, Snapshot};
pub use security::private_directory;
pub use service::serve;
pub use task::{HostPolicy, Operation, Submission, Task};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Host I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Host journal failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Host JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Host refused: {0}")]
    Refused(&'static str),
    #[error("Host owner-only Windows ACL check failed at step {0}")]
    WindowsAcl(i32),
    #[error("Runtime failed: {0}")]
    Runtime(#[from] caidex_runtime::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn random_id() -> Result<String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| Error::Refused("OS randomness unavailable"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
