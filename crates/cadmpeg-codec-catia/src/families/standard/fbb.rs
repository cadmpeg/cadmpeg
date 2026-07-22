//! Byte-level parsing for standard nested CATIA V5 B-rep (`FBB`) streams:
//! edge/vertex tables, trim records, packet triangles, and face parsers.

use crate::families::standard::topology::{
    reconstruct, reconstruct_incidence, reconstruct_incidence_with_edge_classes_and_mesh, Boundary,
    CoedgeUse, EdgeBoundaryLayout, EdgeRow, StandardTopology, TrimRecord,
};
use crate::solve::incidence::reconstruct_incidence_candidates;
use crate::solve::mesh_quotient::MeshQuotient;
use crate::solve::missing_edge::motif_port_points;
use crate::solve::UnionFind;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const FBB_ROW: [u8; 4] = [0x30, 0x04, 0x04, 0xff];
pub(crate) const EDGE_DELIMITER: [u8; 8] = [0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
const TRIM_KINDS: [u8; 14] = [
    0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f,
];

/// Number of face rows in the governing standard topology spine. The spine is
/// the unique largest contiguous stride-eight FBB run; shorter marker runs are
/// not members of this face population. Equal-largest runs leave ownership
/// unresolved.
#[must_use]
pub fn standard_face_count(bytes: &[u8]) -> Option<usize> {
    largest_fbb_run(bytes).map(|(_, count, _)| count)
}

/// Unit frame vector for each positional standard trim packet. The result is
/// index-aligned with the FBB face population; packets without the optional
/// vector retain an empty slot.
#[must_use]
pub fn standard_face_frame_vectors(bytes: &[u8]) -> Vec<Option<[f64; 3]>> {
    let Some((face_start, face_count, _)) = largest_fbb_run(bytes) else {
        return Vec::new();
    };
    let solutions = [1, 2, 3]
        .into_iter()
        .filter_map(|width| parse_trim_chain(bytes, face_start, face_count, width))
        .collect::<Vec<_>>();
    let Ok([records]) = <[Vec<TrimRecord>; 1]>::try_from(solutions) else {
        return Vec::new();
    };
    records
        .into_iter()
        .map(|record| record.frame_vector)
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
    for width in [1, 2, 3] {
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
    for width in [1, 2, 3] {
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

/// Reconstruct standard topology while treating equal curve-class identifiers
/// as interchangeable serialized edge rows during incidence-slot completion.
#[must_use]
pub fn parse_standard_endpoints_with_edge_classes(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    edge_classes: Option<&[usize]>,
) -> Option<StandardTopology> {
    let (_, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, vertex_header) = parse_edge_tables(bytes, after_faces)?;
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
    if edge_ports.len() != edge_candidates.len() || edge_candidates.iter().any(Vec::is_empty) {
        return None;
    }
    let mut domains = Vec::with_capacity(edge_candidates.len() * 2);
    for candidates in edge_candidates {
        let domain = Arc::new(candidates.iter().flatten().copied().collect::<HashSet<_>>());
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
    if !quotient.edge_domains_viable(edge_candidates) {
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
        None,
        face_count,
    )
}

/// Reconstruct standard topology from geometric endpoint candidates while
/// enforcing the serialized endpoint-port equality quotient during search.
#[must_use]
pub fn parse_standard_port_endpoint_candidates(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_ports: &[[u32; 2]],
) -> Option<StandardTopology> {
    let (_, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, vertex_header) = parse_edge_tables(bytes, after_faces)?;
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
    )
}

pub(crate) fn parse_fbb_edge_tables(
    bytes: &[u8],
    position: usize,
) -> Option<(Vec<EdgeRow>, Vec<usize>, usize, usize)> {
    [3, 2, 1]
        .into_iter()
        .find_map(|handle_width| parse_fbb_edge_tables_width(bytes, position, handle_width))
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

pub(crate) fn largest_fbb_run(bytes: &[u8]) -> Option<(usize, usize, usize)> {
    let mut best = None;
    let mut tied = false;
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

pub(crate) fn parse_edge_tables(bytes: &[u8], position: usize) -> Option<(Vec<EdgeRow>, usize)> {
    if let Some(result) = parse_edge_tables_at(bytes, position) {
        return Some(result);
    }
    parse_fbb_edge_tables(bytes, position)
        .filter(|(_, _, vertex_header, _)| parse_vertex_table(bytes, *vertex_header).is_some())
        .map(|(rows, _, vertex_header, _)| (rows, vertex_header))
}

pub(crate) fn parse_edge_tables_at(bytes: &[u8], position: usize) -> Option<(Vec<EdgeRow>, usize)> {
    parse_edge_tables_scoped_at(bytes, position)
        .map(|(rows, _, vertex_header)| (rows, vertex_header))
}

pub(crate) fn parse_edge_tables_scoped_at(
    bytes: &[u8],
    mut position: usize,
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
            if arity > bytes.len().saturating_sub(position) / 2 {
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

pub(crate) fn parse_trim_chain(
    bytes: &[u8],
    end: usize,
    record_count: usize,
    width: usize,
) -> Option<Vec<TrimRecord>> {
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
    for start in 0..prefix.len() {
        if let Some(layout) = parse_trim_record_layout(prefix, start, width) {
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
            let mut records = reversed.clone();
            records.reverse();
            solutions.push(records);
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
        let Some(record) = parse_trim_record(prefix, start, width) else {
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
    <[Vec<TrimRecord>; 1]>::try_from(solutions)
        .ok()
        .map(|[records]| records)
}

pub(crate) struct TrimRecordLayout {
    kind: u8,
    independent_count: usize,
    strip_count: usize,
    lengths: Vec<usize>,
    frame_vector: Option<[f64; 3]>,
    pub(crate) handle_offset: usize,
    handle_count: usize,
    pub(crate) stored_count: usize,
    legacy_42: bool,
    pub(crate) end: usize,
}

pub(crate) fn parse_trim_record_layout(
    bytes: &[u8],
    start: usize,
    width: usize,
) -> Option<TrimRecordLayout> {
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
    let primitive_count = b.checked_add(c)?;
    if !legacy_42 && primitive_count > bytes.len().saturating_sub(position) {
        return None;
    }
    let mut lengths = Vec::with_capacity(primitive_count);
    if !legacy_42 {
        for _ in 0..primitive_count {
            lengths.push(parse_count(bytes, &mut position)?);
        }
        if 3usize.checked_mul(a)?.checked_add(lengths.iter().sum())? != handle_count {
            return None;
        }
    }
    let stored_count = handle_count + usize::from(legacy_42);
    let handle_offset = position;
    let byte_count = stored_count.checked_mul(width)?;
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
        legacy_42,
        end,
    })
}

pub(crate) fn parse_trim_record(bytes: &[u8], start: usize, width: usize) -> Option<TrimRecord> {
    let layout = parse_trim_record_layout(bytes, start, width)?;
    let mut position = layout.handle_offset;
    let mut handles = Vec::with_capacity(layout.stored_count);
    for _ in 0..layout.stored_count {
        let handle = match width {
            1 => u32::from(*bytes.get(position)?),
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
    let mut lengths = layout.lengths;
    if layout.legacy_42 {
        let packed = *handles.first()?;
        lengths = vec![(packed >> 8) as usize, (packed & 0xff) as usize];
        handles.remove(0);
        if lengths.iter().sum::<usize>() != layout.handle_count {
            return None;
        }
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
