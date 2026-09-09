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
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

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
struct ColorWire {
    r: f32,
    g: f32,
    b: f32,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Region {
    /// Arena id.
    pub id: RegionId,
    /// Owning body.
    pub body: BodyId,
    /// Ordered boundary shells. For a solid region, the first shell is the
    /// exterior boundary and all subsequent shells bound voids.
    pub shells: Vec<ShellId>,
}

impl Region {
    /// Exterior boundary of a solid region.
    pub fn exterior_shell(&self) -> Option<&ShellId> {
        self.shells.first()
    }

    /// Ordered void boundaries of a solid region.
    pub fn void_shells(&self) -> impl Iterator<Item = &ShellId> {
        self.shells.iter().skip(1)
    }
}

/// An oriented nonempty boundary of a region.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Shell {
    /// Arena id.
    pub id: ShellId,
    /// Owning region.
    pub region: RegionId,
    /// Faces of the shell.
    faces: Vec<FaceId>,
    /// Edges belonging directly to a wire shell.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    wire_edges: Vec<EdgeId>,
    /// Vertices belonging directly to a shell and not bounding an edge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    free_vertices: Vec<VertexId>,
}

impl Shell {
    /// Admits a shell that owns at least one face, wire edge, or free vertex.
    pub fn new(
        id: ShellId,
        region: RegionId,
        faces: Vec<FaceId>,
        wire_edges: Vec<EdgeId>,
        free_vertices: Vec<VertexId>,
    ) -> Result<Self, &'static str> {
        if faces.is_empty() && wire_edges.is_empty() && free_vertices.is_empty() {
            return Err("shell faces, wire_edges, and free_vertices must not all be empty");
        }
        Ok(Self {
            id,
            region,
            faces,
            wire_edges,
            free_vertices,
        })
    }

    /// Constructs a shell with one face.
    pub fn with_face(id: ShellId, region: RegionId, face: FaceId) -> Self {
        Self {
            id,
            region,
            faces: vec![face],
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        }
    }

    /// Constructs a shell with one wire edge.
    pub fn with_wire_edge(id: ShellId, region: RegionId, edge: EdgeId) -> Self {
        Self {
            id,
            region,
            faces: Vec::new(),
            wire_edges: vec![edge],
            free_vertices: Vec::new(),
        }
    }

    /// Constructs a shell with one free vertex.
    pub fn with_free_vertex(id: ShellId, region: RegionId, vertex: VertexId) -> Self {
        Self {
            id,
            region,
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: vec![vertex],
        }
    }

    /// Faces of the shell.
    pub fn faces(&self) -> &[FaceId] {
        &self.faces
    }

    /// Edges belonging directly to the shell.
    pub fn wire_edges(&self) -> &[EdgeId] {
        &self.wire_edges
    }

    /// Vertices belonging directly to the shell.
    pub fn free_vertices(&self) -> &[VertexId] {
        &self.free_vertices
    }

    /// Edits topology members and preserves the shell when admission fails.
    pub fn edit_topology<R>(
        &mut self,
        edit: impl FnOnce(&mut Vec<FaceId>, &mut Vec<EdgeId>, &mut Vec<VertexId>) -> R,
    ) -> Result<R, &'static str> {
        let mut faces = self.faces.clone();
        let mut wire_edges = self.wire_edges.clone();
        let mut free_vertices = self.free_vertices.clone();
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
        self.faces.push(face);
    }

    /// Appends a wire edge to the shell.
    pub fn add_wire_edge(&mut self, edge: EdgeId) {
        self.wire_edges.push(edge);
    }

    /// Appends a free vertex to the shell.
    pub fn add_free_vertex(&mut self, vertex: VertexId) {
        self.free_vertices.push(vertex);
    }
}

impl<'de> Deserialize<'de> for Shell {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct ShellWire {
            id: ShellId,
            region: RegionId,
            faces: Vec<FaceId>,
            #[serde(default)]
            wire_edges: Vec<EdgeId>,
            #[serde(default)]
            free_vertices: Vec<VertexId>,
        }
        let wire = ShellWire::deserialize(deserializer)?;
        Self::new(
            wire.id,
            wire.region,
            wire.faces,
            wire.wire_edges,
            wire.free_vertices,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// A face: a bounded region of a surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
    #[serde(with = "face_loops_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "Vec<LoopId>"))]
    pub loops: FaceLoops,
    /// Optional display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional display color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Optional geometric tolerance in the document's length unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_tolerance")]
    pub tolerance: Option<crate::units::PositiveScalar>,
}

impl Face {
    /// Explicit outer loop, or the first loop when the source did not classify.
    #[must_use]
    pub fn outer(&self) -> Option<&LoopId> {
        self.loops.outer()
    }

    /// Inner loops, excluding the outer when one is present.
    pub fn inner(&self) -> impl Iterator<Item = &LoopId> {
        self.loops.inner()
    }

    /// Role of `id` when it is a member of this face.
    #[must_use]
    pub fn loop_role(&self, id: &LoopId) -> LoopBoundaryRole {
        self.loops.role(id)
    }
}

/// Face loop ids with at most one outer loop.
///
/// `Unspecified` keeps source order when the source did not classify outer
/// versus inner. That state stays representable so CADIR JSON that emitted
/// `boundary_role: unspecified` is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceLoops {
    ids: Vec<LoopId>,
    classification: FaceLoopClassification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FaceLoopClassification {
    Unspecified,
    Classified { outer: Option<usize> },
}

impl Default for FaceLoops {
    fn default() -> Self {
        Self::unspecified(Vec::new())
    }
}

impl FaceLoops {
    /// Source order with no outer/inner classification.
    #[must_use]
    pub fn unspecified(ids: Vec<LoopId>) -> Self {
        Self {
            ids,
            classification: FaceLoopClassification::Unspecified,
        }
    }

    /// Classified loops. `outer` is omitted when the surface domain is the
    /// exterior. Loop order is outer (when present) then inner.
    #[must_use]
    pub fn classified(outer: Option<LoopId>, inner: Vec<LoopId>) -> Self {
        let mut ids = Vec::with_capacity(outer.is_some() as usize + inner.len());
        let outer_index = outer.as_ref().map(|_| 0);
        if let Some(outer) = outer {
            ids.push(outer);
        }
        ids.extend(inner);
        Self {
            ids,
            classification: FaceLoopClassification::Classified { outer: outer_index },
        }
    }

    /// Explicit outer loop, or the first loop when unclassified.
    #[must_use]
    pub fn outer(&self) -> Option<&LoopId> {
        match self.classification {
            FaceLoopClassification::Unspecified => self.ids.first(),
            FaceLoopClassification::Classified { outer } => {
                outer.and_then(|index| self.ids.get(index))
            }
        }
    }

    /// Inner loops, excluding the outer when one is present.
    pub fn inner(&self) -> impl Iterator<Item = &LoopId> + '_ {
        let outer = match self.classification {
            FaceLoopClassification::Unspecified => (!self.ids.is_empty()).then_some(0),
            FaceLoopClassification::Classified { outer } => outer,
        };
        self.ids.iter().enumerate().filter_map(
            move |(index, id)| {
                if Some(index) == outer {
                    None
                } else {
                    Some(id)
                }
            },
        )
    }

    /// Ordered loop ids.
    #[must_use]
    pub fn as_slice(&self) -> &[LoopId] {
        &self.ids
    }

    /// Number of loops.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether the face has no loops.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Append a loop. A classified face treats the new loop as inner.
    pub fn push(&mut self, id: LoopId) {
        self.ids.push(id);
    }

    /// Drop every loop and return to unspecified.
    pub fn clear(&mut self) {
        self.ids.clear();
        self.classification = FaceLoopClassification::Unspecified;
    }

    /// Whether the source left outer/inner unclassified.
    #[must_use]
    pub fn is_unspecified(&self) -> bool {
        matches!(self.classification, FaceLoopClassification::Unspecified)
    }

    /// Mark loops as classified. `outer` must be a member, or `None` when every
    /// loop is inner.
    pub fn classify_outer(&mut self, outer: Option<&LoopId>) {
        let outer = outer.and_then(|id| self.ids.iter().position(|member| member == id));
        self.classification = FaceLoopClassification::Classified { outer };
    }

    /// Apply per-loop roles in id order. Unspecified-only leaves the face
    /// unclassified. More than one outer keeps the first.
    pub fn apply_roles(&mut self, roles: &[LoopBoundaryRole]) {
        let mut outer = None;
        let mut classified = false;
        for (index, role) in roles.iter().copied().enumerate().take(self.ids.len()) {
            match role {
                LoopBoundaryRole::Unspecified => {}
                LoopBoundaryRole::Outer => {
                    classified = true;
                    if outer.is_none() {
                        outer = Some(index);
                    }
                }
                LoopBoundaryRole::Inner => classified = true,
            }
        }
        if classified {
            self.classification = FaceLoopClassification::Classified { outer };
        }
    }

    fn classify_from_roles(
        &mut self,
        roles: &HashMap<LoopId, LoopBoundaryRole>,
    ) -> Result<(), String> {
        let mut outer = None;
        let mut classified = false;
        for (index, id) in self.ids.iter().enumerate() {
            match roles.get(id).copied().unwrap_or_default() {
                LoopBoundaryRole::Unspecified => {}
                LoopBoundaryRole::Outer => {
                    classified = true;
                    if outer.is_some() {
                        return Err("face has more than one explicit outer loop".into());
                    }
                    outer = Some(index);
                }
                LoopBoundaryRole::Inner => classified = true,
            }
        }
        if classified {
            self.classification = FaceLoopClassification::Classified { outer };
        }
        Ok(())
    }

    /// Role of `id` when it is a member of this face.
    #[must_use]
    pub fn role(&self, id: &LoopId) -> LoopBoundaryRole {
        self.iter()
            .position(|member| member == id)
            .map_or(LoopBoundaryRole::Unspecified, |index| self.role_of(index))
    }

    fn role_of(&self, index: usize) -> LoopBoundaryRole {
        match self.classification {
            FaceLoopClassification::Unspecified => LoopBoundaryRole::Unspecified,
            FaceLoopClassification::Classified { outer: Some(outer) } if outer == index => {
                LoopBoundaryRole::Outer
            }
            FaceLoopClassification::Classified { .. } => LoopBoundaryRole::Inner,
        }
    }
}

impl From<Vec<LoopId>> for FaceLoops {
    fn from(ids: Vec<LoopId>) -> Self {
        Self::unspecified(ids)
    }
}

impl std::ops::Deref for FaceLoops {
    type Target = [LoopId];

    fn deref(&self) -> &[LoopId] {
        &self.ids
    }
}

impl std::ops::DerefMut for FaceLoops {
    fn deref_mut(&mut self) -> &mut [LoopId] {
        &mut self.ids
    }
}

impl PartialEq<Vec<LoopId>> for FaceLoops {
    fn eq(&self, other: &Vec<LoopId>) -> bool {
        self.ids == *other
    }
}

impl PartialEq<FaceLoops> for Vec<LoopId> {
    fn eq(&self, other: &FaceLoops) -> bool {
        *self == other.ids
    }
}

impl<'a> IntoIterator for &'a FaceLoops {
    type Item = &'a LoopId;
    type IntoIter = std::slice::Iter<'a, LoopId>;

    fn into_iter(self) -> Self::IntoIter {
        self.ids.iter()
    }
}

mod face_loops_wire {
    use super::{FaceLoops, LoopId};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &FaceLoops, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.ids.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<FaceLoops, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(FaceLoops::unspecified(Vec::<LoopId>::deserialize(
            deserializer,
        )?))
    }
}

/// Classification of a loop within its owning face.
///
/// Stored on [`FaceLoops`], not on [`Loop`], so a loop cannot disagree with
/// face membership. Serialize still emits `boundary_role` on each loop.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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

thread_local! {
    static LOOP_BOUNDARY_ROLES: RefCell<HashMap<LoopId, LoopBoundaryRole>> =
        RefCell::new(HashMap::new());
}

fn install_loop_boundary_roles(faces: &[Face]) {
    let mut roles = HashMap::with_capacity(faces.iter().map(|face| face.loops.len()).sum());
    for face in faces {
        for (index, id) in face.loops.iter().enumerate() {
            roles.insert(id.clone(), face.loops.role_of(index));
        }
    }
    LOOP_BOUNDARY_ROLES.with(|slot| *slot.borrow_mut() = roles);
}

pub(crate) fn record_loop_boundary_role(id: LoopId, role: LoopBoundaryRole) {
    LOOP_BOUNDARY_ROLES.with(|slot| {
        slot.borrow_mut().insert(id, role);
    });
}

pub(crate) fn rebind_face_loop_roles(faces: &mut [Face]) -> Result<(), String> {
    let result = LOOP_BOUNDARY_ROLES.with(|slot| {
        let roles = slot.borrow();
        for face in faces.iter_mut() {
            face.loops.classify_from_roles(&roles)?;
        }
        Ok(())
    });
    LOOP_BOUNDARY_ROLES.with(|slot| slot.borrow_mut().clear());
    result
}

pub(crate) fn loop_boundary_role_for(id: &LoopId) -> LoopBoundaryRole {
    LOOP_BOUNDARY_ROLES.with(|slot| {
        slot.borrow()
            .get(id)
            .copied()
            .unwrap_or(LoopBoundaryRole::Unspecified)
    })
}

/// A closed boundary of a face, expressed as an ordered ring of coedges or one
/// vertex use at a surface singularity.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Loop {
    /// Arena id.
    pub id: LoopId,
    /// Owning face.
    pub face: FaceId,
    /// Vertex-only or coedge-ring boundary.
    #[cfg_attr(feature = "schema", schemars(with = "LoopBoundarySchemaWire"))]
    pub boundary: LoopBoundary,
}

#[derive(Deserialize)]
struct LoopReadWire {
    id: LoopId,
    face: FaceId,
    #[serde(default)]
    boundary_role: LoopBoundaryRole,
    #[serde(flatten, with = "loop_boundary_wire")]
    boundary: LoopBoundary,
}

impl<'de> Deserialize<'de> for Loop {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = LoopReadWire::deserialize(deserializer)?;
        record_loop_boundary_role(wire.id.clone(), wire.boundary_role);
        Ok(Self {
            id: wire.id,
            face: wire.face,
            boundary: wire.boundary,
        })
    }
}

#[derive(Serialize)]
struct LoopWriteWire<'a> {
    id: &'a LoopId,
    face: &'a FaceId,
    boundary_role: LoopBoundaryRole,
    #[serde(flatten, with = "loop_boundary_wire")]
    boundary: &'a LoopBoundary,
}

impl Serialize for Loop {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        LoopWriteWire {
            id: &self.id,
            face: &self.face,
            boundary_role: loop_boundary_role_for(&self.id),
            boundary: &self.boundary,
        }
        .serialize(serializer)
    }
}

/// One ordered parameter-space representation of a coedge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PcurveUse {
    /// Parameter-space curve carrier.
    pub pcurve: PcurveId,
    /// Whether the source declares this curve isoparametric on the face surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isoparametric: Option<bool>,
    /// Interval on the pcurve's own parameterization used by this coedge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_range: Option<crate::geometry::DirectedParameterRange>,
}

/// The mutually exclusive forms of a loop boundary.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum LoopBoundary {
    /// One unanchored vertex at a surface singularity.
    Vertex {
        /// Referenced pole vertex.
        vertex: VertexId,
        /// Ordered parameter-space images associated with the pole.
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
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LoopRing {
    coedges: Vec<CoedgeId>,
    vertex_uses: Vec<AnchoredVertexUse>,
}

impl LoopRing {
    /// Construct a ring containing one coedge.
    pub fn single(coedge: CoedgeId) -> Self {
        Self {
            coedges: vec![coedge],
            vertex_uses: Vec::new(),
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

    /// Role of this loop on its owning face.
    #[must_use]
    pub fn boundary_role_in(&self, faces: &[Face]) -> LoopBoundaryRole {
        faces
            .iter()
            .find(|face| face.id == self.face)
            .map(|face| face.loop_role(&self.id))
            .unwrap_or_default()
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
pub struct AnchoredVertexUse {
    /// Referenced pole vertex.
    pub vertex: VertexId,
    /// Preceding coedge in the cyclic traversal.
    pub after: CoedgeId,
    /// Ordered parameter-space images associated with this pole occurrence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pcurves: Vec<PcurveUse>,
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(dead_code, reason = "fields define the loop boundary wire schema")]
struct LoopBoundarySchemaWire {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    coedges: Vec<CoedgeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    vertex_uses: Vec<LoopVertexUseSchemaWire>,
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(dead_code, reason = "fields define the loop vertex-use wire schema")]
struct LoopVertexUseSchemaWire {
    vertex: VertexId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after: Option<CoedgeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pcurves: Vec<PcurveUse>,
}

mod loop_boundary_wire {
    use super::{AnchoredVertexUse, CoedgeId, LoopBoundary, LoopRing, PcurveUse, VertexId};
    use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Wire {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        coedges: Vec<CoedgeId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        vertex_uses: Vec<VertexUseWire>,
    }

    #[derive(Serialize, Deserialize)]
    struct VertexUseWire {
        vertex: VertexId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after: Option<CoedgeId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pcurves: Vec<PcurveUse>,
    }

    pub fn serialize<S>(value: &LoopBoundary, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match value {
            LoopBoundary::Vertex { vertex, pcurves } => Wire {
                coedges: Vec::new(),
                vertex_uses: vec![VertexUseWire {
                    vertex: vertex.clone(),
                    after: None,
                    pcurves: pcurves.clone(),
                }],
            },
            LoopBoundary::Ring(ring) => Wire {
                coedges: ring.coedges().to_vec(),
                vertex_uses: ring
                    .vertex_uses()
                    .iter()
                    .map(|vertex_use| VertexUseWire {
                        vertex: vertex_use.vertex.clone(),
                        after: Some(vertex_use.after.clone()),
                        pcurves: vertex_use.pcurves.clone(),
                    })
                    .collect(),
            },
        };
        wire.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<LoopBoundary, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = Wire::deserialize(deserializer)?;
        if wire.coedges.is_empty() {
            let [vertex_use] = <[VertexUseWire; 1]>::try_from(wire.vertex_uses).map_err(|_| {
                D::Error::custom("loop vertex_uses must contain one item when coedges is empty")
            })?;
            if vertex_use.after.is_some() {
                return Err(D::Error::custom(
                    "loop vertex use must omit after when coedges is empty",
                ));
            }
            return Ok(LoopBoundary::Vertex {
                vertex: vertex_use.vertex,
                pcurves: vertex_use.pcurves,
            });
        }

        let vertex_uses = wire
            .vertex_uses
            .into_iter()
            .map(|vertex_use| {
                let after = vertex_use
                    .after
                    .ok_or_else(|| D::Error::custom("loop ring vertex use must include after"))?;
                if !wire.coedges.contains(&after) {
                    return Err(D::Error::custom(
                        "loop ring vertex use after must name a coedge in the ring",
                    ));
                }
                Ok(AnchoredVertexUse {
                    vertex: vertex_use.vertex,
                    after,
                    pcurves: vertex_use.pcurves,
                })
            })
            .collect::<Result<_, D::Error>>()?;
        LoopRing::new(wire.coedges, vertex_uses)
            .map(LoopBoundary::Ring)
            .map_err(D::Error::custom)
    }
}

/// One use of an edge by a loop.
///
/// Coedges form a loop ring through the owning [`Loop`] coedge order, and a
/// radial ring around their shared edge through `radial_next`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
    #[serde(flatten, deserialize_with = "coedge_use_curve_wire::deserialize")]
    #[cfg_attr(feature = "schema", schemars(with = "CoedgeUseCurveSchemaWire"))]
    pub use_curve: Option<CoedgeUseCurve>,
}

thread_local! {
    static COEDGE_RING_NEIGHBORS: RefCell<HashMap<CoedgeId, (CoedgeId, CoedgeId)>> =
        RefCell::new(HashMap::new());
}

fn install_coedge_ring_neighbors(loops: &[Loop], coedges: &[Coedge]) {
    let mut neighbors = HashMap::with_capacity(coedges.len());
    for coedge in coedges {
        if let Some(pair) = loops
            .iter()
            .find(|loop_| loop_.id == coedge.owner_loop)
            .and_then(|loop_| loop_.ring_neighbors_of(coedge))
        {
            neighbors.insert(coedge.id.clone(), pair);
        }
    }
    COEDGE_RING_NEIGHBORS.with(|slot| *slot.borrow_mut() = neighbors);
}

/// Next and previous coedge ids from the owning loop ring.
#[must_use]
pub fn coedge_ring_neighbors(loops: &[Loop], coedge: &Coedge) -> Option<(CoedgeId, CoedgeId)> {
    loops
        .iter()
        .find_map(|loop_| loop_.ring_neighbors_of(coedge))
}

/// Serialize an entity graph with loop roles and coedge neighbors derived from
/// its owning topology. Nested calls restore the enclosing graph's context.
pub fn with_topology_serialization<T>(
    faces: &[Face],
    loops: &[Loop],
    coedges: &[Coedge],
    serialize: impl FnOnce() -> T,
) -> T {
    let _scope = TopologyWireScope::new(faces, loops, coedges);
    serialize()
}

pub(crate) struct TopologyWireScope {
    neighbors: HashMap<CoedgeId, (CoedgeId, CoedgeId)>,
    roles: HashMap<LoopId, LoopBoundaryRole>,
    // A scope must restore the same thread-local state that it installed.
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl TopologyWireScope {
    pub(crate) fn new(faces: &[Face], loops: &[Loop], coedges: &[Coedge]) -> Self {
        let neighbors = COEDGE_RING_NEIGHBORS.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
        let roles = LOOP_BOUNDARY_ROLES.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
        let scope = Self {
            neighbors,
            roles,
            _thread: std::marker::PhantomData,
        };
        install_coedge_ring_neighbors(loops, coedges);
        install_loop_boundary_roles(faces);
        scope
    }
}

impl Drop for TopologyWireScope {
    fn drop(&mut self) {
        COEDGE_RING_NEIGHBORS.with(|slot| *slot.borrow_mut() = std::mem::take(&mut self.neighbors));
        LOOP_BOUNDARY_ROLES.with(|slot| *slot.borrow_mut() = std::mem::take(&mut self.roles));
    }
}

#[derive(Serialize)]
struct CoedgeWriteWire<'a> {
    id: &'a CoedgeId,
    owner_loop: &'a LoopId,
    edge: &'a EdgeId,
    next: CoedgeId,
    previous: CoedgeId,
    radial_next: &'a CoedgeId,
    sense: Sense,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pcurves: &'a Vec<PcurveUse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    use_curve: Option<&'a CurveId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    use_curve_parameter_range: Option<[f64; 2]>,
}

impl Serialize for Coedge {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (next, previous) = COEDGE_RING_NEIGHBORS
            .with(|slot| slot.borrow().get(&self.id).cloned())
            .ok_or_else(|| {
                serde::ser::Error::custom(format!(
                    "coedge {} is absent from its owning loop ring",
                    self.id.as_str()
                ))
            })?;
        CoedgeWriteWire {
            id: &self.id,
            owner_loop: &self.owner_loop,
            edge: &self.edge,
            next,
            previous,
            radial_next: &self.radial_next,
            sense: self.sense,
            pcurves: &self.pcurves,
            use_curve: self.use_curve.as_ref().map(|value| &value.curve),
            use_curve_parameter_range: self
                .use_curve
                .as_ref()
                .map(|value| value.parameter_range.endpoints()),
        }
        .serialize(serializer)
    }
}

/// A coedge-local curve and its loop-traversal interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CoedgeUseCurve {
    /// Local 3D curve carrier.
    pub curve: CurveId,
    /// Interval on the carrier in loop-traversal order.
    pub parameter_range: ParameterInterval,
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(dead_code, reason = "fields define the coedge use-curve wire schema")]
struct CoedgeUseCurveSchemaWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    use_curve: Option<CurveId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    use_curve_parameter_range: Option<[f64; 2]>,
}

mod coedge_use_curve_wire {
    use super::{CoedgeUseCurve, CurveId};
    use serde::{Deserialize, Deserializer};

    #[derive(Deserialize)]
    struct Wire {
        #[serde(default)]
        use_curve: Option<CurveId>,
        #[serde(default)]
        use_curve_parameter_range: Option<[f64; 2]>,
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<CoedgeUseCurve>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = Wire::deserialize(deserializer)?;
        match (wire.use_curve, wire.use_curve_parameter_range) {
            (Some(curve), Some(parameter_range)) => Ok(Some(CoedgeUseCurve {
                curve,
                parameter_range: super::ParameterInterval::new(parameter_range)
                    .map_err(serde::de::Error::custom)?,
            })),
            (None, None) => Ok(None),
            _ => Err(serde::de::Error::custom(
                "use_curve and use_curve_parameter_range must occur together",
            )),
        }
    }
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

/// An edge carrier and its admitted parameter endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "EdgeCarrierWire", into = "EdgeCarrierWire")]
pub struct EdgeCarrier {
    curve: Option<CurveId>,
    param_range: Option<[f64; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct EdgeCarrierWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve: Option<CurveId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    param_range: Option<[f64; 2]>,
}

impl EdgeCarrier {
    /// Construct a carrier without parameter endpoints.
    pub fn unbounded(curve: Option<CurveId>) -> Self {
        Self {
            curve,
            param_range: None,
        }
    }

    /// Admit a carrier and finite endpoints, ordered when the carrier is present.
    pub fn new(
        curve: Option<CurveId>,
        param_range: Option<[f64; 2]>,
    ) -> Result<Self, &'static str> {
        if let Some(range) = param_range {
            if curve.is_some() {
                ParameterInterval::new(range)
                    .map_err(|_| "edge param_range must be finite and ordered")?;
            } else if !range.iter().all(|value| value.is_finite()) {
                return Err("edge param_range endpoints must be finite");
            }
        }
        Ok(Self { curve, param_range })
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
            curve: value.curve,
            param_range: value.param_range,
        }
    }
}

/// An edge between two vertices with an optional curve carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Edge {
    /// Arena id.
    pub id: EdgeId,
    /// Carrier and its admitted parameter range.
    #[serde(flatten)]
    pub carrier: EdgeCarrier,
    /// Start vertex.
    pub start: VertexId,
    /// End vertex.
    pub end: VertexId,
    /// Optional geometric tolerance in the document's length unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_tolerance")]
    pub tolerance: Option<crate::units::PositiveScalar>,
}

impl Edge {
    /// Map the carrier identity without changing its presence or parameter range.
    pub fn map_curve(&mut self, map: impl FnOnce(&CurveId) -> CurveId) {
        if let Some(curve) = &mut self.carrier.curve {
            *curve = map(curve);
        }
    }

    /// Replace the curve while retaining the range if it remains valid.
    pub fn set_curve(&mut self, curve: Option<CurveId>) -> Result<(), &'static str> {
        self.carrier = EdgeCarrier::new(curve, self.param_range())?;
        Ok(())
    }
    /// Replace the parameter endpoints if valid for the carrier.
    pub fn set_param_range(&mut self, range: Option<[f64; 2]>) -> Result<(), &'static str> {
        self.carrier = EdgeCarrier::new(self.curve().clone(), range)?;
        Ok(())
    }
    /// Return the underlying curve carrier.
    pub fn curve(&self) -> &Option<CurveId> {
        &self.carrier.curve
    }
    /// Return the parameter endpoints.
    pub fn param_range(&self) -> Option<[f64; 2]> {
        self.carrier.param_range
    }
}

/// A vertex: a topological point referencing a position carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Vertex {
    /// Arena id.
    pub id: VertexId,
    /// Position carrier.
    pub point: PointId,
    /// Optional geometric tolerance in the document's length unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_tolerance")]
    pub tolerance: Option<crate::units::PositiveScalar>,
}

/// A position carrier for a vertex.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Point {
    /// Arena id.
    pub id: PointId,
    /// Coordinates in the document's length unit.
    pub position: Point3,
    /// Source object carrying this free point, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_object: Option<crate::provenance::SourceObjectAssociation>,
}

crate::units::named_field!(
    deserialize_tolerance,
    Option<crate::units::PositiveScalar>,
    "tolerance"
);

#[cfg(test)]
mod tests {
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
    fn edge_parameter_edits_commit_only_valid_pairs() {
        let mut edge = crate::examples::unit_cube().model.edges.remove(0);
        let original = edge.clone();
        assert!(edge.set_param_range(Some([2.0, 1.0])).is_err());
        assert_eq!(edge, original);
        edge.set_curve(None).unwrap();
        edge.set_param_range(Some([2.0, 1.0])).unwrap();
        let carrierless = edge.clone();
        assert!(edge.set_curve(original.curve().clone()).is_err());
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
            let mut wire = serde_json::json!({"id":id,"region":region,"faces":faces});
            if !edges.is_empty() {
                wire["wire_edges"] = serde_json::json!(edges);
            }
            if !vertices.is_empty() {
                wire["free_vertices"] = serde_json::json!(vertices);
            }
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
        with_topology_serialization, AnchoredVertexUse, Coedge, CoedgeUseCurve, Loop, LoopBoundary,
        LoopRing,
    };

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
            "next": "test:model:coedge#0",
            "previous": "test:model:coedge#0",
            "radial_next": "test:model:coedge#0",
            "sense": "forward",
            "use_curve": "test:model:curve#0",
            "use_curve_parameter_range": [0.25, 0.75]
        })
    }

    #[test]
    fn coedge_use_curve_preserves_the_flat_wire_fields() {
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
        let encoded = with_topology_serialization(
            &[],
            std::slice::from_ref(&loop_),
            std::slice::from_ref(&coedge),
            || serde_json::to_value(&coedge).unwrap(),
        );
        assert_eq!(encoded["use_curve"], "test:model:curve#0");
        assert_eq!(
            encoded["use_curve_parameter_range"],
            serde_json::json!([0.25, 0.75])
        );
    }

    #[test]
    fn topology_serialization_restores_nested_context_after_unwind() {
        let model = crate::examples::unit_cube().model;
        let coedge = &model.coedges[0];
        assert!(serde_json::to_value(coedge).is_err());
        with_topology_serialization(&model.faces, &model.loops, &model.coedges, || {
            let expected = serde_json::to_value(coedge).unwrap();
            let interrupted = std::panic::catch_unwind(|| {
                with_topology_serialization(&[], &[], &[], || {
                    assert!(serde_json::to_value(coedge).is_err());
                    panic!("interrupt nested serialization");
                });
            });
            assert!(interrupted.is_err());
            assert_eq!(serde_json::to_value(coedge).unwrap(), expected);
        });
        assert!(serde_json::to_value(coedge).is_err());
    }

    #[test]
    fn model_deserialization_restores_enclosing_topology_context() {
        let mut model = crate::examples::unit_cube().model;
        let loop_ = &model.loops[0];
        model
            .faces
            .iter_mut()
            .find(|face| face.id == loop_.face)
            .unwrap()
            .loops
            .classify_outer(Some(&loop_.id));
        let empty_model = serde_json::to_value(crate::document::Model::default()).unwrap();
        with_topology_serialization(&model.faces, &model.loops, &model.coedges, || {
            let expected = serde_json::to_value(loop_).unwrap();
            serde_json::from_value::<crate::document::Model>(empty_model.clone()).unwrap();
            assert_eq!(serde_json::to_value(loop_).unwrap(), expected);
            let mut conflicting_loop = expected.clone();
            conflicting_loop["boundary_role"] = serde_json::json!("inner");
            let mut invalid = empty_model.clone();
            invalid["loops"] = serde_json::json!([conflicting_loop]);
            invalid["vertices"] = serde_json::json!("invalid");
            assert!(serde_json::from_value::<crate::document::Model>(invalid).is_err());
            assert_eq!(serde_json::to_value(loop_).unwrap(), expected);
        });
    }

    #[test]
    fn coedge_use_curve_rejects_a_split_wire_pair() {
        let mut json = coedge_json();
        json.as_object_mut()
            .unwrap()
            .remove("use_curve_parameter_range");
        assert!(serde_json::from_value::<Coedge>(json).is_err());
    }

    #[test]
    fn vertex_loop_preserves_the_flat_wire_fields() {
        let json = serde_json::json!({
            "id": "test:model:loop#0",
            "face": "test:model:face#0",
            "boundary_role": "outer",
            "vertex_uses": [{ "vertex": "test:model:vertex#0" }]
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
    fn ring_loop_preserves_the_flat_wire_fields() {
        let json = serde_json::json!({
            "id": "test:model:loop#0",
            "face": "test:model:face#0",
            "boundary_role": "outer",
            "coedges": ["test:model:coedge#0"],
            "vertex_uses": [{
                "vertex": "test:model:vertex#0",
                "after": "test:model:coedge#0"
            }]
        });
        let loop_: Loop = serde_json::from_value(json.clone()).unwrap();
        assert!(matches!(loop_.boundary, LoopBoundary::Ring(_)));
        assert_eq!(serde_json::to_value(loop_).unwrap(), json);
    }

    #[test]
    fn loop_boundary_rejects_split_wire_forms() {
        let vertex_only_with_anchor = serde_json::json!({
            "id": "test:model:loop#0",
            "face": "test:model:face#0",
            "boundary_role": "outer",
            "vertex_uses": [{
                "vertex": "test:model:vertex#0",
                "after": "test:model:coedge#0"
            }]
        });
        assert!(serde_json::from_value::<Loop>(vertex_only_with_anchor).is_err());

        let ring_without_anchor = serde_json::json!({
            "id": "test:model:loop#0",
            "face": "test:model:face#0",
            "boundary_role": "outer",
            "coedges": ["test:model:coedge#0"],
            "vertex_uses": [{ "vertex": "test:model:vertex#0" }]
        });
        assert!(serde_json::from_value::<Loop>(ring_without_anchor).is_err());
    }
}
