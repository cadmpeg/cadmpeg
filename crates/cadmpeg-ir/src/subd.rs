// SPDX-License-Identifier: Apache-2.0
//! Subdivision-surface control cages.

use crate::ids::SubdId;
use crate::math::Point3;
use crate::provenance::SourceObjectAssociation;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A subdivision surface represented by its control cage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubdScheme {
    /// Catmull-Clark subdivision.
    CatmullClark,
}

/// A control-cage vertex and its subdivision tag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubdVertex {
    /// Vertex position.
    pub point: Point3,
    /// Subdivision vertex tag.
    pub tag: SubdVertexTag,
}

/// A control-cage vertex tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubdEdge {
    /// Indices of the two distinct endpoint vertices.
    pub vertices: [u32; 2],
    /// Sharpness at the start and end endpoints.
    pub sharpness: [f64; 2],
    /// Subdivision edge tag.
    pub tag: SubdEdgeTag,
    /// Sector coefficients at the two endpoints.
    pub sector_coefficients: [f64; 2],
}

/// A control-cage edge tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubdFace {
    /// Ordered directed edge uses forming the face boundary.
    pub edges: Vec<SubdEdgeUse>,
}

/// One directed use of a subdivision edge in a face ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SubdEdgeUse {
    /// Index into the parent surface's edge array.
    pub edge: u32,
    /// Whether this use traverses the edge from its second endpoint.
    pub reversed: bool,
}
