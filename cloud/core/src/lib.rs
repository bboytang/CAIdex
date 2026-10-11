//! Versioned cloud data contracts. Account identity never grants Host authority.
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const PROTOCOL_VERSION: u32 = 1;
pub const ACCOUNT_MIGRATION: &str =
    include_str!("../../postgres/migrations/0001_account_scope.sql");

#[derive(Debug, thiserror::Error)]
#[error("Invalid cloud identifier or version")]
pub struct Invalid;

/// Canonical PostgreSQL UUID; construction and deserialization share validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id(String);

impl TryFrom<String> for Id {
    type Error = Invalid;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 36
            || !value.bytes().enumerate().all(|(i, b)| {
                if matches!(i, 8 | 13 | 18 | 23) {
                    b == b'-'
                } else {
                    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
                }
            })
        {
            return Err(Invalid);
        }
        Ok(Self(value))
    }
}

impl From<Id> for String {
    fn from(value: Id) -> Self {
        value.0
    }
}

/// Positive versions fit PostgreSQL bigint and never use client clocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub struct Version(i64);

impl TryFrom<i64> for Version {
    type Error = Invalid;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if value <= 0 {
            return Err(Invalid);
        }
        Ok(Self(value))
    }
}

impl From<Version> for i64 {
    fn from(value: Version) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    Personal { user_id: Id },
    Project { owner_user_id: Id, project_id: Id },
}

/// Server confirmation only; local pending intent and upload consent are separate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncControl {
    pub enabled: bool,
    pub settings_version: Version,
    pub cloud_epoch: Version,
    pub scope_version: Version,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingStop {
    KeepCloud,
    DeleteCloud,
}

/// A device's upload consent does not enable server sync or clear a pending stop.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadConsent {
    pub user_id: Id,
    pub scope_version: Version,
    pub scopes: Vec<Scope>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_versions_reject_invalid_wire_values() {
        for id in ["", "other-account", "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA"] {
            assert!(Id::try_from(id.to_owned()).is_err());
            assert!(serde_json::from_value::<Id>(serde_json::json!(id)).is_err());
        }
        for version in [
            serde_json::json!(0),
            serde_json::json!(-1),
            serde_json::json!(u64::MAX),
        ] {
            assert!(serde_json::from_value::<Version>(version).is_err());
        }
        let id = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        let parsed = Id::try_from(id.to_owned()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), id);
    }

    #[test]
    fn scope_and_control_do_not_accept_privilege_or_secret_fields() {
        let scope = serde_json::json!({"kind":"personal", "user_id":"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"});
        let decoded: Scope = serde_json::from_value(scope.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), scope);
        let mut forged = scope;
        forged["host_execute"] = true.into();
        assert!(serde_json::from_value::<Scope>(forged).is_err());
        let mut control = serde_json::json!({"enabled":false,"settings_version":1,"cloud_epoch":1,"scope_version":1});
        assert!(serde_json::from_value::<SyncControl>(control.clone()).is_ok());
        control["token"] = "secret".into();
        assert!(serde_json::from_value::<SyncControl>(control).is_err());
        assert_eq!(SCHEMA_VERSION, 1);
        assert!(
            ACCOUNT_MIGRATION.contains("INSERT INTO caidex.schema_migrations(version) VALUES (1)")
        );
    }
}
