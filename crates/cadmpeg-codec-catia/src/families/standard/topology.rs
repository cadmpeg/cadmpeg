//! `StandardTopologyDraft` container and face-cycle orientation for standard
//! nested CATIA V5 B-rep streams.

use cadmpeg_core::decode::u64_from_index;

pub(crate) mod admitted;

use crate::families::standard::fbb::{
    boundary_cycles, classify_fbb_edge_layouts, cover_cycle, largest_fbb_run,
    parse_fbb_edge_tables, parse_trim_chain, parse_vertex_table, row_pattern_ends,
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

/// Each face row lists `(point, degree)` entries in increasing point order.
fn duplicate_degree_slot(
    ctx: &DecodeContext<'_>,
    row: &[(usize, u8)],
    point: usize,
) -> Result<Result<usize, usize>, CodecError> {
    ctx.binary_search_by_key(
        row,
        &point,
        |(key, _)| Ok(*key),
        "catia_standard_endpoint_degree_lookup",
    )
}

fn duplicate_degree(
    ctx: &DecodeContext<'_>,
    row: &[(usize, u8)],
    point: usize,
) -> Result<Option<u8>, CodecError> {
    Ok(duplicate_degree_slot(ctx, row, point)?
        .ok()
        .map(|index| row[index].1))
}

fn set_duplicate_degree(
    ctx: &DecodeContext<'_>,
    row: &mut Vec<(usize, u8)>,
    point: usize,
    degree: u8,
) -> Result<(), CodecError> {
    match duplicate_degree_slot(ctx, row, point)? {
        Ok(index) => row[index].1 = degree,
        Err(index) => ctx.insert_vec(
            row,
            index,
            (point, degree),
            "catia_standard_endpoint_degree_entries",
        )?,
    }
    Ok(())
}

fn restore_duplicate_degree(
    ctx: &DecodeContext<'_>,
    row: &mut Vec<(usize, u8)>,
    point: usize,
    before: Option<u8>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "catia_standard_endpoint_degree_restore";
    if let Ok(index) = duplicate_degree_slot(ctx, row, point)? {
        if let Some(degree) = before {
            row[index].1 = degree;
        } else {
            ctx.rotate_left(&mut row[index..], 1, OPERATION)?;
            ctx.truncate_vec(row, row.len() - 1, OPERATION)?;
        }
    }
    Ok(())
}

/// Whether one more use of the edge `[start, end]` keeps every endpoint of
/// the face at degree two or less.
fn duplicate_face_admits_edge(
    ctx: &DecodeContext<'_>,
    row: &[(usize, u8)],
    [start, end]: [usize; 2],
) -> Result<bool, CodecError> {
    Ok(
        duplicate_degree(ctx, row, start)?.unwrap_or_default() + 1 + u8::from(start == end) <= 2
            && (start == end || duplicate_degree(ctx, row, end)?.unwrap_or_default() < 2),
    )
}

/// Mutable standard-nested (or FBB-only) topology hypothesis: the counted spine's
/// face boundaries recovered from the trim-mesh triangle packets, plus the
/// physical edge rows and, for the standard family, the `05 08 01` vertex
/// coordinate table ([spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#5-standard-nested-v5_cfv2-topology-spine)).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StandardTopologyDraft {
    pub(crate) faces: Vec<FaceTopologyDraft>,
    pub(crate) edge_rows: Vec<EdgeRow>,
    pub(crate) vertex_points: Vec<[f64; 3]>,
    pub(crate) logical_vertex_count: usize,
}

/// Classify admitted face partitions and require every physical edge to belong
/// to exactly one partition. Each component is closed when all its edges have
/// two uses; an isolated face has no closed component.
fn classify_body_groups(
    ctx: &DecodeContext<'_>,
    groups: &[impl AsRef<[FaceTopologyDraft]>],
    edge_count: usize,
) -> Result<Option<Vec<BodyKind>>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_body_group_scratch")?;
    let mut seen_edges = HashSet::new();
    let mut kinds = Vec::new();
    for group in ctx.admit_iter(groups, "catia_body_group_kinds")? {
        let faces = group.as_ref();
        let kind = scratch.with_storage(|| -> Result<Option<BodyKind>, CodecError> {
            let mut union = UnionFind::charged(ctx, faces.len(), "catia_body_group_union")?;
            let mut uses = BTreeMap::<usize, (usize, usize)>::new();
            for (face, topology) in ctx.admit_iter(faces, "catia_body_group_uses")?.enumerate() {
                for boundary in ctx.admit_iter(&topology.boundaries, "catia_body_group_uses")? {
                    for coedge in
                        ctx.admit_iter(boundary.coedges.as_slice(), "catia_body_group_uses")?
                    {
                        if coedge.edge_row >= edge_count {
                            return Ok(None);
                        }
                        if let Some((first_face, count)) = ctx.get_mut_btree_map(
                            &mut uses,
                            &coedge.edge_row,
                            "catia_body_group_uses",
                        )? {
                            union.union(ctx, face, *first_face)?;
                            *count += 1;
                        } else {
                            ctx.insert_btree_map(
                                &mut uses,
                                coedge.edge_row,
                                (face, 1),
                                "catia_body_group_uses",
                            )?;
                        }
                    }
                }
            }
            // Component marks are indexed by each component's root face.
            let mut is_root =
                ctx.alloc_filled(faces.len(), false, "catia_body_group_components")?;
            let mut component_count = 0usize;
            for face in ctx.admit_iter(0..faces.len(), "catia_body_group_component_faces")? {
                if !std::mem::replace(&mut is_root[union.find(ctx, face)?], true) {
                    component_count += 1;
                }
            }
            let mut paired = ctx.alloc_filled(faces.len(), false, "catia_body_group_paired")?;
            let mut unpaired = ctx.alloc_filled(faces.len(), false, "catia_body_group_unpaired")?;
            let mut nonmanifold = false;
            for (&edge, &(first_face, count)) in
                ctx.admit_iter(&uses, "catia_body_group_seen_edges")?
            {
                if !ctx.insert_hash_set(&mut seen_edges, edge, "catia_body_group_seen_edges")? {
                    return Ok(None);
                }
                let component = union.find(ctx, first_face)?;
                if count == 2 {
                    paired[component] = true;
                } else {
                    unpaired[component] = true;
                }
                nonmanifold |= count > 2;
            }
            let closed_count = ctx
                .admit_iter(&paired, "catia_body_group_closed_components")?
                .zip(&unpaired)
                .filter(|(paired, unpaired)| **paired && !**unpaired)
                .count();
            Ok(Some(
                if nonmanifold || (closed_count != 0 && closed_count != component_count) {
                    BodyKind::General
                } else if component_count != 0 && closed_count == component_count {
                    BodyKind::Solid
                } else {
                    BodyKind::Sheet
                },
            ))
        })?;
        let Some(kind) = kind else {
            return Ok(None);
        };
        ctx.push_vec(&mut kinds, kind, "catia_body_group_kinds")?;
    }
    Ok((seen_edges.len() == edge_count).then_some(kinds))
}

impl StandardTopologyDraft {
    pub(crate) fn clone_charged(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let mut faces = Vec::new();
        for face in ctx.admit_iter(&self.faces, "catia_standard_topology_copy_faces")? {
            let mut boundaries = Vec::new();
            for boundary in
                ctx.admit_iter(&face.boundaries, "catia_standard_topology_copy_boundaries")?
            {
                let coedges = ctx.copy_slice(
                    boundary.coedges.as_slice(),
                    "catia_standard_topology_copy_coedges",
                )?;
                let coedges = NonEmptyMembers::<CoedgeUse>::try_from(coedges)
                    .map_err(CodecError::malformed)?;
                ctx.push_vec(
                    &mut boundaries,
                    BoundaryDraft { coedges },
                    "catia_standard_topology_copy_boundaries",
                )?;
            }
            ctx.push_vec(
                &mut faces,
                FaceTopologyDraft { boundaries },
                "catia_standard_topology_copy_faces",
            )?;
        }
        let mut edge_rows = Vec::new();
        for row in ctx.admit_iter(&self.edge_rows, "catia_standard_topology_copy_edge_rows")? {
            ctx.push_vec(
                &mut edge_rows,
                row.clone_charged(ctx)?,
                "catia_standard_topology_copy_edge_rows",
            )?;
        }
        Ok(Self {
            faces,
            edge_rows,
            vertex_points: ctx.copy_slice(
                &self.vertex_points,
                "catia_standard_topology_copy_vertex_points",
            )?,
            logical_vertex_count: self.logical_vertex_count,
        })
    }

    /// Number of faces, equal to the largest contiguous `30 04 04 ff` FBB
    /// run's row count ([spec §5.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#52-spine-grammar)).
    #[must_use]
    pub(super) fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// Face-index components connected through shared physical edge rows, in
    /// first-face order.
    pub(super) fn face_components(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<Vec<usize>>, CodecError> {
        let mut union = UnionFind::charged(ctx, self.faces.len(), "catia_face_component_union")?;
        let mut storage = ctx.reserve_scoped(0, "catia_face_component_edges")?;
        let mut first_face_by_edge = HashMap::<usize, usize>::new();
        for (face, topology) in ctx
            .admit_iter(&self.faces, "catia_face_component_edges")?
            .enumerate()
        {
            for boundary in ctx.admit_iter(&topology.boundaries, "catia_face_component_edges")? {
                for coedge in
                    ctx.admit_iter(boundary.coedges.as_slice(), "catia_face_component_edges")?
                {
                    let edge = coedge.edge_row;
                    if let Some(&other) =
                        ctx.get_hash_map(&first_face_by_edge, &edge, "catia_face_component_edges")?
                    {
                        union.union(ctx, face, other)?;
                    } else {
                        storage.with_storage(|| {
                            ctx.insert_hash_map(
                                &mut first_face_by_edge,
                                edge,
                                face,
                                "catia_face_component_edges",
                            )
                        })?;
                    }
                }
            }
        }
        let mut labels = HashMap::<usize, usize>::new();
        let mut components = Vec::<Vec<usize>>::new();
        for (face, _) in ctx
            .admit_iter(&self.faces, "catia_face_component_face_visits")?
            .enumerate()
        {
            let root = union.find(ctx, face)?;
            let component = match ctx.get_hash_map(&labels, &root, "catia_face_component_labels")? {
                Some(&component) => component,
                None => {
                    let next = labels.len();
                    storage.with_storage(|| {
                        ctx.insert_hash_map(&mut labels, root, next, "catia_face_component_labels")
                    })?;
                    next
                }
            };
            if component == components.len() {
                ctx.push_vec(&mut components, Vec::new(), "catia_face_components")?;
            }
            ctx.push_vec(
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
        for &count in ctx.admit_iter(face_groups, "catia_body_group_slices")? {
            let Some((group, rest)) = remaining.split_at_checked(count) else {
                return Ok(None);
            };
            ctx.push_vec(&mut groups, group, "catia_body_group_slices")?;
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
        for &count in ctx.admit_iter(face_groups, "catia_body_group_slices")? {
            let Some((group, rest)) = remaining.split_at_mut_checked(count) else {
                return Ok(None);
            };
            ctx.push_vec(&mut groups, group, "catia_body_group_slices")?;
            remaining = rest;
        }
        if !remaining.is_empty() {
            return Ok(None);
        }
        let Some(kinds) = classify_body_groups(ctx, &groups, self.edge_rows.len())? else {
            return Ok(None);
        };
        for (index, &kind) in ctx
            .admit_iter(&kinds, "catia_standard_body_orientation_kinds")?
            .enumerate()
        {
            if kind == BodyKind::Solid && orient_face_cycles(ctx, groups[index])?.is_none() {
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
        // A vertex's domain is the intersection of the endpoint pairs of its
        // edges; a vertex on no edge may take any point.
        let point_count = self.vertex_points.len();
        let mut storage = ctx.reserve_scoped(0, "catia standard vertex point domains")?;
        let mut constrained = storage.with_storage(|| {
            ctx.alloc_filled(
                self.logical_vertex_count,
                None::<[Option<usize>; 2]>,
                "catia standard vertex point domains",
            )
        })?;
        for (edge, pair) in ctx
            .admit_iter(&edge_vertices, "catia standard vertex point domains")?
            .copied()
            .zip(edge_point_pairs)
        {
            if pair[0] >= point_count || pair[1] >= point_count {
                return Ok(None);
            }
            for vertex in edge {
                let domain = &mut constrained[vertex];
                *domain = Some(match *domain {
                    None => [Some(pair[0]), (pair[1] != pair[0]).then_some(pair[1])],
                    Some(points) => points.map(|point| point.filter(|point| pair.contains(point))),
                });
            }
        }
        if ctx.any_by(
            &constrained,
            |domain| Ok(domain.is_some_and(|points| points.iter().all(Option::is_none))),
            "catia_standard_vertex_domain_scan",
        )? {
            return Ok(None);
        }
        let domains = storage.with_storage(|| {
            ctx.try_collect_vec(
                ctx.admit_iter(&constrained, "catia standard vertex point domain entries")?
                    .map(|domain| -> Result<HashSet<usize>, CodecError> {
                        let mut points = HashSet::new();
                        match domain {
                            Some(constrained) => {
                                for &point in constrained.iter().flatten() {
                                    ctx.insert_hash_set(
                                        &mut points,
                                        point,
                                        "catia standard vertex point domain entries",
                                    )?;
                                }
                            }
                            None => {
                                for point in ctx.admit_iter(
                                    0..point_count,
                                    "catia standard vertex point domain entries",
                                )? {
                                    ctx.insert_hash_set(
                                        &mut points,
                                        point,
                                        "catia standard vertex point domain entries",
                                    )?;
                                }
                            }
                        }
                        Ok(points)
                    }),
                "catia standard vertex point domains",
            )
        })?;
        unique_coordinate_bijection(ctx, &domains, &self.vertex_points)
    }

    /// Logical endpoint components in physical edge-row direction.
    pub(crate) fn edge_vertices(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
        let mut edge_vertices =
            ctx.alloc_filled(self.edge_rows.len(), None, "catia standard edge vertices")?;
        for face in ctx.admit_iter(&self.faces, "catia standard edge vertices")? {
            for boundary in ctx.admit_iter(&face.boundaries, "catia standard edge vertices")? {
                for coedge in
                    ctx.admit_iter(boundary.coedges.as_slice(), "catia standard edge vertices")?
                {
                    let endpoints = if coedge.reversed {
                        [coedge.end_vertex, coedge.start_vertex]
                    } else {
                        [coedge.start_vertex, coedge.end_vertex]
                    };
                    let Some(slot) = edge_vertices.get_mut(coedge.edge_row) else {
                        return Ok(None);
                    };
                    match *slot {
                        Some(previous) if previous != endpoints => return Ok(None),
                        Some(_) => {}
                        None => *slot = Some(endpoints),
                    }
                }
            }
        }

        let mut complete = Vec::new();
        for vertices in ctx
            .admit_iter(&edge_vertices, "catia_standard_edge_vertices_complete")?
            .copied()
        {
            let Some(vertices) = vertices else {
                return Ok(None);
            };
            ctx.push_vec(
                &mut complete,
                vertices,
                "catia_standard_edge_vertices_complete",
            )?;
        }
        Ok(Some(complete))
    }

    /// Replace provisional trim-handle endpoint components with the quotient
    /// induced by one native endpoint-identity pair per physical edge.
    ///
    /// Native identities are global within the parsed topology. Equal values
    /// collapse face-local corners even when adjacent faces use different trim
    /// handles. The pair order is the physical edge-row direction.
    fn with_native_edge_vertices(
        &self,
        ctx: &DecodeContext<'_>,
        edge_ports: &[[u32; 2]],
    ) -> Result<Option<Self>, CodecError> {
        if edge_ports.len() != self.edge_rows.len() {
            return Ok(None);
        }
        const IDENTITIES: &str = "catia_standard_native_vertex_identities";
        let mut storage = ctx.reserve_scoped(0, IDENTITIES)?;
        let mut identities = HashMap::new();
        let mut edge_vertices = Vec::new();
        for ports in ctx.admit_iter(edge_ports, IDENTITIES)? {
            let mut pair = [0; 2];
            for (port, identity) in ports.iter().copied().enumerate() {
                pair[port] = match ctx.get_hash_map(&identities, &identity, IDENTITIES)? {
                    Some(&vertex) => vertex,
                    None => {
                        let next = identities.len();
                        storage.with_storage(|| {
                            ctx.insert_hash_map(&mut identities, identity, next, IDENTITIES)
                        })?;
                        next
                    }
                };
            }
            ctx.push_scoped_vec(
                &mut storage,
                &mut edge_vertices,
                pair,
                "catia_standard_native_edge_vertices",
            )?;
        }
        let mut topology = self.clone_charged(ctx)?;
        for (face_index, source_face) in ctx
            .admit_iter(&self.faces, "catia_standard_native_vertex_rewrite_faces")?
            .enumerate()
        {
            for (boundary_index, source_boundary) in ctx
                .admit_iter(
                    &source_face.boundaries,
                    "catia_standard_native_vertex_rewrite_boundaries",
                )?
                .enumerate()
            {
                for (coedge_index, _) in ctx
                    .admit_iter(
                        source_boundary.coedges.as_slice(),
                        "catia_standard_native_vertex_rewrite_coedges",
                    )?
                    .enumerate()
                {
                    let coedge = &mut topology.faces[face_index].boundaries[boundary_index].coedges
                        [coedge_index];
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
        Ok(Some(topology))
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

impl cadmpeg_core::decode::cost::DecodeCost for EdgeBoundaryLayout {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

impl From<EdgeBoundaryLayout> for u64 {
    fn from(layout: EdgeBoundaryLayout) -> Self {
        match layout {
            EdgeBoundaryLayout::InteriorWithFlankingCorners => 0,
            EdgeBoundaryLayout::CompleteBoundaryRun => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeTableKind {
    First,
    Second,
}

impl EdgeTableKind {
    fn byte(self) -> u8 {
        match self {
            Self::First => 1,
            Self::Second => 2,
        }
    }
}

/// One row of a counted standard/FBB edge table, with handles read big-endian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EdgeRow {
    /// Table-kind byte the row was parsed under (`0x01` or `0x02`; spec
    /// §5.2 `count_header`).
    kind: EdgeTableKind,
    /// The row's BE handle sequence.
    handles: Vec<u32>,
    /// How the handle sequence maps onto a trim boundary.
    boundary_layout: EdgeBoundaryLayout,
}

impl EdgeRow {
    pub(crate) fn new(
        kind: u8,
        handles: Vec<u32>,
        boundary_layout: EdgeBoundaryLayout,
    ) -> Option<Self> {
        let kind = match kind {
            1 => EdgeTableKind::First,
            2 => EdgeTableKind::Second,
            _ => return None,
        };
        let minimum = match boundary_layout {
            EdgeBoundaryLayout::CompleteBoundaryRun => 2,
            EdgeBoundaryLayout::InteriorWithFlankingCorners => 3,
        };
        if handles.len() < minimum {
            return None;
        }
        Some(Self {
            kind,
            handles,
            boundary_layout,
        })
    }

    pub(crate) fn kind(&self) -> u8 {
        self.kind.byte()
    }
    pub(crate) fn handles(&self) -> &[u32] {
        &self.handles
    }
    pub(crate) fn boundary_layout(&self) -> EdgeBoundaryLayout {
        self.boundary_layout
    }

    pub(crate) fn select_flanking_corners(&mut self) -> bool {
        if self.handles.len() < 3 {
            return false;
        }
        self.boundary_layout = EdgeBoundaryLayout::InteriorWithFlankingCorners;
        true
    }

    pub(crate) fn normalize_handles(&mut self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        ctx.fill(&mut self.handles, 0, "catia_mesh_gauge_normalize_handles")
    }

    pub(crate) fn clone_charged(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            kind: self.kind,
            handles: ctx.copy_slice(&self.handles, "catia_standard_edge_row_copy_handles")?,
            boundary_layout: self.boundary_layout,
        })
    }

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
pub(crate) struct FaceTopologyDraft {
    /// The face's boundary cycles; loop count equals boundary-cycle count.
    pub(crate) boundaries: Vec<BoundaryDraft>,
}

/// One provisional boundary of a face's trim mesh, covered by
/// matched edge rows ([spec §5.3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#53-trim-records-indexed-triangle-mesh-packets)–[§5.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#54-physical-edge-identity-and-portvertex-collapse)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundaryDraft {
    /// The physical edge uses covering this cycle, in cycle order.
    pub(crate) coedges: NonEmptyMembers<CoedgeUse>,
}

impl BoundaryDraft {
    /// Retain a nonempty provisional cycle while endpoint classes are resolved.
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
    /// Index into [`StandardTopologyDraft::edge_rows`] for the matched edge
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

impl cadmpeg_core::decode::cost::DecodeCost for CoedgeUse {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TrimRecord {
    pub(crate) packet: TrimPacket,
    pub(super) frame_vector: Option<FiniteVector<3>>,
    pub(crate) kind: u8,
}

/// A comparison reads the packet, the optional frame vector and the kind.
impl cadmpeg_core::decode::cost::DecodeCost for TrimRecord {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        self.packet
            .decode_cost(ctx, operation)?
            .checked_add(u64_from_index(
                std::mem::size_of::<Option<FiniteVector<3>>>() + std::mem::size_of::<u8>(),
            ))
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))
    }
}

pub(crate) fn reconstruct_incidence(
    ctx: &DecodeContext<'_>,
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<[f64; 3]>,
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    face_count: usize,
) -> Result<Option<StandardTopologyDraft>, CodecError> {
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
) -> Result<Option<StandardTopologyDraft>, CodecError> {
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
) -> Result<Option<StandardTopologyDraft>, CodecError> {
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
    let mut face_edges =
        ctx.collect_indexed_vec(face_count, "catia standard face edges", |_| Ok(Vec::new()))?;
    for (edge, &[left, right]) in ctx
        .admit_iter(edge_faces, "catia_standard_face_edge_entries")?
        .enumerate()
    {
        let Some(face) = face_edges.get_mut(left) else {
            return Ok(None);
        };
        ctx.push_vec(face, edge, "catia_standard_face_edge_entries")?;
        if right != left {
            let Some(face) = face_edges.get_mut(right) else {
                return Ok(None);
            };
            ctx.push_vec(face, edge, "catia_standard_face_edge_entries")?;
        }
    }
    let mut faces = Vec::new();
    for incident in ctx.admit_iter(&face_edges, "catia_standard_incidence_faces")? {
        let Some(cycles) = incidence_cycles(ctx, incident, edge_points)? else {
            return Ok(None);
        };
        let mut boundaries = Vec::new();
        for cycle in ctx.admit_iter(cycles, "catia_standard_incidence_boundaries")? {
            let mut coedges = Vec::new();
            for (edge_row, reversed) in ctx
                .admit_iter(cycle.as_slice(), "catia_standard_incidence_cycle_edges")?
                .copied()
            {
                let [stored_start, stored_end] = edge_points[edge_row];
                let [start_vertex, end_vertex] = if reversed {
                    [stored_end, stored_start]
                } else {
                    [stored_start, stored_end]
                };
                ctx.push_vec(
                    &mut coedges,
                    CoedgeUse {
                        edge_row,
                        reversed,
                        start_vertex,
                        end_vertex,
                    },
                    "catia_standard_incidence_coedges",
                )?;
            }
            let coedges =
                NonEmptyMembers::<CoedgeUse>::try_from(coedges).map_err(CodecError::malformed)?;
            ctx.push_vec(
                &mut boundaries,
                BoundaryDraft { coedges },
                "catia_standard_incidence_boundaries",
            )?;
        }
        ctx.push_vec(
            &mut faces,
            FaceTopologyDraft { boundaries },
            "catia_standard_incidence_faces",
        )?;
    }
    if orient_face_cycles(ctx, &mut faces)?.is_none() {
        return Ok(None);
    }
    Ok(Some(StandardTopologyDraft {
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
        degrees: &mut [Vec<(usize, u8)>],
        assignment: &mut [usize],
        used: &mut [bool],
        solutions: &mut Vec<Vec<usize>>,
        operations: &mut usize,
        exhausted: &mut bool,
    ) -> Result<(), CodecError> {
        let _depth = inputs
            .ctx
            .enter_nested("catia_standard_duplicate_face_search")?;
        if *exhausted || solutions.len() > 1 {
            return Ok(());
        }
        let ctx = inputs.ctx;
        if ctx.all_by(
            &*used,
            |value| Ok(*value),
            "catia_standard_duplicate_used_scan",
        )? {
            let closed = ctx.all_by(
                &*degrees,
                |face| {
                    ctx.all_by(
                        face,
                        |(_, degree)| Ok(*degree == 2),
                        "catia_standard_duplicate_degree_scan",
                    )
                },
                "catia_standard_duplicate_face_degree_scan",
            )?;
            let mesh_valid = if !closed {
                false
            } else if let Some(bytes) = inputs.mesh_bytes {
                let mut completed = inputs.ctx.copy_slice(
                    inputs.edge_faces,
                    "catia_standard_duplicate_mesh_edge_faces",
                )?;
                for (&edge, &face) in inputs
                    .ctx
                    .admit_iter(
                        inputs.unresolved,
                        "catia_standard_duplicate_unresolved_scan",
                    )?
                    .zip(
                        inputs
                            .ctx
                            .admit_iter(&*assignment, "catia_standard_duplicate_assignment_scan")?,
                    )
                {
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
                    let copy = inputs
                        .ctx
                        .copy_slice(assignment, "catia_standard_duplicate_solution_values")?;
                    inputs
                        .ctx
                        .push_vec(solutions, copy, "catia_standard_duplicate_solutions")?;
                }
            }
            return Ok(());
        }
        let deficit = ctx.find_map(
            degrees.iter().enumerate(),
            |(face, values)| {
                Ok(ctx
                    .find_map(
                        values,
                        |&(point, degree)| Ok((degree == 1).then_some(point)),
                        "catia_standard_duplicate_deficit_degrees",
                    )?
                    .map(|point| (face, point)))
            },
            "catia_standard_duplicate_deficit_faces",
        )?;
        let mut choices = Vec::new();
        let mut unresolved = inputs.unresolved.iter().enumerate();
        while let Some((index, &edge)) =
            ctx.next_charged(&mut unresolved, "catia_standard_duplicate_choice_scan")?
        {
            if used[index] {
                continue;
            }
            let [start, end] = inputs.edge_points[edge];
            if let Some((_, point)) = deficit {
                if start != point && end != point {
                    continue;
                }
            }
            let faces = deficit.map_or(&degrees[..], |(face, _)| &degrees[face..face + 1]);
            for (domain_index, row) in ctx
                .admit_iter(faces, "catia_standard_duplicate_choice_faces")?
                .enumerate()
            {
                let face = deficit.map_or(domain_index, |(face, _)| face);
                if duplicate_face_admits_edge(ctx, row, [start, end])? {
                    inputs.ctx.push_vec(
                        &mut choices,
                        (index, edge, face),
                        "catia_standard_duplicate_choices",
                    )?;
                }
            }
            if deficit.is_none() {
                break;
            }
        }
        for (index, edge, face) in inputs
            .ctx
            .admit_iter(&choices, "catia_standard_duplicate_choices_scan")?
            .copied()
        {
            if *operations == MAX_DUPLICATE_FACE_OPERATIONS {
                *exhausted = true;
                return Ok(());
            }
            *operations += 1;
            let [start, end] = inputs.edge_points[edge];
            let start_add = 1 + u8::from(start == end);
            let start_degree_before = duplicate_degree(ctx, &degrees[face], start)?;
            let end_degree_before = if start == end {
                None
            } else {
                duplicate_degree(ctx, &degrees[face], end)?
            };
            set_duplicate_degree(
                inputs.ctx,
                &mut degrees[face],
                start,
                start_degree_before.unwrap_or_default() + start_add,
            )?;
            if start != end {
                set_duplicate_degree(
                    inputs.ctx,
                    &mut degrees[face],
                    end,
                    end_degree_before.unwrap_or_default() + 1,
                )?;
            }
            assignment[index] = face;
            used[index] = true;
            search(
                inputs, degrees, assignment, used, solutions, operations, exhausted,
            )?;
            used[index] = false;
            restore_duplicate_degree(ctx, &mut degrees[face], start, start_degree_before)?;
            if start != end {
                restore_duplicate_degree(ctx, &mut degrees[face], end, end_degree_before)?;
            }
            if *exhausted || solutions.len() > 1 {
                return Ok(());
            }
        }
        Ok(())
    }

    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_points.len()
        || ctx.any_by(
            edge_faces,
            |pair| Ok(pair.iter().any(|&face| face >= face_count)),
            "catia_standard_duplicate_edge_face_bounds",
        )?
    {
        return Ok(None);
    }

    let mut completed = ctx.copy_slice(edge_faces, "catia_standard_duplicate_edge_faces")?;
    let mut unresolved = Vec::new();
    for (edge, faces) in ctx
        .admit_iter(edge_faces, "catia_standard_duplicate_edge_faces")?
        .enumerate()
    {
        if faces[0] == faces[1] {
            ctx.push_vec(
                &mut unresolved,
                edge,
                "catia_standard_duplicate_unresolved_edges",
            )?;
        }
    }
    if unresolved.is_empty() {
        return Ok(Some(completed));
    }
    let mut degrees =
        ctx.collect_indexed_vec(face_count, "catia standard endpoint degrees", |_| {
            Ok(Vec::<(usize, u8)>::new())
        })?;
    for (edge, faces) in ctx
        .admit_iter(edge_faces, "catia_standard_duplicate_edge_faces")?
        .enumerate()
    {
        let incident = if faces[0] == faces[1] {
            &faces[..1]
        } else {
            &faces[..]
        };
        for &face in incident {
            for point in edge_points[edge] {
                let Some(next) = duplicate_degree(ctx, &degrees[face], point)?
                    .unwrap_or_default()
                    .checked_add(1)
                else {
                    return Ok(None);
                };
                set_duplicate_degree(ctx, &mut degrees[face], point, next)?;
            }
        }
    }
    for row in ctx.admit_iter(&degrees, "catia_standard_duplicate_degree_rows")? {
        if ctx.any_by(
            row,
            |(_, degree)| Ok(*degree > 2),
            "catia_standard_duplicate_degree_values",
        )? {
            return Ok(None);
        }
    }
    // Most constrained first: order the unresolved edges by how many faces
    // can still take them, counted once per edge.
    let mut ranked = Vec::new();
    ctx.reserve_vec(
        &mut ranked,
        unresolved.len(),
        "catia standard duplicate unresolved edges sort",
    )?;
    for &edge in ctx.admit_iter(
        &unresolved,
        "catia standard duplicate unresolved edges sort",
    )? {
        let free_faces = ctx.fold(
            &degrees,
            0usize,
            |count, row| {
                Ok(count + usize::from(duplicate_face_admits_edge(ctx, row, edge_points[edge])?))
            },
            "catia standard duplicate unresolved edges sort",
        )?;
        ranked.push((free_faces, edge));
    }
    ctx.stable_sort_by(
        &mut ranked,
        |value| &value.0,
        Ord::cmp,
        "catia standard duplicate unresolved edges sort",
    )?;
    for (slot, (_, edge)) in ctx
        .admit_iter(
            &mut unresolved,
            "catia standard duplicate unresolved edges sort",
        )?
        .zip(ranked)
    {
        *slot = edge;
    }

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
    for (&edge, &face) in ctx
        .admit_iter(&unresolved, "catia_standard_duplicate_completed_edges")?
        .zip(ctx.admit_iter(assignment, "catia_standard_duplicate_completed_faces")?)
    {
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
    for (first, &first_edge) in ctx
        .admit_iter(unresolved, "catia_standard_duplicate_equivalence_classes")?
        .enumerate()
    {
        if classified[first] {
            continue;
        }
        let mut left_faces = Vec::new();
        let mut right_faces = Vec::new();
        for (index, &edge) in ctx
            .admit_iter(unresolved, "catia_standard_duplicate_choice_scan")?
            .enumerate()
        {
            const OPERATION: &str = "catia_standard_duplicate_equivalence_rows";
            let unordered = |[start, end]: [usize; 2]| [start.min(end), start.max(end)];
            if unordered(edge_points[first_edge]) != unordered(edge_points[edge])
                || edge_faces[first_edge][0] != edge_faces[edge][0]
            {
                continue;
            }
            let first_row = &edge_rows[first_edge];
            let row = &edge_rows[edge];
            let same_row = edge_classes.is_some_and(|classes| classes[first_edge] == classes[edge])
                || first_row.kind() == row.kind()
                    && first_row.boundary_layout() == row.boundary_layout()
                    && (ctx.equal(first_row.handles(), row.handles(), OPERATION)?
                        || first_row.handles().len() == row.handles().len()
                            && ctx.all_by(
                                first_row.handles().iter().zip(row.handles().iter().rev()),
                                |(left, right)| Ok(left == right),
                                OPERATION,
                            )?);
            if same_row {
                classified[index] = true;
                ctx.push_vec(
                    &mut left_faces,
                    left[index],
                    "catia_standard_duplicate_left_faces",
                )?;
                ctx.push_vec(
                    &mut right_faces,
                    right[index],
                    "catia_standard_duplicate_right_faces",
                )?;
            }
        }
        ctx.sort_unstable_by(
            &mut left_faces,
            |value| value,
            Ord::cmp,
            "catia_standard_duplicate_left_faces_sort",
        )?;
        ctx.sort_unstable_by(
            &mut right_faces,
            |value| value,
            Ord::cmp,
            "catia_standard_duplicate_right_faces_sort",
        )?;
        if !ctx.equal(
            &left_faces,
            &right_faces,
            "catia_standard_duplicate_equivalence_faces",
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn orient_face_cycles(
    ctx: &DecodeContext<'_>,
    faces: &mut [FaceTopologyDraft],
) -> Result<Option<()>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "catia_standard_orientation_boundaries")?;
    let mut boundaries = Vec::new();
    for face in ctx.admit_iter(faces, "catia_standard_orientation_boundaries")? {
        for boundary in ctx.admit_iter(
            &mut face.boundaries,
            "catia_standard_orientation_boundaries",
        )? {
            ctx.push_scoped_vec(
                &mut storage,
                &mut boundaries,
                boundary,
                "catia_standard_orientation_boundaries",
            )?;
        }
    }
    let mut edge_uses = BTreeMap::<usize, Vec<(usize, bool)>>::new();
    for (node, boundary) in ctx
        .admit_iter(&boundaries, "catia_standard_orientation_edge_uses")?
        .enumerate()
    {
        for coedge in ctx.admit_iter(
            boundary.coedges.as_slice(),
            "catia_standard_orientation_edge_uses",
        )? {
            ctx.push_btree_group(
                &mut edge_uses,
                coedge.edge_row,
                (node, coedge.reversed),
                "catia_standard_orientation_edge_uses",
                "catia_standard_orientation_edge_use_entries",
            )?;
        }
    }
    let Some(flips) =
        solve_boundary_orientation_constraints(ctx, boundaries.len(), &edge_uses, true)?
    else {
        return Ok(None);
    };
    for (index, &flip) in ctx
        .admit_iter(&flips, "catia_standard_orientation_flip_scan")?
        .enumerate()
    {
        let boundary = &mut boundaries[index];
        if flip {
            ctx.reverse(
                &mut boundary.coedges[..],
                "catia_standard_orientation_flips",
            )?;
            for coedge in
                ctx.admit_iter(&mut *boundary.coedges, "catia_standard_orientation_flips")?
            {
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
    edge_uses: &BTreeMap<usize, Vec<(usize, bool)>>,
    require_paired_uses: bool,
) -> Result<Option<Vec<bool>>, CodecError> {
    let mut constraints = ctx.collect_indexed_vec(
        boundary_count,
        "catia standard boundary constraints",
        |_| Ok(Vec::<(usize, bool)>::new()),
    )?;
    for (_, uses) in ctx.admit_iter(edge_uses, "catia_standard_boundary_constraint_sources")? {
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
            ctx.push_vec(
                &mut constraints[*left_node],
                (*right_node, parity),
                "catia_standard_boundary_constraint_entries",
            )?;
            ctx.push_vec(
                &mut constraints[*right_node],
                (*left_node, parity),
                "catia_standard_boundary_constraint_entries",
            )?;
        }
    }

    let mut flips = ctx.alloc_filled(boundary_count, None, "catia standard boundary flips")?;
    let mut result = Vec::new();
    for (root, _) in ctx
        .admit_iter(&constraints, "catia_standard_boundary_roots")?
        .enumerate()
    {
        if let Some(flip) = flips[root] {
            ctx.push_vec(&mut result, flip, "catia_standard_boundary_result")?;
            continue;
        }
        flips[root] = Some(false);
        let mut stack = Vec::new();
        ctx.push_vec(&mut stack, (root, false), "catia_standard_boundary_stack")?;
        while let Some((face, flip)) = stack.pop() {
            for &(neighbor, parity) in
                ctx.admit_iter(&constraints[face], "catia_standard_boundary_stack")?
            {
                let required = flip ^ parity;
                match flips[neighbor] {
                    Some(existing) if existing != required => return Ok(None),
                    Some(_) => {}
                    None => {
                        flips[neighbor] = Some(required);
                        ctx.push_vec(
                            &mut stack,
                            (neighbor, required),
                            "catia_standard_boundary_stack",
                        )?;
                    }
                }
            }
        }
        ctx.push_vec(&mut result, false, "catia_standard_boundary_result")?;
    }
    Ok(Some(result))
}

type IncidenceCycle = NonEmptyMembers<(usize, bool)>;

pub(crate) fn incidence_cycles(
    ctx: &DecodeContext<'_>,
    incident: &[usize],
    edge_points: &[[usize; 2]],
) -> Result<Option<Vec<IncidenceCycle>>, CodecError> {
    #[derive(Clone, Copy)]
    struct AdjacentEdge {
        edge: usize,
        vertex: usize,
        reversed: bool,
    }

    if incident.is_empty() {
        return Ok(None);
    }
    // The vertex rows, the edge order and the unconsumed edges are scratch;
    // only the cycles are returned.
    let mut storage = ctx.reserve_scoped(0, "catia_incidence_scratch")?;
    let mut vertex_indices = HashMap::<usize, usize>::new();
    let mut at_vertex = Vec::<Vec<AdjacentEdge>>::new();
    let mut unseen = Vec::<(usize, [usize; 2])>::new();
    let mut remaining = HashSet::new();
    let mut cycles = Vec::new();
    for &edge in ctx.admit_iter(incident, "catia_incidence_seen_edges")? {
        if !storage.with_storage(|| {
            ctx.insert_hash_set(&mut remaining, edge, "catia_incidence_seen_edges")
        })? {
            return Ok(None);
        }
        let Some(&[start, end]) = edge_points.get(edge) else {
            return Ok(None);
        };
        if start == end {
            let mut members = Vec::new();
            ctx.push_vec(&mut members, (edge, false), "catia_incidence_cycle_members")?;
            let members = NonEmptyMembers::try_from(members).map_err(CodecError::malformed)?;
            ctx.push_vec(&mut cycles, members, "catia_incidence_cycles")?;
            continue;
        }
        let mut endpoints = [0; 2];
        for (slot, vertex) in [start, end].into_iter().enumerate() {
            let index = if let Some(&index) =
                ctx.get_hash_map(&vertex_indices, &vertex, "catia_incidence_vertex_indices")?
            {
                index
            } else {
                let index = at_vertex.len();
                storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut vertex_indices,
                        vertex,
                        index,
                        "catia_incidence_vertex_indices",
                    )?;
                    ctx.push_vec(&mut at_vertex, Vec::new(), "catia_incidence_vertex_rows")
                })?;
                index
            };
            endpoints[slot] = index;
        }
        let [start, end] = endpoints;
        storage.with_storage(|| {
            ctx.push_vec(
                &mut at_vertex[start],
                AdjacentEdge {
                    edge,
                    vertex: end,
                    reversed: false,
                },
                "catia_incidence_vertex_edges",
            )?;
            ctx.push_vec(
                &mut at_vertex[end],
                AdjacentEdge {
                    edge,
                    vertex: start,
                    reversed: true,
                },
                "catia_incidence_vertex_edges",
            )?;
            ctx.push_vec(
                &mut unseen,
                (edge, [start, end]),
                "catia_incidence_unseen_edges",
            )
        })?;
    }
    if ctx.any_by(
        &at_vertex,
        |edges| Ok(edges.len() != 2),
        "catia_incidence_vertex_degrees",
    )? {
        return Ok(None);
    }
    ctx.sort_unstable_by_key(
        &mut unseen,
        |value| std::cmp::Reverse(value.0),
        Ord::cmp,
        "catia_incidence_unseen_edges_sort",
    )?;
    // Each component starts at its least unconsumed edge; an edge consumed by
    // an earlier component is skipped.
    while let Some((first, [start_vertex, mut vertex])) = unseen.pop() {
        if !ctx.remove_hash_set(&mut remaining, &first, "catia_incidence_unseen_scan")? {
            continue;
        }
        let mut members = Vec::new();
        ctx.push_vec(
            &mut members,
            (first, false),
            "catia_incidence_cycle_members",
        )?;
        let mut previous_edge = first;
        while vertex != start_vertex {
            // Every vertex has two distinct incident edges. The edge other
            // than the one just traversed continues this closed component.
            let [left, right] = at_vertex[vertex].as_slice() else {
                return Ok(None);
            };
            let next = if left.edge == previous_edge {
                right
            } else {
                left
            };
            if !ctx.remove_hash_set(&mut remaining, &next.edge, "catia_incidence_unseen_scan")? {
                return Ok(None);
            }
            vertex = next.vertex;
            previous_edge = next.edge;
            ctx.push_vec(
                &mut members,
                (next.edge, next.reversed),
                "catia_incidence_cycle_members",
            )?;
        }
        let members = NonEmptyMembers::try_from(members).map_err(CodecError::malformed)?;
        ctx.push_vec(&mut cycles, members, "catia_incidence_cycles")?;
    }
    Ok(Some(cycles))
}

/// Parses the FBB-only spine. Its edge rows and trim handles use one selected
/// big-endian width; the
/// following counted `05 08 01` table supplies vertex coordinates.
pub(crate) fn parse_fbb(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    let Some(face_run) = largest_fbb_run(ctx, bytes)? else {
        return Ok(None);
    };
    let face_start = face_run.face_start();
    let face_count = face_run.face_count();
    let after_faces = face_run.after_faces();
    let Some((mut edge_rows, _, vertex_header, handle_width)) =
        parse_fbb_edge_tables(ctx, bytes, after_faces)?
    else {
        return Ok(None);
    };
    let Some(vertex_points) = parse_vertex_table(ctx, bytes, vertex_header)? else {
        return Ok(None);
    };
    let Some(trims) = parse_trim_chain(ctx, bytes, face_start, face_count, handle_width)? else {
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
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    let Some(topology) = parse_fbb(ctx, bytes)? else {
        return Ok(None);
    };
    topology.with_native_edge_vertices(ctx, edge_ports)
}

pub(super) fn reconstruct(
    ctx: &DecodeContext<'_>,
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<[f64; 3]>,
    trims: &[TrimRecord],
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    let node_count = edge_rows
        .len()
        .checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit("catia_reconstruct_union", u64::MAX, u64::MAX))?;
    let mut union = UnionFind::charged(ctx, node_count, "catia_reconstruct_union")?;
    let mut row_storage = ctx.reserve_scoped(0, "catia_fbb_row_pattern_ends")?;
    let ends = row_storage.with_storage(|| row_pattern_ends(ctx, &edge_rows))?;
    let mut faces = Vec::new();
    for trim in ctx.admit_iter(trims, "catia_reconstruct_faces")? {
        let Some(cycles) = boundary_cycles(ctx, trim.packet.triangles(ctx)?)? else {
            return Ok(None);
        };
        let mut boundaries = Vec::new();
        for cycle in ctx.admit_iter(cycles, "catia_reconstruct_boundaries")? {
            let Some(boundary) = cover_cycle(ctx, &cycle, &edge_rows, &ends, &mut union)? else {
                return Ok(None);
            };
            ctx.push_vec(&mut boundaries, boundary, "catia_reconstruct_boundaries")?;
        }
        ctx.push_vec(
            &mut faces,
            FaceTopologyDraft { boundaries },
            "catia_reconstruct_faces",
        )?;
    }

    let mut storage = ctx.reserve_scoped(0, "catia_reconstruct_roots")?;
    let mut roots = HashMap::new();
    // Node `2 * edge + endpoint` is an edge endpoint; node_count bounds them.
    for node in ctx.admit_iter(0..node_count, "catia_reconstruct_root_edge_rows")? {
        record_union_root(
            ctx,
            &mut storage,
            &mut union,
            &mut roots,
            node,
            "catia_reconstruct_roots",
        )?;
    }
    for face in ctx.admit_iter(&mut faces, "catia_reconstruct_root_faces")? {
        rename_boundary_vertices(ctx, &mut face.boundaries, &mut union, &roots)?;
    }

    Ok(Some(StandardTopologyDraft {
        faces,
        edge_rows,
        vertex_points,
        logical_vertex_count: roots.len(),
    }))
}

/// Names a union-find root by the order of its first recording.
fn record_union_root(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    union: &mut UnionFind,
    roots: &mut HashMap<usize, usize>,
    node: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    let root = union.find(ctx, node)?;
    if !ctx.contains_key_hash_map(roots, &root, operation)? {
        let next = roots.len();
        storage.with_storage(|| ctx.insert_hash_map(roots, root, next, operation))?;
    }
    Ok(())
}

/// The recorded name of a node's union-find root.
fn union_root_name(
    ctx: &DecodeContext<'_>,
    union: &mut UnionFind,
    roots: &HashMap<usize, usize>,
    node: usize,
) -> Result<usize, CodecError> {
    let root = union.find(ctx, node)?;
    ctx.get_hash_map(roots, &root, "catia_topology_root_names")?
        .copied()
        .ok_or_else(|| CodecError::malformed("topology vertex has no recorded root"))
}

fn rename_boundary_vertices(
    ctx: &DecodeContext<'_>,
    boundaries: &mut [BoundaryDraft],
    union: &mut UnionFind,
    roots: &HashMap<usize, usize>,
) -> Result<(), CodecError> {
    for boundary in ctx.admit_iter(boundaries, "catia_topology_root_boundaries")? {
        for coedge in ctx.admit_iter(&mut *boundary.coedges, "catia_topology_root_coedges")? {
            coedge.start_vertex = union_root_name(ctx, union, roots, coedge.start_vertex)?;
            coedge.end_vertex = union_root_name(ctx, union, roots, coedge.end_vertex)?;
        }
    }
    Ok(())
}

pub(crate) fn reconstruct_mesh_selection(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    selected: &[impl std::borrow::Borrow<MeshFaceBoundaryAssignment>],
    unmatched_reversed: &[Vec<Vec<bool>>],
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    if selected.len() != unmatched_reversed.len() {
        return Ok(None);
    }
    let node_count = edge_rows
        .len()
        .checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit("catia_mesh_selection_union", u64::MAX, u64::MAX))?;
    let mut union = UnionFind::charged(ctx, node_count, "catia_mesh_selection_union")?;
    let mut faces = Vec::new();
    for (face, directions) in ctx
        .admit_iter(selected, "catia_mesh_selection_faces")?
        .zip(ctx.admit_iter(unmatched_reversed, "catia_mesh_selection_faces")?)
    {
        let face = std::borrow::Borrow::borrow(face);
        if face.boundaries.len() != directions.len() {
            return Ok(None);
        }
        let mut boundaries = Vec::new();
        for (uses, directions) in ctx
            .admit_iter(&face.boundaries, "catia_mesh_selection_boundaries")?
            .zip(ctx.admit_iter(directions, "catia_mesh_selection_boundaries")?)
        {
            if uses.len() != directions.len() {
                return Ok(None);
            }
            let mut paired_uses = ctx
                .admit_iter(uses, "catia_mesh_selection_coedges")?
                .zip(ctx.admit_iter(directions, "catia_mesh_selection_coedges")?)
                .enumerate();
            let Some((first_index, (first_use, &first_reversed))) = paired_uses.next() else {
                return Ok(None);
            };
            let mut corners = Vec::new();
            for _ in ctx.admit_iter(uses, "catia_mesh_selection_corner_nodes")? {
                let corner = union.push_charged(ctx, "catia_mesh_selection_corner_nodes")?;
                ctx.push_vec(&mut corners, corner, "catia_mesh_selection_corners")?;
            }
            let mut admit_coedge = |use_index: usize,
                                    use_: &MeshBoundaryEdgeCandidate,
                                    unmatched_reversed: bool|
             -> Result<Option<CoedgeUse>, CodecError> {
                let reversed = use_.reversed.unwrap_or(unmatched_reversed);
                if use_.reversed.is_some() && unmatched_reversed != reversed {
                    return Ok(None);
                }
                let start_vertex = corners[use_index];
                let end_vertex = corners[(use_index + 1) % corners.len()];
                let Some(edge_end) = use_
                    .edge
                    .checked_mul(2)
                    .and_then(|start| start.checked_add(1))
                else {
                    return Ok(None);
                };
                let edge_start = edge_end - 1;
                if edge_end >= node_count {
                    return Ok(None);
                }
                if reversed {
                    union.union(ctx, edge_end, start_vertex)?;
                    union.union(ctx, edge_start, end_vertex)?;
                } else {
                    union.union(ctx, edge_start, start_vertex)?;
                    union.union(ctx, edge_end, end_vertex)?;
                }
                Ok(Some(CoedgeUse {
                    edge_row: use_.edge,
                    reversed,
                    start_vertex,
                    end_vertex,
                }))
            };
            let Some(first) = admit_coedge(first_index, first_use, first_reversed)? else {
                return Ok(None);
            };
            let mut coedges = Vec::new();
            ctx.push_vec(&mut coedges, first, "catia_mesh_selection_coedges")?;
            for (use_index, (use_, &unmatched_reversed)) in paired_uses {
                let Some(coedge) = admit_coedge(use_index, use_, unmatched_reversed)? else {
                    return Ok(None);
                };
                ctx.push_vec(&mut coedges, coedge, "catia_mesh_selection_coedges")?;
            }
            let Ok(coedges) = NonEmptyMembers::<CoedgeUse>::try_from(coedges) else {
                return Ok(None);
            };
            ctx.push_vec(
                &mut boundaries,
                BoundaryDraft { coedges },
                "catia_mesh_selection_boundaries",
            )?;
        }
        ctx.push_vec(
            &mut faces,
            FaceTopologyDraft { boundaries },
            "catia_mesh_selection_faces",
        )?;
    }
    let mut storage = ctx.reserve_scoped(0, "catia_mesh_selection_roots")?;
    let mut roots = HashMap::new();
    {
        let mut record_root = |node| {
            record_union_root(
                ctx,
                &mut storage,
                &mut union,
                &mut roots,
                node,
                "catia_mesh_selection_roots",
            )
        };
        for (edge, _) in ctx
            .admit_iter(edge_rows, "catia_mesh_selection_root_edge_rows")?
            .enumerate()
        {
            for endpoint in [0usize, 1] {
                let node = edge
                    .checked_mul(2)
                    .and_then(|start| start.checked_add(endpoint))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "catia_mesh_selection_root_edge_rows",
                            u64::MAX,
                            u64::MAX,
                        )
                    })?;
                record_root(node)?;
            }
        }
        let mut node = node_count;
        for face in ctx.admit_iter(&faces, "catia_mesh_selection_root_faces")? {
            for boundary in
                ctx.admit_iter(&face.boundaries, "catia_mesh_selection_root_boundaries")?
            {
                for _ in ctx.admit_iter(
                    boundary.coedges.as_slice(),
                    "catia_mesh_selection_root_coedges",
                )? {
                    record_root(node)?;
                    node = node.checked_add(1).ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "catia_mesh_selection_root_coedges",
                            u64::MAX,
                            u64::MAX,
                        )
                    })?;
                }
            }
        }
    }
    for (face_index, selected_face) in ctx
        .admit_iter(selected, "catia_mesh_selection_rewrite_faces")?
        .enumerate()
    {
        let selected_face = std::borrow::Borrow::borrow(selected_face);
        for (boundary_index, uses) in ctx
            .admit_iter(
                &selected_face.boundaries,
                "catia_mesh_selection_rewrite_boundaries",
            )?
            .enumerate()
        {
            for (coedge_index, _) in ctx
                .admit_iter(uses, "catia_mesh_selection_rewrite_coedges")?
                .enumerate()
            {
                let coedge =
                    &mut faces[face_index].boundaries[boundary_index].coedges[coedge_index];
                coedge.start_vertex =
                    union_root_name(ctx, &mut union, &roots, coedge.start_vertex)?;
                coedge.end_vertex = union_root_name(ctx, &mut union, &roots, coedge.end_vertex)?;
            }
        }
    }
    let mut owned_rows = Vec::new();
    ctx.reserve_vec(
        &mut owned_rows,
        edge_rows.len(),
        "catia_mesh_selection_edge_copy",
    )?;
    for row in ctx.admit_iter(edge_rows, "catia_mesh_selection_edge_rows")? {
        owned_rows.push(EdgeRow {
            kind: row.kind,
            handles: ctx.copy_slice(row.handles(), "catia_mesh_selection_handle_copy")?,
            boundary_layout: row.boundary_layout,
        });
    }
    Ok(Some(StandardTopologyDraft {
        faces,
        edge_rows: owned_rows,
        logical_vertex_count: roots.len(),
        vertex_points: ctx.copy_slice(vertex_points, "catia_mesh_selection_point_copy")?,
    }))
}

#[cfg(test)]
mod tests;
