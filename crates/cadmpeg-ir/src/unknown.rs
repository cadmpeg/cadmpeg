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
            id: record.id().clone(),
            links: record.links().to_vec(),
        }
    }
}

/// A recognized source record represented by location, digest, links, and
/// optional retained bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct UnknownRecord {
    /// Arena id.
    id: UnknownId,
    /// Byte offset of the record within its source stream.
    offset: u64,
    /// Byte length of the record's span.
    byte_len: u64,
    /// Lowercase hex SHA-256 of the record bytes, for integrity and dedup.
    sha256: String,
    /// Preserved record bytes, when retained by the decoder.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::bytes::option"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
    data: Option<Vec<u8>>,
    /// Related entity IDs from any document arena.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    links: Vec<String>,
}

#[derive(Deserialize)]
struct UnknownRecordWire {
    id: UnknownId,
    offset: u64,
    byte_len: u64,
    sha256: String,
    #[serde(default, with = "crate::bytes::option")]
    data: Option<Vec<u8>>,
    #[serde(default)]
    links: Vec<String>,
}

impl UnknownRecord {
    /// Retains source bytes and derives their length and SHA-256 digest.
    #[must_use]
    pub fn retained(id: UnknownId, offset: u64, data: Vec<u8>, links: Vec<String>) -> Self {
        Self {
            id,
            offset,
            byte_len: data.len() as u64,
            sha256: crate::hash::sha256_hex(&data),
            data: Some(data),
            links,
        }
    }

    /// Records unavailable source bytes by their measured length and digest.
    #[must_use]
    pub fn unavailable(
        id: UnknownId,
        offset: u64,
        byte_len: u64,
        sha256: impl Into<String>,
        links: Vec<String>,
    ) -> Self {
        Self {
            id,
            offset,
            byte_len,
            sha256: sha256.into(),
            data: None,
            links,
        }
    }

    fn from_wire(wire: UnknownRecordWire) -> Self {
        Self {
            id: wire.id,
            offset: wire.offset,
            byte_len: wire.byte_len,
            sha256: wire.sha256,
            data: wire.data,
            links: wire.links,
        }
    }

    pub(crate) fn into_parts(self) -> (UnknownId, u64, u64, String, Option<Vec<u8>>, Vec<String>) {
        (
            self.id,
            self.offset,
            self.byte_len,
            self.sha256,
            self.data,
            self.links,
        )
    }

    /// Returns the arena id.
    #[must_use]
    pub fn id(&self) -> &UnknownId {
        &self.id
    }

    /// Replaces the arena id during namespace composition.
    pub fn set_id(&mut self, id: UnknownId) {
        self.id = id;
    }

    /// Returns the byte offset within the source stream.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }

    /// Returns the byte length of the record span.
    #[must_use]
    pub const fn byte_len(&self) -> u64 {
        self.byte_len
    }

    /// Returns the lowercase hexadecimal SHA-256 of the record bytes.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Returns the retained bytes when available.
    #[must_use]
    pub fn data(&self) -> Option<&[u8]> {
        self.data.as_deref()
    }

    /// Retains source bytes and replaces their measured length and digest with
    /// values derived from those bytes.
    pub fn retain_data(&mut self, data: Vec<u8>) {
        self.byte_len = data.len() as u64;
        self.sha256 = crate::hash::sha256_hex(&data);
        self.data = Some(data);
    }

    /// Returns the related entity IDs.
    #[must_use]
    pub fn links(&self) -> &[String] {
        &self.links
    }

    /// Returns the related entity IDs for reference resolution.
    #[must_use]
    pub fn links_mut(&mut self) -> &mut Vec<String> {
        &mut self.links
    }
}

impl<'de> Deserialize<'de> for UnknownRecord {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        UnknownRecordWire::deserialize(deserializer).map(Self::from_wire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_record_derives_extent_and_digest() {
        let record = UnknownRecord::retained(
            UnknownId("synthetic:unknown#0".into()),
            7,
            vec![1, 2, 3],
            vec!["synthetic:point#0".into()],
        );

        assert_eq!(record.byte_len(), 3);
        assert_eq!(record.sha256(), crate::hash::sha256_hex(&[1, 2, 3]));
        assert_eq!(record.data(), Some([1, 2, 3].as_slice()));
    }

    #[test]
    fn deserialization_preserves_stored_extent_and_digest() {
        let wire = serde_json::json!({
            "id": "synthetic:unknown#0",
            "offset": 7,
            "byte_len": 99,
            "sha256": "wire-value",
            "data": "AQID",
            "links": ["synthetic:point#0"]
        });

        let record: UnknownRecord =
            serde_json::from_value(wire.clone()).expect("deserialize unknown-record wire");

        assert_eq!(record.byte_len(), 99);
        assert_eq!(record.sha256(), "wire-value");
        assert_eq!(
            serde_json::to_value(record).expect("serialize unknown-record wire"),
            wire
        );
    }
}
