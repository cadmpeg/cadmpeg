// SPDX-License-Identifier: Apache-2.0
//! Format-independent container inspection results.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::dialect::DialectLayers;

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
    format: String,
    /// Container kind, for example, `"zip"`.
    pub container_kind: String,
    /// Enumerated entries.
    pub entries: Vec<ContainerEntry>,
    /// Codec-defined informational notes.
    pub notes: Vec<String>,
    /// Dialect identification, one entry per format layer the inspection read.
    ///
    /// Always serialized. Summaries written before the field existed omit the
    /// key and read back as unclassified.
    #[serde(default)]
    dialects: Option<DialectLayers>,
}

impl ContainerSummary {
    /// Constructs a classified summary whose format is its primary layer's format.
    #[must_use]
    pub fn classified(
        dialects: DialectLayers,
        container_kind: impl Into<String>,
        entries: Vec<ContainerEntry>,
        notes: Vec<String>,
    ) -> Self {
        Self {
            format: dialects.primary().format.clone(),
            container_kind: container_kind.into(),
            entries,
            notes,
            dialects: Some(dialects),
        }
    }

    /// Constructs an unclassified summary for a known source format.
    #[must_use]
    pub fn unclassified(
        format: impl Into<String>,
        container_kind: impl Into<String>,
        entries: Vec<ContainerEntry>,
        notes: Vec<String>,
    ) -> Self {
        Self {
            format: format.into(),
            container_kind: container_kind.into(),
            entries,
            notes,
            dialects: None,
        }
    }

    /// Returns the source format id.
    #[must_use]
    pub fn format(&self) -> &str {
        &self.format
    }

    /// Returns the classified dialect layers, if inspection classified them.
    #[must_use]
    pub fn dialects(&self) -> Option<&DialectLayers> {
        self.dialects.as_ref()
    }

    /// Removes and returns the classified dialect layers.
    pub fn take_dialects(&mut self) -> Option<DialectLayers> {
        self.dialects.take()
    }

    /// Replaces the classification and derives the summary format from it.
    pub fn set_dialects(&mut self, dialects: DialectLayers) {
        self.format.clone_from(&dialects.primary().format);
        self.dialects = Some(dialects);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::ContainerSummary;
    use crate::dialect::{Admission, DialectId};
    use crate::dialect::{DialectLayers, DialectMatch};

    /// The field is part of the wire format: a summary that named no layer says
    /// so with null rather than by omitting the key. A summary written
    /// before the field existed still reads back.
    #[test]
    fn an_unclassified_summary_serializes_an_empty_dialects_key() {
        let mut summary = ContainerSummary::unclassified("rhino", "flat", Vec::new(), Vec::new());

        let bare = serde_json::to_string(&summary).expect("a summary serializes");
        assert!(bare.contains("\"dialects\":null"), "{bare}");
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

        let primary = DialectMatch {
            format: "rhino".into(),
            dialect: Some(DialectId::pinned("rhino:archive-80")),
            declared: BTreeMap::new(),
            instance: None,
            admission: Admission::Admitted,
        };
        let extra = DialectMatch {
            format: "acis".into(),
            dialect: Some(DialectId::pinned("acis:save-format-217")),
            declared: BTreeMap::new(),
            instance: None,
            admission: Admission::Admitted,
        };
        summary.set_dialects(
            DialectLayers::new(primary.clone(), vec![extra.clone()])
                .expect("the extra uses another format"),
        );
        let classified = serde_json::to_value(&summary).expect("a summary serializes");
        assert_eq!(
            classified["dialects"],
            serde_json::json!({"primary": primary, "extra": [extra]})
        );

        let restored: ContainerSummary =
            serde_json::from_value(classified).expect("classified summary reads");
        assert_eq!(
            restored
                .dialects()
                .expect("the summary remains classified")
                .primary()
                .format,
            "rhino"
        );
    }
}
