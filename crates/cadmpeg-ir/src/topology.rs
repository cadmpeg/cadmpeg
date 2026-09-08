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

/// An oriented boundary of a region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
    pub parameter_range: Option<[f64; 2]>,
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
            use_curve_parameter_range: self.use_curve.as_ref().map(|value| value.parameter_range),
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
    pub parameter_range: [f64; 2],
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
                parameter_range,
            })),
            (None, None) => Ok(None),
            _ => Err(serde::de::Error::custom(
                "use_curve and use_curve_parameter_range must occur together",
            )),
        }
    }
}

/// An edge: a bounded segment of a 3D curve between two vertices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
    /// A carrier-less degenerate or tolerant edge has no canonical domain;
    /// finite native endpoint values may still be retained without ordering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param_range: Option<[f64; 2]>,
    /// Optional geometric tolerance in the document's length unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_tolerance")]
    pub tolerance: Option<crate::units::PositiveScalar>,
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

fn deserialize_tolerance<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<crate::units::PositiveScalar>, D::Error> {
    crate::units::deserialize_named(deserializer, "tolerance")
}

#[cfg(test)]
mod tests {
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
                parameter_range: [0.25, 0.75],
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
