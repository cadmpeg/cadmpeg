// SPDX-License-Identifier: Apache-2.0
//! The B-rep topology graph.
//!
//! Layout follows the ASM/ACIS hierarchy documented in the f3d topology spec:
//! `body → lump → shell → face → loop → coedge → edge → vertex`, with geometry
//! attached by reference (`face → surface`, `edge → curve`, `coedge → pcurve`,
//! `vertex → point`). Entities are stored in flat arenas on
//! [`crate::document::CadIr`] and refer to each other by id.

use crate::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, LumpId, PcurveId, PointId, ShellId,
    SurfaceId, VertexId,
};
use crate::math::Point3;
use crate::provenance::EntityMeta;
use crate::transform::Transform;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// RGBA color, components in `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Color {
    /// Red.
    pub r: f32,
    /// Green.
    pub g: f32,
    /// Blue.
    pub b: f32,
    /// Alpha (opacity).
    pub a: f32,
}

/// Orientation of an entity relative to the geometry it references. On a coedge
/// it says whether the coedge runs along or against its edge's curve; on a face
/// whether the face normal agrees with its surface normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Sense {
    /// Same direction as the referenced geometry.
    Forward,
    /// Opposite direction to the referenced geometry.
    Reversed,
}

/// A top-level solid or sheet body.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    #[default]
    Solid,
    Sheet,
}

/// A top-level solid or sheet body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Body {
    /// Arena id.
    pub id: BodyId,
    /// Whether the body encloses volume or represents an open sheet.
    #[serde(default)]
    pub kind: BodyKind,
    /// Constituent lumps.
    pub lumps: Vec<LumpId>,
    /// Optional world placement of the body's geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<Transform>,
    /// Optional display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional display color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Provenance/exactness metadata.
    pub meta: EntityMeta,
}

/// A connected region of a body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Lump {
    /// Arena id.
    pub id: LumpId,
    /// Owning body.
    pub body: BodyId,
    /// Boundary shells (typically one outer, plus voids).
    pub shells: Vec<ShellId>,
    /// Provenance/exactness metadata.
    pub meta: EntityMeta,
}

/// An oriented boundary of a lump.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Shell {
    /// Arena id.
    pub id: ShellId,
    /// Owning lump.
    pub lump: LumpId,
    /// Faces of the shell.
    pub faces: Vec<FaceId>,
    /// Provenance/exactness metadata.
    pub meta: EntityMeta,
}

/// A face: a bounded region of a surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Face {
    /// Arena id.
    pub id: FaceId,
    /// Owning shell.
    pub shell: ShellId,
    /// Underlying surface carrier.
    pub surface: SurfaceId,
    /// Whether the face normal agrees with the surface normal.
    pub sense: Sense,
    /// Boundary loops (first is conventionally the outer loop).
    pub loops: Vec<LoopId>,
    /// Optional display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional display color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Provenance/exactness metadata.
    pub meta: EntityMeta,
}

/// A closed boundary loop of a face, expressed as an ordered ring of coedges.
/// The ordering in `coedges` is the ring order; each coedge's `next` should
/// point to the following entry (validation enforces the ring closes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Loop {
    /// Arena id.
    pub id: LoopId,
    /// Owning face.
    pub face: FaceId,
    /// Coedges in ring order.
    pub coedges: Vec<CoedgeId>,
    /// Provenance/exactness metadata.
    pub meta: EntityMeta,
}

/// A coedge: one side of an edge as used by a particular loop. Each edge is
/// shared by exactly two coedges (its `partner`), one per adjacent face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Coedge {
    /// Arena id.
    pub id: CoedgeId,
    /// Owning loop.
    pub owner_loop: LoopId,
    /// Underlying edge.
    pub edge: EdgeId,
    /// Next coedge in the loop ring.
    pub next: CoedgeId,
    /// Previous coedge in the loop ring.
    pub previous: CoedgeId,
    /// The coedge on the adjacent face sharing this edge, if the model is
    /// manifold at this edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partner: Option<CoedgeId>,
    /// Direction relative to the edge curve.
    pub sense: Sense,
    /// Optional parameter-space image of this coedge on the face surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcurve: Option<PcurveId>,
    /// Provenance/exactness metadata.
    pub meta: EntityMeta,
}

/// An edge: a bounded segment of a 3D curve between two vertices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Edge {
    /// Arena id.
    pub id: EdgeId,
    /// Underlying 3D curve carrier. `None` for a degenerate/tolerant edge with
    /// no attributed curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<CurveId>,
    /// Start vertex.
    pub start: VertexId,
    /// End vertex.
    pub end: VertexId,
    /// Parameter range `[t_start, t_end]` on the curve, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param_range: Option<[f64; 2]>,
    /// Provenance/exactness metadata.
    pub meta: EntityMeta,
}

/// A vertex: a topological point referencing a position carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Vertex {
    /// Arena id.
    pub id: VertexId,
    /// Position carrier.
    pub point: PointId,
    /// Provenance/exactness metadata.
    pub meta: EntityMeta,
}

/// A position carrier for a vertex.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Point {
    /// Arena id.
    pub id: PointId,
    /// Coordinates in the document's length unit.
    pub position: Point3,
    /// Provenance/exactness metadata.
    pub meta: EntityMeta,
}
