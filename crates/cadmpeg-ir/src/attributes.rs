// SPDX-License-Identifier: Apache-2.0
//! Source-native attributes attached to IR entities.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::drawings::DrawingId;
use crate::features::{ConfigurationId, FeatureId, ParameterId};
use crate::ids::{
    AttributeId, BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, OccurrenceId, PmiId, PointId,
    ProductDefinitionId, RegionId, ShellId, SurfaceId, VertexId,
};
use crate::presentation::PresentationId;
use crate::references::InstanceTarget;
use crate::scalar::FiniteReal;
use crate::semantic_annotations::SemanticAnnotationId;
use crate::sketches::SketchId;
use cadmpeg_core::text::NonBlankString;

/// An entity which owns a source attribute.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, cadmpeg_derive::DeserializeTagged,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum AttributeTarget {
    /// Attribute is owned by the document as a whole, not a specific entity.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
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
    Region(RegionId),
    /// Attribute is owned by a surface carrier.
    Surface(SurfaceId),
    /// Attribute is owned by a curve carrier.
    Curve(CurveId),
    /// Attribute is owned by a point carrier.
    Point(PointId),
    /// Attribute is owned by a feature of the design.
    Feature(FeatureId),
    /// Attribute is owned by a sketch of the design.
    Sketch(SketchId),
    /// Attribute is owned by a design parameter.
    Parameter(ParameterId),
    /// Attribute is owned by a product definition.
    ProductDefinition(ProductDefinitionId),
    /// Attribute is owned by an occurrence.
    Occurrence(OccurrenceId),
    /// Attribute is owned by a tessellation, by its id.
    Tessellation(String),
    /// Attribute is owned by a PMI annotation.
    Pmi(PmiId),
    /// Attribute is owned by a semantic annotation.
    SemanticAnnotation(SemanticAnnotationId),
    /// Attribute is owned by a drawing record.
    Drawing(DrawingId),
    /// Attribute is owned by a view presentation.
    ViewPresentation(PresentationId),
    /// A configuration's own value of the like-named attribute of its
    /// scope's definition.
    Configuration(ConfigurationId),
    /// Attribute is owned by a neutral record that no other arm names: the
    /// field name of its arena in the model, the design, or the topology, and
    /// its id.
    Record {
        /// The arena's field name.
        arena: NonBlankString,
        /// The record's id.
        id: String,
    },
}

/// One ordered typed value from a source attribute record.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
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
    /// A link to a local object, by the source's object name.
    Reference(String),
    /// The design parameter that a property is where the source states its
    /// value by an expression.
    Parameter(ParameterId),
    /// A fixed- or variable-length numeric vector value.
    Vector(#[cfg_attr(feature = "schema", schemars(with = "Vec<f64>"))] Vec<FiniteReal>),
    /// An ordered map, repeated keys kept.
    Map(Vec<AttributeMapEntry>),
    /// A link to sub-elements of a local object.
    SubReference {
        /// The source's object name, or `None` for the attribute's own
        /// object.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_object"
        )]
        object: Option<String>,
        /// The source's sub-element names, in source order.
        subelements: Vec<String>,
    },
    /// A link to an object of another document.
    ExternalReference {
        /// The other document.
        document: String,
        /// The source's object name in that document, or `None` for the
        /// document itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_object"
        )]
        object: Option<String>,
        /// The source's sub-element names, in source order.
        subelements: Vec<String>,
    },
    /// An empty link slot.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    EmptyReference,
    /// A typed JSON tree.
    Json(AttributeJson),
}

/// One entry of an ordered attribute map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AttributeMapEntry {
    /// The entry's key.
    pub key: String,
    /// The entry's value.
    pub value: String,
}

/// A typed JSON value.
///
/// Its nesting has no ceiling of its own: the JSON depth limit bounds it.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum AttributeJson {
    /// `null`.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Null,
    /// A Boolean.
    Boolean(bool),
    /// A signed integer.
    Integer(i64),
    /// An unsigned integer.
    Unsigned(u64),
    /// A finite number.
    Float(#[cfg_attr(feature = "schema", schemars(with = "f64"))] FiniteReal),
    /// A text.
    String(String),
    /// An array.
    Array(Vec<AttributeJson>),
    /// An object, each key once.
    Object(
        #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
        BTreeMap<String, AttributeJson>,
    ),
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
    /// The entity this attribute is attached to, in the placed instance and
    /// the state that hold it.
    pub target: InstanceTarget<AttributeTarget>,
    /// Source attribute name, as recorded in the native attribute table.
    pub name: String,
    /// Ordered typed values carried by this attribute; length and types are
    /// source-defined and vary per attribute name. `None` where the source
    /// states no value at that position.
    pub values: Vec<Option<AttributeValue>>,
    /// The native record this attribute was projected from.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
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

    #[test]
    fn a_configuration_target_and_a_parameter_value_are_adjacently_tagged() {
        let target = AttributeTarget::Configuration(
            crate::features::ConfigurationId::mint("test:design:configuration#large").unwrap(),
        );
        let wire = serde_json::json!({"kind": "configuration", "value": "test:design:configuration#large"});
        assert_eq!(
            cadmpeg_core::serde_erase::json::to_value(&target).unwrap(),
            wire
        );
        assert_eq!(
            cadmpeg_core::serde_erase::json::from_value::<AttributeTarget>(wire).unwrap(),
            target
        );
        let value = super::AttributeValue::Parameter(
            crate::features::ParameterId::mint("test:design:parameter#mass").unwrap(),
        );
        let wire = serde_json::json!({"kind": "parameter", "value": "test:design:parameter#mass"});
        assert_eq!(
            cadmpeg_core::serde_erase::json::to_value(&value).unwrap(),
            wire
        );
        assert_eq!(
            cadmpeg_core::serde_erase::json::from_value::<super::AttributeValue>(wire).unwrap(),
            value
        );
    }

    #[test]
    fn a_configuration_target_and_a_parameter_value_name_design_records() {
        use crate::report::check::Check;

        let mut ir = crate::document::CadIr::empty();
        ir.model.attributes.push(super::SourceAttribute {
            id: crate::ids::AttributeId::mint("test:model:attribute#part-number").unwrap(),
            target: AttributeTarget::Configuration(
                crate::features::ConfigurationId::mint("test:design:configuration#large").unwrap(),
            )
            .into(),
            name: "Part Number".into(),
            values: vec![Some(super::AttributeValue::Parameter(
                crate::features::ParameterId::mint("test:design:parameter#mass").unwrap(),
            ))],
            native_ref: None,
        });
        let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
        let missing = report
            .findings
            .iter()
            .filter(|finding| finding.check == Check::ReferentialIntegrity)
            .map(|finding| finding.message.as_str())
            .collect::<Vec<_>>();
        assert!(
            missing
                .iter()
                .any(|message| message.contains("configuration")),
            "{missing:?}"
        );
        assert!(
            missing.iter().any(|message| message.contains("parameter")),
            "{missing:?}"
        );
    }
}

#[cfg(test)]
mod wire_forms;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_object, String, "object");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");

mod identity_rewrite;
