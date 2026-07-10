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
/// coordinate table (spec ?5).
#[derive(Debug, Clone, PartialEq)]
pub struct StandardTopology {
    faces: Vec<FaceTopology>,
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<[f64; 3]>,
    logical_vertex_count: usize,
}

impl StandardTopology {
    /// Number of faces, equal to the largest contiguous `30 04 04 ff` FBB
    /// run's row count (spec ?5.2).
    #[must_use]
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// Per-face reconstructed boundaries, in FBB row order (spec ?5.1: face
    /// ordinal `i` binds to FBB row `i`).
    #[must_use]
    pub fn faces(&self) -> &[FaceTopology] {
        &self.faces
    }

    /// The counted spine's physical edge rows, in table order (spec ?5.2).
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
        if solutions.len() > 1 {
            return;
        }
    }
}

/// One row of the counted standard/FBB edge table: `02 <arity_u8>
/// <payload[arity*2]>` (spec ?5.2), handles read big-endian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeRow {
    /// Table-kind byte the row was parsed under (`0x01` or `0x02`; spec
    /// ?5.2 `count_header`).
    pub kind: u8,
    /// The row's BE handle sequence `[p0, interior?, p1]`; the first and
    /// last entries are the row's graph endpoint ports (spec ?5.4).
    pub handles: Vec<u32>,
}

/// One face's reconstructed boundary cycles (spec ?5.3): one outer cycle
/// plus one per hole, in the order recovered from the trim mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceTopology {
    /// The face's boundary cycles; loop count equals boundary-cycle count.
    pub boundaries: Vec<Boundary>,
}

/// One closed boundary cycle of a face's trim mesh, covered end-to-end by
/// matched edge rows (spec ?5.3-?5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boundary {
    /// The mesh-boundary handle cycle recovered from triangle-edge
    /// incidence cancellation, rotated to start at its minimum handle.
    pub mesh_handles: Vec<u32>,
    /// The physical edge uses covering this cycle, in cycle order.
    pub coedges: Vec<CoedgeUse>,
}

/// One physical edge's use within a face boundary, oriented by its match
/// against the recovered boundary cycle (spec ?5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoedgeUse {
    /// Index into [`StandardTopology::edge_rows`] for the matched edge
    /// row.
    pub edge_row: usize,
    /// `true` when the edge row's handle sequence matched the boundary
    /// cycle in reverse; orientation comes from this match, not a stored
    /// sense bit (spec ?5.4).
    pub reversed: bool,
    /// Logical-vertex (union-find component) index at this coedge's start,
    /// in boundary-cycle traversal direction.
    pub start_vertex: usize,
    /// Logical-vertex (union-find component) index at this coedge's end,
    /// in boundary-cycle traversal direction.
    pub end_vertex: usize,
}

#[derive(Debug)]
struct TrimRecord {
    triangles: Vec<[u32; 3]>,
    end: usize,
}

/// Parses the counted standard spine, positional trim packets, mesh boundary
/// cycles, physical edge uses, and port/corner vertex equivalence classes.
/// Returns `None` unless every positional face boundary is unambiguous.
#[must_use]
pub fn parse_standard(bytes: &[u8]) -> Option<StandardTopology> {
    let (face_start, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, vertex_header) = parse_edge_tables(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;

    let trims = parse_trim_records(&bytes[..face_start]);
    if trims.len() < face_count {
        return None;
    }
    let trims = &trims[trims.len() - face_count..];

    reconstruct(edge_rows, vertex_points, trims)
}

/// Parses the FBB-only spine. Its edge rows and trim handles are `u24be`; its
/// coordinate records are not part of the counted spine and remain unbound.
#[must_use]
pub fn parse_fbb(bytes: &[u8]) -> Option<StandardTopology> {
    let (face_start, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, _) = parse_fbb_edge_tables(bytes, after_faces)?;
    let trims = parse_trim_records_with_width(&bytes[..face_start], 3);
    if trims.len() < face_count {
        return None;
    }
    reconstruct(edge_rows, Vec::new(), &trims[trims.len() - face_count..])
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
            rows.push(EdgeRow { kind, handles });
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

fn parse_edge_tables(bytes: &[u8], mut position: usize) -> Option<(Vec<EdgeRow>, usize)> {
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
            rows.push(EdgeRow { kind, handles });
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
    if mask & 8 != 0 {
        position = position.checked_add(12)?;
        bytes.get(..position)?;
    }

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
    cover_cycle_by_interiors(cycle, rows, union)
}

fn cover_cycle_by_interiors(
    cycle: &[u32],
    rows: &[EdgeRow],
    union: &mut UnionFind,
) -> Option<Boundary> {
    let length = cycle.len();
    let mut matches = Vec::new();
    for (edge_row, row) in rows.iter().enumerate() {
        let interior = row.handles.get(1..row.handles.len() - 1)?;
        if interior.is_empty() {
            continue;
        }
        let mut row_matches = Vec::new();
        for start in 0..length {
            let forward = interior
                .iter()
                .enumerate()
                .all(|(offset, handle)| cycle[(start + offset) % length] == *handle);
            let reversed = interior
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
            matches.push((start + length - 1, interior.len() + 1, edge_row, reversed));
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
    Some(Boundary {
        mesh_handles: cycle.to_vec(),
        coedges,
    })
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
