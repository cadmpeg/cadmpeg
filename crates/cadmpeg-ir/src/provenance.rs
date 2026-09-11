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

    /// Construct from a generated `FORMAT` constant.
    #[must_use]
    pub fn from_registry(id: &str) -> Self {
        Self::parse(id).unwrap_or_else(|| panic!("unknown registry format id {id}"))
    }
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
pub struct SourceObjectAssociation {
    /// Source format identifier.
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub format: CodecFormat,
    /// Native source object identifier.
    #[serde(deserialize_with = "deserialize_object_id")]
    pub object_id: crate::products::NonEmptyString,
    /// Effective source object name, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Effective source object color, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Effective source object visibility, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Native source layer identifier, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    /// Native instance identifiers from outermost to innermost.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instance_path: Vec<String>,
}

crate::units::named_field!(
    deserialize_object_id,
    crate::products::NonEmptyString,
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
/// one. Build a compile-time name with `const { StreamName::literal("…") }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "String", into = "String")]
pub struct StreamName(std::borrow::Cow<'static, str>);

/// The empty string does not name a container stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a source stream name cannot be empty")]
pub struct EmptyStreamName;

impl StreamName {
    /// Names a stream from a non-empty literal.
    ///
    /// Call it inside a `const` block: an empty literal is then a compile
    /// error rather than a run-time panic.
    #[must_use]
    pub const fn literal(name: &'static str) -> Self {
        if name.is_empty() {
            panic!("a source stream name cannot be empty");
        }
        Self(std::borrow::Cow::Borrowed(name))
    }

    /// Returns the stream name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
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

    pub(crate) fn stream_ref(&self) -> &Arc<str> {
        &self.location.stream
    }

    pub(crate) fn rebind_stream(&mut self, stream: Arc<str>) {
        self.location.stream = stream;
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
struct SourceProvenanceWire {
    format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stream: Option<StreamName>,
    offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    fn a_source_provenance_stream_is_named_or_absent() {
        let root = SourceProvenance::root("iges", 12);
        let wire = serde_json::to_value(&root).expect("serialize");
        assert_eq!(wire, serde_json::json!({"format": "iges", "offset": 12}));
        let read: SourceProvenance = serde_json::from_value(wire).expect("root reads back");
        assert_eq!(read.stream(), None);
        assert_eq!(read, root);

        let named = SourceProvenance::in_stream(
            "fcstd",
            const { StreamName::literal("Document.xml") },
            0,
        );
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
