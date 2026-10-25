// SPDX-License-Identifier: Apache-2.0
//! Source-native attributes attached to IR entities.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{AttributeId, BodyId, CoedgeId, EdgeId, FaceId, LoopId, ShellId, VertexId};
use crate::scalar::FiniteReal;
use crate::units::FiniteVector;

/// One ordered map entry. Repeated keys remain distinct entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AttributeMapEntry {
    /// Map key.
    pub key: String,
    /// Map value.
    pub value: String,
}

/// A property expression with typed parameter references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AttributeExpression {
    /// Property path selected by the expression.
    pub path: String,
    /// Ordered text fragments and parameter references.
    pub expression: crate::features::Expression,
    /// Optional expression comment.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_comment"
    )]
    pub comment: Option<String>,
}

/// A position and quaternion, in component order x, y, z, w.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AttributePlacement {
    /// Position coordinates.
    pub position: FiniteVector<3>,
    /// Quaternion components, with at least one nonzero component.
    pub quaternion: FiniteVector<4>,
}

/// A numeric collection with known slots or unknown whole arity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "values",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AttributeNumericList {
    /// Finite scalars. A null list has unknown arity; a null item is unavailable.
    Float(
        #[cfg_attr(feature = "schema", schemars(with = "Option<Vec<Option<f64>>>"))]
        Option<Vec<Option<FiniteReal>>>,
    ),
    /// Three-component finite vectors with the same nullable slot semantics.
    Vector(Option<Vec<Option<FiniteVector<3>>>>),
    /// Curvature records retain their eight numeric components and slot positions.
    Curvature(Option<Vec<Option<FiniteVector<8>>>>),
    /// Position and quaternion values with the same nullable slot semantics.
    Placement(Option<Vec<Option<AttributePlacement>>>),
    /// Packed color integers with the same nullable slot semantics.
    Color(Option<Vec<Option<u32>>>),
}

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

/// One ordered typed value from a source attribute record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
    Reference(String),
    /// A fixed- or variable-length numeric vector value.
    Vector(#[cfg_attr(feature = "schema", schemars(with = "Vec<f64>"))] Vec<FiniteReal>),
    /// Ordered map entries, including repeated keys.
    Map(Vec<AttributeMapEntry>),
    /// A property expression not consumed by a held parameter.
    Expression(AttributeExpression),
    /// A local object reference with ordered subelement names.
    SubReference {
        /// Identity of the referenced local object.
        object: String,
        /// Ordered subelement names.
        subelements: Vec<String>,
    },
    /// A reference into another document.
    ExternalReference {
        /// External document identifier or path.
        document: String,
        /// Object name in that external document.
        object: String,
        /// Ordered subelement names.
        subelements: Vec<String>,
    },
    /// A numeric collection that preserves unavailable positions and whole arity.
    NumericList(AttributeNumericList),
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
    pub values: Vec<AttributeValue>,
}

#[cfg(test)]
mod tests {
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

#[cfg(test)]
mod value_tests;

cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");

cadmpeg_core::named_optional_field!(deserialize_comment, String, "comment");
