//! Native Gemini protocol. No tool execution or implicit credential discovery.
mod catalog;
mod client;
mod content;
mod stream;
mod transfer;
pub use caidex_provider_custom::{ClientOptions, Error, Limits};
pub use catalog::{ModelCatalog, ModelsPage, NativeModel};
pub use client::{GeminiClient, GeminiConfig};
pub use content::{CandidateOutcome, NativeResponse};
pub use stream::{ContentEvent, ContentStream, NativeStreamResponse, NativeStreamState};
pub use transfer::{NativeStreamEvent, NativeStreamingResponse};
