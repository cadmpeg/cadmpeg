// SPDX-License-Identifier: Apache-2.0
//! Datum plane references, datum point constructions and decal mappings.

use super::geometry::FeatureSupportPlaneFrame;
use super::selections::{EdgeSelection, FaceSelection, VertexSelection};
use super::FeatureId;
use crate::scalar::Fraction;
use cadmpeg_derive::DeserializeName;

use super::{FeatureOperation, UnresolvedFamily};
use crate::design::construction::ConstructionInput;
use crate::design::options::NativeOptions;
use crate::features::geometry::{FeatureDirection3, FinitePoint3};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};

#[doc = "Readable geometry, construction inputs and source options of a datum axis."]
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DatumAxisDraft")]
pub struct DatumAxis {
    origin: Option<FinitePoint3>,
    direction: Option<FeatureDirection3>,
    inputs: Vec<ConstructionInput>,
    native_options: NativeOptions,
    source_settings: NativeOptions,
}

impl Serialize for DatumAxis {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = 3 + usize::from(self.origin.is_some()) + usize::from(self.direction.is_some());
        let mut wire = serializer.serialize_struct("DatumAxisDraft", fields)?;
        if let Some(origin) = &self.origin {
            wire.serialize_field("origin", origin)?;
        }
        if let Some(direction) = &self.direction {
            wire.serialize_field("direction", direction)?;
        }
        wire.serialize_field("inputs", &self.inputs)?;
        wire.serialize_field("native_options", &self.native_options)?;
        wire.serialize_field("source_settings", &self.source_settings)?;
        wire.end()
    }
}

#[doc = "Input fields for checked datum-axis construction."]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DatumAxisDraft {
    #[doc = "A known point on the axis."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_origin"
    )]
    pub origin: Option<FinitePoint3>,
    #[doc = "The known direction."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direction"
    )]
    pub direction: Option<FeatureDirection3>,
    #[doc = "Established construction input occurrences."]
    pub inputs: Vec<ConstructionInput>,
    #[doc = "Options whose meaning remains unresolved."]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    #[doc = "Source settings with established equivalent design meaning."]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

impl DatumAxisDraft {
    #[doc = "Whether the draft carries a readable axis fact."]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.origin.is_none()
            && self.direction.is_none()
            && self.inputs.is_empty()
            && self.native_options.is_empty()
            && self.source_settings.is_empty()
    }

    #[doc = "Canonicalize a readable draft or the unresolved axis family."]
    pub fn into_operation(self) -> Result<FeatureOperation, &'static str> {
        if self.is_empty() {
            Ok(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumAxis,
            })
        } else {
            DatumAxis::try_from(self).map(|axis| FeatureOperation::DatumAxis { axis })
        }
    }
}

impl TryFrom<DatumAxisDraft> for DatumAxis {
    type Error = &'static str;
    fn try_from(draft: DatumAxisDraft) -> Result<Self, Self::Error> {
        if draft.is_empty() {
            return Err("an empty datum axis uses the unresolved datum-axis family");
        }
        if draft
            .native_options
            .keys()
            .any(|key| draft.source_settings.contains_key(key))
        {
            return Err("datum-axis option keys must occur in one classification");
        }
        let DatumAxisDraft {
            origin,
            direction,
            inputs,
            native_options,
            source_settings,
        } = draft;
        Ok(Self {
            origin,
            direction,
            inputs,
            native_options,
            source_settings,
        })
    }
}

impl From<DatumAxis> for DatumAxisDraft {
    fn from(axis: DatumAxis) -> Self {
        let DatumAxis {
            origin,
            direction,
            inputs,
            native_options,
            source_settings,
        } = axis;
        Self {
            origin,
            direction,
            inputs,
            native_options,
            source_settings,
        }
    }
}

impl DatumAxis {
    #[doc = "Construct a complete fixed axis."]
    #[must_use]
    pub fn complete(origin: FinitePoint3, direction: FeatureDirection3) -> Self {
        Self {
            origin: Some(origin),
            direction: Some(direction),
            inputs: Vec::new(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        }
    }

    #[doc = "The known origin."]
    #[must_use]
    pub fn origin(&self) -> Option<&FinitePoint3> {
        self.origin.as_ref()
    }

    #[doc = "The known direction."]
    #[must_use]
    pub fn direction(&self) -> Option<&FeatureDirection3> {
        self.direction.as_ref()
    }

    #[doc = "The complete geometric pair, when both components are known."]
    #[must_use]
    pub fn geometry(&self) -> Option<(&FinitePoint3, &FeatureDirection3)> {
        self.origin().zip(self.direction())
    }

    #[doc = "Construction input occurrences."]
    #[must_use]
    pub fn inputs(&self) -> &[ConstructionInput] {
        &self.inputs
    }

    #[doc = "Options with unresolved meaning."]
    #[must_use]
    pub fn native_options(&self) -> &NativeOptions {
        &self.native_options
    }

    #[doc = "Source settings with equivalent design meaning."]
    #[must_use]
    pub fn source_settings(&self) -> &NativeOptions {
        &self.source_settings
    }

    #[doc = "Whether the axis has attached construction or source controls."]
    #[must_use]
    pub fn has_controls(&self) -> bool {
        !self.inputs.is_empty()
            || !self.native_options.is_empty()
            || !self.source_settings.is_empty()
    }
}

impl FeatureOperation {
    #[doc = "Construct a datum axis with complete geometry and empty controls."]
    #[must_use]
    pub fn datum_axis(origin: FinitePoint3, direction: FeatureDirection3) -> Self {
        Self::DatumAxis {
            axis: DatumAxis::complete(origin, direction),
        }
    }
}

rewrite_record!(DatumAxis, []; {origin, direction, inputs, native_options, source_settings});

#[cfg(feature = "schema")]
impl JsonSchema for DatumAxis {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "DatumAxis".into()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = DatumAxisDraft::json_schema(generator);
        schema.insert(
            "allOf".into(),
            cadmpeg_core::json!([{"anyOf": [
                {"required":["origin"]}, {"required":["direction"]},
                {"properties":{"inputs":{"minItems":1}}},
                {"properties":{"native_options":{"minProperties":1}}},
                {"properties":{"source_settings":{"minProperties":1}}}
            ]}]),
        );
        if let Some(serde_json::Value::Object(properties)) = schema.get_mut("properties") {
            properties.insert(
                "origin".into(),
                generator.subschema_for::<FinitePoint3>().into(),
            );
            properties.insert(
                "direction".into(),
                generator.subschema_for::<FeatureDirection3>().into(),
            );
        }
        schema
    }
}

cadmpeg_core::named_optional_field!(deserialize_origin, FinitePoint3, "origin");
cadmpeg_core::named_optional_field!(deserialize_direction, FeatureDirection3, "direction");

#[cfg(test)]
mod tests;

/// Parametric support of an offset datum plane.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DatumPlaneReference {
    /// Another datum-plane feature.
    Feature {
        /// Referenced datum-plane feature.
        feature: FeatureId,
    },
    /// A selected planar face whose geometry defines the support plane.
    Face {
        /// Selected planar face.
        face: FaceSelection,
    },
    /// A resolved support plane without a face identity.
    ResolvedPlane {
        /// Finite support plane with nonzero perpendicular directions.
        frame: FeatureSupportPlaneFrame,
    },
}

/// Sketch point operand resolved by a datum-point construction or retained in
/// native form.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SketchPointSelection {
    /// Selection exists semantically but its sketch entity is not resolved.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Unresolved,
    /// Point in a planar sketch.
    Planar {
        /// Owning planar sketch.
        sketch: crate::sketches::SketchId,
        /// Selected point entity.
        point: crate::sketches::SketchEntityId,
        /// Format-native persistent selection reference.
        native: String,
    },
    /// Point in a model-space sketch.
    Spatial {
        /// Owning model-space sketch.
        sketch: crate::sketches::SpatialSketchId,
        /// Selected point entity.
        point: crate::sketches::SpatialSketchEntityId,
        /// Format-native persistent selection reference.
        native: String,
    },
    /// Format-native selection reference.
    Native(String),
}

/// Construction rule used to derive one datum point.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum DatumPointConstruction {
    /// Center of one selected circular edge.
    CircleCenter {
        /// Selected circular edge.
        edge: EdgeSelection,
    },
    /// Intersection of two selected edges.
    TwoEdgeIntersection {
        /// Selected edges in source order.
        edges: [EdgeSelection; 2],
    },
    /// Intersection of three selected planes.
    ThreePlaneIntersection {
        /// Selected planes in source order.
        planes: Box<[DatumPlaneReference; 3]>,
    },
    /// One selected topological vertex.
    Vertex {
        /// Selected vertex.
        vertex: VertexSelection,
    },
    /// One selected point from a planar or model-space sketch.
    SketchPoint {
        /// Selected sketch point.
        point: SketchPointSelection,
    },
    /// Intersection of one selected edge and one selected plane.
    EdgePlaneIntersection {
        /// Selected edge.
        edge: EdgeSelection,
        /// Selected plane.
        plane: DatumPlaneReference,
    },
    /// Point at a normalized position along one selected edge.
    DistanceOnEdge {
        /// Selected edge.
        edge: EdgeSelection,
        /// Fraction from the path start in the closed interval from zero through one.
        fraction: Fraction,
    },
}

/// Rule that maps a raster decal onto its selected faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum DecalMapping {
    /// Scale the complete raster to the selected faces' native parameter domain.
    FitToFaces,
}

impl DatumPointConstruction {
    /// Return construction features referenced by this rule.
    pub fn feature_references(&self) -> impl Iterator<Item = &FeatureId> {
        let references = match self {
            Self::ThreePlaneIntersection { planes } => planes.each_ref().map(|plane| match plane {
                DatumPlaneReference::Feature { feature } => Some(feature),
                DatumPlaneReference::Face { .. } | DatumPlaneReference::ResolvedPlane { .. } => {
                    None
                }
            }),
            Self::EdgePlaneIntersection {
                plane: DatumPlaneReference::Feature { feature },
                ..
            } => [Some(feature), None, None],
            Self::Vertex {
                vertex: VertexSelection::Generated { vertex, .. },
            } => [Some(&vertex.feature), None, None],
            Self::CircleCenter { .. }
            | Self::TwoEdgeIntersection { .. }
            | Self::Vertex { .. }
            | Self::SketchPoint { .. }
            | Self::EdgePlaneIntersection { .. }
            | Self::DistanceOnEdge { .. } => [None; 3],
        };
        references.into_iter().flatten()
    }
}
