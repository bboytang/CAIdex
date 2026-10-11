//! Chat data only. Credentials, Provider carriers, drafts and Memory have no wire fields.
use crate::Id;
use serde::{Deserialize, Serialize};

pub const PROTOCOL: u32 = 1;
pub const MAX_WIRE: usize = 12 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub enabled: bool,
    pub version: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Entity {
    Conversation {
        id: Id,
        project_id: Option<Id>,
        title: String,
    },
    Attachment {
        id: Id,
        conversation: Id,
        name: String,
        media_type: String,
        data: String,
    },
    Message {
        id: Id,
        conversation: Id,
        parent: Option<Id>,
        role: String,
        text: String,
        attachments: Vec<Id>,
        ending: String,
        created_at_millis: i64,
    },
    Delete {
        id: Id,
    },
}
impl Entity {
    pub fn id(&self) -> &Id {
        match self {
            Self::Conversation { id, .. }
            | Self::Attachment { id, .. }
            | Self::Message { id, .. }
            | Self::Delete { id } => id,
        }
    }
    pub fn conversation(&self) -> &Id {
        match self {
            Self::Conversation { id, .. } | Self::Delete { id } => id,
            Self::Attachment { conversation, .. } | Self::Message { conversation, .. } => {
                conversation
            }
        }
    }
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Conversation { .. } => "conversation",
            Self::Attachment { .. } => "attachment",
            Self::Message { .. } => "message",
            Self::Delete { .. } => "delete",
        }
    }
    pub fn valid(&self) -> bool {
        match self {
            Self::Conversation { title, .. } => !title.is_empty() && title.len() <= 1024,
            Self::Attachment {
                name,
                media_type,
                data,
                ..
            } => {
                !name.is_empty()
                    && name.len() <= 256
                    && !name
                        .chars()
                        .any(|c| c.is_control() || c == '/' || c == '\\')
                    && matches!(
                        media_type.as_str(),
                        "image/png" | "image/jpeg" | "application/pdf" | "text/plain"
                    )
                    && !data.is_empty()
                    && data.len() <= 11184812
            }
            Self::Message {
                id,
                parent,
                role,
                text,
                attachments,
                ending,
                created_at_millis,
                ..
            } => {
                parent.as_ref() != Some(id)
                    && matches!(role.as_str(), "user" | "assistant")
                    && text.len() <= 1048576
                    && attachments.len() <= 4
                    && attachments
                        .iter()
                        .enumerate()
                        .all(|(i, a)| !attachments[..i].contains(a))
                    && matches!(
                        ending.as_str(),
                        "complete"
                            | "interrupted"
                            | "incomplete"
                            | "failed"
                            | "cancelled"
                            | "truncated"
                            | "invalid"
                    )
                    && *created_at_millis >= 0
            }
            Self::Delete { .. } => true,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetControl {
    pub protocol: u32,
    pub operation: Id,
    pub base_version: i64,
    pub enabled: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Push {
    pub protocol: u32,
    pub operation: Id,
    pub version: i64,
    pub entity: Entity,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pull {
    pub protocol: u32,
    pub version: i64,
    pub after: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub operation: Id,
    pub hash: String,
    pub sequence: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub after: i64,
    pub next: i64,
    pub entity: Option<Entity>,
}
