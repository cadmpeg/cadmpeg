// SPDX-License-Identifier: Apache-2.0
//! Decode-time source fidelity and byte-accounting sidecar.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::annotations::Annotations;
use crate::byte_ledger::ByteLedger;
use crate::document::CadIr;
use crate::native::NativeConvertError;
use crate::unknown::UnknownRecord;

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
    /// Retained bytes, when available. Opaque ledger recovery requires data.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::bytes::option"
    )]
    #[schemars(with = "Option<String>")]
    pub data: Option<Vec<u8>>,
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
    /// Find one retained source record by stable identifier.
    pub fn retained_record(&self, id: &str) -> Option<&RetainedSourceRecord> {
        self.retained_records.iter().find(|record| record.id == id)
    }

    /// Transfer source accounting from decoder records into this sidecar.
    pub fn retain_unknown_records(&mut self, stream: &str, records: &[UnknownRecord]) {
        self.retained_records
            .extend(records.iter().map(|record| RetainedSourceRecord {
                id: record.id.to_string(),
                stream: stream.into(),
                offset: record.offset,
                byte_len: record.byte_len,
                sha256: record.sha256.clone(),
                data: record.data.clone(),
            }));
    }

    /// Store source records in this sidecar and source-independent refs in the product model.
    pub fn attach_native_unknown_records(
        &mut self,
        ir: &mut CadIr,
        format: &str,
        records: &[UnknownRecord],
    ) -> Result<(), NativeConvertError> {
        self.retained_records.extend(records.iter().map(|record| {
            let stream = self
                .annotations
                .provenance
                .get(&record.id.0)
                .and_then(|provenance| self.annotations.streams.get(provenance.stream as usize))
                .cloned()
                .unwrap_or_else(|| "source".into());
            RetainedSourceRecord {
                id: record.id.to_string(),
                stream,
                offset: record.offset,
                byte_len: record.byte_len,
                sha256: record.sha256.clone(),
                data: record.data.clone(),
            }
        }));
        let product_records = records
            .iter()
            .map(crate::NativeUnknownRecord::from)
            .collect::<Vec<_>>();
        ir.set_native_unknowns(format, &product_records)
    }

    /// Join product links with retained records into a codec-local source view.
    pub fn native_unknown_records(
        &self,
        ir: &CadIr,
        format: &str,
    ) -> Result<Vec<UnknownRecord>, NativeConvertError> {
        ir.native_unknowns(format)?
            .into_iter()
            .map(|reference| {
                let retained = self.retained_record(&reference.id.0).ok_or_else(|| {
                    NativeConvertError::MissingRetainedSourceRecord(reference.id.0.clone())
                })?;
                Ok(UnknownRecord {
                    id: reference.id,
                    offset: retained.offset,
                    byte_len: retained.byte_len,
                    sha256: retained.sha256.clone(),
                    data: retained.data.clone(),
                    links: reference.links,
                })
            })
            .collect()
    }

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
    use crate::ids::UnknownId;
    use crate::{
        validate_with_source_fidelity, ByteSpan, ByteSpanClass, CadIr, Check, UnknownRecord,
    };

    #[test]
    fn sidecar_version_is_independent_and_explicit() {
        let value = serde_json::to_value(SourceFidelity::default()).expect("serialize sidecar");

        assert_eq!(value["schema_version"], SOURCE_FIDELITY_VERSION);
        assert!(value.get("ir_version").is_none());
    }

    #[test]
    fn transfers_unknown_source_accounting_without_product_fields() {
        let record = UnknownRecord {
            id: UnknownId("native:x#1".into()),
            offset: 7,
            byte_len: 3,
            sha256: crate::hash::sha256_hex(b"abc"),
            data: Some(b"abc".to_vec()),
            links: vec!["body:1".into()],
        };
        let mut sidecar = SourceFidelity::default();
        sidecar.retain_unknown_records("objects", std::slice::from_ref(&record));

        assert_eq!(sidecar.retained_records[0].stream, "objects");
        assert_eq!(sidecar.retained_records[0].offset, 7);
        let product = crate::NativeUnknownRecord::from(&record);
        let value = serde_json::to_value(product).expect("serialize product record");
        assert_eq!(value["links"][0], "body:1");
        assert!(value.get("offset").is_none());
        assert!(value.get("byte_len").is_none());
        assert!(value.get("sha256").is_none());
        assert!(value.get("data").is_none());

        let mut ir = CadIr::empty(crate::units::Units::default());
        ir.set_native_unknowns("native", &[crate::NativeUnknownRecord::from(&record)])
            .expect("store product record");
        assert_eq!(
            sidecar
                .native_unknown_records(&ir, "native")
                .expect("hydrate codec source view"),
            vec![record]
        );
    }

    #[test]
    fn source_view_joins_only_authoritative_product_refs() {
        let referenced = UnknownRecord {
            id: UnknownId("native:object#1".into()),
            offset: 7,
            byte_len: 3,
            sha256: crate::hash::sha256_hex(b"abc"),
            data: Some(b"abc".to_vec()),
            links: Vec::new(),
        };
        let source_only = UnknownRecord {
            id: UnknownId("native:file:source-image#0".into()),
            offset: 0,
            byte_len: 6,
            sha256: crate::hash::sha256_hex(b"source"),
            data: Some(b"source".to_vec()),
            links: Vec::new(),
        };
        let mut ir = CadIr::empty(crate::units::Units::default());
        ir.set_native_unknowns("native", &[crate::NativeUnknownRecord::from(&referenced)])
            .expect("store product ref");
        let mut sidecar = SourceFidelity::default();
        sidecar.retain_unknown_records("native", &[referenced.clone(), source_only]);

        assert_eq!(
            sidecar.native_unknown_records(&ir, "native").unwrap(),
            vec![referenced]
        );
    }

    #[test]
    fn source_view_rejects_a_product_ref_without_retained_bytes() {
        let mut ir = CadIr::empty(crate::units::Units::default());
        ir.set_native_unknowns(
            "native",
            &[crate::NativeUnknownRecord {
                id: UnknownId("native:missing#1".into()),
                links: Vec::new(),
            }],
        )
        .expect("store product ref");

        assert!(matches!(
            SourceFidelity::default().native_unknown_records(&ir, "native"),
            Err(NativeConvertError::MissingRetainedSourceRecord(id))
                if id == "native:missing#1"
        ));
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
                data: Some(data.to_vec()),
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

    #[test]
    fn rejects_opaque_recovery_without_retained_bytes() {
        let mut sidecar = recovery_sidecar(b"opaque");
        sidecar.retained_records[0].data = None;
        let report = validate_with_source_fidelity(
            &CadIr::empty(crate::units::Units::default()),
            &sidecar,
            Vec::new(),
        );

        assert!(report.findings.iter().any(|finding| {
            finding.check == Check::ByteAccounting
                && finding.message.contains("has no recovery bytes")
        }));
    }

    #[test]
    fn byte_ledger_validation_rejects_a_gap() {
        let sidecar = SourceFidelity {
            byte_ledger: ByteLedger {
                source_length: 4,
                spans: vec![ByteSpan {
                    start: 1,
                    end: 4,
                    class: ByteSpanClass::Structural,
                    owner: "stream".into(),
                    meaning: "payload".into(),
                    retained_record: None,
                }],
            },
            ..SourceFidelity::default()
        };

        let findings = validate_with_source_fidelity(
            &CadIr::empty(crate::units::Units::default()),
            &sidecar,
            Vec::new(),
        )
        .findings;
        assert!(findings.iter().any(|finding| {
            finding.check == Check::ByteAccounting
                && finding.message == "byte ledger has a gap before offset 1"
        }));
    }

    #[test]
    fn finalize_coalesces_equivalent_adjacent_byte_spans() {
        let mut sidecar = SourceFidelity {
            byte_ledger: ByteLedger {
                source_length: 4,
                spans: vec![
                    ByteSpan {
                        start: 0,
                        end: 2,
                        class: ByteSpanClass::Typed,
                        owner: "card:1".into(),
                        meaning: "data".into(),
                        retained_record: None,
                    },
                    ByteSpan {
                        start: 2,
                        end: 4,
                        class: ByteSpanClass::Typed,
                        owner: "card:1".into(),
                        meaning: "data".into(),
                        retained_record: None,
                    },
                ],
            },
            ..SourceFidelity::default()
        };

        sidecar.finalize();

        assert_eq!(sidecar.byte_ledger.spans.len(), 1);
        assert_eq!(sidecar.byte_ledger.spans[0].start, 0);
        assert_eq!(sidecar.byte_ledger.spans[0].end, 4);
    }

    #[test]
    fn validates_sidecar_annotations_against_product_entities() {
        let mut sidecar = SourceFidelity::default();
        sidecar.annotations.provenance.insert(
            "missing:entity#0".into(),
            crate::Provenance {
                stream: 0,
                offset: 0,
                tag: None,
            },
        );
        let report = validate_with_source_fidelity(
            &CadIr::empty(crate::units::Units::default()),
            &sidecar,
            Vec::new(),
        );

        assert!(report.findings.iter().any(|finding| {
            finding.check == Check::Annotations
                && finding.message == "provenance key does not resolve to an entity"
        }));
    }

    #[test]
    fn sidecar_annotations_may_name_source_only_retained_records() {
        let mut sidecar = SourceFidelity {
            retained_records: vec![RetainedSourceRecord {
                id: "source:record#1".into(),
                stream: "source".into(),
                offset: 0,
                byte_len: 1,
                sha256: crate::hash::sha256_hex(b"x"),
                data: Some(b"x".to_vec()),
            }],
            ..SourceFidelity::default()
        };
        sidecar.annotations.streams.push("source".into());
        sidecar.annotations.provenance.insert(
            "source:record#1".into(),
            crate::Provenance {
                stream: 0,
                offset: 0,
                tag: None,
            },
        );

        let report = validate_with_source_fidelity(
            &CadIr::empty(crate::units::Units::default()),
            &sidecar,
            Vec::new(),
        );

        assert!(!report.findings.iter().any(|finding| {
            finding.check == Check::Annotations
                && finding.entity.as_deref() == Some("source:record#1")
        }));
    }
}
