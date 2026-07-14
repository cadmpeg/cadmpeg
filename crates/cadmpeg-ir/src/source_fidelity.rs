// SPDX-License-Identifier: Apache-2.0
//! Decode-time source fidelity and byte-accounting sidecar.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::annotations::Annotations;
use crate::byte_ledger::ByteLedger;

/// Source bytes retained to support exact recovery of opaque ledger spans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RetainedSourceRecord {
    /// Stable identifier named by opaque byte-ledger spans.
    pub id: String,
    /// Source stream containing the retained range.
    pub stream: String,
    /// First byte offset in the source stream.
    pub offset: u64,
    /// Number of retained bytes.
    pub byte_len: u64,
    /// Lowercase hexadecimal SHA-256 of `data`.
    pub sha256: String,
    /// Retained bytes. Recovery records always contain data.
    #[serde(with = "crate::bytes")]
    #[schemars(with = "String")]
    pub data: Vec<u8>,
}

/// Source-fidelity schema version produced by this build.
pub const SOURCE_FIDELITY_VERSION: &str = "1";

/// Source-byte accounting and conversion facts accompanying one decoded IR.
///
/// This value is not part of the neutral product schema. Its version evolves
/// independently from [`crate::IR_VERSION`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceFidelity {
    /// Independently versioned sidecar schema.
    pub schema_version: String,
    /// Complete ownership of the decoded source byte stream.
    pub byte_ledger: ByteLedger,
    /// Sparse source locations and conversion exactness.
    #[serde(default)]
    pub annotations: Annotations,
    /// Opaque source ranges retained for exact recovery.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retained_records: Vec<RetainedSourceRecord>,
}

impl Default for SourceFidelity {
    fn default() -> Self {
        Self {
            schema_version: SOURCE_FIDELITY_VERSION.into(),
            byte_ledger: ByteLedger::default(),
            annotations: Annotations::default(),
            retained_records: Vec::new(),
        }
    }
}

impl SourceFidelity {
    /// Canonicalize sidecar collections independently from the product model.
    pub fn finalize(&mut self) {
        self.byte_ledger.finalize();
        self.retained_records.sort_by(|left, right| {
            (&left.stream, left.offset, &left.id).cmp(&(&right.stream, right.offset, &right.id))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{validate_with_source_fidelity, ByteSpan, ByteSpanClass, CadIr, Check};

    #[test]
    fn sidecar_version_is_independent_and_explicit() {
        let value = serde_json::to_value(SourceFidelity::default()).expect("serialize sidecar");

        assert_eq!(value["schema_version"], SOURCE_FIDELITY_VERSION);
        assert!(value.get("ir_version").is_none());
    }

    fn recovery_sidecar(data: &[u8]) -> SourceFidelity {
        SourceFidelity {
            byte_ledger: ByteLedger {
                source_length: data.len() as u64,
                spans: vec![ByteSpan {
                    start: 0,
                    end: data.len() as u64,
                    class: ByteSpanClass::Opaque,
                    owner: "decoder".into(),
                    meaning: "untyped source bytes".into(),
                    retained_record: Some("source:opaque#1".into()),
                }],
            },
            retained_records: vec![RetainedSourceRecord {
                id: "source:opaque#1".into(),
                stream: "source".into(),
                offset: 0,
                byte_len: data.len() as u64,
                sha256: crate::hash::sha256_hex(data),
                data: data.to_vec(),
            }],
            ..SourceFidelity::default()
        }
    }

    #[test]
    fn validates_complete_opaque_recovery() {
        let report = validate_with_source_fidelity(
            &CadIr::empty(crate::units::Units::default()),
            &recovery_sidecar(b"opaque"),
            Vec::new(),
        );

        assert!(!report
            .findings
            .iter()
            .any(|finding| finding.check == Check::ByteAccounting));
    }

    #[test]
    fn rejects_recovery_record_range_and_digest_disagreement() {
        let mut sidecar = recovery_sidecar(b"opaque");
        sidecar.retained_records[0].offset = 1;
        sidecar.retained_records[0].sha256 = "0".repeat(64);
        let report = validate_with_source_fidelity(
            &CadIr::empty(crate::units::Units::default()),
            &sidecar,
            Vec::new(),
        );
        let messages = report
            .findings
            .iter()
            .filter(|finding| finding.check == Check::ByteAccounting)
            .map(|finding| finding.message.as_str())
            .collect::<Vec<_>>();

        assert!(messages
            .iter()
            .any(|message| message.contains("ranges disagree")));
        assert!(messages
            .iter()
            .any(|message| message.contains("digest disagrees")));
    }
}
