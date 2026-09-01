// SPDX-License-Identifier: Apache-2.0
//! Format-independent container inspection results.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::dialect::{DialectLayers, FormatIdentity};

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerSummary {
    classification: FormatIdentity<DialectLayers>,
    /// Container kind, for example, `"zip"`.
    pub container_kind: String,
    /// Enumerated entries.
    pub entries: Vec<ContainerEntry>,
    /// Codec-defined informational notes.
    pub notes: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ContainerSummaryWire<Strings, Entries, Notes, Dialects: Default> {
    format: Strings,
    container_kind: Strings,
    entries: Entries,
    notes: Notes,
    /// Always serialized. Summaries written before the field existed omit the
    /// key and read back as unclassified.
    #[serde(default)]
    dialects: Dialects,
}

type OwnedContainerSummaryWire =
    ContainerSummaryWire<String, Vec<ContainerEntry>, Vec<String>, Option<DialectLayers>>;

impl Serialize for ContainerSummary {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ContainerSummaryWire {
            format: self.format(),
            container_kind: self.container_kind.as_str(),
            entries: self.entries.as_slice(),
            notes: self.notes.as_slice(),
            dialects: self.dialects(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ContainerSummary {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = OwnedContainerSummaryWire::deserialize(deserializer)?;
        let classification = FormatIdentity::from_wire(wire.format, wire.dialects)
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            classification,
            container_kind: wire.container_kind,
            entries: wire.entries,
            notes: wire.notes,
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for ContainerSummary {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ContainerSummary".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::ContainerSummary").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        OwnedContainerSummaryWire::json_schema(generator)
    }
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
            classification: FormatIdentity::classified(dialects),
            container_kind: container_kind.into(),
            entries,
            notes,
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
            classification: FormatIdentity::unclassified(format),
            container_kind: container_kind.into(),
            entries,
            notes,
        }
    }

    /// Returns the source format id.
    #[must_use]
    pub fn format(&self) -> &str {
        self.classification.format()
    }

    /// Returns the classified dialect layers, if inspection classified them.
    #[must_use]
    pub fn dialects(&self) -> Option<&DialectLayers> {
        self.classification.classified_payload()
    }
}

#[cfg(test)]
mod tests {
    use super::ContainerSummary;
    use crate::dialect::DialectId;
    use crate::dialect::{DialectLayers, DialectMatch};

    /// The field is part of the wire format: a summary that named no layer says
    /// so with null rather than by omitting the key. A summary written
    /// before the field existed still reads back.
    #[test]
    fn an_unclassified_summary_serializes_a_null_dialects_key() {
        let summary = ContainerSummary::unclassified("rhino", "flat", Vec::new(), Vec::new());

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
    }

    #[test]
    fn classified_summary_wire_uses_and_requires_the_primary_format() {
        let primary = DialectMatch::admitted(DialectId::pinned("rhino:archive-80"));
        let extra = DialectMatch::admitted(DialectId::pinned("acis:save-format-217"));
        let summary = ContainerSummary::classified(
            DialectLayers::new(primary.clone(), vec![extra.clone()])
                .expect("the extra uses another format"),
            "flat",
            Vec::new(),
            Vec::new(),
        );
        let classified = serde_json::to_value(&summary).expect("a summary serializes");
        assert_eq!(
            classified["dialects"],
            serde_json::json!({"primary": primary, "extra": [extra]})
        );
        assert_eq!(classified["format"], "rhino");

        let restored: ContainerSummary =
            serde_json::from_value(classified.clone()).expect("classified summary reads");
        assert_eq!(
            restored
                .dialects()
                .expect("the summary remains classified")
                .primary()
                .format(),
            "rhino"
        );

        let mut malformed = classified;
        malformed["format"] = serde_json::json!("step");
        let error = serde_json::from_value::<ContainerSummary>(malformed)
            .expect_err("a classified summary must match its primary dialect format");
        assert_eq!(
            error.to_string(),
            "format \"step\" does not match classified payload format \"rhino\""
        );
    }
}
