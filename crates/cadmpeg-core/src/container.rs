// SPDX-License-Identifier: Apache-2.0
//! Format-independent container inspection results.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

use crate::dialect::{DialectLayers, DialectMatch};

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    /// Always serialized. Summaries written before the field existed omit the
    /// key and read back as unclassified.
    #[serde(default, serialize_with = "serialize_dialect_layers")]
    #[cfg_attr(feature = "schema", schemars(with = "Vec<DialectMatch>"))]
    pub dialects: Option<DialectLayers>,
}

// Serde's `serialize_with` callback receives a reference to the field.
#[allow(clippy::ref_option)]
fn serialize_dialect_layers<S: Serializer>(
    layers: &Option<DialectLayers>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match layers {
        Some(layers) => layers.serialize(serializer),
        None => serializer.collect_seq(std::iter::empty::<&DialectMatch>()),
    }
}

#[derive(Deserialize)]
struct ContainerSummaryWire {
    format: String,
    container_kind: String,
    entries: Vec<ContainerEntry>,
    notes: Vec<String>,
    #[serde(default, rename = "dialects")]
    flat_dialects: Vec<DialectMatch>,
}

impl<'de> Deserialize<'de> for ContainerSummary {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut wire = ContainerSummaryWire::deserialize(deserializer)?;
        let dialects = split_dialect_layers(&wire.format, &mut wire.flat_dialects)
            .map_err(D::Error::custom)?;
        Ok(Self {
            format: wire.format,
            container_kind: wire.container_kind,
            entries: wire.entries,
            notes: wire.notes,
            dialects,
        })
    }
}

fn split_dialect_layers(
    format: &str,
    layers: &mut Vec<DialectMatch>,
) -> Result<Option<DialectLayers>, String> {
    if layers.is_empty() {
        return Ok(None);
    }
    let mut primary = layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| layer.format == format);
    let Some((primary_index, _)) = primary.next() else {
        return Err(format!(
            "populated dialects for format {format:?} contain no primary layer"
        ));
    };
    if primary.next().is_some() {
        return Err(format!(
            "populated dialects for format {format:?} contain multiple primary layers"
        ));
    }
    let primary = layers.remove(primary_index);
    DialectLayers::new(primary, std::mem::take(layers))
        .map(Some)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::ContainerSummary;
    use crate::dialect::{Admission, DialectId};
    use crate::dialect::{DialectLayers, DialectMatch};

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
            dialects: None,
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

        let primary = DialectMatch {
            format: "rhino".into(),
            dialect: Some(DialectId::pinned("rhino:archive-80")),
            declared: BTreeMap::new(),
            admission: Admission::Admitted,
        };
        let extra = DialectMatch {
            format: "acis".into(),
            dialect: Some(DialectId::pinned("acis:save-format-217")),
            declared: BTreeMap::new(),
            admission: Admission::Admitted,
        };
        summary.dialects = Some(
            DialectLayers::new(primary.clone(), vec![extra.clone()])
                .expect("the extra uses another format"),
        );
        let classified = serde_json::to_value(&summary).expect("a summary serializes");
        assert_eq!(classified["dialects"], serde_json::json!([primary, extra]));

        let mut legacy_reordered = classified;
        legacy_reordered["dialects"]
            .as_array_mut()
            .expect("dialects serialize as an array")
            .swap(0, 1);
        let restored: ContainerSummary =
            serde_json::from_value(legacy_reordered).expect("legacy row order reads");
        assert_eq!(
            restored
                .dialects
                .as_ref()
                .expect("the summary remains classified")
                .primary()
                .format,
            "rhino"
        );
    }
}
