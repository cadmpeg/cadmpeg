//! Mesh missing-edge enumeration for standard nested B-rep streams.
//!
//! Recovers unmatched edge-row placements against serialized face coverage.

use crate::families::standard::fbb::{
    boundary_cycles, largest_fbb_run, parse_edge_tables, parse_edge_tables_scoped_at,
    parse_fbb_edge_tables, parse_trim_chain, parse_vertex_table,
};
use crate::families::standard::topology::{incidence_cycles, EdgeRow, TrimRecord};
#[cfg(test)]
use crate::families::standard::topology::{reconstruct_mesh_selection, StandardTopology};
use crate::solve::mesh_quotient::{MeshConstraintBudget, MAX_MESH_CONSTRAINT_OPERATIONS};
use crate::solve::UnionFind;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

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
    if let Some((edge_rows, _, _)) = parse_edge_tables_scoped_at(bytes, after_faces) {
        return edge_rows
            .iter()
            .enumerate()
            .map(|(edge, row)| {
                row.handles.first().zip(row.handles.last())?;
                let start = edge.checked_mul(2)?;
                Some([u32::try_from(start).ok()?, u32::try_from(start + 1).ok()?])
            })
            .collect();
    }
    let (edge_rows, scopes, _, _) = parse_fbb_edge_tables(bytes, after_faces)?;
    let mut identity_by_handle = HashMap::new();
    edge_rows
        .iter()
        .zip(scopes)
        .map(|(row, scope)| {
            let mut pair = [0; 2];
            for (port, handle) in [*row.handles.first()?, *row.handles.last()?]
                .into_iter()
                .enumerate()
            {
                let next = u32::try_from(identity_by_handle.len()).ok()?;
                pair[port] = *identity_by_handle.entry((scope, handle)).or_insert(next);
            }
            Some(pair)
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
    let local_ports = standard_edge_port_identities(bytes)?;
    let mut solutions = Vec::new();
    for width in [1, 2, 3] {
        let Some(trims) = parse_trim_chain(bytes, face_start, face_count, width) else {
            continue;
        };
        let cycles = trims
            .iter()
            .map(|trim| boundary_cycles(&trim.triangles))
            .collect::<Option<Vec<_>>>()?;
        let occurrences = mesh_edge_occurrences(&edge_rows, &cycles)?;
        let mut union = UnionFind::new(edge_rows.len() * 2);
        let mut node_by_identity = HashMap::new();
        for (edge, ports) in local_ports.iter().enumerate() {
            for (side, identity) in ports.iter().copied().enumerate() {
                let node = edge * 2 + side;
                if let Some(previous) = node_by_identity.insert(identity, node) {
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
    for width in [1, 2, 3] {
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

/// Complete repeated standard edge-face slots from exact trim-boundary
/// occurrences. Rows without two distinct matched face occurrences retain
/// their serialized slots for incidence closure.
#[must_use]
pub(crate) fn resolve_standard_edge_faces(
    bytes: &[u8],
    serialized: &[[usize; 2]],
) -> Option<Vec<[usize; 2]>> {
    let Some(runs) = standard_mesh_edge_runs(bytes) else {
        return Some(serialized.to_vec());
    };
    resolve_edge_faces_from_runs(serialized, &runs)
}

pub(crate) fn unique_duplicate_face_assignment<F>(
    serialized: &[[usize; 2]],
    allowed_faces: &[Vec<usize>],
    face_count: usize,
    mut valid: F,
) -> Option<Vec<[usize; 2]>>
where
    F: FnMut(&[[usize; 2]]) -> bool,
{
    const MAX_STATES: usize = 4_096;

    pub(crate) fn search<F>(
        branches: &[(usize, Vec<usize>)],
        at: usize,
        assignment: &mut [[usize; 2]],
        states: &mut usize,
        exhausted: &mut bool,
        solutions: &mut Vec<Vec<[usize; 2]>>,
        valid: &mut F,
    ) where
        F: FnMut(&[[usize; 2]]) -> bool,
    {
        if *exhausted || solutions.len() > 1 {
            return;
        }
        if at == branches.len() {
            if valid(assignment) && !solutions.iter().any(|solution| solution == assignment) {
                solutions.push(assignment.to_vec());
            }
            return;
        }
        if *states >= MAX_STATES {
            *exhausted = true;
            return;
        }
        *states += 1;
        let (edge, options) = &branches[at];
        for &face in options {
            assignment[*edge][1] = face;
            search(
                branches,
                at + 1,
                assignment,
                states,
                exhausted,
                solutions,
                valid,
            );
            if *exhausted || solutions.len() > 1 {
                return;
            }
        }
    }

    if serialized.len() != allowed_faces.len()
        || serialized.iter().flatten().any(|face| *face >= face_count)
        || allowed_faces
            .iter()
            .flatten()
            .any(|face| *face >= face_count)
    {
        return None;
    }
    let unresolved = serialized
        .iter()
        .enumerate()
        .filter_map(|(edge, faces)| (faces[0] == faces[1]).then_some(edge))
        .collect::<Vec<_>>();
    if unresolved.is_empty() {
        return Some(serialized.to_vec());
    }
    let mut assignment = serialized.to_vec();
    let mut branches = Vec::new();
    for edge in unresolved {
        let mut options = allowed_faces[edge]
            .iter()
            .copied()
            .filter(|face| *face != assignment[edge][0])
            .collect::<Vec<_>>();
        options.sort_unstable();
        options.dedup();
        match options.as_slice() {
            [] => return None,
            [face] => assignment[edge][1] = *face,
            _ => branches.push((edge, options)),
        }
    }
    branches.sort_unstable_by_key(|(edge, options)| (options.len(), *edge));
    let mut states = 0;
    let mut exhausted = false;
    let mut solutions = Vec::new();
    search(
        &branches,
        0,
        &mut assignment,
        &mut states,
        &mut exhausted,
        &mut solutions,
        &mut valid,
    );
    (!exhausted)
        .then(|| <[Vec<[usize; 2]>; 1]>::try_from(solutions).ok())
        .flatten()
        .map(|[solution]| solution)
}

/// Complete repeated standard edge-face slots when carrier incidence and a
/// complete trim-boundary partition select one common assignment.
pub(crate) fn resolve_standard_duplicate_edge_faces(
    bytes: &[u8],
    serialized: &[[usize; 2]],
    allowed_faces: &[Vec<usize>],
) -> Option<Vec<[usize; 2]>> {
    let face_count = largest_fbb_run(bytes)?.1;
    unique_duplicate_face_assignment(serialized, allowed_faces, face_count, |assignment| {
        standard_mesh_boundary_assignments(bytes, assignment, None).is_some()
    })
}

/// Test whether one face can select endpoint pairs that form closed cycles.
/// The search is bounded independently of the global incidence solver.
pub(crate) fn face_endpoint_candidates_close(
    edge_faces: &[[usize; 2]],
    candidates: &[Vec<[usize; 2]>],
    face: usize,
) -> bool {
    const MAX_STATES: usize = 65_536;

    fn pair_fits(degrees: &HashMap<usize, u8>, pair: [usize; 2]) -> bool {
        let left = usize::from(degrees.get(&pair[0]).copied().unwrap_or(0));
        let right = usize::from(degrees.get(&pair[1]).copied().unwrap_or(0));
        if pair[0] == pair[1] {
            left + 2 <= 2
        } else {
            left < 2 && right < 2
        }
    }

    pub(crate) fn adjust(degrees: &mut HashMap<usize, u8>, pair: [usize; 2], increase: bool) {
        for point in pair {
            if increase {
                *degrees.entry(point).or_default() += 1;
            } else {
                let remove = if let Some(degree) = degrees.get_mut(&point) {
                    *degree -= 1;
                    *degree == 0
                } else {
                    false
                };
                if remove {
                    degrees.remove(&point);
                }
            }
        }
    }

    pub(crate) fn search(
        all_edges: &[usize],
        branches: &[(usize, Vec<[usize; 2]>)],
        at: usize,
        selected: &mut [[usize; 2]],
        degrees: &mut HashMap<usize, u8>,
        states: &mut usize,
    ) -> Option<bool> {
        if at == branches.len() {
            return Some(incidence_cycles(all_edges, selected).is_some());
        }
        let (edge, candidates) = &branches[at];
        let viable = candidates
            .iter()
            .copied()
            .filter(|pair| pair_fits(degrees, *pair))
            .collect::<Vec<_>>();
        if viable.len() > 1 {
            if *states >= MAX_STATES {
                return None;
            }
            *states += 1;
        }
        for pair in viable {
            adjust(degrees, pair, true);
            selected[*edge] = pair;
            if search(all_edges, branches, at + 1, selected, degrees, states)? {
                return Some(true);
            }
            adjust(degrees, pair, false);
        }
        Some(false)
    }

    if edge_faces.len() != candidates.len() {
        return false;
    }
    let edges = edge_faces
        .iter()
        .enumerate()
        .filter_map(|(edge, faces)| faces.contains(&face).then_some(edge))
        .collect::<Vec<_>>();
    if edges.is_empty() {
        return false;
    }
    let mut selected = vec![[0; 2]; candidates.len()];
    let mut degrees = HashMap::new();
    let mut branches = Vec::new();
    for &edge in &edges {
        let mut options = candidates[edge].clone();
        for pair in &mut options {
            pair.sort_unstable();
        }
        options.sort_unstable();
        options.dedup();
        match options.as_slice() {
            [] => return false,
            [pair] => {
                if !pair_fits(&degrees, *pair) {
                    return false;
                }
                selected[edge] = *pair;
                adjust(&mut degrees, *pair, true);
            }
            _ => branches.push((edge, options)),
        }
    }
    branches.sort_unstable_by_key(|(edge, options)| (options.len(), *edge));
    search(&edges, &branches, 0, &mut selected, &mut degrees, &mut 0).unwrap_or(false)
}

pub(crate) fn resolve_edge_faces_from_runs(
    serialized: &[[usize; 2]],
    runs: &[MeshEdgeRun],
) -> Option<Vec<[usize; 2]>> {
    let mut occurrence_faces = vec![Vec::new(); serialized.len()];
    for run in runs {
        let faces = occurrence_faces.get_mut(run.edge)?;
        if !faces.contains(&run.face) {
            faces.push(run.face);
        }
    }
    let mut resolved = serialized.to_vec();
    for (faces, occurrences) in resolved.iter_mut().zip(occurrence_faces) {
        if faces[0] != faces[1] || occurrences.len() < 2 {
            continue;
        }
        if occurrences.len() != 2 || !occurrences.contains(&faces[0]) {
            return None;
        }
        faces[1] = *occurrences.iter().find(|face| **face != faces[0])?;
    }
    Some(resolved)
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum MeshFaceAssignmentDomain {
    Ordered(Vec<Vec<MeshEdgePlacementCandidate>>),
    UnorderedFullCycle(Vec<usize>),
    DeferredValidation(MeshFaceCoverage),
}

/// One admissible placement of an unmatched physical edge within a recovered
/// trim-boundary gap. Domains contain only placements participating in a
/// complete end-to-end partition of every gap on the face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MeshEdgePlacementCandidate {
    /// Physical edge-row ordinal.
    pub edge: usize,
    /// Positional face ordinal.
    pub face: usize,
    /// Boundary-cycle ordinal within the face.
    pub cycle: usize,
    /// First covered boundary-segment index.
    pub start: usize,
    /// Boundary-segment index immediately after the covered run.
    pub end: usize,
    /// Number of consecutive boundary segments covered by the edge.
    pub segment_count: usize,
}

/// One placement within a complete face assignment, together with the point
/// pairs allowed by its two currently bound trim corners. An absent domain
/// means that at least one corner has no exact point binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshEdgePlacementEndpointCandidate {
    /// Span-consistent placement in its face boundary.
    pub placement: MeshEdgePlacementCandidate,
    /// Unordered logical-point pairs allowed at the placement corners.
    pub endpoint_pairs: Option<Vec<[usize; 2]>>,
}

/// One physical-edge use in a complete candidate trim boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshBoundaryEdgeCandidate {
    /// Physical edge-row ordinal.
    pub edge: usize,
    /// Boundary-segment index at which the use begins.
    pub start: usize,
    /// Boundary-segment index immediately after the use.
    pub end: usize,
    /// Stored-row direction when an interior handle sequence fixes it.
    pub reversed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MeshFaceBoundaryDomain {
    Ordered(Vec<MeshFaceBoundaryAssignment>),
    UnorderedFullCycle(Vec<usize>),
    DeferredValidation(MeshDeferredFaceBoundary),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MeshDeferredFaceBoundary {
    pub(crate) cycles: Vec<MeshDeferredBoundaryCycle>,
    pub(crate) missing_edges: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MeshDeferredBoundaryCycle {
    pub(crate) length: usize,
    pub(crate) exact_uses: Vec<(MeshBoundaryEdgeCandidate, usize)>,
}

/// One complete choice of all unmatched placements on a face, expressed as
/// ordered physical-edge uses for each serialized trim cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshFaceBoundaryAssignment {
    /// Trim cycles in serialized cycle order.
    pub boundaries: Vec<Vec<MeshBoundaryEdgeCandidate>>,
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
    for width in [1, 2, 3] {
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

pub(crate) fn bounded_oriented_trail_orders(
    trails: &[Vec<usize>],
    limit: usize,
) -> Option<Vec<Vec<usize>>> {
    fn visit(
        trails: &[Vec<usize>],
        limit: usize,
        used: u64,
        edges: &mut Vec<usize>,
        orders: &mut Vec<Vec<usize>>,
    ) -> bool {
        if orders.len() > limit {
            return false;
        }
        if used.count_ones() as usize == trails.len() {
            orders.push(edges.clone());
            return orders.len() <= limit;
        }
        for (index, trail) in trails.iter().enumerate() {
            if used & (1 << index) != 0 {
                continue;
            }
            for reversed in [false, true] {
                if reversed && trail.len() == 1 {
                    continue;
                }
                let before = edges.len();
                if reversed {
                    edges.extend(trail.iter().rev());
                } else {
                    edges.extend(trail);
                }
                if !visit(trails, limit, used | (1 << index), edges, orders) {
                    return false;
                }
                edges.truncate(before);
            }
        }
        true
    }

    if trails.len() > u64::BITS as usize {
        return None;
    }
    let mut orders = Vec::new();
    visit(trails, limit, 0, &mut Vec::new(), &mut orders).then_some(orders)
}

pub(crate) fn bounded_endpoint_cycle_orders(
    missing: &[usize],
    edge_candidates: &[Vec<[usize; 2]>],
    limit: usize,
) -> Option<Vec<Vec<usize>>> {
    struct Search<'a> {
        missing: &'a [usize],
        transitions: &'a HashMap<usize, Vec<(usize, usize)>>,
        limit: usize,
        operations_left: usize,
        orders: HashSet<Vec<usize>>,
    }

    impl Search<'_> {
        fn walk(
            &mut self,
            first_point: usize,
            current_point: usize,
            used: u64,
            order: &mut Vec<usize>,
        ) -> bool {
            let Some(operations_left) = self.operations_left.checked_sub(1) else {
                return false;
            };
            self.operations_left = operations_left;
            if order.len() == self.missing.len() {
                if current_point == first_point {
                    self.orders.insert(order.clone());
                }
                return self.orders.len() <= self.limit;
            }
            let Some(transition_count) = self.transitions.get(&current_point).map(Vec::len) else {
                return true;
            };
            for index in 0..transition_count {
                let (rank, next_point) = self.transitions[&current_point][index];
                let Some(operations_left) = self.operations_left.checked_sub(1) else {
                    return false;
                };
                self.operations_left = operations_left;
                if used & (1 << rank) != 0 {
                    continue;
                }
                order.push(self.missing[rank]);
                if !self.walk(first_point, next_point, used | (1 << rank), order) {
                    return false;
                }
                order.pop();
            }
            true
        }
    }

    if missing.is_empty()
        || missing.len() > u64::BITS as usize
        || missing
            .iter()
            .any(|&edge| edge_candidates.get(edge).is_none_or(Vec::is_empty))
    {
        return None;
    }
    let mut missing = missing.to_vec();
    missing.sort_unstable();
    let first_edge = missing[0];
    let mut transitions = HashMap::<usize, Vec<(usize, usize)>>::new();
    for (rank, &edge) in missing.iter().enumerate().skip(1) {
        for &[left, right] in &edge_candidates[edge] {
            transitions.entry(left).or_default().push((rank, right));
            if left != right {
                transitions.entry(right).or_default().push((rank, left));
            }
        }
    }
    for values in transitions.values_mut() {
        values.sort_unstable();
        values.dedup();
    }
    let mut search = Search {
        missing: &missing,
        transitions: &transitions,
        limit,
        operations_left: limit.saturating_mul(16),
        orders: HashSet::new(),
    };
    let mut first_pairs = edge_candidates[first_edge].clone();
    for pair in &mut first_pairs {
        pair.sort_unstable();
    }
    first_pairs.sort_unstable();
    first_pairs.dedup();
    for [first_point, current_point] in first_pairs {
        let mut order = vec![first_edge];
        if !search.walk(first_point, current_point, 1, &mut order) {
            return None;
        }
    }
    if search.orders.is_empty() {
        return None;
    }
    let mut orders = search.orders.into_iter().collect::<Vec<_>>();
    orders.sort_unstable();
    Some(orders)
}

fn standard_mesh_missing_edge_assignment_domains(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: Option<&[Vec<[usize; 2]>]>,
    canonicalize_spans: bool,
    defer_validation: bool,
) -> Option<Vec<MeshFaceAssignmentDomain>> {
    const MAX_ASSIGNMENTS_PER_FACE: usize = 65_536;
    type PlacementConstraints<'a> = (
        Option<&'a [[u32; 2]]>,
        &'a HashMap<MeshCorner, u32>,
        Option<(&'a [Arc<HashSet<usize>>], &'a [PointTransitions])>,
        &'a MeshCornerPoints,
    );
    type PointTransitions = HashMap<usize, Arc<HashSet<usize>>>;
    type DeadState = (usize, usize, u64, Option<u32>, Vec<usize>, bool);

    fn enumerate_face(
        face: usize,
        gaps: &[MeshBoundaryGap],
        cycle_lengths: &[usize],
        missing: &[usize],
        _rows: &[EdgeRow],
        constraints: PlacementConstraints<'_>,
        canonicalize_spans: bool,
    ) -> Option<Vec<Vec<MeshEdgePlacementCandidate>>> {
        struct Search<'a> {
            face: usize,
            gaps: &'a [MeshBoundaryGap],
            cycle_lengths: &'a [usize],
            missing: &'a [usize],
            edge_ports: Option<&'a [[u32; 2]]>,
            corner_ports: &'a HashMap<MeshCorner, u32>,
            edge_points: Option<&'a [Arc<HashSet<usize>>]>,
            point_transitions: Option<&'a [PointTransitions]>,
            corner_points: &'a MeshCornerPoints,
            canonical_spans: bool,
            canonical_gap_partitions: bool,
            dead_states: HashSet<DeadState>,
            assignments: usize,
            complete: Vec<Vec<MeshEdgePlacementCandidate>>,
        }
        impl Search<'_> {
            #[allow(clippy::too_many_arguments)]
            fn walk(
                &mut self,
                gap: usize,
                offset: usize,
                used: u64,
                current_port: Option<u32>,
                current_points: Option<Arc<HashSet<usize>>>,
                gap_placed_start: usize,
                placed: &mut Vec<MeshEdgePlacementCandidate>,
            ) -> Option<()> {
                let mut points = current_points
                    .as_ref()
                    .map(|points| points.iter().copied().collect::<Vec<_>>())
                    .unwrap_or_default();
                points.sort_unstable();
                let has_flexible = placed.len() > gap_placed_start;
                let state = (gap, offset, used, current_port, points, has_flexible);
                if self.dead_states.contains(&state) {
                    return Some(());
                }
                let before = self.assignments;
                self.walk_state(
                    gap,
                    offset,
                    used,
                    current_port,
                    current_points,
                    gap_placed_start,
                    placed,
                )?;
                if self.assignments == before {
                    self.dead_states.insert(state);
                }
                Some(())
            }

            #[allow(clippy::too_many_arguments)]
            fn walk_state(
                &mut self,
                gap: usize,
                offset: usize,
                used: u64,
                current_port: Option<u32>,
                current_points: Option<Arc<HashSet<usize>>>,
                gap_placed_start: usize,
                placed: &mut Vec<MeshEdgePlacementCandidate>,
            ) -> Option<()> {
                if self.assignments > MAX_ASSIGNMENTS_PER_FACE {
                    return None;
                }
                if gap == self.gaps.len() {
                    if used.count_ones() as usize == self.missing.len() {
                        self.assignments += 1;
                        self.complete.push(placed.clone());
                    }
                    return Some(());
                }
                let target = self.gaps[gap].length;
                let can_expand_gap = self.canonical_gap_partitions
                    && offset < target
                    && placed.len() > gap_placed_start;
                if offset == target || can_expand_gap {
                    let value = &self.gaps[gap];
                    let end = (value.start + value.length) % self.cycle_lengths[value.cycle];
                    let port_closes = current_port
                        .zip(
                            self.corner_ports
                                .get(&(self.face, value.cycle, end))
                                .copied(),
                        )
                        .is_none_or(|(actual, expected)| actual == expected);
                    let end_points = self.corner_points.get(&(self.face, value.cycle, end));
                    let points_close = current_points
                        .as_ref()
                        .zip(end_points)
                        .is_none_or(|(actual, expected)| !actual.is_disjoint(expected));
                    if port_closes && points_close {
                        let next_port = self.gaps.get(gap + 1).and_then(|next| {
                            self.corner_ports
                                .get(&(self.face, next.cycle, next.start))
                                .copied()
                        });
                        let next_points = self.gaps.get(gap + 1).and_then(|next| {
                            self.corner_points
                                .get(&(self.face, next.cycle, next.start))
                                .cloned()
                                .map(Arc::new)
                        });
                        let saved = placed[gap_placed_start..].to_vec();
                        if offset < target {
                            let slack = target - offset;
                            let Some(flexible) = placed.get_mut(gap_placed_start) else {
                                return Some(());
                            };
                            flexible.segment_count = flexible.segment_count.checked_add(slack)?;
                            let mut at = value.start;
                            for placement in &mut placed[gap_placed_start..] {
                                placement.start = at % self.cycle_lengths[value.cycle];
                                at = at.checked_add(placement.segment_count)?;
                                placement.end = at % self.cycle_lengths[value.cycle];
                            }
                        }
                        self.walk(
                            gap + 1,
                            0,
                            used,
                            next_port,
                            next_points,
                            placed.len(),
                            placed,
                        )?;
                        placed.truncate(gap_placed_start);
                        placed.extend(saved);
                        if offset == target {
                            return Some(());
                        }
                    } else if offset == target {
                        return Some(());
                    }
                }
                for rank in 0..self.missing.len() {
                    if used & (1 << rank) != 0 {
                        continue;
                    }
                    let edge = self.missing[rank];
                    let remaining = target - offset;
                    let canonical_span = self
                        .canonical_spans
                        .then(|| {
                            if self.canonical_gap_partitions {
                                return Some(1);
                            }
                            self.missing
                                .iter()
                                .enumerate()
                                .filter(|(other, _)| *other != rank && used & (1 << other) == 0)
                                .try_fold(remaining, |available, _| available.checked_sub(1))
                        })
                        .flatten();
                    let spans: Box<dyn Iterator<Item = usize>> = if let Some(span) = canonical_span
                    {
                        Box::new(std::iter::once(span))
                    } else {
                        Box::new(1..=remaining)
                    };
                    for segment_count in spans.filter(|span| *span > 0 && *span <= remaining) {
                        let mut next_ports = match (self.edge_ports, current_port) {
                            (Some(edge_ports), Some(current)) if edge_ports[edge][0] == current => {
                                vec![Some(edge_ports[edge][1])]
                            }
                            (Some(edge_ports), Some(current)) if edge_ports[edge][1] == current => {
                                vec![Some(edge_ports[edge][0])]
                            }
                            (Some(_), Some(_)) => continue,
                            (Some(edge_ports), None) => edge_ports[edge].map(Some).to_vec(),
                            (None, _) => vec![None],
                        };
                        next_ports.sort_unstable();
                        next_ports.dedup();
                        let next_points = self.edge_points.and_then(|edge_points| {
                            (!edge_points[edge].is_empty()).then(|| match &current_points {
                                None => edge_points[edge].clone(),
                                Some(current) if current.len() == 1 => {
                                    let point = *current.iter().next().expect("singleton domain");
                                    self.point_transitions
                                        .and_then(|transitions| {
                                            transitions[edge].get(&point).cloned()
                                        })
                                        .unwrap_or_default()
                                }
                                Some(current) => Arc::new(
                                    current
                                        .iter()
                                        .filter_map(|point| {
                                            self.point_transitions.and_then(|transitions| {
                                                transitions[edge].get(point)
                                            })
                                        })
                                        .flat_map(|points| points.iter().copied())
                                        .collect(),
                                ),
                            })
                        });
                        if next_points.as_ref().is_some_and(|points| points.is_empty()) {
                            continue;
                        }
                        let value = MeshEdgePlacementCandidate {
                            edge,
                            face: self.face,
                            cycle: self.gaps[gap].cycle,
                            start: (self.gaps[gap].start + offset)
                                % self.cycle_lengths[self.gaps[gap].cycle],
                            end: (self.gaps[gap].start + offset + segment_count)
                                % self.cycle_lengths[self.gaps[gap].cycle],
                            segment_count,
                        };
                        placed.push(value);
                        for next_port in next_ports {
                            self.walk(
                                gap,
                                offset + segment_count,
                                used | (1 << rank),
                                next_port,
                                next_points.clone(),
                                gap_placed_start,
                                placed,
                            )?;
                        }
                        placed.pop();
                    }
                }
                drop(current_points);
                Some(())
            }
        }

        let (edge_ports, corner_ports, endpoint_constraints, corner_points) = constraints;
        if missing.len() > u64::BITS as usize {
            return None;
        }
        let (edge_points, point_transitions) = endpoint_constraints.unzip();
        let mut search = Search {
            face,
            gaps,
            cycle_lengths,
            missing,
            edge_ports,
            corner_ports,
            edge_points,
            point_transitions,
            corner_points,
            // Mesh span allocation does not change the ordered edge uses or
            // their endpoint quotient. Keep one allocation for each edge
            // order and gap partition in topology searches; the public
            // placement API above still enumerates every span allocation.
            canonical_spans: canonicalize_spans,
            canonical_gap_partitions: canonicalize_spans,
            dead_states: HashSet::new(),
            assignments: 0,
            complete: Vec::new(),
        };
        let first_port = gaps
            .first()
            .and_then(|gap| corner_ports.get(&(face, gap.cycle, gap.start)).copied());
        let first_points = gaps
            .first()
            .and_then(|gap| corner_points.get(&(face, gap.cycle, gap.start)).cloned())
            .map(Arc::new);
        search.walk(0, 0, 0, first_port, first_points, 0, &mut Vec::new())?;
        if search.assignments == 0 || search.assignments > MAX_ASSIGNMENTS_PER_FACE {
            return None;
        }
        Some(search.complete)
    }

    fn endpoint_trail_assignments(
        face: usize,
        gaps: &[MeshBoundaryGap],
        cycle_lengths: &[usize],
        missing: &[usize],
        rows: &[EdgeRow],
        edge_points: &[Option<[usize; 2]>],
        corner_points: &MeshCornerPoints,
    ) -> Option<Vec<Vec<MeshEdgePlacementCandidate>>> {
        struct EndpointTrail {
            edges: Vec<usize>,
            start: usize,
            end: usize,
        }

        if gaps.is_empty()
            || missing
                .iter()
                .any(|&edge| edge_points.get(edge).is_none_or(Option::is_none))
        {
            return None;
        }
        let mut at_point = HashMap::<usize, Vec<usize>>::new();
        for &edge in missing {
            for point in edge_points[edge]? {
                at_point.entry(point).or_default().push(edge);
            }
        }
        if at_point.values().any(|edges| edges.len() > 2) {
            return None;
        }
        let mut unseen = missing.iter().copied().collect::<HashSet<_>>();
        let mut trails = Vec::<EndpointTrail>::new();
        while !unseen.is_empty() {
            let first = unseen
                .iter()
                .copied()
                .filter(|edge| {
                    edge_points[*edge]
                        .is_some_and(|pair| pair.iter().any(|point| at_point[point].len() == 1))
                })
                .min()
                .or_else(|| unseen.iter().copied().min())?;
            let endpoints = edge_points[first]?;
            let start = endpoints
                .iter()
                .copied()
                .find(|point| at_point[point].len() == 1)
                .unwrap_or(endpoints[0]);
            let mut point = start;
            let mut edge = first;
            let mut trail = Vec::new();
            loop {
                if !unseen.remove(&edge) {
                    break;
                }
                trail.push(edge);
                let endpoints = edge_points[edge]?;
                point = if endpoints[0] == point {
                    endpoints[1]
                } else if endpoints[1] == point {
                    endpoints[0]
                } else {
                    return None;
                };
                let Some(next) = at_point[&point]
                    .iter()
                    .copied()
                    .find(|candidate| unseen.contains(candidate))
                else {
                    break;
                };
                edge = next;
            }
            if point == start && (gaps.len() != 1 || trail.len() != missing.len()) {
                return None;
            }
            trails.push(EndpointTrail {
                edges: trail,
                start,
                end: point,
            });
        }
        if gaps.len() > 1 {
            if trails.len() != gaps.len() {
                return None;
            }
            let mut available = trails;
            available.sort_by_key(|trail| trail.edges.iter().copied().min());
            let mut placements = Vec::with_capacity(missing.len());
            for gap in gaps {
                let candidates = available
                    .iter()
                    .enumerate()
                    .flat_map(|(index, trail)| {
                        [false, true].map(move |reversed| (index, trail, reversed))
                    })
                    .filter_map(|(index, trail, reversed)| {
                        let trail_start = if reversed { trail.end } else { trail.start };
                        let trail_end = if reversed { trail.start } else { trail.end };
                        let gap_end = (gap.start + gap.length) % cycle_lengths[gap.cycle];
                        if !corner_points
                            .get(&(face, gap.cycle, gap.start))?
                            .contains(&trail_start)
                            || !corner_points
                                .get(&(face, gap.cycle, gap_end))?
                                .contains(&trail_end)
                        {
                            return None;
                        }
                        let span = trail.edges.len();
                        (span <= gap.length).then_some((index, span, reversed))
                    })
                    .collect::<Vec<_>>();
                let [(trail_index, minimum_span, reversed)] = candidates.as_slice() else {
                    return None;
                };
                let (trail_index, minimum_span, reversed) =
                    (*trail_index, *minimum_span, *reversed);
                let trail = available.remove(trail_index);
                let edges: Box<dyn Iterator<Item = usize>> = if reversed {
                    Box::new(trail.edges.into_iter().rev())
                } else {
                    Box::new(trail.edges.into_iter())
                };
                let edges = edges.collect::<Vec<_>>();
                let slack = gap.length - minimum_span;
                let mut offset = 0usize;
                for (index, edge) in edges.into_iter().enumerate() {
                    let mut segment_count = 1usize;
                    if index == 0 {
                        segment_count = segment_count.checked_add(slack)?;
                    }
                    placements.push(MeshEdgePlacementCandidate {
                        edge,
                        face,
                        cycle: gap.cycle,
                        start: (gap.start + offset) % cycle_lengths[gap.cycle],
                        end: (gap.start + offset + segment_count) % cycle_lengths[gap.cycle],
                        segment_count,
                    });
                    offset = offset.checked_add(segment_count)?;
                }
            }
            return Some(vec![placements]);
        }
        let [gap] = gaps else {
            return None;
        };
        if cycle_lengths.len() != 1
            || gap.start != 0
            || gap.cycle != 0
            || gap.length != cycle_lengths[0]
            || gap.length != missing.len()
            || missing.iter().any(|&edge| rows[edge].handles.len() != 2)
        {
            return None;
        }
        if trails.len() > u64::BITS as usize {
            return None;
        }
        let orders = bounded_oriented_trail_orders(
            &trails
                .iter()
                .map(|trail| trail.edges.clone())
                .collect::<Vec<_>>(),
            MAX_ASSIGNMENTS_PER_FACE,
        )?;
        Some(
            orders
                .into_iter()
                .map(|order| {
                    order
                        .into_iter()
                        .enumerate()
                        .map(|(offset, edge)| MeshEdgePlacementCandidate {
                            edge,
                            face,
                            cycle: gap.cycle,
                            start: offset,
                            end: (offset + 1) % gap.length,
                            segment_count: 1,
                        })
                        .collect()
                })
                .collect(),
        )
    }

    fn endpoint_cycle_assignments(
        face: usize,
        gaps: &[MeshBoundaryGap],
        cycle_lengths: &[usize],
        missing: &[usize],
        rows: &[EdgeRow],
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Option<Vec<Vec<MeshEdgePlacementCandidate>>> {
        let [gap] = gaps else {
            return None;
        };
        if cycle_lengths.len() != 1
            || gap.start != 0
            || gap.cycle != 0
            || gap.length != cycle_lengths[0]
            || gap.length != missing.len()
            || missing.iter().any(|&edge| rows[edge].handles.len() != 2)
        {
            return None;
        }
        bounded_endpoint_cycle_orders(missing, edge_candidates, MAX_ASSIGNMENTS_PER_FACE).map(
            |orders| {
                orders
                    .into_iter()
                    .map(|order| {
                        order
                            .into_iter()
                            .enumerate()
                            .map(|(offset, edge)| MeshEdgePlacementCandidate {
                                edge,
                                face,
                                cycle: 0,
                                start: offset,
                                end: (offset + 1) % gap.length,
                                segment_count: 1,
                            })
                            .collect()
                    })
                    .collect()
            },
        )
    }

    let (face_start, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, _) = parse_edge_tables(bytes, after_faces)?;
    if edge_candidates.is_some_and(|candidates| candidates.len() != edge_rows.len()) {
        return None;
    }
    let edge_point_domains = edge_candidates.map(|candidates| {
        candidates
            .iter()
            .map(|pairs| Arc::new(pairs.iter().flatten().copied().collect::<HashSet<_>>()))
            .collect::<Vec<_>>()
    });
    let edge_point_transitions = edge_candidates.map(|candidates| {
        candidates
            .iter()
            .map(|pairs| {
                let mut transitions = HashMap::<usize, HashSet<usize>>::new();
                for [left, right] in pairs {
                    transitions.entry(*left).or_default().insert(*right);
                    transitions.entry(*right).or_default().insert(*left);
                }
                transitions
                    .into_iter()
                    .map(|(point, targets)| (point, Arc::new(targets)))
                    .collect()
            })
            .collect::<Vec<_>>()
    });
    let endpoint_constraints = edge_point_domains
        .as_deref()
        .zip(edge_point_transitions.as_deref());
    let coverage = standard_mesh_face_coverage(bytes, edge_faces)?;
    let edge_ports = standard_mesh_edge_ports(bytes)?;
    let singleton_edge_points = edge_candidates.map(|candidates| {
        candidates
            .iter()
            .map(|domain| {
                <[[usize; 2]; 1]>::try_from(domain.as_slice())
                    .ok()
                    .map(|[pair]| pair)
            })
            .collect::<Vec<_>>()
    });
    let edge_runs = standard_mesh_edge_runs(bytes)?;
    let mut solutions = Vec::new();
    for width in [1, 2, 3] {
        let Some(trims) = parse_trim_chain(bytes, face_start, face_count, width) else {
            continue;
        };
        let cycles = trims
            .iter()
            .map(|trim| boundary_cycles(&trim.triangles))
            .collect::<Option<Vec<_>>>()?;
        let mut corner_ports = HashMap::<MeshCorner, u32>::new();
        let mut corner_points = MeshCornerPoints::new();
        for run in &edge_runs {
            let length = cycles[run.face][run.cycle].len();
            let end = (run.start + run.segment_count) % length;
            let ports = edge_ports[run.edge];
            let oriented = if run.reversed {
                [ports[1], ports[0]]
            } else {
                ports
            };
            for (corner, port) in [(run.start, oriented[0]), (end, oriented[1])] {
                match corner_ports.insert((run.face, run.cycle, corner), port) {
                    Some(stored) if stored != port => return None,
                    Some(_) | None => {}
                }
            }
            if let Some(candidates) = edge_candidates {
                let points = candidates[run.edge]
                    .iter()
                    .flatten()
                    .copied()
                    .collect::<HashSet<_>>();
                if !points.is_empty() {
                    for corner in [run.start, end] {
                        corner_points
                            .entry((run.face, run.cycle, corner))
                            .and_modify(|stored| stored.retain(|point| points.contains(point)))
                            .or_insert_with(|| points.clone());
                    }
                }
            }
        }
        let assignment_results = coverage.iter().map(|face| {
            let cycle_lengths = cycles[face.face].iter().map(Vec::len).collect::<Vec<_>>();
            let unordered_full_cycle = edge_candidates.and_then(|candidates| {
                let [gap] = face.gaps.as_slice() else {
                    return None;
                };
                (cycles[face.face].len() == 1
                    && gap.cycle == 0
                    && gap.start == 0
                    && gap.length == cycles[face.face][0].len()
                    && gap.length == face.missing_edges.len()
                    && face
                        .missing_edges
                        .iter()
                        .all(|&edge| edge_rows[edge].handles.len() == 2)
                    && defer_validation
                    && face
                        .missing_edges
                        .iter()
                        .any(|&edge| candidates[edge].len() > 1))
                .then(|| face.missing_edges.clone())
            });
            if let Some(edges) = unordered_full_cycle {
                return Some(MeshFaceAssignmentDomain::UnorderedFullCycle(edges));
            }
            let cycle_assignments = edge_candidates.and_then(|candidates| {
                endpoint_cycle_assignments(
                    face.face,
                    &face.gaps,
                    &cycle_lengths,
                    &face.missing_edges,
                    &edge_rows,
                    candidates,
                )
            });
            let assignments = cycle_assignments
                .or_else(|| {
                    singleton_edge_points.as_ref().and_then(|edge_points| {
                        endpoint_trail_assignments(
                            face.face,
                            &face.gaps,
                            &cycle_lengths,
                            &face.missing_edges,
                            &edge_rows,
                            edge_points,
                            &corner_points,
                        )
                    })
                })
                .or_else(|| {
                    enumerate_face(
                        face.face,
                        &face.gaps,
                        &cycle_lengths,
                        &face.missing_edges,
                        &edge_rows,
                        (
                            Some(&edge_ports),
                            &corner_ports,
                            endpoint_constraints,
                            &corner_points,
                        ),
                        canonicalize_spans,
                    )
                })
                .or_else(|| {
                    enumerate_face(
                        face.face,
                        &face.gaps,
                        &cycle_lengths,
                        &face.missing_edges,
                        &edge_rows,
                        (None, &HashMap::new(), endpoint_constraints, &corner_points),
                        canonicalize_spans,
                    )
                })
                .or_else(|| {
                    enumerate_face(
                        face.face,
                        &face.gaps,
                        &cycle_lengths,
                        &face.missing_edges,
                        &edge_rows,
                        (None, &HashMap::new(), None, &MeshCornerPoints::new()),
                        canonicalize_spans,
                    )
                });
            assignments
                .map(MeshFaceAssignmentDomain::Ordered)
                .or_else(|| {
                    defer_validation
                        .then(|| MeshFaceAssignmentDomain::DeferredValidation(face.clone()))
                })
        });
        let domains = assignment_results.collect::<Option<Vec<_>>>()?;
        solutions.push(domains);
    }
    <[Vec<MeshFaceAssignmentDomain>; 1]>::try_from(solutions)
        .ok()
        .map(|[domains]| domains)
}

pub(crate) fn standard_mesh_missing_edge_assignments(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: Option<&[Vec<[usize; 2]>]>,
    canonicalize_spans: bool,
) -> Option<Vec<Vec<Vec<MeshEdgePlacementCandidate>>>> {
    standard_mesh_missing_edge_assignment_domains(
        bytes,
        edge_faces,
        edge_candidates,
        canonicalize_spans,
        false,
    )?
    .into_iter()
    .map(|domain| match domain {
        MeshFaceAssignmentDomain::Ordered(assignments) => Some(assignments),
        MeshFaceAssignmentDomain::UnorderedFullCycle(_)
        | MeshFaceAssignmentDomain::DeferredValidation(_) => None,
    })
    .collect()
}

/// Project complete unmatched-edge assignments to the placement domain for
/// each face. Every unmatched row may cover any positive remaining span;
/// row arity fixes a span only after its interior handles match the boundary.
#[cfg(test)]
#[must_use]
pub fn standard_mesh_missing_edge_placements(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
) -> Option<Vec<Vec<MeshEdgePlacementCandidate>>> {
    standard_mesh_missing_edge_assignments(bytes, edge_faces, None, false).map(|faces| {
        faces
            .into_iter()
            .map(|assignments| {
                let mut placements = assignments
                    .into_iter()
                    .flatten()
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                placements.sort_unstable();
                placements
            })
            .collect()
    })
}

pub(crate) fn standard_mesh_boundary_assignments(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: Option<&[Vec<[usize; 2]>]>,
) -> Option<Vec<Vec<MeshFaceBoundaryAssignment>>> {
    standard_mesh_boundary_domains_impl(bytes, edge_faces, edge_candidates, false)?
        .into_iter()
        .map(|domain| match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => Some(assignments),
            MeshFaceBoundaryDomain::UnorderedFullCycle(_)
            | MeshFaceBoundaryDomain::DeferredValidation(_) => None,
        })
        .collect()
}

pub(crate) fn standard_mesh_boundary_domains_impl(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: Option<&[Vec<[usize; 2]>]>,
    defer_validation: bool,
) -> Option<Vec<MeshFaceBoundaryDomain>> {
    let domains = standard_mesh_missing_edge_assignment_domains(
        bytes,
        edge_faces,
        edge_candidates,
        true,
        defer_validation,
    )?;
    let runs = standard_mesh_edge_runs(bytes)?;
    let (face_start, face_count, _) = largest_fbb_run(bytes)?;
    let cycle_solutions = [1, 2, 3]
        .into_iter()
        .filter_map(|width| parse_trim_chain(bytes, face_start, face_count, width))
        .map(|trims| {
            trims
                .iter()
                .map(|trim| {
                    boundary_cycles(&trim.triangles)
                        .map(|cycles| cycles.into_iter().map(|cycle| cycle.len()).collect())
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()?;
    let [cycle_lengths] = <[Vec<Vec<usize>>; 1]>::try_from(cycle_solutions).ok()?;
    domains
        .into_iter()
        .enumerate()
        .map(|(face, domain)| match domain {
            MeshFaceAssignmentDomain::UnorderedFullCycle(edges) => {
                Some(MeshFaceBoundaryDomain::UnorderedFullCycle(edges))
            }
            MeshFaceAssignmentDomain::DeferredValidation(coverage) => {
                let mut cycles = cycle_lengths[face]
                    .iter()
                    .copied()
                    .map(|length| MeshDeferredBoundaryCycle {
                        length,
                        exact_uses: Vec::new(),
                    })
                    .collect::<Vec<_>>();
                for run in runs.iter().filter(|run| run.face == face) {
                    let length = cycles[run.cycle].length;
                    cycles[run.cycle].exact_uses.push((
                        MeshBoundaryEdgeCandidate {
                            edge: run.edge,
                            start: run.start,
                            end: (run.start + run.segment_count) % length,
                            reversed: Some(run.reversed),
                        },
                        run.segment_count,
                    ));
                }
                for cycle in &mut cycles {
                    cycle
                        .exact_uses
                        .sort_unstable_by_key(|(use_, _)| use_.start);
                }
                Some(MeshFaceBoundaryDomain::DeferredValidation(
                    MeshDeferredFaceBoundary {
                        cycles,
                        missing_edges: coverage.missing_edges,
                    },
                ))
            }
            MeshFaceAssignmentDomain::Ordered(assignments) => assignments
                .into_iter()
                .map(|assignment| {
                    let mut boundaries = vec![Vec::new(); cycle_lengths[face].len()];
                    for run in runs.iter().filter(|run| run.face == face) {
                        boundaries[run.cycle].push((
                            MeshBoundaryEdgeCandidate {
                                edge: run.edge,
                                start: run.start,
                                end: (run.start + run.segment_count)
                                    % cycle_lengths[face][run.cycle],
                                reversed: edge_candidates.is_none().then_some(run.reversed),
                            },
                            run.segment_count,
                        ));
                    }
                    for placement in assignment {
                        boundaries[placement.cycle].push((
                            MeshBoundaryEdgeCandidate {
                                edge: placement.edge,
                                start: placement.start,
                                end: placement.end,
                                reversed: None,
                            },
                            placement.segment_count,
                        ));
                    }
                    let boundaries = boundaries
                        .into_iter()
                        .enumerate()
                        .map(|(cycle, mut uses)| {
                            uses.sort_unstable_by_key(|(edge, _)| edge.start);
                            let length = cycle_lengths[face][cycle];
                            let mut coverage = vec![0u8; length];
                            for (edge, segment_count) in &uses {
                                for offset in 0..*segment_count {
                                    let covered = &mut coverage[(edge.start + offset) % length];
                                    *covered = covered.checked_add(1)?;
                                }
                            }
                            coverage
                                .iter()
                                .all(|count| *count == 1)
                                .then(|| uses.into_iter().map(|(edge, _)| edge).collect::<Vec<_>>())
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some(MeshFaceBoundaryAssignment { boundaries })
                })
                .collect::<Option<Vec<_>>>()
                .map(MeshFaceBoundaryDomain::Ordered),
        })
        .collect()
}

/// Materialize one complete face-assignment selection and one direction for
/// each ordered edge use into its abstract logical-corner quotient.
#[cfg(test)]
#[must_use]
pub fn parse_standard_mesh_selection(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    selected_assignments: &[usize],
    edge_directions: &[Vec<Vec<bool>>],
) -> Option<StandardTopology> {
    let (_, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, vertex_header) = parse_edge_tables(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    let assignments = standard_mesh_boundary_assignments(bytes, edge_faces, None)?;
    if selected_assignments.len() != face_count || edge_directions.len() != face_count {
        return None;
    }
    let selected = assignments
        .iter()
        .zip(selected_assignments)
        .map(|(face, &assignment)| face.get(assignment).cloned())
        .collect::<Option<Vec<_>>>()?;
    reconstruct_mesh_selection(edge_rows, vertex_points, &selected, edge_directions)
}

fn boundary_endpoint_support(
    boundary: &[MeshBoundaryEdgeCandidate],
    edge_candidates: &[Vec<[usize; 2]>],
    budget: &MeshConstraintBudget,
) -> Option<HashMap<usize, HashSet<[usize; 2]>>> {
    #[derive(Clone, Copy)]
    struct State {
        pair: [usize; 2],
        start: usize,
        end: usize,
    }

    let layers = boundary
        .iter()
        .map(|use_| {
            edge_candidates.get(use_.edge).and_then(|pairs| {
                (!pairs.is_empty()).then(|| {
                    pairs
                        .iter()
                        .flat_map(|&pair| {
                            let mut unordered = pair;
                            unordered.sort_unstable();
                            [
                                State {
                                    pair: unordered,
                                    start: unordered[0],
                                    end: unordered[1],
                                },
                                State {
                                    pair: unordered,
                                    start: unordered[1],
                                    end: unordered[0],
                                },
                            ]
                        })
                        .collect::<Vec<_>>()
                })
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let first_layer = layers.first()?;
    let layer_states = layers
        .iter()
        .try_fold(0usize, |total, layer| total.checked_add(layer.len()))?;
    let first_points = first_layer
        .iter()
        .map(|state| state.start)
        .collect::<HashSet<_>>();
    let mut supported = layers
        .iter()
        .map(|layer| vec![false; layer.len()])
        .collect::<Vec<_>>();
    for first_point in first_points {
        if !budget.charge_by(layer_states.checked_mul(3)?) {
            return None;
        }
        let mut forward = layers
            .iter()
            .map(|layer| vec![false; layer.len()])
            .collect::<Vec<_>>();
        for (state, reachable) in first_layer.iter().zip(&mut forward[0]) {
            *reachable = state.start == first_point;
        }
        for layer in 1..layers.len() {
            if !budget.charge_by(layers[layer - 1].len() + layers[layer].len()) {
                return None;
            }
            let reachable_points = layers[layer - 1]
                .iter()
                .zip(&forward[layer - 1])
                .filter_map(|(state, reachable)| reachable.then_some(state.end))
                .collect::<HashSet<_>>();
            for (right, right_state) in layers[layer].iter().enumerate() {
                forward[layer][right] = reachable_points.contains(&right_state.start);
            }
        }
        let mut backward = layers
            .iter()
            .map(|layer| vec![false; layer.len()])
            .collect::<Vec<_>>();
        let last = layers.len() - 1;
        for (state, (reachable, value)) in layers[last]
            .iter()
            .zip(forward[last].iter().zip(&mut backward[last]))
        {
            *value = *reachable && state.end == first_point;
        }
        for layer in (0..last).rev() {
            let supported_points = layers[layer + 1]
                .iter()
                .zip(&backward[layer + 1])
                .filter_map(|(state, supported)| supported.then_some(state.start))
                .collect::<HashSet<_>>();
            for (left, left_state) in layers[layer].iter().enumerate() {
                backward[layer][left] = supported_points.contains(&left_state.end);
            }
        }
        if backward[0].iter().any(|supported| *supported) {
            for layer in 0..layers.len() {
                for state in 0..layers[layer].len() {
                    supported[layer][state] |= forward[layer][state] && backward[layer][state];
                }
            }
        }
    }
    let mut by_edge = HashMap::<usize, HashSet<[usize; 2]>>::new();
    for (layer, use_) in boundary.iter().enumerate() {
        let values = layers[layer]
            .iter()
            .zip(&supported[layer])
            .filter_map(|(state, supported)| supported.then_some(state.pair))
            .collect::<HashSet<_>>();
        if values.is_empty() {
            return None;
        }
        by_edge
            .entry(use_.edge)
            .and_modify(|stored| stored.retain(|pair| values.contains(pair)))
            .or_insert(values);
    }
    by_edge
        .values()
        .all(|domain| !domain.is_empty())
        .then_some(by_edge)
}

/// Prune endpoint-pair domains through every ordered trim-boundary candidate.
/// A pair survives only when each incident face retains a complete assignment
/// whose ordered cycles admit a closed head-to-tail traversal using that pair.
#[must_use]
pub fn standard_mesh_prune_endpoint_candidates(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
) -> Option<Vec<Vec<[usize; 2]>>> {
    if edge_faces.len() != edge_candidates.len() {
        return None;
    }
    let (_, _, after_faces) = largest_fbb_run(bytes)?;
    let (_, vertex_header) = parse_edge_tables(bytes, after_faces)?;
    let point_count = parse_vertex_table(bytes, vertex_header)?.len();
    let complete_domain = (0..point_count)
        .flat_map(|left| ((left + 1)..point_count).map(move |right| [left, right]))
        .collect::<Vec<_>>();
    let mut candidates = edge_candidates
        .iter()
        .map(|domain| {
            if domain.is_empty() {
                complete_domain.clone()
            } else {
                domain.clone()
            }
        })
        .collect::<Vec<_>>();
    let mut faces = standard_mesh_boundary_assignments(bytes, edge_faces, None)?;
    let budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    loop {
        let before = (
            faces.iter().map(Vec::len).sum::<usize>(),
            candidates.iter().map(Vec::len).sum::<usize>(),
        );
        let mut face_supports = Vec::with_capacity(faces.len());
        for assignments in &mut faces {
            let evaluated = assignments
                .iter()
                .enumerate()
                .filter_map(|(index, assignment)| {
                    let mut support = HashMap::<usize, HashSet<[usize; 2]>>::new();
                    for boundary in &assignment.boundaries {
                        for (edge, domain) in
                            boundary_endpoint_support(boundary, &candidates, &budget)?
                        {
                            support
                                .entry(edge)
                                .and_modify(|stored| stored.retain(|pair| domain.contains(pair)))
                                .or_insert(domain);
                        }
                    }
                    support
                        .values()
                        .all(|domain| !domain.is_empty())
                        .then_some((index, support))
                })
                .collect::<Vec<_>>();
            if evaluated.is_empty() {
                return None;
            }
            *assignments = evaluated
                .iter()
                .map(|(index, _)| assignments[*index].clone())
                .collect();
            face_supports.push(
                evaluated
                    .into_iter()
                    .map(|(_, support)| support)
                    .collect::<Vec<_>>(),
            );
        }
        for (edge, domain) in candidates.iter_mut().enumerate() {
            let mut allowed = None::<HashSet<[usize; 2]>>;
            let mut incident = edge_faces[edge].to_vec();
            incident.sort_unstable();
            incident.dedup();
            for face in incident {
                let support = face_supports[face]
                    .iter()
                    .filter_map(|assignment| assignment.get(&edge))
                    .flatten()
                    .copied()
                    .collect::<HashSet<_>>();
                if support.is_empty() {
                    return None;
                }
                if let Some(allowed) = &mut allowed {
                    allowed.retain(|pair| support.contains(pair));
                } else {
                    allowed = Some(support);
                }
            }
            let allowed = allowed?;
            domain.retain(|pair| {
                let mut pair = *pair;
                pair.sort_unstable();
                allowed.contains(&pair)
            });
            if domain.is_empty() {
                return None;
            }
        }
        let after = (
            faces.iter().map(Vec::len).sum::<usize>(),
            candidates.iter().map(Vec::len).sum::<usize>(),
        );
        if after == before {
            break;
        }
    }
    Some(candidates)
}

type MeshCorner = (usize, usize, usize);
type MeshCornerPoints = HashMap<MeshCorner, HashSet<usize>>;

fn standard_mesh_assignment_corner_points(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[Option<[usize; 2]>],
) -> Option<(Vec<Vec<Vec<MeshEdgePlacementCandidate>>>, MeshCornerPoints)> {
    let edge_rows = standard_edge_rows(bytes)?;
    if edge_rows.len() != edge_points.len() || edge_rows.len() != edge_faces.len() {
        return None;
    }
    let runs = standard_mesh_edge_runs(bytes)?;
    let assignments = standard_mesh_missing_edge_assignments(bytes, edge_faces, None, true)?;
    let (face_start, face_count, _) = largest_fbb_run(bytes)?;
    let cycle_solutions = [1, 2, 3]
        .into_iter()
        .filter_map(|width| parse_trim_chain(bytes, face_start, face_count, width))
        .map(|trims| {
            trims
                .iter()
                .map(|trim| {
                    boundary_cycles(&trim.triangles)
                        .map(|cycles| cycles.iter().map(Vec::len).collect::<Vec<_>>())
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()?;
    let [cycle_lengths] = <[Vec<Vec<usize>>; 1]>::try_from(cycle_solutions).ok()?;
    let mut corner_points = MeshCornerPoints::new();
    let mut run_constraints = Vec::new();
    for run in runs {
        let Some(pair) = edge_points[run.edge] else {
            continue;
        };
        let candidates = HashSet::from(pair);
        let positions = [
            (run.face, run.cycle, run.start),
            (
                run.face,
                run.cycle,
                run.start.checked_add(run.segment_count)? % cycle_lengths[run.face][run.cycle],
            ),
        ];
        for position in positions {
            if let Some(stored) = corner_points.get_mut(&position) {
                stored.retain(|point| candidates.contains(point));
                if stored.is_empty() {
                    return None;
                }
            } else {
                corner_points.insert(position, candidates.clone());
            }
        }
        run_constraints.push((positions[0], positions[1], pair));
    }
    loop {
        let before = corner_points.values().map(HashSet::len).sum::<usize>();
        for &(left, right, pair) in &run_constraints {
            let left_single = <[usize; 1]>::try_from(
                corner_points
                    .get(&left)?
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
            )
            .ok()
            .map(|[point]| point);
            let right_single = <[usize; 1]>::try_from(
                corner_points
                    .get(&right)?
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
            )
            .ok()
            .map(|[point]| point);
            if let Some(point) = left_single {
                corner_points
                    .get_mut(&right)?
                    .retain(|candidate| *candidate != point && pair.contains(candidate));
            }
            if let Some(point) = right_single {
                corner_points
                    .get_mut(&left)?
                    .retain(|candidate| *candidate != point && pair.contains(candidate));
            }
            if corner_points.get(&left)?.is_empty() || corner_points.get(&right)?.is_empty() {
                return None;
            }
        }
        let after = corner_points.values().map(HashSet::len).sum::<usize>();
        if after == before {
            break;
        }
    }
    Some((assignments, corner_points))
}

/// Retain endpoint constraints on each placement inside each complete face
/// assignment. Assignment and placement order are unchanged from the serialized
/// face and edge order.
#[must_use]
pub fn standard_mesh_missing_edge_endpoint_assignments(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[Option<[usize; 2]>],
) -> Option<Vec<Vec<Vec<MeshEdgePlacementEndpointCandidate>>>> {
    let (assignments, corner_points) =
        standard_mesh_assignment_corner_points(bytes, edge_faces, edge_points)?;
    Some(
        assignments
            .into_iter()
            .map(|face| {
                face.into_iter()
                    .map(|assignment| {
                        assignment
                            .into_iter()
                            .map(|placement| {
                                let endpoint_pairs = corner_points
                                    .get(&(placement.face, placement.cycle, placement.start))
                                    .zip(corner_points.get(&(
                                        placement.face,
                                        placement.cycle,
                                        placement.end,
                                    )))
                                    .map(|(starts, ends)| {
                                        let mut pairs = starts
                                            .iter()
                                            .flat_map(|&start| {
                                                ends.iter().filter(move |&&end| start != end).map(
                                                    move |&end| {
                                                        let mut pair = [start, end];
                                                        pair.sort_unstable();
                                                        pair
                                                    },
                                                )
                                            })
                                            .collect::<Vec<_>>();
                                        pairs.sort_unstable();
                                        pairs.dedup();
                                        pairs
                                    });
                                MeshEdgePlacementEndpointCandidate {
                                    placement,
                                    endpoint_pairs,
                                }
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect(),
    )
}

/// Enforce resolved edge endpoint pairs and complete opposite-face placement
/// domains across correlated face assignments. A face assignment is removed as
/// a unit when any of its placements has no compatible endpoint pair.
#[must_use]
pub fn standard_mesh_pruned_missing_edge_endpoint_assignments(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[Option<[usize; 2]>],
) -> Option<Vec<Vec<Vec<MeshEdgePlacementEndpointCandidate>>>> {
    let mut faces =
        standard_mesh_missing_edge_endpoint_assignments(bytes, edge_faces, edge_points)?;
    loop {
        let before = (
            faces.iter().map(Vec::len).sum::<usize>(),
            faces
                .iter()
                .flatten()
                .flatten()
                .filter_map(|candidate| candidate.endpoint_pairs.as_ref().map(Vec::len))
                .sum::<usize>(),
        );
        let mut face_domains = HashMap::<(usize, usize), Option<HashSet<[usize; 2]>>>::new();
        for (face, assignments) in faces.iter().enumerate() {
            for edge in edge_faces
                .iter()
                .enumerate()
                .filter_map(|(edge, incident)| incident.contains(&face).then_some(edge))
            {
                let candidates = assignments
                    .iter()
                    .filter_map(|assignment| {
                        assignment
                            .iter()
                            .find(|candidate| candidate.placement.edge == edge)
                    })
                    .collect::<Vec<_>>();
                if candidates.len() != assignments.len()
                    || candidates
                        .iter()
                        .any(|candidate| candidate.endpoint_pairs.is_none())
                {
                    face_domains.insert((face, edge), None);
                    continue;
                }
                let domain = candidates
                    .into_iter()
                    .flat_map(|candidate| {
                        candidate
                            .endpoint_pairs
                            .as_ref()
                            .into_iter()
                            .flatten()
                            .copied()
                    })
                    .collect::<HashSet<_>>();
                face_domains.insert((face, edge), Some(domain));
            }
        }
        for assignments in &mut faces {
            assignments.retain_mut(|assignment| {
                assignment.iter_mut().all(|candidate| {
                    let edge = candidate.placement.edge;
                    let seed = edge_points[edge].map(|mut pair| {
                        pair.sort_unstable();
                        pair
                    });
                    let opposite = edge_faces[edge]
                        .into_iter()
                        .find(|&face| face != candidate.placement.face)
                        .and_then(|face| face_domains.get(&(face, edge)))
                        .and_then(Option::as_ref);
                    let Some(domain) = &mut candidate.endpoint_pairs else {
                        return true;
                    };
                    domain.retain(|pair| {
                        seed.is_none_or(|seed| same_unordered_pair(*pair, seed))
                            && opposite.is_none_or(|opposite| opposite.contains(pair))
                    });
                    !domain.is_empty()
                })
            });
            if assignments.is_empty() {
                return None;
            }
        }
        let after = (
            faces.iter().map(Vec::len).sum::<usize>(),
            faces
                .iter()
                .flatten()
                .flatten()
                .filter_map(|candidate| candidate.endpoint_pairs.as_ref().map(Vec::len))
                .sum::<usize>(),
        );
        if after == before {
            break;
        }
    }
    Some(faces)
}

/// Derive endpoint-pair domains for unmatched rows whose candidate placement
/// corners are both bound by exact matched edge runs. Input pairs are physical
/// edge-row ordered; pair orientation is ignored in the returned domains
/// because a missing placement has not yet selected its traversal direction.
#[must_use]
pub fn standard_mesh_placement_endpoint_pairs(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[Option<[usize; 2]>],
) -> Option<Vec<Vec<[usize; 2]>>> {
    let edge_rows = standard_edge_rows(bytes)?;
    if edge_rows.len() != edge_points.len() || edge_rows.len() != edge_faces.len() {
        return None;
    }
    let assignments =
        standard_mesh_pruned_missing_edge_endpoint_assignments(bytes, edge_faces, edge_points)?;
    let mut domains = vec![Vec::new(); edge_rows.len()];
    let mut placement_counts = vec![0usize; edge_rows.len()];
    let mut bound_counts = vec![0usize; edge_rows.len()];
    for face in assignments {
        let mut placements = face.into_iter().flatten().collect::<Vec<_>>();
        placements.sort_unstable_by_key(|candidate| candidate.placement);
        placements.dedup_by_key(|candidate| candidate.placement);
        for candidate in placements {
            let edge = candidate.placement.edge;
            placement_counts[edge] += 1;
            if let Some(pairs) = candidate.endpoint_pairs {
                bound_counts[edge] += 1;
                for pair in pairs {
                    if !domains[edge].contains(&pair) {
                        domains[edge].push(pair);
                    }
                }
            }
        }
    }
    for (edge, domain) in domains.iter_mut().enumerate() {
        if bound_counts[edge] == placement_counts[edge] {
            domain.sort_unstable();
        } else {
            domain.clear();
        }
    }
    Some(domains)
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
                    if ports[0] == ports[1] || left != right {
                        *pair = Some([left, right]);
                    }
                }
            }
        }
        if before == (port_points.len(), resolved.iter().flatten().count()) {
            break;
        }
    }
    let (resolved_ports, resolved_candidates): (Vec<_>, Vec<_>) = edge_ports
        .iter()
        .copied()
        .zip(resolved.iter().copied())
        .filter_map(|(ports, pair)| pair.map(|pair| (ports, vec![pair])))
        .unzip();
    edge_port_candidate_assignment(&resolved_ports, &resolved_candidates, false)?;
    Some(resolved)
}

/// Propagate endpoint points through the subgraph of edges carrying native
/// endpoint identities. Edges without a native identity pair remain
/// unresolved and do not weaken or invalidate known components.
#[must_use]
pub fn propagate_partial_edge_port_points(
    edge_ports: &[Option<[u32; 2]>],
    endpoint_pairs: &[Option<[usize; 2]>],
) -> Option<Vec<Option<[usize; 2]>>> {
    if edge_ports.len() != endpoint_pairs.len() {
        return None;
    }
    let known = edge_ports
        .iter()
        .enumerate()
        .filter_map(|(edge, ports)| ports.map(|ports| (edge, ports)))
        .collect::<Vec<_>>();
    if known.is_empty() {
        return Some(endpoint_pairs.to_vec());
    }
    let ports = known.iter().map(|(_, ports)| *ports).collect::<Vec<_>>();
    let pairs = known
        .iter()
        .map(|(edge, _)| endpoint_pairs[*edge])
        .collect::<Vec<_>>();
    let propagated = propagate_edge_port_points(&ports, &pairs)?;
    let mut resolved = endpoint_pairs.to_vec();
    for ((edge, _), pair) in known.into_iter().zip(propagated) {
        resolved[edge] = pair;
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
    solution_key: Option<Vec<[usize; 2]>>,
    ambiguous: bool,
    exhausted: bool,
    states: usize,
    require_unique: bool,
}

impl PortCandidateSearch<'_> {
    fn compatible(&self, edge: usize, pair: [usize; 2]) -> Vec<[usize; 2]> {
        let mut oriented = vec![pair];
        if pair[0] != pair[1] {
            oriented.push([pair[1], pair[0]]);
        }
        oriented.retain(|points| {
            (self.ports[edge][0] == self.ports[edge][1]) == (points[0] == points[1])
                && self.ports[edge].iter().zip(points).all(|(&port, point)| {
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

    fn assign(&mut self, edge: usize, points: [usize; 2]) -> Vec<(u32, usize)> {
        let mut inserted = Vec::new();
        for (&port, point) in self.ports[edge].iter().zip(points) {
            if let std::collections::hash_map::Entry::Vacant(entry) = self.port_points.entry(port) {
                entry.insert(point);
                self.point_ports.insert(point, port);
                inserted.push((port, point));
            }
        }
        self.edge_pairs[edge] = Some(points);
        inserted
    }

    fn unassign(&mut self, edge: usize, inserted: Vec<(u32, usize)>) {
        self.edge_pairs[edge] = None;
        for (port, point) in inserted {
            self.port_points.remove(&port);
            self.point_ports.remove(&point);
        }
    }

    fn rollback(&mut self, propagated: Vec<(usize, Vec<(u32, usize)>)>) {
        for (edge, inserted) in propagated.into_iter().rev() {
            self.unassign(edge, inserted);
        }
    }

    pub(crate) fn search(&mut self) {
        // Native-port binding precedes geometric incidence fallback but can
        // still contain symmetric coordinate assignments. Ambiguity beyond
        // this bound is retained for later paths rather than partially bound.
        const MAX_STATES: usize = 1_024;
        if self.ambiguous || self.exhausted || (!self.require_unique && self.solution.is_some()) {
            return;
        }
        let mut propagated = Vec::new();
        let branch = loop {
            let mut best = None;
            let mut progress = false;
            let mut incomplete = false;
            for edge in 0..self.ports.len() {
                if self.edge_pairs[edge].is_some() {
                    continue;
                }
                incomplete = true;
                let mut options = self.candidates[edge]
                    .iter()
                    .flat_map(|pair| self.compatible(edge, *pair));
                let first = options.next();
                let second = options.next();
                if first.is_none() {
                    self.rollback(propagated);
                    return;
                }
                if second.is_some() {
                    let count = 2 + options.count();
                    if best.is_none_or(|(stored, _)| count < stored) {
                        best = Some((count, edge));
                    }
                    continue;
                }
                let points = first.expect("one compatible endpoint assignment");
                let inserted = self.assign(edge, points);
                propagated.push((edge, inserted));
                progress = true;
            }
            if !incomplete {
                break None;
            }
            if progress {
                continue;
            }
            break best.map(|(_, edge)| edge);
        };
        let Some(edge) = branch else {
            let candidate = self.edge_pairs.iter().copied().collect::<Option<Vec<_>>>();
            if let Some(candidate) = candidate {
                let key = candidate
                    .iter()
                    .map(|pair| {
                        if pair[0] <= pair[1] {
                            *pair
                        } else {
                            [pair[1], pair[0]]
                        }
                    })
                    .collect::<Vec<_>>();
                match &self.solution_key {
                    Some(solution) if self.require_unique && *solution != key => {
                        self.ambiguous = true;
                    }
                    None => {
                        self.solution = Some(candidate);
                        self.solution_key = Some(key);
                    }
                    Some(_) => {}
                }
            }
            self.rollback(propagated);
            return;
        };
        if self.states >= MAX_STATES {
            self.exhausted = true;
        } else {
            self.states += 1;
            'candidates: for candidate in 0..self.candidates[edge].len() {
                for points in self.compatible(edge, self.candidates[edge][candidate]) {
                    let inserted = self.assign(edge, points);
                    self.search();
                    self.unassign(edge, inserted);
                    if !self.require_unique && self.solution.is_some() {
                        break 'candidates;
                    }
                }
            }
        }
        self.rollback(propagated);
    }
}

/// Bind native edge endpoint identities to coordinate rows while respecting
/// every edge's geometrically admissible unordered endpoint pairs.
#[must_use]
pub fn bind_edge_port_candidates(
    ports: &[[u32; 2]],
    candidates: &[Vec<[usize; 2]>],
) -> Option<Vec<[usize; 2]>> {
    edge_port_candidate_assignment(ports, candidates, true)
}

fn edge_port_candidate_assignment(
    ports: &[[u32; 2]],
    candidates: &[Vec<[usize; 2]>],
    require_unique: bool,
) -> Option<Vec<[usize; 2]>> {
    if ports.len() != candidates.len() || candidates.iter().any(Vec::is_empty) {
        return None;
    }
    let mut dependencies = UnionFind::new(ports.len());
    let mut edge_by_port = HashMap::new();
    let mut edge_by_point = HashMap::new();
    for edge in 0..ports.len() {
        for port in ports[edge] {
            if let Some(previous) = edge_by_port.insert(port, edge) {
                dependencies.union(previous, edge);
            }
        }
        for point in candidates[edge].iter().flatten() {
            if let Some(previous) = edge_by_point.insert(*point, edge) {
                dependencies.union(previous, edge);
            }
        }
    }
    let mut components = HashMap::<usize, Vec<usize>>::new();
    for edge in 0..ports.len() {
        components
            .entry(dependencies.find(edge))
            .or_default()
            .push(edge);
    }
    let mut components = components.into_values().collect::<Vec<_>>();
    components.sort_by_key(|component| component[0]);
    let mut solution = vec![None; ports.len()];
    for component in components {
        let component_ports = component
            .iter()
            .map(|edge| ports[*edge])
            .collect::<Vec<_>>();
        let component_candidates = component
            .iter()
            .map(|edge| candidates[*edge].clone())
            .collect::<Vec<_>>();
        let mut search = PortCandidateSearch {
            ports: &component_ports,
            candidates: &component_candidates,
            port_points: HashMap::new(),
            point_ports: HashMap::new(),
            edge_pairs: vec![None; component.len()],
            solution: None,
            solution_key: None,
            ambiguous: false,
            exhausted: false,
            states: 0,
            require_unique,
        };
        search.search();
        if search.ambiguous || search.exhausted {
            return None;
        }
        let component_solution = search.solution?;
        for (&edge, pair) in component.iter().zip(component_solution) {
            solution[edge] = Some(pair);
        }
    }
    solution.into_iter().collect()
}

pub(crate) fn same_unordered_pair(left: [usize; 2], right: [usize; 2]) -> bool {
    left == right || left == [right[1], right[0]]
}

pub(crate) fn motif_port_points(
    trims: &[TrimRecord],
    vertex_count: usize,
) -> Option<HashMap<u32, usize>> {
    fn columns(record: &TrimRecord) -> Option<([u32; 2], [u32; 2])> {
        let expected = record
            .independent_count
            .checked_mul(3)?
            .checked_add(record.strip_lengths.iter().sum())?
            .checked_add(record.fan_lengths.iter().sum())?;
        if expected != record.handles.len() {
            return None;
        }
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
