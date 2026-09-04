// SPDX-License-Identifier: Apache-2.0
//! Typed container inspection reports.

use cadmpeg_core::dialect::{DialectLayers, FormatIdentity};
use cadmpeg_core::ContainerEntry;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::report::LossNote;
use serde::de::Error as _;
use std::fmt;

/// Physical envelope of an inspected document.
///
/// Serialize as the historical container_kind string. Deserialize rejects any
/// other string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerKind {
    /// Rhino 3DM chunk stream.
    ThreeDmChunks,
    /// IGES binary envelope.
    Binary,
    /// Compound File Binary.
    Cfb,
    /// SolidWorks CFB with compound streams.
    CompoundFileBinary,
    /// IGES compressed ASCII.
    CompressedAscii,
    /// IGES fixed ASCII.
    FixedAscii,
    /// Unframed byte stream used in tests and some scanners.
    Flat,
    /// STEP Part 21 clear text.
    Iso10303ClearText,
    /// STEP Part 21 inside ZIP.
    Iso10303Zip,
    /// Creo PSB.
    Psb,
    /// SolidWorks block table.
    SldprtBlocks,
    /// NX splmsstr.
    Splmsstr,
    /// SAT/SAB kernel stream.
    Stream,
    /// CATIA V5 CFV2.
    V5Cfv2,
    /// ZIP archive.
    Zip,
}

impl ContainerKind {
    /// Parse a container_kind wire string.
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        Some(match id {
            "3dm-chunks" => Self::ThreeDmChunks,
            "binary" => Self::Binary,
            "cfb" => Self::Cfb,
            "compound-file-binary" => Self::CompoundFileBinary,
            "compressed-ascii" => Self::CompressedAscii,
            "fixed-ascii" => Self::FixedAscii,
            "flat" => Self::Flat,
            "iso-10303-21-clear-text" => Self::Iso10303ClearText,
            "iso-10303-21-zip" => Self::Iso10303Zip,
            "psb" => Self::Psb,
            "sldprt-blocks" => Self::SldprtBlocks,
            "splmsstr" => Self::Splmsstr,
            "stream" => Self::Stream,
            "v5-cfv2" => Self::V5Cfv2,
            "zip" => Self::Zip,
            _ => return None,
        })
    }

    /// Wire label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ThreeDmChunks => "3dm-chunks",
            Self::Binary => "binary",
            Self::Cfb => "cfb",
            Self::CompoundFileBinary => "compound-file-binary",
            Self::CompressedAscii => "compressed-ascii",
            Self::FixedAscii => "fixed-ascii",
            Self::Flat => "flat",
            Self::Iso10303ClearText => "iso-10303-21-clear-text",
            Self::Iso10303Zip => "iso-10303-21-zip",
            Self::Psb => "psb",
            Self::SldprtBlocks => "sldprt-blocks",
            Self::Splmsstr => "splmsstr",
            Self::Stream => "stream",
            Self::V5Cfv2 => "v5-cfv2",
            Self::Zip => "zip",
        }
    }
}

impl fmt::Display for ContainerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<str> for ContainerKind {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for ContainerKind {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl Serialize for ContainerKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ContainerKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let id = String::deserialize(deserializer)?;
        Self::parse(&id)
            .ok_or_else(|| D::Error::custom(format!("container_kind: unknown value {id}")))
    }
}

/// The result of inspecting a container without decoding its geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerSummary {
    classification: FormatIdentity<DialectLayers>,
    /// Container kind, for example, `"zip"`.
    pub container_kind: ContainerKind,
    /// Enumerated entries.
    pub entries: Vec<ContainerEntry>,
    /// Losses resolved during inspection.
    pub losses: Vec<LossNote>,
    /// Codec-defined informational notes.
    pub notes: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ContainerSummaryWire<
    Strings,
    Entries,
    Losses: Default + AsRef<[LossNote]>,
    Notes,
    Dialects: Default,
> {
    format: Strings,
    container_kind: Strings,
    entries: Entries,
    /// Omitted when empty. Summaries written before typed inspection losses
    /// existed therefore read back with no recorded losses.
    #[serde(default, skip_serializing_if = "losses_empty")]
    losses: Losses,
    notes: Notes,
    /// Always serialized. Summaries written before the field existed omit the
    /// key and read back as unclassified.
    #[serde(default)]
    dialects: Dialects,
}

fn losses_empty(losses: &impl AsRef<[LossNote]>) -> bool {
    losses.as_ref().is_empty()
}

type OwnedContainerSummaryWire = ContainerSummaryWire<
    String,
    Vec<ContainerEntry>,
    Vec<LossNote>,
    Vec<String>,
    Option<DialectLayers>,
>;

impl Serialize for ContainerSummary {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ContainerSummaryWire {
            format: self.format(),
            container_kind: self.container_kind.as_str(),
            entries: self.entries.as_slice(),
            losses: self.losses.as_slice(),
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
            container_kind: ContainerKind::parse(&wire.container_kind).ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "container_kind: unknown value {}",
                    wire.container_kind
                ))
            })?,
            entries: wire.entries,
            losses: wire.losses,
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
        let mut schema = OwnedContainerSummaryWire::json_schema(generator);
        crate::schema::require_object_fields(&mut schema, ["dialects"]);
        schema
    }
}

impl ContainerSummary {
    /// Constructs a classified summary whose format is its primary layer's format.
    #[must_use]
    pub fn classified(
        dialects: DialectLayers,
        container_kind: ContainerKind,
        entries: Vec<ContainerEntry>,
        losses: Vec<LossNote>,
        notes: Vec<String>,
    ) -> Self {
        Self {
            classification: FormatIdentity::classified(dialects),
            container_kind,
            entries,
            losses,
            notes,
        }
    }

    /// Constructs an unclassified summary for a known source format.
    #[must_use]
    pub fn unclassified(
        format: impl Into<String>,
        container_kind: ContainerKind,
        entries: Vec<ContainerEntry>,
        losses: Vec<LossNote>,
        notes: Vec<String>,
    ) -> Self {
        Self {
            classification: FormatIdentity::unclassified(format),
            container_kind,
            entries,
            losses,
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
    use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};

    use super::{ContainerKind, ContainerSummary};

    /// Current writers emit the dialect field and omit an empty loss set.
    /// Readers still accept summaries written before either field existed.
    #[test]
    fn an_unclassified_summary_serializes_required_empty_fields() {
        let summary = ContainerSummary::unclassified(
            "rhino",
            ContainerKind::Flat,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        let bare = serde_json::to_string(&summary).expect("a summary serializes");
        assert!(!bare.contains("\"losses\""), "{bare}");
        assert!(bare.contains("\"dialects\":null"), "{bare}");
        assert_eq!(
            serde_json::from_str::<ContainerSummary>(&bare).expect("a summary round-trips"),
            summary
        );

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
            DialectLayers::of(primary.clone()).with(extra.clone()),
            ContainerKind::Flat,
            Vec::new(),
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

    #[cfg(feature = "schema")]
    #[test]
    fn current_summary_schema_requires_the_always_serialized_dialects_field() {
        let schema = serde_json::to_value(schemars::schema_for!(ContainerSummary))
            .expect("summary schema serializes");
        let required = schema["required"]
            .as_array()
            .expect("summary schema has required fields");
        assert!(
            required.iter().any(|field| field == "dialects"),
            "{schema:#}"
        );
        assert!(
            !required.iter().any(|field| field == "losses"),
            "{schema:#}"
        );
    }
}
