//! Provider-neutral views over the Responses boundary. Wire JSON stays intact;
//! adapters interpret fields without discarding unknown items or opaque data.
//! HTTP, provider authentication and tool execution are not owned by this crate.

mod provider;
mod registry;
mod responses;
mod sse;
mod stream;

pub use provider::{
    CancellationToken, ContextHeaders, CredentialRequirement, ModelProvider, ProviderError,
    ProviderFuture, ProviderResponse, ProviderResult, ProviderStream, ProviderStreamEvent,
    REQUEST_HEADERS, RESPONSE_HEADERS, RequestContext, StreamingResponse,
};
pub use registry::{
    CapabilitySupport, CompatibilityLevel, CompatibilityReport, EvidenceSource, ModelCapabilities,
    ModelMetadata, ModelRegistry,
};
pub use responses::{
    CanonicalRequest, CanonicalResponse, ResponseEvent, ResponseItem, ResponsesDialect, ToolCall,
    ToolInput, ToolKind, ToolResult, ToolSearchCall, ToolSearchOutput, Usage,
};
pub use sse::{SseDecoder, SseEvent};
pub use stream::{ResponsesStream, StreamEvent, StreamState};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("invalid model request")]
    InvalidRequest,
    #[error("invalid model response")]
    InvalidResponse,
    #[error("invalid or duplicate model metadata")]
    InvalidMetadata,
    #[error("invalid Responses item")]
    InvalidItem,
    #[error("invalid Responses event")]
    InvalidEvent,
    #[error("invalid Responses usage")]
    InvalidUsage,
    #[error("invalid UTF-8 in event stream")]
    InvalidUtf8,
    #[error("event stream frame limit must be positive")]
    InvalidLimit,
    #[error("event stream frame exceeded the configured limit")]
    FrameTooLarge,
    #[error("event stream has closed")]
    StreamClosed,
    #[error("event stream ended before a terminal response")]
    UnexpectedEnd,
    #[error("response event sequence did not advance")]
    InvalidSequence,
    #[error("event stream mixed response identities")]
    ResponseMismatch,
}

pub type Result<T> = std::result::Result<T, Error>;
