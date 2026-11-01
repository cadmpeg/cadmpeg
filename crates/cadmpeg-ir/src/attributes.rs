// SPDX-License-Identifier: Apache-2.0
//! Source-native attributes attached to IR entities.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{AttributeId, BodyId, CoedgeId, EdgeId, FaceId, LoopId, ShellId, VertexId};
use crate::scalar::FiniteReal;

/// One ordered map entry. Repeated keys remain separate entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AttributeMapEntry {
    /// Source key.
    pub key: String,
    /// Source value.
    pub value: String,
}

/// An expression carried by an attribute rather than by a parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AttributeExpression {
    /// Source property path.
    pub path: String,
    /// Text and resolved parameter tokens.
    pub expression: crate::features::Expression,
    /// Source comment, when present.
    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
    #[cfg_attr(
        feature = "schema",
        schemars(required, schema_with = "nullable_string_schema")
    )]
    pub comment: Option<String>,
}

mod json;
pub use json::AttributeJson;

/// An entity which owns a source attribute.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum AttributeTarget {
    /// Attribute is owned by the document as a whole, not a specific entity.
    Document,
    /// Attribute is owned by a body.
    Body(BodyId),
    /// Attribute is owned by a face.
    Face(FaceId),
    /// Attribute is owned by a shell.
    Shell(ShellId),
    /// Attribute is owned by a loop.
    Loop(LoopId),
    /// Attribute is owned by a coedge.
    Coedge(CoedgeId),
    /// Attribute is owned by an edge.
    Edge(EdgeId),
    /// Attribute is owned by a vertex.
    Vertex(VertexId),
    /// Attribute is owned by a region.
    Region(crate::ids::RegionId),
    /// Attribute is owned by a surface carrier.
    Surface(crate::ids::SurfaceId),
    /// Attribute is owned by a curve carrier.
    Curve(crate::ids::CurveId),
    /// Attribute is owned by a point carrier.
    Point(crate::ids::PointId),
    /// Attribute is owned by a feature definition.
    Feature(crate::features::FeatureId),
    /// Attribute is owned by a sketch definition.
    Sketch(crate::sketches::SketchId),
    /// Attribute is owned by a parameter.
    Parameter(crate::features::ParameterId),
    /// Attribute is owned by a reusable product.
    ProductDefinition(crate::ids::ProductDefinitionId),
    /// Attribute is owned by a placed occurrence.
    Occurrence(crate::ids::OccurrenceId),
    /// Attribute is owned by a tessellation.
    Tessellation(String),
    /// Attribute is owned by a PMI annotation.
    Pmi(crate::ids::PmiId),
    /// Attribute is owned by a semantic annotation.
    SemanticAnnotation(crate::semantic_annotations::SemanticAnnotationId),
    /// Attribute is owned by a drawing.
    Drawing(crate::drawings::DrawingId),
    /// Attribute is owned by an object view provider.
    ViewPresentation(crate::presentation::PresentationId),
    /// Ownership by an existing neutral record in a registered model arena.
    Record {
        /// Registered neutral arena containing the record.
        arena: crate::document::ArenaName,
        /// Identity of a record in that arena.
        id: String,
    },
    /// Attribute is owned by a retained typed native object with no neutral entity.
    Source {
        /// Identity of an existing typed native record or source-object association.
        source_id: String,
    },
}

impl AttributeTarget {
    /// Record target for the first neutral record linked to a native owner.
    ///
    /// Returns `None` when no neutral record's `native_ref` names the owner;
    /// the attribute then has no neutral home and is not projected.
    pub fn record_for(model: &crate::document::Model, owner: &str) -> Option<Self> {
        crate::schema::native_links::linking_record(model, owner).map(|(arena, id)| Self::Record {
            arena,
            id,
        })
    }
}

/// One ordered typed value from a source attribute record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(!try_from))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields, try_from = "AttributeValueWire")]
pub enum AttributeValue {
    /// A signed integer value.
    Integer(i64),
    /// A floating-point value.
    Float(#[cfg_attr(feature = "schema", schemars(with = "f64"))] FiniteReal),
    /// A text value.
    String(String),
    /// A boolean value.
    Boolean(bool),
    /// A string-encoded reference to another entity, opaque to this crate.
    Reference(
        #[serde(deserialize_with = "deserialize_local_object_reference")]
        #[cfg_attr(feature = "schema", schemars(length(min = 1)))]
        String,
    ),
    /// A fixed- or variable-length numeric vector value.
    Vector(#[cfg_attr(feature = "schema", schemars(with = "Vec<f64>"))] Vec<FiniteReal>),
    /// Ordered source map entries, including repeated keys.
    Map(Vec<AttributeMapEntry>),
    /// A property expression not consumed by a held parameter.
    Expression(AttributeExpression),
    /// A local object link with source subelement names.
    SubReference {
        /// Native object identity, absent when the source states no object.
        #[cfg_attr(
            feature = "schema",
            schemars(required, schema_with = "nullable_string_schema", length(min = 1))
        )]
        object: Option<String>,
        /// Ordered subelement names.
        #[cfg_attr(feature = "schema", schemars(length(min = 1)))]
        subelements: Vec<String>,
    },
    /// A link into another document.
    ExternalReference {
        /// Source document name or path.
        document: String,
        /// External object name, absent when the source states no object.
        #[cfg_attr(
            feature = "schema",
            schemars(required, schema_with = "nullable_string_schema", length(min = 1))
        )]
        object: Option<String>,
        /// Ordered subelement names.
        subelements: Vec<String>,
    },
    /// A readable local link slot with no object and no subelement name.
    EmptyReference,
    /// A closed, typed JSON tree.
    Json(AttributeJson),
}

impl AttributeValue {
    /// A float value, or `None` when `value` is not finite.
    #[must_use]
    pub fn float(value: f64) -> Option<Self> {
        FiniteReal::new(value).map(Self::Float)
    }

    /// A vector value, or `None` when a component is not finite.
    pub fn vector(values: impl IntoIterator<Item = f64>) -> Option<Self> {
        values
            .into_iter()
            .map(FiniteReal::new)
            .collect::<Option<Vec<_>>>()
            .map(Self::Vector)
    }
}

/// A linked source attribute record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SourceAttribute {
    /// Stable id of this attribute record.
    pub id: AttributeId,
    /// Typed native record from which this neutral record was projected.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
    /// Entity this attribute is attached to.
    pub target: AttributeTarget,
    /// Source attribute name, as recorded in the native attribute table.
    pub name: String,
    /// Ordered typed values carried by this attribute; length and types are
    /// source-defined and vary per attribute name.
    pub values: Vec<Option<AttributeValue>>,
}

impl SourceAttribute {
    /// Construct an attribute whose value slots were all read.
    pub fn new(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        id: AttributeId,
        native_ref: Option<String>,
        target: AttributeTarget,
        name: String,
        values: Vec<AttributeValue>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut slots = Vec::new();
        ctx.reserve_working(&mut slots, values.len(), "attribute value slots")?;
        slots.extend(values.into_iter().map(Some));
        Ok(Self {
            id,
            native_ref,
            target,
            name,
            values: slots,
        })
    }
}

#[cfg(test)]
mod tests {
    mod named_values;
    mod nullable_values;
    use super::AttributeTarget;

    #[test]
    fn attribute_target_copy_preserves_the_target() {
        let target = AttributeTarget::Face(crate::ids::FaceId::mint("test:model:face#17").unwrap());
        assert_eq!(target.clone(), target);
        assert_eq!(AttributeTarget::Document.clone(), AttributeTarget::Document);
    }
}

mod identity_rewrite;
mod value_rewrite;
mod wire;
use wire::AttributeValueWire;

cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");

fn deserialize_local_object_reference<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<String, D::Error> {
    let value = String::deserialize(d)?;
    if value.is_empty() {
        Err(serde::de::Error::custom(
            "an absent local object uses empty_reference",
        ))
    } else {
        Ok(value)
    }
}

#[cfg(feature = "schema")]
fn nullable_string_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    <Option<String> as schemars::JsonSchema>::json_schema(generator)
}
