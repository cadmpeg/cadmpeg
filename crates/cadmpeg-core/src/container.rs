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
    /// Empty while a codec has not yet been migrated to classify. Once
    /// populated, exactly one entry's `format` equals [`Self::format`]: that
    /// entry is the primary layer.
    ///
    /// Enforced by `cadmpeg_ir::codec::Codec::inspect`, the one wrapper every
    /// backend's summary passes through on its way to a caller. See
    /// [`crate::dialect::debug_assert_primary_layer`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dialects: Vec<DialectMatch>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{ContainerSummary, DialectMatch};
    use crate::dialect::{Admission, DialectId};

    /// The staged field is invisible on the wire until a codec populates it, so
    /// adding it moved no persisted byte.
    #[test]
    fn an_unclassified_summary_serializes_without_a_dialects_key() {
        let mut summary = ContainerSummary {
            format: "rhino".into(),
            container_kind: "flat".into(),
            entries: Vec::new(),
            notes: Vec::new(),
            dialects: Vec::new(),
        };

        let bare = serde_json::to_string(&summary).expect("a summary serializes");
        assert!(!bare.contains("dialects"), "{bare}");
        assert_eq!(
            serde_json::from_str::<ContainerSummary>(&bare).expect("a summary round-trips"),
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
