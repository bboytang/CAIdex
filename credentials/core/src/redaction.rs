use crate::{Error, Result, Secret};
use serde_json::Value;
use std::sync::Mutex;

const MASK: &str = "[REDACTED]";
fn sensitive(name: &str) -> bool {
    let normalized: String = name
        .chars()
        .filter(|c| *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect();
    matches!(
        normalized.as_str(),
        "authorization"
            | "proxyauthorization"
            | "apikey"
            | "xapikey"
            | "xgoogapikey"
            | "accesstoken"
            | "refreshtoken"
            | "clientsecret"
            | "cookie"
            | "setcookie"
            | "password"
            | "token"
            | "secret"
    )
}

/// Use on diagnostic copies only, never on Runtime history or provider wire data.
/// Keeps previously registered values masked after replacement/removal too.
#[derive(Default)]
pub struct Redactor {
    values: Mutex<Vec<Secret>>,
}
impl Redactor {
    pub fn register(&self, value: &Secret) -> Result<()> {
        let mut values = self.values.lock().map_err(|_| Error::BackendUnavailable)?;
        if !values.iter().any(|old| old.expose() == value.expose()) {
            values.push(Secret::new(value.expose().to_owned())?);
            values.sort_by_key(|value| std::cmp::Reverse(value.expose().len()));
        }
        Ok(())
    }
    pub fn text(&self, text: &str) -> String {
        if let Ok(value @ (Value::Object(_) | Value::Array(_))) = serde_json::from_str(text) {
            return self.json(&value).to_string();
        }
        let Ok(values) = self.values.lock() else {
            return MASK.into();
        };
        let mut text = text.to_owned();
        for value in values.iter() {
            text = text.replace(value.expose(), MASK);
        }
        text.split('\n')
            .map(|line| {
                if let Some(index) = line.find([':', '='])
                    && sensitive(line[..index].trim())
                {
                    let name = &line[..index];
                    return format!("{name}: {MASK}");
                }
                line.to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn json(&self, value: &Value) -> Value {
        match value {
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, value)| {
                        (
                            key.clone(),
                            if sensitive(key) {
                                Value::String(MASK.into())
                            } else {
                                self.json(value)
                            },
                        )
                    })
                    .collect(),
            ),
            Value::Array(values) => {
                Value::Array(values.iter().map(|value| self.json(value)).collect())
            }
            Value::String(text) => Value::String(self.text(text)),
            value => value.clone(),
        }
    }
}
