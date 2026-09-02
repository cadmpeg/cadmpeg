// SPDX-License-Identifier: Apache-2.0
//! Retained source records without a typed IR interpretation.
#![deny(clippy::disallowed_methods)]

use crate::ids::UnknownId;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A format-specific product record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct NativeUnknownRecord {
    /// Arena id.
    pub id: UnknownId,
    /// Related entity IDs from any document arena.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
}

impl From<&UnknownRecord> for NativeUnknownRecord {
    fn from(record: &UnknownRecord) -> Self {
        Self {
            id: record.id.clone(),
            links: record.links.clone(),
        }
    }
}

/// A recognized source record represented by location, digest, links, and
/// optional retained bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct UnknownRecord {
    /// Arena id.
    pub id: UnknownId,
    /// Byte offset of the record within its source stream.
    pub offset: u64,
    /// Byte length of the record's span.
    pub byte_len: u64,
    /// Lowercase hex SHA-256 of the record bytes, for integrity and dedup.
    pub sha256: String,
    /// Preserved record bytes, when retained by the decoder.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::bytes::option"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
    pub data: Option<Vec<u8>>,
    /// Related entity IDs from any document arena.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
}
