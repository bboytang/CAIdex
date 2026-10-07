//! Native Gemini protocol. No tool execution or implicit credential discovery.
mod catalog;
mod client;
pub use caidex_provider_custom::{ClientOptions, Error, Limits};
pub use catalog::{ModelCatalog, ModelsPage, NativeModel};
pub use client::{GeminiClient, GeminiConfig};
