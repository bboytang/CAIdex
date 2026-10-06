//! The version-pinned Codex stdio boundary. Runtime payloads remain opaque here.

mod stdio;

pub use stdio::{AppServer, Error, RequestId, RpcClient, RpcError, ServerEvent};

pub const CODEX_VERSION: &str = "0.160.1";
pub const CODEX_COMMIT: &str = "d27764b82f7118f674371e6d6e76271d9d606edb";
