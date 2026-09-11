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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DecodeSidecar {
    /// SHA-256 of the exact CADIR bytes this sidecar describes.
    pub ir_sha256: String,
    /// Native decode transfer report.
    pub report: DecodeReport,
    /// Decode-time annotations and retained source records.
    pub fidelity: SourceFidelity,
}

impl DecodeSidecar {
    /// Binds decode metadata to exact serialized CADIR bytes.
    pub fn bind(ir_bytes: &[u8], report: DecodeReport, fidelity: SourceFidelity) -> Self {
        Self::bind_sha256(crate::hash::sha256_hex(ir_bytes), report, fidelity)
    }

    /// Binds decode metadata to a previously computed CADIR SHA-256 digest.
    pub fn bind_sha256(
        ir_sha256: impl Into<String>,
        report: DecodeReport,
        fidelity: SourceFidelity,
    ) -> Self {
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

    /// Serializes this sidecar as canonical compact JSON.
    pub fn to_canonical_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Parses a decode sidecar.
    pub fn from_json(text: &str) -> Result<Self, DecodeSidecarParseError> {
        serde_json::from_str(text).map_err(DecodeSidecarParseError::Json)
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
}

/// The retained image of a source record: its bytes, or the length and digest
/// of bytes that are not retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "retention", rename_all = "snake_case", deny_unknown_fields)]
pub enum RetainedBytes {
    /// The source bytes themselves. Their length and digest are functions of
    /// them, so neither is stored.
    Inline {
        /// Retained source bytes.
        #[serde(with = "crate::bytes")]
        #[cfg_attr(feature = "schema", schemars(with = "String"))]
        data: Vec<u8>,
    },
    /// Unavailable source bytes, described by their length and digest.
    Digest {
        /// Number of source bytes.
        byte_len: u64,
        /// Lowercase hexadecimal SHA-256 of the source bytes.
        sha256: String,
    },
}

/// Source bytes retained for native recovery or replay.
///
/// The record is addressed by its id in
/// [`SourceFidelity::retained_records`], so it carries no id of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RetainedSourceRecord {
    /// Source stream containing the record.
    stream: String,
    /// First byte offset in the source stream.
    offset: u64,
    /// Retained image of the source bytes.
    bytes: RetainedBytes,
}

impl RetainedSourceRecord {
    /// Retains source bytes.
    #[must_use]
    pub fn retained(stream: impl Into<String>, offset: u64, data: Vec<u8>) -> Self {
        Self {
            stream: stream.into(),
            offset,
            bytes: RetainedBytes::Inline { data },
        }
    }

    /// Records unavailable source bytes by their stored length and digest.
    #[must_use]
    pub fn unavailable(
        stream: impl Into<String>,
        offset: u64,
        byte_len: u64,
        sha256: impl Into<String>,
    ) -> Self {
        Self {
            stream: stream.into(),
            offset,
            bytes: RetainedBytes::Digest {
                byte_len,
                sha256: sha256.into(),
            },
        }
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
    pub fn byte_len(&self) -> u64 {
        match &self.bytes {
            RetainedBytes::Inline { data } => data.len() as u64,
            RetainedBytes::Digest { byte_len, .. } => *byte_len,
        }
    }

    /// Returns the lowercase hexadecimal SHA-256 of the source bytes.
    #[must_use]
    pub fn sha256(&self) -> String {
        match &self.bytes {
            RetainedBytes::Inline { data } => crate::hash::sha256_hex(data),
            RetainedBytes::Digest { sha256, .. } => sha256.clone(),
        }
    }

    /// Returns the retained bytes when available.
    #[must_use]
    pub fn data(&self) -> Option<&[u8]> {
        match &self.bytes {
            RetainedBytes::Inline { data } => Some(data),
            RetainedBytes::Digest { .. } => None,
        }
    }
}

/// Decode-time source annotations and retained native records.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SourceFidelity {
    /// Sparse source locations and conversion exactness.
    #[serde(default)]
    pub annotations: Annotations,
    /// Native records retained for recovery or replay, keyed by record id.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub retained_records: std::collections::BTreeMap<String, RetainedSourceRecord>,
}

impl SourceFidelity {
    /// Fidelity carrying only the given annotations.
    pub fn with_annotations(annotations: Annotations) -> Self {
        Self {
            annotations,
            retained_records: std::collections::BTreeMap::new(),
        }
    }

    /// Finds a retained source record by identifier.
    pub fn retained_record(&self, id: &str) -> Option<&RetainedSourceRecord> {
        self.retained_records.get(id)
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
        for record in records {
            let (id, offset, byte_len, sha256, data, _) = record.into_parts();
            self.retained_records.insert(
                id.into_string(),
                retained_source_record(stream.into(), offset, byte_len, sha256, data),
            );
        }
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
                retained_records.insert(
                    id.into_string(),
                    retained_source_record(stream, offset, byte_len, sha256, data),
                );
                product
            }),
        )
    }
}

fn retained_source_record(
    stream: String,
    offset: u64,
    byte_len: u64,
    sha256: String,
    data: Option<Vec<u8>>,
) -> RetainedSourceRecord {
    data.map_or_else(
        || RetainedSourceRecord::unavailable(stream.clone(), offset, byte_len, sha256.clone()),
        |data| RetainedSourceRecord::retained(stream.clone(), offset, data),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(data: &[u8]) -> RetainedSourceRecord {
        RetainedSourceRecord::retained("source", 0, data.to_vec())
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
    fn retained_records_are_keyed_by_their_id() {
        let mut sidecar = SourceFidelity::default();
        sidecar.retained_records.insert("b".into(), record(&[2]));
        sidecar.retained_records.insert("a".into(), record(&[1]));
        assert_eq!(
            sidecar.retained_records.keys().collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(sidecar.retained_record("a"), Some(&record(&[1])));

        // A record id is a map key, so a document cannot carry two records
        // under one id: serde_json keeps the last value for a repeated key.
        let duplicated = serde_json::from_str::<SourceFidelity>(
            r#"{"retained_records":{
                 "a":{"stream":"source","offset":0,
                      "bytes":{"retention":"inline","data":"AQ=="}},
                 "a":{"stream":"source","offset":0,
                      "bytes":{"retention":"inline","data":"Ag=="}}}}"#,
        )
        .expect("a repeated key is not a parse error");
        assert_eq!(duplicated.retained_records.len(), 1);
        assert_eq!(
            duplicated
                .retained_record("a")
                .and_then(RetainedSourceRecord::data),
            Some([2].as_slice())
        );
    }

    #[test]
    fn a_retained_record_derives_its_length_and_digest_from_inline_bytes() {
        let inline = record(&[1, 2, 3]);
        assert_eq!(inline.byte_len(), 3);
        assert_eq!(inline.sha256(), crate::hash::sha256_hex(&[1, 2, 3]));
        assert_eq!(inline.data(), Some([1, 2, 3].as_slice()));

        let wire = serde_json::to_value(&inline).expect("serializes");
        assert_eq!(wire["bytes"]["retention"], "inline");
        assert!(wire["bytes"].get("byte_len").is_none());
        assert!(wire["bytes"].get("sha256").is_none());
        assert_eq!(
            serde_json::from_value::<RetainedSourceRecord>(wire.clone()).expect("round trip"),
            inline
        );

        let mut restated = wire;
        restated["bytes"]["byte_len"] = serde_json::json!(3);
        let error = serde_json::from_value::<RetainedSourceRecord>(restated)
            .unwrap_err()
            .to_string();
        assert!(error.contains("byte_len"), "{error}");
    }

    #[test]
    fn unknown_conversion_keeps_the_retained_bytes_as_the_record_image() {
        for attach in [false, true] {
            let unknown: UnknownRecord = serde_json::from_value(serde_json::json!({
                "id": "synthetic:model:unknown#0",
                "offset": 7,
                "byte_len": 3,
                "sha256": crate::hash::sha256_hex(&[1, 2, 3]),
                "data": "AQID"
            }))
            .unwrap();
            let mut fidelity = SourceFidelity::default();
            if attach {
                fidelity
                    .attach_native_unknown_records(&mut CadIr::empty(), "synthetic", [unknown])
                    .unwrap();
            } else {
                fidelity.retain_unknown_records("source", [unknown]);
            }
            let retained = fidelity
                .retained_record("synthetic:model:unknown#0")
                .expect("record is keyed by its id");
            assert_eq!(retained.byte_len(), 3);
            assert_eq!(retained.sha256(), crate::hash::sha256_hex(&[1, 2, 3]));
            assert_eq!(retained.data(), Some([1, 2, 3].as_slice()));
        }
    }

    #[test]
    fn decode_sidecar_binds_exact_ir_bytes() {
        let sidecar = DecodeSidecar::bind(b"cad-ir", report(), SourceFidelity::default());
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
        let sidecar = DecodeSidecar::bind(b"cad-ir", report(), SourceFidelity::default());
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
    fn a_sidecar_record_cannot_restate_the_length_of_its_inline_bytes() {
        let mut fidelity = SourceFidelity::default();
        fidelity
            .retained_records
            .insert("record".into(), record(b"payload"));
        let sidecar = DecodeSidecar::bind(b"cad-ir", report(), fidelity);
        let mut value = serde_json::to_value(sidecar).unwrap();
        value["fidelity"]["retained_records"]["record"]["bytes"]["byte_len"] = 1.into();
        let json = value.to_string();

        let error = serde_json::from_str::<DecodeSidecar>(&json)
            .expect_err("an inline record has no length field");
        assert!(error.to_string().contains("byte_len"), "{error}");
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
