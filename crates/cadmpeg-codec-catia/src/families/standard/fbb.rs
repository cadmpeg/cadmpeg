//! Byte-level parsing for standard nested CATIA V5 B-rep (`FBB`) streams:
//! edge/vertex tables, trim records, packet triangles, and face parsers.

use cadmpeg_core::decode::{View, WorkBudget};

use crate::families::standard::topology::{
    reconstruct, reconstruct_incidence, reconstruct_incidence_with_edge_classes_and_mesh, Boundary,
    CoedgeUse, EdgeBoundaryLayout, EdgeRow, StandardTopology, TrimRecord,
};
use crate::layout::fbb_face_row as fbb_row;
use crate::solve::incidence::reconstruct_incidence_candidates;
use crate::solve::mesh_quotient::MeshQuotient;
use crate::solve::missing_edge::{expand_deferred_edge_port_components, motif_port_points};
use crate::solve::UnionFind;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(crate) const EDGE_DELIMITER: [u8; 8] = [0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
const VERTEX_RECORD_BYTES: usize = 3 + 3 * size_of::<f32>();
const TRIM_KINDS: [u8; 14] = [
    0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f,
];

// A unit vector is stored as three binary32 values. Rounding a unit
// direction to binary32 changes its squared norm by less than 2.1e-7; this
// bound leaves room for binary32 arithmetic used by a writer without
// admitting a materially non-unit frame.
const FRAME_VECTOR_NORM2_TOLERANCE: f64 = 1e-6;

/// Number of face rows in the governing standard topology spine. The spine is
/// the unique largest contiguous stride-eight FBB run; shorter marker runs are
/// not members of this face population. Equal-largest runs leave ownership
/// unresolved.
#[must_use]
pub fn standard_face_count(bytes: &[u8]) -> Option<usize> {
    let selected = selected_standard_run(bytes)?;
    let layouts = fbb_population_layouts(bytes);
    if layouts.is_empty()
        || layouts.iter().any(|layout| {
            (layout.face_start, layout.face_count, layout.after_faces)
                == (selected.0, selected.1, selected.2)
        })
    {
        Some(selected.1)
    } else {
        None
    }
}

/// Number of physical edge rows in the admitted standard edge-table form.
///
/// The count is available without solving trim incidence or mesh topology,
/// so it can gate the independent `0x60` support-table walk.
#[must_use]
pub(crate) fn standard_edge_count(bytes: &[u8]) -> Option<usize> {
    let (_, _, after_faces) = selected_standard_run(bytes)?;
    parse_standard_edge_tables(bytes, after_faces).map(|(rows, _)| rows.len())
}

/// Number of physical edge rows in the width-selected FBB-only tables.
#[must_use]
pub(crate) fn fbb_only_edge_count(bytes: &[u8]) -> Option<usize> {
    let (_, _, after_faces) = largest_fbb_run(bytes)?;
    parse_fbb_edge_tables(bytes, after_faces).map(|(rows, _, _, _)| rows.len())
}

/// RGBA display color for each positional standard face row.
#[must_use]
pub fn standard_face_colors(bytes: &[u8]) -> Option<Vec<[u8; 4]>> {
    let (start, count, _) = selected_standard_run(bytes)?;
    let marker: [u8; 4] = bytes.get(start..start + fbb_row::ALPHA)?.try_into().ok()?;
    (0..count)
        .map(|index| {
            let row =
                bytes.get(start + index * fbb_row::LEN..start + (index + 1) * fbb_row::LEN)?;
            (row[..fbb_row::ALPHA] == marker).then_some([
                row[fbb_row::RED],
                row[fbb_row::GREEN],
                row[fbb_row::BLUE],
                row[fbb_row::ALPHA],
            ])
        })
        .collect()
}

fn trim_frame_vectors(
    bytes: &[u8],
    face_start: usize,
    face_count: usize,
) -> Option<Vec<Option<[f64; 3]>>> {
    let solutions = [1, 2, 3]
        .into_iter()
        .filter_map(|width| parse_trim_chain(bytes, face_start, face_count, width))
        .collect::<Vec<_>>();
    let [records] = <[Vec<TrimRecord>; 1]>::try_from(solutions).ok()?;
    Some(
        records
            .into_iter()
            .map(|record| record.frame_vector)
            .collect(),
    )
}

/// Unit frame vector for each positional standard trim packet. The result is
/// index-aligned with the expected face-roster population; packets without the
/// optional vector retain an empty slot. When every detected FBB population
/// has one unique trim chain and their concatenated length equals
/// `expected_face_count`, the result concatenates those population-local
/// vectors in source order; otherwise it uses the established
/// single-population selection.
#[must_use]
pub fn standard_face_frame_vectors(
    bytes: &[u8],
    expected_face_count: usize,
) -> Vec<Option<[f64; 3]>> {
    let runs = crate::container::fbb_run_ranges(bytes);
    if runs.len() > 1 {
        let combined = runs
            .iter()
            .map(|range| trim_frame_vectors(bytes, range.start, range.len() / fbb_row::LEN))
            .collect::<Option<Vec<_>>>();
        if let Some(vectors) = combined
            .filter(|vectors| vectors.iter().map(Vec::len).sum::<usize>() == expected_face_count)
        {
            return vectors.into_iter().flatten().collect();
        }
    }
    let Some((face_start, face_count, _)) = selected_standard_run(bytes) else {
        return Vec::new();
    };
    trim_frame_vectors(bytes, face_start, face_count).unwrap_or_default()
}

/// Return the counted vertex table of an admitted standard nested spine.
#[must_use]
pub(crate) fn standard_vertex_points(bytes: &[u8]) -> Option<Vec<[f64; 3]>> {
    let (_, _, after_faces) = selected_standard_run(bytes)?;
    let (_, vertex_header) = parse_standard_edge_tables(bytes, after_faces)?;
    parse_vertex_table(bytes, vertex_header)
}

/// Coordinates from the counted vertex table following a complete FBB-only
/// edge-table walk.
#[must_use]
pub(crate) fn fbb_only_vertex_points(bytes: &[u8]) -> Option<Vec<[f64; 3]>> {
    let (_, _, after_faces) = largest_fbb_run(bytes)?;
    let (_, _, vertex_header, _) = parse_fbb_edge_tables(bytes, after_faces)?;
    parse_vertex_table(bytes, vertex_header)
}

/// Parses the counted standard spine, positional trim packets, mesh boundary
/// cycles, physical edge uses, and port/corner vertex equivalence classes.
/// Returns `None` unless every positional face boundary is unambiguous.
#[must_use]
pub fn parse_standard(bytes: &[u8]) -> Option<StandardTopology> {
    let (face_start, face_count, after_faces) = selected_standard_run(bytes)?;
    let (edge_rows, vertex_header, handle_width) =
        parse_standard_edge_tables_with_width(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    let trims = parse_trim_chain(bytes, face_start, face_count, handle_width)?;
    reconstruct(edge_rows, vertex_points, &trims)
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
    let (face_start, face_count, after_faces) = selected_standard_run(bytes)?;
    let (edge_rows, vertex_header, handle_width) =
        parse_standard_edge_tables_with_width(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    if edge_rows.len() != edge_faces.len() || edge_rows.len() != circle_anchors.len() {
        return None;
    }
    let trims = parse_trim_chain(bytes, face_start, face_count, handle_width)?;
    let port_points = motif_port_points(&trims, vertex_points.len())?;
    let edge_points = edge_rows
        .iter()
        .map(|row| {
            Some([
                *port_points.get(row.handles.first()?)?,
                *port_points.get(row.handles.last()?)?,
            ])
        })
        .collect::<Option<Vec<[usize; 2]>>>()?;
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
    if !anchors_match {
        return None;
    }
    reconstruct_incidence(
        edge_rows,
        vertex_points,
        edge_faces,
        &edge_points,
        face_count,
    )
}

/// Reconstruct standard topology while treating equal curve-class identifiers
/// as interchangeable serialized edge rows during incidence-slot completion.
#[must_use]
pub fn parse_standard_endpoints_with_edge_classes(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    edge_classes: Option<&[usize]>,
) -> Option<StandardTopology> {
    let (_, face_count, after_faces) = selected_standard_run(bytes)?;
    let (edge_rows, vertex_header) = parse_standard_edge_tables(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_points.len()
        || edge_classes.is_some_and(|classes| classes.len() != edge_rows.len())
        || edge_points
            .iter()
            .flatten()
            .any(|point| *point >= vertex_points.len())
    {
        return None;
    }
    reconstruct_incidence_with_edge_classes_and_mesh(
        edge_rows,
        vertex_points,
        edge_faces,
        edge_points,
        face_count,
        edge_classes,
        Some(bytes),
    )
}

/// Collapse equal endpoint identities and propagate correlated edge-pair
/// support to a fixpoint. Only serialized pairs supported by both resulting
/// port domains are retained.
#[must_use]
pub fn prune_edge_candidates_by_port_domains(
    edge_ports: &[[u32; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
) -> Option<Vec<Vec<[usize; 2]>>> {
    prune_edge_candidates_by_port_domains_with_deferred(edge_ports, edge_candidates, &[])
}

/// Apply trim-port equality to endpoint candidates whose duplicate face slot
/// is settled. Rows with an open duplicate-face domain do not contribute their
/// candidate set to port-domain propagation; their candidates are filtered by
/// the settled neighbouring ports after that propagation completes.
#[must_use]
pub fn prune_edge_candidates_by_port_domains_with_deferred(
    edge_ports: &[[u32; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    deferred_edges: &[bool],
) -> Option<Vec<Vec<[usize; 2]>>> {
    if edge_ports.len() != edge_candidates.len() || edge_candidates.iter().any(Vec::is_empty) {
        return None;
    }
    if !deferred_edges.is_empty() && deferred_edges.len() != edge_candidates.len() {
        return None;
    }
    let mut effective_deferred = if deferred_edges.is_empty() {
        edge_candidates.iter().map(|_| false).collect()
    } else {
        deferred_edges.to_vec()
    };
    if !expand_deferred_edge_port_components(edge_ports, &mut effective_deferred) {
        return None;
    }
    let is_deferred = |edge: usize| effective_deferred[edge];
    let all_points = edge_candidates
        .iter()
        .flatten()
        .flatten()
        .copied()
        .collect::<HashSet<_>>();
    let mut domains = Vec::with_capacity(edge_candidates.len() * 2);
    for (edge, candidates) in edge_candidates.iter().enumerate() {
        let domain = Arc::new(if is_deferred(edge) {
            all_points.clone()
        } else {
            candidates.iter().flatten().copied().collect::<HashSet<_>>()
        });
        domains.push(domain.clone());
        domains.push(domain);
    }
    let mut quotient = MeshQuotient {
        union: UnionFind::new(edge_candidates.len() * 2),
        domains,
        members: (0..edge_candidates.len() * 2)
            .map(|node| vec![node])
            .collect(),
    };
    let mut node_by_port = HashMap::new();
    for (edge, ports) in edge_ports.iter().enumerate() {
        for (endpoint, port) in ports.iter().copied().enumerate() {
            let node = edge * 2 + endpoint;
            if let Some(&previous) = node_by_port.get(&port) {
                quotient.merge(previous, node)?;
            } else {
                node_by_port.insert(port, node);
            }
        }
    }
    let mut constrained_candidates = edge_candidates.to_vec();
    for (edge, candidates) in constrained_candidates.iter_mut().enumerate() {
        if is_deferred(edge) {
            candidates.clear();
        }
    }
    if !quotient.edge_domains_viable(&constrained_candidates) {
        return None;
    }
    edge_candidates
        .iter()
        .enumerate()
        .map(|(edge, candidates)| {
            let left = quotient.union.find(edge * 2);
            let right = quotient.union.find(edge * 2 + 1);
            let mut filtered = candidates
                .iter()
                .copied()
                .filter(|pair| {
                    if left == right {
                        pair[0] == pair[1] && quotient.domains[left].contains(&pair[0])
                    } else {
                        (quotient.domains[left].contains(&pair[0])
                            && quotient.domains[right].contains(&pair[1]))
                            || (quotient.domains[left].contains(&pair[1])
                                && quotient.domains[right].contains(&pair[0]))
                    }
                })
                .collect::<Vec<_>>();
            for pair in &mut filtered {
                pair.sort_unstable();
            }
            filtered.sort_unstable();
            filtered.dedup();
            (!filtered.is_empty()).then_some(filtered)
        })
        .collect()
}

/// Reconstruct standard topology while resolving edges that have multiple
/// geometrically valid endpoint pairs. Candidate pairs and edge rows use their
/// serialized order as the stable gauge when equivalent assignments permute
/// indistinguishable line rows. The selected assignment must close every face
/// cycle and satisfy radial orientation. Search charges the supplied topology
/// phase budget.
#[must_use]
pub fn parse_standard_endpoint_candidates(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    budget: &WorkBudget<'_>,
) -> Option<StandardTopology> {
    let (_, face_count, after_faces) = selected_standard_run(bytes)?;
    let (edge_rows, vertex_header) = parse_standard_edge_tables(bytes, after_faces)?;
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
        None,
        face_count,
        budget,
    )
}

/// Reconstruct standard topology from geometric endpoint candidates while
/// enforcing the serialized endpoint-port equality quotient during search.
/// Search charges the supplied topology phase budget.
#[must_use]
pub fn parse_standard_port_endpoint_candidates(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_ports: &[[u32; 2]],
    budget: &WorkBudget<'_>,
) -> Option<StandardTopology> {
    let (_, face_count, after_faces) = selected_standard_run(bytes)?;
    let (edge_rows, vertex_header) = parse_standard_edge_tables(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_candidates.len()
        || edge_rows.len() != edge_ports.len()
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
        Some(edge_ports),
        face_count,
        budget,
    )
}

/// Reconstruct an FBB-only topology from one exact endpoint pair per edge row.
///
/// The FBB-only carrier uses its own two-table delimiter walk rather than the
/// standard edge-table grammar. Those counted tables provide the physical edge
/// rows and the counted vertex table provides the coordinate population. When
/// the native endpoint registry has already selected every pair, face
/// incidence can be closed directly. This path does not infer endpoint
/// identities from trim order.
#[must_use]
pub(crate) fn parse_fbb_endpoints_with_edge_classes(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    edge_classes: Option<&[usize]>,
) -> Option<StandardTopology> {
    let (_, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, _, vertex_header, _) = parse_fbb_edge_tables(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_points.len()
        || edge_classes.is_some_and(|classes| classes.len() != edge_rows.len())
        || edge_points
            .iter()
            .flatten()
            .any(|point| *point >= vertex_points.len())
    {
        return None;
    }
    reconstruct_incidence_with_edge_classes_and_mesh(
        edge_rows,
        vertex_points,
        edge_faces,
        edge_points,
        face_count,
        edge_classes,
        Some(bytes),
    )
}

pub(crate) fn parse_fbb_edge_tables(
    bytes: &[u8],
    position: usize,
) -> Option<(Vec<EdgeRow>, Vec<usize>, usize, usize)> {
    // FBB-only tables select one width by the complete table-and-vertex walk;
    // accepting the first delimiter match would assign a wrong handle grammar.
    let solutions = [1, 2, 3]
        .into_iter()
        .filter_map(|handle_width| {
            let parsed = parse_fbb_edge_tables_width(bytes, position, handle_width)?;
            parse_vertex_table(bytes, parsed.2)
                .is_some()
                .then_some(parsed)
        })
        .collect::<Vec<_>>();
    <[_; 1]>::try_from(solutions)
        .ok()
        .map(|[solution]| solution)
}

pub(crate) fn parse_fbb_edge_tables_width(
    bytes: &[u8],
    mut position: usize,
    handle_width: usize,
) -> Option<(Vec<EdgeRow>, Vec<usize>, usize, usize)> {
    let mut rows = Vec::new();
    let mut scopes = Vec::new();
    let mut table_count = 0;
    let mut delimiter_family = None;
    loop {
        if bytes.get(position) != Some(&0x01) {
            return None;
        }
        let kind = *bytes.get(position + 1)?;
        let expected_kind = u8::try_from(table_count + 1).ok()?;
        if kind != expected_kind {
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
            if arity > bytes.len().saturating_sub(position) / handle_width {
                return None;
            }
            let mut handles = Vec::with_capacity(arity);
            for _ in 0..arity {
                let mut encoded = [0u8; 4];
                encoded[4 - handle_width..]
                    .copy_from_slice(bytes.get(position..position + handle_width)?);
                handles.push(u32::from_be_bytes(encoded));
                position += handle_width;
            }
            rows.push(EdgeRow {
                kind,
                handles,
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            });
            scopes.push(table_count);
        }
        table_count += 1;
        let delimiter = bytes.get(position..position + EDGE_DELIMITER.len())?;
        let family = match handle_width {
            2 if delimiter[0] == 0x10
                && delimiter[1] >= 0x14
                && delimiter[1] != 0x24
                && delimiter[1] & 0x0f == 0x04
                && delimiter[2..] == EDGE_DELIMITER[2..] =>
            {
                delimiter[1] >> 4
            }
            1 | 3 if delimiter == EDGE_DELIMITER => 0x02,
            _ => return None,
        };
        if delimiter_family
            .replace(family)
            .is_some_and(|value| value != family)
        {
            return None;
        }
        position += EDGE_DELIMITER.len();
        if bytes.get(position..position + 2) == Some(&[0x01, 0x06]) {
            break;
        }
    }
    (table_count == 2).then_some((rows, scopes, position, handle_width))
}

/// Recover the row layout used by an FBB-only table from its trim boundaries.
///
/// Complete rows remain complete whenever their stored sequence occurs on a
/// recovered cycle. Some mixed FBB tables store flanking corner handles around
/// an interior sample sequence instead; that form is admitted only when the
/// complete sequence has no occurrence and the interior sequence has at most
/// one occurrence per cycle. Rows with no boundary match remain complete so
/// their fixed unmatched span is preserved for the later placement solver.
pub(crate) fn classify_fbb_edge_layouts(rows: &mut [EdgeRow], trims: &[TrimRecord]) -> Option<()> {
    let cycles = trims
        .iter()
        .map(|trim| boundary_cycles(&trim.triangles))
        .collect::<Option<Vec<_>>>()?;
    for row in rows {
        let complete_matches = cycles
            .iter()
            .flat_map(|face| face.iter())
            .map(|cycle| pattern_match_count(cycle, &row.handles))
            .sum::<usize>();
        if complete_matches != 0 {
            continue;
        }
        let Some(interior) = row.handles.get(1..row.handles.len().checked_sub(1)?) else {
            continue;
        };
        if interior.is_empty() {
            continue;
        }
        let interior_counts = cycles
            .iter()
            .flat_map(|face| face.iter())
            .map(|cycle| pattern_match_count(cycle, interior))
            .collect::<Vec<_>>();
        if interior_counts.iter().sum::<usize>() != 0
            && interior_counts.iter().all(|count| *count <= 1)
        {
            row.boundary_layout = EdgeBoundaryLayout::InteriorWithFlankingCorners;
        }
    }
    Some(())
}

fn pattern_match_count(cycle: &[u32], pattern: &[u32]) -> usize {
    if pattern.is_empty() || pattern.len() > cycle.len() {
        return 0;
    }
    (0..cycle.len())
        .filter(|start| {
            let forward = pattern
                .iter()
                .enumerate()
                .all(|(offset, handle)| cycle[(*start + offset) % cycle.len()] == *handle);
            let reversed = pattern
                .iter()
                .rev()
                .enumerate()
                .all(|(offset, handle)| cycle[(*start + offset) % cycle.len()] == *handle);
            forward || reversed
        })
        .count()
}

/// One independently source-closed standard FBB population.
///
/// A marker run becomes a population only when its fixed-width edge tables,
/// counted vertex table, trim chain, and reconstructed topology all select one
/// result. Marker count alone is not a body or a topology binding.
#[derive(Debug, Clone)]
pub(crate) struct StandardFbbGroup {
    pub(crate) face_start: usize,
    pub(crate) face_count: usize,
    pub(crate) after_faces: usize,
    pub(crate) topology: StandardTopology,
}

/// Find every independently source-closed standard FBB population.
///
/// The result is intentionally not reduced to the largest run. A caller that
/// has a single result may select it; a caller that has multiple results must
/// bind their carrier and incidence rosters before creating neutral bodies.
#[must_use]
pub(crate) fn standard_fbb_groups(bytes: &[u8]) -> Vec<StandardFbbGroup> {
    crate::container::fbb_run_ranges(bytes)
        .into_iter()
        .filter_map(|range| parse_standard_group(bytes, range.start, range.len() / fbb_row::LEN))
        .collect()
}

/// A source-closed FBB layout whose topology may still require the global
/// endpoint solver. The edge and vertex counts are structural population
/// keys; they are not body selection by themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FbbPopulationLayout {
    pub(crate) face_start: usize,
    pub(crate) face_count: usize,
    pub(crate) after_faces: usize,
    pub(crate) edge_count: usize,
    pub(crate) vertex_count: usize,
    pub(crate) fbb_edge_table: bool,
}

fn vertex_table_end(bytes: &[u8], position: usize) -> Option<usize> {
    if bytes.get(position..position + 2) != Some(&[0x01, 0x06]) {
        return None;
    }
    let mut cursor = position + 2;
    let count = parse_count(bytes, &mut cursor)?;
    let end = cursor.checked_add(count.checked_mul(VERTEX_RECORD_BYTES)?)?;
    (parse_vertex_table(bytes, position).is_some()).then_some(end)
}

/// Return one source-closed FBB population as a self-contained topology
/// spine. The slice starts at its complete trim chain and ends after its
/// counted vertex table, so the existing single-population parsers can be
/// reused without seeing neighboring populations.
#[must_use]
pub(crate) fn population_spine<'a>(
    bytes: &'a [u8],
    layout: &FbbPopulationLayout,
) -> Option<&'a [u8]> {
    let (_, vertex_header, handle_width) =
        parse_standard_edge_tables_with_width(bytes, layout.after_faces).or_else(|| {
            parse_fbb_edge_tables(bytes, layout.after_faces).map(
                |(_, _, vertex_header, handle_width)| (Vec::new(), vertex_header, handle_width),
            )
        })?;
    let trim_start =
        parse_trim_chain_start(bytes, layout.face_start, layout.face_count, handle_width)?.0;
    let end = vertex_table_end(bytes, vertex_header)?;
    bytes.get(trim_start..end)
}

/// Find every FBB face run with a complete local trim, edge-table, and vertex
/// walk, without requiring endpoint incidence to be solved.
#[must_use]
pub(crate) fn fbb_population_layouts(bytes: &[u8]) -> Vec<FbbPopulationLayout> {
    crate::container::fbb_run_ranges(bytes)
        .into_iter()
        .filter_map(|range| {
            let face_count = range.len() / fbb_row::LEN;
            let after_faces = range.end;
            let (edge_rows, vertex_header, handle_width, fbb_edge_table) =
                parse_standard_edge_tables_with_width(bytes, after_faces)
                    .map(|(rows, vertex_header, handle_width)| {
                        (rows, vertex_header, handle_width, false)
                    })
                    .or_else(|| {
                        parse_fbb_edge_tables(bytes, after_faces).map(
                            |(rows, _, vertex_header, handle_width)| {
                                (rows, vertex_header, handle_width, true)
                            },
                        )
                    })?;
            let vertex_count = parse_vertex_table(bytes, vertex_header)?.len();
            parse_trim_chain(bytes, range.start, face_count, handle_width)?;
            Some(FbbPopulationLayout {
                face_start: range.start,
                face_count,
                after_faces,
                edge_count: edge_rows.len(),
                vertex_count,
                fbb_edge_table,
            })
        })
        .collect()
}

fn parse_standard_group(
    bytes: &[u8],
    face_start: usize,
    face_count: usize,
) -> Option<StandardFbbGroup> {
    let after_faces = face_start.checked_add(face_count.checked_mul(fbb_row::LEN)?)?;
    let (edge_rows, vertex_header, handle_width) =
        parse_standard_edge_tables_with_width(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    let trims = parse_trim_chain(bytes, face_start, face_count, handle_width)?;
    let topology = reconstruct(edge_rows, vertex_points, &trims)?;
    Some(StandardFbbGroup {
        face_start,
        face_count,
        after_faces,
        topology,
    })
}

pub(crate) fn selected_standard_run(bytes: &[u8]) -> Option<(usize, usize, usize)> {
    let ranges = crate::container::fbb_run_ranges(bytes);
    if let [range] = ranges.as_slice() {
        // A single marker run has no competing population to disambiguate.
        return Some((range.start, range.len() / fbb_row::LEN, range.end));
    }
    let groups = standard_fbb_groups(bytes);
    match groups.as_slice() {
        [group] if group.topology.face_count() == group.face_count => {
            Some((group.face_start, group.face_count, group.after_faces))
        }
        [] => largest_fbb_run(bytes),
        _ => None,
    }
}

pub(crate) fn largest_fbb_run(bytes: &[u8]) -> Option<(usize, usize, usize)> {
    let mut best = None;
    let mut tied = false;
    let mut position = 0;
    while position + fbb_row::LEN <= bytes.len() {
        if crate::container::is_fbb_row(&bytes[position..]) {
            let start = position;
            let mut count = 0;
            while position + fbb_row::LEN <= bytes.len()
                && crate::container::is_fbb_row(&bytes[position..])
            {
                count += 1;
                position += fbb_row::LEN;
            }
            if best.is_none_or(|(_, best_count, _)| count > best_count) {
                best = Some((start, count, position));
                tied = false;
            } else if best.is_some_and(|(_, best_count, _)| count == best_count) {
                tied = true;
            }
        } else {
            position += 1;
        }
    }
    if tied {
        None
    } else {
        best
    }
}

#[cfg(test)]
mod appearance_tests {
    use super::standard_face_colors;

    #[test]
    fn face_colors_are_abgr_and_require_one_marker_family() {
        let bytes = [
            0xb0, 4, 4, 0xff, 0x99, 0x1f, 0x1a, 0xd1, 0xb0, 4, 4, 0xff, 0xff, 0xe0, 0x3d, 0x14,
        ];
        assert_eq!(
            standard_face_colors(&bytes),
            Some(vec![[0xd1, 0x1a, 0x1f, 0x99], [0x14, 0x3d, 0xe0, 0xff]])
        );
        let mut mixed = bytes;
        mixed[8] = 0x30;
        assert_eq!(standard_face_colors(&mixed), None);
    }
}

fn parse_count(bytes: &[u8], position: &mut usize) -> Option<usize> {
    let first = *bytes.get(*position)?;
    *position += 1;
    if first != 0xff {
        return Some(usize::from(first));
    }
    let value = View::u32_le_at(bytes, *position)?;
    *position += 4;
    usize::try_from(value).ok()
}

pub(crate) fn parse_edge_tables(bytes: &[u8], position: usize) -> Option<(Vec<EdgeRow>, usize)> {
    if let Some(result) = parse_standard_edge_tables(bytes, position) {
        return Some(result);
    }
    parse_fbb_edge_tables(bytes, position).map(|(rows, _, vertex_header, _)| (rows, vertex_header))
}

pub(crate) fn parse_standard_edge_tables(
    bytes: &[u8],
    position: usize,
) -> Option<(Vec<EdgeRow>, usize)> {
    parse_standard_edge_tables_with_width(bytes, position)
        .map(|(rows, vertex_header, _)| (rows, vertex_header))
}

pub(crate) fn parse_standard_edge_tables_with_width(
    bytes: &[u8],
    position: usize,
) -> Option<(Vec<EdgeRow>, usize, usize)> {
    parse_standard_edge_tables_scoped(bytes, position)
        .map(|(rows, _, vertex_header, handle_width)| (rows, vertex_header, handle_width))
}

pub(crate) fn parse_standard_edge_tables_scoped(
    bytes: &[u8],
    position: usize,
) -> Option<(Vec<EdgeRow>, Vec<usize>, usize, usize)> {
    // The full standard spine uses u16be rows and may contain one or more
    // counted tables. Keep that grammar first so a malformed standard walk
    // cannot silently enter the compact form below.
    if let Some((rows, scopes, vertex_header)) = parse_edge_tables_scoped_width(bytes, position, 2)
    {
        if parse_vertex_table(bytes, vertex_header).is_some() {
            return Some((rows, scopes, vertex_header, 2));
        }
    }

    // CATIA also emits a compact standard spine with one kind-01 table. Its
    // rows use one selected width and the table is closed directly by the
    // counted vertex table. A two-table walk belongs to the separate FBB-only
    // family and must not be admitted through this fallback.
    if bytes.get(position..position + 2) != Some(&[0x01, 0x01]) {
        return None;
    }
    let (rows, scopes, vertex_header, handle_width) =
        parse_edge_tables_scoped_at_with_width(bytes, position)?;
    (!rows.is_empty()
        && scopes.iter().all(|scope| *scope == 0)
        && rows
            .iter()
            .all(|row| row.boundary_layout == EdgeBoundaryLayout::CompleteBoundaryRun))
    .then_some((rows, scopes, vertex_header, handle_width))
}

#[cfg(test)]
pub(crate) fn parse_edge_tables_at(bytes: &[u8], position: usize) -> Option<(Vec<EdgeRow>, usize)> {
    parse_edge_tables_scoped_at(bytes, position)
        .map(|(rows, _, vertex_header)| (rows, vertex_header))
}

#[cfg(test)]
pub(crate) fn parse_edge_tables_scoped_at(
    bytes: &[u8],
    position: usize,
) -> Option<(Vec<EdgeRow>, Vec<usize>, usize)> {
    parse_edge_tables_scoped_at_with_width(bytes, position)
        .map(|(rows, scopes, vertex_header, _)| (rows, scopes, vertex_header))
}

fn parse_edge_tables_scoped_at_with_width(
    bytes: &[u8],
    position: usize,
) -> Option<(Vec<EdgeRow>, Vec<usize>, usize, usize)> {
    let solutions = [1, 2, 3]
        .into_iter()
        .filter_map(|handle_width| {
            let parsed = parse_edge_tables_scoped_width(bytes, position, handle_width)?;
            parse_vertex_table(bytes, parsed.2).is_some().then_some((
                parsed.0,
                parsed.1,
                parsed.2,
                handle_width,
            ))
        })
        .collect::<Vec<_>>();
    <[_; 1]>::try_from(solutions)
        .ok()
        .map(|[solution]| solution)
}

fn parse_edge_tables_scoped_width(
    bytes: &[u8],
    mut position: usize,
    handle_width: usize,
) -> Option<(Vec<EdgeRow>, Vec<usize>, usize)> {
    let mut rows = Vec::new();
    let mut scopes = Vec::new();
    let mut scope = 0usize;
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
            position += 1;
            let arity = parse_count(bytes, &mut position)?;
            if arity < 2 {
                return None;
            }
            if arity > bytes.len().saturating_sub(position) / handle_width {
                return None;
            }
            let mut handles = Vec::with_capacity(arity);
            for _ in 0..arity {
                let mut encoded = [0u8; 4];
                encoded[4 - handle_width..]
                    .copy_from_slice(bytes.get(position..position + handle_width)?);
                handles.push(u32::from_be_bytes(encoded));
                position += handle_width;
            }
            rows.push(EdgeRow {
                kind,
                handles,
                boundary_layout: if arity == 2 {
                    EdgeBoundaryLayout::CompleteBoundaryRun
                } else {
                    EdgeBoundaryLayout::InteriorWithFlankingCorners
                },
            });
            scopes.push(scope);
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
        scope = scope.checked_add(1)?;
    }
    Some((rows, scopes, position))
}

pub(crate) fn parse_vertex_table(bytes: &[u8], mut position: usize) -> Option<Vec<[f64; 3]>> {
    if !bytes.get(position..)?.starts_with(&[0x01, 0x06]) {
        return None;
    }
    position += 2;
    let count = parse_count(bytes, &mut position)?;
    if count > bytes.len().saturating_sub(position) / VERTEX_RECORD_BYTES {
        return None;
    }
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(position..position + 3)? != [0x05, 0x08, 0x01] {
            return None;
        }
        position += 3;
        let mut point = [0.0; 3];
        for coordinate in &mut point {
            let value = View::f32_le_at(bytes, position)?;
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

pub(crate) fn parse_trim_chain(
    bytes: &[u8],
    end: usize,
    record_count: usize,
    width: usize,
) -> Option<Vec<TrimRecord>> {
    parse_trim_chain_start(bytes, end, record_count, width).map(|(_, records)| records)
}

fn parse_trim_chain_start(
    bytes: &[u8],
    end: usize,
    record_count: usize,
    width: usize,
) -> Option<(usize, Vec<TrimRecord>)> {
    let compact = parse_trim_chain_with_length_encoding(bytes, end, record_count, width, false);
    let wide_u16be = (width == 2)
        .then(|| parse_trim_chain_with_length_encoding(bytes, end, record_count, width, true));
    match (compact, wide_u16be.flatten()) {
        (Some((compact_start, compact)), Some((wide_start, wide)))
            if compact_start == wide_start && compact == wide =>
        {
            Some((compact_start, compact))
        }
        (Some(records), None) | (None, Some(records)) => Some(records),
        (None, None) | (Some(_), Some(_)) => None,
    }
}

fn parse_trim_chain_with_length_encoding(
    bytes: &[u8],
    end: usize,
    record_count: usize,
    width: usize,
    wide_u16be: bool,
) -> Option<(usize, Vec<TrimRecord>)> {
    struct Frame {
        end: usize,
        remaining: usize,
        next_predecessor: usize,
    }

    fn backtrack(frames: &mut Vec<Frame>, reversed: &mut Vec<TrimRecord>) {
        let had_parent = frames.len() > 1;
        frames.pop();
        if had_parent {
            reversed.pop();
        }
    }

    let prefix = bytes.get(..end)?;
    let mut predecessors = HashMap::<usize, Vec<usize>>::new();
    for (start, marker) in prefix.windows(2).enumerate() {
        if marker[0] != 0x01 || !TRIM_KINDS.contains(&marker[1]) {
            continue;
        }
        if let Some(layout) =
            parse_trim_record_layout_with_length_encoding(prefix, start, width, wide_u16be)
        {
            predecessors.entry(layout.end).or_default().push(start);
        }
    }

    let mut solutions = Vec::new();
    let mut reversed = Vec::with_capacity(record_count);
    let mut frames = vec![Frame {
        end,
        remaining: record_count,
        next_predecessor: 0,
    }];
    while !frames.is_empty() && solutions.len() <= 1 {
        let frame = frames.len() - 1;
        if frames[frame].remaining == 0 {
            let chain_start = frames[frame].end;
            let mut records = reversed.clone();
            records.reverse();
            solutions.push((chain_start, records));
            backtrack(&mut frames, &mut reversed);
            continue;
        }
        let predecessor = predecessors
            .get(&frames[frame].end)
            .and_then(|records| records.get(frames[frame].next_predecessor))
            .copied();
        let Some(start) = predecessor else {
            backtrack(&mut frames, &mut reversed);
            continue;
        };
        frames[frame].next_predecessor += 1;
        let Some(record) = parse_trim_record_with_length_encoding(prefix, start, width, wide_u16be)
        else {
            continue;
        };
        let remaining = frames[frame].remaining - 1;
        reversed.push(record);
        frames.push(Frame {
            end: start,
            remaining,
            next_predecessor: 0,
        });
    }
    <[(usize, Vec<TrimRecord>); 1]>::try_from(solutions)
        .ok()
        .map(|[solution]| solution)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TrimRecordLayout {
    kind: u8,
    independent_count: usize,
    strip_count: usize,
    lengths: Vec<usize>,
    frame_vector: Option<[f64; 3]>,
    pub(crate) handle_offset: usize,
    handle_count: usize,
    pub(crate) stored_count: usize,
    packed_two_strip_lengths: bool,
    pub(crate) end: usize,
}

#[cfg(test)]
pub(crate) fn parse_trim_record_layout(
    bytes: &[u8],
    start: usize,
    width: usize,
) -> Option<TrimRecordLayout> {
    let compact = parse_trim_record_layout_with_length_encoding(bytes, start, width, false);
    let wide_u16be = (width == 2)
        .then(|| parse_trim_record_layout_with_length_encoding(bytes, start, width, true));
    match (compact, wide_u16be.flatten()) {
        (Some(compact), Some(wide)) if compact == wide => Some(compact),
        (Some(layout), None) | (None, Some(layout)) => Some(layout),
        (None, None) | (Some(_), Some(_)) => None,
    }
}

fn parse_trim_record_layout_with_length_encoding(
    bytes: &[u8],
    start: usize,
    width: usize,
    wide_u16be: bool,
) -> Option<TrimRecordLayout> {
    if wide_u16be && width != 2 {
        return None;
    }
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
    let b_start = position;
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
    let handle_count = usize::try_from(View::u32_le_at(bytes, position)?).ok()?;
    position += 4;
    if handle_count == 0 {
        return None;
    }
    let frame_vector = if mask & 8 != 0 {
        let components = [
            f64::from(View::f32_le_at(bytes, position)?),
            f64::from(View::f32_le_at(bytes, position + 4)?),
            f64::from(View::f32_le_at(bytes, position + 8)?),
        ];
        position += 12;
        let norm2 = components.iter().map(|value| value * value).sum::<f64>();
        if !components.iter().all(|value| value.is_finite())
            || (norm2 - 1.0).abs() >= FRAME_VECTOR_NORM2_TOLERANCE
        {
            return None;
        }
        Some(components)
    } else {
        None
    };

    // A two-strip packet stores K0 and K1 as two raw bytes before the H lane.
    // The bytes are not a handle.  At width two they happen to occupy one
    // handle-sized slot; at width three they do not, so sizing the lane as
    // `(N + 1) * width` would consume one byte from the next packet.
    let packed_two_strip_lengths =
        kind == 0x42 && b == 2 && bytes.get(b_start).is_some_and(|encoded| *encoded == 2);
    let primitive_count = b.checked_add(c)?;
    if !packed_two_strip_lengths && primitive_count > bytes.len().saturating_sub(position) {
        return None;
    }
    let mut lengths = Vec::with_capacity(primitive_count);
    if !packed_two_strip_lengths {
        for _ in 0..primitive_count {
            let length = if wide_u16be {
                let value = View::u16_be_at(bytes, position)?;
                position += 2;
                usize::from(value)
            } else {
                parse_count(bytes, &mut position)?
            };
            lengths.push(length);
        }
        if 3usize.checked_mul(a)?.checked_add(lengths.iter().sum())? != handle_count {
            return None;
        }
    }
    let stored_count = handle_count;
    let handle_offset = position;
    let byte_count = if packed_two_strip_lengths {
        2usize.checked_add(handle_count.checked_mul(width)?)?
    } else {
        stored_count.checked_mul(width)?
    };
    let end = handle_offset.checked_add(byte_count)?;
    bytes.get(handle_offset..end)?;
    Some(TrimRecordLayout {
        kind,
        independent_count: a,
        strip_count: b,
        lengths,
        frame_vector,
        handle_offset,
        handle_count,
        stored_count,
        packed_two_strip_lengths,
        end,
    })
}

#[cfg(test)]
pub(crate) fn parse_trim_record(bytes: &[u8], start: usize, width: usize) -> Option<TrimRecord> {
    let compact = parse_trim_record_with_length_encoding(bytes, start, width, false);
    let wide_u16be =
        (width == 2).then(|| parse_trim_record_with_length_encoding(bytes, start, width, true));
    match (compact, wide_u16be.flatten()) {
        (Some(compact), Some(wide)) if compact == wide => Some(compact),
        (Some(record), None) | (None, Some(record)) => Some(record),
        (None, None) | (Some(_), Some(_)) => None,
    }
}

fn parse_trim_record_with_length_encoding(
    bytes: &[u8],
    start: usize,
    width: usize,
    wide_u16be: bool,
) -> Option<TrimRecord> {
    let layout = parse_trim_record_layout_with_length_encoding(bytes, start, width, wide_u16be)?;
    let mut position = layout.handle_offset;
    let mut lengths = layout.lengths;
    if layout.packed_two_strip_lengths {
        let packed = bytes.get(position..position + 2)?;
        position += 2;
        lengths = vec![usize::from(packed[0]), usize::from(packed[1])];
        if lengths.iter().sum::<usize>() != layout.handle_count {
            return None;
        }
    }
    let mut handles = Vec::with_capacity(layout.stored_count);
    for _ in 0..layout.stored_count {
        let handle = match width {
            1 => u32::from(*bytes.get(position)?),
            2 => u32::from(View::u16_be_at(bytes, position)?),
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

    let triangles = packet_triangles(
        layout.independent_count,
        layout.strip_count,
        lengths.len().checked_sub(layout.strip_count)?,
        &lengths,
        &handles,
    )?;
    Some(TrimRecord {
        triangles,
        frame_vector: layout.frame_vector,
        handles,
        independent_count: layout.independent_count,
        strip_lengths: lengths[..layout.strip_count].to_vec(),
        fan_lengths: lengths[layout.strip_count..].to_vec(),
        kind: layout.kind,
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

pub(crate) fn boundary_cycles(triangles: &[[u32; 3]]) -> Option<Vec<Vec<u32>>> {
    let mut edge_directions = HashMap::<(u32, u32), u8>::new();
    for &[a, b, c] in triangles {
        for (start, end) in [(a, b), (b, c), (c, a)] {
            if start == end {
                return None;
            }
            let (edge, direction) = if start < end {
                ((start, end), 1)
            } else {
                ((end, start), 2)
            };
            let directions = edge_directions.entry(edge).or_default();
            if *directions & direction != 0 {
                return None;
            }
            *directions |= direction;
        }
    }
    let mut successors = HashMap::new();
    for (&(low, high), &directions) in &edge_directions {
        let boundary = match directions {
            1 => Some((low, high)),
            2 => Some((high, low)),
            3 => None,
            _ => return None,
        };
        if boundary.is_some_and(|(start, end)| successors.insert(start, end).is_some()) {
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

pub(crate) fn cover_cycle(
    cycle: &[u32],
    rows: &[EdgeRow],
    union: &mut UnionFind,
) -> Option<Boundary> {
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

#[cfg(test)]
mod endpoint_tests {
    use super::parse_fbb_endpoints_with_edge_classes;

    fn synthetic_fbb_triangle() -> Vec<u8> {
        let mut bytes = vec![0x01, 0x41, 0x01, 0xff];
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&[0, 1, 2]);

        bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0, 0, 0, 0]);
        bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0, 0, 0, 0]);

        bytes.extend_from_slice(&[0x01, 0x01, 0x03]);
        for handles in [[0, 1], [1, 2], [2, 0]] {
            bytes.extend_from_slice(&[0x02, 0x02]);
            bytes.extend_from_slice(&handles);
        }
        bytes.extend_from_slice(&[
            0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x10, 0x24, 0x04,
            0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x06, 0x03,
        ]);
        for point in [[0.0_f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
            for coordinate in point {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn fbb_endpoint_reconstruction_uses_the_native_edge_pairs() {
        let bytes = synthetic_fbb_triangle();
        let topology = parse_fbb_endpoints_with_edge_classes(
            &bytes,
            &[[0, 1], [0, 1], [0, 1]],
            &[[0, 1], [1, 2], [0, 2]],
            Some(&[0, 1, 2]),
        )
        .expect("native endpoint pairs close the FBB face");

        assert_eq!(topology.face_count(), 2);
        assert_eq!(topology.edge_rows().len(), 3);
        assert_eq!(
            topology.vertex_points(),
            &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        );
        assert_eq!(topology.edge_vertices(), Some(vec![[0, 1], [1, 2], [0, 2]]));
    }
}

#[cfg(test)]
mod tests {
    use super::boundary_cycles;

    #[test]
    fn boundary_cycles_cancel_opposite_triangle_edges() {
        let triangles = [[0, 1, 2], [0, 2, 3]];
        assert_eq!(boundary_cycles(&triangles), Some(vec![vec![0, 1, 2, 3]]));
    }

    #[test]
    fn boundary_cycles_reject_duplicate_directed_edges() {
        let triangles = [[0, 1, 2], [0, 1, 3]];
        assert_eq!(boundary_cycles(&triangles), None);
    }
}
