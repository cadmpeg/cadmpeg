// SPDX-License-Identifier: Apache-2.0
//! Deterministic B-rep emission engine shared by format codecs.
//!
//! [`TopologyBuilder`] stages a `body → region → shell → face → loop → coedge`
//! hierarchy plus the `edge`, `vertex`, and `point` carriers a codec produces
//! while walking a source container, then appends every entity into a
//! [`Model`] in one pass. It replaces the hand-rolled ring-walk and finalize
//! skeletons each codec would otherwise carry.
//!
//! # Two design pillars
//!
//! **Pillar 1 — the codec owns every id.** The builder never renumbers,
//! generates, or reassigns identifiers. A caller supplies fully-formed typed
//! ids ([`BodyId`], [`FaceId`], …), usually built from the source format's own
//! ordinals through the codec's existing `format!` scheme. The builder only
//! records those ids, checks them for uniqueness, and wires cross-references by
//! the exact strings it was handed. This is what preserves the
//! *identical input bytes → identical ids* guarantee across adoption: swapping
//! a codec's inline arena pushes for builder calls changes no emitted id.
//! [`IdAllocator`] is an opt-in convenience for ordinal-scheme codecs; it only
//! formats strings and holds no state that could drift.
//!
//! **Pillar 2 — the builder does not sort.** [`TopologyBuilder::finish`]
//! appends per arena in a fixed layer order (bodies, regions, shells, faces,
//! loops, coedges, edges, vertices, points) and, within each arena, in
//! first-registration order. Lexicographic canonical ordering is applied later
//! and solely by [`Model::finalize`], on the uniform decode path. There is no
//! second sort path here; two codecs that register the same entities in
//! different orders converge only after `finalize`, never before it.
//!
//! # What the builder owns
//!
//! - Id uniqueness across all of its registrations.
//! - Loop-ring wiring: a closed ring's `next`/`previous` links follow the slice
//!   order the coedges were registered in.
//! - Radial defaulting: a coedge's `radial_next` is itself (a laminar boundary)
//!   until an explicit [`radial_ring`](TopologyBuilder::radial_ring) links it.
//! - Parent-list aggregation: `Body::regions`, `Region::shells`, `Shell::faces`,
//!   and `Face::loops` accumulate child ids in registration order. That order is
//!   semantic — the first loop registered against a face is conventionally its
//!   outer boundary.
//! - Fixed-layer append order at [`finish`](TopologyBuilder::finish).

use std::collections::{HashMap, HashSet};

use super::{
    Body, BodyKind, Coedge, Color, Edge, Face, Loop, LoopBoundaryRole, PcurveUse, Point, Region,
    Sense, Shell, Vertex, VertexUse,
};
use crate::document::Model;
use crate::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    VertexId,
};
use crate::math::Point3;
use crate::provenance::SourceObjectAssociation;
use crate::transform::Transform;

/// A failure encountered while staging or finishing topology.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildError {
    /// An id was registered more than once across all arenas. The string is the
    /// colliding id.
    #[error("duplicate id: {0}")]
    DuplicateId(String),
    /// A child named a parent that was never registered. The string is the
    /// missing parent id.
    #[error("unknown owner: {0}")]
    UnknownOwner(String),
    /// [`edge`](TopologyBuilder::edge) was called twice for one id with
    /// differing specifications. The string is the edge id.
    #[error("conflicting redefinition of edge: {0}")]
    ConflictingRedefinition(String),
    /// A ring was registered with neither coedges nor vertex uses. The string is
    /// the loop id.
    #[error("empty ring: {0}")]
    EmptyRing(String),
    /// A radial ring named a coedge that no ring registered. The string is the
    /// missing coedge id.
    #[error("unknown coedge in radial ring: {0}")]
    UnknownCoedge(String),
    /// A radial ring spanned coedges that do not share one edge, so it is not a
    /// ring around a single shared edge. The string describes the mismatch.
    #[error("radial ring not anchored to a single shared edge: {0}")]
    DanglingRadial(String),
}

/// Formats ordinal-scheme topology ids under a fixed prefix.
///
/// This is an opt-in helper for codecs whose native identity is an ordinal:
/// `IdAllocator::new("nx:brep").face(3)` yields the [`FaceId`]
/// `"nx:brep:face#3"`. It allocates nothing and holds no counter — it only
/// concatenates `"{prefix}:{kind}#{ordinal}"`, so identical ordinals always map
/// to identical ids (Pillar 1). Codecs with a bespoke id scheme construct typed
/// ids directly and skip this type.
#[derive(Debug, Clone)]
pub struct IdAllocator {
    prefix: String,
}

impl IdAllocator {
    /// Create an allocator that prefixes every id with `prefix`.
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
        }
    }

    fn id(&self, kind: &str, ordinal: usize) -> String {
        format!("{}:{}#{}", self.prefix, kind, ordinal)
    }

    /// `"{prefix}:body#{ordinal}"`.
    pub fn body(&self, ordinal: usize) -> BodyId {
        BodyId(self.id("body", ordinal))
    }

    /// `"{prefix}:region#{ordinal}"`.
    pub fn region(&self, ordinal: usize) -> RegionId {
        RegionId(self.id("region", ordinal))
    }

    /// `"{prefix}:shell#{ordinal}"`.
    pub fn shell(&self, ordinal: usize) -> ShellId {
        ShellId(self.id("shell", ordinal))
    }

    /// `"{prefix}:face#{ordinal}"`.
    pub fn face(&self, ordinal: usize) -> FaceId {
        FaceId(self.id("face", ordinal))
    }

    /// `"{prefix}:loop#{ordinal}"`.
    pub fn loop_(&self, ordinal: usize) -> LoopId {
        LoopId(self.id("loop", ordinal))
    }

    /// `"{prefix}:coedge#{ordinal}"`.
    pub fn coedge(&self, ordinal: usize) -> CoedgeId {
        CoedgeId(self.id("coedge", ordinal))
    }

    /// `"{prefix}:edge#{ordinal}"`.
    pub fn edge(&self, ordinal: usize) -> EdgeId {
        EdgeId(self.id("edge", ordinal))
    }

    /// `"{prefix}:vertex#{ordinal}"`.
    pub fn vertex(&self, ordinal: usize) -> VertexId {
        VertexId(self.id("vertex", ordinal))
    }

    /// `"{prefix}:point#{ordinal}"`.
    pub fn point(&self, ordinal: usize) -> PointId {
        PointId(self.id("point", ordinal))
    }
}

/// Per-body attributes supplied to [`TopologyBuilder::body`].
///
/// `regions` is intentionally absent: the builder aggregates it from
/// [`region`](TopologyBuilder::region) registrations in order.
#[derive(Debug, Clone, Default)]
pub struct BodySpec {
    /// Dimensional kind of the body.
    pub kind: BodyKind,
    /// Optional world placement of the body's geometry.
    pub transform: Option<Transform>,
    /// Optional display name.
    pub name: Option<String>,
    /// Optional display color.
    pub color: Option<Color>,
    /// Whether the source displays the body, when recorded.
    pub visible: Option<bool>,
}

/// Per-face attributes supplied to [`TopologyBuilder::face`].
///
/// `loops` is intentionally absent: the builder aggregates it from
/// [`ring`](TopologyBuilder::ring) registrations in order, so the first ring
/// registered against a face becomes its conventionally-outer loop.
#[derive(Debug, Clone)]
pub struct FaceSpec {
    /// Underlying surface carrier.
    pub surface: SurfaceId,
    /// Whether the face normal agrees with the surface normal.
    pub sense: Sense,
    /// Optional display name.
    pub name: Option<String>,
    /// Optional display color.
    pub color: Option<Color>,
    /// Optional geometric tolerance in the document's length unit.
    pub tolerance: Option<f64>,
}

/// One coedge within a ring, supplied to [`TopologyBuilder::ring`].
///
/// The `next`, `previous`, and `radial_next` links are not fields here: the
/// builder derives `next`/`previous` from ring slice order and defaults
/// `radial_next` to the coedge itself until [`radial_ring`] links it.
///
/// [`radial_ring`]: TopologyBuilder::radial_ring
#[derive(Debug, Clone)]
pub struct CoedgeSpec {
    /// Arena id for this coedge.
    pub id: CoedgeId,
    /// Underlying edge this coedge traverses.
    pub edge: EdgeId,
    /// Direction relative to the edge curve.
    pub sense: Sense,
    /// Ordered parameter-space images of this coedge on the face surface.
    pub pcurves: Vec<PcurveUse>,
    /// Optional coedge-local 3D carrier used instead of the shared edge curve.
    pub use_curve: Option<CurveId>,
    /// Interval on the coedge-local 3D carrier in loop-traversal order.
    pub use_curve_parameter_range: Option<[f64; 2]>,
}

/// Per-edge attributes supplied to [`TopologyBuilder::edge`].
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeSpec {
    /// Underlying 3D curve carrier. `None` for a degenerate/tolerant edge.
    pub curve: Option<CurveId>,
    /// Start vertex.
    pub start: VertexId,
    /// End vertex.
    pub end: VertexId,
    /// Parameter range `[t_start, t_end]` on the curve's parameterization.
    pub param_range: Option<[f64; 2]>,
    /// Optional geometric tolerance in the document's length unit.
    pub tolerance: Option<f64>,
}

/// Stages a B-rep topology graph and appends it into a [`Model`] in one pass.
///
/// See the [module documentation](self) for the two design pillars and the
/// id-stability contract. Registration methods validate eagerly (id uniqueness,
/// owner existence); [`finish`](Self::finish) resolves deferred radial rings and
/// appends every arena in fixed layer order without sorting.
#[derive(Debug, Default)]
pub struct TopologyBuilder {
    bodies: Vec<Body>,
    regions: Vec<Region>,
    shells: Vec<Shell>,
    faces: Vec<Face>,
    loops: Vec<Loop>,
    coedges: Vec<Coedge>,
    edges: Vec<Edge>,
    vertices: Vec<Vertex>,
    points: Vec<Point>,

    /// Every registered id, for global uniqueness.
    ids: HashSet<String>,
    /// Locations of parent entities that aggregate child lists.
    body_index: HashMap<BodyId, usize>,
    region_index: HashMap<RegionId, usize>,
    shell_index: HashMap<ShellId, usize>,
    face_index: HashMap<FaceId, usize>,
    /// Location of each staged edge, for idempotent re-registration.
    edge_index: HashMap<EdgeId, usize>,
    /// Location of each staged coedge, for deferred radial wiring.
    coedge_index: HashMap<CoedgeId, usize>,
    /// Radial rings recorded by [`radial_ring`](Self::radial_ring), resolved at
    /// [`finish`](Self::finish).
    radial_rings: Vec<Vec<CoedgeId>>,
}

impl TopologyBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim `id` for global uniqueness, or fail with [`BuildError::DuplicateId`].
    fn claim(&mut self, id: &str) -> Result<(), BuildError> {
        if self.ids.insert(id.to_owned()) {
            Ok(())
        } else {
            Err(BuildError::DuplicateId(id.to_owned()))
        }
    }

    /// Register a body.
    pub fn body(&mut self, id: BodyId, spec: BodySpec) -> Result<(), BuildError> {
        self.claim(&id.0)?;
        let index = self.bodies.len();
        self.bodies.push(Body {
            id: id.clone(),
            kind: spec.kind,
            regions: Vec::new(),
            transform: spec.transform,
            name: spec.name,
            color: spec.color,
            visible: spec.visible,
        });
        self.body_index.insert(id, index);
        Ok(())
    }

    /// Register a region owned by a previously registered body.
    pub fn region(&mut self, id: RegionId, body: &BodyId) -> Result<(), BuildError> {
        let owner = *self
            .body_index
            .get(body)
            .ok_or_else(|| BuildError::UnknownOwner(body.0.clone()))?;
        self.claim(&id.0)?;
        self.bodies[owner].regions.push(id.clone());
        let index = self.regions.len();
        self.regions.push(Region {
            id: id.clone(),
            body: body.clone(),
            shells: Vec::new(),
        });
        self.region_index.insert(id, index);
        Ok(())
    }

    /// Register a shell owned by a previously registered region.
    pub fn shell(&mut self, id: ShellId, region: &RegionId) -> Result<(), BuildError> {
        let owner = *self
            .region_index
            .get(region)
            .ok_or_else(|| BuildError::UnknownOwner(region.0.clone()))?;
        self.claim(&id.0)?;
        self.regions[owner].shells.push(id.clone());
        let index = self.shells.len();
        self.shells.push(Shell {
            id: id.clone(),
            region: region.clone(),
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        self.shell_index.insert(id, index);
        Ok(())
    }

    /// Record a wire edge belonging directly to a shell.
    ///
    /// This aggregates a reference; the edge itself is registered through
    /// [`edge`](Self::edge). The reference is not required to resolve here.
    pub fn wire_edge(&mut self, shell: &ShellId, edge: EdgeId) -> Result<(), BuildError> {
        let owner = *self
            .shell_index
            .get(shell)
            .ok_or_else(|| BuildError::UnknownOwner(shell.0.clone()))?;
        self.shells[owner].wire_edges.push(edge);
        Ok(())
    }

    /// Record a free vertex belonging directly to a shell.
    ///
    /// This aggregates a reference; the vertex itself is registered through
    /// [`vertex`](Self::vertex). The reference is not required to resolve here.
    pub fn free_vertex(&mut self, shell: &ShellId, vertex: VertexId) -> Result<(), BuildError> {
        let owner = *self
            .shell_index
            .get(shell)
            .ok_or_else(|| BuildError::UnknownOwner(shell.0.clone()))?;
        self.shells[owner].free_vertices.push(vertex);
        Ok(())
    }

    /// Register a face owned by a previously registered shell.
    pub fn face(&mut self, id: FaceId, shell: &ShellId, spec: FaceSpec) -> Result<(), BuildError> {
        let owner = *self
            .shell_index
            .get(shell)
            .ok_or_else(|| BuildError::UnknownOwner(shell.0.clone()))?;
        self.claim(&id.0)?;
        self.shells[owner].faces.push(id.clone());
        let index = self.faces.len();
        self.faces.push(Face {
            id: id.clone(),
            shell: shell.clone(),
            surface: spec.surface,
            sense: spec.sense,
            loops: Vec::new(),
            name: spec.name,
            color: spec.color,
            tolerance: spec.tolerance,
        });
        self.face_index.insert(id, index);
        Ok(())
    }

    /// Register a loop (a closed ring of coedges, or a vertex-only pole loop)
    /// owned by a previously registered face.
    ///
    /// For a coedge ring, `next` and `previous` are wired cyclically in the
    /// order the coedges appear in `coedges`, and every `radial_next` defaults
    /// to the coedge itself (a laminar boundary); call
    /// [`radial_ring`](Self::radial_ring) to link non-laminar edges. A loop with
    /// no coedges and no vertex uses is rejected as [`BuildError::EmptyRing`]; a
    /// loop with only vertex uses is a valid pole loop.
    pub fn ring(
        &mut self,
        id: LoopId,
        face: &FaceId,
        role: LoopBoundaryRole,
        coedges: Vec<CoedgeSpec>,
        vertex_uses: Vec<VertexUse>,
    ) -> Result<(), BuildError> {
        let owner = *self
            .face_index
            .get(face)
            .ok_or_else(|| BuildError::UnknownOwner(face.0.clone()))?;
        if coedges.is_empty() && vertex_uses.is_empty() {
            return Err(BuildError::EmptyRing(id.0.clone()));
        }
        self.claim(&id.0)?;

        let count = coedges.len();
        let coedge_ids: Vec<CoedgeId> = coedges.iter().map(|spec| spec.id.clone()).collect();
        // Claim every coedge id before staging any, so a duplicate leaves no
        // half-registered ring behind.
        for coedge_id in &coedge_ids {
            self.claim(&coedge_id.0)?;
        }
        for (position, spec) in coedges.into_iter().enumerate() {
            let next = coedge_ids[(position + 1) % count].clone();
            let previous = coedge_ids[(position + count - 1) % count].clone();
            let coedge_index = self.coedges.len();
            self.coedges.push(Coedge {
                id: spec.id.clone(),
                owner_loop: id.clone(),
                edge: spec.edge,
                next,
                previous,
                radial_next: spec.id.clone(),
                sense: spec.sense,
                pcurves: spec.pcurves,
                use_curve: spec.use_curve,
                use_curve_parameter_range: spec.use_curve_parameter_range,
            });
            self.coedge_index.insert(spec.id, coedge_index);
        }

        self.faces[owner].loops.push(id.clone());
        self.loops.push(Loop {
            id,
            face: face.clone(),
            boundary_role: role,
            coedges: coedge_ids,
            vertex_uses,
        });
        Ok(())
    }

    /// Record an explicit radial ring of coedges around one shared edge.
    ///
    /// The coedges are linked through `radial_next` cyclically in slice order,
    /// overriding the laminar self-default set by [`ring`](Self::ring).
    /// Resolution is deferred to [`finish`](Self::finish), so this may be called
    /// before or after the rings that register the coedges. A ring of fewer than
    /// two coedges is a laminar no-op. Every coedge should appear in at most one
    /// radial ring; if a coedge appears in several, the last one wins.
    pub fn radial_ring(&mut self, coedges: &[CoedgeId]) {
        if coedges.len() >= 2 {
            self.radial_rings.push(coedges.to_vec());
        }
    }

    /// Register an edge, idempotently by id.
    ///
    /// A first registration stages the edge. A repeat registration with an
    /// identical [`EdgeSpec`] is a no-op; a repeat with a differing spec fails
    /// with [`BuildError::ConflictingRedefinition`]. Reusing an id already held
    /// by a non-edge entity fails with [`BuildError::DuplicateId`].
    pub fn edge(&mut self, id: EdgeId, spec: EdgeSpec) -> Result<(), BuildError> {
        let edge = Edge {
            id: id.clone(),
            curve: spec.curve,
            start: spec.start,
            end: spec.end,
            param_range: spec.param_range,
            tolerance: spec.tolerance,
        };
        if let Some(&existing) = self.edge_index.get(&id) {
            return if self.edges[existing] == edge {
                Ok(())
            } else {
                Err(BuildError::ConflictingRedefinition(id.0))
            };
        }
        self.claim(&id.0)?;
        let index = self.edges.len();
        self.edges.push(edge);
        self.edge_index.insert(id, index);
        Ok(())
    }

    /// Register a vertex referencing a position carrier.
    pub fn vertex(
        &mut self,
        id: VertexId,
        point: PointId,
        tolerance: Option<f64>,
    ) -> Result<(), BuildError> {
        self.claim(&id.0)?;
        self.vertices.push(Vertex {
            id,
            point,
            tolerance,
        });
        Ok(())
    }

    /// Register a position carrier.
    pub fn point(
        &mut self,
        id: PointId,
        position: Point3,
        source_object: Option<SourceObjectAssociation>,
    ) -> Result<(), BuildError> {
        self.claim(&id.0)?;
        self.points.push(Point {
            id,
            position,
            source_object,
        });
        Ok(())
    }

    /// Resolve deferred radial rings and append every staged arena into `model`.
    ///
    /// Arenas append in fixed layer order — bodies, regions, shells, faces,
    /// loops, coedges, edges, vertices, points — each in first-registration
    /// order. Nothing is sorted; canonical ordering remains
    /// [`Model::finalize`]'s responsibility. Radial rings are resolved first: a
    /// ring naming an unregistered coedge fails with
    /// [`BuildError::UnknownCoedge`], and a ring whose coedges do not all share
    /// one edge fails with [`BuildError::DanglingRadial`].
    pub fn finish(self, model: &mut Model) -> Result<(), BuildError> {
        let TopologyBuilder {
            bodies,
            regions,
            shells,
            faces,
            loops,
            mut coedges,
            edges,
            vertices,
            points,
            coedge_index,
            radial_rings,
            ..
        } = self;

        for ring in &radial_rings {
            let indices = ring
                .iter()
                .map(|coedge| {
                    coedge_index
                        .get(coedge)
                        .copied()
                        .ok_or_else(|| BuildError::UnknownCoedge(coedge.0.clone()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let shared = &coedges[indices[0]].edge;
            if let Some(&stray) = indices[1..]
                .iter()
                .find(|&&index| &coedges[index].edge != shared)
            {
                return Err(BuildError::DanglingRadial(format!(
                    "coedge {} traverses edge {} but the ring shares edge {}",
                    coedges[stray].id.0, coedges[stray].edge.0, shared.0
                )));
            }
            let count = ring.len();
            for (position, &index) in indices.iter().enumerate() {
                coedges[index].radial_next = ring[(position + 1) % count].clone();
            }
        }

        model.bodies.extend(bodies);
        model.regions.extend(regions);
        model.shells.extend(shells);
        model.faces.extend(faces);
        model.loops.extend(loops);
        model.coedges.extend(coedges);
        model.edges.extend(edges);
        model.vertices.extend(vertices);
        model.points.extend(points);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn face_spec() -> FaceSpec {
        FaceSpec {
            surface: SurfaceId("surface".into()),
            sense: Sense::Forward,
            name: None,
            color: None,
            tolerance: None,
        }
    }

    fn coedge_spec(id: &str, edge: &str) -> CoedgeSpec {
        CoedgeSpec {
            id: CoedgeId(id.into()),
            edge: EdgeId(edge.into()),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
            use_curve_parameter_range: None,
        }
    }

    fn edge_spec() -> EdgeSpec {
        EdgeSpec {
            curve: None,
            start: VertexId("v0".into()),
            end: VertexId("v1".into()),
            param_range: None,
            tolerance: None,
        }
    }

    /// Register `body`/`region`/`shell`/`face` so ring tests have an owner.
    fn scaffold() -> TopologyBuilder {
        let mut builder = TopologyBuilder::new();
        builder
            .body(BodyId("body".into()), BodySpec::default())
            .unwrap();
        builder
            .region(RegionId("region".into()), &BodyId("body".into()))
            .unwrap();
        builder
            .shell(ShellId("shell".into()), &RegionId("region".into()))
            .unwrap();
        builder
            .face(FaceId("face".into()), &ShellId("shell".into()), face_spec())
            .unwrap();
        builder
    }

    #[test]
    fn id_allocator_formats_ordinal_scheme() {
        let ids = IdAllocator::new("nx:brep");
        assert_eq!(ids.face(3), FaceId("nx:brep:face#3".into()));
        assert_eq!(ids.loop_(0), LoopId("nx:brep:loop#0".into()));
        assert_eq!(ids.coedge(12), CoedgeId("nx:brep:coedge#12".into()));
    }

    #[test]
    fn duplicate_id_is_rejected() {
        let mut builder = TopologyBuilder::new();
        builder
            .body(BodyId("x".into()), BodySpec::default())
            .unwrap();
        // A second entity of any arena reusing the id collides.
        assert_eq!(
            builder.point(PointId("x".into()), Point3::new(0.0, 0.0, 0.0), None),
            Err(BuildError::DuplicateId("x".into()))
        );
    }

    #[test]
    fn unknown_owner_is_rejected() {
        let mut builder = TopologyBuilder::new();
        assert_eq!(
            builder.region(RegionId("r".into()), &BodyId("absent".into())),
            Err(BuildError::UnknownOwner("absent".into()))
        );
        assert_eq!(
            builder.face(FaceId("f".into()), &ShellId("absent".into()), face_spec()),
            Err(BuildError::UnknownOwner("absent".into()))
        );
        assert_eq!(
            builder.ring(
                LoopId("l".into()),
                &FaceId("absent".into()),
                LoopBoundaryRole::Outer,
                vec![coedge_spec("c", "e")],
                Vec::new(),
            ),
            Err(BuildError::UnknownOwner("absent".into()))
        );
    }

    #[test]
    fn empty_ring_is_rejected() {
        let mut builder = scaffold();
        assert_eq!(
            builder.ring(
                LoopId("l".into()),
                &FaceId("face".into()),
                LoopBoundaryRole::Outer,
                Vec::new(),
                Vec::new(),
            ),
            Err(BuildError::EmptyRing("l".into()))
        );
    }

    #[test]
    fn vertex_only_pole_loop_is_accepted() {
        let mut builder = scaffold();
        builder
            .ring(
                LoopId("pole".into()),
                &FaceId("face".into()),
                LoopBoundaryRole::Outer,
                Vec::new(),
                vec![VertexUse {
                    vertex: VertexId("v".into()),
                    after: None,
                    pcurves: Vec::new(),
                }],
            )
            .unwrap();
    }

    #[test]
    fn edge_registration_is_idempotent() {
        let mut builder = TopologyBuilder::new();
        builder.edge(EdgeId("e".into()), edge_spec()).unwrap();
        // Identical re-registration is a no-op, not a duplicate.
        builder.edge(EdgeId("e".into()), edge_spec()).unwrap();

        let mut model = Model::default();
        builder.finish(&mut model).unwrap();
        assert_eq!(model.edges.len(), 1);
    }

    #[test]
    fn conflicting_edge_redefinition_is_rejected() {
        let mut builder = TopologyBuilder::new();
        builder.edge(EdgeId("e".into()), edge_spec()).unwrap();
        let mut other = edge_spec();
        other.tolerance = Some(1.0e-6);
        assert_eq!(
            builder.edge(EdgeId("e".into()), other),
            Err(BuildError::ConflictingRedefinition("e".into()))
        );
    }

    #[test]
    fn ring_wires_next_and_previous_cyclically() {
        let mut builder = scaffold();
        builder
            .ring(
                LoopId("l".into()),
                &FaceId("face".into()),
                LoopBoundaryRole::Outer,
                vec![
                    coedge_spec("c0", "e0"),
                    coedge_spec("c1", "e1"),
                    coedge_spec("c2", "e2"),
                ],
                Vec::new(),
            )
            .unwrap();
        let mut model = Model::default();
        builder.finish(&mut model).unwrap();

        // Registration order is preserved (no sort).
        let ids: Vec<&str> = model.coedges.iter().map(|c| c.id.0.as_str()).collect();
        assert_eq!(ids, ["c0", "c1", "c2"]);

        assert_eq!(model.coedges[0].next.0, "c1");
        assert_eq!(model.coedges[0].previous.0, "c2");
        assert_eq!(model.coedges[1].next.0, "c2");
        assert_eq!(model.coedges[1].previous.0, "c0");
        assert_eq!(model.coedges[2].next.0, "c0");
        assert_eq!(model.coedges[2].previous.0, "c1");
        // radial_next defaults to self (laminar).
        for coedge in &model.coedges {
            assert_eq!(coedge.radial_next, coedge.id);
        }
        // The loop aggregates its coedges in ring order.
        assert_eq!(
            model.loops[0].coedges,
            vec![
                CoedgeId("c0".into()),
                CoedgeId("c1".into()),
                CoedgeId("c2".into())
            ]
        );
        // The face aggregates its loop.
        assert_eq!(model.faces[0].loops, vec![LoopId("l".into())]);
    }

    #[test]
    fn radial_ring_links_shared_edge() {
        let mut builder = scaffold();
        builder
            .ring(
                LoopId("l0".into()),
                &FaceId("face".into()),
                LoopBoundaryRole::Outer,
                vec![coedge_spec("a", "shared")],
                Vec::new(),
            )
            .unwrap();
        builder
            .face(
                FaceId("face2".into()),
                &ShellId("shell".into()),
                face_spec(),
            )
            .unwrap();
        builder
            .ring(
                LoopId("l1".into()),
                &FaceId("face2".into()),
                LoopBoundaryRole::Outer,
                vec![coedge_spec("b", "shared")],
                Vec::new(),
            )
            .unwrap();
        builder.radial_ring(&[CoedgeId("a".into()), CoedgeId("b".into())]);

        let mut model = Model::default();
        builder.finish(&mut model).unwrap();
        let a = model.coedges.iter().find(|c| c.id.0 == "a").unwrap();
        let b = model.coedges.iter().find(|c| c.id.0 == "b").unwrap();
        assert_eq!(a.radial_next.0, "b");
        assert_eq!(b.radial_next.0, "a");
    }

    #[test]
    fn radial_ring_with_unregistered_coedge_is_rejected() {
        let mut builder = scaffold();
        builder
            .ring(
                LoopId("l".into()),
                &FaceId("face".into()),
                LoopBoundaryRole::Outer,
                vec![coedge_spec("a", "e")],
                Vec::new(),
            )
            .unwrap();
        builder.radial_ring(&[CoedgeId("a".into()), CoedgeId("ghost".into())]);

        let mut model = Model::default();
        assert_eq!(
            builder.finish(&mut model),
            Err(BuildError::UnknownCoedge("ghost".into()))
        );
    }

    #[test]
    fn radial_ring_across_distinct_edges_is_rejected() {
        let mut builder = scaffold();
        builder
            .ring(
                LoopId("l".into()),
                &FaceId("face".into()),
                LoopBoundaryRole::Outer,
                vec![coedge_spec("a", "edge_a"), coedge_spec("b", "edge_b")],
                Vec::new(),
            )
            .unwrap();
        builder.radial_ring(&[CoedgeId("a".into()), CoedgeId("b".into())]);

        let mut model = Model::default();
        assert!(matches!(
            builder.finish(&mut model),
            Err(BuildError::DanglingRadial(_))
        ));
    }

    #[test]
    fn interleaved_registration_preserves_first_registration_order() {
        let mut builder = TopologyBuilder::new();
        // Two bodies registered in non-lexicographic order, faces interleaved.
        builder
            .body(BodyId("b1".into()), BodySpec::default())
            .unwrap();
        builder
            .region(RegionId("r1".into()), &BodyId("b1".into()))
            .unwrap();
        builder
            .shell(ShellId("s1".into()), &RegionId("r1".into()))
            .unwrap();
        builder
            .face(FaceId("f_first".into()), &ShellId("s1".into()), face_spec())
            .unwrap();

        builder
            .body(BodyId("b0".into()), BodySpec::default())
            .unwrap();
        builder
            .region(RegionId("r0".into()), &BodyId("b0".into()))
            .unwrap();
        builder
            .shell(ShellId("s0".into()), &RegionId("r0".into()))
            .unwrap();
        builder
            .face(FaceId("f_zero".into()), &ShellId("s0".into()), face_spec())
            .unwrap();
        builder
            .face(FaceId("f_mid".into()), &ShellId("s1".into()), face_spec())
            .unwrap();

        let mut model = Model::default();
        builder.finish(&mut model).unwrap();

        // finish appends in first-registration order and never sorts.
        assert_eq!(
            model
                .bodies
                .iter()
                .map(|b| b.id.0.as_str())
                .collect::<Vec<_>>(),
            ["b1", "b0"]
        );
        assert_eq!(
            model
                .faces
                .iter()
                .map(|f| f.id.0.as_str())
                .collect::<Vec<_>>(),
            ["f_first", "f_zero", "f_mid"]
        );
    }

    /// Reconstruct [`crate::examples::unit_cube`]'s topology through the builder
    /// using the same id strings, and prove the two finalized models are equal.
    ///
    /// This freezes the emit semantics the codecs will adopt: given identical
    /// geometry carriers, the builder reproduces byte-identical topology. Only
    /// the topology is rebuilt here; surfaces and curves are geometry carriers a
    /// codec pushes directly (the builder owns no surface/curve arena), so they
    /// are copied verbatim to isolate the topology under test.
    #[test]
    fn reconstructs_unit_cube_topology() {
        use crate::document::CadIr;
        use crate::units::Units;

        let expected = crate::examples::unit_cube();

        let mut got = CadIr::empty(Units::default());
        got.model.surfaces = expected.model.surfaces.clone();
        got.model.curves = expected.model.curves.clone();

        let scale = 10.0_f64;
        let corners = [
            (0.0, 0.0, 0.0),
            (scale, 0.0, 0.0),
            (scale, scale, 0.0),
            (0.0, scale, 0.0),
            (0.0, 0.0, scale),
            (scale, 0.0, scale),
            (scale, scale, scale),
            (0.0, scale, scale),
        ];
        let edge_defs = [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ];
        // (face name, ring of (edge index, forward)).
        let face_defs: [(&str, [(usize, bool); 4]); 6] = [
            ("bottom", [(0, true), (1, true), (2, true), (3, true)]),
            ("top", [(7, false), (6, false), (5, false), (4, false)]),
            ("front", [(0, false), (8, true), (4, true), (9, false)]),
            ("right", [(1, false), (9, true), (5, true), (10, false)]),
            ("back", [(2, false), (10, true), (6, true), (11, false)]),
            ("left", [(3, false), (11, true), (7, true), (8, false)]),
        ];

        let mut builder = TopologyBuilder::new();

        let body = BodyId("synthetic:cube:body#0".into());
        builder
            .body(
                body.clone(),
                BodySpec {
                    kind: BodyKind::Solid,
                    transform: None,
                    name: Some("unit cube".into()),
                    color: None,
                    visible: None,
                },
            )
            .unwrap();
        let region = RegionId("synthetic:cube:region#0".into());
        builder.region(region.clone(), &body).unwrap();
        let shell = ShellId("synthetic:cube:shell#0".into());
        builder.shell(shell.clone(), &region).unwrap();

        // Faces, loops, and coedges; collect the two coedges of each edge.
        let mut edge_to_coedges: HashMap<usize, Vec<CoedgeId>> = HashMap::new();
        for (name, ring) in &face_defs {
            let face = FaceId(format!("synthetic:cube:face#{name}"));
            builder
                .face(
                    face.clone(),
                    &shell,
                    FaceSpec {
                        surface: SurfaceId(format!("synthetic:cube:surface#{name}")),
                        sense: Sense::Forward,
                        name: Some(format!("{name} face")),
                        color: None,
                        tolerance: None,
                    },
                )
                .unwrap();
            let coedges = ring
                .iter()
                .enumerate()
                .map(|(index, (edge_index, forward))| {
                    let id = CoedgeId(format!("synthetic:cube:coedge#{name}:{index}"));
                    edge_to_coedges
                        .entry(*edge_index)
                        .or_default()
                        .push(id.clone());
                    CoedgeSpec {
                        id,
                        edge: EdgeId(format!("synthetic:cube:edge#{edge_index}")),
                        sense: if *forward {
                            Sense::Forward
                        } else {
                            Sense::Reversed
                        },
                        pcurves: Vec::new(),
                        use_curve: None,
                        use_curve_parameter_range: None,
                    }
                })
                .collect();
            builder
                .ring(
                    LoopId(format!("synthetic:cube:loop#{name}")),
                    &face,
                    LoopBoundaryRole::Outer,
                    coedges,
                    Vec::new(),
                )
                .unwrap();
        }

        // Edges (the line curve carriers were copied above).
        for (index, (from, to)) in edge_defs.iter().enumerate() {
            let (ax, ay, az) = corners[*from];
            let (bx, by, bz) = corners[*to];
            let len = ((bx - ax).powi(2) + (by - ay).powi(2) + (bz - az).powi(2)).sqrt();
            builder
                .edge(
                    EdgeId(format!("synthetic:cube:edge#{index}")),
                    EdgeSpec {
                        curve: Some(CurveId(format!("synthetic:cube:curve#{index}"))),
                        start: VertexId(format!("synthetic:cube:vertex#{from}")),
                        end: VertexId(format!("synthetic:cube:vertex#{to}")),
                        param_range: Some([0.0, len]),
                        tolerance: None,
                    },
                )
                .unwrap();
        }

        // Points and vertices.
        for (index, (x, y, z)) in corners.iter().enumerate() {
            builder
                .point(
                    PointId(format!("synthetic:cube:point#{index}")),
                    Point3::new(*x, *y, *z),
                    None,
                )
                .unwrap();
            builder
                .vertex(
                    VertexId(format!("synthetic:cube:vertex#{index}")),
                    PointId(format!("synthetic:cube:point#{index}")),
                    None,
                )
                .unwrap();
        }

        // Radial rings: each edge is shared by exactly two coedges.
        for coedges in edge_to_coedges.values() {
            builder.radial_ring(coedges);
        }

        builder.finish(&mut got.model).unwrap();
        got.finalize();

        assert_eq!(got.model, expected.model);
    }
}
