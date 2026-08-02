// SPDX-License-Identifier: Apache-2.0
//! Boundary-representation topology.
//!
//! Flat arenas in [`crate::document::Model`] store the hierarchy
//! `body → region → shell → face → loop → coedge → edge → vertex`. Faces,
//! edges, coedges, and vertices reference surface, curve, pcurve, and point
//! carriers by typed ID.

use crate::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use crate::math::Point3;
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

/// Orientation relative to referenced geometry.
///
/// For a coedge this compares traversal with its edge curve. For a face it
/// compares the face normal with its surface normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Sense {
    /// Same direction as the referenced geometry.
    Forward,
    /// Opposite direction to the referenced geometry.
    Reversed,
}

/// A top-level solid, sheet, wire, or general body.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    /// A closed, volume-bounding solid body.
    #[default]
    Solid,
    /// An open, zero-thickness sheet body.
    Sheet,
    /// A one-dimensional body composed of wires.
    Wire,
    /// A body containing mixed-dimensional topology.
    General,
}

/// A top-level solid, sheet, wire, or general body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Body {
    /// Arena id.
    pub id: BodyId,
    /// The dimensional kind of topology contained by the body.
    #[serde(default)]
    pub kind: BodyKind,
    /// Constituent regions.
    pub regions: Vec<RegionId>,
    /// Optional world placement of the body's geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<Transform>,
    /// Optional display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional display color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Whether the source document displays the body. `None` when the source
    /// format does not record body visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
}

/// A connected region of a body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Region {
    /// Arena id.
    pub id: RegionId,
    /// Owning body.
    pub body: BodyId,
    /// Boundary shells (typically one outer, plus voids).
    pub shells: Vec<ShellId>,
}

/// An oriented boundary of a region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Shell {
    /// Arena id.
    pub id: ShellId,
    /// Owning region.
    pub region: RegionId,
    /// Faces of the shell.
    pub faces: Vec<FaceId>,
    /// Edges belonging directly to a wire shell.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wire_edges: Vec<EdgeId>,
    /// Vertices belonging directly to a shell and not bounding an edge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub free_vertices: Vec<VertexId>,
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
    /// Optional geometric tolerance in the document's length unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f64>,
}

/// A loop's boundary role within its owning face.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoopBoundaryRole {
    /// The source does not classify this loop as outer or inner.
    #[default]
    Unspecified,
    /// The loop is the explicit exterior boundary of the face.
    Outer,
    /// The loop bounds material excluded from the face; all loops may be inner
    /// when the surface parameter domain supplies the exterior boundary.
    Inner,
}

/// A closed boundary of a face, expressed as an ordered ring of coedges or one
/// vertex use at a surface singularity. The ordering in `coedges` is the ring
/// order; each coedge's `next` should point to the following entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Loop {
    /// Arena id.
    pub id: LoopId,
    /// Owning face.
    pub face: FaceId,
    /// Boundary role within the owning face.
    #[serde(default)]
    pub boundary_role: LoopBoundaryRole,
    /// Coedges in ring order for an edge loop.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coedges: Vec<CoedgeId>,
    /// Ordered pole-vertex occurrences within the cyclic loop traversal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vertex_uses: Vec<VertexUse>,
}

/// One ordered parameter-space representation of a coedge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PcurveUse {
    /// Parameter-space curve carrier.
    pub pcurve: PcurveId,
    /// Whether the source declares this curve isoparametric on the face surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isoparametric: Option<bool>,
    /// Interval on the pcurve's own parameterization used by this coedge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_range: Option<[f64; 2]>,
}

/// One pole-vertex occurrence in a loop traversal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VertexUse {
    /// Referenced pole vertex.
    pub vertex: VertexId,
    /// Preceding coedge in the cyclic traversal, absent for a vertex-only loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<CoedgeId>,
    /// Ordered parameter-space images associated with this pole occurrence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pcurves: Vec<PcurveUse>,
}

/// One use of an edge by a loop.
///
/// Coedges form a loop ring through `next` and `previous`, and a radial ring
/// around their shared edge through `radial_next`.
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
    /// Next coedge around the edge; self-reference denotes a laminar boundary.
    pub radial_next: CoedgeId,
    /// Direction relative to the edge curve.
    pub sense: Sense,
    /// Ordered parameter-space images of this coedge on the face surface.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pcurves: Vec<PcurveUse>,
    /// Optional coedge-local 3D carrier used instead of the shared edge curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_curve: Option<CurveId>,
    /// Interval on the coedge-local 3D carrier in loop-traversal order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_curve_parameter_range: Option<[f64; 2]>,
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
    /// Parameter range `[t_start, t_end]` on the curve's own
    /// parameterization, when known: the start vertex lies at `t_start`.
    /// Conic parameters are angles from the reference direction; line
    /// parameters are signed distances along the unit direction in the
    /// document's length unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param_range: Option<[f64; 2]>,
    /// Optional geometric tolerance in the document's length unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f64>,
}

/// A vertex: a topological point referencing a position carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Vertex {
    /// Arena id.
    pub id: VertexId,
    /// Position carrier.
    pub point: PointId,
    /// Optional geometric tolerance in the document's length unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f64>,
}

/// A position carrier for a vertex.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Point {
    /// Arena id.
    pub id: PointId,
    /// Coordinates in the document's length unit.
    pub position: Point3,
    /// Source object carrying this free point, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_object: Option<crate::provenance::SourceObjectAssociation>,
}
