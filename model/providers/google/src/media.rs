use crate::RequestOptions;
use base64::Engine;
use caidex_model_core::{ProviderError, ProviderResult};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn invalid_options() -> ProviderError {
    ProviderError::new(400, "invalid_google_image_options")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "unsupported_google_images")
}
/// Executor-owned mapping of image detail intent to native per-Part resolution.
/// Scales/token costs are not equivalent across providers. The presence of a
/// mapping declares per-Part capability; never infer it from the model name.
#[derive(Debug)]
pub struct ImageDetailMapping {
    source: String,
    level: String,
}
impl ImageDetailMapping {
    pub fn new(source: String, level: String) -> ProviderResult<Self> {
        if !matches!(source.as_str(), "low" | "high" | "original")
            || !matches!(
                level.as_str(),
                "MEDIA_RESOLUTION_LOW"
                    | "MEDIA_RESOLUTION_MEDIUM"
                    | "MEDIA_RESOLUTION_HIGH"
                    | "MEDIA_RESOLUTION_ULTRA_HIGH"
            )
        {
            return Err(invalid_options());
        }
        Ok(Self { source, level })
    }
}
fn allowed(mime: &str, result: bool) -> bool {
    matches!(mime, "image/png" | "image/jpeg" | "image/webp")
        || (!result && matches!(mime, "image/heic" | "image/heif"))
}
pub(crate) fn validate_options(options: &RequestOptions<'_>) -> ProviderResult<()> {
    for (mimes, result) in [
        (options.image_mime_types, false),
        (options.tool_result_image_mime_types, true),
    ] {
        let mut seen = BTreeSet::new();
        if mimes
            .iter()
            .any(|mime| !allowed(mime, result) || !seen.insert(*mime))
        {
            return Err(invalid_options());
        }
    }
    let mut details = BTreeSet::new();
    if options
        .image_detail_mappings
        .iter()
        .any(|mapping| !details.insert(mapping.source.as_str()))
    {
        return Err(invalid_options());
    }
    Ok(())
}
pub(crate) fn image(
    block: &Value,
    options: &RequestOptions<'_>,
    result: bool,
    max_bytes: usize,
) -> ProviderResult<Value> {
    let invalid = || ProviderError::new(400, "invalid_google_image");
    let mimes = if result {
        options.tool_result_image_mime_types
    } else {
        options.image_mime_types
    };
    if mimes.is_empty() {
        return Err(unsupported());
    }
    if block.get("file_id").is_some() {
        return Err(ProviderError::new(400, "unsupported_google_image_source"));
    }
    let url = block["image_url"].as_str().ok_or_else(invalid)?;
    if url.len() > max_bytes {
        return Err(invalid());
    }
    let data = url
        .strip_prefix("data:")
        .ok_or_else(|| ProviderError::new(400, "unsupported_google_image_source"))?;
    let (header, data) = data.split_once(',').ok_or_else(invalid)?;
    let mime = header.strip_suffix(";base64").ok_or_else(invalid)?;
    if !allowed(mime, result) || !mimes.contains(&mime) {
        return Err(unsupported());
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| invalid())?;
    if decoded.is_empty() || decoded.len() > max_bytes {
        return Err(invalid());
    }
    let mut part = json!({"inlineData":{"mimeType":mime,"data":data}});
    let detail = match crate::content::present(block, "detail") {
        None => "auto",
        Some(Value::String(detail)) => detail.as_str(),
        _ => return Err(invalid()),
    };
    if detail != "auto" {
        if !matches!(detail, "low" | "high" | "original") {
            return Err(invalid());
        }
        // FunctionResponsePart has only inlineData, no per-Part resolution.
        let mapping = options
            .image_detail_mappings
            .iter()
            .find(|m| !result && m.source == detail)
            .ok_or_else(|| ProviderError::new(400, "unsupported_google_image_detail"))?;
        part["mediaResolution"] = json!({"level":mapping.level});
    }
    Ok(part)
}
pub(crate) fn tool_output(
    output: &Value,
    order: usize,
    options: &RequestOptions<'_>,
    max_bytes: usize,
) -> ProviderResult<Value> {
    let mut response = json!({"response":{"output":output}});
    let Some(blocks) = output.as_array() else {
        return Ok(response);
    };
    let mut projected = Vec::new();
    let mut media = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        if block["type"] == "input_image" {
            let mut part = image(block, options, true, max_bytes)?;
            // Call ordinal + block position is stable on full-prefix replay and
            // unique across every result in this request, without changing IDs.
            let name = format!("caidex_image_{order}_{index}");
            part["inlineData"]["displayName"] = name.clone().into();
            media.push(part);
            let mut reference = block.clone();
            reference["image_url"] = json!({"$ref":name});
            projected.push(reference);
        } else {
            crate::request::text_part(block, false)?;
            projected.push(block.clone());
        }
    }
    response["response"]["output"] = json!(projected);
    if !media.is_empty() {
        let mut names = media
            .iter()
            .map(|part| part["inlineData"]["displayName"].as_str().unwrap())
            .collect();
        validate_references(&response["response"], &mut names)?;
        response["parts"] = json!(media);
    }
    Ok(response)
}

fn validate_references(value: &Value, names: &mut BTreeSet<&str>) -> ProviderResult<()> {
    match value {
        Value::Object(object) if object.len() == 1 && object.contains_key("$ref") => {
            if !object["$ref"]
                .as_str()
                .is_some_and(|name| names.remove(name))
            {
                return Err(ProviderError::new(400, "invalid_google_image_reference"));
            }
        }
        Value::Object(object) => {
            for child in object.values() {
                validate_references(child, names)?;
            }
        }
        Value::Array(array) => {
            for child in array {
                validate_references(child, names)?;
            }
        }
        _ => {}
    }
    Ok(())
}
