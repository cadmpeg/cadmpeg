// SPDX-License-Identifier: Apache-2.0
//! Provenance and exactness value types.
//!
//! [`Exactness`] classifies how an IR value relates to source bytes. Current
//! documents store exactness and source locations in
//! [`crate::annotations::Annotations`].

use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::Arc;

use crate::design::names::CodecId;
use std::fmt;

/// Whether source geometry is a standalone object or a support of another object.
/// Supports retain their mathematics when their owner cannot be projected; writers
/// must not promote them to standalone geometry merely because they are unused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SourceGeometryRole {
    /// Geometry independently present in the source document.
    Independent,
    /// Geometry used to define another source object.
    Support,
}

/// The identity of the source object a point, carrier, cage, or tessellation
/// was read from, and nothing else: its name, colour, visibility, and layer
/// have their own homes (D21).
///
/// `format` identifies the source format. `object_id` is the source format's
/// native object identifier, not an IR arena identifier. `instance_path`
/// contains native instance identifiers in outermost-to-innermost order; an
/// empty path means that the object is not nested in an instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SourceObjectAssociation {
    /// The codec of the source format.
    pub format: CodecId,
    /// Source ownership, when it can be established independently of projection.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_geometry_role"
    )]
    pub geometry_role: Option<SourceGeometryRole>,
    /// Native source object identifier.
    #[serde(deserialize_with = "deserialize_object_id")]
    pub object_id: cadmpeg_core::text::NonBlankString,
    /// Native instance identifiers from outermost to innermost.
    pub instance_path: Vec<String>,
}

crate::units::named_field!(
    deserialize_object_id,
    cadmpeg_core::text::NonBlankString,
    "object_id"
);

/// Provenance for bytes identified by a typed location.
///
/// The location type distinguishes report provenance from a shared
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
/// one. Build a compile-time name with [`crate::stream_name!`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct StreamName(std::borrow::Cow<'static, str>);

#[cfg(feature = "schema")]
impl JsonSchema for StreamName {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "StreamName".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": "string", "minLength": 1})
    }
}

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

    /// Append a value to an admitted stream-name prefix.
    ///
    /// The prefix proves non-empty output.
    #[must_use]
    pub fn with_suffix(self, suffix: impl fmt::Display) -> Self {
        Self(std::borrow::Cow::Owned(format!("{}{}", self.0, suffix)))
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

/// Owner of retained source bytes. An empty wire spelling names the root stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceOwner {
    /// Bytes in the root source stream.
    Root,
    /// Bytes in a named container stream.
    Named(StreamName),
}

impl SourceOwner {
    /// Return the retained-record wire spelling.
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Root => "",
            Self::Named(stream) => stream.as_str(),
        }
    }
}

impl From<String> for SourceOwner {
    fn from(stream: String) -> Self {
        if stream.is_empty() {
            Self::Root
        } else {
            Self::Named(StreamName(std::borrow::Cow::Owned(stream)))
        }
    }
}

impl From<&str> for SourceOwner {
    fn from(stream: &str) -> Self {
        Self::from(stream.to_owned())
    }
}

impl From<StreamName> for SourceOwner {
    fn from(stream: StreamName) -> Self {
        Self::Named(stream)
    }
}

impl Serialize for SourceOwner {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SourceOwner {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from(String::deserialize(deserializer)?))
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
/// The admitted name travels with the provenance in memory and on the wire.
/// Moving or merging annotations cannot invalidate the stream reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationLocation {
    stream: Arc<StreamName>,
}

/// Provenance attached to an entity in [`crate::Annotations`].
pub type AnnotationProvenance = Provenance<AnnotationLocation>;

impl Provenance<AnnotationLocation> {
    pub(crate) fn annotation(stream: Arc<StreamName>, offset: u64, tag: Option<String>) -> Self {
        Self {
            location: AnnotationLocation { stream },
            offset,
            tag,
        }
    }

    /// Return the owned source stream name.
    #[must_use]
    pub fn stream(&self) -> &str {
        self.location.stream.as_str()
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
        deserialize_with = "deserialize_tag"
    )]
    tag: Option<String>,
}

impl<'de> Deserialize<'de> for Provenance<AnnotationLocation> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = AnnotationProvenanceWire::deserialize(deserializer)?;
        Ok(Self {
            location: AnnotationLocation {
                stream: Arc::new(wire.stream),
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
        deserialize_with = "deserialize_stream"
    )]
    stream: Option<StreamName>,
    offset: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tag"
    )]
    tag: Option<String>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
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
    #[cfg(feature = "schema")]
    use super::StreamName;
    use super::{SourceObjectAssociation, SourceProvenance};
    use crate::test_support::wire;

    #[cfg(feature = "schema")]
    #[test]
    fn provenance_schemas_use_admitted_stream_names_and_codec_ids() {
        let schema = serde_json::to_value(schemars::schema_for!(StreamName)).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["minLength"], 1);
        let schema = serde_json::to_value(schemars::schema_for!(SourceObjectAssociation)).unwrap();
        assert_eq!(schema["properties"]["format"]["$ref"], "#/$defs/CodecId");
        assert_eq!(schema["$defs"]["CodecId"]["pattern"], "^[a-z0-9]+$");
    }

    #[test]
    fn a_source_object_names_any_codec_id() {
        for format in ["3dm", "usd", "step"] {
            let wire =
                serde_json::json!({"format": format, "object_id": "#42", "instance_path": []});
            let value: SourceObjectAssociation =
                serde_json::from_value(wire.clone()).expect("a codec id");
            assert_eq!(value.format.as_str(), format);
            assert_eq!(serde_json::to_value(value).expect("serialize"), wire);
        }
        for format in ["", "Step", "st-ep", "step "] {
            serde_json::from_value::<SourceObjectAssociation>(
                serde_json::json!({"format": format, "object_id": "#42", "instance_path": []}),
            )
            .expect_err("not a codec id");
        }
    }

    #[test]
    fn a_source_provenance_stream_is_named_or_absent() {
        let root = SourceProvenance::root("iges", 12);
        let wire = serde_json::to_value(&root).expect("serialize");
        assert_eq!(wire, serde_json::json!({"format": "iges", "offset": 12}));
        let read: SourceProvenance = serde_json::from_value(wire).expect("root reads back");
        assert_eq!(
            wire::field_or_default::<Option<String>>(&read, "stream").as_deref(),
            None
        );
        assert_eq!(read, root);

        let named = SourceProvenance::in_stream("fcstd", crate::stream_name!("Document.xml"), 0);
        let wire = serde_json::to_value(&named).expect("serialize");
        assert_eq!(
            wire,
            serde_json::json!({"format": "fcstd", "stream": "Document.xml", "offset": 0})
        );
        let read: SourceProvenance = serde_json::from_value(wire).expect("named reads back");
        assert_eq!(
            wire::field_or_default::<Option<String>>(&read, "stream").as_deref(),
            Some("Document.xml")
        );

        let error = serde_json::from_value::<SourceProvenance>(
            serde_json::json!({"format": "iges", "stream": "", "offset": 0}),
        )
        .expect_err("the empty string does not name a stream")
        .to_string();
        assert!(error.contains("stream name cannot be empty"), "{error}");
    }

    #[test]
    fn stream_name_composition_keeps_the_admitted_prefix() {
        let prefix = crate::stream_name!("codec:");
        assert_eq!(prefix.clone().with_suffix("").as_str(), "codec:");
        assert_eq!(prefix.with_suffix(7).as_str(), "codec:7");
    }

    #[test]
    fn stream_suffix_uses_bytes_without_collection_slots() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let name = crate::stream_name!("codec:").with_suffix(7);
        assert_eq!(name.as_str(), "codec:7");
        ctx.finish_session().unwrap();
    }

    #[test]
    fn source_object_identity_is_nonempty_and_preserves_wire() {
        let wire = serde_json::json!({"format": "step", "object_id": "#42", "instance_path": []});
        let value: SourceObjectAssociation =
            serde_json::from_value(wire.clone()).expect("nonempty identity");
        assert_eq!(serde_json::to_value(value).expect("serialize"), wire);
        let error = serde_json::from_value::<SourceObjectAssociation>(
            serde_json::json!({"format": "step", "object_id": "", "instance_path": []}),
        )
        .expect_err("empty identity");
        assert!(error.to_string().contains("object_id"));
    }

    #[test]
    fn source_geometry_role_preserves_ownership_and_requires_omission_for_absence() {
        for role in ["independent", "support"] {
            let wire = serde_json::json!({
                "format": "iges", "object_id": "D1", "instance_path": [], "geometry_role": role
            });
            let value: SourceObjectAssociation =
                serde_json::from_value(wire.clone()).expect("source ownership");
            assert_eq!(serde_json::to_value(value).expect("serialize"), wire);
        }
        for role in [serde_json::Value::Null, serde_json::json!("unknown")] {
            let error = serde_json::from_value::<SourceObjectAssociation>(serde_json::json!({
                "format": "iges", "object_id": "D1", "instance_path": [], "geometry_role": role
            }))
            .expect_err("invalid ownership");
            assert!(error.to_string().contains("geometry_role"));
        }
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(
    deserialize_geometry_role,
    SourceGeometryRole,
    "geometry_role"
);
cadmpeg_core::named_optional_field!(deserialize_tag, String, "tag");
cadmpeg_core::named_optional_field!(deserialize_stream, StreamName, "stream");

mod identity_rewrite;

mod serialization;
