// SPDX-License-Identifier: Apache-2.0
//! Source annotations and retained native records produced during decode.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::annotations::Annotations;
use crate::document::CadIr;
use crate::native::NativeConvertError;
use crate::report::DecodeReport;
use crate::unknown::UnknownRecord;

/// Current serialized sidecar version.
pub const SOURCE_FIDELITY_VERSION: &str = "3";

/// Current serialized decode-sidecar version.
pub const DECODE_SIDECAR_VERSION: &str = "1";

/// A decode report and source fidelity bound to exact CADIR bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DecodeSidecar {
    /// Serialized sidecar format version.
    pub version: String,
    /// SHA-256 of the exact CADIR bytes this sidecar describes.
    pub ir_sha256: String,
    /// Native decode transfer report.
    pub report: DecodeReport,
    /// Decode-time annotations and retained source records.
    pub fidelity: SourceFidelity,
}

impl DecodeSidecar {
    /// Binds decode metadata to exact serialized CADIR bytes.
    pub fn bind(ir_bytes: &[u8], report: DecodeReport, mut fidelity: SourceFidelity) -> Self {
        fidelity.finalize();
        Self {
            version: DECODE_SIDECAR_VERSION.into(),
            ir_sha256: crate::hash::sha256_hex(ir_bytes),
            report,
            fidelity,
        }
    }

    /// Returns whether this sidecar is bound to the supplied CADIR bytes.
    pub fn matches(&self, ir_bytes: &[u8]) -> bool {
        self.ir_sha256 == crate::hash::sha256_hex(ir_bytes)
    }

    /// Serializes this sidecar as canonical compact JSON.
    pub fn to_canonical_json(&self) -> Result<String, serde_json::Error> {
        let mut canonical = self.clone();
        canonical.fidelity.finalize();
        serde_json::to_string(&canonical)
    }

    /// Parses and validates a decode sidecar.
    pub fn from_json(text: &str) -> Result<Self, DecodeSidecarParseError> {
        let sidecar: Self = serde_json::from_str(text).map_err(DecodeSidecarParseError::Json)?;
        if sidecar.version != DECODE_SIDECAR_VERSION {
            return Err(DecodeSidecarParseError::Version {
                found: sidecar.version,
            });
        }
        sidecar
            .fidelity
            .validate()
            .map_err(DecodeSidecarParseError::Fidelity)?;
        Ok(sidecar)
    }
}

/// Returns the sidecar path for a CADIR path.
pub fn decode_sidecar_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let stem = file_name.strip_suffix(".json").unwrap_or(file_name);
    path.with_file_name(format!("{stem}.fidelity.json"))
}

/// Failure parsing a decode sidecar.
#[derive(Debug, thiserror::Error)]
pub enum DecodeSidecarParseError {
    /// Invalid JSON.
    #[error("invalid decode-sidecar JSON: {0}")]
    Json(serde_json::Error),
    /// Unsupported sidecar version.
    #[error("unsupported decode-sidecar version: {found}")]
    Version {
        /// Version found in the sidecar.
        found: String,
    },
    /// Invalid source fidelity.
    #[error(transparent)]
    Fidelity(FidelityError),
}

/// Source bytes retained for native recovery or replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RetainedSourceRecord {
    /// Stable record identifier.
    pub id: String,
    /// Source stream containing the record.
    pub stream: String,
    /// First byte offset in the source stream.
    pub offset: u64,
    /// Number of source bytes.
    pub byte_len: u64,
    /// Lowercase hexadecimal SHA-256 of the source bytes.
    pub sha256: String,
    /// Retained bytes, when available.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::bytes::option"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
    pub data: Option<Vec<u8>>,
}

/// Validation failure in source metadata.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FidelityError {
    /// The sidecar declares an unsupported schema version.
    #[error("unsupported source-fidelity version: {found}")]
    Version {
        /// The version found in the sidecar.
        found: String,
    },
    /// Two retained records share an identifier.
    #[error("duplicate retained source record: {id}")]
    DuplicateRecord {
        /// The repeated identifier.
        id: String,
    },
    /// Retained data has the wrong length.
    #[error("retained source record {id} declares {declared} bytes but contains {actual}")]
    Length {
        /// The record identifier.
        id: String,
        /// Declared byte length.
        declared: u64,
        /// Actual retained byte length.
        actual: u64,
    },
    /// Retained data has the wrong digest.
    #[error("retained source record {id} does not match its SHA-256 digest")]
    Digest {
        /// The record identifier.
        id: String,
    },
}

/// Decode-time source annotations and retained native records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SourceFidelity {
    /// Serialized representation version.
    pub version: String,
    /// Sparse source locations and conversion exactness.
    #[serde(default)]
    pub annotations: Annotations,
    /// Native records retained for recovery or replay.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retained_records: Vec<RetainedSourceRecord>,
}

impl Default for SourceFidelity {
    fn default() -> Self {
        Self {
            version: SOURCE_FIDELITY_VERSION.into(),
            annotations: Annotations::default(),
            retained_records: Vec::new(),
        }
    }
}

impl SourceFidelity {
    /// Sorts retained records into canonical source order.
    pub fn finalize(&mut self) {
        self.retained_records.sort_by(|left, right| {
            (&left.stream, left.offset, &left.id).cmp(&(&right.stream, right.offset, &right.id))
        });
    }

    /// Finds a retained source record by identifier.
    pub fn retained_record(&self, id: &str) -> Option<&RetainedSourceRecord> {
        self.retained_records.iter().find(|record| record.id == id)
    }

    /// Retains source records without adding them to the product model.
    ///
    /// The records are consumed: their retained bytes move into the sidecar
    /// rather than being copied into it.
    pub fn retain_unknown_records(
        &mut self,
        stream: &str,
        records: impl IntoIterator<Item = UnknownRecord>,
    ) {
        self.retained_records
            .extend(records.into_iter().map(|record| RetainedSourceRecord {
                id: record.id.0,
                stream: stream.into(),
                offset: record.offset,
                byte_len: record.byte_len,
                sha256: record.sha256,
                data: record.data,
            }));
    }

    /// Stores source bytes in the sidecar and references in the product model.
    ///
    /// The records are consumed and split as they arrive: the retained bytes
    /// move into the sidecar and the identity and links move into the product
    /// arena, so neither the retained source image nor the derived product
    /// references are ever duplicated. A caller that still needs a record
    /// afterwards clones that record itself.
    pub fn attach_native_unknown_records(
        &mut self,
        ir: &mut CadIr,
        format: &str,
        records: impl IntoIterator<Item = UnknownRecord>,
    ) -> Result<(), NativeConvertError> {
        let Self {
            annotations,
            retained_records,
            ..
        } = self;
        ir.set_native_unknowns_from(
            format,
            records.into_iter().map(|record| {
                let stream = annotations
                    .provenance
                    .get(&record.id.0)
                    .and_then(|provenance| annotations.streams.get(provenance.stream as usize))
                    .cloned()
                    .unwrap_or_else(|| "source".into());
                let product = crate::NativeUnknownRecord {
                    id: record.id.clone(),
                    links: record.links,
                };
                retained_records.push(RetainedSourceRecord {
                    id: record.id.0,
                    stream,
                    offset: record.offset,
                    byte_len: record.byte_len,
                    sha256: record.sha256,
                    data: record.data,
                });
                product
            }),
        )
    }

    /// Validates retained record identity and payload integrity.
    pub fn validate(&self) -> Result<(), FidelityError> {
        if self.version != SOURCE_FIDELITY_VERSION {
            return Err(FidelityError::Version {
                found: self.version.clone(),
            });
        }
        let mut ids = std::collections::BTreeSet::new();
        for record in &self.retained_records {
            if !ids.insert(&record.id) {
                return Err(FidelityError::DuplicateRecord {
                    id: record.id.clone(),
                });
            }
            if let Some(data) = &record.data {
                let actual = data.len() as u64;
                if actual != record.byte_len {
                    return Err(FidelityError::Length {
                        id: record.id.clone(),
                        declared: record.byte_len,
                        actual,
                    });
                }
                if crate::hash::sha256_hex(data) != record.sha256 {
                    return Err(FidelityError::Digest {
                        id: record.id.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Parses and validates a sidecar.
    pub fn from_json(text: &str) -> Result<Self, SourceFidelityParseError> {
        let sidecar: Self = serde_json::from_str(text).map_err(SourceFidelityParseError::Json)?;
        sidecar
            .validate()
            .map_err(SourceFidelityParseError::Fidelity)?;
        Ok(sidecar)
    }
}

/// Failure parsing source metadata.
#[derive(Debug, thiserror::Error)]
pub enum SourceFidelityParseError {
    /// Invalid JSON.
    #[error("invalid source-fidelity JSON: {0}")]
    Json(serde_json::Error),
    /// Invalid source metadata.
    #[error(transparent)]
    Fidelity(FidelityError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, data: &[u8]) -> RetainedSourceRecord {
        RetainedSourceRecord {
            id: id.into(),
            stream: "source".into(),
            offset: 0,
            byte_len: data.len() as u64,
            sha256: crate::hash::sha256_hex(data),
            data: Some(data.to_vec()),
        }
    }

    #[test]
    fn finalize_orders_retained_records() {
        let mut sidecar = SourceFidelity {
            retained_records: vec![record("b", &[2]), record("a", &[1])],
            ..SourceFidelity::default()
        };
        sidecar.finalize();
        assert_eq!(sidecar.retained_records[0].id, "a");
    }

    #[test]
    fn validation_rejects_false_payload_metadata() {
        let mut sidecar = SourceFidelity::default();
        sidecar.retained_records.push(record("a", &[1, 2]));
        sidecar.retained_records[0].sha256 = crate::hash::sha256_hex(&[2, 1]);
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::Digest { .. })
        ));
    }

    #[test]
    fn decode_sidecar_binds_exact_ir_bytes_and_validates_versions() {
        let report = DecodeReport {
            format: "test".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::default(),
            transfer_ledger: crate::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };
        let sidecar = DecodeSidecar::bind(b"cad-ir", report, SourceFidelity::default());
        assert!(sidecar.matches(b"cad-ir"));
        assert!(!sidecar.matches(b"changed"));

        let json = sidecar.to_canonical_json().expect("serialize sidecar");
        assert_eq!(DecodeSidecar::from_json(&json).unwrap(), sidecar);
        let wrong_version = json.replacen("\"version\":\"1\"", "\"version\":\"2\"", 1);
        assert!(matches!(
            DecodeSidecar::from_json(&wrong_version),
            Err(DecodeSidecarParseError::Version { .. })
        ));
    }

    #[test]
    fn decode_sidecar_path_replaces_only_a_trailing_json_suffix() {
        assert_eq!(
            decode_sidecar_path(Path::new("part.cadir.json")),
            PathBuf::from("part.cadir.fidelity.json")
        );
        assert_eq!(
            decode_sidecar_path(Path::new("part.cadir")),
            PathBuf::from("part.cadir.fidelity.json")
        );
    }
}
