use std::{collections::BTreeMap, sync::OnceLock};

use serde_json::Value;

/// Schema presence describes the wire surface, not provider or tool availability.
#[derive(Clone, Debug)]
pub struct MethodInfo {
    pub experimental: bool,
    pub params_required: bool,
}

#[derive(Debug)]
pub struct ProtocolSurface {
    pub client_requests: BTreeMap<String, MethodInfo>,
    pub server_requests: BTreeMap<String, MethodInfo>,
    pub notifications: BTreeMap<String, MethodInfo>,
}

pub fn protocol_surface() -> &'static ProtocolSurface {
    static SURFACE: OnceLock<ProtocolSurface> = OnceLock::new();
    SURFACE.get_or_init(|| {
        let stable: Value = serde_json::from_str(include_str!(
            "../../../upstream/codex/schemas/protocol.stable.json"
        ))
        .expect("pinned stable schema");
        let experimental: Value = serde_json::from_str(include_str!(
            "../../../upstream/codex/schemas/protocol.experimental.json"
        ))
        .expect("pinned experimental schema");
        let methods = |name: &str| {
            let stable_methods = variants(&stable, name);
            variants(&experimental, name)
                .into_iter()
                .map(|(method, params_required)| {
                    let experimental = !stable_methods.contains_key(&method);
                    (
                        method,
                        MethodInfo {
                            experimental,
                            params_required,
                        },
                    )
                })
                .collect()
        };
        ProtocolSurface {
            client_requests: methods("ClientRequest"),
            server_requests: methods("ServerRequest"),
            notifications: methods("ServerNotification"),
        }
    })
}

fn variants(schema: &Value, name: &str) -> BTreeMap<String, bool> {
    schema["definitions"][name]["oneOf"]
        .as_array()
        .expect("pinned method variants")
        .iter()
        .map(|variant| {
            let method = variant["properties"]["method"]["enum"][0]
                .as_str()
                .expect("pinned method name")
                .to_owned();
            let required = variant["required"]
                .as_array()
                .is_some_and(|required| required.iter().any(|key| key == "params"));
            (method, required)
        })
        .collect()
}
