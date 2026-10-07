//! Native Anthropic protocol and HTTP client. ModelProvider conversion and
//! Codex history mapping are subsequent work, not mocked interfaces.
mod catalog;
mod client;
mod message;
mod projection;
mod stream;
mod tools;
mod transfer;

pub use caidex_provider_custom::{ClientOptions, Error, Limits};
pub use catalog::{ModelCatalog, ModelsPage, NativeModel};
pub use client::{AnthropicClient, AnthropicConfig};
pub use message::{MessageOutcome, NativeMessage};
pub use stream::{MessageEvent, MessageStream, NativeStreamState};
pub use tools::ToolMap;
pub use transfer::{NativeStreamEvent, NativeStreamingResponse};

use caidex_model_core::{ProviderError, ProviderResult};
use serde_json::Value;

fn invalid_message() -> ProviderError {
    ProviderError::new(502, "anthropic_invalid_message")
}
fn string<'a>(wire: &'a Value, key: &str) -> Option<&'a str> {
    wire.get(key)?.as_str().filter(|text| !text.is_empty())
}
pub(crate) fn validate_block(block: &Value, complete: bool) -> ProviderResult<()> {
    let kind = string(block, "type").ok_or_else(invalid_message)?;
    let valid = match kind {
        "text" => {
            block["text"].is_string()
                && block
                    .get("citations")
                    .is_none_or(|v| v.is_null() || v.is_array())
        }
        "thinking" => {
            block["thinking"].is_string()
                && block["signature"].is_string()
                && (!complete || !block["signature"].as_str().unwrap().is_empty())
        }
        "redacted_thinking" => string(block, "data").is_some(),
        "tool_use" | "server_tool_use" => {
            string(block, "id").is_some()
                && string(block, "name").is_some()
                && block["input"].is_object()
        }
        // New native blocks remain opaque. Their eventual Codex conversion must
        // explicitly support them or fail; parsing is not permission to execute.
        _ => block.is_object(),
    };
    if valid {
        Ok(())
    } else {
        Err(invalid_message())
    }
}
pub(crate) fn validate_usage(usage: &Value) -> ProviderResult<()> {
    if !usage.is_object()
        || [
            "input_tokens",
            "output_tokens",
            "cache_creation_input_tokens",
            "cache_read_input_tokens",
        ]
        .iter()
        .any(|name| {
            usage
                .get(*name)
                .is_some_and(|value| !value.is_null() && value.as_u64().is_none())
        })
    {
        return Err(invalid_message());
    }
    Ok(())
}
