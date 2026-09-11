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
/// Serialize as the historical `container_kind` string. Deserialize rejects any
/// other string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerKind {
    /// Rhino 3DM chunk stream.
    ThreeDmChunks,
    /// IGES binary envelope.
    Binary,
    /// Compound File Binary.
    Cfb,
    /// `SolidWorks` CFB with compound streams.
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
    /// `SolidWorks` block table.
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
    /// Parse a `container_kind` wire string.
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ContainerSummary {
    /// Format identity: classified dialect layers, or a bare known format.
    identity: FormatIdentity<DialectLayers>,
    /// Container kind, for example, `"zip"`.
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub container_kind: ContainerKind,
    /// Enumerated entries.
    pub entries: Vec<ContainerEntry>,
    /// Losses resolved during inspection. Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub losses: Vec<LossNote>,
    /// Codec-defined informational notes.
    pub notes: Vec<String>,
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
            identity: FormatIdentity::classified(dialects),
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
            identity: FormatIdentity::unclassified(format),
            container_kind,
            entries,
            losses,
            notes,
        }
    }

    /// Returns the source format id.
    #[must_use]
    pub fn format(&self) -> &str {
        self.identity.format()
    }

    /// Returns the classified dialect layers, if inspection classified them.
    #[must_use]
    pub const fn dialects(&self) -> Option<&DialectLayers> {
        self.identity.classified_payload()
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};

    use super::{ContainerKind, ContainerSummary};

    #[test]
    fn an_unclassified_summary_states_its_format_inside_the_identity() {
        let summary = ContainerSummary::unclassified(
            "rhino",
            ContainerKind::Flat,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        let bare = serde_json::to_value(&summary).expect("a summary serializes");
        assert!(bare.get("losses").is_none(), "{bare}");
        assert_eq!(bare["identity"]["classification"], "unclassified");
        assert_eq!(bare["identity"]["format"], "rhino");
        assert!(bare.get("format").is_none(), "{bare}");
        assert_eq!(
            serde_json::from_value::<ContainerSummary>(bare).expect("a summary round-trips"),
            summary
        );
    }

    #[test]
    fn a_classified_summary_carries_its_format_once() {
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
        assert_eq!(classified["identity"]["classification"], "classified");
        assert_eq!(
            classified["identity"]["dialects"],
            serde_json::json!({"primary": primary, "extra": [extra]})
        );
        assert!(classified["identity"].get("format").is_none());

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

        let mut restated = classified;
        restated["identity"]["format"] = serde_json::json!("step");
        let error = serde_json::from_value::<ContainerSummary>(restated)
            .expect_err("a classified identity carries no second format");
        assert!(error.to_string().contains("format"), "{error}");
    }

    #[cfg(feature = "schema")]
    #[test]
    fn current_summary_schema_requires_its_identity() {
        let schema = serde_json::to_value(schemars::schema_for!(ContainerSummary))
            .expect("summary schema serializes");
        let required = schema["required"]
            .as_array()
            .expect("summary schema has required fields");
        assert!(
            required.iter().any(|field| field == "identity"),
            "{schema:#}"
        );
        assert!(
            !required.iter().any(|field| field == "losses"),
            "{schema:#}"
        );
    }
}
