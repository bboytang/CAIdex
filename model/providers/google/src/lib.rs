//! Native Gemini protocol. No tool execution or implicit credential discovery.
mod catalog;
mod client;
mod content;
mod history;
mod request;
mod stream;
mod tools;
mod transfer;
pub use caidex_provider_custom::{ClientOptions, Error, Limits};
pub use catalog::{ModelCatalog, ModelsPage, NativeModel};
pub use client::{GeminiClient, GeminiConfig};
pub use content::{CandidateOutcome, NativeResponse};
pub use history::NativeHistory;
pub use request::GenerateContentRequest;
pub use stream::{ContentEvent, ContentStream, NativeStreamResponse, NativeStreamState};
pub use tools::ToolMap;
pub use transfer::{NativeStreamEvent, NativeStreamingResponse};
