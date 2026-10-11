use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{Error, Result, random_id};

pub(crate) const OWNER: &str = "owner";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Observe,
    Execute,
    Approve,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub client_id: String,
    pub digest: String,
    pub scopes: Vec<Scope>,
    pub revoked: bool,
}

impl Grant {
    pub(crate) fn issue(id: String, scopes: Vec<Scope>) -> Result<(Self, String)> {
        if id == OWNER
            || id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || scopes.is_empty()
            || scopes.len() > 3
        {
            return Err(Error::Refused("invalid Host client/scopes"));
        }
        let token = format!("{}{}", random_id()?, random_id()?);
        let digest = format!("{:x}", Sha256::digest(token.as_bytes()));
        Ok((
            Self {
                client_id: id,
                digest,
                scopes,
                revoked: false,
            },
            token,
        ))
    }
}

pub(crate) fn authenticate(
    token: &str,
    owner: &str,
    clients: &BTreeMap<String, Grant>,
) -> Option<String> {
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    if bool::from(token.as_bytes().ct_eq(owner.as_bytes())) {
        return Some(OWNER.into());
    }
    let digest = format!("{:x}", Sha256::digest(token.as_bytes()));
    clients
        .values()
        .find(|grant| {
            !grant.revoked && bool::from(digest.as_bytes().ct_eq(grant.digest.as_bytes()))
        })
        .map(|grant| grant.client_id.clone())
}

pub(crate) fn permits(clients: &BTreeMap<String, Grant>, id: &str, scope: Option<Scope>) -> bool {
    id == OWNER
        || clients.get(id).is_some_and(|grant| {
            !grant.revoked && scope.is_some_and(|scope| grant.scopes.contains(&scope))
        })
}
