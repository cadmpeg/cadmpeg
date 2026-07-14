// SPDX-License-Identifier: Apache-2.0
//! Byte-level topology for standard nested CATIA V5 B-rep streams.

use std::collections::{HashMap, HashSet};

const FBB_ROW: [u8; 4] = [0x30, 0x04, 0x04, 0xff];
const EDGE_DELIMITER: [u8; 8] = [0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
const TRIM_KINDS: [u8; 14] = [
    0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f,
];

/// Reconstructed standard-nested (or FBB-only) topology: the counted spine's
/// face boundaries recovered from the trim-mesh triangle packets, plus the
/// physical edge rows and, for the standard family, the `05 08 01` vertex
/// coordinate table ([spec ?5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#5-standard-nested-v5_cfv2-topology-spine)).
#[derive(Debug, Clone, PartialEq)]
pub struct StandardTopology {
    faces: Vec<FaceTopology>,
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<[f64; 3]>,
    logical_vertex_count: usize,
}

impl StandardTopology {
    /// Number of faces, equal to the largest contiguous `30 04 04 ff` FBB
    /// run's row count ([spec ?5.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#52-spine-grammar)).
    #[must_use]
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// Per-face reconstructed boundaries, in FBB row order ([spec ?5.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#51-positional-binding): face
    /// ordinal `i` binds to FBB row `i`).
    #[must_use]
    pub fn faces(&self) -> &[FaceTopology] {
        &self.faces
    }

    /// The counted spine's physical edge rows, in table order ([spec ?5.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#52-spine-grammar)).
    #[must_use]
    pub fn edge_rows(&self) -> &[EdgeRow] {
        &self.edge_rows
    }

    /// The `05 08 01` vertex coordinate table, in table order. Empty for a
    /// topology built by [`parse_fbb`], whose coordinate records are not
    /// part of the counted spine.
    #[must_use]
    pub fn vertex_points(&self) -> &[[f64; 3]] {
        &self.vertex_points
    }

    /// Number of port/corner equivalence classes. Coordinate rows are a
    /// separate stored table and are not assigned to these classes here.
    #[must_use]
    pub fn logical_vertex_count(&self) -> usize {
        self.logical_vertex_count
    }

    /// Bind logical port/corner components to coordinate-row indices from one
    /// exact unordered endpoint pair per physical edge. A result is returned
    /// only when the induced bijection is unique.
    #[must_use]
    pub fn bind_vertex_points(&self, edge_point_pairs: &[[usize; 2]]) -> Option<Vec<usize>> {
        if edge_point_pairs.len() != self.edge_rows.len()
            || self.logical_vertex_count != self.vertex_points.len()
        {
            return None;
        }
        let edge_vertices = self.edge_vertices()?;
        let all_points: HashSet<usize> = (0..self.vertex_points.len()).collect();
        let mut domains = vec![all_points; self.logical_vertex_count];
        for (edge, pair) in edge_vertices.into_iter().zip(edge_point_pairs) {
            if pair[0] >= self.vertex_points.len() || pair[1] >= self.vertex_points.len() {
                return None;
            }
            let [start, end] = edge;
            let candidates = HashSet::from(*pair);
            domains[start].retain(|point| candidates.contains(point));
            domains[end].retain(|point| candidates.contains(point));
        }
        if domains.iter().any(HashSet::is_empty) {
            return None;
        }

        let mut solutions = Vec::new();
        unique_bijections(
            &domains,
            &mut vec![None; domains.len()],
            &mut HashSet::new(),
            &mut solutions,
        );
        (solutions.len() == 1).then(|| solutions.remove(0))
    }

    /// Logical endpoint components in physical edge-row direction.
    #[must_use]
    pub fn edge_vertices(&self) -> Option<Vec<[usize; 2]>> {
        let mut edge_vertices = vec![None; self.edge_rows.len()];
        for face in &self.faces {
            for boundary in &face.boundaries {
                for coedge in &boundary.coedges {
                    let endpoints = if coedge.reversed {
                        [coedge.end_vertex, coedge.start_vertex]
                    } else {
                        [coedge.start_vertex, coedge.end_vertex]
                    };
                    match edge_vertices[coedge.edge_row] {
                        Some(previous) if previous != endpoints => return None,
                        Some(_) => {}
                        None => edge_vertices[coedge.edge_row] = Some(endpoints),
                    }
                }
            }
        }

        edge_vertices.into_iter().collect()
    }

    /// Replace provisional trim-handle endpoint components with the quotient
    /// induced by one native endpoint-identity pair per physical edge.
    ///
    /// Native identities are global within the parsed topology. Equal values
    /// collapse face-local corners even when adjacent faces use different trim
    /// handles. The pair order is the physical edge-row direction.
    #[must_use]
    pub fn with_native_edge_vertices(&self, edge_ports: &[[u32; 2]]) -> Option<Self> {
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

fn unique_bijections(
    domains: &[HashSet<usize>],
    assignment: &mut [Option<usize>],
    used: &mut HashSet<usize>,
    solutions: &mut Vec<Vec<usize>>,
) {
    if solutions.len() > 1 {
        return;
    }
    let next = assignment
        .iter()
        .enumerate()
        .filter(|(_, value)| value.is_none())
        .min_by_key(|(vertex, _)| {
            domains[*vertex]
                .iter()
                .filter(|point| !used.contains(point))
                .count()
        })
        .map(|(vertex, _)| vertex);
    let Some(vertex) = next else {
        solutions.push(
            assignment
                .iter()
                .map(|value| value.expect("complete assignment"))
                .collect(),
        );
        return;
    };
    let mut candidates: Vec<usize> = domains[vertex]
        .iter()
        .filter(|point| !used.contains(point))
        .copied()
        .collect();
    candidates.sort_unstable();
    for point in candidates {
        assignment[vertex] = Some(point);
        used.insert(point);
        unique_bijections(domains, assignment, used, solutions);
        used.remove(&point);
        assignment[vertex] = None;
        if solutions.len() > 9 {
            return;
        }
    }
}

/// The boundary meaning of an edge-row handle sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeBoundaryLayout {
    /// The first and last handles are endpoint ports outside the trim-handle
    /// namespace; the handles between them match the boundary.
    InteriorWithFlankingCorners,
    /// Every handle belongs to the trim boundary, including both endpoints.
    CompleteBoundaryRun,
}

/// One row of a counted standard/FBB edge table, with handles read big-endian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeRow {
    /// Table-kind byte the row was parsed under (`0x01` or `0x02`; spec
    /// ?5.2 `count_header`).
    pub kind: u8,
    /// The row's BE handle sequence.
    pub handles: Vec<u32>,
    /// How the handle sequence maps onto a trim boundary.
    pub boundary_layout: EdgeBoundaryLayout,
}

impl EdgeRow {
    fn boundary_pattern(&self) -> Option<&[u32]> {
        match self.boundary_layout {
            EdgeBoundaryLayout::InteriorWithFlankingCorners => {
                self.handles.get(1..self.handles.len().checked_sub(1)?)
            }
            EdgeBoundaryLayout::CompleteBoundaryRun => Some(self.handles.as_slice()),
        }
        .filter(|pattern| !pattern.is_empty())
    }

    fn boundary_span(&self, pattern_start: usize, cycle_len: usize) -> Option<(usize, usize)> {
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

/// One face's reconstructed boundary cycles ([spec ?5.3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#53-trim-records-indexed-triangle-mesh-packets)): one outer cycle
/// plus one per hole, in the order recovered from the trim mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceTopology {
    /// The face's boundary cycles; loop count equals boundary-cycle count.
    pub boundaries: Vec<Boundary>,
}

/// One closed boundary cycle of a face's trim mesh, covered end-to-end by
/// matched edge rows ([spec ?5.3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#53-trim-records-indexed-triangle-mesh-packets)?[?5.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#54-physical-edge-identity-and-portvertex-collapse)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boundary {
    /// The physical edge uses covering this cycle, in cycle order.
    pub coedges: Vec<CoedgeUse>,
}

/// One physical edge's use within a face boundary, oriented by its match
/// against the recovered boundary cycle ([spec ?5.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#54-physical-edge-identity-and-portvertex-collapse)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoedgeUse {
    /// Index into [`StandardTopology::edge_rows`] for the matched edge
    /// row.
    pub edge_row: usize,
    /// `true` when the edge row's handle sequence matched the boundary
    /// cycle in reverse; orientation comes from this match, not a stored
    /// sense bit ([spec ?5.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#54-physical-edge-identity-and-portvertex-collapse)).
    pub reversed: bool,
    /// Logical-vertex (union-find component) index at this coedge's start,
    /// in boundary-cycle traversal direction.
    pub start_vertex: usize,
    /// Logical-vertex (union-find component) index at this coedge's end,
    /// in boundary-cycle traversal direction.
    pub end_vertex: usize,
}

#[derive(Debug, Clone)]
struct TrimRecord {
    triangles: Vec<[u32; 3]>,
    frame_vector: Option<[f64; 3]>,
    handles: Vec<u32>,
    kind: u8,
    end: usize,
}

/// Unit frame vectors carried by framed standard trim packets, in packet order.
///
/// Only the planar packet kinds are returned. Their positional order binds them
/// to the standard plane bounds records.
#[must_use]
pub fn standard_plane_normals(bytes: &[u8]) -> Vec<[f64; 3]> {
    parse_trim_records(bytes)
        .into_iter()
        .filter(|record| matches!(record.kind, 0x49 | 0x4a | 0x4b | 0x4c | 0x4e | 0x4f))
        .filter_map(|record| record.frame_vector)
        .collect()
}

/// Return the counted standard-spine vertex table. The edge-table width family
/// is resolved structurally before accepting the following `01 06` table.
#[must_use]
pub fn standard_vertex_points(bytes: &[u8]) -> Option<Vec<[f64; 3]>> {
    let (_, _, after_faces) = largest_fbb_run(bytes)?;
    let (_, vertex_header) = parse_edge_tables(bytes, after_faces)?;
    parse_vertex_table(bytes, vertex_header)
}

/// Parses the counted standard spine, positional trim packets, mesh boundary
/// cycles, physical edge uses, and port/corner vertex equivalence classes.
/// Returns `None` unless every positional face boundary is unambiguous.
#[must_use]
pub fn parse_standard(bytes: &[u8]) -> Option<StandardTopology> {
    let (face_start, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, vertex_header) = parse_edge_tables(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    let mut solutions = Vec::new();
    for width in [2, 3] {
        let Some(trims) = parse_trim_chain(bytes, face_start, face_count, width) else {
            continue;
        };
        if let Some(topology) = reconstruct(edge_rows.clone(), vertex_points.clone(), &trims) {
            solutions.push(topology);
        }
    }
    <[StandardTopology; 1]>::try_from(solutions)
        .ok()
        .map(|[topology]| topology)
}

/// Reconstruct regular-motif standard topology by replaying the trim packet's
/// vertex-allocation program. The allocation is accepted only when it covers
/// the complete vertex table and reproduces every supplied circle endpoint
/// anchor.
#[must_use]
pub fn parse_standard_motif(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    circle_anchors: &[Option<[usize; 2]>],
) -> Option<StandardTopology> {
    let (face_start, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, vertex_header) = parse_edge_tables(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    if edge_rows.len() != edge_faces.len() || edge_rows.len() != circle_anchors.len() {
        return None;
    }
    let mut solutions = Vec::new();
    for width in [2, 3] {
        let Some(trims) = parse_trim_chain(bytes, face_start, face_count, width) else {
            continue;
        };
        let Some(port_points) = motif_port_points(&trims, vertex_points.len()) else {
            continue;
        };
        let Some(edge_points) = edge_rows
            .iter()
            .map(|row| {
                Some([
                    *port_points.get(row.handles.first()?)?,
                    *port_points.get(row.handles.last()?)?,
                ])
            })
            .collect::<Option<Vec<[usize; 2]>>>()
        else {
            continue;
        };
        let anchors_match = edge_points
            .iter()
            .zip(circle_anchors)
            .all(|(points, anchor)| {
                anchor.is_none_or(|mut anchor| {
                    anchor.sort_unstable();
                    let mut points = *points;
                    points.sort_unstable();
                    points == anchor
                })
            });
        if anchors_match {
            if let Some(topology) = reconstruct_incidence(
                edge_rows.clone(),
                vertex_points.clone(),
                edge_faces,
                &edge_points,
                face_count,
            ) {
                solutions.push(topology);
            }
        }
    }
    <[StandardTopology; 1]>::try_from(solutions)
        .ok()
        .map(|[topology]| topology)
}

/// Reconstruct standard topology from byte-derived endpoint coordinate rows.
/// The endpoint graph is accepted only when all face incidences close and the
/// radial orientation constraints are consistent.
#[must_use]
pub fn parse_standard_endpoints(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
) -> Option<StandardTopology> {
    let (_, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, vertex_header) = parse_edge_tables(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_points.len()
        || edge_points
            .iter()
            .flatten()
            .any(|point| *point >= vertex_points.len())
    {
        return None;
    }
    reconstruct_incidence(
        edge_rows,
        vertex_points,
        edge_faces,
        edge_points,
        face_count,
    )
}

/// Reconstruct standard topology while resolving edges that have multiple
/// geometrically valid endpoint pairs. Candidate pairs and edge rows use their
/// serialized order as the stable gauge when equivalent assignments permute
/// indistinguishable line rows. The selected assignment must close every face
/// cycle and satisfy radial orientation.
#[must_use]
pub fn parse_standard_endpoint_candidates(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
) -> Option<StandardTopology> {
    let (_, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, vertex_header) = parse_edge_tables(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_candidates.len()
        || edge_candidates.iter().any(Vec::is_empty)
        || edge_candidates
            .iter()
            .flatten()
            .flatten()
            .any(|point| *point >= vertex_points.len())
    {
        return None;
    }

    reconstruct_incidence_candidates(
        &edge_rows,
        &vertex_points,
        edge_faces,
        edge_candidates,
        face_count,
    )
}

struct IncidenceCandidateSearch<'a> {
    choices: &'a [Vec<[usize; 2]>],
    edge_faces: &'a [[usize; 2]],
    face_edges: Vec<Vec<usize>>,
    edge_rows: &'a [EdgeRow],
    vertex_points: &'a [[f64; 3]],
    face_count: usize,
    assignment: Vec<Option<[usize; 2]>>,
    degrees: Vec<Vec<u8>>,
    solution: Option<StandardTopology>,
    states: usize,
}

impl IncidenceCandidateSearch<'_> {
    fn candidate_fits(&self, edge: usize, pair: [usize; 2]) -> bool {
        let mut faces = self.edge_faces[edge].to_vec();
        faces.sort_unstable();
        faces.dedup();
        faces
            .iter()
            .all(|&face| pair.iter().all(|&point| self.degrees[face][point] < 2))
    }

    fn feasible(&self) -> bool {
        for face in 0..self.face_count {
            for point in 0..self.vertex_points.len() {
                if self.degrees[face][point] != 1 {
                    continue;
                }
                let can_complete = self.face_edges[face].iter().copied().any(|edge| {
                    self.assignment[edge].is_none()
                        && self.edge_faces[edge].contains(&face)
                        && self.choices[edge]
                            .iter()
                            .any(|pair| pair.contains(&point) && self.candidate_fits(edge, *pair))
                });
                if !can_complete {
                    return false;
                }
            }
        }
        true
    }

    fn search(&mut self) {
        // Candidate incidence is a fallback after native-port and trim-mesh
        // propagation. Keep it bounded so unresolved geometric ambiguity
        // declines atomically instead of making container decode unbounded.
        const MAX_STATES: usize = 1_024;
        if self.solution.is_some() || self.states >= MAX_STATES {
            return;
        }
        self.states += 1;
        let next = (0..self.choices.len())
            .filter(|edge| self.assignment[*edge].is_none())
            .map(|edge| {
                let count = self.choices[edge]
                    .iter()
                    .filter(|pair| self.candidate_fits(edge, **pair))
                    .count();
                (count, edge)
            })
            .min();
        let Some((count, edge)) = next else {
            let points = self.assignment.iter().copied().collect::<Option<Vec<_>>>();
            self.solution = points.and_then(|points| {
                reconstruct_incidence(
                    self.edge_rows.to_vec(),
                    self.vertex_points.to_vec(),
                    self.edge_faces,
                    &points,
                    self.face_count,
                )
            });
            return;
        };
        if count == 0 {
            return;
        }
        for candidate in 0..self.choices[edge].len() {
            let pair = self.choices[edge][candidate];
            if !self.candidate_fits(edge, pair) {
                continue;
            }
            let mut faces = self.edge_faces[edge].to_vec();
            faces.sort_unstable();
            faces.dedup();
            for &face in &faces {
                for &point in &pair {
                    self.degrees[face][point] += 1;
                }
            }
            self.assignment[edge] = Some(pair);
            if self.feasible() {
                self.search();
            }
            self.assignment[edge] = None;
            for &face in &faces {
                for &point in &pair {
                    self.degrees[face][point] -= 1;
                }
            }
        }
    }
}

fn reconstruct_incidence_candidates(
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    face_count: usize,
) -> Option<StandardTopology> {
    let mut choices = edge_candidates.to_vec();
    for candidates in &mut choices {
        for pair in candidates.iter_mut() {
            pair.sort_unstable();
        }
        let mut seen = HashSet::new();
        candidates.retain(|pair| seen.insert(*pair));
    }
    let mut face_edges = vec![Vec::new(); face_count];
    for (edge, faces) in edge_faces.iter().enumerate() {
        for &face in faces {
            if face < face_count && !face_edges[face].contains(&edge) {
                face_edges[face].push(edge);
            }
        }
    }
    let mut search = IncidenceCandidateSearch {
        choices: &choices,
        edge_faces,
        face_edges,
        edge_rows,
        vertex_points,
        face_count,
        assignment: vec![None; choices.len()],
        degrees: vec![vec![0; vertex_points.len()]; face_count],
        solution: None,
        states: 0,
    };
    for edge in 0..choices.len() {
        let [pair] = choices[edge].as_slice() else {
            continue;
        };
        if !search.candidate_fits(edge, *pair) {
            return None;
        }
        let mut faces = edge_faces[edge].to_vec();
        faces.sort_unstable();
        faces.dedup();
        for face in faces {
            for point in pair {
                search.degrees[face][*point] += 1;
            }
        }
        search.assignment[edge] = Some(*pair);
    }
    if !search.feasible() {
        return None;
    }
    search.search();
    search.solution
}

/// Return the endpoint-port handles for the standard edge table, in physical
/// edge-row order.
#[must_use]
pub fn standard_edge_ports(bytes: &[u8]) -> Option<Vec<[u32; 2]>> {
    let (_, _, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, _) = parse_edge_tables(bytes, after_faces)?;
    edge_rows
        .iter()
        .map(|row| Some([*row.handles.first()?, *row.handles.last()?]))
        .collect()
}

/// Return the counted physical edge rows in their serialized table order.
///
/// Each row retains its table-kind byte, native handle width semantics, and
/// complete handle sequence even when full topology reconstruction is not yet
/// possible.
#[must_use]
pub fn standard_edge_rows(bytes: &[u8]) -> Option<Vec<EdgeRow>> {
    let (_, _, after_faces) = largest_fbb_run(bytes)?;
    parse_edge_tables(bytes, after_faces).map(|(rows, _)| rows)
}

pub(crate) fn standard_edge_port_identities(bytes: &[u8]) -> Option<Vec<[u32; 2]>> {
    let (_, _, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, _) = parse_edge_tables(bytes, after_faces)?;
    let mut identities = HashMap::new();
    edge_rows
        .iter()
        .map(|row| {
            [*row.handles.first()?, *row.handles.last()?]
                .map(|handle| {
                    let next = identities.len();
                    u32::try_from(*identities.entry((row.kind, handle)).or_insert(next)).ok()
                })
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .and_then(|ports| <[u32; 2]>::try_from(ports).ok())
        })
        .collect()
}

/// Collapse physical edge endpoints through every exact trim-mesh occurrence.
/// The returned component identifiers are compact and stable within this
/// result; they are not coordinate-row indices.
#[must_use]
pub fn standard_mesh_edge_ports(bytes: &[u8]) -> Option<Vec<[u32; 2]>> {
    let (face_start, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, _) = parse_edge_tables(bytes, after_faces)?;
    let mut solutions = Vec::new();
    for width in [2, 3] {
        let Some(trims) = parse_trim_chain(bytes, face_start, face_count, width) else {
            continue;
        };
        let cycles = trims
            .iter()
            .map(|trim| boundary_cycles(&trim.triangles))
            .collect::<Option<Vec<_>>>()?;
        let occurrences = mesh_edge_occurrences(&edge_rows, &cycles)?;
        let mut union = UnionFind::new(edge_rows.len() * 2);
        let mut table_ports = HashMap::new();
        for (edge, row) in edge_rows.iter().enumerate() {
            for (side, handle) in [row.handles.first()?, row.handles.last()?]
                .into_iter()
                .enumerate()
            {
                let node = edge * 2 + side;
                if let Some(previous) = table_ports.insert((row.kind, *handle), node) {
                    union.union(previous, node);
                }
            }
        }
        let mut corners = HashMap::new();
        for (edge, row) in edge_rows.iter().enumerate() {
            let Some(_) = row.boundary_pattern() else {
                continue;
            };
            for occurrence in &occurrences[edge] {
                let cycle = &cycles[occurrence.face][occurrence.cycle];
                let (before, segment_count) = row.boundary_span(occurrence.start, cycle.len())?;
                let after = (before + segment_count) % cycle.len();
                let before_node = *corners
                    .entry((occurrence.face, occurrence.cycle, before))
                    .or_insert_with(|| union.push());
                let after_node = *corners
                    .entry((occurrence.face, occurrence.cycle, after))
                    .or_insert_with(|| union.push());
                if occurrence.reversed {
                    union.union(edge * 2 + 1, before_node);
                    union.union(edge * 2, after_node);
                } else {
                    union.union(edge * 2, before_node);
                    union.union(edge * 2 + 1, after_node);
                }
            }
        }
        let mut roots = HashMap::new();
        let mut ports = Vec::with_capacity(edge_rows.len());
        for edge in 0..edge_rows.len() {
            let pair = [edge * 2, edge * 2 + 1].map(|node| {
                let root = union.find(node);
                let next = roots.len();
                u32::try_from(*roots.entry(root).or_insert(next)).ok()
            });
            ports.push([pair[0]?, pair[1]?]);
        }
        solutions.push(ports);
    }
    <[Vec<[u32; 2]>; 1]>::try_from(solutions)
        .ok()
        .map(|[ports]| ports)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MeshEdgeOccurrence {
    face: usize,
    cycle: usize,
    start: usize,
    reversed: bool,
}

/// One exact occurrence of a physical edge row on a trim-mesh boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshEdgeRun {
    /// Physical edge-row ordinal.
    pub edge: usize,
    /// Positional face ordinal.
    pub face: usize,
    /// Boundary-cycle ordinal within the face.
    pub cycle: usize,
    /// First covered boundary-segment index in cycle traversal order.
    pub start: usize,
    /// Number of consecutive boundary segments covered by this occurrence.
    pub segment_count: usize,
    /// Whether cycle traversal follows the row's handle sequence in reverse.
    pub reversed: bool,
}

fn mesh_edge_occurrences(
    edge_rows: &[EdgeRow],
    cycles: &[Vec<Vec<u32>>],
) -> Option<Vec<Vec<MeshEdgeOccurrence>>> {
    let mut locations = HashMap::<u32, Vec<(usize, usize, usize)>>::new();
    for (face, face_cycles) in cycles.iter().enumerate() {
        for (cycle, handles) in face_cycles.iter().enumerate() {
            for (position, handle) in handles.iter().copied().enumerate() {
                locations
                    .entry(handle)
                    .or_default()
                    .push((face, cycle, position));
            }
        }
    }
    edge_rows
        .iter()
        .map(|row| {
            let Some(pattern) = row.boundary_pattern() else {
                return Some(Vec::new());
            };
            let mut matches = HashMap::<(usize, usize, usize), bool>::new();
            for &(face, cycle, start) in locations.get(&pattern[0]).into_iter().flatten() {
                let handles = &cycles[face][cycle];
                if pattern
                    .iter()
                    .enumerate()
                    .all(|(offset, handle)| handles[(start + offset) % handles.len()] == *handle)
                {
                    matches.insert((face, cycle, start), false);
                }
            }
            for &(face, cycle, start) in locations.get(pattern.last()?).into_iter().flatten() {
                let handles = &cycles[face][cycle];
                if pattern
                    .iter()
                    .rev()
                    .enumerate()
                    .all(|(offset, handle)| handles[(start + offset) % handles.len()] == *handle)
                {
                    matches.entry((face, cycle, start)).or_insert(true);
                }
            }
            let mut cycle_counts = HashMap::new();
            for &(face, cycle, _) in matches.keys() {
                *cycle_counts.entry((face, cycle)).or_insert(0usize) += 1;
            }
            if cycle_counts.values().any(|count| *count > 1) {
                return None;
            }
            let mut occurrences = matches
                .into_iter()
                .map(|((face, cycle, start), reversed)| MeshEdgeOccurrence {
                    face,
                    cycle,
                    start,
                    reversed,
                })
                .collect::<Vec<_>>();
            occurrences
                .sort_by_key(|occurrence| (occurrence.face, occurrence.cycle, occurrence.start));
            Some(occurrences)
        })
        .collect()
}

/// Recover every exact physical-edge occurrence on the trim mesh.
///
/// Standard `u16be` rows match their interior handles and include the two
/// flanking boundary segments. FBB `u24be` rows match their complete handle
/// sequence and cover one fewer segment than handles. A result exists only
/// when exactly one trim-handle width parses the complete face chain.
#[must_use]
pub fn standard_mesh_edge_runs(bytes: &[u8]) -> Option<Vec<MeshEdgeRun>> {
    let (face_start, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, _) = parse_edge_tables(bytes, after_faces)?;
    let mut solutions = Vec::new();
    for width in [2, 3] {
        let Some(trims) = parse_trim_chain(bytes, face_start, face_count, width) else {
            continue;
        };
        let cycles = trims
            .iter()
            .map(|trim| boundary_cycles(&trim.triangles))
            .collect::<Option<Vec<_>>>()?;
        let occurrences = mesh_edge_occurrences(&edge_rows, &cycles)?;
        let mut runs = Vec::new();
        for (edge, edge_occurrences) in occurrences.iter().enumerate() {
            for occurrence in edge_occurrences {
                let cycle_len = cycles[occurrence.face][occurrence.cycle].len();
                let (start, segment_count) =
                    edge_rows[edge].boundary_span(occurrence.start, cycle_len)?;
                runs.push(MeshEdgeRun {
                    edge,
                    face: occurrence.face,
                    cycle: occurrence.cycle,
                    start,
                    segment_count,
                    reversed: occurrence.reversed,
                });
            }
        }
        runs.sort_by_key(|run| (run.face, run.cycle, run.start, run.edge));
        solutions.push(runs);
    }
    <[Vec<MeshEdgeRun>; 1]>::try_from(solutions)
        .ok()
        .map(|[runs]| runs)
}

/// One uncovered run in a trim-mesh boundary cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshBoundaryGap {
    /// Boundary-cycle ordinal within the face.
    pub cycle: usize,
    /// First uncovered boundary-segment index.
    pub start: usize,
    /// Number of consecutive uncovered boundary segments.
    pub length: usize,
}

/// Exact matched and unmatched physical-edge coverage for one trim face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshFaceCoverage {
    /// Positional face ordinal.
    pub face: usize,
    /// Maximal uncovered runs after matching every serialized edge interior.
    pub gaps: Vec<MeshBoundaryGap>,
    /// Incident physical-edge rows with no interior occurrence on this face.
    pub missing_edges: Vec<usize>,
}

/// Recover exact face-local mesh coverage without assigning unmatched edge rows
/// to gaps. A result exists only for a unique trim-handle width and when every
/// matched interior occurs on one of its two serialized incident faces.
#[must_use]
pub fn standard_mesh_face_coverage(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
) -> Option<Vec<MeshFaceCoverage>> {
    let (face_start, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, _) = parse_edge_tables(bytes, after_faces)?;
    if edge_rows.len() != edge_faces.len() {
        return None;
    }
    let mut solutions = Vec::new();
    for width in [2, 3] {
        let Some(trims) = parse_trim_chain(bytes, face_start, face_count, width) else {
            continue;
        };
        let cycles = trims
            .iter()
            .map(|trim| boundary_cycles(&trim.triangles))
            .collect::<Option<Vec<_>>>()?;
        let occurrences = mesh_edge_occurrences(&edge_rows, &cycles)?;
        if occurrences.iter().enumerate().any(|(edge, values)| {
            values
                .iter()
                .any(|occurrence| !edge_faces[edge].contains(&occurrence.face))
        }) {
            continue;
        }
        let mut coverage = Vec::with_capacity(face_count);
        for (face, face_cycles) in cycles.iter().enumerate() {
            let mut gaps = Vec::new();
            for (cycle_index, cycle) in face_cycles.iter().enumerate() {
                let mut covered = vec![false; cycle.len()];
                for (edge, values) in occurrences.iter().enumerate() {
                    for occurrence in values.iter().filter(|occurrence| {
                        occurrence.face == face && occurrence.cycle == cycle_index
                    }) {
                        let (start, segment_count) =
                            edge_rows[edge].boundary_span(occurrence.start, cycle.len())?;
                        for offset in 0..segment_count {
                            let slot = &mut covered[(start + offset) % cycle.len()];
                            if *slot {
                                return None;
                            }
                            *slot = true;
                        }
                    }
                }
                if covered.iter().all(|value| !*value) {
                    gaps.push(MeshBoundaryGap {
                        cycle: cycle_index,
                        start: 0,
                        length: cycle.len(),
                    });
                } else {
                    for start in (0..covered.len()).filter(|&index| {
                        !covered[index] && covered[(index + covered.len() - 1) % covered.len()]
                    }) {
                        let length = (0..covered.len())
                            .take_while(|offset| !covered[(start + offset) % covered.len()])
                            .count();
                        gaps.push(MeshBoundaryGap {
                            cycle: cycle_index,
                            start,
                            length,
                        });
                    }
                }
            }
            let missing_edges = edge_rows
                .iter()
                .enumerate()
                .filter_map(|(edge, _)| {
                    (edge_faces[edge].contains(&face)
                        && !occurrences[edge]
                            .iter()
                            .any(|occurrence| occurrence.face == face))
                    .then_some(edge)
                })
                .collect();
            coverage.push(MeshFaceCoverage {
                face,
                gaps,
                missing_edges,
            });
        }
        solutions.push(coverage);
    }
    <[Vec<MeshFaceCoverage>; 1]>::try_from(solutions)
        .ok()
        .map(|[coverage]| coverage)
}

/// Propagate byte-level endpoint ports through independently resolved physical
/// edge endpoint pairs. The result is rejected atomically when any port mapping
/// contradicts a resolved pair.
#[must_use]
pub fn propagate_edge_port_points(
    edge_ports: &[[u32; 2]],
    endpoint_pairs: &[Option<[usize; 2]>],
) -> Option<Vec<Option<[usize; 2]>>> {
    if edge_ports.len() != endpoint_pairs.len() {
        return None;
    }
    let mut resolved = endpoint_pairs.to_vec();
    let mut port_points = HashMap::<u32, usize>::new();

    for port in edge_ports.iter().flatten().copied().collect::<HashSet<_>>() {
        let mut intersection: Option<HashSet<usize>> = None;
        for (ports, pair) in edge_ports.iter().zip(&resolved) {
            let Some(pair) = pair else { continue };
            if ports.contains(&port) {
                let points = HashSet::from(*pair);
                intersection = Some(match intersection {
                    Some(current) => current.intersection(&points).copied().collect(),
                    None => points,
                });
            }
        }
        if let Some(points) = intersection {
            if points.len() == 1 {
                port_points.insert(port, *points.iter().next()?);
            }
        }
    }

    loop {
        let before = (port_points.len(), resolved.iter().flatten().count());
        for (ports, pair) in edge_ports.iter().zip(&resolved) {
            let Some([left, right]) = *pair else {
                continue;
            };
            match (port_points.get(&ports[0]), port_points.get(&ports[1])) {
                (Some(&point), None) if point == left => {
                    port_points.insert(ports[1], right);
                }
                (Some(&point), None) if point == right => {
                    port_points.insert(ports[1], left);
                }
                (None, Some(&point)) if point == left => {
                    port_points.insert(ports[0], right);
                }
                (None, Some(&point)) if point == right => {
                    port_points.insert(ports[0], left);
                }
                (Some(&left_point), Some(&right_point))
                    if !same_unordered_pair([left_point, right_point], [left, right]) =>
                {
                    return None;
                }
                _ => {}
            }
        }
        for (ports, pair) in edge_ports.iter().zip(&mut resolved) {
            if pair.is_none() {
                if let (Some(&left), Some(&right)) =
                    (port_points.get(&ports[0]), port_points.get(&ports[1]))
                {
                    if left != right {
                        *pair = Some([left, right]);
                    }
                }
            }
        }
        if before == (port_points.len(), resolved.iter().flatten().count()) {
            break;
        }
    }
    Some(resolved)
}

struct PortCandidateSearch<'a> {
    ports: &'a [[u32; 2]],
    candidates: &'a [Vec<[usize; 2]>],
    port_points: HashMap<u32, usize>,
    point_ports: HashMap<usize, u32>,
    edge_pairs: Vec<Option<[usize; 2]>>,
    solution: Option<Vec<[usize; 2]>>,
    states: usize,
}

impl PortCandidateSearch<'_> {
    fn compatible(&self, edge: usize, pair: [usize; 2]) -> Vec<[usize; 2]> {
        let mut oriented = vec![pair];
        if pair[0] != pair[1] {
            oriented.push([pair[1], pair[0]]);
        }
        oriented.retain(|points| {
            self.ports[edge].iter().zip(points).all(|(&port, point)| {
                self.port_points
                    .get(&port)
                    .is_none_or(|stored| *stored == *point)
                    && self
                        .point_ports
                        .get(point)
                        .is_none_or(|stored| *stored == port)
            })
        });
        oriented
    }

    fn search(&mut self) {
        // Native-port binding precedes geometric incidence fallback but can
        // still contain symmetric coordinate assignments. Ambiguity beyond
        // this bound is retained for later paths rather than partially bound.
        const MAX_STATES: usize = 1_024;
        if self.solution.is_some() || self.states >= MAX_STATES {
            return;
        }
        self.states += 1;
        let next = (0..self.ports.len())
            .filter(|edge| self.edge_pairs[*edge].is_none())
            .map(|edge| {
                let count = self.candidates[edge]
                    .iter()
                    .map(|pair| self.compatible(edge, *pair).len())
                    .sum::<usize>();
                (count, edge)
            })
            .min();
        let Some((count, edge)) = next else {
            self.solution = self.edge_pairs.iter().copied().collect();
            return;
        };
        if count == 0 {
            return;
        }
        for candidate in 0..self.candidates[edge].len() {
            for points in self.compatible(edge, self.candidates[edge][candidate]) {
                let mut inserted = Vec::new();
                for (&port, point) in self.ports[edge].iter().zip(points) {
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        self.port_points.entry(port)
                    {
                        entry.insert(point);
                        self.point_ports.insert(point, port);
                        inserted.push((port, point));
                    }
                }
                self.edge_pairs[edge] = Some(points);
                self.search();
                self.edge_pairs[edge] = None;
                for (port, point) in inserted {
                    self.port_points.remove(&port);
                    self.point_ports.remove(&point);
                }
            }
        }
    }
}

/// Bind native edge endpoint identities to coordinate rows while respecting
/// every edge's geometrically admissible unordered endpoint pairs.
#[must_use]
pub fn bind_edge_port_candidates(
    ports: &[[u32; 2]],
    candidates: &[Vec<[usize; 2]>],
) -> Option<Vec<[usize; 2]>> {
    if ports.len() != candidates.len() || candidates.iter().any(Vec::is_empty) {
        return None;
    }
    let mut search = PortCandidateSearch {
        ports,
        candidates,
        port_points: HashMap::new(),
        point_ports: HashMap::new(),
        edge_pairs: vec![None; ports.len()],
        solution: None,
        states: 0,
    };
    search.search();
    search.solution
}

fn same_unordered_pair(left: [usize; 2], right: [usize; 2]) -> bool {
    left == right || left == [right[1], right[0]]
}

fn motif_port_points(trims: &[TrimRecord], vertex_count: usize) -> Option<HashMap<u32, usize>> {
    fn columns(record: &TrimRecord) -> Option<([u32; 2], [u32; 2])> {
        Some((
            [*record.handles.first()?, *record.handles.get(1)?],
            [
                *record.handles.get(record.handles.len().checked_sub(2)?)?,
                *record.handles.last()?,
            ],
        ))
    }
    fn emit(seen: &mut HashMap<u32, usize>, handle: u32) {
        let next = seen.len();
        seen.entry(handle).or_insert(next);
    }
    fn emit_column(seen: &mut HashMap<u32, usize>, column: [u32; 2]) {
        emit(seen, column[0]);
        emit(seen, column[1]);
    }

    let mut seen = HashMap::new();
    let mut at = 0usize;
    if trims.get(0..3)?.iter().all(|record| record.kind == 0x4a) {
        let (first_a, first_b) = columns(&trims[0])?;
        let (third_a, third_b) = columns(&trims[2])?;
        for column in [third_a, first_b, first_a, third_b] {
            emit_column(&mut seen, column);
        }
        at = 3;
    }
    if trims.get(at..at + 3).is_some_and(|records| {
        records[0].kind == 0x42 && records[1].kind == 0x4a && records[2].kind == 0x42
    }) {
        let (strip0_first, strip0_last) = columns(&trims[at])?;
        let (quad_first, _) = columns(&trims[at + 1])?;
        let (strip1_first, _) = columns(&trims[at + 2])?;
        for column in [strip0_last, strip0_first, quad_first, strip1_first] {
            emit_column(&mut seen, column);
        }
        at += 3;
    }
    while trims.get(at).is_some_and(|record| record.kind == 0x4a) {
        let (first, last) = columns(&trims[at])?;
        emit_column(&mut seen, first);
        emit_column(&mut seen, last);
        at += 1;
    }
    while at < trims.len() {
        if trims.get(at..at + 3).is_some_and(|records| {
            records[0].kind == 0x42 && records[1].kind == 0x4a && records[2].kind == 0x42
        }) {
            let ([a0, b0], [a1, b1]) = columns(&trims[at])?;
            let ([c, d], [qa, qb]) = columns(&trims[at + 1])?;
            let ([e, g], [sc, sd]) = columns(&trims[at + 2])?;
            if [qa, qb] == [a0, b0] && [sc, sd] == [c, d] {
                for handle in [a1, b1, b0, d, g, a0, c, e] {
                    emit(&mut seen, handle);
                }
                at += 3;
                continue;
            }
        }
        if trims
            .get(at..at + 2)
            .is_some_and(|records| records[0].kind == 0x4a && records[1].kind == 0x4a)
            && trims[at].handles.len() >= 4
            && trims[at + 1].handles.len() >= 2
        {
            for handle in [
                trims[at + 1].handles[0],
                trims[at].handles[2],
                trims[at].handles[3],
                trims[at + 1].handles[1],
            ] {
                emit(&mut seen, handle);
            }
            at += 2;
            continue;
        }
        at += 1;
    }
    (seen.len() == vertex_count
        && seen.values().copied().collect::<HashSet<_>>().len() == vertex_count)
        .then_some(seen)
}

fn reconstruct_incidence(
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<[f64; 3]>,
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    face_count: usize,
) -> Option<StandardTopology> {
    let mut faces = Vec::with_capacity(face_count);
    for face in 0..face_count {
        let incident: Vec<usize> = edge_faces
            .iter()
            .enumerate()
            .filter_map(|(edge, adjacent)| adjacent.contains(&face).then_some(edge))
            .collect();
        let cycles = incidence_cycles(&incident, edge_points)?;
        faces.push(FaceTopology {
            boundaries: cycles
                .into_iter()
                .map(|cycle| Boundary {
                    coedges: cycle
                        .into_iter()
                        .map(|(edge_row, reversed)| {
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
                        })
                        .collect(),
                })
                .collect(),
        });
    }
    orient_face_cycles(&mut faces)?;
    Some(StandardTopology {
        faces,
        edge_rows,
        logical_vertex_count: vertex_points.len(),
        vertex_points,
    })
}

fn orient_face_cycles(faces: &mut [FaceTopology]) -> Option<()> {
    let boundary_nodes = faces
        .iter()
        .enumerate()
        .flat_map(|(face, value)| (0..value.boundaries.len()).map(move |boundary| (face, boundary)))
        .collect::<Vec<_>>();
    let node_by_boundary = boundary_nodes
        .iter()
        .enumerate()
        .map(|(node, boundary)| (*boundary, node))
        .collect::<HashMap<_, _>>();
    let mut edge_uses = HashMap::<usize, Vec<(usize, bool)>>::new();
    for (face_index, face) in faces.iter().enumerate() {
        for (boundary_index, boundary) in face.boundaries.iter().enumerate() {
            let node = node_by_boundary[&(face_index, boundary_index)];
            for coedge in &boundary.coedges {
                edge_uses
                    .entry(coedge.edge_row)
                    .or_default()
                    .push((node, coedge.reversed));
            }
        }
    }
    let mut constraints = vec![Vec::<(usize, bool)>::new(); boundary_nodes.len()];
    for uses in edge_uses.values() {
        let [(left_node, left_reversed), (right_node, right_reversed)] = uses.as_slice() else {
            return None;
        };
        let parity = left_reversed == right_reversed;
        if left_node == right_node {
            if parity {
                return None;
            }
        } else {
            constraints[*left_node].push((*right_node, parity));
            constraints[*right_node].push((*left_node, parity));
        }
    }

    let mut flips = vec![None; boundary_nodes.len()];
    for root in 0..boundary_nodes.len() {
        if flips[root].is_some() {
            continue;
        }
        flips[root] = Some(false);
        let mut stack = vec![root];
        while let Some(face) = stack.pop() {
            let flip = flips[face]?;
            for &(neighbor, parity) in &constraints[face] {
                let required = flip ^ parity;
                match flips[neighbor] {
                    Some(existing) if existing != required => return None,
                    Some(_) => {}
                    None => {
                        flips[neighbor] = Some(required);
                        stack.push(neighbor);
                    }
                }
            }
        }
    }
    for ((face_index, boundary_index), flip) in boundary_nodes.into_iter().zip(flips) {
        if flip? {
            let boundary = &mut faces[face_index].boundaries[boundary_index];
            boundary.coedges.reverse();
            for coedge in &mut boundary.coedges {
                coedge.reversed = !coedge.reversed;
                std::mem::swap(&mut coedge.start_vertex, &mut coedge.end_vertex);
            }
        }
    }
    Some(())
}

fn incidence_cycles(
    incident: &[usize],
    edge_points: &[[usize; 2]],
) -> Option<Vec<Vec<(usize, bool)>>> {
    if incident.is_empty() {
        return None;
    }
    let mut at_vertex = HashMap::<usize, Vec<usize>>::new();
    for &edge in incident {
        let [start, end] = edge_points[edge];
        if start == end {
            return None;
        }
        at_vertex.entry(start).or_default().push(edge);
        at_vertex.entry(end).or_default().push(edge);
    }
    if at_vertex.values().any(|edges| edges.len() != 2) {
        return None;
    }
    let mut unseen: HashSet<usize> = incident.iter().copied().collect();
    let mut cycles = Vec::new();
    while let Some(&first) = unseen.iter().min() {
        let start_vertex = edge_points[first][0];
        let mut vertex = start_vertex;
        let mut edge = first;
        let mut cycle = Vec::new();
        loop {
            if !unseen.remove(&edge) {
                return None;
            }
            let endpoints = edge_points[edge];
            let reversed = endpoints[1] == vertex;
            if !reversed && endpoints[0] != vertex {
                return None;
            }
            vertex = if reversed { endpoints[0] } else { endpoints[1] };
            cycle.push((edge, reversed));
            if vertex == start_vertex {
                break;
            }
            edge = *at_vertex
                .get(&vertex)?
                .iter()
                .find(|candidate| unseen.contains(candidate))?;
        }
        cycles.push(cycle);
    }
    Some(cycles)
}

/// Parses the FBB-only spine. Its edge rows and trim handles are `u24be`; the
/// following counted `05 08 01` table supplies vertex coordinates.
#[must_use]
pub fn parse_fbb(bytes: &[u8]) -> Option<StandardTopology> {
    let (face_start, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, _) = parse_fbb_edge_tables(bytes, after_faces)?;
    let vertex_points = standard_vertex_points(bytes)?;
    let trims = parse_trim_chain(bytes, face_start, face_count, 3)?;
    reconstruct(edge_rows, vertex_points, &trims)
}

/// Parse an FBB-only spine and apply its global native endpoint identities.
/// This closes the cross-face quotient independently of face-local trim-handle
/// names.
#[must_use]
pub fn parse_fbb_with_native_vertices(
    bytes: &[u8],
    edge_ports: &[[u32; 2]],
) -> Option<StandardTopology> {
    parse_fbb(bytes)?.with_native_edge_vertices(edge_ports)
}

fn reconstruct(
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<[f64; 3]>,
    trims: &[TrimRecord],
) -> Option<StandardTopology> {
    let mut union = UnionFind::new(edge_rows.len() * 2);
    let mut faces = Vec::with_capacity(trims.len());
    for trim in trims {
        let cycles = boundary_cycles(&trim.triangles)?;
        let mut boundaries = Vec::with_capacity(cycles.len());
        for cycle in cycles {
            boundaries.push(cover_cycle(&cycle, &edge_rows, &mut union)?);
        }
        faces.push(FaceTopology { boundaries });
    }

    let mut roots = HashMap::new();
    for node in 0..union.len() {
        let root = union.find(node);
        let next = roots.len();
        roots.entry(root).or_insert(next);
    }
    for face in &mut faces {
        for boundary in &mut face.boundaries {
            for coedge in &mut boundary.coedges {
                coedge.start_vertex = roots[&union.find(coedge.start_vertex)];
                coedge.end_vertex = roots[&union.find(coedge.end_vertex)];
            }
        }
    }

    Some(StandardTopology {
        faces,
        edge_rows,
        vertex_points,
        logical_vertex_count: roots.len(),
    })
}

fn parse_fbb_edge_tables(bytes: &[u8], mut position: usize) -> Option<(Vec<EdgeRow>, usize)> {
    let mut rows = Vec::new();
    let mut table_count = 0;
    loop {
        if bytes.get(position) != Some(&0x01) {
            return None;
        }
        let kind = *bytes.get(position + 1)?;
        if !matches!(kind, 1 | 2) {
            return None;
        }
        position += 2;
        let count = parse_count(bytes, &mut position)?;
        for _ in 0..count {
            if bytes.get(position) != Some(&0x02) {
                return None;
            }
            position += 1;
            let arity = parse_count(bytes, &mut position)?;
            if arity < 2 {
                return None;
            }
            let mut handles = Vec::with_capacity(arity);
            for _ in 0..arity {
                handles.push(u32::from_be_bytes([
                    0,
                    *bytes.get(position)?,
                    *bytes.get(position + 1)?,
                    *bytes.get(position + 2)?,
                ]));
                position += 3;
            }
            rows.push(EdgeRow {
                kind,
                handles,
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            });
        }
        table_count += 1;
        if bytes.get(position..)?.starts_with(&EDGE_DELIMITER) {
            position += EDGE_DELIMITER.len();
        } else {
            return None;
        }
        if bytes.get(position..position + 2) == Some(&[0x01, 0x06]) {
            break;
        }
    }
    (table_count == 2).then_some((rows, position))
}

fn largest_fbb_run(bytes: &[u8]) -> Option<(usize, usize, usize)> {
    let mut best = None;
    let mut position = 0;
    while position + 8 <= bytes.len() {
        if bytes[position..].starts_with(&FBB_ROW) {
            let start = position;
            let mut count = 0;
            while position + 8 <= bytes.len() && bytes[position..].starts_with(&FBB_ROW) {
                count += 1;
                position += 8;
            }
            if best.is_none_or(|(_, best_count, _)| count > best_count) {
                best = Some((start, count, position));
            }
        } else {
            position += 1;
        }
    }
    best
}

fn parse_count(bytes: &[u8], position: &mut usize) -> Option<usize> {
    let first = *bytes.get(*position)?;
    *position += 1;
    if first != 0xff {
        return Some(usize::from(first));
    }
    let value = u32::from_le_bytes(bytes.get(*position..*position + 4)?.try_into().ok()?);
    *position += 4;
    usize::try_from(value).ok()
}

fn parse_edge_tables(bytes: &[u8], position: usize) -> Option<(Vec<EdgeRow>, usize)> {
    if let Some(result) = parse_edge_tables_at(bytes, position) {
        return Some(result);
    }
    parse_fbb_edge_tables(bytes, position)
        .filter(|(_, vertex_header)| parse_vertex_table(bytes, *vertex_header).is_some())
}

fn parse_edge_tables_at(bytes: &[u8], mut position: usize) -> Option<(Vec<EdgeRow>, usize)> {
    let mut rows = Vec::new();
    loop {
        if bytes.get(position) != Some(&0x01) {
            return None;
        }
        let kind = *bytes.get(position + 1)?;
        if !matches!(kind, 0x01 | 0x02) {
            return None;
        }
        position += 2;
        let count = parse_count(bytes, &mut position)?;
        for _ in 0..count {
            if bytes.get(position) != Some(&0x02) {
                return None;
            }
            let arity = usize::from(*bytes.get(position + 1)?);
            position += 2;
            if arity < 2 {
                return None;
            }
            let mut handles = Vec::with_capacity(arity);
            for _ in 0..arity {
                handles.push(u32::from(u16::from_be_bytes(
                    bytes.get(position..position + 2)?.try_into().ok()?,
                )));
                position += 2;
            }
            rows.push(EdgeRow {
                kind,
                handles,
                boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
            });
        }
        let mut saw_delimiter = false;
        while bytes.get(position..)?.starts_with(&EDGE_DELIMITER) {
            saw_delimiter = true;
            position += EDGE_DELIMITER.len();
        }
        if !saw_delimiter {
            return None;
        }
        if bytes.get(position..position + 2) == Some(&[0x01, 0x06]) {
            break;
        }
    }
    Some((rows, position))
}

fn parse_vertex_table(bytes: &[u8], mut position: usize) -> Option<Vec<[f64; 3]>> {
    if bytes.get(position..position + 2)? != [0x01, 0x06] {
        return None;
    }
    position += 2;
    let count = parse_count(bytes, &mut position)?;
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(position..position + 3)? != [0x05, 0x08, 0x01] {
            return None;
        }
        position += 3;
        let mut point = [0.0; 3];
        for coordinate in &mut point {
            let value = f32::from_le_bytes(bytes.get(position..position + 4)?.try_into().ok()?);
            if !value.is_finite() {
                return None;
            }
            *coordinate = f64::from(value);
            position += 4;
        }
        points.push(point);
    }
    Some(points)
}

fn parse_trim_records(bytes: &[u8]) -> Vec<TrimRecord> {
    parse_trim_records_with_width(bytes, 2)
}

fn parse_trim_records_with_width(bytes: &[u8], width: usize) -> Vec<TrimRecord> {
    let mut records = Vec::new();
    let mut position = 0;
    while position + 2 <= bytes.len() {
        if let Some(record) = parse_trim_record(bytes, position, width) {
            position = record.end;
            records.push(record);
        } else {
            position += 1;
        }
    }
    records
}

fn parse_trim_chain(
    bytes: &[u8],
    end: usize,
    record_count: usize,
    width: usize,
) -> Option<Vec<TrimRecord>> {
    fn walk(
        predecessors: &HashMap<usize, Vec<(usize, TrimRecord)>>,
        end: usize,
        remaining: usize,
        reversed: &mut Vec<TrimRecord>,
        solutions: &mut Vec<Vec<TrimRecord>>,
    ) {
        if solutions.len() > 1 {
            return;
        }
        if remaining == 0 {
            let mut records = reversed.clone();
            records.reverse();
            solutions.push(records);
            return;
        }
        let Some(records) = predecessors.get(&end) else {
            return;
        };
        for (start, record) in records {
            reversed.push(record.clone());
            walk(predecessors, *start, remaining - 1, reversed, solutions);
            reversed.pop();
        }
    }

    let prefix = bytes.get(..end)?;
    let mut predecessors = HashMap::<usize, Vec<(usize, TrimRecord)>>::new();
    for start in 0..prefix.len() {
        if let Some(record) = parse_trim_record(prefix, start, width) {
            predecessors
                .entry(record.end)
                .or_default()
                .push((start, record));
        }
    }

    let mut solutions = Vec::new();
    walk(
        &predecessors,
        end,
        record_count,
        &mut Vec::with_capacity(record_count),
        &mut solutions,
    );
    <[Vec<TrimRecord>; 1]>::try_from(solutions)
        .ok()
        .map(|[records]| records)
}

fn parse_trim_record(bytes: &[u8], start: usize, width: usize) -> Option<TrimRecord> {
    if bytes.get(start) != Some(&0x01) {
        return None;
    }
    let kind = *bytes.get(start + 1)?;
    if !TRIM_KINDS.contains(&kind) {
        return None;
    }
    let mask = kind & 0x0f;
    let mut position = start + 2;
    let a = if mask & 1 != 0 {
        parse_count(bytes, &mut position)?
    } else {
        0
    };
    let b = if mask & 2 != 0 {
        parse_count(bytes, &mut position)?
    } else {
        0
    };
    let c = if mask & 4 != 0 {
        parse_count(bytes, &mut position)?
    } else {
        0
    };
    if bytes.get(position) != Some(&0xff) {
        return None;
    }
    position += 1;
    let handle_count = usize::try_from(u32::from_le_bytes(
        bytes.get(position..position + 4)?.try_into().ok()?,
    ))
    .ok()?;
    position += 4;
    if !(1..=500_000).contains(&handle_count) {
        return None;
    }
    let frame_vector = if mask & 8 != 0 {
        let components = [
            f64::from(f32::from_le_bytes(
                bytes.get(position..position + 4)?.try_into().ok()?,
            )),
            f64::from(f32::from_le_bytes(
                bytes.get(position + 4..position + 8)?.try_into().ok()?,
            )),
            f64::from(f32::from_le_bytes(
                bytes.get(position + 8..position + 12)?.try_into().ok()?,
            )),
        ];
        position += 12;
        let norm2 = components.iter().map(|value| value * value).sum::<f64>();
        (components.iter().all(|value| value.is_finite()) && (norm2 - 1.0).abs() < 2e-4)
            .then_some(components)
    } else {
        None
    };

    let legacy_42 = kind == 0x42 && b == 2 && width == 2;
    let mut lengths = Vec::with_capacity(b + c);
    if !legacy_42 {
        for _ in 0..b + c {
            lengths.push(parse_count(bytes, &mut position)?);
        }
        if 3usize.checked_mul(a)?.checked_add(lengths.iter().sum())? != handle_count {
            return None;
        }
    }
    let stored_count = handle_count + usize::from(legacy_42);
    let mut handles = Vec::with_capacity(stored_count);
    for _ in 0..stored_count {
        let handle = match width {
            2 => u32::from(u16::from_be_bytes(
                bytes.get(position..position + 2)?.try_into().ok()?,
            )),
            3 => u32::from_be_bytes([
                0,
                *bytes.get(position)?,
                *bytes.get(position + 1)?,
                *bytes.get(position + 2)?,
            ]),
            _ => return None,
        };
        handles.push(handle);
        position += width;
    }
    if legacy_42 {
        let packed = *handles.first()?;
        lengths = vec![(packed >> 8) as usize, (packed & 0xff) as usize];
        handles.remove(0);
        if lengths.iter().sum::<usize>() != handle_count {
            return None;
        }
    }

    let triangles = packet_triangles(a, b, c, &lengths, &handles)?;
    Some(TrimRecord {
        triangles,
        frame_vector,
        handles,
        kind,
        end: position,
    })
}

fn packet_triangles(
    independent: usize,
    strips: usize,
    fans: usize,
    lengths: &[usize],
    handles: &[u32],
) -> Option<Vec<[u32; 3]>> {
    let mut triangles = Vec::new();
    for triple in handles.get(..3 * independent)?.chunks_exact(3) {
        triangles.push([triple[0], triple[1], triple[2]]);
    }
    let mut position = 3 * independent;
    for &length in lengths.get(..strips)? {
        let strip = handles.get(position..position + length)?;
        for index in 0..length.saturating_sub(2) {
            triangles.push(if index % 2 == 0 {
                [strip[index], strip[index + 1], strip[index + 2]]
            } else {
                [strip[index + 1], strip[index], strip[index + 2]]
            });
        }
        position += length;
    }
    for &length in lengths.get(strips..strips + fans)? {
        let fan = handles.get(position..position + length)?;
        for index in 1..length.saturating_sub(1) {
            triangles.push([fan[0], fan[index], fan[index + 1]]);
        }
        position += length;
    }
    (position == handles.len()).then_some(triangles)
}

fn boundary_cycles(triangles: &[[u32; 3]]) -> Option<Vec<Vec<u32>>> {
    let mut counts = HashMap::<(u32, u32), usize>::new();
    for &[a, b, c] in triangles {
        for edge in [(a, b), (b, c), (c, a)] {
            *counts.entry(edge).or_default() += 1;
        }
    }
    let undirected: HashSet<(u32, u32)> = counts
        .keys()
        .map(|&(start, end)| (start.min(end), start.max(end)))
        .collect();
    for (low, high) in undirected {
        if low == high {
            return None;
        }
        let forward = counts.get(&(low, high)).copied().unwrap_or(0);
        let reverse = counts.get(&(high, low)).copied().unwrap_or(0);
        if !matches!((forward, reverse), (1, 0 | 1) | (0, 1)) {
            return None;
        }
    }
    let mut successors = HashMap::new();
    for (&(start, end), &count) in &counts {
        if count > 0
            && counts.get(&(end, start)).copied().unwrap_or(0) == 0
            && successors.insert(start, end).is_some()
        {
            return None;
        }
    }
    let mut seen = HashSet::new();
    let mut cycles = Vec::new();
    for &start in successors.keys() {
        if seen.contains(&start) {
            continue;
        }
        let mut cycle = vec![start];
        seen.insert(start);
        let mut current = *successors.get(&start)?;
        while current != start {
            if !seen.insert(current) {
                return None;
            }
            cycle.push(current);
            current = *successors.get(&current)?;
        }
        let minimum = cycle
            .iter()
            .enumerate()
            .min_by_key(|(_, handle)| *handle)
            .map(|(index, _)| index)?;
        cycle.rotate_left(minimum);
        cycles.push(cycle);
    }
    cycles.sort();
    (!cycles.is_empty()).then_some(cycles)
}

fn cover_cycle(cycle: &[u32], rows: &[EdgeRow], union: &mut UnionFind) -> Option<Boundary> {
    cover_cycle_by_rows(cycle, rows, union)
}

fn cover_cycle_by_rows(cycle: &[u32], rows: &[EdgeRow], union: &mut UnionFind) -> Option<Boundary> {
    let length = cycle.len();
    let mut matches = Vec::new();
    for (edge_row, row) in rows.iter().enumerate() {
        let Some(pattern) = row.boundary_pattern() else {
            continue;
        };
        let mut row_matches = Vec::new();
        for start in 0..length {
            let forward = pattern
                .iter()
                .enumerate()
                .all(|(offset, handle)| cycle[(start + offset) % length] == *handle);
            let reversed = pattern
                .iter()
                .rev()
                .enumerate()
                .all(|(offset, handle)| cycle[(start + offset) % length] == *handle);
            if forward {
                row_matches.push((start, false));
            } else if reversed {
                row_matches.push((start, true));
            }
        }
        if row_matches.len() == 1 {
            let (start, reversed) = row_matches[0];
            let (boundary_start, segment_count) = row.boundary_span(start, length)?;
            matches.push((boundary_start, segment_count, edge_row, reversed));
        } else if !row_matches.is_empty() {
            return None;
        }
    }
    if matches.is_empty() {
        return None;
    }

    let mut coverage = vec![0u8; length];
    for &(start, edge_count, _, _) in &matches {
        for offset in 0..edge_count {
            coverage[(start + offset) % length] =
                coverage[(start + offset) % length].checked_add(1)?;
        }
    }
    if coverage.iter().any(|count| *count != 1) {
        return None;
    }
    matches.sort_by_key(|entry| entry.0 % length);
    let mut corner_nodes = HashMap::new();
    for &(start, edge_count, _, _) in &matches {
        let end = (start + edge_count) % length;
        corner_nodes
            .entry(start % length)
            .or_insert_with(|| union.push());
        corner_nodes.entry(end).or_insert_with(|| union.push());
    }
    let mut coedges = Vec::with_capacity(matches.len());
    for (start, edge_count, edge_row, reversed) in matches {
        let start_node = corner_nodes[&(start % length)];
        let end_node = corner_nodes[&((start + edge_count) % length)];
        let edge_start = edge_row * 2;
        let edge_end = edge_start + 1;
        if reversed {
            union.union(edge_end, start_node);
            union.union(edge_start, end_node);
        } else {
            union.union(edge_start, start_node);
            union.union(edge_end, end_node);
        }
        coedges.push(CoedgeUse {
            edge_row,
            reversed,
            start_vertex: start_node,
            end_vertex: end_node,
        });
    }
    Some(Boundary { coedges })
}

#[derive(Debug)]
struct UnionFind {
    parents: Vec<usize>,
}

impl UnionFind {
    fn new(length: usize) -> Self {
        Self {
            parents: (0..length).collect(),
        }
    }

    fn len(&self) -> usize {
        self.parents.len()
    }

    fn push(&mut self) -> usize {
        let index = self.parents.len();
        self.parents.push(index);
        index
    }

    fn find(&mut self, node: usize) -> usize {
        let parent = self.parents[node];
        if parent != node {
            self.parents[node] = self.find(parent);
        }
        self.parents[node]
    }

    fn union(&mut self, left: usize, right: usize) {
        let left = self.find(left);
        let right = self.find(right);
        if left != right {
            self.parents[right] = left;
        }
    }
}

#[cfg(test)]
mod motif_tests {
    use super::{
        bind_edge_port_candidates, motif_port_points, parse_trim_chain, propagate_edge_port_points,
        reconstruct_incidence, reconstruct_incidence_candidates, EdgeBoundaryLayout, EdgeRow,
        TrimRecord,
    };

    fn triangle_packet(handles: [u16; 3]) -> Vec<u8> {
        let mut bytes = vec![0x01, 0x41, 0x01, 0xff, 0x03, 0x00, 0x00, 0x00];
        for handle in handles {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn trim_chain_requires_exact_packet_count_and_boundary_landing() {
        let incidental = triangle_packet([90, 91, 92]);
        let first = triangle_packet([0, 1, 2]);
        let second = triangle_packet([3, 4, 5]);
        let mut bytes = incidental;
        bytes.push(0);
        bytes.extend_from_slice(&first);
        bytes.extend_from_slice(&second);

        let records = parse_trim_chain(&bytes, bytes.len(), 2, 2).expect("exact chain");
        assert_eq!(records[0].handles, [0, 1, 2]);
        assert_eq!(records[1].handles, [3, 4, 5]);
        assert!(parse_trim_chain(&bytes, bytes.len(), 2, 3).is_none());
    }

    fn trim(kind: u8, handles: [u32; 4]) -> TrimRecord {
        TrimRecord {
            triangles: Vec::new(),
            frame_vector: None,
            handles: handles.to_vec(),
            kind,
            end: 0,
        }
    }

    #[test]
    fn allocation_program_replays_seed_tooth_and_transition() {
        let trims = [
            trim(0x4a, [0, 1, 2, 3]),
            trim(0x4a, [10, 11, 12, 13]),
            trim(0x4a, [20, 21, 22, 23]),
            trim(0x42, [30, 31, 32, 33]),
            trim(0x4a, [40, 41, 30, 31]),
            trim(0x42, [50, 51, 40, 41]),
            trim(0x4a, [60, 61, 62, 63]),
        ];
        let points = motif_port_points(&trims, 20).expect("complete motif allocation");
        let order = [
            20, 21, 2, 3, 0, 1, 22, 23, 32, 33, 30, 31, 40, 41, 50, 51, 60, 61, 62, 63,
        ];
        for (index, handle) in order.into_iter().enumerate() {
            assert_eq!(points[&handle], index);
        }
    }

    #[test]
    fn endpoint_incidence_builds_oriented_tetrahedron_cycles() {
        let rows: Vec<_> = (0..6)
            .map(|edge| EdgeRow {
                kind: 1,
                handles: vec![edge * 2, edge * 2 + 1],
                boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
            })
            .collect();
        let points = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let edge_faces = [[0, 1], [0, 2], [0, 3], [1, 3], [1, 2], [2, 3]];
        let edge_points = [[0, 1], [1, 2], [2, 0], [0, 3], [3, 1], [2, 3]];
        let topology = reconstruct_incidence(rows, points, &edge_faces, &edge_points, 4)
            .expect("closed oriented incidence");
        assert_eq!(topology.face_count(), 4);
        assert!(topology
            .faces()
            .iter()
            .all(|face| { face.boundaries.len() == 1 && face.boundaries[0].coedges.len() == 3 }));
        let mut uses = vec![Vec::new(); 6];
        for face in topology.faces() {
            for coedge in &face.boundaries[0].coedges {
                uses[coedge.edge_row].push(coedge.reversed);
            }
        }
        assert!(uses
            .iter()
            .all(|senses| senses == &[false, true] || senses == &[true, false]));
    }

    #[test]
    fn endpoint_candidate_search_selects_a_face_closing_assignment() {
        let rows: Vec<_> = (0..6)
            .map(|edge| EdgeRow {
                kind: 1,
                handles: vec![edge * 2, edge * 2 + 1],
                boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
            })
            .collect();
        let points = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let edge_faces = [[0, 1], [0, 2], [0, 3], [1, 3], [1, 2], [2, 3]];
        let candidates = vec![
            vec![[0, 2], [0, 1]],
            vec![[1, 2]],
            vec![[0, 2]],
            vec![[0, 3]],
            vec![[1, 3]],
            vec![[2, 3]],
        ];
        let topology =
            reconstruct_incidence_candidates(&rows, &points, &edge_faces, &candidates, 4)
                .expect("unique face-closing endpoint assignment");
        assert_eq!(topology.edge_vertices().expect("edge vertices")[0], [0, 1]);
    }

    #[test]
    fn radial_orientation_solves_each_face_boundary_independently() {
        let rows = (0..18)
            .map(|edge| EdgeRow {
                kind: 1,
                handles: vec![edge * 2, edge * 2 + 1],
                boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
            })
            .collect();
        let points = (0..12).map(|point| [f64::from(point), 0.0, 0.0]).collect();
        let edge_faces = [
            [8, 2],
            [8, 3],
            [4, 0],
            [7, 0],
            [4, 1],
            [7, 1],
            [2, 4],
            [3, 4],
            [7, 6],
            [7, 5],
            [8, 6],
            [8, 5],
            [1, 0],
            [1, 0],
            [3, 2],
            [3, 2],
            [6, 5],
            [6, 5],
        ];
        let edge_points = [
            [0, 1],
            [0, 1],
            [2, 4],
            [3, 5],
            [2, 4],
            [3, 5],
            [6, 7],
            [6, 7],
            [8, 9],
            [8, 9],
            [10, 11],
            [10, 11],
            [2, 3],
            [4, 5],
            [0, 6],
            [1, 7],
            [8, 10],
            [9, 11],
        ];
        let topology = reconstruct_incidence(rows, points, &edge_faces, &edge_points, 9)
            .expect("orientable multi-boundary shell");
        assert_eq!(topology.faces()[4].boundaries.len(), 2);
        let mut uses = vec![Vec::new(); 18];
        for face in topology.faces() {
            for boundary in &face.boundaries {
                for coedge in &boundary.coedges {
                    uses[coedge.edge_row].push(coedge.reversed);
                }
            }
        }
        assert!(uses
            .iter()
            .all(|senses| senses == &[false, true] || senses == &[true, false]));
    }

    #[test]
    fn endpoint_ports_propagate_resolved_pairs_to_unresolved_edges() {
        let ports = [[10, 11], [11, 12], [12, 13], [13, 10]];
        let pairs = [Some([0, 1]), Some([1, 2]), None, Some([3, 0])];
        assert_eq!(
            propagate_edge_port_points(&ports, &pairs),
            Some(vec![Some([0, 1]), Some([1, 2]), Some([2, 3]), Some([3, 0]),])
        );
    }

    #[test]
    fn endpoint_ports_reject_contradictory_pair_constraints() {
        let ports = [[10, 11], [11, 12], [12, 10]];
        let pairs = [Some([0, 1]), Some([1, 2]), Some([0, 3])];
        assert_eq!(propagate_edge_port_points(&ports, &pairs), None);
    }

    #[test]
    fn native_edge_identities_bind_ambiguous_coordinate_pairs() {
        let ports = [[10, 11], [12, 13], [10, 12], [11, 13]];
        let candidates = [
            vec![[0, 1]],
            vec![[2, 3]],
            vec![[0, 2], [1, 3]],
            vec![[1, 3], [0, 2]],
        ];
        assert_eq!(
            bind_edge_port_candidates(&ports, &candidates),
            Some(vec![[0, 1], [2, 3], [0, 2], [1, 3]])
        );
    }
}
