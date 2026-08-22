// SPDX-License-Identifier: Apache-2.0
//! Subdivision-surface control cages.

use crate::ids::SubdId;
use crate::math::Point3;
use crate::provenance::SourceObjectAssociation;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A subdivision surface represented by its control cage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdSurface {
    /// Arena identity.
    pub id: SubdId,
    /// Subdivision scheme.
    pub scheme: SubdScheme,
    /// Control-cage vertices.
    pub vertices: Vec<SubdVertex>,
    /// Control-cage edges.
    pub edges: Vec<SubdEdge>,
    /// Control-cage faces.
    pub faces: Vec<SubdFace>,
    /// Native source-object identity and effective display metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_object: Option<SourceObjectAssociation>,
}

/// Subdivision scheme used by a control cage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SubdScheme {
    /// Catmull-Clark subdivision.
    CatmullClark,
}

/// A control-cage vertex and its subdivision tag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdVertex {
    /// Vertex position.
    pub point: Point3,
    /// Subdivision vertex tag.
    pub tag: SubdVertexTag,
    /// Optional secondary-grip topology owned by this vertex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_grips: Option<SubdVertexGripLayout>,
}

/// Compass direction of the root edge in a control-cage grid frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SubdGripDirection {
    /// Positive grid-y direction.
    North,
    /// Positive grid-x direction.
    East,
    /// Negative grid-y direction.
    South,
    /// Negative grid-x direction.
    West,
}

/// Typed secondary-grip layout for one subdivision vertex.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdVertexGripLayout {
    /// Direction of the native root edge; wedge zero is the north slot.
    pub direction: SubdGripDirection,
    /// Wedges in north-anchored order.
    pub wedges: Vec<SubdGripWedge>,
}

/// One wedge in a secondary-grip layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdGripWedge {
    /// IR edge for this fan slot, or `None` for a phantom boundary slot.
    pub edge: Option<u32>,
    /// Face in the sector following this slot, or `None` for a boundary gap.
    pub sector_face: Option<u32>,
    /// Whether this slot was inserted to complete a boundary gap.
    pub phantom: bool,
    /// Spoke grips ordered nearest-first from the owning vertex.
    pub spokes: Vec<Option<SubdSecondaryGrip>>,
    /// Sector-grid grips ordered by the spoke-k position, then the next-spoke
    /// position, with `S[k] * S[k + 1]` slots.
    pub sectors: Vec<Option<SubdSecondaryGrip>>,
}

/// A secondary grip point and its source grip-array identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdSecondaryGrip {
    /// Index in the source cage's `0g` grip array.
    pub source_index: u32,
    /// Grip position in document units.
    pub point: Point3,
    /// Positive rational grip weight.
    pub weight: f64,
}

/// A control-cage vertex tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SubdVertexTag {
    /// Smooth vertex.
    Smooth,
    /// Crease vertex.
    Crease,
    /// Corner vertex.
    Corner,
    /// Dart vertex.
    Dart,
}

/// A control-cage edge with endpoint sharpness and sector coefficients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdEdge {
    /// Indices of the two distinct endpoint vertices.
    pub vertices: [u32; 2],
    /// Sharpness at the start and end endpoints.
    pub sharpness: [f64; 2],
    /// Subdivision edge tag.
    pub tag: SubdEdgeTag,
    /// Parametric knot interval, when the source cage exposes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knot_interval: Option<f64>,
    /// Sector coefficients at the two endpoints.
    pub sector_coefficients: [f64; 2],
}

/// A control-cage edge tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SubdEdgeTag {
    /// Smooth edge.
    Smooth,
    /// Smooth-X edge with the source's distinct subdivision behavior.
    SmoothX,
    /// Crease edge.
    Crease,
}

/// A subdivision face bounded by a directed edge ring.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdFace {
    /// Ordered directed edge uses forming the face boundary.
    pub edges: Vec<SubdEdgeUse>,
}

/// One directed use of a subdivision edge in a face ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdEdgeUse {
    /// Index into the parent surface's edge array.
    pub edge: u32,
    /// Whether this use traverses the edge from its second endpoint.
    pub reversed: bool,
}

#[cfg(test)]
mod tests;
