// SPDX-License-Identifier: Apache-2.0
//! Retained source records without a typed IR interpretation.
#![deny(clippy::disallowed_methods)]

use crate::ids::UnknownId;
use crate::source_fidelity::RetainedBytes;
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

/// A recognized source record represented by location, retained image, and
/// links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct UnknownRecord {
    /// Arena id.
    id: UnknownId,
    /// Byte offset of the record within its source stream.
    offset: u64,
    /// Retained image of the record bytes: the bytes themselves, or the
    /// length and digest of bytes that are not retained. One or the other,
    /// never both, so no record can state an extent or a digest that
    /// contradicts the bytes beside it.
    retention: RetainedBytes,
    /// Related entity IDs from any document arena.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    links: Vec<String>,
}

impl UnknownRecord {
    /// Retains source bytes and derives their length and SHA-256 digest.
    #[must_use]
    pub fn retained(id: UnknownId, offset: u64, data: Vec<u8>, links: Vec<String>) -> Self {
        Self {
            id,
            offset,
            retention: RetainedBytes::Inline { data },
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
            retention: RetainedBytes::Digest {
                byte_len,
                sha256: sha256.into(),
            },
            links,
        }
    }

    pub(crate) fn into_parts(self) -> (UnknownId, u64, RetainedBytes, Vec<String>) {
        (self.id, self.offset, self.retention, self.links)
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
    pub fn byte_len(&self) -> u64 {
        self.retention.byte_len()
    }

    /// Returns the lowercase hexadecimal SHA-256 of the record bytes.
    #[must_use]
    pub fn sha256(&self) -> String {
        self.retention.sha256()
    }

    /// Returns the retained bytes when available.
    #[must_use]
    pub fn data(&self) -> Option<&[u8]> {
        self.retention.data()
    }

    /// Retains source bytes. Their length and digest become functions of them.
    pub fn retain_data(&mut self, data: Vec<u8>) {
        self.retention = RetainedBytes::Inline { data };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_record_derives_extent_and_digest() {
        let record = UnknownRecord::retained(
            UnknownId::mint("synthetic:model:unknown#0").expect("valid identity"),
            7,
            vec![1, 2, 3],
            vec!["synthetic:model:point#0".into()],
        );

        assert_eq!(record.byte_len(), 3);
        assert_eq!(record.sha256(), crate::hash::sha256_hex(&[1, 2, 3]));
        assert_eq!(record.data(), Some([1, 2, 3].as_slice()));
    }

    #[test]
    fn an_unretained_record_states_its_extent_and_digest() {
        let wire = serde_json::json!({
            "id": "synthetic:model:unknown#0",
            "offset": 7,
            "retention": {
                "retention": "digest",
                "byte_len": 99,
                "sha256": "wire-value"
            },
            "links": ["synthetic:model:point#0"]
        });

        let record: UnknownRecord =
            serde_json::from_value(wire.clone()).expect("deserialize unknown-record wire");

        assert_eq!(record.byte_len(), 99);
        assert_eq!(record.sha256(), "wire-value");
        assert_eq!(record.data(), None);
        assert_eq!(
            serde_json::to_value(record).expect("serialize unknown-record wire"),
            wire
        );
    }

    /// An extent or a digest beside the retained bytes has no place on the
    /// wire, and an unknown key is refused rather than dropped.
    #[test]
    fn a_retained_record_cannot_restate_its_extent_or_digest() {
        let wire = serde_json::json!({
            "id": "synthetic:model:unknown#0",
            "offset": 7,
            "retention": {"retention": "inline", "data": "AQID"}
        });
        let record: UnknownRecord =
            serde_json::from_value(wire.clone()).expect("deserialize unknown-record wire");
        assert_eq!(record.byte_len(), 3);
        assert_eq!(record.sha256(), crate::hash::sha256_hex(&[1, 2, 3]));
        assert_eq!(
            serde_json::to_value(record).expect("serialize unknown-record wire"),
            wire
        );

        for (key, value) in [
            ("byte_len", serde_json::json!(99)),
            ("sha256", serde_json::json!("not-the-digest-of-the-bytes")),
        ] {
            let mut restated = wire.clone();
            restated["retention"][key] = value;
            let error = serde_json::from_value::<UnknownRecord>(restated)
                .expect_err("the retained bytes own their extent and digest")
                .to_string();
            assert!(error.contains(key), "{error}");
        }

        let mut bogus = wire;
        bogus["zz_bogus"] = serde_json::json!(true);
        let error = serde_json::from_value::<UnknownRecord>(bogus)
            .expect_err("an unknown key is refused")
            .to_string();
        assert!(error.contains("zz_bogus"), "{error}");
    }
}
