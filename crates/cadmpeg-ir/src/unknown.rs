// SPDX-License-Identifier: Apache-2.0
//! Retained source records without a typed IR interpretation.
#![deny(clippy::disallowed_methods)]

use crate::ids::UnknownId;
use crate::native::NativeRecord;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

/// A format-specific product record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    #[schemars(with = "Option<String>")]
    pub data: Option<Vec<u8>>,
    /// Related entity IDs from any document arena.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
}

impl UnknownRecord {
    pub(crate) fn into_native_record(self) -> NativeRecord {
        let mut fields = Map::new();
        fields.insert("offset".into(), Value::Number(Number::from(self.offset)));
        fields.insert(
            "byte_len".into(),
            Value::Number(Number::from(self.byte_len)),
        );
        fields.insert("sha256".into(), Value::String(self.sha256));
        if let Some(data) = self.data {
            fields.insert("data".into(), Value::String(STANDARD.encode(data)));
        }
        if !self.links.is_empty() {
            fields.insert(
                "links".into(),
                Value::Array(self.links.into_iter().map(Value::String).collect()),
            );
        }
        NativeRecord {
            id: self.id.0,
            fields,
        }
    }
}
