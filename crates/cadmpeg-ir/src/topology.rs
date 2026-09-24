// SPDX-License-Identifier: Apache-2.0
//! Boundary-representation topology.
//!
//! Flat arenas in [`crate::document::Model`] store the hierarchy
//! `body → region → shell → face → loop → coedge → edge → vertex`. Faces,
//! edges, coedges, and vertices reference surface, curve, pcurve, and point
//! carriers by typed ID.

use crate::features::{BodySelectionError, FinitePoint3, NonEmptyMembers};
use crate::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityKey, IdentityNamespace, LoopId, PcurveId,
    PointId, RegionId, ShellId, SurfaceId, VertexId,
};
use crate::math::Point3;
use crate::transform::Transform;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// RGBA color, components in `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "ColorWire")]
pub struct Color {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct ColorWire {
    /// Red component.
    r: f32,
    /// Green component.
    g: f32,
    /// Blue component.
    b: f32,
    /// Opacity component.
    a: f32,
}

impl TryFrom<ColorWire> for Color {
    type Error = String;

    fn try_from(wire: ColorWire) -> Result<Self, Self::Error> {
        for (field, value) in [("r", wire.r), ("g", wire.g), ("b", wire.b), ("a", wire.a)] {
            if !(0.0..=1.0).contains(&value) {
                return Err(format!("color {field} must be finite and in [0, 1]"));
            }
        }
        Ok(Self {
            r: wire.r,
            g: wire.g,
            b: wire.b,
            a: wire.a,
        })
    }
}

impl Color {
    /// Construct RGBA components in the closed unit interval.
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Option<Self> {
        Self::try_from(ColorWire { r, g, b, a }).ok()
    }

    /// Convert eight-bit RGBA components to the closed unit interval.
    pub fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: f32::from(r) / 255.0,
            g: f32::from(g) / 255.0,
            b: f32::from(b) / 255.0,
            a: f32::from(a) / 255.0,
        }
    }

    /// Red component.
    pub const fn r(self) -> f32 {
        self.r
    }
    /// Green component.
    pub const fn g(self) -> f32 {
        self.g
    }
    /// Blue component.
    pub const fn b(self) -> f32 {
        self.b
    }
    /// Opacity component.
    pub const fn a(self) -> f32 {
        self.a
    }

    /// Replace opacity with its complement.
    #[must_use]
    pub fn invert_alpha(self) -> Self {
        Self {
            a: 1.0 - self.a,
            ..self
        }
    }

    /// Replace opacity with a component in the closed unit interval.
    pub fn with_alpha(self, a: f32) -> Option<Self> {
        Self::new(self.r, self.g, self.b, a)
    }
}

/// Orientation relative to referenced geometry.
///
/// For a coedge this compares traversal with its edge curve. For a face it
/// compares the face normal with its surface normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum Sense {
    /// Same direction as the referenced geometry.
    Forward,
    /// Opposite direction to the referenced geometry.
    Reversed,
}

/// A top-level solid, sheet, wire, or general body.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Body {
    /// Arena id.
    pub id: BodyId,
    /// The dimensional kind of topology contained by the body.
    #[serde(default)]
    pub kind: BodyKind,
    /// Constituent regions.
    pub regions: Vec<RegionId>,
    /// Optional world placement of the body's geometry.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    pub transform: Option<Transform>,
    /// Optional display name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Optional display color.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_color"
    )]
    pub color: Option<Color>,
    /// Whether the source document displays the body. `None` when the source
    /// format does not record body visibility.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visible"
    )]
    pub visible: Option<bool>,
}

/// A connected region of a body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Region {
    /// Arena id.
    pub id: RegionId,
    /// Owning body.
    pub body: BodyId,
    /// Ordered boundary shells. For a solid region, the first shell is the
    /// exterior boundary and all subsequent shells bound voids.
    pub shells: Vec<ShellId>,
}

/// One member of a shell: a face, a wire edge, or a free vertex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ShellMember {
    /// A face bounded by the owning shell.
    Face {
        /// Arena id of the face.
        id: FaceId,
    },
    /// An edge belonging directly to a wire shell.
    WireEdge {
        /// Arena id of the edge.
        id: EdgeId,
    },
    /// A vertex belonging directly to the shell and bounding no edge.
    FreeVertex {
        /// Arena id of the vertex.
        id: VertexId,
    },
}

/// The members of a shell, sorted into the three kinds.
///
/// The wire carries one non-empty member list, so "all three lists are empty"
/// has no spelling and no arm refuses it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "NonEmptyMembers<ShellMember>", into = "Vec<ShellMember>")]
struct ShellMembers {
    faces: Vec<FaceId>,
    wire_edges: Vec<EdgeId>,
    free_vertices: Vec<VertexId>,
}

impl From<NonEmptyMembers<ShellMember>> for ShellMembers {
    fn from(members: NonEmptyMembers<ShellMember>) -> Self {
        let mut sorted = Self {
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        };
        for member in members {
            match member {
                ShellMember::Face { id } => sorted.faces.push(id),
                ShellMember::WireEdge { id } => sorted.wire_edges.push(id),
                ShellMember::FreeVertex { id } => sorted.free_vertices.push(id),
            }
        }
        sorted
    }
}

impl From<ShellMembers> for Vec<ShellMember> {
    fn from(members: ShellMembers) -> Self {
        members
            .faces
            .into_iter()
            .map(|id| ShellMember::Face { id })
            .chain(
                members
                    .wire_edges
                    .into_iter()
                    .map(|id| ShellMember::WireEdge { id }),
            )
            .chain(
                members
                    .free_vertices
                    .into_iter()
                    .map(|id| ShellMember::FreeVertex { id }),
            )
            .collect()
    }
}

/// An oriented nonempty boundary of a region.
///
/// The members travel as one tagged list, so a shell that owns nothing is
/// unrepresentable beyond the empty list the member type refuses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Shell {
    /// Arena id.
    pub id: ShellId,
    /// Owning region.
    pub region: RegionId,
    /// Faces, wire edges, and free vertices owned by the shell.
    #[cfg_attr(feature = "schema", schemars(with = "Vec<ShellMember>"))]
    members: ShellMembers,
}

impl Shell {
    /// Admits a shell that owns at least one face, wire edge, or free vertex.
    pub fn new(
        id: ShellId,
        region: RegionId,
        faces: Vec<FaceId>,
        wire_edges: Vec<EdgeId>,
        free_vertices: Vec<VertexId>,
    ) -> Result<Self, BodySelectionError> {
        let members = ShellMembers {
            faces,
            wire_edges,
            free_vertices,
        };
        Ok(Self {
            id,
            region,
            members: NonEmptyMembers::try_from(Vec::<ShellMember>::from(members))?.into(),
        })
    }

    /// Constructs a shell with one face.
    pub fn with_face(id: ShellId, region: RegionId, face: FaceId) -> Self {
        Self {
            id,
            region,
            members: ShellMembers {
                faces: vec![face],
                wire_edges: Vec::new(),
                free_vertices: Vec::new(),
            },
        }
    }

    /// Constructs a shell with one wire edge.
    pub fn with_wire_edge(id: ShellId, region: RegionId, edge: EdgeId) -> Self {
        Self {
            id,
            region,
            members: ShellMembers {
                faces: Vec::new(),
                wire_edges: vec![edge],
                free_vertices: Vec::new(),
            },
        }
    }

    /// Constructs a shell with one free vertex.
    pub fn with_free_vertex(id: ShellId, region: RegionId, vertex: VertexId) -> Self {
        Self {
            id,
            region,
            members: ShellMembers {
                faces: Vec::new(),
                wire_edges: Vec::new(),
                free_vertices: vec![vertex],
            },
        }
    }

    /// Faces of the shell.
    pub fn faces(&self) -> &[FaceId] {
        &self.members.faces
    }

    /// Edges belonging directly to the shell.
    pub fn wire_edges(&self) -> &[EdgeId] {
        &self.members.wire_edges
    }

    /// Vertices belonging directly to the shell.
    pub fn free_vertices(&self) -> &[VertexId] {
        &self.members.free_vertices
    }

    /// Edits topology members and preserves the shell when admission fails.
    pub fn edit_topology<R>(
        &mut self,
        edit: impl FnOnce(&mut Vec<FaceId>, &mut Vec<EdgeId>, &mut Vec<VertexId>) -> R,
    ) -> Result<R, BodySelectionError> {
        let mut faces = self.members.faces.clone();
        let mut wire_edges = self.members.wire_edges.clone();
        let mut free_vertices = self.members.free_vertices.clone();
        let result = edit(&mut faces, &mut wire_edges, &mut free_vertices);
        *self = Self::new(
            self.id.clone(),
            self.region.clone(),
            faces,
            wire_edges,
            free_vertices,
        )?;
        Ok(result)
    }

    /// Appends a face to the shell.
    pub fn add_face(&mut self, face: FaceId) {
        self.members.faces.push(face);
    }

    /// Appends a wire edge to the shell.
    pub fn add_wire_edge(&mut self, edge: EdgeId) {
        self.members.wire_edges.push(edge);
    }

    /// Appends a free vertex to the shell.
    pub fn add_free_vertex(&mut self, vertex: VertexId) {
        self.members.free_vertices.push(vertex);
    }
}

/// A face: a bounded region of a surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Face {
    /// Arena id.
    pub id: FaceId,
    /// Owning shell.
    pub shell: ShellId,
    /// Underlying surface carrier.
    pub surface: SurfaceId,
    /// Whether the face normal agrees with the surface normal.
    pub sense: Sense,
    /// Boundary loops. Classification lives here so a loop cannot disagree
    /// with face membership.
    pub loops: FaceLoops,
    /// Optional display name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Optional display color.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_color"
    )]
    pub color: Option<Color>,
    /// Optional geometric tolerance in the document's length unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_tolerance")]
    pub tolerance: Option<crate::scalar::PositiveReal>,
}

impl Face {
    /// Role of `id` when it is a member of this face.
    #[must_use]
    pub fn loop_role(&self, id: &LoopId) -> LoopBoundaryRole {
        self.loops.role(id)
    }
}

/// Face loop ids in one of the two states a source can state.
///
/// The wire is one tagged object, so a loop has no role of its own to disagree
/// with the face's classification and the round trip is stable by
/// construction. `Unspecified` keeps source order when the source did not
/// classify outer versus inner; `Classified` names the outer loop once and
/// requires it, so a classification that states no outer boundary is not a
/// state.
///
/// `loops` on `Unspecified` is a `Vec`, not a non-empty list: a closed surface
/// with no boundary is a face with no loop, and nx, sat, step and freecad all
/// produce one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "classification", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum FaceLoops {
    /// The source did not classify outer versus inner; loops keep source order.
    Unspecified {
        /// Boundary loops in source order.
        loops: Vec<LoopId>,
    },
    /// The source classified the boundary.
    Classified {
        /// Outer loop. A classified face states one.
        outer: LoopId,
        /// Inner loops in source order.
        inner: Vec<LoopId>,
    },
}

impl FaceLoops {
    /// Source order with no outer/inner classification.
    #[must_use]
    pub const fn unspecified(ids: Vec<LoopId>) -> Self {
        Self::Unspecified { loops: ids }
    }

    /// Classified loops. Loop order is outer then inner.
    #[must_use]
    pub const fn classified(outer: LoopId, inner: Vec<LoopId>) -> Self {
        Self::Classified { outer, inner }
    }

    /// Ordered loop ids: outer first when the face states one.
    pub fn iter(&self) -> impl Iterator<Item = &LoopId> + '_ {
        match self {
            Self::Unspecified { loops } => Box::new(loops.iter()) as Box<dyn Iterator<Item = _>>,
            Self::Classified { outer, inner } => {
                Box::new(std::iter::once(outer).chain(inner.iter()))
            }
        }
    }

    /// Whether `id` is a loop of this face.
    #[must_use]
    pub fn contains(&self, id: &LoopId) -> bool {
        self.iter().any(|member| member == id)
    }

    /// Ordered loop ids as an owned list.
    #[must_use]
    pub fn to_vec(&self) -> Vec<LoopId> {
        self.iter().cloned().collect()
    }

    /// Number of loops.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Unspecified { loops } => loops.len(),
            Self::Classified { inner, .. } => 1 + inner.len(),
        }
    }

    /// Whether the face has no loops.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Role of `id` when it is a member of this face.
    #[must_use]
    pub fn role(&self, id: &LoopId) -> LoopBoundaryRole {
        match self {
            Self::Unspecified { .. } => LoopBoundaryRole::Unspecified,
            Self::Classified { outer, inner } => {
                if outer == id {
                    LoopBoundaryRole::Outer
                } else if inner.iter().any(|member| member == id) {
                    LoopBoundaryRole::Inner
                } else {
                    LoopBoundaryRole::Unspecified
                }
            }
        }
    }
}

impl PartialEq<Vec<LoopId>> for FaceLoops {
    fn eq(&self, other: &Vec<LoopId>) -> bool {
        self.iter().eq(other.iter())
    }
}

impl PartialEq<FaceLoops> for Vec<LoopId> {
    fn eq(&self, other: &FaceLoops) -> bool {
        other.iter().eq(self.iter())
    }
}

impl<'a> IntoIterator for &'a FaceLoops {
    type Item = &'a LoopId;
    type IntoIter = Box<dyn Iterator<Item = &'a LoopId> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

/// Classification of a loop within its owning face.
///
/// Stored on [`FaceLoops`], not on [`Loop`], so a loop cannot disagree with
/// face membership, and carried on the wire beside the loop id it classifies.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
/// vertex use at a surface singularity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Loop {
    /// Arena id.
    pub id: LoopId,
    /// Owning face.
    pub face: FaceId,
    /// Vertex-only or coedge-ring boundary.
    pub boundary: LoopBoundary,
}

/// One ordered parameter-space representation of a coedge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PcurveUse {
    /// Parameter-space curve carrier.
    pub pcurve: PcurveId,
    /// Whether the source declares this curve isoparametric on the face surface.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_isoparametric"
    )]
    pub isoparametric: Option<bool>,
    /// Interval on the pcurve's own parameterization used by this coedge.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_parameter_range"
    )]
    pub parameter_range: Option<crate::geometry::DirectedParameterRange>,
}

/// The mutually exclusive forms of a loop boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LoopBoundary {
    /// One unanchored vertex at a surface singularity.
    Vertex {
        /// Referenced pole vertex.
        vertex: VertexId,
        /// Ordered parameter-space images associated with the pole.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pcurves: Vec<PcurveUse>,
    },
    /// An ordered coedge ring and its anchored pole occurrences.
    Ring(LoopRing),
}

/// Structural error in a loop coedge ring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopRingError(String);

impl std::fmt::Display for LoopRingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for LoopRingError {}

/// A checked, ordered coedge ring and its anchored pole occurrences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "LoopRingWire")]
pub struct LoopRing {
    coedges: Vec<CoedgeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    vertex_uses: Vec<AnchoredVertexUse>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct LoopRingWire {
    /// Coedges in source traversal order.
    coedges: Vec<CoedgeId>,
    /// Pole occurrences in source traversal order.
    #[serde(default)]
    vertex_uses: Vec<AnchoredVertexUse>,
}

impl TryFrom<LoopRingWire> for LoopRing {
    type Error = LoopRingError;

    fn try_from(wire: LoopRingWire) -> Result<Self, Self::Error> {
        Self::new(wire.coedges, wire.vertex_uses)
    }
}

impl LoopRing {
    /// Construct a ring containing one coedge.
    pub fn single(coedge: CoedgeId) -> Self {
        Self {
            coedges: vec![coedge],
            vertex_uses: Vec::new(),
        }
    }

    /// Construct a generated ring from a nonempty vertex cycle.
    ///
    /// Each coedge identity is composed from `namespace`, `key_prefix`, and
    /// its source ordinal. The corresponding vertex use is anchored to that
    /// generated coedge, so the returned ring carries the member and anchor
    /// relationship without reopening checked construction.
    pub fn from_vertices(
        namespace: &IdentityNamespace,
        key_prefix: &IdentityKey,
        vertices: NonEmptyMembers<VertexId>,
    ) -> Self {
        let member_count = vertices.len();
        let mut coedges = Vec::with_capacity(member_count);
        let mut vertex_uses = Vec::with_capacity(member_count);
        for (ordinal, vertex) in vertices.into_iter().enumerate() {
            let coedge = CoedgeId::compose(namespace, key_prefix.clone().colon(ordinal));
            let vertex_use = AnchoredVertexUse {
                vertex,
                after: coedge.clone(),
                pcurves: Vec::new(),
            };
            coedges.push(coedge);
            vertex_uses.push(vertex_use);
        }
        Self {
            coedges,
            vertex_uses,
        }
    }

    /// Append a distinct coedge in traversal order.
    pub fn try_push(&mut self, coedge: CoedgeId) -> Result<(), LoopRingError> {
        if self.coedges.contains(&coedge) {
            return Err(LoopRingError("loop ring coedges must be distinct".into()));
        }
        self.coedges.push(coedge);
        Ok(())
    }

    /// Build a nonempty ring of distinct coedges whose anchors belong to that ring.
    pub fn new(
        coedges: Vec<CoedgeId>,
        vertex_uses: Vec<AnchoredVertexUse>,
    ) -> Result<Self, LoopRingError> {
        if coedges.is_empty() {
            return Err(LoopRingError("loop ring must contain a coedge".into()));
        }
        let members: HashSet<_> = coedges.iter().collect();
        if members.len() != coedges.len() {
            return Err(LoopRingError("loop ring coedges must be distinct".into()));
        }
        if vertex_uses
            .iter()
            .any(|vertex_use| !members.contains(&vertex_use.after))
        {
            return Err(LoopRingError(
                "loop ring vertex-use after must name a coedge in the ring".into(),
            ));
        }
        Ok(Self {
            coedges,
            vertex_uses,
        })
    }

    /// Coedges in source traversal order.
    #[must_use]
    pub fn coedges(&self) -> &[CoedgeId] {
        &self.coedges
    }

    /// Pole occurrences in source traversal order.
    #[must_use]
    pub fn vertex_uses(&self) -> &[AnchoredVertexUse] {
        &self.vertex_uses
    }
}

impl Loop {
    /// Replace the complete coedge ring after checking its local structure.
    pub fn replace_ring(
        &mut self,
        coedges: Vec<CoedgeId>,
        vertex_uses: Vec<AnchoredVertexUse>,
    ) -> Result<(), LoopRingError> {
        if !matches!(&self.boundary, LoopBoundary::Ring(_)) {
            return Err(LoopRingError(
                "cannot replace the ring of a vertex-only loop".into(),
            ));
        }
        self.boundary = LoopBoundary::Ring(LoopRing::new(coedges, vertex_uses)?);
        Ok(())
    }

    /// Returns the ordered coedges when this is a ring boundary.
    #[must_use]
    pub fn coedges(&self) -> &[CoedgeId] {
        match &self.boundary {
            LoopBoundary::Vertex { .. } => &[],
            LoopBoundary::Ring(ring) => ring.coedges(),
        }
    }

    /// Returns the anchored vertex uses when this is a ring boundary.
    #[must_use]
    pub fn anchored_vertex_uses(&self) -> &[AnchoredVertexUse] {
        match &self.boundary {
            LoopBoundary::Vertex { .. } => &[],
            LoopBoundary::Ring(ring) => ring.vertex_uses(),
        }
    }

    /// Returns the singular vertex and its parameter-space images.
    #[must_use]
    pub fn singular_vertex(&self) -> Option<(&VertexId, &[PcurveUse])> {
        match &self.boundary {
            LoopBoundary::Vertex { vertex, pcurves } => Some((vertex, pcurves)),
            LoopBoundary::Ring(_) => None,
        }
    }

    /// Iterates over every vertex referenced directly by this boundary.
    pub fn vertices(&self) -> impl Iterator<Item = &VertexId> {
        let (singular, anchored) = match &self.boundary {
            LoopBoundary::Vertex { vertex, .. } => (Some(vertex), &[][..]),
            LoopBoundary::Ring(ring) => (None, ring.vertex_uses()),
        };
        singular
            .into_iter()
            .chain(anchored.iter().map(|use_| &use_.vertex))
    }

    /// Next coedge in this ring after `id`.
    #[must_use]
    pub fn next_coedge(&self, id: &CoedgeId) -> Option<&CoedgeId> {
        let ring = self.coedges();
        let index = ring.iter().position(|coedge| coedge == id)?;
        ring.get((index + 1) % ring.len())
    }

    /// Previous coedge in this ring before `id`.
    #[must_use]
    pub fn previous_coedge(&self, id: &CoedgeId) -> Option<&CoedgeId> {
        let ring = self.coedges();
        let index = ring.iter().position(|coedge| coedge == id)?;
        ring.get((index + ring.len() - 1) % ring.len())
    }

    /// Ring neighbors of `coedge` when this loop owns it.
    #[must_use]
    pub fn ring_neighbors_of(&self, coedge: &Coedge) -> Option<(CoedgeId, CoedgeId)> {
        if self.id != coedge.owner_loop {
            return None;
        }
        Some((
            self.next_coedge(&coedge.id)?.clone(),
            self.previous_coedge(&coedge.id)?.clone(),
        ))
    }

    /// Iterates over every parameter-space image attached to a boundary vertex.
    pub fn vertex_pcurves(&self) -> impl Iterator<Item = &PcurveUse> {
        let (singular, anchored) = match &self.boundary {
            LoopBoundary::Vertex { pcurves, .. } => (Some(pcurves.as_slice()), &[][..]),
            LoopBoundary::Ring(ring) => (None, ring.vertex_uses()),
        };
        singular
            .into_iter()
            .flatten()
            .chain(anchored.iter().flat_map(|use_| use_.pcurves.iter()))
    }

    /// Iterates over boundary vertices with their optional ring anchor and pcurves.
    pub fn vertex_occurrences(
        &self,
    ) -> impl Iterator<Item = (&VertexId, Option<&CoedgeId>, &[PcurveUse])> {
        let (singular, anchored) = match &self.boundary {
            LoopBoundary::Vertex { vertex, pcurves } => {
                (Some((vertex, pcurves.as_slice())), &[][..])
            }
            LoopBoundary::Ring(ring) => (None, ring.vertex_uses()),
        };
        singular
            .into_iter()
            .map(|(vertex, pcurves)| (vertex, None, pcurves))
            .chain(
                anchored
                    .iter()
                    .map(|use_| (&use_.vertex, Some(&use_.after), use_.pcurves.as_slice())),
            )
    }
}

/// One pole-vertex occurrence anchored after a coedge in a ring traversal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AnchoredVertexUse {
    /// Referenced pole vertex.
    pub vertex: VertexId,
    /// Preceding coedge in the cyclic traversal.
    pub after: CoedgeId,
    /// Ordered parameter-space images associated with this pole occurrence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pcurves: Vec<PcurveUse>,
}

/// One use of an edge by a loop.
///
/// Coedges form a loop ring through the owning [`Loop`] coedge order, and a
/// radial ring around their shared edge through `radial_next`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Coedge {
    /// Arena id.
    pub id: CoedgeId,
    /// Owning loop.
    pub owner_loop: LoopId,
    /// Underlying edge.
    pub edge: EdgeId,
    /// Next coedge around the edge; self-reference denotes a laminar boundary.
    pub radial_next: CoedgeId,
    /// Direction relative to the edge curve.
    pub sense: Sense,
    /// Ordered parameter-space images of this coedge on the face surface.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pcurves: Vec<PcurveUse>,
    /// Optional coedge-local 3D carrier used instead of the shared edge curve.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_use_curve"
    )]
    pub use_curve: Option<CoedgeUseCurve>,
}

/// Next and previous coedge ids from the owning loop ring.
#[must_use]
pub fn coedge_ring_neighbors(loops: &[Loop], coedge: &Coedge) -> Option<(CoedgeId, CoedgeId)> {
    loops
        .iter()
        .find_map(|loop_| loop_.ring_neighbors_of(coedge))
}

/// A coedge-local curve and its loop-traversal interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CoedgeUseCurve {
    /// Local 3D curve carrier.
    pub curve: CurveId,
    /// Interval on the carrier in loop-traversal order.
    pub parameter_range: ParameterInterval,
}

/// A finite ordered parameter interval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "[f64; 2]", into = "[f64; 2]")]
pub struct ParameterInterval([f64; 2]);

impl ParameterInterval {
    /// Admit finite endpoints in increasing or equal order.
    pub fn new(endpoints: [f64; 2]) -> Result<Self, &'static str> {
        if endpoints.iter().all(|value| value.is_finite()) && endpoints[0] <= endpoints[1] {
            Ok(Self(endpoints))
        } else {
            Err("parameter_range must be finite and ordered")
        }
    }
    /// Return the interval endpoints.
    pub const fn endpoints(self) -> [f64; 2] {
        self.0
    }
    pub(crate) const fn as_raw(&self) -> &[f64; 2] {
        &self.0
    }
}

impl TryFrom<[f64; 2]> for ParameterInterval {
    type Error = &'static str;
    fn try_from(value: [f64; 2]) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ParameterInterval> for [f64; 2] {
    fn from(value: ParameterInterval) -> Self {
        value.0
    }
}

impl From<IncreasingParameterInterval> for ParameterInterval {
    /// Carry a strictly increasing interval. Its endpoints are finite and in
    /// increasing order, so nothing is checked.
    fn from(value: IncreasingParameterInterval) -> Self {
        Self(value.endpoints())
    }
}

/// A finite parameter interval whose first endpoint is strictly below its
/// second.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "[f64; 2]", into = "[f64; 2]")]
pub struct IncreasingParameterInterval([f64; 2]);

impl IncreasingParameterInterval {
    /// Admit finite endpoints in strictly increasing order.
    pub fn new(endpoints: [f64; 2]) -> Option<Self> {
        (endpoints.iter().all(|value| value.is_finite()) && endpoints[0] < endpoints[1])
            .then_some(Self(endpoints))
    }
    /// Return the interval endpoints.
    pub const fn endpoints(self) -> [f64; 2] {
        self.0
    }
    /// Return the lower endpoint.
    pub const fn lower(self) -> f64 {
        self.0[0]
    }
    /// Return the upper endpoint.
    pub const fn upper(self) -> f64 {
        self.0[1]
    }
}

impl TryFrom<[f64; 2]> for IncreasingParameterInterval {
    type Error = &'static str;
    fn try_from(value: [f64; 2]) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("parameter interval must be finite and strictly increasing")
    }
}

impl From<IncreasingParameterInterval> for [f64; 2] {
    fn from(value: IncreasingParameterInterval) -> Self {
        value.0
    }
}

/// An edge carrier and its admitted parameter endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "EdgeCarrierWire"))]
#[serde(try_from = "EdgeCarrierWire", into = "EdgeCarrierWire")]
pub enum EdgeCarrier {
    /// Neither a carrier curve nor parameter endpoints.
    Free,
    /// Finite parameter endpoints with no carrier curve to order them against.
    Endpoints([crate::scalar::FiniteReal; 2]),
    /// A carrier curve with no parameter endpoints.
    Curve(CurveId),
    /// A carrier curve and the ordered interval admitted on it.
    Bounded(CurveId, ParameterInterval),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct EdgeCarrierWire {
    /// Curve carrying the edge, when the source states one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_curve"
    )]
    curve: Option<CurveId>,
    /// Parameter endpoints on the carrier, when the source states them.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_param_range"
    )]
    param_range: Option<[f64; 2]>,
}

impl EdgeCarrier {
    /// Construct a carrier without parameter endpoints.
    pub fn unbounded(curve: Option<CurveId>) -> Self {
        match curve {
            Some(curve) => Self::Curve(curve),
            None => Self::Free,
        }
    }

    /// Admit a carrier and finite endpoints, ordered when the carrier is present.
    pub fn new(
        curve: Option<CurveId>,
        param_range: Option<[f64; 2]>,
    ) -> Result<Self, &'static str> {
        match (curve, param_range) {
            (None, None) => Ok(Self::Free),
            (Some(curve), None) => Ok(Self::Curve(curve)),
            (Some(curve), Some(range)) => ParameterInterval::new(range)
                .map(|interval| Self::Bounded(curve, interval))
                .map_err(|_| "edge param_range must be finite and ordered"),
            (None, Some([start, end])) => {
                let start = crate::scalar::FiniteReal::new(start);
                let end = crate::scalar::FiniteReal::new(end);
                match (start, end) {
                    (Some(start), Some(end)) => Ok(Self::Endpoints([start, end])),
                    _ => Err("edge param_range endpoints must be finite"),
                }
            }
        }
    }

    /// Return the carrier curve, when the carrier states one.
    pub const fn curve(&self) -> Option<&CurveId> {
        match self {
            Self::Free | Self::Endpoints(_) => None,
            Self::Curve(curve) | Self::Bounded(curve, _) => Some(curve),
        }
    }

    /// Return the parameter endpoints, when the carrier states them.
    pub fn param_range(&self) -> Option<[f64; 2]> {
        match self {
            Self::Free | Self::Curve(_) => None,
            Self::Endpoints([start, end]) => Some([start.get(), end.get()]),
            Self::Bounded(_, interval) => Some(interval.endpoints()),
        }
    }

    /// Map the carrier identity without changing its presence or parameter range.
    fn map_curve(&mut self, map: impl FnOnce(&CurveId) -> CurveId) {
        match self {
            Self::Free | Self::Endpoints(_) => {}
            Self::Curve(curve) | Self::Bounded(curve, _) => *curve = map(curve),
        }
    }
}

impl TryFrom<EdgeCarrierWire> for EdgeCarrier {
    type Error = &'static str;
    fn try_from(wire: EdgeCarrierWire) -> Result<Self, Self::Error> {
        Self::new(wire.curve, wire.param_range)
    }
}

impl From<EdgeCarrier> for EdgeCarrierWire {
    fn from(value: EdgeCarrier) -> Self {
        Self {
            curve: value.curve().cloned(),
            param_range: value.param_range(),
        }
    }
}

/// An edge between two vertices with an optional curve carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Edge {
    /// Arena id.
    pub id: EdgeId,
    /// Carrier and its admitted parameter range.
    pub carrier: EdgeCarrier,
    /// Start vertex.
    pub start: VertexId,
    /// End vertex.
    pub end: VertexId,
    /// Optional geometric tolerance in the document's length unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_tolerance")]
    pub tolerance: Option<crate::scalar::PositiveReal>,
}

impl Edge {
    /// Map the carrier identity without changing its presence or parameter range.
    pub fn map_curve(&mut self, map: impl FnOnce(&CurveId) -> CurveId) {
        self.carrier.map_curve(map);
    }

    /// Replace the curve while retaining the range if it remains valid.
    pub fn set_curve(&mut self, curve: Option<CurveId>) -> Result<(), &'static str> {
        self.carrier = EdgeCarrier::new(curve, self.param_range())?;
        Ok(())
    }
    /// Replace the parameter endpoints with an admitted interval. The
    /// interval is finite and ordered, so it is valid with or without a
    /// carrier curve.
    pub fn set_param_range(&mut self, range: Option<ParameterInterval>) {
        self.carrier = match (self.curve().cloned(), range) {
            (curve, None) => EdgeCarrier::unbounded(curve),
            (Some(curve), Some(range)) => EdgeCarrier::Bounded(curve, range),
            (None, Some(range)) => EdgeCarrier::Endpoints(range.finite_endpoints()),
        };
    }
    /// Return the underlying curve carrier.
    pub const fn curve(&self) -> Option<&CurveId> {
        self.carrier.curve()
    }
    /// Return the parameter endpoints.
    pub fn param_range(&self) -> Option<[f64; 2]> {
        self.carrier.param_range()
    }
}

/// A vertex: a topological point referencing a position carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Vertex {
    /// Arena id.
    pub id: VertexId,
    /// Position carrier.
    pub point: PointId,
    /// Optional geometric tolerance in the document's length unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_tolerance")]
    pub tolerance: Option<crate::scalar::PositiveReal>,
}

/// A position carrier for a vertex.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "PointWire")]
pub struct Point {
    /// Arena id.
    pub id: PointId,
    /// Coordinates in the document's length unit.
    position: FinitePoint3,
    /// Source object carrying this free point, when known.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source_object"
    )]
    pub source_object: Option<crate::provenance::SourceObjectAssociation>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PointWire {
    /// Arena id.
    id: PointId,
    /// Coordinates in the document's length unit.
    position: Point3,
    /// Source object carrying this free point, when known.
    #[serde(default, deserialize_with = "deserialize_source_object")]
    source_object: Option<crate::provenance::SourceObjectAssociation>,
}

impl Point {
    /// The refusal of a position with a non-finite coordinate.
    pub const NON_FINITE_POSITION: &'static str = "point position coordinates must be finite";

    /// Construct a position carrier at admitted coordinates.
    #[must_use]
    pub fn new(
        id: PointId,
        position: FinitePoint3,
        source_object: Option<crate::provenance::SourceObjectAssociation>,
    ) -> Self {
        Self {
            id,
            position,
            source_object,
        }
    }

    /// Return the admitted coordinates in the document's length unit.
    #[must_use]
    pub const fn position(&self) -> FinitePoint3 {
        self.position
    }

    /// Replace the coordinates with admitted ones.
    pub fn set_position(&mut self, position: FinitePoint3) {
        self.position = position;
    }
}

impl TryFrom<PointWire> for Point {
    type Error = &'static str;
    fn try_from(wire: PointWire) -> Result<Self, Self::Error> {
        let position = FinitePoint3::new(wire.position).ok_or(Self::NON_FINITE_POSITION)?;
        Ok(Self::new(wire.id, position, wire.source_object))
    }
}

cadmpeg_core::named_optional_field!(
    deserialize_tolerance,
    crate::scalar::PositiveReal,
    "tolerance"
);

#[cfg(test)]
mod tests {
    #[test]
    fn increasing_intervals_admit_only_finite_strictly_increasing_endpoints() {
        use super::IncreasingParameterInterval;

        let interval = IncreasingParameterInterval::new([-1.0, 2.5]).expect("increasing");
        assert_eq!(interval.endpoints(), [-1.0, 2.5]);
        assert_eq!([interval.lower(), interval.upper()], [-1.0, 2.5]);
        let wire = serde_json::to_value(interval).unwrap();
        assert_eq!(wire, serde_json::json!([-1.0, 2.5]));
        assert_eq!(
            serde_json::from_value::<IncreasingParameterInterval>(wire).unwrap(),
            interval
        );
        for refused in [
            [1.0, 1.0],
            [2.0, 1.0],
            [f64::NAN, 1.0],
            [0.0, f64::INFINITY],
            [f64::NEG_INFINITY, 0.0],
        ] {
            assert!(IncreasingParameterInterval::new(refused).is_none());
            assert_eq!(
                IncreasingParameterInterval::try_from(refused),
                Err("parameter interval must be finite and strictly increasing")
            );
        }
        assert!(
            serde_json::from_value::<IncreasingParameterInterval>(serde_json::json!([1.0, 1.0]))
                .is_err()
        );
    }

    #[test]
    fn face_loops_are_one_tagged_object_that_round_trips() {
        use super::{FaceLoops, LoopBoundaryRole};
        use crate::ids::LoopId;

        // The old per-member `role` key has no spelling, so a document that
        // pairs a loop with a role is refused as an unknown shape.
        assert!(serde_json::from_value::<FaceLoops>(serde_json::json!([
            {"id": "t:b:loop#1", "role": "outer"},
            {"id": "t:b:loop#2", "role": "unspecified"}
        ]))
        .is_err());

        for wire in [
            serde_json::json!({
                "classification": "unspecified",
                "loops": ["t:b:loop#1", "t:b:loop#2"]
            }),
            serde_json::json!({
                "classification": "classified",
                "outer": "t:b:loop#1",
                "inner": ["t:b:loop#2"]
            }),
        ] {
            let loops: FaceLoops = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(&loops).unwrap(), wire);
        }

        // A classification states an outer loop. The outer-less spelling is
        // refused for the missing key, so it is not a second spelling of an
        // unclassified face.
        let error = serde_json::from_value::<FaceLoops>(
            serde_json::json!({"classification": "classified", "inner": ["t:b:loop#2"]}),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("outer"), "{error}");

        let outer = LoopId::mint("t:b:loop#1").unwrap();
        let inner = LoopId::mint("t:b:loop#2").unwrap();
        let classified = FaceLoops::classified(outer.clone(), vec![inner.clone()]);
        assert_eq!(classified.role(&outer), LoopBoundaryRole::Outer);
        assert_eq!(classified.role(&inner), LoopBoundaryRole::Inner);
        let unspecified = FaceLoops::unspecified(vec![outer.clone(), inner.clone()]);
        assert_eq!(unspecified.role(&outer), LoopBoundaryRole::Unspecified);
    }

    #[test]
    fn parameter_interval_admission_preserves_direction_contracts() {
        use super::{EdgeCarrier, ParameterInterval};
        use crate::geometry::DirectedParameterRange;
        assert!(ParameterInterval::new([2.0, 1.0]).is_err());
        assert!(ParameterInterval::new([f64::INFINITY, 1.0]).is_err());
        assert_eq!(
            ParameterInterval::new([1.0, 1.0]).unwrap().endpoints(),
            [1.0, 1.0]
        );
        assert_eq!(
            DirectedParameterRange::new([2.0, 1.0]).unwrap().endpoints(),
            [2.0, 1.0]
        );
        assert!(DirectedParameterRange::new([1.0, 1.0]).is_err());
        assert!(serde_json::from_str::<ParameterInterval>("[2,1]").is_err());
        assert!(serde_json::from_str::<DirectedParameterRange>("[1,1]").is_err());
        let wire = serde_json::json!({"param_range": [2.0, 1.0]});
        let carrier: EdgeCarrier = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(carrier).unwrap(), wire);
        let attributed = serde_json::json!({"curve":"test:model:curve#1", "param_range":[2.0,1.0]});
        assert!(serde_json::from_value::<EdgeCarrier>(attributed).is_err());
    }

    #[test]
    fn an_admitted_interval_bounds_a_carrier_or_sets_free_endpoints() {
        use super::{EdgeCarrier, IncreasingParameterInterval, ParameterInterval};
        use crate::scalar::FiniteReal;

        let increasing = IncreasingParameterInterval::new([-1.5, 2.0]).expect("increasing");
        let interval = ParameterInterval::from(increasing);
        assert_eq!(interval.endpoints(), [-1.5, 2.0]);
        assert_eq!(
            interval.finite_endpoints().map(FiniteReal::get),
            interval.endpoints()
        );

        let mut edge = crate::examples::unit_cube()
            .expect("valid unit cube fixture")
            .model
            .edges
            .remove(0);
        let curve = edge.curve().cloned().expect("the cube edges have carriers");
        edge.set_param_range(Some(interval));
        assert_eq!(edge.carrier, EdgeCarrier::Bounded(curve.clone(), interval));
        assert_eq!(
            Ok(edge.carrier.clone()),
            EdgeCarrier::new(Some(curve), Some([-1.5, 2.0]))
        );
        edge.set_param_range(None);
        assert_eq!(edge.param_range(), None);
        assert!(edge.curve().is_some());

        edge.set_curve(None)
            .expect("a carrier without endpoints can drop its curve");
        let equal = ParameterInterval::new([3.0, 3.0]).expect("equal endpoints are ordered");
        edge.set_param_range(Some(equal));
        assert_eq!(
            edge.carrier,
            EdgeCarrier::Endpoints(equal.finite_endpoints())
        );
        assert_eq!(
            Ok(edge.carrier.clone()),
            EdgeCarrier::new(None, Some([3.0, 3.0]))
        );
    }

    #[test]
    fn edge_parameter_edits_commit_only_valid_pairs() {
        let mut edge = crate::examples::unit_cube()
            .expect("valid unit cube fixture")
            .model
            .edges
            .remove(0);
        let original = edge.clone();
        assert!(super::EdgeCarrier::new(edge.curve().cloned(), Some([2.0, 1.0])).is_err());
        assert_eq!(edge, original);
        edge.set_curve(None).unwrap();
        edge.carrier = super::EdgeCarrier::new(edge.curve().cloned(), Some([2.0, 1.0])).unwrap();
        let carrierless = edge.clone();
        assert!(edge.set_curve(original.curve().cloned()).is_err());
        assert_eq!(edge, carrierless);
    }

    #[test]
    fn shell_admission_requires_one_topology_member_and_preserves_wire_fields() {
        let id = super::ShellId::mint("test:model:shell#1").unwrap();
        let region = super::RegionId::mint("test:model:region#1").unwrap();
        let face = super::FaceId::mint("test:model:face#1").unwrap();
        let edge = super::EdgeId::mint("test:model:edge#1").unwrap();
        let vertex = super::VertexId::mint("test:model:vertex#1").unwrap();
        for mask in 0..8 {
            let faces = if mask & 1 != 0 {
                vec![face.clone(), face.clone()]
            } else {
                Vec::new()
            };
            let edges = if mask & 2 != 0 {
                vec![edge.clone()]
            } else {
                Vec::new()
            };
            let vertices = if mask & 4 != 0 {
                vec![vertex.clone()]
            } else {
                Vec::new()
            };
            let admitted = super::Shell::new(
                id.clone(),
                region.clone(),
                faces.clone(),
                edges.clone(),
                vertices.clone(),
            );
            assert_eq!(admitted.is_ok(), mask != 0);
            let mut members = Vec::new();
            for face in &faces {
                members.push(serde_json::json!({"kind": "face", "id": face}));
            }
            for edge in &edges {
                members.push(serde_json::json!({"kind": "wire_edge", "id": edge}));
            }
            for vertex in &vertices {
                members.push(serde_json::json!({"kind": "free_vertex", "id": vertex}));
            }
            let wire = serde_json::json!({"id":id,"region":region,"members":members});
            let decoded = serde_json::from_value::<super::Shell>(wire.clone());
            assert_eq!(decoded.is_ok(), mask != 0);
            if let Ok(shell) = decoded {
                assert_eq!(serde_json::to_value(shell).unwrap(), wire);
            }
        }
    }

    #[test]
    fn shell_topology_edits_admit_the_whole_replacement_and_keep_old_values_on_failure() {
        let mut shell = super::Shell::with_face(
            super::ShellId::mint("test:model:shell#1").unwrap(),
            super::RegionId::mint("test:model:region#1").unwrap(),
            super::FaceId::mint("test:model:face#1").unwrap(),
        );
        let original = shell.clone();
        assert!(shell
            .edit_topology(|faces, edges, vertices| {
                faces.clear();
                edges.clear();
                vertices.clear();
            })
            .is_err());
        assert_eq!(shell, original);
        let vertex = super::VertexId::mint("test:model:vertex#1").unwrap();
        shell
            .edit_topology(|faces, _, vertices| {
                faces.clear();
                vertices.push(vertex.clone());
            })
            .unwrap();
        assert!(shell.faces().is_empty());
        assert_eq!(shell.free_vertices(), &[vertex]);
    }

    #[test]
    fn color_rejects_invalid_components_on_construction_and_wire() {
        for invalid in [-1.0, 1.1, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for index in 0..4 {
                let mut values = [0.5; 4];
                values[index] = invalid;
                assert!(super::Color::new(values[0], values[1], values[2], values[3]).is_none());
            }
        }
        for field in ["r", "g", "b", "a"] {
            let mut value = serde_json::json!({"r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0});
            value[field] = serde_json::json!(-1.0);
            let error =
                serde_json::from_value::<super::Color>(value).expect_err("invalid component");
            assert!(error.to_string().contains(field));
        }
        let color = super::Color::new(0.0, 0.25, 0.5, 1.0).expect("valid color");
        assert_eq!(
            serde_json::to_value(color).expect("serialize"),
            serde_json::json!({"r": 0.0, "g": 0.25, "b": 0.5, "a": 1.0})
        );
        assert!(color.with_alpha(-1.0).is_none());
        assert_eq!(color.a(), 1.0);
    }

    use super::{
        coedge_ring_neighbors, AnchoredVertexUse, Coedge, CoedgeUseCurve, Face, FaceLoops, Loop,
        LoopBoundary, LoopBoundaryRole, LoopId, LoopRing,
    };
    use crate::features::NonEmptyMembers;

    #[test]
    fn incremental_loop_ring_keeps_order_and_rejects_duplicates() {
        let first = super::CoedgeId::mint("test:model:coedge#0").unwrap();
        let second = super::CoedgeId::mint("test:model:coedge#1").unwrap();
        let mut ring = LoopRing::single(first.clone());
        ring.try_push(second.clone()).unwrap();
        assert_eq!(ring.coedges(), &[first.clone(), second]);
        let before = ring.clone();
        assert!(ring.try_push(first).is_err());
        assert_eq!(ring, before);
    }

    #[test]
    fn generated_loop_ring_anchors_each_vertex_to_its_generated_coedge() {
        let coedge_namespace = crate::identity_namespace!("test", "model", "coedge");
        let vertex_namespace = crate::identity_namespace!("test", "model", "vertex");
        let mut vertices =
            NonEmptyMembers::one(super::VertexId::compose(&vertex_namespace, 3_usize));
        vertices.push(super::VertexId::compose(&vertex_namespace, 8_usize));

        let ring = LoopRing::from_vertices(
            &coedge_namespace,
            &crate::ids::IdentityKey::from(4_usize).colon(2_usize),
            vertices,
        );
        assert_eq!(
            ring.coedges()
                .iter()
                .map(super::CoedgeId::as_str)
                .collect::<Vec<_>>(),
            ["test:model:coedge#4:2:0", "test:model:coedge#4:2:1"]
        );
        assert!(ring
            .vertex_uses()
            .iter()
            .zip(ring.coedges())
            .all(|(vertex_use, coedge)| vertex_use.after == *coedge));
    }

    #[test]
    fn loop_ring_rejects_empty_or_duplicate_coedges_and_foreign_anchors() {
        assert!(LoopRing::new(Vec::new(), Vec::new()).is_err());

        let coedge: super::CoedgeId = "test:model:coedge#0".try_into().unwrap();
        let foreign: super::CoedgeId = "test:model:coedge#foreign".try_into().unwrap();
        let vertex: super::VertexId = "test:model:vertex#0".try_into().unwrap();
        assert!(LoopRing::new(vec![coedge.clone(), coedge.clone()], Vec::new()).is_err());
        assert!(LoopRing::new(
            vec![coedge],
            vec![AnchoredVertexUse {
                vertex,
                after: foreign,
                pcurves: Vec::new(),
            }],
        )
        .is_err());
    }

    #[test]
    fn replacing_a_ring_rejects_invalid_contents_without_changing_the_loop() {
        let coedge: super::CoedgeId = "test:model:coedge#0".try_into().unwrap();
        let mut loop_ = Loop {
            id: "test:model:loop#0".try_into().unwrap(),
            face: "test:model:face#0".try_into().unwrap(),
            boundary: LoopBoundary::Ring(LoopRing::new(vec![coedge.clone()], Vec::new()).unwrap()),
        };
        let before = loop_.clone();
        assert!(loop_.replace_ring(Vec::new(), Vec::new()).is_err());
        assert_eq!(loop_, before);
        assert!(loop_
            .replace_ring(
                vec![coedge],
                vec![AnchoredVertexUse {
                    vertex: "test:model:vertex#0".try_into().unwrap(),
                    after: "test:model:coedge#foreign".try_into().unwrap(),
                    pcurves: Vec::new(),
                }],
            )
            .is_err());
        assert_eq!(loop_, before);
    }

    fn coedge_json() -> serde_json::Value {
        serde_json::json!({
            "id": "test:model:coedge#0",
            "owner_loop": "test:model:loop#0",
            "edge": "test:model:edge#0",
            "radial_next": "test:model:coedge#0",
            "sense": "forward",
            "use_curve": {
                "curve": "test:model:curve#0",
                "parameter_range": [0.25, 0.75]
            }
        })
    }

    #[test]
    fn the_coedge_use_curve_is_one_nested_key() {
        let coedge: Coedge = serde_json::from_value(coedge_json()).unwrap();
        assert_eq!(
            coedge.use_curve,
            Some(CoedgeUseCurve {
                curve: "test:model:curve#0".try_into().expect("valid identity"),
                parameter_range: crate::topology::ParameterInterval::new([0.25, 0.75]).unwrap(),
            })
        );
        let loop_ = Loop {
            id: coedge.owner_loop.clone(),
            face: "test:model:face#0".try_into().expect("valid identity"),
            boundary: LoopBoundary::Ring(
                super::LoopRing::new(vec![coedge.id.clone()], Vec::new()).expect("valid loop ring"),
            ),
        };
        assert_eq!(
            coedge_ring_neighbors(std::slice::from_ref(&loop_), &coedge),
            Some((coedge.id.clone(), coedge.id.clone()))
        );
        let encoded = serde_json::to_value(&coedge).unwrap();
        assert_eq!(
            encoded["use_curve"],
            serde_json::json!({
                "curve": "test:model:curve#0",
                "parameter_range": [0.25, 0.75]
            })
        );
        assert_eq!(
            serde_json::from_value::<Coedge>(encoded).unwrap().use_curve,
            coedge.use_curve
        );
    }

    #[test]
    fn a_coedge_states_no_ring_neighbor_and_its_loop_order_survives_a_round_trip() {
        let model = crate::examples::unit_cube()
            .expect("valid unit cube fixture")
            .model;
        let coedge = &model.coedges[0];
        let wire = serde_json::to_value(coedge).unwrap();
        assert!(wire.get("next").is_none(), "{wire}");
        assert!(wire.get("previous").is_none(), "{wire}");

        let mut with_next = wire.clone();
        with_next["next"] = serde_json::json!(coedge.id.as_str());
        let error = serde_json::from_value::<Coedge>(with_next)
            .expect_err("the ring order is the loop's, not the coedge's")
            .to_string();
        assert!(error.contains("next"), "{error}");
        assert_eq!(&serde_json::from_value::<Coedge>(wire).unwrap(), coedge);

        let before = coedge_ring_neighbors(&model.loops, coedge).expect("a ring neighbor pair");
        let round_tripped: crate::document::Model =
            serde_json::from_value(serde_json::to_value(&model).unwrap()).unwrap();
        let after = coedge_ring_neighbors(&round_tripped.loops, &round_tripped.coedges[0])
            .expect("a ring neighbor pair");
        assert_eq!(before, after);
    }

    #[test]
    fn a_half_stated_coedge_use_curve_has_no_encoding() {
        let mut without_range = coedge_json();
        without_range["use_curve"]
            .as_object_mut()
            .expect("a use-curve object")
            .remove("parameter_range");
        assert!(serde_json::from_value::<Coedge>(without_range).is_err());

        let mut without_curve = coedge_json();
        without_curve["use_curve"]
            .as_object_mut()
            .expect("a use-curve object")
            .remove("curve");
        assert!(serde_json::from_value::<Coedge>(without_curve).is_err());

        let mut bogus = coedge_json();
        bogus["use_curve"]["zz_bogus"] = serde_json::json!(1);
        let error = serde_json::from_value::<Coedge>(bogus)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "{error}");

        let mut absent = coedge_json();
        absent
            .as_object_mut()
            .expect("a coedge object")
            .remove("use_curve");
        assert_eq!(
            serde_json::from_value::<Coedge>(absent).unwrap().use_curve,
            None
        );
    }

    #[test]
    fn the_vertex_loop_boundary_is_one_nested_tagged_object() {
        let json = serde_json::json!({
            "id": "test:model:loop#0",
            "face": "test:model:face#0",
            "boundary": {
                "kind": "vertex",
                "vertex": "test:model:vertex#0"
            }
        });
        let loop_: Loop = serde_json::from_value(json.clone()).unwrap();
        assert!(matches!(
            loop_.boundary,
            LoopBoundary::Vertex { ref vertex, ref pcurves }
                if vertex.as_str() == "test:model:vertex#0" && pcurves.is_empty()
        ));
        assert_eq!(serde_json::to_value(loop_).unwrap(), json);
    }

    #[test]
    fn the_ring_loop_boundary_is_one_nested_tagged_object() {
        let json = serde_json::json!({
            "id": "test:model:loop#0",
            "face": "test:model:face#0",
            "boundary": {
                "kind": "ring",
                "coedges": ["test:model:coedge#0"],
                "vertex_uses": [{
                    "vertex": "test:model:vertex#0",
                    "after": "test:model:coedge#0"
                }]
            }
        });
        let loop_: Loop = serde_json::from_value(json.clone()).unwrap();
        assert!(matches!(loop_.boundary, LoopBoundary::Ring(_)));
        assert_eq!(serde_json::to_value(loop_).unwrap(), json);
    }

    #[test]
    fn a_loop_boundary_carries_no_key_of_the_other_form() {
        let anchored_vertex = serde_json::json!({
            "id": "test:model:loop#0",
            "face": "test:model:face#0",
            "boundary": {
                "kind": "vertex",
                "vertex": "test:model:vertex#0",
                "after": "test:model:coedge#0"
            }
        });
        let error = serde_json::from_value::<Loop>(anchored_vertex)
            .unwrap_err()
            .to_string();
        assert!(error.contains("after"), "{error}");

        let ring_without_anchor = serde_json::json!({
            "id": "test:model:loop#0",
            "face": "test:model:face#0",
            "boundary": {
                "kind": "ring",
                "coedges": ["test:model:coedge#0"],
                "vertex_uses": [{ "vertex": "test:model:vertex#0" }]
            }
        });
        assert!(serde_json::from_value::<Loop>(ring_without_anchor).is_err());

        let bogus = serde_json::json!({
            "id": "test:model:loop#0",
            "face": "test:model:face#0",
            "boundary": {
                "kind": "ring",
                "coedges": ["test:model:coedge#0"],
                "zz_bogus": 1
            }
        });
        let error = serde_json::from_value::<Loop>(bogus)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "{error}");
    }

    #[test]
    fn a_classified_face_states_its_outer_loop_once() {
        let mut model = crate::examples::unit_cube()
            .expect("valid unit cube fixture")
            .model;
        let face = &mut model.faces[0];
        let mut members = face.loops.iter().cloned();
        let outer = members.next().expect("a cube face states a loop");
        let inner = members.collect();
        face.loops = FaceLoops::classified(outer.clone(), inner);
        let wire = serde_json::to_value(&*face).unwrap();
        assert_eq!(
            wire["loops"]["classification"],
            serde_json::json!("classified")
        );
        assert_eq!(wire["loops"]["outer"], serde_json::json!(outer.as_str()));
        assert!(wire["loops"].get("loops").is_none());
        let restored: Face = serde_json::from_value(wire).unwrap();
        assert_eq!(restored.loop_role(&outer), LoopBoundaryRole::Outer);
        let FaceLoops::Classified {
            outer: restored_outer,
            inner,
        } = &restored.loops
        else {
            panic!("classified loops")
        };
        assert_eq!(restored_outer, &outer);
        for inner in inner {
            assert_eq!(restored.loop_role(inner), LoopBoundaryRole::Inner);
        }
    }

    #[test]
    fn an_unclassified_face_states_its_loops_in_source_order() {
        let model = crate::examples::unit_cube()
            .expect("valid unit cube fixture")
            .model;
        let face = &model.faces[0];
        assert!(matches!(face.loops, FaceLoops::Unspecified { .. }));
        let wire = serde_json::to_value(face).unwrap();
        assert_eq!(
            wire["loops"]["classification"],
            serde_json::json!("unspecified")
        );
        assert!(wire["loops"].get("outer").is_none());
        assert_eq!(
            wire["loops"]["loops"]
                .as_array()
                .expect("a loops array")
                .len(),
            face.loops.len()
        );
        let restored: Face = serde_json::from_value(wire).unwrap();
        assert!(matches!(restored.loops, FaceLoops::Unspecified { .. }));
    }

    #[test]
    fn a_face_states_either_a_classification_or_a_loop_order() {
        for wire in [
            serde_json::json!({"classification": "classified", "outer": "test:model:loop#0", "inner": []}),
            serde_json::json!({"classification": "unspecified", "loops": ["test:model:loop#0"]}),
        ] {
            let loops: FaceLoops = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(&loops).unwrap(), wire);
        }
        // Mixing the two arms' keys names no shape.
        assert!(serde_json::from_value::<FaceLoops>(serde_json::json!({
            "classification": "classified",
            "outer": "test:model:loop#0",
            "loops": ["test:model:loop#0"]
        }))
        .is_err());
        // A face with no boundary has a spelling: nx, sat, step and freecad
        // each produce one, so `loops` is a plain list.
        let none: FaceLoops =
            serde_json::from_value(serde_json::json!({"classification": "unspecified",
                "loops": []}))
            .unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn an_unclassified_face_states_no_outer_loop() {
        let unclassified = FaceLoops::unspecified(vec![
            LoopId::mint("test:model:loop#0").expect("identity"),
            LoopId::mint("test:model:loop#1").expect("identity"),
        ]);
        assert!(matches!(unclassified, FaceLoops::Unspecified { .. }));
        assert_eq!(unclassified.len(), 2);
        assert_eq!(
            unclassified.role(&LoopId::mint("test:model:loop#0").expect("identity")),
            LoopBoundaryRole::Unspecified
        );
    }

    #[test]
    fn a_face_with_two_outer_loops_has_no_encoding() {
        // A face states one outer loop in one key, so a second has nowhere to
        // go: the wire has no shape for it.
        let wire = serde_json::json!({
            "id": "test:model:face#0",
            "shell": "test:model:shell#0",
            "surface": "test:model:surface#0",
            "sense": "forward",
            "loops": {
                "classification": "classified",
                "outer": ["test:model:loop#0", "test:model:loop#1"],
                "inner": []
            }
        });
        assert!(serde_json::from_value::<Face>(wire).is_err());
    }

    #[test]
    fn a_face_loop_list_rejects_an_unknown_key_by_name() {
        let wire = serde_json::json!({
            "id": "test:model:face#0",
            "shell": "test:model:shell#0",
            "surface": "test:model:surface#0",
            "sense": "forward",
            "loops": {
                "classification": "classified",
                "outer": "test:model:loop#0",
                "inner": [],
                "zz_bogus": 1
            }
        });
        let error = serde_json::from_value::<Face>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "{error}");
    }

    #[test]
    fn a_loop_record_states_no_role_of_its_own() {
        let wire = serde_json::json!({
            "id": "test:model:loop#0",
            "face": "test:model:face#0",
            "boundary_role": "outer",
            "boundary": { "kind": "vertex", "vertex": "test:model:vertex#0" }
        });
        let error = serde_json::from_value::<Loop>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("boundary_role"), "{error}");
    }

    #[test]
    fn a_point_position_must_be_finite() {
        use super::{Point, PointWire};
        use crate::features::FinitePoint3;
        use crate::ids::PointId;
        use crate::math::Point3;

        let id = PointId::mint("t:model:point#0").expect("identity grammar");
        for coordinates in [
            Point3::new(f64::NAN, 0.0, 0.0),
            Point3::new(0.0, f64::INFINITY, 0.0),
            Point3::new(0.0, 0.0, f64::NEG_INFINITY),
        ] {
            assert_eq!(
                Point::try_from(PointWire {
                    id: id.clone(),
                    position: coordinates,
                    source_object: None,
                }),
                Err(Point::NON_FINITE_POSITION)
            );
        }
        assert_eq!(
            Point::NON_FINITE_POSITION,
            "point position coordinates must be finite"
        );
        let point = Point::new(
            id,
            FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("a finite position is a point"),
            None,
        );
        assert_eq!(point.position().get(), Point3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn a_read_point_position_must_be_finite() {
        use super::{Point, PointWire};
        use crate::ids::PointId;
        use crate::math::Point3;

        // Reading a point is `PointWire` followed by this conversion, so the
        // conversion is where a wire position that is not finite is refused.
        // JSON itself states no infinity: `1e400` is out of range for the
        // number it would be read into, and the parser refuses it first.
        assert_eq!(
            Point::try_from(PointWire {
                id: PointId::mint("t:model:point#0").expect("identity grammar"),
                position: Point3::new(f64::INFINITY, 0.0, 0.0),
                source_object: None,
            }),
            Err("point position coordinates must be finite")
        );

        // The serialized shape is unchanged: the same keys read back, and an
        // unknown key is still refused.
        let point = serde_json::from_str::<Point>(
            r#"{"id":"t:model:point#0","position":{"x":1.0,"y":2.0,"z":3.0}}"#,
        )
        .expect("a finite position reads");
        assert_eq!(point.position().get(), Point3::new(1.0, 2.0, 3.0));
        assert_eq!(
            serde_json::to_string(&point).expect("a point writes"),
            r#"{"id":"t:model:point#0","position":{"x":1.0,"y":2.0,"z":3.0}}"#
        );
        assert!(serde_json::from_str::<Point>(
            r#"{"id":"t:model:point#0","position":{"x":1.0,"y":2.0,"z":3.0},"extra":1}"#
        )
        .is_err());
    }

    #[test]
    fn a_point_hands_back_and_takes_its_admitted_position() {
        use super::Point;
        use crate::features::FinitePoint3;
        use crate::ids::PointId;
        use crate::math::Point3;

        let raw = Point3::new(-0.0, f64::MAX, 5.0e-324);
        let mut point = Point::new(
            PointId::mint("t:model:point#0").expect("identity grammar"),
            FinitePoint3::new(raw).expect("a finite position is a point"),
            None,
        );
        let admitted = point.position();
        assert_eq!(
            admitted,
            FinitePoint3::new(raw).expect("finite coordinates")
        );
        assert_eq!(
            [admitted.x, admitted.y, admitted.z].map(f64::to_bits),
            [raw.x, raw.y, raw.z].map(f64::to_bits)
        );

        let moved = FinitePoint3::new(Point3::new(1.0, -2.0, 3.5)).expect("finite coordinates");
        point.set_position(moved);
        assert_eq!(point.position(), moved);
        assert_eq!(point.position().get(), Point3::new(1.0, -2.0, 3.5));
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_transform, Transform, "transform");
cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_color, Color, "color");
cadmpeg_core::named_optional_field!(deserialize_visible, bool, "visible");
cadmpeg_core::named_optional_field!(deserialize_isoparametric, bool, "isoparametric");
cadmpeg_core::named_optional_field!(
    deserialize_parameter_range,
    crate::geometry::DirectedParameterRange,
    "parameter_range"
);
cadmpeg_core::named_optional_field!(deserialize_use_curve, CoedgeUseCurve, "use_curve");
cadmpeg_core::named_optional_field!(deserialize_curve, CurveId, "curve");
cadmpeg_core::named_optional_field!(deserialize_param_range, [f64; 2], "param_range");
cadmpeg_core::named_optional_field!(
    deserialize_source_object,
    crate::provenance::SourceObjectAssociation,
    "source_object"
);
