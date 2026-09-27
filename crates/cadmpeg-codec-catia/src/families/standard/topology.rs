//! `StandardTopology` container and face-cycle orientation for standard
//! nested CATIA V5 B-rep streams.

use crate::families::standard::fbb::{
    boundary_cycles, classify_fbb_edge_layouts, cover_cycle, largest_fbb_run,
    parse_fbb_edge_tables, parse_trim_chain, parse_vertex_table,
};
use crate::families::standard::trim_packet::TrimPacket;
use crate::solve::matching::unique_coordinate_bijection;
use crate::solve::missing_edge::{
    standard_mesh_boundary_assignments, MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment,
};
use crate::solve::union_find::UnionFind;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::{features::NonEmptyMembers, topology::BodyKind};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Reconstructed standard-nested (or FBB-only) topology: the counted spine's
/// face boundaries recovered from the trim-mesh triangle packets, plus the
/// physical edge rows and, for the standard family, the `05 08 01` vertex
/// coordinate table ([spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#5-standard-nested-v5_cfv2-topology-spine)).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StandardTopology {
    pub(crate) faces: Vec<FaceTopology>,
    pub(crate) edge_rows: Vec<EdgeRow>,
    pub(crate) vertex_points: Vec<[f64; 3]>,
    pub(crate) logical_vertex_count: usize,
}

/// Classify admitted face partitions and require every physical edge to belong
/// to exactly one partition. Each component is closed when all its edges have
/// two uses; an isolated face has no closed component.
fn classify_body_groups(
    ctx: &DecodeContext<'_>,
    groups: &[impl AsRef<[FaceTopology]>],
    edge_count: usize,
) -> Result<Option<Vec<BodyKind>>, CodecError> {
    let mut seen_edges = HashSet::new();
    let mut kinds = Vec::new();
    for group in groups {
        let faces = group.as_ref();
        let mut union = UnionFind::charged(ctx, faces.len(), "catia_body_group_union")?;
        let mut uses = HashMap::<usize, (usize, usize)>::new();
        for (face, topology) in faces.iter().enumerate() {
            for coedge in topology
                .boundaries
                .iter()
                .flat_map(|boundary| &boundary.coedges)
            {
                if coedge.edge_row >= edge_count {
                    return Ok(None);
                }
                if let Some((first_face, count)) = uses.get_mut(&coedge.edge_row) {
                    union.union(face, *first_face);
                    *count += 1;
                } else {
                    crate::resource::insert_map(
                        ctx,
                        &mut uses,
                        coedge.edge_row,
                        (face, 1),
                        "catia_body_group_uses",
                    )?;
                }
            }
        }
        let mut components = HashSet::new();
        for face in 0..faces.len() {
            crate::resource::insert_set(
                ctx,
                &mut components,
                union.find(face),
                "catia_body_group_components",
            )?;
        }
        let mut paired_components = HashSet::new();
        let mut unpaired_components = HashSet::new();
        for (&edge, &(first_face, count)) in &uses {
            if !crate::resource::insert_set(
                ctx,
                &mut seen_edges,
                edge,
                "catia_body_group_seen_edges",
            )? {
                return Ok(None);
            }
            let component = union.find(first_face);
            if count == 2 {
                crate::resource::insert_set(
                    ctx,
                    &mut paired_components,
                    component,
                    "catia_body_group_paired",
                )?;
            } else {
                crate::resource::insert_set(
                    ctx,
                    &mut unpaired_components,
                    component,
                    "catia_body_group_unpaired",
                )?;
            }
        }
        let closed_count = paired_components.difference(&unpaired_components).count();
        crate::resource::push(
            ctx,
            &mut kinds,
            if uses.values().any(|(_, count)| *count > 2)
                || (closed_count != 0 && closed_count != components.len())
            {
                BodyKind::General
            } else if !components.is_empty() && closed_count == components.len() {
                BodyKind::Solid
            } else {
                BodyKind::Sheet
            },
            "catia_body_group_kinds",
        )?;
    }
    Ok((seen_edges.len() == edge_count).then_some(kinds))
}

impl StandardTopology {
    /// Number of faces, equal to the largest contiguous `30 04 04 ff` FBB
    /// run's row count ([spec §5.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#52-spine-grammar)).
    #[must_use]
    pub(super) fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// Per-face reconstructed boundaries, in FBB row order ([spec §5.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#51-positional-binding): face
    /// ordinal `i` binds to FBB row `i`).
    #[must_use]
    pub(crate) fn faces(&self) -> &[FaceTopology] {
        &self.faces
    }

    /// Face-index components connected through shared physical edge rows, in
    /// first-face order.
    pub(super) fn face_components(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<Vec<usize>>, CodecError> {
        let mut union = UnionFind::charged(ctx, self.faces.len(), "catia_face_component_union")?;
        let mut first_face_by_edge = HashMap::<usize, usize>::new();
        for (face, topology) in self.faces.iter().enumerate() {
            for edge in topology
                .boundaries
                .iter()
                .flat_map(|boundary| &boundary.coedges)
                .map(|coedge| coedge.edge_row)
            {
                if let Some(&other) = first_face_by_edge.get(&edge) {
                    union.union(face, other);
                } else {
                    crate::resource::insert_map(
                        ctx,
                        &mut first_face_by_edge,
                        edge,
                        face,
                        "catia_face_component_edges",
                    )?;
                }
            }
        }
        let mut labels = HashMap::<usize, usize>::new();
        let mut components = Vec::<Vec<usize>>::new();
        for face in 0..self.faces.len() {
            let root = union.find(face);
            let next = labels.len();
            if !labels.contains_key(&root) {
                crate::resource::insert_map(
                    ctx,
                    &mut labels,
                    root,
                    next,
                    "catia_face_component_labels",
                )?;
            }
            let component = labels[&root];
            if component == components.len() {
                crate::resource::push(ctx, &mut components, Vec::new(), "catia_face_components")?;
            }
            crate::resource::push(
                ctx,
                &mut components[component],
                face,
                "catia_face_component_members",
            )?;
        }
        Ok(components)
    }

    /// The counted spine's physical edge rows, in table order ([spec §5.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#52-spine-grammar)).
    #[must_use]
    pub(super) fn edge_rows(&self) -> &[EdgeRow] {
        &self.edge_rows
    }

    /// The `05 08 01` vertex coordinate table, in table order. Empty for a
    /// topology built by [`parse_fbb`], whose coordinate records are not
    /// part of the counted spine.
    #[must_use]
    pub(super) fn vertex_points(&self) -> &[[f64; 3]] {
        &self.vertex_points
    }

    /// Number of port/corner equivalence classes. Coordinate rows are a
    /// separate stored table and are not assigned to these classes here.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn logical_vertex_count(&self) -> usize {
        self.logical_vertex_count
    }

    /// Classify each consecutive FBB face group from physical-edge incidence.
    /// An edge cannot belong to faces in two different groups.
    pub(super) fn body_kinds(
        &self,
        ctx: &DecodeContext<'_>,
        face_groups: &[usize],
    ) -> Result<Option<Vec<BodyKind>>, CodecError> {
        let mut remaining = self.faces.as_slice();
        let mut groups = Vec::new();
        for &count in face_groups {
            let Some((group, rest)) = remaining.split_at_checked(count) else {
                return Ok(None);
            };
            crate::resource::push(ctx, &mut groups, group, "catia_body_group_slices")?;
            remaining = rest;
        }
        if !remaining.is_empty() {
            return Ok(None);
        }
        classify_body_groups(ctx, &groups, self.edge_rows.len())
    }

    /// Orient every incidence-closed FBB face group independently. Open sheet
    /// and non-manifold general groups retain their reconstructed loop senses.
    pub(super) fn orient_solid_body_cycles(
        &mut self,
        ctx: &DecodeContext<'_>,
        face_groups: &[usize],
    ) -> Result<Option<()>, CodecError> {
        let mut remaining = self.faces.as_mut_slice();
        let mut groups = Vec::new();
        for &count in face_groups {
            let Some((group, rest)) = remaining.split_at_mut_checked(count) else {
                return Ok(None);
            };
            crate::resource::push(ctx, &mut groups, group, "catia_body_group_slices")?;
            remaining = rest;
        }
        if !remaining.is_empty() {
            return Ok(None);
        }
        let Some(kinds) = classify_body_groups(ctx, &groups, self.edge_rows.len())? else {
            return Ok(None);
        };
        for (group, kind) in groups.into_iter().zip(kinds) {
            if kind == BodyKind::Solid && orient_face_cycles(ctx, group)?.is_none() {
                return Ok(None);
            }
        }
        Ok(Some(()))
    }

    /// Bind logical port/corner components to coordinate-row indices from one
    /// exact unordered endpoint pair per physical edge. A result is returned
    /// only when the induced bijection is unique.
    pub(super) fn bind_vertex_points(
        &self,
        ctx: &DecodeContext<'_>,
        edge_point_pairs: &[[usize; 2]],
    ) -> Result<Option<Vec<usize>>, CodecError> {
        if edge_point_pairs.len() != self.edge_rows.len()
            || self.logical_vertex_count != self.vertex_points.len()
        {
            return Ok(None);
        }
        let Some(edge_vertices) = self.edge_vertices(ctx)? else {
            return Ok(None);
        };
        let mut domains = ctx.alloc_filled(
            self.logical_vertex_count,
            HashSet::new(),
            "catia standard vertex point domains",
        )?;
        for domain in &mut domains {
            crate::resource::reserve_set(
                ctx,
                domain,
                self.vertex_points.len(),
                "catia standard vertex point domain entries",
            )?;
            domain.extend(0..self.vertex_points.len());
        }
        for (edge, pair) in edge_vertices.into_iter().zip(edge_point_pairs) {
            if pair[0] >= self.vertex_points.len() || pair[1] >= self.vertex_points.len() {
                return Ok(None);
            }
            let [start, end] = edge;
            let candidates = HashSet::from(*pair);
            domains[start].retain(|point| candidates.contains(point));
            domains[end].retain(|point| candidates.contains(point));
        }
        if domains.iter().any(HashSet::is_empty) {
            return Ok(None);
        }

        unique_coordinate_bijection(ctx, &domains, &self.vertex_points)
    }

    /// Logical endpoint components in physical edge-row direction.
    pub(crate) fn edge_vertices(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
        let mut edge_vertices =
            ctx.alloc_filled(self.edge_rows.len(), None, "catia standard edge vertices")?;
        for face in &self.faces {
            for boundary in &face.boundaries {
                for coedge in &boundary.coedges {
                    let endpoints = if coedge.reversed {
                        [coedge.end_vertex, coedge.start_vertex]
                    } else {
                        [coedge.start_vertex, coedge.end_vertex]
                    };
                    match edge_vertices[coedge.edge_row] {
                        Some(previous) if previous != endpoints => return Ok(None),
                        Some(_) => {}
                        None => edge_vertices[coedge.edge_row] = Some(endpoints),
                    }
                }
            }
        }

        Ok(edge_vertices.into_iter().collect())
    }

    /// Replace provisional trim-handle endpoint components with the quotient
    /// induced by one native endpoint-identity pair per physical edge.
    ///
    /// Native identities are global within the parsed topology. Equal values
    /// collapse face-local corners even when adjacent faces use different trim
    /// handles. The pair order is the physical edge-row direction.
    #[must_use]
    fn with_native_edge_vertices(&self, edge_ports: &[[u32; 2]]) -> Option<Self> {
        if edge_ports.len() != self.edge_rows.len() {
            return None;
        }
        let mut identities = HashMap::new();
        let mut edge_vertices = Vec::with_capacity(edge_ports.len());
        for ports in edge_ports {
            let pair = ports.map(|identity| {
                let next = identities.len();
                *identities.entry(identity).or_insert(next)
            });
            edge_vertices.push(pair);
        }
        let mut topology = self.clone();
        for face in &mut topology.faces {
            for boundary in &mut face.boundaries {
                for coedge in &mut boundary.coedges {
                    let [start, end] = edge_vertices[coedge.edge_row];
                    [coedge.start_vertex, coedge.end_vertex] = if coedge.reversed {
                        [end, start]
                    } else {
                        [start, end]
                    };
                }
            }
        }
        topology.logical_vertex_count = identities.len();
        Some(topology)
    }
}

/// The boundary meaning of an edge-row handle sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum EdgeBoundaryLayout {
    /// The first and last handles are endpoint ports in the global trim-handle
    /// namespace; the handles between them match the boundary.
    InteriorWithFlankingCorners,
    /// Every handle belongs to the trim boundary, including both endpoints.
    CompleteBoundaryRun,
}

/// One row of a counted standard/FBB edge table, with handles read big-endian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EdgeRow {
    /// Table-kind byte the row was parsed under (`0x01` or `0x02`; spec
    /// §5.2 `count_header`).
    pub(crate) kind: u8,
    /// The row's BE handle sequence.
    pub(crate) handles: Vec<u32>,
    /// How the handle sequence maps onto a trim boundary.
    pub(crate) boundary_layout: EdgeBoundaryLayout,
}

impl EdgeRow {
    pub(crate) fn boundary_pattern(&self) -> Option<&[u32]> {
        match self.boundary_layout {
            EdgeBoundaryLayout::InteriorWithFlankingCorners => {
                self.handles.get(1..self.handles.len().checked_sub(1)?)
            }
            EdgeBoundaryLayout::CompleteBoundaryRun => Some(self.handles.as_slice()),
        }
        .filter(|pattern| !pattern.is_empty())
    }

    pub(crate) fn boundary_span(
        &self,
        pattern_start: usize,
        cycle_len: usize,
    ) -> Option<(usize, usize)> {
        match self.boundary_layout {
            EdgeBoundaryLayout::InteriorWithFlankingCorners => Some((
                (pattern_start + cycle_len.checked_sub(1)?) % cycle_len,
                self.handles.len().checked_sub(1)?,
            )),
            EdgeBoundaryLayout::CompleteBoundaryRun => {
                Some((pattern_start, self.handles.len().checked_sub(1)?))
            }
        }
    }
}

/// One face's reconstructed boundary cycles ([spec §5.3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#53-trim-records-indexed-triangle-mesh-packets)): one outer cycle
/// plus one per hole, in the order recovered from the trim mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FaceTopology {
    /// The face's boundary cycles; loop count equals boundary-cycle count.
    pub(crate) boundaries: Vec<Boundary>,
}

/// One closed boundary cycle of a face's trim mesh, covered end-to-end by
/// matched edge rows ([spec §5.3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#53-trim-records-indexed-triangle-mesh-packets)–[§5.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#54-physical-edge-identity-and-portvertex-collapse)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Boundary {
    /// The physical edge uses covering this cycle, in cycle order.
    pub(crate) coedges: NonEmptyCoedges,
}

/// A face boundary cycle admitted with at least one matched coedge use.
pub(crate) type NonEmptyCoedges = NonEmptyMembers<CoedgeUse>;

impl Boundary {
    /// Admit one reconstructed cycle after its source matching has completed.
    pub(crate) fn new(coedges: Vec<CoedgeUse>) -> Option<Self> {
        Some(Self {
            coedges: NonEmptyMembers::try_from(coedges).ok()?,
        })
    }
}

/// One physical edge's use within a face boundary, oriented by its match
/// against the recovered boundary cycle ([spec §5.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#54-physical-edge-identity-and-portvertex-collapse)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CoedgeUse {
    /// Index into [`StandardTopology::edge_rows`] for the matched edge
    /// row.
    pub(crate) edge_row: usize,
    /// `true` when the edge row's handle sequence matched the boundary
    /// cycle in reverse; orientation comes from this match, not a stored
    /// sense bit ([spec §5.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#54-physical-edge-identity-and-portvertex-collapse)).
    pub(crate) reversed: bool,
    /// Logical-vertex (union-find component) index at this coedge's start,
    /// in boundary-cycle traversal direction.
    pub(crate) start_vertex: usize,
    /// Logical-vertex (union-find component) index at this coedge's end,
    /// in boundary-cycle traversal direction.
    pub(crate) end_vertex: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TrimRecord {
    pub(crate) packet: TrimPacket,
    pub(super) frame_vector: Option<FiniteVector<3>>,
    pub(crate) kind: u8,
}

pub(crate) fn reconstruct_incidence(
    ctx: &DecodeContext<'_>,
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<[f64; 3]>,
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    face_count: usize,
) -> Result<Option<StandardTopology>, CodecError> {
    reconstruct_incidence_with_edge_classes(
        ctx,
        edge_rows,
        vertex_points,
        edge_faces,
        edge_points,
        face_count,
        None,
    )
}

fn reconstruct_incidence_with_edge_classes(
    ctx: &DecodeContext<'_>,
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<[f64; 3]>,
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    face_count: usize,
    edge_classes: Option<&[usize]>,
) -> Result<Option<StandardTopology>, CodecError> {
    reconstruct_incidence_with_edge_classes_and_mesh(
        ctx,
        edge_rows,
        vertex_points,
        edge_faces,
        edge_points,
        face_count,
        StandardIncidenceEvidence {
            edge_classes,
            mesh_bytes: None,
        },
    )
}

#[derive(Clone, Copy)]
pub(super) struct StandardIncidenceEvidence<'a> {
    pub(super) edge_classes: Option<&'a [usize]>,
    pub(super) mesh_bytes: Option<&'a [u8]>,
}

pub(super) fn reconstruct_incidence_with_edge_classes_and_mesh(
    ctx: &DecodeContext<'_>,
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<[f64; 3]>,
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    face_count: usize,
    evidence: StandardIncidenceEvidence<'_>,
) -> Result<Option<StandardTopology>, CodecError> {
    let StandardIncidenceEvidence {
        edge_classes,
        mesh_bytes,
    } = evidence;
    let Some(completed_edge_faces) = complete_duplicate_face_slots(
        ctx,
        &edge_rows,
        edge_faces,
        edge_points,
        face_count,
        edge_classes,
        mesh_bytes,
    )?
    else {
        return Ok(None);
    };
    let edge_faces = completed_edge_faces.as_slice();
    let mut face_edges = ctx.alloc_filled(face_count, Vec::new(), "catia standard face edges")?;
    for (edge, &[left, right]) in edge_faces.iter().enumerate() {
        let Some(face) = face_edges.get_mut(left) else {
            return Ok(None);
        };
        crate::resource::push(ctx, face, edge, "catia_standard_face_edge_entries")?;
        if right != left {
            let Some(face) = face_edges.get_mut(right) else {
                return Ok(None);
            };
            crate::resource::push(ctx, face, edge, "catia_standard_face_edge_entries")?;
        }
    }
    let mut faces = Vec::with_capacity(face_count);
    for incident in face_edges {
        let Some(cycles) = incidence_cycles(&incident, edge_points) else {
            return Ok(None);
        };
        faces.push(FaceTopology {
            boundaries: cycles
                .into_iter()
                .map(|cycle| Boundary {
                    coedges: cycle.map(|(edge_row, reversed)| {
                        let [stored_start, stored_end] = edge_points[edge_row];
                        let [start_vertex, end_vertex] = if reversed {
                            [stored_end, stored_start]
                        } else {
                            [stored_start, stored_end]
                        };
                        CoedgeUse {
                            edge_row,
                            reversed,
                            start_vertex,
                            end_vertex,
                        }
                    }),
                })
                .collect(),
        });
    }
    if orient_face_cycles(ctx, &mut faces)?.is_none() {
        return Ok(None);
    }
    Ok(Some(StandardTopology {
        faces,
        edge_rows,
        logical_vertex_count: vertex_points.len(),
        vertex_points,
    }))
}

pub(super) fn complete_duplicate_face_slots(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    face_count: usize,
    edge_classes: Option<&[usize]>,
    mesh_bytes: Option<&[u8]>,
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    const MAX_DUPLICATE_FACE_OPERATIONS: usize = 65_536;

    struct SearchInputs<'a, 'ctx> {
        ctx: &'a DecodeContext<'ctx>,
        unresolved: &'a [usize],
        edge_rows: &'a [EdgeRow],
        edge_faces: &'a [[usize; 2]],
        edge_points: &'a [[usize; 2]],
        edge_classes: Option<&'a [usize]>,
        mesh_bytes: Option<&'a [u8]>,
    }

    fn search(
        inputs: &SearchInputs<'_, '_>,
        degrees: &mut [BTreeMap<usize, u8>],
        assignment: &mut [usize],
        used: &mut [bool],
        solutions: &mut Vec<Vec<usize>>,
        operations: &mut usize,
        exhausted: &mut bool,
    ) -> Result<(), CodecError> {
        if *exhausted || solutions.len() > 1 {
            return Ok(());
        }
        if used.iter().all(|value| *value) {
            let closed = degrees
                .iter()
                .all(|face| face.values().all(|degree| *degree == 2));
            let mesh_valid = if !closed {
                false
            } else if let Some(bytes) = inputs.mesh_bytes {
                let mut completed = inputs.edge_faces.to_vec();
                for (&edge, &face) in inputs.unresolved.iter().zip(assignment.iter()) {
                    completed[edge][1] = face;
                }
                standard_mesh_boundary_assignments(inputs.ctx, bytes, &completed, None)?.is_some()
            } else {
                true
            };
            if mesh_valid {
                let distinct = if let Some(existing) = solutions.first() {
                    !duplicate_face_assignments_equivalent(
                        inputs.ctx,
                        inputs.unresolved,
                        inputs.edge_rows,
                        inputs.edge_faces,
                        inputs.edge_points,
                        inputs.edge_classes,
                        [existing, assignment],
                    )?
                } else {
                    true
                };
                if distinct {
                    solutions.push(assignment.to_vec());
                }
            }
            return Ok(());
        }
        let deficit = degrees.iter().enumerate().find_map(|(face, values)| {
            values
                .iter()
                .find_map(|(&point, &degree)| (degree == 1).then_some((face, point)))
        });
        let unresolved = inputs
            .unresolved
            .iter()
            .enumerate()
            .filter(|(index, _)| !used[*index]);
        let candidates: Box<dyn Iterator<Item = (usize, &usize)>> = match deficit {
            Some((_, point)) => Box::new(unresolved.filter(move |(_, edge)| {
                let [start, end] = inputs.edge_points[**edge];
                start == point || end == point
            })),
            None => Box::new(unresolved.take(1)),
        };
        let choices = candidates
            .flat_map(|(index, &edge)| {
                let faces: Box<dyn Iterator<Item = usize>> = match deficit {
                    Some((face, _)) => Box::new(std::iter::once(face)),
                    None => Box::new(0..degrees.len()),
                };
                faces.map(move |face| (index, edge, face))
            })
            .filter(|(_, edge, face)| {
                let [start, end] = inputs.edge_points[*edge];
                degrees[*face].get(&start).copied().unwrap_or_default() + 1 + u8::from(start == end)
                    <= 2
                    && (start == end || degrees[*face].get(&end).copied().unwrap_or_default() < 2)
            })
            .collect::<Vec<_>>();
        for (index, edge, face) in choices {
            if *operations == MAX_DUPLICATE_FACE_OPERATIONS {
                *exhausted = true;
                return Ok(());
            }
            *operations += 1;
            let [start, end] = inputs.edge_points[edge];
            let start_add = 1 + u8::from(start == end);
            let start_degree_before = degrees[face].get(&start).copied();
            let end_degree_before = if start == end {
                None
            } else {
                degrees[face].get(&end).copied()
            };
            *degrees[face].entry(start).or_default() += start_add;
            if start != end {
                *degrees[face].entry(end).or_default() += 1;
            }
            assignment[index] = face;
            used[index] = true;
            search(
                inputs, degrees, assignment, used, solutions, operations, exhausted,
            )?;
            used[index] = false;
            match start_degree_before {
                Some(degree) => {
                    degrees[face].insert(start, degree);
                }
                None => {
                    degrees[face].remove(&start);
                }
            }
            if start != end {
                match end_degree_before {
                    Some(degree) => {
                        degrees[face].insert(end, degree);
                    }
                    None => {
                        degrees[face].remove(&end);
                    }
                }
            }
            if *exhausted || solutions.len() > 1 {
                return Ok(());
            }
        }
        Ok(())
    }

    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_points.len()
        || edge_faces.iter().flatten().any(|face| *face >= face_count)
    {
        return Ok(None);
    }

    let mut completed = edge_faces.to_vec();
    let mut unresolved = edge_faces
        .iter()
        .enumerate()
        .filter_map(|(edge, faces)| (faces[0] == faces[1]).then_some(edge))
        .collect::<Vec<_>>();
    if unresolved.is_empty() {
        return Ok(Some(completed));
    }
    let mut degrees = ctx.alloc_filled(
        face_count,
        BTreeMap::<usize, u8>::new(),
        "catia standard endpoint degrees",
    )?;
    for (edge, faces) in edge_faces.iter().enumerate() {
        let mut incident = *faces;
        incident.sort_unstable();
        for &face in if incident[0] == incident[1] {
            &incident[..1]
        } else {
            &incident[..]
        } {
            for &point in &edge_points[edge] {
                let degree = degrees[face].entry(point).or_default();
                let Some(next) = degree.checked_add(1) else {
                    return Ok(None);
                };
                *degree = next;
            }
        }
    }
    if degrees
        .iter()
        .flat_map(BTreeMap::values)
        .any(|degree| *degree > 2)
    {
        return Ok(None);
    }
    unresolved.sort_by_key(|&edge| {
        let [start, end] = edge_points[edge];
        degrees
            .iter()
            .filter(|face| {
                face.get(&start).copied().unwrap_or_default() + 1 + u8::from(start == end) <= 2
                    && (start == end || face.get(&end).copied().unwrap_or_default() < 2)
            })
            .count()
    });

    let mut solutions = Vec::new();
    let mut operations = 0;
    let mut exhausted = false;
    let inputs = SearchInputs {
        ctx,
        unresolved: &unresolved,
        edge_rows,
        edge_faces,
        edge_points,
        edge_classes,
        mesh_bytes: None,
    };
    let mut assignment = ctx.alloc_filled(
        unresolved.len(),
        0,
        "catia standard unresolved edge assignment",
    )?;
    let mut assigned = ctx.alloc_filled(
        unresolved.len(),
        false,
        "catia standard unresolved edge marks",
    )?;
    search(
        &inputs,
        &mut degrees,
        &mut assignment,
        &mut assigned,
        &mut solutions,
        &mut operations,
        &mut exhausted,
    )?;
    if exhausted {
        return Ok(None);
    }
    if solutions.len() > 1 {
        let Some(bytes) = mesh_bytes else {
            return Ok(None);
        };
        solutions.clear();
        let inputs = SearchInputs {
            ctx,
            unresolved: &unresolved,
            edge_rows,
            edge_faces,
            edge_points,
            edge_classes,
            mesh_bytes: Some(bytes),
        };
        let mut assignment = ctx.alloc_filled(
            unresolved.len(),
            0,
            "catia standard unresolved edge assignment",
        )?;
        let mut assigned = ctx.alloc_filled(
            unresolved.len(),
            false,
            "catia standard unresolved edge marks",
        )?;
        search(
            &inputs,
            &mut degrees,
            &mut assignment,
            &mut assigned,
            &mut solutions,
            &mut operations,
            &mut exhausted,
        )?;
        if exhausted {
            return Ok(None);
        }
    }
    let [assignment] = solutions.as_slice() else {
        return Ok(None);
    };
    for (&edge, &face) in unresolved.iter().zip(assignment) {
        completed[edge][1] = face;
    }
    Ok(Some(completed))
}

fn duplicate_face_assignments_equivalent(
    ctx: &DecodeContext<'_>,
    unresolved: &[usize],
    edge_rows: &[EdgeRow],
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    edge_classes: Option<&[usize]>,
    assignments: [&[usize]; 2],
) -> Result<bool, CodecError> {
    let [left, right] = assignments;
    let mut classified = ctx.alloc_filled(
        unresolved.len(),
        false,
        "catia standard duplicate assignment marks",
    )?;
    for first in 0..unresolved.len() {
        if classified[first] {
            continue;
        }
        let first_edge = unresolved[first];
        let mut left_faces = Vec::new();
        let mut right_faces = Vec::new();
        for (index, &edge) in unresolved.iter().enumerate() {
            let same_row = edge_classes.is_some_and(|classes| classes[first_edge] == classes[edge])
                || edge_rows[first_edge].kind == edge_rows[edge].kind
                    && edge_rows[first_edge].boundary_layout == edge_rows[edge].boundary_layout
                    && (edge_rows[first_edge].handles == edge_rows[edge].handles
                        || edge_rows[first_edge]
                            .handles
                            .iter()
                            .eq(edge_rows[edge].handles.iter().rev()));
            let mut first_points = edge_points[first_edge];
            let mut points = edge_points[edge];
            first_points.sort_unstable();
            points.sort_unstable();
            if same_row
                && first_points == points
                && edge_faces[first_edge][0] == edge_faces[edge][0]
            {
                classified[index] = true;
                left_faces.push(left[index]);
                right_faces.push(right[index]);
            }
        }
        left_faces.sort_unstable();
        right_faces.sort_unstable();
        if left_faces != right_faces {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn orient_face_cycles(
    ctx: &DecodeContext<'_>,
    faces: &mut [FaceTopology],
) -> Result<Option<()>, CodecError> {
    let boundaries = faces
        .iter_mut()
        .flat_map(|face| &mut face.boundaries)
        .collect::<Vec<_>>();
    let mut edge_uses = HashMap::<usize, Vec<(usize, bool)>>::new();
    for (node, boundary) in boundaries.iter().enumerate() {
        for coedge in &boundary.coedges {
            edge_uses
                .entry(coedge.edge_row)
                .or_default()
                .push((node, coedge.reversed));
        }
    }
    let Some(flips) =
        solve_boundary_orientation_constraints(ctx, boundaries.len(), &edge_uses, true)?
    else {
        return Ok(None);
    };
    for (boundary, flip) in boundaries.into_iter().zip(flips) {
        if flip {
            boundary.coedges.reverse();
            for coedge in &mut boundary.coedges {
                coedge.reversed = !coedge.reversed;
                std::mem::swap(&mut coedge.start_vertex, &mut coedge.end_vertex);
            }
        }
    }
    Ok(Some(()))
}

pub(crate) fn solve_boundary_orientation_constraints(
    ctx: &DecodeContext<'_>,
    boundary_count: usize,
    edge_uses: &HashMap<usize, Vec<(usize, bool)>>,
    require_paired_uses: bool,
) -> Result<Option<Vec<bool>>, CodecError> {
    let mut constraints = ctx.alloc_filled(
        boundary_count,
        Vec::<(usize, bool)>::new(),
        "catia standard boundary constraints",
    )?;
    for uses in edge_uses.values() {
        let [(left_node, left_reversed), (right_node, right_reversed)] = uses.as_slice() else {
            if !require_paired_uses && uses.len() == 1 {
                continue;
            }
            return Ok(None);
        };
        if *left_node >= boundary_count || *right_node >= boundary_count {
            return Ok(None);
        }
        let parity = left_reversed == right_reversed;
        if left_node == right_node {
            if parity {
                return Ok(None);
            }
        } else {
            constraints[*left_node].push((*right_node, parity));
            constraints[*right_node].push((*left_node, parity));
        }
    }

    let mut flips = ctx.alloc_filled(boundary_count, None, "catia standard boundary flips")?;
    let mut result = Vec::new();
    for root in 0..boundary_count {
        if let Some(flip) = flips[root] {
            result.push(flip);
            continue;
        }
        flips[root] = Some(false);
        let mut stack = vec![(root, false)];
        while let Some((face, flip)) = stack.pop() {
            for &(neighbor, parity) in &constraints[face] {
                let required = flip ^ parity;
                match flips[neighbor] {
                    Some(existing) if existing != required => return Ok(None),
                    Some(_) => {}
                    None => {
                        flips[neighbor] = Some(required);
                        stack.push((neighbor, required));
                    }
                }
            }
        }
        result.push(false);
    }
    Ok(Some(result))
}

pub(crate) fn incidence_cycles(
    incident: &[usize],
    edge_points: &[[usize; 2]],
) -> Option<Vec<NonEmptyMembers<(usize, bool)>>> {
    #[derive(Clone, Copy)]
    struct AdjacentEdge {
        edge: usize,
        vertex: usize,
        reversed: bool,
    }

    if incident.is_empty() {
        return None;
    }
    let mut vertex_indices = HashMap::<usize, usize>::new();
    let mut at_vertex = Vec::<Vec<AdjacentEdge>>::new();
    let mut unseen = BTreeMap::new();
    let mut seen_edges = HashSet::new();
    let mut cycles = Vec::new();
    for &edge in incident {
        if !seen_edges.insert(edge) {
            return None;
        }
        let [start, end] = *edge_points.get(edge)?;
        if start == end {
            cycles.push(NonEmptyMembers::one((edge, false)));
            continue;
        }
        let [start, end] = [start, end].map(|vertex| {
            *vertex_indices.entry(vertex).or_insert_with(|| {
                let index = at_vertex.len();
                at_vertex.push(Vec::new());
                index
            })
        });
        at_vertex[start].push(AdjacentEdge {
            edge,
            vertex: end,
            reversed: false,
        });
        at_vertex[end].push(AdjacentEdge {
            edge,
            vertex: start,
            reversed: true,
        });
        unseen.insert(edge, [start, end]);
    }
    let at_vertex = at_vertex
        .into_iter()
        .map(|edges| <[AdjacentEdge; 2]>::try_from(edges).ok())
        .collect::<Option<Vec<_>>>()?;
    while let Some((first, [start_vertex, mut vertex])) = unseen.pop_first() {
        let mut cycle = NonEmptyMembers::one((first, false));
        let mut previous_edge = first;
        while vertex != start_vertex {
            // Every vertex has two distinct incident edges. The edge other
            // than the one just traversed continues this closed component.
            let [left, right] = at_vertex[vertex];
            let next = if left.edge == previous_edge {
                right
            } else {
                left
            };
            unseen.remove(&next.edge);
            vertex = next.vertex;
            previous_edge = next.edge;
            cycle.push((next.edge, next.reversed));
        }
        cycles.push(cycle);
    }
    Some(cycles)
}

/// Parses the FBB-only spine. Its edge rows and trim handles use one selected
/// big-endian width; the
/// following counted `05 08 01` table supplies vertex coordinates.
pub(crate) fn parse_fbb(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<StandardTopology>, CodecError> {
    let Some(face_run) = largest_fbb_run(bytes) else {
        return Ok(None);
    };
    let face_start = face_run.face_start();
    let face_count = face_run.face_count();
    let after_faces = face_run.after_faces();
    let Some((mut edge_rows, _, vertex_header, handle_width)) =
        parse_fbb_edge_tables(bytes, after_faces)
    else {
        return Ok(None);
    };
    let Some(vertex_points) = parse_vertex_table(bytes, vertex_header) else {
        return Ok(None);
    };
    let Some(trims) = parse_trim_chain(bytes, face_start, face_count, handle_width) else {
        return Ok(None);
    };
    if classify_fbb_edge_layouts(ctx, &mut edge_rows, &trims)?.is_none() {
        return Ok(None);
    }
    reconstruct(ctx, edge_rows, vertex_points, &trims)
}

/// Parse an FBB-only spine and apply its global native endpoint identities.
/// This closes the cross-face quotient independently of face-local trim-handle
/// names.
pub(super) fn parse_fbb_with_native_vertices(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_ports: &[[u32; 2]],
) -> Result<Option<StandardTopology>, CodecError> {
    Ok(parse_fbb(ctx, bytes)?.and_then(|topology| topology.with_native_edge_vertices(edge_ports)))
}

pub(super) fn reconstruct(
    ctx: &DecodeContext<'_>,
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<[f64; 3]>,
    trims: &[TrimRecord],
) -> Result<Option<StandardTopology>, CodecError> {
    let node_count = edge_rows
        .len()
        .checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit("catia_reconstruct_union", u64::MAX, u64::MAX))?;
    let mut union = UnionFind::charged(ctx, node_count, "catia_reconstruct_union")?;
    let mut faces = Vec::new();
    for trim in trims {
        let Some(cycles) = boundary_cycles(ctx, trim.packet.triangles())? else {
            return Ok(None);
        };
        let mut boundaries = Vec::new();
        for cycle in cycles {
            let Some(boundary) = cover_cycle(ctx, &cycle, &edge_rows, &mut union)? else {
                return Ok(None);
            };
            crate::resource::push(
                ctx,
                &mut boundaries,
                boundary,
                "catia_reconstruct_boundaries",
            )?;
        }
        crate::resource::push(
            ctx,
            &mut faces,
            FaceTopology { boundaries },
            "catia_reconstruct_faces",
        )?;
    }

    let mut roots = HashMap::new();
    for node in 0..union.len() {
        let root = union.find(node);
        let next = roots.len();
        if !roots.contains_key(&root) {
            crate::resource::insert_map(ctx, &mut roots, root, next, "catia_reconstruct_roots")?;
        }
    }
    for face in &mut faces {
        for boundary in &mut face.boundaries {
            for coedge in &mut boundary.coedges {
                coedge.start_vertex = roots[&union.find(coedge.start_vertex)];
                coedge.end_vertex = roots[&union.find(coedge.end_vertex)];
            }
        }
    }

    Ok(Some(StandardTopology {
        faces,
        edge_rows,
        vertex_points,
        logical_vertex_count: roots.len(),
    }))
}

pub(crate) fn reconstruct_mesh_selection(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    selected: &[impl std::borrow::Borrow<MeshFaceBoundaryAssignment>],
    unmatched_reversed: &[Vec<Vec<bool>>],
) -> Result<Option<StandardTopology>, CodecError> {
    if selected.len() != unmatched_reversed.len() {
        return Ok(None);
    }
    let node_count = edge_rows
        .len()
        .checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit("catia_mesh_selection_union", u64::MAX, u64::MAX))?;
    let mut union = UnionFind::charged(ctx, node_count, "catia_mesh_selection_union")?;
    let mut faces = Vec::new();
    for (face, directions) in selected.iter().zip(unmatched_reversed) {
        let face = std::borrow::Borrow::borrow(face);
        if face.boundaries.len() != directions.len() {
            return Ok(None);
        }
        let mut boundaries = Vec::new();
        for (uses, directions) in face.boundaries.iter().zip(directions) {
            if uses.len() != directions.len() {
                return Ok(None);
            }
            let mut paired_uses = uses.iter().zip(directions).enumerate();
            let Some((first_index, (first_use, &first_reversed))) = paired_uses.next() else {
                return Ok(None);
            };
            let mut corners = Vec::new();
            for _ in 0..uses.len() {
                let corner = union.push_charged(ctx, "catia_mesh_selection_corner_nodes")?;
                crate::resource::push(ctx, &mut corners, corner, "catia_mesh_selection_corners")?;
            }
            let mut admit_coedge = |use_index: usize,
                                    use_: &MeshBoundaryEdgeCandidate,
                                    unmatched_reversed: bool|
             -> Option<CoedgeUse> {
                let reversed = use_.reversed.unwrap_or(unmatched_reversed);
                if use_.reversed.is_some() && unmatched_reversed != reversed {
                    return None;
                }
                let start_vertex = corners[use_index];
                let end_vertex = corners[(use_index + 1) % corners.len()];
                let edge_start = use_.edge.checked_mul(2)?;
                let edge_end = edge_start.checked_add(1)?;
                if edge_end >= node_count {
                    return None;
                }
                if reversed {
                    union.union(edge_end, start_vertex);
                    union.union(edge_start, end_vertex);
                } else {
                    union.union(edge_start, start_vertex);
                    union.union(edge_end, end_vertex);
                }
                Some(CoedgeUse {
                    edge_row: use_.edge,
                    reversed,
                    start_vertex,
                    end_vertex,
                })
            };
            let Some(first) = admit_coedge(first_index, first_use, first_reversed) else {
                return Ok(None);
            };
            let mut coedges = Vec::new();
            crate::resource::push(ctx, &mut coedges, first, "catia_mesh_selection_coedges")?;
            for (use_index, (use_, &unmatched_reversed)) in paired_uses {
                let Some(coedge) = admit_coedge(use_index, use_, unmatched_reversed) else {
                    return Ok(None);
                };
                crate::resource::push(ctx, &mut coedges, coedge, "catia_mesh_selection_coedges")?;
            }
            let Ok(coedges) = NonEmptyCoedges::try_from(coedges) else {
                return Ok(None);
            };
            crate::resource::push(
                ctx,
                &mut boundaries,
                Boundary { coedges },
                "catia_mesh_selection_boundaries",
            )?;
        }
        crate::resource::push(
            ctx,
            &mut faces,
            FaceTopology { boundaries },
            "catia_mesh_selection_faces",
        )?;
    }
    let mut roots = HashMap::new();
    for node in 0..union.len() {
        let root = union.find(node);
        let next = roots.len();
        if !roots.contains_key(&root) {
            crate::resource::insert_map(ctx, &mut roots, root, next, "catia_mesh_selection_roots")?;
        }
    }
    for face in &mut faces {
        for boundary in &mut face.boundaries {
            for coedge in &mut boundary.coedges {
                coedge.start_vertex = roots[&union.find(coedge.start_vertex)];
                coedge.end_vertex = roots[&union.find(coedge.end_vertex)];
            }
        }
    }
    let mut owned_rows = Vec::new();
    crate::resource::reserve_vec(
        ctx,
        &mut owned_rows,
        edge_rows.len(),
        "catia_mesh_selection_edge_copy",
    )?;
    for row in edge_rows {
        owned_rows.push(EdgeRow {
            kind: row.kind,
            handles: crate::resource::copy_slice(
                ctx,
                &row.handles,
                "catia_mesh_selection_handle_copy",
            )?,
            boundary_layout: row.boundary_layout,
        });
    }
    Ok(Some(StandardTopology {
        faces,
        edge_rows: owned_rows,
        logical_vertex_count: roots.len(),
        vertex_points: crate::resource::copy_slice(
            ctx,
            vertex_points,
            "catia_mesh_selection_point_copy",
        )?,
    }))
}

#[cfg(test)]
mod tests;
