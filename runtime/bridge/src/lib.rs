//! CAIdex's version-pinned Codex boundary; no upstream Rust types escape it.

mod protocol;
mod runtime;
mod stdio;

pub use protocol::{MethodInfo, ProtocolSurface, protocol_surface};
pub use runtime::{
    ApprovalDecision, ClientOptions, Interaction, InteractionKind, Runtime, RuntimeClient,
    RuntimeEvent, RuntimeInfo,
};
pub use stdio::{AppServer, Error, RequestId, RpcClient, RpcError, ServerEvent};

pub const CODEX_VERSION: &str = "0.160.1";
pub const CODEX_COMMIT: &str = "d27764b82f7118f674371e6d6e76271d9d606edb";
