// SPDX-License-Identifier: Apache-2.0
//! Format-independent container inspection results.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::dialect::DialectMatch;

/// One stream or segment in a container summary.
///
/// `role` and `attributes` are codec-defined. The ordered attribute map keeps
/// the format-independent summary deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ContainerEntry {
    /// Entry name/path within the container.
    pub name: String,
    /// Codec-defined role classification.
    pub role: String,
    /// Compression method label (for example, `"stored"` or `"deflate"`).
    pub compression: String,
    /// Compressed size in bytes.
    pub compressed_size: u64,
    /// Uncompressed size in bytes.
    pub uncompressed_size: u64,
    /// Extra codec-extracted attributes, sorted by key.
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

/// The result of inspecting a container without decoding its geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ContainerSummary {
    /// Source format id.
    pub format: String,
    /// Container kind, for example, `"zip"`.
    pub container_kind: String,
    /// Enumerated entries.
    pub entries: Vec<ContainerEntry>,
    /// Codec-defined informational notes.
    pub notes: Vec<String>,
    /// Dialect identification, one entry per format layer the inspection read.
    ///
    /// Empty only when the inspection identified no layer at all. Once
    /// populated, exactly one entry's `format` equals [`Self::format`]: that
    /// entry is the primary layer.
    ///
    /// Enforced by `cadmpeg_ir::codec::Codec::inspect`, the one wrapper every
    /// backend's summary passes through on its way to a caller.
    ///
    /// Always serialized. Summaries written before the field existed omit the
    /// key and read back empty.
    #[serde(default)]
    pub dialects: Vec<DialectMatch>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{ContainerSummary, DialectMatch};
    use crate::dialect::{Admission, DialectId};

    /// The field is part of the wire format: a summary that named no layer says
    /// so with an empty list rather than by omitting the key. A summary written
    /// before the field existed still reads back.
    #[test]
    fn an_unclassified_summary_serializes_an_empty_dialects_key() {
        let mut summary = ContainerSummary {
            format: "rhino".into(),
            container_kind: "flat".into(),
            entries: Vec::new(),
            notes: Vec::new(),
            dialects: Vec::new(),
        };

        let bare = serde_json::to_string(&summary).expect("a summary serializes");
        assert!(bare.contains("\"dialects\":[]"), "{bare}");
        assert_eq!(
            serde_json::from_str::<ContainerSummary>(&bare).expect("a summary round-trips"),
            summary
        );

        // A summary persisted before the field existed omits the key entirely.
        let legacy = r#"{"format":"rhino","container_kind":"flat","entries":[],"notes":[]}"#;
        assert_eq!(
            serde_json::from_str::<ContainerSummary>(legacy).expect("a legacy summary reads"),
            summary
        );

        summary.dialects.push(DialectMatch {
            format: "rhino".into(),
            dialect: Some(DialectId::pinned("rhino:archive-80")),
            declared: BTreeMap::new(),
            admission: Admission::Admitted,
        });
        let classified = serde_json::to_string(&summary).expect("a summary serializes");
        assert!(classified.contains("rhino:archive-80"), "{classified}");
    }
}
