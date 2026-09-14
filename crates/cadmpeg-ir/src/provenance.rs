// SPDX-License-Identifier: Apache-2.0
//! Provenance and exactness value types.
//!
//! [`Exactness`] classifies how an IR value relates to source bytes. Current
//! documents store exactness and source locations in
//! [`crate::annotations::Annotations`].

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::Arc;

use crate::topology::Color;
use serde::de::Error as _;
use std::fmt;

/// Registry codec format id stored on a source-object association.
///
/// The variant set is the generated `FORMAT` constants. Serialize as the
/// registry string. Deserialize rejects any other string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CodecFormat {
    /// `acis`
    Acis,
    /// `catia`
    Catia,
    /// `creo`
    Creo,
    /// `f3d`
    F3d,
    /// `fcstd`
    Fcstd,
    /// `iges`
    Iges,
    /// `inventor`
    Inventor,
    /// `nx`
    Nx,
    /// `parasolid`
    Parasolid,
    /// `rhino`
    Rhino,
    /// `sat`
    Sat,
    /// `sldprt`
    Sldprt,
    /// `step`
    Step,
}

impl CodecFormat {
    /// Parse a generated registry format id.
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        Some(match id {
            "acis" => Self::Acis,
            "catia" => Self::Catia,
            "creo" => Self::Creo,
            "f3d" => Self::F3d,
            "fcstd" => Self::Fcstd,
            "iges" => Self::Iges,
            "inventor" => Self::Inventor,
            "nx" => Self::Nx,
            "parasolid" => Self::Parasolid,
            "rhino" => Self::Rhino,
            "sat" => Self::Sat,
            "sldprt" => Self::Sldprt,
            "step" => Self::Step,
            _ => return None,
        })
    }

    /// The generated registry format id.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Acis => "acis",
            Self::Catia => "catia",
            Self::Creo => "creo",
            Self::F3d => "f3d",
            Self::Fcstd => "fcstd",
            Self::Iges => "iges",
            Self::Inventor => "inventor",
            Self::Nx => "nx",
            Self::Parasolid => "parasolid",
            Self::Rhino => "rhino",
            Self::Sat => "sat",
            Self::Sldprt => "sldprt",
            Self::Step => "step",
        }
    }

    /// Check a generated `FORMAT` constant against the registry.
    ///
    /// The [`codec_format!`](crate::codec_format) macro evaluates this in a
    /// const initializer and turns `None` into a compile error. Runtime text
    /// must use [`CodecFormat::parse`], which returns `None` instead of
    /// panicking.
    #[must_use]
    pub const fn from_registry_id(id: &str) -> Option<Self> {
        let bytes = id.as_bytes();
        if registry_id_is(bytes, b"acis") {
            Some(Self::Acis)
        } else if registry_id_is(bytes, b"catia") {
            Some(Self::Catia)
        } else if registry_id_is(bytes, b"creo") {
            Some(Self::Creo)
        } else if registry_id_is(bytes, b"f3d") {
            Some(Self::F3d)
        } else if registry_id_is(bytes, b"fcstd") {
            Some(Self::Fcstd)
        } else if registry_id_is(bytes, b"iges") {
            Some(Self::Iges)
        } else if registry_id_is(bytes, b"inventor") {
            Some(Self::Inventor)
        } else if registry_id_is(bytes, b"nx") {
            Some(Self::Nx)
        } else if registry_id_is(bytes, b"parasolid") {
            Some(Self::Parasolid)
        } else if registry_id_is(bytes, b"rhino") {
            Some(Self::Rhino)
        } else if registry_id_is(bytes, b"sat") {
            Some(Self::Sat)
        } else if registry_id_is(bytes, b"sldprt") {
            Some(Self::Sldprt)
        } else if registry_id_is(bytes, b"step") {
            Some(Self::Step)
        } else {
            None
        }
    }
}

/// True when the registry id `bytes` spells exactly `expected`.
const fn registry_id_is(bytes: &[u8], expected: &[u8]) -> bool {
    if bytes.len() != expected.len() {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != expected[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// The [`CodecFormat`] a generated `FORMAT` constant names.
///
/// The id is read in the initializer of a `const` item, so a codec whose
/// `FORMAT` is not a registry id fails `cargo check` with E0080 instead of
/// panicking when the association is built.
#[macro_export]
macro_rules! codec_format {
    ($id:expr) => {{
        const CODEC_FORMAT: $crate::CodecFormat = match $crate::CodecFormat::from_registry_id($id) {
            Some(format) => format,
            None => {
                assert!(
                    false,
                    "a registry format id names one of the generated FORMAT constants"
                );
                $crate::CodecFormat::Step
            }
        };
        CODEC_FORMAT
    }};
}

impl fmt::Display for CodecFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for CodecFormat {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CodecFormat {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let id = String::deserialize(deserializer)?;
        Self::parse(&id).ok_or_else(|| D::Error::custom(format!("format: unknown codec id {id}")))
    }
}

/// Native object identity and effective display metadata for a free carrier.
///
/// `format` identifies the source format. `object_id` is the source format's
/// native object identifier, not an IR arena identifier. `name`, `color`, and
/// `visible` are the effective object display values. `layer` is the native
/// layer identifier. `instance_path` contains native instance identifiers in
/// outermost-to-innermost order; an empty path means that the object is not
/// nested in an instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SourceObjectAssociation {
    /// Source format identifier.
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub format: CodecFormat,
    /// Native source object identifier.
    #[serde(deserialize_with = "deserialize_object_id")]
    pub object_id: cadmpeg_core::text::NonBlankString,
    /// Effective source object name, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "cadmpeg_core::absent_key::present"
    )]
    pub name: Option<String>,
    /// Effective source object color, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "cadmpeg_core::absent_key::present"
    )]
    pub color: Option<Color>,
    /// Effective source object visibility, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "cadmpeg_core::absent_key::present"
    )]
    pub visible: Option<bool>,
    /// Native source layer identifier, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "cadmpeg_core::absent_key::present"
    )]
    pub layer: Option<String>,
    /// Native instance identifiers from outermost to innermost.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instance_path: Vec<String>,
}

crate::units::named_field!(
    deserialize_object_id,
    cadmpeg_core::text::NonBlankString,
    "object_id"
);

/// Provenance for bytes identified by a typed location.
///
/// The location type distinguishes report provenance from an interned
/// annotation stream. Both forms share byte-offset and source-tag semantics
/// without admitting one form where the other is required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance<Location> {
    location: Location,
    /// Byte offset of the record within its source stream.
    pub offset: u64,
    /// Source record/class name/tag, when the decoder can attribute one.
    pub tag: Option<String>,
}

/// Name of a container stream inside a source format.
///
/// The empty string is not a stream name; the root stream is the absence of
/// one. Build a compile-time name with [`stream_name!`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "String", into = "String")]
pub struct StreamName(std::borrow::Cow<'static, str>);

/// A static stream name whose non-empty invariant was admitted during const
/// evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticStreamName {
    value: &'static str,
}

impl StaticStreamName {
    /// Admit a static stream name, returning `None` for the empty string.
    #[must_use]
    pub const fn new(value: &'static str) -> Option<Self> {
        if value.is_empty() {
            None
        } else {
            Some(Self { value })
        }
    }
}

/// The empty string does not name a container stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a source stream name cannot be empty")]
pub struct EmptyStreamName;

impl StreamName {
    /// Construct a stream name from a static proof.
    #[must_use]
    pub const fn from_static(proof: StaticStreamName) -> Self {
        Self(std::borrow::Cow::Borrowed(proof.value))
    }

    /// Returns the stream name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Build a checked stream name from a literal.
///
/// ```compile_fail
/// # fn main() {
/// let _ = cadmpeg_ir::stream_name!("");
/// # }
/// ```
#[macro_export]
macro_rules! stream_name {
    ($value:literal) => {{
        const STATIC_STREAM_NAME: $crate::StaticStreamName =
            match $crate::StaticStreamName::new($value) {
                Some(name) => name,
                None => panic!("a source stream name cannot be empty"),
            };
        $crate::StreamName::from_static(STATIC_STREAM_NAME)
    }};
}

impl TryFrom<String> for StreamName {
    type Error = EmptyStreamName;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        if name.is_empty() {
            return Err(EmptyStreamName);
        }
        Ok(Self(std::borrow::Cow::Owned(name)))
    }
}

impl From<StreamName> for String {
    fn from(name: StreamName) -> Self {
        name.0.into_owned()
    }
}

/// Source format and optional named stream used by a report loss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    format: String,
    stream: Option<StreamName>,
}

/// Source provenance attached to a report loss.
pub type SourceProvenance = Provenance<SourceLocation>;

/// Opaque owned reference to an annotation stream.
///
/// The referenced name travels with the provenance. A stream-table index is
/// created only by the annotation wire adapter, so an in-memory provenance
/// cannot dangle after annotations are moved or merged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationLocation {
    stream: Arc<str>,
}

/// Provenance attached to an entity in [`crate::Annotations`].
pub type AnnotationProvenance = Provenance<AnnotationLocation>;

impl Provenance<AnnotationLocation> {
    pub(crate) fn annotation(stream: Arc<str>, offset: u64, tag: Option<String>) -> Self {
        Self {
            location: AnnotationLocation { stream },
            offset,
            tag,
        }
    }

    /// Return the owned source stream name.
    #[must_use]
    pub fn stream(&self) -> &str {
        &self.location.stream
    }
}

impl Provenance<SourceLocation> {
    /// Construct provenance relative to a format's root source stream.
    pub fn root(format: impl Into<String>, offset: u64) -> Self {
        Self {
            location: SourceLocation {
                format: format.into(),
                stream: None,
            },
            offset,
            tag: None,
        }
    }

    /// Construct provenance relative to a named container stream.
    pub fn in_stream(format: impl Into<String>, stream: StreamName, offset: u64) -> Self {
        Self {
            location: SourceLocation {
                format: format.into(),
                stream: Some(stream),
            },
            offset,
            tag: None,
        }
    }

    /// Attach a source record or class name.
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Attach an optional source record or class name.
    #[must_use]
    pub fn with_optional_tag(mut self, tag: Option<String>) -> Self {
        self.tag = tag;
        self
    }

    /// Return the source format identifier.
    #[must_use]
    pub fn format(&self) -> &str {
        &self.location.format
    }

    /// Return the named container stream, or `None` for the root stream.
    #[must_use]
    pub fn stream(&self) -> Option<&str> {
        self.location.stream.as_ref().map(StreamName::as_str)
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct AnnotationProvenanceWire {
    stream: StreamName,
    offset: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "cadmpeg_core::absent_key::present"
    )]
    tag: Option<String>,
}

impl Serialize for Provenance<AnnotationLocation> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        AnnotationProvenanceWire {
            stream: StreamName(std::borrow::Cow::Owned(self.location.stream.to_string())),
            offset: self.offset,
            tag: self.tag.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Provenance<AnnotationLocation> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = AnnotationProvenanceWire::deserialize(deserializer)?;
        Ok(Self {
            location: AnnotationLocation {
                stream: Arc::<str>::from(wire.stream.as_str()),
            },
            offset: wire.offset,
            tag: wire.tag,
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for Provenance<AnnotationLocation> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AnnotationProvenance".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        AnnotationProvenanceWire::json_schema(generator)
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SourceProvenanceWire {
    format: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "cadmpeg_core::absent_key::present"
    )]
    stream: Option<StreamName>,
    offset: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "cadmpeg_core::absent_key::present"
    )]
    tag: Option<String>,
}

impl From<Provenance<SourceLocation>> for SourceProvenanceWire {
    fn from(provenance: Provenance<SourceLocation>) -> Self {
        Self {
            format: provenance.location.format,
            stream: provenance.location.stream,
            offset: provenance.offset,
            tag: provenance.tag,
        }
    }
}

impl From<SourceProvenanceWire> for Provenance<SourceLocation> {
    fn from(wire: SourceProvenanceWire) -> Self {
        Self {
            location: SourceLocation {
                format: wire.format,
                stream: wire.stream,
            },
            offset: wire.offset,
            tag: wire.tag,
        }
    }
}

impl Serialize for Provenance<SourceLocation> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        SourceProvenanceWire::from(self.clone()).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Provenance<SourceLocation> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        SourceProvenanceWire::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for Provenance<SourceLocation> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SourceProvenance".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        SourceProvenanceWire::json_schema(generator)
    }
}

/// How an entity or field value was established from its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum Exactness {
    /// Read verbatim from the source stream with no transformation beyond
    /// documented unit conversion.
    ByteExact,
    /// Computed deterministically from byte-exact inputs.
    Derived,
    /// Filled in from context or convention rather than an explicit source field.
    Inferred,
    /// Origin or trustworthiness could not be established.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_format_parser_is_total_for_runtime_text() {
        const STEP: CodecFormat = crate::codec_format!("step");

        assert_eq!(
            CodecFormat::from_registry_id("step"),
            Some(CodecFormat::Step)
        );
        assert_eq!(CodecFormat::from_registry_id("unknown"), None);
        assert_eq!(STEP, CodecFormat::Step);
    }

    #[test]
    fn a_source_provenance_stream_is_named_or_absent() {
        let root = SourceProvenance::root("iges", 12);
        let wire = serde_json::to_value(&root).expect("serialize");
        assert_eq!(wire, serde_json::json!({"format": "iges", "offset": 12}));
        let read: SourceProvenance = serde_json::from_value(wire).expect("root reads back");
        assert_eq!(read.stream(), None);
        assert_eq!(read, root);

        let named = SourceProvenance::in_stream("fcstd", crate::stream_name!("Document.xml"), 0);
        let wire = serde_json::to_value(&named).expect("serialize");
        assert_eq!(
            wire,
            serde_json::json!({"format": "fcstd", "stream": "Document.xml", "offset": 0})
        );
        let read: SourceProvenance = serde_json::from_value(wire).expect("named reads back");
        assert_eq!(read.stream(), Some("Document.xml"));

        let error = serde_json::from_value::<SourceProvenance>(
            serde_json::json!({"format": "iges", "stream": "", "offset": 0}),
        )
        .expect_err("the empty string does not name a stream")
        .to_string();
        assert!(error.contains("stream name cannot be empty"), "{error}");
    }

    #[test]
    fn source_object_identity_is_nonempty_and_preserves_wire() {
        let wire = serde_json::json!({"format": "step", "object_id": "#42"});
        let value: SourceObjectAssociation =
            serde_json::from_value(wire.clone()).expect("nonempty identity");
        assert_eq!(serde_json::to_value(value).expect("serialize"), wire);
        let error = serde_json::from_value::<SourceObjectAssociation>(
            serde_json::json!({"format": "step", "object_id": ""}),
        )
        .expect_err("empty identity");
        assert!(error.to_string().contains("object_id"));
    }
}
