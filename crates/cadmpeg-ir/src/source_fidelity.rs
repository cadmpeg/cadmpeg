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

/// A decode report and source fidelity bound to exact CADIR bytes.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DecodeSidecar {
    /// SHA-256 of the exact CADIR bytes this sidecar describes.
    pub ir_sha256: String,
    /// Native decode transfer report.
    pub report: DecodeReport,
    /// Decode-time annotations and retained source records.
    pub fidelity: SourceFidelity,
}

/// Read shape of [`DecodeSidecar`], validated before it becomes one.
#[derive(Deserialize)]
struct DecodeSidecarWire {
    ir_sha256: String,
    report: DecodeReport,
    fidelity: SourceFidelity,
}

impl<'de> Deserialize<'de> for DecodeSidecar {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = DecodeSidecarWire::deserialize(deserializer)?;
        Self::from_wire(wire).map_err(serde::de::Error::custom)
    }
}

impl DecodeSidecar {
    fn from_wire(wire: DecodeSidecarWire) -> Result<Self, DecodeSidecarParseError> {
        wire.fidelity
            .validate()
            .map_err(DecodeSidecarParseError::Fidelity)?;
        Ok(Self {
            ir_sha256: wire.ir_sha256,
            report: wire.report,
            fidelity: wire.fidelity,
        })
    }

    /// Binds decode metadata to exact serialized CADIR bytes.
    pub fn bind(ir_bytes: &[u8], report: DecodeReport, fidelity: SourceFidelity) -> Self {
        Self::bind_sha256(crate::hash::sha256_hex(ir_bytes), report, fidelity)
    }

    /// Binds decode metadata to a previously computed CADIR SHA-256 digest.
    pub fn bind_sha256(
        ir_sha256: impl Into<String>,
        report: DecodeReport,
        mut fidelity: SourceFidelity,
    ) -> Self {
        fidelity.finalize();
        Self {
            ir_sha256: ir_sha256.into(),
            report,
            fidelity,
        }
    }

    /// Returns whether this sidecar is bound to the supplied CADIR bytes.
    pub fn matches(&self, ir_bytes: &[u8]) -> bool {
        self.ir_sha256 == crate::hash::sha256_hex(ir_bytes)
    }

    /// Finalizes this sidecar and serializes it as canonical compact JSON.
    pub fn to_canonical_json(&mut self) -> Result<String, serde_json::Error> {
        self.fidelity.finalize();
        serde_json::to_string(&*self)
    }

    /// Parses a decode sidecar and validates its retained records.
    pub fn from_json(text: &str) -> Result<Self, DecodeSidecarParseError> {
        let wire: DecodeSidecarWire =
            serde_json::from_str(text).map_err(DecodeSidecarParseError::Json)?;
        Self::from_wire(wire)
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
    /// Invalid source fidelity.
    #[error(transparent)]
    Fidelity(FidelityError),
}

/// Source bytes retained for native recovery or replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RetainedSourceRecord {
    /// Stable record identifier.
    id: String,
    /// Source stream containing the record.
    stream: String,
    /// First byte offset in the source stream.
    offset: u64,
    /// Number of source bytes.
    byte_len: u64,
    /// Lowercase hexadecimal SHA-256 of the source bytes.
    sha256: String,
    /// Retained bytes, when available.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::bytes::option"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
    data: Option<Vec<u8>>,
}

impl RetainedSourceRecord {
    /// Retains source bytes and derives their length and SHA-256 digest.
    #[must_use]
    pub fn retained(
        id: impl Into<String>,
        stream: impl Into<String>,
        offset: u64,
        data: Vec<u8>,
    ) -> Self {
        Self {
            id: id.into(),
            stream: stream.into(),
            offset,
            byte_len: data.len() as u64,
            sha256: crate::hash::sha256_hex(&data),
            data: Some(data),
        }
    }

    /// Records unavailable source bytes by their stored length and digest.
    #[must_use]
    pub fn unavailable(
        id: impl Into<String>,
        stream: impl Into<String>,
        offset: u64,
        byte_len: u64,
        sha256: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            stream: stream.into(),
            offset,
            byte_len,
            sha256: sha256.into(),
            data: None,
        }
    }

    fn from_unknown(stream: String, record: UnknownRecord) -> Self {
        let (id, offset, byte_len, sha256, data, _) = record.into_parts();
        match data {
            Some(data) => Self::retained(id.into_string(), stream, offset, data),
            None => Self::unavailable(id.into_string(), stream, offset, byte_len, sha256),
        }
    }

    /// Returns the stable record identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the source stream containing the record.
    #[must_use]
    pub fn stream(&self) -> &str {
        &self.stream
    }

    /// Returns the first byte offset in the source stream.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }

    /// Returns the number of source bytes.
    #[must_use]
    pub const fn byte_len(&self) -> u64 {
        self.byte_len
    }

    /// Returns the lowercase hexadecimal SHA-256 of the source bytes.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Returns the retained bytes when available.
    #[must_use]
    pub fn data(&self) -> Option<&[u8]> {
        self.data.as_deref()
    }
}

/// Validation failure in source metadata.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FidelityError {
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SourceFidelity {
    /// Sparse source locations and conversion exactness.
    #[serde(default)]
    pub annotations: Annotations,
    /// Native records retained for recovery or replay.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retained_records: Vec<RetainedSourceRecord>,
}

impl SourceFidelity {
    /// Fidelity carrying only the given annotations.
    pub fn with_annotations(annotations: Annotations) -> Self {
        Self {
            annotations,
            retained_records: Vec::new(),
        }
    }

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
        self.retained_records.extend(
            records
                .into_iter()
                .map(|record| RetainedSourceRecord::from_unknown(stream.into(), record)),
        );
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
        } = self;
        ir.set_native_unknowns_from(
            format,
            records.into_iter().map(|record| {
                let (id, offset, byte_len, sha256, data, links) = record.into_parts();
                let stream = annotations.provenance.get(id.as_str()).map_or_else(
                    || "source".into(),
                    |provenance| provenance.stream().to_owned(),
                );
                let product = crate::NativeUnknownRecord {
                    id: id.clone(),
                    links,
                };
                let retained = match data {
                    Some(data) => {
                        RetainedSourceRecord::retained(id.into_string(), stream, offset, data)
                    }
                    None => RetainedSourceRecord::unavailable(
                        id.into_string(),
                        stream,
                        offset,
                        byte_len,
                        sha256,
                    ),
                };
                retained_records.push(retained);
                product
            }),
        )
    }

    /// Validates retained record identity and payload integrity.
    pub fn validate(&self) -> Result<(), FidelityError> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, data: &[u8]) -> RetainedSourceRecord {
        RetainedSourceRecord::retained(id.to_owned(), "source", 0, data.to_vec())
    }

    fn report() -> DecodeReport {
        DecodeReport::unclassified(
            "test",
            crate::report::DecodeTransfer::full(true),
            std::collections::BTreeMap::default(),
            Vec::new(),
            Vec::new(),
            crate::report::TransferLedger::default(),
        )
    }

    #[test]
    fn finalize_orders_retained_records() {
        let mut sidecar = SourceFidelity {
            retained_records: vec![record("b", &[2]), record("a", &[1])],
            ..SourceFidelity::default()
        };
        sidecar.finalize();
        assert_eq!(sidecar.retained_records[0].id(), "a");
    }

    #[test]
    fn validation_rejects_false_payload_metadata() {
        let mut wire = serde_json::to_value(record("a", &[1, 2])).unwrap();
        wire["sha256"] = crate::hash::sha256_hex(&[2, 1]).into();
        let malformed = serde_json::from_value(wire).unwrap();
        let mut sidecar = SourceFidelity::default();
        sidecar.retained_records.push(malformed);
        assert!(matches!(
            sidecar.validate(),
            Err(FidelityError::Digest { .. })
        ));
    }

    #[test]
    fn decode_sidecar_binds_exact_ir_bytes() {
        let mut sidecar = DecodeSidecar::bind(b"cad-ir", report(), SourceFidelity::default());
        assert!(sidecar.matches(b"cad-ir"));
        assert!(!sidecar.matches(b"changed"));

        let digest_sidecar = DecodeSidecar::bind_sha256(
            crate::hash::sha256_hex(b"cad-ir"),
            sidecar.report.clone(),
            sidecar.fidelity.clone(),
        );
        assert_eq!(digest_sidecar, sidecar);

        let json = sidecar.to_canonical_json().expect("serialize sidecar");
        assert_eq!(DecodeSidecar::from_json(&json).unwrap(), sidecar);
    }

    #[test]
    fn decode_sidecar_uses_decode_report_dialect_omission_policy() {
        let mut sidecar = DecodeSidecar::bind(b"cad-ir", report(), SourceFidelity::default());
        let json = sidecar.to_canonical_json().expect("serialize sidecar");
        assert!(json.contains("\"dialects\":null"), "{json}");
        let truncated = json.replace(",\"dialects\":null", "");
        assert!(!truncated.contains("dialects"), "{truncated}");

        let parsed = DecodeSidecar::from_json(&truncated)
            .expect("the reusable decode-report wire accepts omitted dialects");
        assert!(parsed.report.dialects().is_none());
        assert_eq!(
            serde_json::from_str::<DecodeSidecar>(&truncated)
                .expect("direct sidecar deserialization uses the same policy"),
            parsed
        );
    }

    #[test]
    fn decode_sidecar_reports_a_missing_report_at_the_outer_boundary() {
        let sidecar = DecodeSidecar::bind(b"cad-ir", report(), SourceFidelity::default());
        let mut value = serde_json::to_value(sidecar).unwrap();
        value.as_object_mut().unwrap().remove("report");

        let error = DecodeSidecar::from_json(&value.to_string())
            .expect_err("a sidecar requires its report object");
        assert!(
            matches!(error, DecodeSidecarParseError::Json(ref error) if error.to_string().contains("missing field `report`")),
            "{error}"
        );
    }

    #[test]
    fn public_decode_sidecar_deserialization_validates_retained_payloads() {
        let mut fidelity = SourceFidelity::default();
        fidelity.retained_records.push(record("record", b"payload"));
        let sidecar = DecodeSidecar::bind(b"cad-ir", report(), fidelity);
        let mut value = serde_json::to_value(sidecar).unwrap();
        value["fidelity"]["retained_records"][0]["byte_len"] = 1.into();
        let json = value.to_string();

        let direct_error = serde_json::from_str::<DecodeSidecar>(&json)
            .expect_err("public deserialization must validate fidelity");
        assert!(
            direct_error
                .to_string()
                .contains("retained source record record declares 1 bytes but contains 7"),
            "{direct_error}"
        );
        assert!(matches!(
            DecodeSidecar::from_json(&json),
            Err(DecodeSidecarParseError::Fidelity(
                FidelityError::Length { .. }
            ))
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
