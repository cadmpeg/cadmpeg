// SPDX-License-Identifier: Apache-2.0
//! Datum plane references, datum point constructions and decal mappings.

use super::selections::EdgeSelection;
use super::selections::FaceSelection;
use super::selections::VertexSelection;
use super::FeatureSupportPlaneFrame;
use crate::scalar::Fraction;
use crate::FeatureId;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parametric support of an offset datum plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
