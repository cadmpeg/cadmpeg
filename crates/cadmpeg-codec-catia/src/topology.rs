// SPDX-License-Identifier: Apache-2.0
//! Byte-level topology for standard nested CATIA V5 B-rep streams.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use cadmpeg_ir::topology::BodyKind;

const FBB_ROW: [u8; 4] = [0x30, 0x04, 0x04, 0xff];
const EDGE_DELIMITER: [u8; 8] = [0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
const MAX_FACE_EQUATION_CACHE_ENTRIES: usize = 4_096;
const TRIM_KINDS: [u8; 14] = [
    0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f,
];

fn domains_have_distinct_matching<'a>(
    domains: impl IntoIterator<Item = &'a [usize]>,
    point_count: usize,
) -> bool {
    let domains = domains.into_iter().collect::<Vec<_>>();
    if domains.len() > point_count {
        return false;
    }
    let mut owner = vec![None; point_count];
    let mut matched = vec![false; domains.len()];
    let mut matched_count = 0usize;
    while matched_count < domains.len() {
        let mut distance = vec![usize::MAX; domains.len()];
        let mut queue = VecDeque::new();
        for root in 0..domains.len() {
            if !matched[root] {
                distance[root] = 0;
                queue.push_back(root);
            }
        }
        let mut shortest = usize::MAX;
        while let Some(root) = queue.pop_front() {
            if distance[root] >= shortest {
                continue;
            }
            for &point in domains[root] {
                if point >= point_count {
                    continue;
                }
                if let Some(next) = owner[point] {
                    if distance[next] == usize::MAX {
                        distance[next] = distance[root] + 1;
                        queue.push_back(next);
                    }
                } else {
                    shortest = distance[root];
                }
            }
        }
        if shortest == usize::MAX {
            return false;
        }
        let mut cursor = vec![0usize; domains.len()];
        let mut incoming = vec![None; domains.len()];
        let mut augmented = 0usize;
        for start in 0..domains.len() {
            if matched[start] || distance[start] != 0 {
                continue;
            }
            let mut roots = vec![start];
            let mut free_point = None;
            while let Some(&root) = roots.last() {
                let mut advanced = false;
                while cursor[root] < domains[root].len() {
                    let point = domains[root][cursor[root]];
                    cursor[root] += 1;
                    if point >= point_count {
                        continue;
                    }
                    match owner[point] {
                        None if distance[root] == shortest => {
                            free_point = Some(point);
                            advanced = true;
                            break;
                        }
                        Some(next) if distance[next] == distance[root] + 1 => {
                            incoming[next] = Some(point);
                            roots.push(next);
                            advanced = true;
                            break;
                        }
                        _ => {}
                    }
                }
                if free_point.is_some() {
                    break;
                }
                if !advanced {
                    distance[root] = usize::MAX;
                    roots.pop();
                }
            }
            let Some(mut point) = free_point else {
                continue;
            };
            for (index, &root) in roots.iter().enumerate().rev() {
                owner[point] = Some(root);
                if index != 0 {
                    let Some(previous) = incoming[root] else {
                        return false;
                    };
                    point = previous;
                }
            }
            matched[start] = true;
            matched_count += 1;
            augmented += 1;
        }
        if augmented == 0 {
            return false;
        }
    }
    true
}

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

fn component_root(parents: &mut [usize], index: usize) -> usize {
    if parents[index] != index {
        parents[index] = component_root(parents, parents[index]);
    }
    parents[index]
}

fn union_components(parents: &mut [usize], left: usize, right: usize) {
    let left = component_root(parents, left);
    let right = component_root(parents, right);
    parents[left] = right;
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

    /// Face-index components connected through shared physical edge rows, in
    /// first-face order.
    #[must_use]
    pub fn face_components(&self) -> Vec<Vec<usize>> {
        let mut parents: Vec<usize> = (0..self.faces.len()).collect();
        let mut first_face_by_edge = HashMap::<usize, usize>::new();
        for (face, topology) in self.faces.iter().enumerate() {
            for edge in topology
                .boundaries
                .iter()
                .flat_map(|boundary| &boundary.coedges)
                .map(|coedge| coedge.edge_row)
            {
                if let Some(other) = first_face_by_edge.insert(edge, face) {
                    union_components(&mut parents, face, other);
                }
            }
        }
        let mut labels = HashMap::<usize, usize>::new();
        let mut components = Vec::<Vec<usize>>::new();
        for face in 0..self.faces.len() {
            let root = component_root(&mut parents, face);
            let next = labels.len();
            let component = *labels.entry(root).or_insert(next);
            if component == components.len() {
                components.push(Vec::new());
            }
            components[component].push(face);
        }
        components
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

    /// Classify each consecutive FBB face group from physical-edge incidence.
    /// An edge cannot belong to faces in two different groups.
    #[must_use]
    pub fn body_kinds(&self, face_groups: &[usize]) -> Option<Vec<BodyKind>> {
        if face_groups.iter().sum::<usize>() != self.faces.len() {
            return None;
        }
        let mut body_by_face = Vec::with_capacity(self.faces.len());
        for (body, count) in face_groups.iter().copied().enumerate() {
            body_by_face.extend(std::iter::repeat_n(body, count));
        }
        let mut uses = vec![HashMap::<usize, usize>::new(); face_groups.len()];
        let mut bodies_by_edge = vec![HashSet::new(); self.edge_rows.len()];
        for (face, topology) in self.faces.iter().enumerate() {
            let body = body_by_face[face];
            for coedge in topology
                .boundaries
                .iter()
                .flat_map(|boundary| &boundary.coedges)
            {
                if coedge.edge_row >= self.edge_rows.len() {
                    return None;
                }
                bodies_by_edge[coedge.edge_row].insert(body);
                *uses[body].entry(coedge.edge_row).or_default() += 1;
            }
        }
        if bodies_by_edge.iter().any(|bodies| bodies.len() != 1) {
            return None;
        }
        Some(
            uses.into_iter()
                .map(|uses| {
                    if uses.values().any(|count| *count > 2) {
                        BodyKind::General
                    } else if !uses.is_empty() && uses.values().all(|count| *count == 2) {
                        BodyKind::Solid
                    } else {
                        BodyKind::Sheet
                    }
                })
                .collect(),
        )
    }

    /// Orient every incidence-closed FBB face group independently. Open sheet
    /// and non-manifold general groups retain their reconstructed loop senses.
    pub fn orient_solid_body_cycles(&mut self, face_groups: &[usize]) -> Option<()> {
        let kinds = self.body_kinds(face_groups)?;
        let mut start = 0usize;
        for (&count, kind) in face_groups.iter().zip(kinds) {
            let end = start.checked_add(count)?;
            if kind == BodyKind::Solid {
                orient_face_cycles(self.faces.get_mut(start..end)?)?;
            }
            start = end;
        }
        (start == self.faces.len()).then_some(())
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

        unique_coordinate_bijection(&domains, &self.vertex_points)
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

/// Number of face rows in the governing standard topology spine. The spine is
/// the unique largest contiguous stride-eight FBB run; shorter marker runs are
/// not members of this face population. Equal-largest runs leave ownership
/// unresolved.
#[must_use]
pub fn standard_face_count(bytes: &[u8]) -> Option<usize> {
    largest_fbb_run(bytes).map(|(_, count, _)| count)
}

fn unique_coordinate_bijection(
    domains: &[HashSet<usize>],
    points: &[[f64; 3]],
) -> Option<Vec<usize>> {
    fn matching(
        domains: &[Vec<usize>],
        slots_by_class: &[Vec<usize>],
        slot_classes: &[usize],
        forced: Option<(usize, usize)>,
    ) -> Option<Vec<usize>> {
        let mut owner = vec![None; slot_classes.len()];
        let mut order = (0..domains.len()).collect::<Vec<_>>();
        order.sort_unstable_by_key(|vertex| {
            let count = forced
                .filter(|(forced_vertex, _)| forced_vertex == vertex)
                .map_or_else(
                    || {
                        domains[*vertex]
                            .iter()
                            .map(|class| slots_by_class[*class].len())
                            .sum()
                    },
                    |(_, class)| slots_by_class[class].len(),
                );
            (count, *vertex)
        });
        let mut seen_vertices = vec![0usize; domains.len()];
        let mut seen_slots = vec![0usize; slot_classes.len()];
        let mut incoming_slot = vec![None; domains.len()];
        let mut via_vertex = vec![None; slot_classes.len()];
        for (generation, start) in order.into_iter().enumerate() {
            let generation = generation + 1;
            let mut queue = VecDeque::from([start]);
            seen_vertices[start] = generation;
            incoming_slot[start] = None;
            let mut free_slot = None;
            while let Some(vertex) = queue.pop_front() {
                let slots = match forced.filter(|(forced_vertex, _)| *forced_vertex == vertex) {
                    Some((_, class)) => slots_by_class[class].clone(),
                    None => domains[vertex]
                        .iter()
                        .flat_map(|class| slots_by_class[*class].iter().copied())
                        .collect(),
                };
                for slot in slots {
                    if seen_slots[slot] == generation {
                        continue;
                    }
                    seen_slots[slot] = generation;
                    via_vertex[slot] = Some(vertex);
                    let Some(next) = owner[slot] else {
                        free_slot = Some(slot);
                        break;
                    };
                    if seen_vertices[next] != generation {
                        seen_vertices[next] = generation;
                        incoming_slot[next] = Some(slot);
                        queue.push_back(next);
                    }
                }
                if free_slot.is_some() {
                    break;
                }
            }
            let mut slot = free_slot?;
            loop {
                let vertex = via_vertex[slot]?;
                owner[slot] = Some(vertex);
                let Some(previous) = incoming_slot[vertex] else {
                    break;
                };
                slot = previous;
            }
        }
        let mut assignment = vec![None; domains.len()];
        for (slot, vertex) in owner.into_iter().enumerate() {
            assignment[vertex?] = Some(slot_classes[slot]);
        }
        assignment.into_iter().collect()
    }

    if domains.len() != points.len()
        || domains
            .iter()
            .any(|domain| domain.is_empty() || domain.iter().any(|point| *point >= points.len()))
    {
        return None;
    }
    let mut representatives = Vec::<usize>::new();
    let mut point_classes = Vec::with_capacity(points.len());
    for (point, position) in points.iter().enumerate() {
        let class = representatives
            .iter()
            .position(|representative| points[*representative] == *position)
            .unwrap_or_else(|| {
                representatives.push(point);
                representatives.len() - 1
            });
        point_classes.push(class);
    }
    let class_domains = domains
        .iter()
        .map(|domain| {
            let mut classes = domain
                .iter()
                .map(|point| point_classes[*point])
                .collect::<Vec<_>>();
            classes.sort_unstable();
            classes.dedup();
            classes
        })
        .collect::<Vec<_>>();
    let mut capacities = vec![0usize; representatives.len()];
    for class in &point_classes {
        capacities[*class] += 1;
    }
    let mut slot_classes = Vec::with_capacity(points.len());
    let mut slots_by_class = vec![Vec::new(); capacities.len()];
    for (class, capacity) in capacities.into_iter().enumerate() {
        for _ in 0..capacity {
            let slot = slot_classes.len();
            slot_classes.push(class);
            slots_by_class[class].push(slot);
        }
    }
    let classes = matching(&class_domains, &slots_by_class, &slot_classes, None)?;
    for (vertex, domain) in class_domains.iter().enumerate() {
        for &class in domain {
            if class != classes[vertex]
                && matching(
                    &class_domains,
                    &slots_by_class,
                    &slot_classes,
                    Some((vertex, class)),
                )
                .is_some()
            {
                return None;
            }
        }
    }
    let mut available = vec![Vec::new(); representatives.len()];
    for (point, class) in point_classes.into_iter().enumerate() {
        available[class].push(point);
    }
    let mut used = vec![0usize; available.len()];
    Some(
        classes
            .iter()
            .map(|class| {
                let point = available[*class][used[*class]];
                used[*class] += 1;
                point
            })
            .collect(),
    )
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
    independent_count: usize,
    strip_lengths: Vec<usize>,
    fan_lengths: Vec<usize>,
    kind: u8,
    end: usize,
}

/// Primitive partition of one selected positional standard trim packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrimPacketLayout {
    /// Packet kind byte (`0x40 | primitive-mask`).
    pub kind: u8,
    /// Number of independent triangle triples at the start of the handle lane.
    pub independent_triangles: usize,
    /// Ordered handle counts of the packet's triangle strips.
    pub strip_lengths: Vec<usize>,
    /// Ordered handle counts of the packet's triangle fans.
    pub fan_lengths: Vec<usize>,
    /// Total number of handles in the packet lane.
    pub handle_count: usize,
}

/// Return the primitive partitions of the unique width-selected standard trim
/// chain, in positional face order. An absent or width-ambiguous chain returns
/// an empty vector.
#[must_use]
pub fn standard_trim_packet_layouts(bytes: &[u8]) -> Vec<TrimPacketLayout> {
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
        .map(|record| TrimPacketLayout {
            kind: record.kind,
            independent_triangles: record.independent_count,
            strip_lengths: record.strip_lengths,
            fan_lengths: record.fan_lengths,
            handle_count: record.handles.len(),
        })
        .collect()
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

/// Reconstruct standard topology from byte-derived endpoint coordinate rows.
/// The endpoint graph is accepted only when all face incidences close and the
/// radial orientation constraints are consistent.
#[must_use]
pub fn parse_standard_endpoints(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
) -> Option<StandardTopology> {
    parse_standard_endpoints_with_edge_classes(bytes, edge_faces, edge_points, None)
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

fn prune_incidence_choices(
    choices: &mut [Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
) -> Option<()> {
    fn unique_faces(faces: [usize; 2]) -> impl Iterator<Item = usize> {
        faces
            .into_iter()
            .enumerate()
            .filter_map(move |(rank, face)| (rank == 0 || face != faces[0]).then_some(face))
    }

    fn fits(degrees: &[Vec<u8>], edge_faces: &[[usize; 2]], edge: usize, pair: [usize; 2]) -> bool {
        unique_faces(edge_faces[edge]).all(|face| {
            pair.iter().enumerate().all(|(rank, &point)| {
                let multiplicity = 1 + usize::from(rank == 0 && pair[0] == pair[1]);
                usize::from(degrees[face][point]) + multiplicity <= 2
            })
        })
    }

    if choices.len() != edge_faces.len()
        || choices.iter().any(Vec::is_empty)
        || edge_faces.iter().flatten().any(|face| *face >= face_count)
        || choices
            .iter()
            .flatten()
            .flatten()
            .any(|point| *point >= point_count)
    {
        return None;
    }
    let mut face_edges = vec![Vec::new(); face_count];
    for (edge, faces) in edge_faces.iter().copied().enumerate() {
        for face in unique_faces(faces) {
            face_edges[face].push(edge);
        }
    }
    let mut fixed = vec![false; choices.len()];
    let mut degrees = vec![vec![0u8; point_count]; face_count];
    loop {
        let mut changed = false;
        for edge in 0..choices.len() {
            if fixed[edge] {
                continue;
            }
            let before = choices[edge].len();
            choices[edge].retain(|pair| fits(&degrees, edge_faces, edge, *pair));
            changed |= choices[edge].len() != before;
            let [pair] = choices[edge].as_slice() else {
                if choices[edge].is_empty() {
                    return None;
                }
                continue;
            };
            for face in unique_faces(edge_faces[edge]) {
                for point in pair {
                    degrees[face][*point] = degrees[face][*point].checked_add(1)?;
                }
            }
            fixed[edge] = true;
            changed = true;
        }
        for face in 0..face_count {
            for (point, &degree) in degrees[face].iter().enumerate() {
                if degree != 1 {
                    continue;
                }
                let supporting_edges = face_edges[face]
                    .iter()
                    .copied()
                    .filter(|&edge| {
                        !fixed[edge] && choices[edge].iter().any(|pair| pair.contains(&point))
                    })
                    .collect::<Vec<_>>();
                let (&edge, rest) = supporting_edges.split_first()?;
                if rest.iter().all(|candidate| *candidate == edge) {
                    let before = choices[edge].len();
                    choices[edge].retain(|pair| pair.contains(&point));
                    if choices[edge].is_empty() {
                        return None;
                    }
                    changed |= choices[edge].len() != before;
                }
            }
        }
        if !changed {
            return Some(());
        }
    }
}

fn incidence_choice_components(
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
) -> Vec<Vec<usize>> {
    let mut union = UnionFind::new(choices.len());
    let mut owner = HashMap::<(usize, usize), usize>::new();
    let ambiguous = choices
        .iter()
        .enumerate()
        .filter_map(|(edge, pairs)| (pairs.len() > 1).then_some(edge))
        .collect::<Vec<_>>();
    for &edge in &ambiguous {
        let faces = edge_faces[edge];
        for (rank, face) in faces.into_iter().enumerate() {
            if rank > 0 && face == faces[0] {
                continue;
            }
            for point in choices[edge].iter().flatten().copied() {
                match owner.entry((face, point)) {
                    std::collections::hash_map::Entry::Occupied(entry) => {
                        union.union(*entry.get(), edge);
                    }
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(edge);
                    }
                }
            }
        }
    }
    let mut by_root = HashMap::<usize, Vec<usize>>::new();
    for edge in ambiguous {
        by_root.entry(union.find(edge)).or_default().push(edge);
    }
    let mut components = by_root.into_values().collect::<Vec<_>>();
    for component in &mut components {
        component.sort_unstable();
    }
    components.sort_by_key(|component| component[0]);
    components
}

struct IncidenceComponentSearch<'a> {
    choices: &'a [Vec<[usize; 2]>],
    edge_faces: &'a [[usize; 2]],
    face_edges: &'a [Vec<usize>],
    mesh_assignments: Option<&'a [Vec<MeshFaceBoundaryAssignment>]>,
    active: Vec<bool>,
    edges: &'a [usize],
    constraints: Vec<(usize, usize)>,
    assignment: Vec<Option<[usize; 2]>>,
    degrees: Vec<Vec<u8>>,
    solutions: Vec<Vec<(usize, [usize; 2])>>,
    solution_filter: Option<MeshEndpointSolutionFilter<'a>>,
    dead_states: HashSet<Vec<Option<[usize; 2]>>>,
    states: usize,
    exhausted: bool,
}

impl IncidenceComponentSearch<'_> {
    fn charge_branch(&mut self, option_count: usize) -> bool {
        const MAX_STATES: usize = 4_096;

        if option_count <= 1 {
            return true;
        }
        if self.states >= MAX_STATES {
            self.exhausted = true;
            return false;
        }
        self.states += 1;
        true
    }

    fn degree_candidate_fits(&self, edge: usize, pair: [usize; 2]) -> bool {
        let faces = self.edge_faces[edge];
        faces.into_iter().enumerate().all(|(rank, face)| {
            (rank > 0 && face == faces[0])
                || pair.iter().enumerate().all(|(point_rank, &point)| {
                    let multiplicity = 1 + usize::from(point_rank == 0 && pair[0] == pair[1]);
                    usize::from(self.degrees[face][point]) + multiplicity <= 2
                })
        })
    }

    fn candidate_fits(&self, edge: usize, pair: [usize; 2]) -> bool {
        if !self.degree_candidate_fits(edge, pair) {
            return false;
        }
        let Some(mesh_assignments) = self.mesh_assignments else {
            return true;
        };
        let mut faces = self.edge_faces[edge].to_vec();
        faces.sort_unstable();
        faces.dedup();
        faces.into_iter().all(|face| {
            mesh_assignments.get(face).is_some_and(|assignments| {
                assignments.iter().any(|assignment| {
                    mesh_assignment_endpoint_cycles_viable_where(
                        assignment,
                        self.choices,
                        |candidate_edge, candidate_pair| {
                            let selected = if candidate_edge == edge {
                                Some(pair)
                            } else {
                                self.assignment[candidate_edge]
                            };
                            selected.is_none_or(|selected| {
                                same_unordered_pair(selected, candidate_pair)
                            })
                        },
                    )
                    .unwrap_or(true)
                })
            })
        })
    }

    fn constraint_options(&self, face: usize, point: usize) -> Vec<(usize, [usize; 2])> {
        let mut options = self.face_edges[face]
            .iter()
            .copied()
            .filter(|&edge| self.active[edge] && self.assignment[edge].is_none())
            .flat_map(|edge| {
                self.choices[edge]
                    .iter()
                    .copied()
                    .filter(move |pair| pair.contains(&point))
                    .map(move |pair| (edge, pair))
            })
            .filter(|(edge, pair)| self.candidate_fits(*edge, *pair))
            .collect::<Vec<_>>();
        options.sort_unstable();
        options.dedup();
        options
    }

    fn branch_options(&self) -> Option<Vec<(usize, [usize; 2])>> {
        for &edge in self.edges {
            if self.assignment[edge].is_some() {
                continue;
            }
            let mut viable = self.choices[edge]
                .iter()
                .copied()
                .filter(|pair| self.candidate_fits(edge, *pair));
            let pair = viable.next()?;
            if viable.next().is_none() {
                return Some(vec![(edge, pair)]);
            }
        }
        let mut constrained = None::<Vec<(usize, [usize; 2])>>;
        for &(face, point) in &self.constraints {
            if self.degrees[face][point] != 1 {
                continue;
            }
            let options = self.constraint_options(face, point);
            if options.is_empty() {
                return None;
            }
            if constrained
                .as_ref()
                .is_none_or(|stored| options.len() < stored.len())
            {
                constrained = Some(options);
            }
        }
        if constrained.is_some() {
            return constrained;
        }
        let edge = self
            .edges
            .iter()
            .copied()
            .filter(|&edge| self.assignment[edge].is_none())
            .min_by_key(|&edge| {
                self.choices[edge]
                    .iter()
                    .filter(|pair| self.candidate_fits(edge, **pair))
                    .count()
            });
        Some(edge.map_or_else(Vec::new, |edge| {
            self.choices[edge]
                .iter()
                .copied()
                .filter(|pair| self.candidate_fits(edge, *pair))
                .map(|pair| (edge, pair))
                .collect()
        }))
    }

    fn adjust(&mut self, edge: usize, pair: [usize; 2], increase: bool) {
        let faces = self.edge_faces[edge];
        for (rank, face) in faces.into_iter().enumerate() {
            if rank > 0 && face == faces[0] {
                continue;
            }
            for point in pair {
                if increase {
                    self.degrees[face][point] += 1;
                } else {
                    self.degrees[face][point] -= 1;
                }
            }
        }
    }

    fn ordered_faces_feasible(&self, faces: impl IntoIterator<Item = usize>) -> bool {
        let Some(mesh_assignments) = self.mesh_assignments else {
            return true;
        };
        faces.into_iter().all(|face| {
            mesh_assignments.get(face).is_some_and(|assignments| {
                assignments.iter().any(|assignment| {
                    mesh_assignment_endpoint_cycles_viable_where(
                        assignment,
                        self.choices,
                        |edge, pair| {
                            self.assignment[edge]
                                .is_none_or(|selected| same_unordered_pair(selected, pair))
                        },
                    )
                    .unwrap_or(true)
                })
            })
        })
    }

    fn face_configuration_options(&self) -> Option<MeshFaceEndpointConfigurations> {
        const MAX_FACE_CONFIGURATIONS: usize = 4_096;

        let mesh_assignments = self.mesh_assignments?;
        let mut best = None::<Vec<Vec<(usize, [usize; 2])>>>;
        for (face, assignments) in mesh_assignments.iter().enumerate() {
            if !self.face_edges[face]
                .iter()
                .any(|edge| self.active[*edge] && self.assignment[*edge].is_none())
            {
                continue;
            }
            let Some(configurations) = mesh_face_endpoint_configurations(
                assignments,
                self.choices,
                &self.assignment,
                MAX_FACE_CONFIGURATIONS,
            ) else {
                continue;
            };
            let mut projected = configurations
                .into_iter()
                .map(|configuration| {
                    configuration
                        .into_iter()
                        .filter(|(edge, _)| self.active[*edge] && self.assignment[*edge].is_none())
                        .collect::<Vec<_>>()
                })
                .collect::<HashSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            projected.sort_unstable();
            if projected.is_empty() {
                return Some(Vec::new());
            }
            if projected.iter().all(Vec::is_empty) {
                continue;
            }
            if best
                .as_ref()
                .is_none_or(|stored| projected.len() < stored.len())
            {
                best = Some(projected);
            }
        }
        best
    }

    fn search_face_configurations(&mut self, options: MeshFaceEndpointConfigurations) {
        for option in options {
            let mut assigned = Vec::new();
            let mut viable = true;
            for (edge, pair) in option {
                if !self.active[edge] || self.assignment[edge].is_some() {
                    continue;
                }
                if !self.candidate_fits(edge, pair) {
                    viable = false;
                    break;
                }
                self.adjust(edge, pair, true);
                self.assignment[edge] = Some(pair);
                assigned.push((edge, pair));
            }
            if viable && !assigned.is_empty() {
                self.search();
            }
            for (edge, pair) in assigned.into_iter().rev() {
                self.assignment[edge] = None;
                self.adjust(edge, pair, false);
            }
            if self.exhausted {
                return;
            }
        }
    }

    fn search(&mut self) {
        if self.exhausted {
            return;
        }
        let state = self
            .edges
            .iter()
            .map(|&edge| self.assignment[edge])
            .collect::<Vec<_>>();
        if self.dead_states.contains(&state) {
            return;
        }
        let solutions_before = self.solutions.len();
        self.search_state();
        if !self.exhausted && self.solutions.len() == solutions_before {
            self.dead_states.insert(state);
        }
    }

    fn search_state(&mut self) {
        const MAX_SOLUTIONS: usize = 256;
        if self.exhausted {
            return;
        }
        if self.solutions.len() >= MAX_SOLUTIONS {
            self.exhausted = true;
            return;
        }
        if let Some(options) = self.face_configuration_options() {
            if !options.is_empty() && self.charge_branch(options.len()) {
                self.search_face_configurations(options);
            }
            return;
        }
        let Some(options) = self.branch_options() else {
            return;
        };
        if options.is_empty() {
            if self
                .edges
                .iter()
                .any(|&edge| self.assignment[edge].is_none())
                || self
                    .constraints
                    .iter()
                    .any(|&(face, point)| self.degrees[face][point] == 1)
            {
                return;
            }
            let solution = self
                .edges
                .iter()
                .map(|&edge| Some((edge, self.assignment[edge]?)))
                .collect::<Option<Vec<_>>>()
                .expect("every component edge is assigned");
            if self.solution_filter.is_none_or(|filter| filter(&solution)) {
                self.solutions.push(solution);
            }
            return;
        }
        if !self.charge_branch(options.len()) {
            return;
        }
        for (edge, pair) in options {
            if self.assignment[edge].is_some() || !self.candidate_fits(edge, pair) {
                continue;
            }
            self.adjust(edge, pair, true);
            self.assignment[edge] = Some(pair);
            let mut faces = self.edge_faces[edge].to_vec();
            faces.sort_unstable();
            faces.dedup();
            if self.ordered_faces_feasible(faces) {
                self.search();
            }
            self.assignment[edge] = None;
            self.adjust(edge, pair, false);
        }
    }
}

fn component_incidence_pair_solutions<F>(
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&[Vec<MeshFaceBoundaryAssignment>]>,
    solution_valid: &F,
) -> Option<Vec<Vec<[usize; 2]>>>
where
    F: Fn(&[[usize; 2]]) -> bool,
{
    const MAX_PAIR_SOLUTIONS: usize = 256;
    let components = incidence_choice_components(choices, edge_faces);
    let mut face_edges = vec![Vec::new(); face_count];
    for (edge, faces) in edge_faces.iter().copied().enumerate() {
        for (rank, face) in faces.into_iter().enumerate() {
            if (rank == 0 || face != faces[0]) && !face_edges[face].contains(&edge) {
                face_edges[face].push(edge);
            }
        }
    }
    let mut fixed = vec![None; choices.len()];
    let mut degrees = vec![vec![0u8; point_count]; face_count];
    for (edge, pairs) in choices.iter().enumerate() {
        let [pair] = pairs.as_slice() else {
            continue;
        };
        fixed[edge] = Some(*pair);
        let faces = edge_faces[edge];
        for (rank, face) in faces.into_iter().enumerate() {
            if rank > 0 && face == faces[0] {
                continue;
            }
            for point in pair {
                degrees[face][*point] = degrees[face][*point].checked_add(1)?;
            }
        }
    }
    if components.is_empty() {
        let pairs = fixed.into_iter().collect::<Option<Vec<_>>>()?;
        return solution_valid(&pairs).then_some(vec![pairs]);
    }
    let component_count = components.len();
    let mut combined = vec![fixed.clone()];
    for (component_index, component) in components.into_iter().enumerate() {
        let mut active = vec![false; choices.len()];
        let mut constraints = HashSet::<(usize, usize)>::new();
        for &edge in &component {
            active[edge] = true;
            let faces = edge_faces[edge];
            for (rank, face) in faces.into_iter().enumerate() {
                if rank > 0 && face == faces[0] {
                    continue;
                }
                for point in choices[edge].iter().flatten() {
                    constraints.insert((face, *point));
                }
            }
        }
        let mut constraints = constraints.into_iter().collect::<Vec<_>>();
        constraints.sort_unstable();
        let filter = |solution: &[MeshEndpointPair]| {
            combined.iter().any(|prefix| {
                let mut assignment = prefix.clone();
                for &(edge, pair) in solution {
                    assignment[edge] = Some(pair);
                }
                assignment
                    .into_iter()
                    .collect::<Option<Vec<_>>>()
                    .is_some_and(|pairs| solution_valid(&pairs))
            })
        };
        let solution_filter = (component_index + 1 == component_count)
            .then_some(&filter as &dyn Fn(&[MeshEndpointPair]) -> bool);
        let (exhausted, solutions) = {
            let mut search = IncidenceComponentSearch {
                choices,
                edge_faces,
                face_edges: &face_edges,
                mesh_assignments,
                active,
                edges: &component,
                constraints,
                assignment: fixed.clone(),
                degrees: degrees.clone(),
                solutions: Vec::new(),
                solution_filter,
                dead_states: HashSet::new(),
                states: 0,
                exhausted: false,
            };
            search.search();
            (search.exhausted, search.solutions)
        };
        if exhausted || solutions.is_empty() {
            return None;
        }
        let result_count = combined.len().checked_mul(solutions.len())?;
        if result_count > MAX_PAIR_SOLUTIONS {
            return None;
        }
        combined = combined
            .into_iter()
            .flat_map(|assignment| {
                solutions.iter().map(move |solution| {
                    let mut assignment = assignment.clone();
                    for &(edge, pair) in solution {
                        assignment[edge] = Some(pair);
                    }
                    assignment
                })
            })
            .collect();
    }
    combined
        .into_iter()
        .map(|assignment| assignment.into_iter().collect::<Option<Vec<_>>>())
        .collect()
}

fn reconstruct_incidence_candidates(
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_ports: Option<&[[u32; 2]]>,
    face_count: usize,
) -> Option<StandardTopology> {
    if edge_ports.is_some_and(|ports| ports.len() != edge_candidates.len()) {
        return None;
    }
    let port_compatible = |pairs: &[[usize; 2]]| {
        edge_ports.is_none_or(|ports| {
            let singleton = pairs
                .iter()
                .copied()
                .map(|pair| vec![pair])
                .collect::<Vec<_>>();
            bind_edge_port_candidates(ports, &singleton).is_some()
        })
    };
    let pair_solutions = incidence_endpoint_pair_solutions(
        edge_rows,
        vertex_points,
        edge_faces,
        edge_candidates,
        face_count,
        None,
        &port_compatible,
    )?;
    let mut solution = None;
    for pairs in pair_solutions {
        let pairs = match edge_ports {
            Some(ports) => {
                let singleton = pairs.into_iter().map(|pair| vec![pair]).collect::<Vec<_>>();
                bind_edge_port_candidates(ports, &singleton)?
            }
            None => pairs,
        };
        let candidate = reconstruct_incidence(
            edge_rows.to_vec(),
            vertex_points.to_vec(),
            edge_faces,
            &pairs,
            face_count,
        )?;
        match &solution {
            Some(stored) if *stored != candidate => return None,
            None => solution = Some(candidate),
            Some(_) => {}
        }
    }
    solution
}

fn incidence_endpoint_pair_solutions<F>(
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    face_count: usize,
    mesh_assignments: Option<&[Vec<MeshFaceBoundaryAssignment>]>,
    solution_valid: &F,
) -> Option<Vec<Vec<[usize; 2]>>>
where
    F: Fn(&[[usize; 2]]) -> bool,
{
    let mut choices = edge_candidates.to_vec();
    for candidates in &mut choices {
        for pair in candidates.iter_mut() {
            pair.sort_unstable();
        }
        candidates.sort_unstable();
        candidates.dedup();
    }
    prune_incidence_choices(&mut choices, edge_faces, face_count, vertex_points.len())?;
    let complete_valid = |points: &[[usize; 2]]| {
        solution_valid(points)
            && reconstruct_incidence(
                edge_rows.to_vec(),
                vertex_points.to_vec(),
                edge_faces,
                points,
                face_count,
            )
            .is_some()
    };
    let solutions = component_incidence_pair_solutions(
        &choices,
        edge_faces,
        face_count,
        vertex_points.len(),
        mesh_assignments,
        &complete_valid,
    )?;
    (!solutions.is_empty()).then_some(solutions)
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

fn unique_duplicate_face_assignment<F>(
    serialized: &[[usize; 2]],
    allowed_faces: &[Vec<usize>],
    face_count: usize,
    mut valid: F,
) -> Option<Vec<[usize; 2]>>
where
    F: FnMut(&[[usize; 2]]) -> bool,
{
    const MAX_STATES: usize = 4_096;

    fn search<F>(
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
        standard_mesh_boundary_assignments(bytes, assignment).is_some()
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

    fn adjust(degrees: &mut HashMap<usize, u8>, pair: [usize; 2], increase: bool) {
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

    fn search(
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

fn resolve_edge_faces_from_runs(
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

/// Retain every complete span-consistent assignment for edge rows that have no
/// exact interior-handle occurrence. The outer vector is face order; each face
/// contains complete assignments, and each assignment contains one placement
/// per missing edge use. The result is atomic when a face has more than 65,536
/// complete assignments.
#[must_use]
pub fn standard_mesh_missing_edge_assignments(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
) -> Option<Vec<Vec<Vec<MeshEdgePlacementCandidate>>>> {
    standard_mesh_missing_edge_assignments_impl(bytes, edge_faces, None, false)
}

fn standard_mesh_missing_edge_assignments_impl(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: Option<&[Vec<[usize; 2]>]>,
    canonicalize_spans: bool,
) -> Option<Vec<Vec<Vec<MeshEdgePlacementCandidate>>>> {
    const MAX_ASSIGNMENTS_PER_FACE: usize = 65_536;
    const MAX_EXHAUSTIVE_GAPS: usize = 9;
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
                    if current_port
                        .zip(
                            self.corner_ports
                                .get(&(self.face, value.cycle, end))
                                .copied(),
                        )
                        .is_some_and(|(actual, expected)| actual != expected)
                    {
                        return Some(());
                    }
                    let end_points = self.corner_points.get(&(self.face, value.cycle, end));
                    if current_points
                        .as_ref()
                        .zip(end_points)
                        .is_some_and(|(actual, expected)| actual.is_disjoint(expected))
                    {
                        return Some(());
                    }
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
            canonical_spans: canonicalize_spans
                && (gaps.len() == 1 || gaps.len() > MAX_EXHAUSTIVE_GAPS),
            canonical_gap_partitions: canonicalize_spans && gaps.len() > MAX_EXHAUSTIVE_GAPS,
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

        fn order_trails(
            trails: &[EndpointTrail],
            used: u64,
            edges: &mut Vec<usize>,
            orders: &mut Vec<Vec<usize>>,
        ) {
            if used.count_ones() as usize == trails.len() {
                orders.push(edges.clone());
                return;
            }
            for (index, trail) in trails.iter().enumerate() {
                if used & (1 << index) != 0 {
                    continue;
                }
                for reversed in [false, true] {
                    if reversed && trail.edges.len() == 1 {
                        continue;
                    }
                    let before = edges.len();
                    if reversed {
                        edges.extend(trail.edges.iter().rev());
                    } else {
                        edges.extend(&trail.edges);
                    }
                    order_trails(trails, used | (1 << index), edges, orders);
                    edges.truncate(before);
                }
            }
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
        let mut orders = Vec::new();
        order_trails(&trails, 0, &mut Vec::new(), &mut orders);
        (orders.len() <= MAX_ASSIGNMENTS_PER_FACE).then(|| {
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
                .collect()
        })
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
        let assignments = coverage
            .iter()
            .map(|face| {
                let cycle_lengths = cycles[face.face].iter().map(Vec::len).collect::<Vec<_>>();
                let assignments = singleton_edge_points
                    .as_ref()
                    .and_then(|edge_points| {
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
            })
            .collect::<Option<Vec<_>>>()?;
        solutions.push(assignments);
    }
    <[Vec<Vec<Vec<MeshEdgePlacementCandidate>>>; 1]>::try_from(solutions)
        .ok()
        .map(|[assignments]| assignments)
}

/// Project complete unmatched-edge assignments to the placement domain for
/// each face. Every unmatched row may cover any positive remaining span;
/// row arity fixes a span only after its interior handles match the boundary.
#[must_use]
pub fn standard_mesh_missing_edge_placements(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
) -> Option<Vec<Vec<MeshEdgePlacementCandidate>>> {
    standard_mesh_missing_edge_assignments(bytes, edge_faces).map(|faces| {
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

/// Combine exact interior-handle runs with each complete unmatched-edge
/// assignment to form ordered, gap-free candidate boundaries.
#[must_use]
pub fn standard_mesh_boundary_assignments(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
) -> Option<Vec<Vec<MeshFaceBoundaryAssignment>>> {
    standard_mesh_boundary_assignments_impl(bytes, edge_faces, None)
}

fn standard_mesh_boundary_assignments_impl(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: Option<&[Vec<[usize; 2]>]>,
) -> Option<Vec<Vec<MeshFaceBoundaryAssignment>>> {
    let assignments =
        standard_mesh_missing_edge_assignments_impl(bytes, edge_faces, edge_candidates, true)?;
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
    assignments
        .into_iter()
        .enumerate()
        .map(|(face, assignments)| {
            assignments
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
        })
        .collect()
}

/// Materialize one complete face-assignment selection and one direction for
/// each ordered edge use into its abstract logical-corner quotient.
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
    let assignments = standard_mesh_boundary_assignments(bytes, edge_faces)?;
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
    work: &mut usize,
) -> Option<HashMap<usize, HashSet<[usize; 2]>>> {
    const MAX_BOUNDARY_SUPPORT_WORK: usize = 20_000_000;

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
    let first_points = first_layer
        .iter()
        .map(|state| state.start)
        .collect::<HashSet<_>>();
    let mut supported = layers
        .iter()
        .map(|layer| vec![false; layer.len()])
        .collect::<Vec<_>>();
    for first_point in first_points {
        let mut forward = layers
            .iter()
            .map(|layer| vec![false; layer.len()])
            .collect::<Vec<_>>();
        for (state, reachable) in first_layer.iter().zip(&mut forward[0]) {
            *reachable = state.start == first_point;
        }
        for layer in 1..layers.len() {
            *work = work.checked_add(layers[layer - 1].len() + layers[layer].len())?;
            if *work > MAX_BOUNDARY_SUPPORT_WORK {
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
    let mut faces = standard_mesh_boundary_assignments(bytes, edge_faces)?;
    loop {
        let before = (
            faces.iter().map(Vec::len).sum::<usize>(),
            candidates.iter().map(Vec::len).sum::<usize>(),
        );
        let mut face_supports = Vec::with_capacity(faces.len());
        let mut work = 0usize;
        for assignments in &mut faces {
            let evaluated = assignments
                .iter()
                .enumerate()
                .filter_map(|(index, assignment)| {
                    let mut support = HashMap::<usize, HashSet<[usize; 2]>>::new();
                    for boundary in &assignment.boundaries {
                        for (edge, domain) in
                            boundary_endpoint_support(boundary, &candidates, &mut work)?
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
    let assignments = standard_mesh_missing_edge_assignments_impl(bytes, edge_faces, None, true)?;
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

    fn search(&mut self) {
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

fn motif_port_points(trims: &[TrimRecord], vertex_count: usize) -> Option<HashMap<u32, usize>> {
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

fn reconstruct_incidence(
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<[f64; 3]>,
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    face_count: usize,
) -> Option<StandardTopology> {
    reconstruct_incidence_with_edge_classes(
        edge_rows,
        vertex_points,
        edge_faces,
        edge_points,
        face_count,
        None,
    )
}

fn reconstruct_incidence_with_edge_classes(
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<[f64; 3]>,
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    face_count: usize,
    edge_classes: Option<&[usize]>,
) -> Option<StandardTopology> {
    reconstruct_incidence_with_edge_classes_and_mesh(
        edge_rows,
        vertex_points,
        edge_faces,
        edge_points,
        face_count,
        edge_classes,
        None,
    )
}

fn reconstruct_incidence_with_edge_classes_and_mesh(
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<[f64; 3]>,
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    face_count: usize,
    edge_classes: Option<&[usize]>,
    mesh_bytes: Option<&[u8]>,
) -> Option<StandardTopology> {
    let completed_edge_faces = complete_duplicate_face_slots(
        &edge_rows,
        edge_faces,
        edge_points,
        face_count,
        edge_classes,
        mesh_bytes,
    )?;
    let edge_faces = completed_edge_faces.as_slice();
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

fn complete_duplicate_face_slots(
    edge_rows: &[EdgeRow],
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    face_count: usize,
    edge_classes: Option<&[usize]>,
    mesh_bytes: Option<&[u8]>,
) -> Option<Vec<[usize; 2]>> {
    struct SearchInputs<'a> {
        unresolved: &'a [usize],
        edge_rows: &'a [EdgeRow],
        edge_faces: &'a [[usize; 2]],
        edge_points: &'a [[usize; 2]],
        edge_classes: Option<&'a [usize]>,
        mesh_bytes: Option<&'a [u8]>,
    }

    fn search(
        inputs: &SearchInputs<'_>,
        degrees: &mut [Vec<u8>],
        assignment: &mut [usize],
        used: &mut [bool],
        solutions: &mut Vec<Vec<usize>>,
    ) {
        if solutions.len() > 1 {
            return;
        }
        if used.iter().all(|value| *value) {
            let closed = degrees
                .iter()
                .all(|face| face.iter().all(|degree| matches!(degree, 0 | 2)));
            let mesh_valid = closed
                && inputs.mesh_bytes.is_none_or(|bytes| {
                    let mut completed = inputs.edge_faces.to_vec();
                    for (&edge, &face) in inputs.unresolved.iter().zip(assignment.iter()) {
                        completed[edge][1] = face;
                    }
                    standard_mesh_boundary_assignments(bytes, &completed).is_some()
                });
            if mesh_valid
                && solutions.first().is_none_or(|existing| {
                    !duplicate_face_assignments_equivalent(
                        inputs.unresolved,
                        inputs.edge_rows,
                        inputs.edge_faces,
                        inputs.edge_points,
                        inputs.edge_classes,
                        existing,
                        assignment,
                    )
                })
            {
                solutions.push(assignment.to_vec());
            }
            return;
        }
        let deficit = degrees.iter().enumerate().find_map(|(face, values)| {
            values
                .iter()
                .position(|degree| *degree == 1)
                .map(|point| (face, point))
        });
        let choices = inputs
            .unresolved
            .iter()
            .enumerate()
            .filter(|(index, edge)| {
                if used[*index] {
                    return false;
                }
                let [start, end] = inputs.edge_points[**edge];
                deficit.is_none_or(|(_, point)| start == point || end == point)
            })
            .flat_map(|(index, &edge)| {
                let faces: Box<dyn Iterator<Item = usize>> = match deficit {
                    Some((face, _)) => Box::new(std::iter::once(face)),
                    None => Box::new(0..degrees.len()),
                };
                faces.map(move |face| (index, edge, face))
            })
            .filter(|(_, edge, face)| {
                let [start, end] = inputs.edge_points[*edge];
                degrees[*face][start] + 1 + u8::from(start == end) <= 2
                    && (start == end || degrees[*face][end] < 2)
            })
            .collect::<Vec<_>>();
        for (index, edge, face) in choices {
            let [start, end] = inputs.edge_points[edge];
            let start_add = 1 + u8::from(start == end);
            degrees[face][start] += start_add;
            if start != end {
                degrees[face][end] += 1;
            }
            assignment[index] = face;
            used[index] = true;
            search(inputs, degrees, assignment, used, solutions);
            used[index] = false;
            degrees[face][start] -= start_add;
            if start != end {
                degrees[face][end] -= 1;
            }
        }
    }

    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_points.len()
        || edge_faces.iter().flatten().any(|face| *face >= face_count)
    {
        return None;
    }

    let mut completed = edge_faces.to_vec();
    let mut unresolved = edge_faces
        .iter()
        .enumerate()
        .filter_map(|(edge, faces)| (faces[0] == faces[1]).then_some(edge))
        .collect::<Vec<_>>();
    if unresolved.is_empty() {
        return Some(completed);
    }
    let point_count = edge_points
        .iter()
        .flatten()
        .max()
        .copied()
        .map(|point| point + 1)?;
    let mut degrees = vec![vec![0u8; point_count]; face_count];
    for (edge, faces) in edge_faces.iter().enumerate() {
        let mut incident = *faces;
        incident.sort_unstable();
        for &face in if incident[0] == incident[1] {
            &incident[..1]
        } else {
            &incident[..]
        } {
            for &point in &edge_points[edge] {
                degrees[face][point] = degrees[face][point].checked_add(1)?;
            }
        }
    }
    if degrees.iter().flatten().any(|degree| *degree > 2) {
        return None;
    }
    unresolved.sort_by_key(|&edge| {
        let [start, end] = edge_points[edge];
        degrees
            .iter()
            .filter(|face| {
                face[start] + 1 + u8::from(start == end) <= 2 && (start == end || face[end] < 2)
            })
            .count()
    });

    let mut solutions = Vec::new();
    let inputs = SearchInputs {
        unresolved: &unresolved,
        edge_rows,
        edge_faces,
        edge_points,
        edge_classes,
        mesh_bytes: None,
    };
    search(
        &inputs,
        &mut degrees,
        &mut vec![0; unresolved.len()],
        &mut vec![false; unresolved.len()],
        &mut solutions,
    );
    if solutions.len() > 1 {
        let bytes = mesh_bytes?;
        solutions.clear();
        let inputs = SearchInputs {
            unresolved: &unresolved,
            edge_rows,
            edge_faces,
            edge_points,
            edge_classes,
            mesh_bytes: Some(bytes),
        };
        search(
            &inputs,
            &mut degrees,
            &mut vec![0; unresolved.len()],
            &mut vec![false; unresolved.len()],
            &mut solutions,
        );
    }
    let [assignment] = solutions.as_slice() else {
        return None;
    };
    for (&edge, &face) in unresolved.iter().zip(assignment) {
        completed[edge][1] = face;
    }
    Some(completed)
}

fn duplicate_face_assignments_equivalent(
    unresolved: &[usize],
    edge_rows: &[EdgeRow],
    edge_faces: &[[usize; 2]],
    edge_points: &[[usize; 2]],
    edge_classes: Option<&[usize]>,
    left: &[usize],
    right: &[usize],
) -> bool {
    let mut classified = vec![false; unresolved.len()];
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
            return false;
        }
    }
    true
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
    let mut cycles = Vec::new();
    for &edge in incident {
        let [start, end] = edge_points[edge];
        if start == end {
            cycles.push(vec![(edge, false)]);
            continue;
        }
        at_vertex.entry(start).or_default().push(edge);
        at_vertex.entry(end).or_default().push(edge);
    }
    if at_vertex.values().any(|edges| edges.len() != 2) {
        return None;
    }
    let mut unseen: HashSet<usize> = incident
        .iter()
        .copied()
        .filter(|edge| edge_points[*edge][0] != edge_points[*edge][1])
        .collect();
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

/// Parses the FBB-only spine. Its edge rows and trim handles use one selected
/// big-endian width; the
/// following counted `05 08 01` table supplies vertex coordinates.
#[must_use]
pub fn parse_fbb(bytes: &[u8]) -> Option<StandardTopology> {
    let (face_start, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, _, vertex_header, handle_width) = parse_fbb_edge_tables(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    let trims = parse_trim_chain(bytes, face_start, face_count, handle_width)?;
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

fn reconstruct_mesh_selection(
    edge_rows: Vec<EdgeRow>,
    vertex_points: Vec<[f64; 3]>,
    selected: &[MeshFaceBoundaryAssignment],
    unmatched_reversed: &[Vec<Vec<bool>>],
) -> Option<StandardTopology> {
    if selected.len() != unmatched_reversed.len() {
        return None;
    }
    let mut union = UnionFind::new(edge_rows.len() * 2);
    let mut faces = Vec::with_capacity(selected.len());
    for (face, directions) in selected.iter().zip(unmatched_reversed) {
        if face.boundaries.len() != directions.len() {
            return None;
        }
        let mut boundaries = Vec::with_capacity(face.boundaries.len());
        for (uses, directions) in face.boundaries.iter().zip(directions) {
            if uses.len() != directions.len() || uses.is_empty() {
                return None;
            }
            let corners = (0..uses.len()).map(|_| union.push()).collect::<Vec<_>>();
            let mut coedges = Vec::with_capacity(uses.len());
            for (use_index, (use_, &unmatched_reversed)) in uses.iter().zip(directions).enumerate()
            {
                let reversed = use_.reversed.unwrap_or(unmatched_reversed);
                if use_.reversed.is_some() && unmatched_reversed != reversed {
                    return None;
                }
                let start_vertex = corners[use_index];
                let end_vertex = corners[(use_index + 1) % corners.len()];
                let edge_start = use_.edge.checked_mul(2)?;
                let edge_end = edge_start.checked_add(1)?;
                if edge_end >= edge_rows.len() * 2 {
                    return None;
                }
                if reversed {
                    union.union(edge_end, start_vertex);
                    union.union(edge_start, end_vertex);
                } else {
                    union.union(edge_start, start_vertex);
                    union.union(edge_end, end_vertex);
                }
                coedges.push(CoedgeUse {
                    edge_row: use_.edge,
                    reversed,
                    start_vertex,
                    end_vertex,
                });
            }
            boundaries.push(Boundary { coedges });
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
        logical_vertex_count: roots.len(),
        vertex_points,
    })
}

#[derive(Clone)]
struct MeshQuotient {
    union: UnionFind,
    domains: Vec<Arc<HashSet<usize>>>,
    members: Vec<Vec<usize>>,
}

impl MeshQuotient {
    fn signature(&mut self) -> Vec<(Vec<usize>, Vec<usize>)> {
        let mut components = Vec::new();
        for node in 0..self.union.len() {
            if self.union.find(node) != node {
                continue;
            }
            let mut members = self.members[node].clone();
            members.sort_unstable();
            let mut domain = self.domains[node].iter().copied().collect::<Vec<_>>();
            domain.sort_unstable();
            components.push((members, domain));
        }
        components.sort_unstable();
        components
    }

    fn root_count(&mut self) -> usize {
        (0..self.union.len())
            .filter(|node| self.union.find(*node) == *node)
            .count()
    }

    fn merge(&mut self, left: usize, right: usize) -> Option<usize> {
        let left = self.union.find(left);
        let right = self.union.find(right);
        if left == right {
            return Some(left);
        }
        let intersection = self.domains[left]
            .intersection(&self.domains[right])
            .copied()
            .collect::<HashSet<_>>();
        if intersection.is_empty() {
            return None;
        }
        self.union.union(left, right);
        let root = self.union.find(left);
        self.domains[root] = Arc::new(intersection);
        let child = if root == left { right } else { left };
        let child_members = std::mem::take(&mut self.members[child]);
        self.members[root].extend(child_members);
        Some(root)
    }

    fn edge_domains_viable(&mut self, edge_candidates: &[Vec<[usize; 2]>]) -> bool {
        self.propagate_edge_domains(
            edge_candidates
                .iter()
                .enumerate()
                .filter_map(|(edge, candidates)| (!candidates.is_empty()).then_some(edge)),
            edge_candidates,
        )
    }

    fn propagate_component_edge_domains(
        &mut self,
        root: usize,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> bool {
        let edges = self.members[root]
            .iter()
            .map(|node| node / 2)
            .filter(|edge| !edge_candidates[*edge].is_empty())
            .collect::<HashSet<_>>();
        self.propagate_edge_domains(edges, edge_candidates)
    }

    fn propagate_edge_domains(
        &mut self,
        edges: impl IntoIterator<Item = usize>,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> bool {
        fn enqueue_component_edges(
            root: usize,
            members: &[Vec<usize>],
            edge_candidates: &[Vec<[usize; 2]>],
            queue: &mut VecDeque<usize>,
            queued: &mut HashSet<usize>,
        ) {
            for edge in members[root].iter().map(|node| node / 2) {
                if !edge_candidates[edge].is_empty() && queued.insert(edge) {
                    queue.push_back(edge);
                }
            }
        }

        let mut queue = VecDeque::new();
        let mut queued = HashSet::new();
        for edge in edges {
            if queued.insert(edge) {
                queue.push_back(edge);
            }
        }
        while let Some(edge) = queue.pop_front() {
            queued.remove(&edge);
            let candidates = &edge_candidates[edge];
            if candidates.is_empty() {
                continue;
            }
            let start = self.union.find(edge * 2);
            let end = self.union.find(edge * 2 + 1);
            if start == end {
                let supported = candidates
                    .iter()
                    .filter(|pair| pair[0] == pair[1])
                    .map(|pair| pair[0])
                    .filter(|point| self.domains[start].contains(point))
                    .collect::<HashSet<_>>();
                if supported.is_empty() {
                    return false;
                }
                if supported != *self.domains[start] {
                    self.domains[start] = Arc::new(supported);
                    enqueue_component_edges(
                        start,
                        &self.members,
                        edge_candidates,
                        &mut queue,
                        &mut queued,
                    );
                }
                continue;
            }

            let starts = self.domains[start].clone();
            let ends = self.domains[end].clone();
            let mut supported_starts = HashSet::new();
            let mut supported_ends = HashSet::new();
            for &[left, right] in candidates {
                if starts.contains(&left) && ends.contains(&right) {
                    supported_starts.insert(left);
                    supported_ends.insert(right);
                }
                if starts.contains(&right) && ends.contains(&left) {
                    supported_starts.insert(right);
                    supported_ends.insert(left);
                }
            }
            if supported_starts.is_empty() || supported_ends.is_empty() {
                return false;
            }
            if supported_starts != *self.domains[start] {
                self.domains[start] = Arc::new(supported_starts);
                enqueue_component_edges(
                    start,
                    &self.members,
                    edge_candidates,
                    &mut queue,
                    &mut queued,
                );
            }
            if supported_ends != *self.domains[end] {
                self.domains[end] = Arc::new(supported_ends);
                enqueue_component_edges(
                    end,
                    &self.members,
                    edge_candidates,
                    &mut queue,
                    &mut queued,
                );
            }
        }
        true
    }

    fn merge_singleton_coordinate_roots(&mut self, edge_candidates: &[Vec<[usize; 2]>]) -> bool {
        loop {
            let mut roots_by_point = HashMap::<usize, Vec<usize>>::new();
            for node in 0..self.union.len() {
                let root = self.union.find(node);
                if root != node || self.domains[root].len() != 1 {
                    continue;
                }
                let Some(&point) = self.domains[root].iter().next() else {
                    return false;
                };
                roots_by_point.entry(point).or_default().push(root);
            }
            let mut changed = false;
            for roots in roots_by_point.into_values() {
                let Some((&first, rest)) = roots.split_first() else {
                    continue;
                };
                for &root in rest {
                    if self.merge(first, root).is_none() {
                        return false;
                    }
                    changed = true;
                }
            }
            if !changed {
                return true;
            }
            if !self.edge_domains_viable(edge_candidates) {
                return false;
            }
        }
    }

    fn close_coordinate_roots(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Option<HashMap<usize, usize>> {
        fn pair_supported(candidates: &[[usize; 2]], left: usize, right: usize) -> bool {
            candidates.is_empty()
                || candidates
                    .iter()
                    .any(|pair| same_unordered_pair(*pair, [left, right]))
        }

        #[allow(clippy::too_many_arguments)]
        fn walk(
            domains: &[Vec<usize>],
            edges: &[[usize; 2]],
            edge_ids: &[usize],
            root_edges: &[Vec<usize>],
            edge_candidates: &[Vec<[usize; 2]>],
            component_points: &HashSet<usize>,
            assigned: &mut [Option<usize>],
            point_uses: &mut [usize],
            solutions: &mut Vec<Vec<usize>>,
            states: &mut usize,
            exhausted: &mut bool,
        ) {
            const MAX_COORDINATE_CLOSURE_STATES: usize = 256;

            fn rollback(
                assigned: &mut [Option<usize>],
                point_uses: &mut [usize],
                propagated: Vec<(usize, usize)>,
            ) {
                for (root, point) in propagated.into_iter().rev() {
                    point_uses[point] -= 1;
                    assigned[root] = None;
                }
            }

            if solutions.len() > 1 || *exhausted {
                return;
            }
            let viable_values = |root: usize, assigned: &[Option<usize>]| {
                domains[root]
                    .iter()
                    .copied()
                    .filter(|point| {
                        root_edges[root].iter().all(|edge| {
                            let [left, right] = edges[*edge];
                            let other = if left == root { right } else { left };
                            assigned[other].is_none_or(|other_point| {
                                pair_supported(
                                    &edge_candidates[edge_ids[*edge]],
                                    *point,
                                    other_point,
                                )
                            })
                        })
                    })
                    .collect::<Vec<_>>()
            };

            let mut propagated = Vec::new();
            let branch = loop {
                let remaining = assigned.iter().filter(|point| point.is_none()).count();
                let unused = component_points
                    .iter()
                    .filter(|point| point_uses[**point] == 0)
                    .count();
                if remaining < unused {
                    break None;
                }
                let mut best = None;
                let mut dead = false;
                let mut progress = false;
                let mut supported_unused = HashSet::new();
                for root in 0..assigned.len() {
                    if assigned[root].is_some() {
                        continue;
                    }
                    let values = viable_values(root, assigned);
                    if values.is_empty() {
                        dead = true;
                        break;
                    }
                    supported_unused.extend(
                        values
                            .iter()
                            .copied()
                            .filter(|point| point_uses[*point] == 0),
                    );
                    if let [point] = values.as_slice() {
                        assigned[root] = Some(*point);
                        point_uses[*point] += 1;
                        propagated.push((root, *point));
                        progress = true;
                    } else if best
                        .as_ref()
                        .is_none_or(|(_, stored): &(usize, Vec<usize>)| values.len() < stored.len())
                    {
                        best = Some((root, values));
                    }
                }
                if dead {
                    break None;
                }
                if progress {
                    continue;
                }
                if component_points
                    .iter()
                    .any(|point| point_uses[*point] == 0 && !supported_unused.contains(point))
                {
                    break None;
                }
                break Some(best);
            };
            let Some(branch) = branch else {
                rollback(assigned, point_uses, propagated);
                return;
            };
            let Some((root, values)) = branch else {
                if component_points.iter().all(|point| point_uses[*point] > 0) {
                    solutions.push(
                        assigned
                            .iter()
                            .copied()
                            .collect::<Option<Vec<_>>>()
                            .expect("complete coordinate assignment"),
                    );
                }
                rollback(assigned, point_uses, propagated);
                return;
            };
            if *states >= MAX_COORDINATE_CLOSURE_STATES {
                *exhausted = true;
                rollback(assigned, point_uses, propagated);
                return;
            }
            *states += 1;
            for point in values {
                assigned[root] = Some(point);
                point_uses[point] += 1;
                walk(
                    domains,
                    edges,
                    edge_ids,
                    root_edges,
                    edge_candidates,
                    component_points,
                    assigned,
                    point_uses,
                    solutions,
                    states,
                    exhausted,
                );
                point_uses[point] -= 1;
                assigned[root] = None;
                if solutions.len() > 1 || *exhausted {
                    break;
                }
            }
            rollback(assigned, point_uses, propagated);
        }

        let mut roots = Vec::new();
        for node in 0..self.union.len() {
            if self.union.find(node) == node {
                roots.push(node);
            }
        }
        if roots.len() < point_count {
            return None;
        }
        if roots.len() == point_count {
            return self.point_assignment(point_count, edge_candidates);
        }
        let root_indices = roots
            .iter()
            .enumerate()
            .map(|(index, root)| (*root, index))
            .collect::<HashMap<_, _>>();
        let edges = edge_candidates
            .iter()
            .enumerate()
            .map(|(edge, _)| {
                Some([
                    *root_indices.get(&self.union.find(edge * 2))?,
                    *root_indices.get(&self.union.find(edge * 2 + 1))?,
                ])
            })
            .collect::<Option<Vec<_>>>()?;
        let domains = roots
            .iter()
            .map(|root| {
                self.domains[*root]
                    .iter()
                    .copied()
                    .filter(|point| *point < point_count)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if domains.iter().any(Vec::is_empty) {
            return None;
        }
        if domains
            .iter()
            .flatten()
            .copied()
            .collect::<HashSet<_>>()
            .len()
            != point_count
        {
            return None;
        }
        let mut dependency = UnionFind::new(roots.len());
        for [left, right] in &edges {
            dependency.union(*left, *right);
        }
        let mut root_by_point = HashMap::new();
        for (root, domain) in domains.iter().enumerate() {
            for point in domain {
                if let Some(previous) = root_by_point.insert(*point, root) {
                    dependency.union(previous, root);
                }
            }
        }
        let mut components = HashMap::<usize, Vec<usize>>::new();
        for root in 0..roots.len() {
            components
                .entry(dependency.find(root))
                .or_default()
                .push(root);
        }
        let mut components = components.into_values().collect::<Vec<_>>();
        components.sort_by_key(|component| component[0]);
        let mut assignment = vec![None; roots.len()];
        for component in components {
            let component_set = component.iter().copied().collect::<HashSet<_>>();
            let local_index = component
                .iter()
                .enumerate()
                .map(|(local, global)| (*global, local))
                .collect::<HashMap<_, _>>();
            let edge_ids = edges
                .iter()
                .enumerate()
                .filter_map(|(edge, [left, _])| component_set.contains(left).then_some(edge))
                .collect::<Vec<_>>();
            let local_edges = edge_ids
                .iter()
                .map(|edge| {
                    let [left, right] = edges[*edge];
                    Some([*local_index.get(&left)?, *local_index.get(&right)?])
                })
                .collect::<Option<Vec<_>>>()?;
            let local_domains = component
                .iter()
                .map(|root| domains[*root].clone())
                .collect::<Vec<_>>();
            let component_points = local_domains
                .iter()
                .flatten()
                .copied()
                .collect::<HashSet<_>>();
            let mut root_edges = vec![Vec::new(); component.len()];
            for (edge, [left, right]) in local_edges.iter().copied().enumerate() {
                root_edges[left].push(edge);
                if right != left {
                    root_edges[right].push(edge);
                }
            }
            let mut solutions = Vec::new();
            let mut states = 0;
            let mut exhausted = false;
            walk(
                &local_domains,
                &local_edges,
                &edge_ids,
                &root_edges,
                edge_candidates,
                &component_points,
                &mut vec![None; component.len()],
                &mut vec![0; point_count],
                &mut solutions,
                &mut states,
                &mut exhausted,
            );
            if exhausted {
                return None;
            }
            let [local_assignment] = solutions.as_slice() else {
                return None;
            };
            for (&root, &point) in component.iter().zip(local_assignment) {
                assignment[root] = Some(point);
            }
        }
        let assignment = assignment.into_iter().collect::<Option<Vec<_>>>()?;
        let mut root_by_point = HashMap::new();
        for (&root, &point) in roots.iter().zip(&assignment) {
            if let Some(previous) = root_by_point.insert(point, root) {
                let merged = self.merge(previous, root)?;
                root_by_point.insert(point, merged);
            }
        }
        if !self.edge_domains_viable(edge_candidates) {
            return None;
        }
        self.point_assignment(point_count, edge_candidates)
    }

    fn assignment_has_option(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> bool {
        fn edge_start(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge.checked_mul(2)?.checked_add(usize::from(reversed))
        }

        fn edge_end(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge
                .checked_mul(2)?
                .checked_add(usize::from(!reversed))
        }

        #[derive(Clone)]
        struct State {
            boundary_index: usize,
            at: usize,
            directions: Vec<bool>,
            quotient: MeshQuotient,
        }

        fn advance(
            state: &mut State,
            boundary: &[MeshBoundaryEdgeCandidate],
            reversed: bool,
        ) -> bool {
            if state.at > 0 {
                let Some(previous_end) =
                    edge_end(boundary[state.at - 1], state.directions[state.at - 1])
                else {
                    return false;
                };
                let Some(current_start) = edge_start(boundary[state.at], reversed) else {
                    return false;
                };
                if state.quotient.merge(previous_end, current_start).is_none() {
                    return false;
                }
            }
            state.directions.push(reversed);
            state.at += 1;
            true
        }

        let mut states = vec![State {
            boundary_index: 0,
            at: 0,
            directions: Vec::new(),
            quotient: self.clone(),
        }];
        while let Some(mut state) = states.pop() {
            loop {
                if state.boundary_index == assignment.boundaries.len() {
                    return true;
                }
                let boundary = &assignment.boundaries[state.boundary_index];
                if boundary.is_empty() {
                    break;
                }
                if state.at == boundary.len() {
                    let Some(last_end) =
                        edge_end(boundary[state.at - 1], state.directions[state.at - 1])
                    else {
                        break;
                    };
                    let Some(first_start) = edge_start(boundary[0], state.directions[0]) else {
                        break;
                    };
                    if state.quotient.merge(last_end, first_start).is_none() {
                        break;
                    }
                    if !state.quotient.edge_domains_viable(edge_candidates) {
                        break;
                    }
                    state.boundary_index += 1;
                    state.at = 0;
                    state.directions.clear();
                    continue;
                }
                if let Some(reversed) = boundary[state.at].reversed {
                    if !advance(&mut state, boundary, reversed) {
                        break;
                    }
                    continue;
                }
                for reversed in [true, false] {
                    let mut next = state.clone();
                    if advance(&mut next, boundary, reversed)
                        && next.quotient.edge_domains_viable(edge_candidates)
                    {
                        states.push(next);
                    }
                }
                break;
            }
        }
        false
    }

    #[cfg(test)]
    fn assignment_options(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Vec<(Vec<Vec<bool>>, Self)> {
        const MAX_ORIENTED_OPTIONS: usize = 4_096;

        fn edge_start(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge.checked_mul(2)?.checked_add(usize::from(reversed))
        }

        fn edge_end(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge
                .checked_mul(2)?
                .checked_add(usize::from(!reversed))
        }

        fn boundary_options(
            quotient: MeshQuotient,
            boundary: &[MeshBoundaryEdgeCandidate],
            edge_candidates: &[Vec<[usize; 2]>],
        ) -> Vec<(Vec<bool>, MeshQuotient)> {
            fn advance(
                boundary: &[MeshBoundaryEdgeCandidate],
                at: usize,
                reversed: bool,
                directions: &mut Vec<bool>,
                mut quotient: MeshQuotient,
                edge_candidates: &[Vec<[usize; 2]>],
                output: &mut Vec<(Vec<bool>, MeshQuotient)>,
            ) {
                if at > 0 {
                    let Some(previous_end) = edge_end(boundary[at - 1], directions[at - 1]) else {
                        return;
                    };
                    let Some(current_start) = edge_start(boundary[at], reversed) else {
                        return;
                    };
                    let Some(root) = quotient.merge(previous_end, current_start) else {
                        return;
                    };
                    if !quotient.propagate_component_edge_domains(root, edge_candidates) {
                        return;
                    }
                }
                directions.push(reversed);
                walk(
                    boundary,
                    at + 1,
                    directions,
                    quotient,
                    edge_candidates,
                    output,
                );
                directions.pop();
            }

            fn walk(
                boundary: &[MeshBoundaryEdgeCandidate],
                at: usize,
                directions: &mut Vec<bool>,
                mut quotient: MeshQuotient,
                edge_candidates: &[Vec<[usize; 2]>],
                output: &mut Vec<(Vec<bool>, MeshQuotient)>,
            ) {
                if output.len() >= MAX_ORIENTED_OPTIONS {
                    return;
                }
                if at == boundary.len() {
                    let Some(last_end) = edge_end(boundary[at - 1], directions[at - 1]) else {
                        return;
                    };
                    let Some(first_start) = edge_start(boundary[0], directions[0]) else {
                        return;
                    };
                    let Some(root) = quotient.merge(last_end, first_start) else {
                        return;
                    };
                    if quotient.propagate_component_edge_domains(root, edge_candidates) {
                        output.push((directions.clone(), quotient));
                    }
                    return;
                }
                if let Some(reversed) = boundary[at].reversed {
                    advance(
                        boundary,
                        at,
                        reversed,
                        directions,
                        quotient,
                        edge_candidates,
                        output,
                    );
                } else {
                    advance(
                        boundary,
                        at,
                        false,
                        directions,
                        quotient.clone(),
                        edge_candidates,
                        output,
                    );
                    advance(
                        boundary,
                        at,
                        true,
                        directions,
                        quotient,
                        edge_candidates,
                        output,
                    );
                }
            }

            if boundary.is_empty() {
                return Vec::new();
            }
            let mut output = Vec::new();
            walk(
                boundary,
                0,
                &mut Vec::new(),
                quotient,
                edge_candidates,
                &mut output,
            );
            output
        }

        let mut options = vec![(Vec::new(), self.clone())];
        for boundary in &assignment.boundaries {
            let mut next = Vec::new();
            for (directions, quotient) in options {
                for (boundary_directions, quotient) in
                    boundary_options(quotient, boundary, edge_candidates)
                {
                    let mut directions = directions.clone();
                    directions.push(boundary_directions);
                    next.push((directions, quotient));
                    if next.len() >= MAX_ORIENTED_OPTIONS {
                        break;
                    }
                }
                if next.len() >= MAX_ORIENTED_OPTIONS {
                    break;
                }
            }
            options = next;
            if options.is_empty() {
                break;
            }
        }
        options
    }

    fn assignment_options_limited(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
        oriented_edges: &HashSet<usize>,
        limit: usize,
    ) -> Vec<(Vec<Vec<bool>>, Self)> {
        fn edge_start(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge.checked_mul(2)?.checked_add(usize::from(reversed))
        }

        fn edge_end(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge
                .checked_mul(2)?
                .checked_add(usize::from(!reversed))
        }

        #[allow(clippy::too_many_arguments)]
        fn walk(
            boundaries: &[Vec<MeshBoundaryEdgeCandidate>],
            boundary_index: usize,
            at: usize,
            boundary_directions: &mut Vec<bool>,
            directions: &mut Vec<Vec<bool>>,
            mut quotient: MeshQuotient,
            edge_candidates: &[Vec<[usize; 2]>],
            output: &mut Vec<(Vec<Vec<bool>>, MeshQuotient)>,
            seen: &mut HashSet<MeshOrientationSignature>,
            oriented_edges: &HashSet<usize>,
            limit: usize,
        ) {
            if output.len() >= limit {
                return;
            }
            if boundary_index == boundaries.len() {
                if !uses_canonical_edge_direction_gauge(boundaries, directions, oriented_edges) {
                    return;
                }
                let canonical_directions = directions
                    .iter()
                    .map(|boundary| {
                        let complement = boundary.iter().map(|value| !value).collect::<Vec<_>>();
                        if complement < *boundary {
                            complement
                        } else {
                            boundary.clone()
                        }
                    })
                    .collect::<Vec<_>>();
                let signature = (quotient.signature(), canonical_directions);
                if seen.insert(signature) {
                    output.push((directions.clone(), quotient));
                }
                return;
            }
            let boundary = &boundaries[boundary_index];
            if boundary.is_empty() {
                return;
            }
            if at == boundary.len() {
                let Some(last_end) = edge_end(boundary[at - 1], boundary_directions[at - 1]) else {
                    return;
                };
                let Some(first_start) = edge_start(boundary[0], boundary_directions[0]) else {
                    return;
                };
                let Some(root) = quotient.merge(last_end, first_start) else {
                    return;
                };
                if !quotient.propagate_component_edge_domains(root, edge_candidates) {
                    return;
                }
                directions.push(std::mem::take(boundary_directions));
                walk(
                    boundaries,
                    boundary_index + 1,
                    0,
                    boundary_directions,
                    directions,
                    quotient,
                    edge_candidates,
                    output,
                    seen,
                    oriented_edges,
                    limit,
                );
                *boundary_directions = directions.pop().unwrap_or_default();
                return;
            }
            let mut advance = |reversed: bool, mut quotient: MeshQuotient| {
                if at > 0 {
                    let Some(previous_end) =
                        edge_end(boundary[at - 1], boundary_directions[at - 1])
                    else {
                        return;
                    };
                    let Some(current_start) = edge_start(boundary[at], reversed) else {
                        return;
                    };
                    let Some(root) = quotient.merge(previous_end, current_start) else {
                        return;
                    };
                    if !quotient.propagate_component_edge_domains(root, edge_candidates) {
                        return;
                    }
                }
                boundary_directions.push(reversed);
                walk(
                    boundaries,
                    boundary_index,
                    at + 1,
                    boundary_directions,
                    directions,
                    quotient,
                    edge_candidates,
                    output,
                    seen,
                    oriented_edges,
                    limit,
                );
                boundary_directions.pop();
            };
            match boundary[at].reversed {
                Some(reversed) => advance(reversed, quotient),
                None => {
                    advance(false, quotient.clone());
                    advance(true, quotient);
                }
            }
        }

        if limit == 0 {
            return Vec::new();
        }
        if assignment.boundaries.iter().any(Vec::is_empty) {
            return Vec::new();
        }
        let unknown = assignment
            .boundaries
            .iter()
            .flatten()
            .filter(|use_| use_.reversed.is_none())
            .count();
        if unknown <= 12 {
            let mut output = Vec::new();
            let mut seen = HashSet::new();
            let combinations = 1usize << unknown;
            for mask in 0..combinations {
                if output.len() >= limit {
                    break;
                }
                let mut variable = 0usize;
                let directions = assignment
                    .boundaries
                    .iter()
                    .map(|boundary| {
                        boundary
                            .iter()
                            .map(|use_| {
                                use_.reversed.unwrap_or_else(|| {
                                    let shift = unknown - variable - 1;
                                    variable += 1;
                                    mask & (1usize << shift) != 0
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                if !uses_canonical_edge_direction_gauge(
                    &assignment.boundaries,
                    &directions,
                    oriented_edges,
                ) {
                    continue;
                }
                let mut quotient = self.clone();
                let viable =
                    assignment
                        .boundaries
                        .iter()
                        .zip(&directions)
                        .all(|(boundary, directions)| {
                            (0..boundary.len()).all(|index| {
                                let next = (index + 1) % boundary.len();
                                let Some(left_end) = edge_end(boundary[index], directions[index])
                                else {
                                    return false;
                                };
                                let Some(right_start) =
                                    edge_start(boundary[next], directions[next])
                                else {
                                    return false;
                                };
                                let Some(root) = quotient.merge(left_end, right_start) else {
                                    return false;
                                };
                                quotient.propagate_component_edge_domains(root, edge_candidates)
                            })
                        });
                if !viable {
                    continue;
                }
                let canonical_directions = directions
                    .iter()
                    .map(|boundary| {
                        let complement = boundary.iter().map(|value| !value).collect::<Vec<_>>();
                        if complement < *boundary {
                            complement
                        } else {
                            boundary.clone()
                        }
                    })
                    .collect::<Vec<_>>();
                if seen.insert((quotient.signature(), canonical_directions)) {
                    output.push((directions, quotient));
                }
            }
            return output;
        }
        let mut output = Vec::new();
        let mut seen = HashSet::new();
        walk(
            &assignment.boundaries,
            0,
            0,
            &mut Vec::new(),
            &mut Vec::new(),
            self.clone(),
            edge_candidates,
            &mut output,
            &mut seen,
            oriented_edges,
            limit,
        );
        output
    }

    fn point_assignment(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Option<HashMap<usize, usize>> {
        let mut solutions = self.point_assignments(point_count, edge_candidates, 2);
        (solutions.len() == 1).then(|| solutions.remove(0))
    }

    fn point_assignment_exists(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> bool {
        !self
            .point_assignments(point_count, edge_candidates, 1)
            .is_empty()
    }

    fn point_assignments(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        solution_limit: usize,
    ) -> Vec<HashMap<usize, usize>> {
        type PointNeighbors = HashMap<usize, HashSet<usize>>;

        fn remaining_domains_match(values: &[(usize, Vec<usize>)], point_count: usize) -> bool {
            domains_have_distinct_matching(
                values.iter().map(|(_, values)| values.as_slice()),
                point_count,
            )
        }

        #[allow(clippy::too_many_arguments)]
        fn value_viable(
            root: usize,
            point: usize,
            domains: &[Arc<HashSet<usize>>],
            edge_roots: &[[usize; 2]],
            root_edges: &[Vec<usize>],
            edge_candidates: &[Vec<[usize; 2]>],
            edge_neighbors: &[PointNeighbors],
            assigned: &[Option<usize>],
            used: &HashSet<usize>,
        ) -> bool {
            root_edges[root].iter().all(|&edge_index| {
                let edge = edge_roots[edge_index];
                let candidates = &edge_candidates[edge_index];
                let other = if edge[0] == root {
                    edge[1]
                } else if edge[1] == root {
                    edge[0]
                } else {
                    return true;
                };
                if other == root {
                    return candidates.is_empty()
                        || edge_neighbors[edge_index]
                            .get(&point)
                            .is_some_and(|neighbors| neighbors.contains(&point));
                }
                if let Some(other_point) = assigned[other] {
                    return candidates.is_empty()
                        || edge_neighbors[edge_index]
                            .get(&point)
                            .is_some_and(|neighbors| neighbors.contains(&other_point));
                }
                if candidates.is_empty() {
                    domains[other]
                        .iter()
                        .any(|other_point| *other_point != point && !used.contains(other_point))
                } else {
                    edge_neighbors[edge_index]
                        .get(&point)
                        .is_some_and(|neighbors| {
                            neighbors.iter().any(|other_point| {
                                *other_point != point
                                    && !used.contains(other_point)
                                    && domains[other].contains(other_point)
                            })
                        })
                }
            })
        }

        #[allow(clippy::too_many_arguments)]
        fn walk(
            domains: &[Arc<HashSet<usize>>],
            edge_roots: &[[usize; 2]],
            root_edges: &[Vec<usize>],
            edge_candidates: &[Vec<[usize; 2]>],
            edge_neighbors: &[PointNeighbors],
            assigned: &mut [Option<usize>],
            used: &mut HashSet<usize>,
            solutions: &mut Vec<Vec<usize>>,
            solution_limit: usize,
        ) {
            fn rollback(
                assigned: &mut [Option<usize>],
                used: &mut HashSet<usize>,
                propagated: Vec<(usize, usize)>,
            ) {
                for (root, point) in propagated.into_iter().rev() {
                    assigned[root] = None;
                    used.remove(&point);
                }
            }

            if solutions.len() >= solution_limit {
                return;
            }
            let values_for = |root: usize, assigned: &[Option<usize>], used: &HashSet<usize>| {
                domains[root]
                    .iter()
                    .copied()
                    .filter(|point| !used.contains(point))
                    .filter(|point| {
                        value_viable(
                            root,
                            *point,
                            domains,
                            edge_roots,
                            root_edges,
                            edge_candidates,
                            edge_neighbors,
                            assigned,
                            used,
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let mut propagated = Vec::new();
            let branch = loop {
                let values = assigned
                    .iter()
                    .enumerate()
                    .filter(|(_, point)| point.is_none())
                    .map(|(root, _)| (root, values_for(root, assigned, used)))
                    .collect::<Vec<_>>();
                if values.is_empty() {
                    break Some(None);
                }
                if values.iter().any(|(_, values)| values.is_empty())
                    || !remaining_domains_match(&values, assigned.len())
                {
                    break None;
                }
                let mut dead = false;
                let mut progress = false;
                for root in 0..assigned.len() {
                    if assigned[root].is_some() {
                        continue;
                    }
                    let values = values_for(root, assigned, used);
                    let Some(&point) = values.first() else {
                        dead = true;
                        break;
                    };
                    if values.len() != 1 {
                        continue;
                    }
                    if !used.insert(point) {
                        dead = true;
                        break;
                    }
                    assigned[root] = Some(point);
                    propagated.push((root, point));
                    progress = true;
                }
                if dead {
                    break None;
                }
                if !progress {
                    break Some(
                        values
                            .into_iter()
                            .min_by_key(|(root, values)| (values.len(), *root)),
                    );
                }
            };
            let Some(branch) = branch else {
                rollback(assigned, used, propagated);
                return;
            };
            let Some((root, values)) = branch else {
                if let Some(solution) = assigned.iter().copied().collect::<Option<Vec<_>>>() {
                    solutions.push(solution);
                }
                rollback(assigned, used, propagated);
                return;
            };
            for point in values {
                assigned[root] = Some(point);
                used.insert(point);
                walk(
                    domains,
                    edge_roots,
                    root_edges,
                    edge_candidates,
                    edge_neighbors,
                    assigned,
                    used,
                    solutions,
                    solution_limit,
                );
                used.remove(&point);
                assigned[root] = None;
                if solutions.len() >= solution_limit {
                    break;
                }
            }
            rollback(assigned, used, propagated);
        }

        let mut roots = Vec::new();
        for node in 0..self.union.len() {
            let root = self.union.find(node);
            if root == node {
                roots.push(root);
            }
        }
        if roots.len() != point_count {
            return Vec::new();
        }
        let domains = roots
            .iter()
            .map(|root| self.domains[*root].clone())
            .collect::<Vec<_>>();
        let root_indices = roots
            .iter()
            .enumerate()
            .map(|(index, root)| (*root, index))
            .collect::<HashMap<_, _>>();
        let Some(edge_roots) = edge_candidates
            .iter()
            .enumerate()
            .map(|(edge, _)| {
                Some([
                    *root_indices.get(&self.union.find(edge * 2))?,
                    *root_indices.get(&self.union.find(edge * 2 + 1))?,
                ])
            })
            .collect::<Option<Vec<_>>>()
        else {
            return Vec::new();
        };
        let mut root_edges = vec![Vec::new(); roots.len()];
        for (edge_index, edge) in edge_roots.iter().enumerate() {
            root_edges[edge[0]].push(edge_index);
            if edge[1] != edge[0] {
                root_edges[edge[1]].push(edge_index);
            }
        }
        let edge_neighbors = edge_candidates
            .iter()
            .map(|candidates| {
                let mut neighbors = PointNeighbors::new();
                for [left, right] in candidates {
                    neighbors.entry(*left).or_default().insert(*right);
                    neighbors.entry(*right).or_default().insert(*left);
                }
                neighbors
            })
            .collect::<Vec<_>>();

        let mut solutions = Vec::new();
        walk(
            &domains,
            &edge_roots,
            &root_edges,
            edge_candidates,
            &edge_neighbors,
            &mut vec![None; domains.len()],
            &mut HashSet::new(),
            &mut solutions,
            solution_limit,
        );
        solutions
            .into_iter()
            .map(|solution| roots.iter().copied().zip(solution).collect())
            .collect()
    }
}

type MeshFaceSelection = Option<(usize, Vec<Vec<bool>>)>;
type MeshEndpointPair = (usize, [usize; 2]);
type MeshEndpointSolutionFilter<'a> = &'a dyn Fn(&[MeshEndpointPair]) -> bool;
type MeshFaceEndpointConfiguration = Vec<MeshEndpointPair>;
type MeshFaceEndpointConfigurations = Vec<MeshFaceEndpointConfiguration>;
type MeshQuotientSignature = Vec<(Vec<usize>, Vec<usize>)>;
type MeshOrientationSignature = (MeshQuotientSignature, Vec<Vec<bool>>);
type MeshFaceEquationCache = RefCell<HashMap<(usize, MeshQuotientSignature), Vec<[usize; 2]>>>;

fn changed_quotient_edges(left: &MeshQuotient, right: &MeshQuotient) -> HashSet<usize> {
    let mut left = left.clone();
    let mut right = right.clone();
    (0..left.union.len())
        .filter_map(|node| {
            let left_root = left.union.find(node);
            let right_root = right.union.find(node);
            (left_root != right_root
                || left.members[left_root] != right.members[right_root]
                || left.domains[left_root] != right.domains[right_root])
                .then_some(node / 2)
        })
        .collect()
}

struct MeshSelectionSearch<'a> {
    assignments: &'a [Vec<MeshFaceBoundaryAssignment>],
    possible_face_equations: Vec<Vec<[usize; 2]>>,
    possible_face_choices: Vec<Vec<Vec<[usize; 2]>>>,
    face_work: Vec<Option<usize>>,
    edge_candidates: &'a [Vec<[usize; 2]>],
    edge_rows: &'a [EdgeRow],
    vertex_points: &'a [[f64; 3]],
    selected: Vec<MeshFaceSelection>,
    states: usize,
    solution: Option<(StandardTopology, Vec<usize>)>,
    stop_after_first_solution: bool,
    ambiguous: bool,
    exhausted: bool,
    face_equation_cache: MeshFaceEquationCache,
}

fn possible_face_equations(faces: &[Vec<MeshFaceBoundaryAssignment>]) -> Vec<Vec<[usize; 2]>> {
    fn ports(use_: MeshBoundaryEdgeCandidate, end: bool) -> [Option<usize>; 2] {
        let port = |reversed: bool| {
            use_.edge.checked_mul(2)?.checked_add(usize::from(if end {
                !reversed
            } else {
                reversed
            }))
        };
        match use_.reversed {
            Some(reversed) => [port(reversed), None],
            None => [port(false), port(true)],
        }
    }

    faces
        .iter()
        .map(|assignments| {
            let mut equations = HashSet::new();
            for assignment in assignments {
                for boundary in &assignment.boundaries {
                    if boundary.is_empty() {
                        continue;
                    }
                    for index in 0..boundary.len() {
                        let left = ports(boundary[index], true);
                        let right = ports(boundary[(index + 1) % boundary.len()], false);
                        for left in left.into_iter().flatten() {
                            for right in right.into_iter().flatten() {
                                equations.insert(if left <= right {
                                    [left, right]
                                } else {
                                    [right, left]
                                });
                            }
                        }
                    }
                }
            }
            let mut equations = equations.into_iter().collect::<Vec<_>>();
            equations.sort_unstable();
            equations
        })
        .collect()
}

fn possible_face_choices(
    faces: &[Vec<MeshFaceBoundaryAssignment>],
    face_equations: &[Vec<[usize; 2]>],
) -> Vec<Vec<Vec<[usize; 2]>>> {
    fn port(use_: MeshBoundaryEdgeCandidate, reversed: bool, end: bool) -> Option<usize> {
        use_.edge
            .checked_mul(2)?
            .checked_add(usize::from(if end { !reversed } else { reversed }))
    }

    faces
        .iter()
        .zip(face_equations)
        .map(|(assignments, fallback)| {
            let mut choices = HashSet::new();
            for assignment in assignments {
                let unknown = assignment
                    .boundaries
                    .iter()
                    .flatten()
                    .filter(|use_| use_.reversed.is_none())
                    .count();
                let Some(combinations) = 1usize.checked_shl(unknown as u32) else {
                    return vec![fallback.clone()];
                };
                if combinations > 4_096 {
                    return vec![fallback.clone()];
                }
                for mask in 0..combinations {
                    let mut variable = 0usize;
                    let directions = assignment
                        .boundaries
                        .iter()
                        .map(|boundary| {
                            boundary
                                .iter()
                                .map(|use_| {
                                    use_.reversed.unwrap_or_else(|| {
                                        let shift = unknown - variable - 1;
                                        variable += 1;
                                        mask & (1usize << shift) != 0
                                    })
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>();
                    let Some(mut equations) = assignment
                        .boundaries
                        .iter()
                        .zip(&directions)
                        .map(|(boundary, directions)| {
                            (0..boundary.len())
                                .map(|index| {
                                    let next = (index + 1) % boundary.len();
                                    let left = port(boundary[index], directions[index], true)?;
                                    let right = port(boundary[next], directions[next], false)?;
                                    Some(if left <= right {
                                        [left, right]
                                    } else {
                                        [right, left]
                                    })
                                })
                                .collect::<Option<Vec<_>>>()
                        })
                        .collect::<Option<Vec<_>>>()
                        .map(|boundaries| boundaries.into_iter().flatten().collect::<Vec<_>>())
                    else {
                        continue;
                    };
                    equations.sort_unstable();
                    equations.dedup();
                    choices.insert(equations);
                }
            }
            let mut choices = choices.into_iter().collect::<Vec<_>>();
            choices.sort_unstable();
            choices
        })
        .collect()
}

fn deduplicate_mesh_quotient_assignments(faces: &mut [Vec<MeshFaceBoundaryAssignment>]) {
    fn canonical_cycle(boundary: &[MeshBoundaryEdgeCandidate]) -> Vec<(usize, Option<bool>)> {
        fn rotations(values: &[(usize, Option<bool>)]) -> Vec<Vec<(usize, Option<bool>)>> {
            (0..values.len())
                .map(|start| {
                    values[start..]
                        .iter()
                        .chain(&values[..start])
                        .copied()
                        .collect()
                })
                .collect()
        }

        let forward = boundary
            .iter()
            .map(|use_| (use_.edge, use_.reversed))
            .collect::<Vec<_>>();
        let reversed = boundary
            .iter()
            .rev()
            .map(|use_| (use_.edge, use_.reversed.map(|value| !value)))
            .collect::<Vec<_>>();
        rotations(&forward)
            .into_iter()
            .chain(rotations(&reversed))
            .min()
            .unwrap_or_default()
    }

    for assignments in faces {
        let mut seen = HashSet::new();
        assignments.retain(|assignment| {
            let mut signature = assignment
                .boundaries
                .iter()
                .map(|boundary| canonical_cycle(boundary))
                .collect::<Vec<_>>();
            signature.sort_unstable();
            seen.insert(signature)
        });
    }
}

fn mesh_assignment_endpoint_cycles_viable_where(
    assignment: &MeshFaceBoundaryAssignment,
    edge_candidates: &[Vec<[usize; 2]>],
    allowed: impl Fn(usize, [usize; 2]) -> bool + Copy,
) -> Option<bool> {
    const MAX_LOCAL_ENDPOINT_STATES: usize = 65_536;

    for boundary in &assignment.boundaries {
        if boundary.is_empty() {
            return Some(false);
        }
        if boundary
            .iter()
            .any(|use_| edge_candidates.get(use_.edge).is_none_or(Vec::is_empty))
        {
            return None;
        }
        let candidates = |edge: usize| {
            edge_candidates[edge]
                .iter()
                .copied()
                .filter(move |pair| allowed(edge, *pair))
        };
        let mut states = HashSet::new();
        for [left, right] in candidates(boundary[0].edge) {
            states.insert((left, right));
            states.insert((right, left));
            if states.len() > MAX_LOCAL_ENDPOINT_STATES {
                return None;
            }
        }
        for use_ in &boundary[1..] {
            let mut next = HashSet::new();
            for &(start, current) in &states {
                for [left, right] in candidates(use_.edge) {
                    if left == current {
                        next.insert((start, right));
                    }
                    if right == current {
                        next.insert((start, left));
                    }
                    if next.len() > MAX_LOCAL_ENDPOINT_STATES {
                        return None;
                    }
                }
            }
            states = next;
            if states.is_empty() {
                return Some(false);
            }
        }
        if !states.into_iter().any(|(start, current)| start == current) {
            return Some(false);
        }
    }
    Some(true)
}

fn mesh_assignment_endpoint_cycles_viable_with(
    assignment: &MeshFaceBoundaryAssignment,
    edge_candidates: &[Vec<[usize; 2]>],
    required: Option<(usize, [usize; 2])>,
) -> Option<bool> {
    mesh_assignment_endpoint_cycles_viable_where(assignment, edge_candidates, |edge, pair| {
        required.is_none_or(|(required_edge, required_pair)| {
            edge != required_edge || same_unordered_pair(pair, required_pair)
        })
    })
}

fn mesh_assignment_endpoint_cycles_viable(
    assignment: &MeshFaceBoundaryAssignment,
    edge_candidates: &[Vec<[usize; 2]>],
) -> bool {
    mesh_assignment_endpoint_cycles_viable_with(assignment, edge_candidates, None).unwrap_or(true)
}

fn mesh_face_endpoint_configurations(
    assignments: &[MeshFaceBoundaryAssignment],
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[Option<[usize; 2]>],
    limit: usize,
) -> Option<MeshFaceEndpointConfigurations> {
    fn insert_pair(
        configuration: &mut MeshFaceEndpointConfiguration,
        edge: usize,
        mut pair: [usize; 2],
    ) -> bool {
        pair.sort_unstable();
        match configuration.iter().find(|(stored, _)| *stored == edge) {
            Some((_, stored)) => *stored == pair,
            None => {
                configuration.push((edge, pair));
                true
            }
        }
    }

    fn boundary_configurations(
        boundary: &[MeshBoundaryEdgeCandidate],
        edge_candidates: &[Vec<[usize; 2]>],
        selected: &[Option<[usize; 2]>],
        work: &mut usize,
        limit: usize,
    ) -> Option<MeshFaceEndpointConfigurations> {
        if boundary.is_empty()
            || boundary
                .iter()
                .any(|use_| edge_candidates.get(use_.edge).is_none_or(Vec::is_empty))
        {
            return None;
        }
        let allowed = |edge: usize, pair: [usize; 2]| {
            selected
                .get(edge)
                .copied()
                .flatten()
                .is_none_or(|stored| same_unordered_pair(stored, pair))
        };
        let mut states = Vec::<(usize, usize, MeshFaceEndpointConfiguration)>::new();
        for &pair @ [left, right] in &edge_candidates[boundary[0].edge] {
            if !allowed(boundary[0].edge, pair) {
                continue;
            }
            for (start, current) in [(left, right), (right, left)] {
                let mut configuration = Vec::new();
                if insert_pair(&mut configuration, boundary[0].edge, pair) {
                    states.push((start, current, configuration));
                }
            }
        }
        for use_ in &boundary[1..] {
            let mut next = Vec::new();
            for (start, current, configuration) in states {
                for &pair @ [left, right] in &edge_candidates[use_.edge] {
                    if !allowed(use_.edge, pair) {
                        continue;
                    }
                    for endpoint in [
                        (left == current).then_some(right),
                        (right == current).then_some(left),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        *work = work.checked_add(1)?;
                        if *work > limit {
                            return None;
                        }
                        let mut configuration = configuration.clone();
                        if insert_pair(&mut configuration, use_.edge, pair) {
                            next.push((start, endpoint, configuration));
                        }
                    }
                }
            }
            states = next;
            if states.is_empty() {
                return Some(Vec::new());
            }
        }
        let mut seen = HashSet::new();
        Some(
            states
                .into_iter()
                .filter(|(start, current, _)| start == current)
                .filter_map(|(_, _, mut configuration)| {
                    configuration.sort_unstable();
                    seen.insert(configuration.clone()).then_some(configuration)
                })
                .collect(),
        )
    }

    if limit == 0 || selected.len() != edge_candidates.len() {
        return None;
    }
    let mut work = 0usize;
    let mut configurations = HashSet::new();
    for assignment in assignments {
        let mut combined = vec![Vec::new()];
        for boundary in &assignment.boundaries {
            let boundary =
                boundary_configurations(boundary, edge_candidates, selected, &mut work, limit)?;
            let mut next = Vec::new();
            for stored in combined {
                for candidate in &boundary {
                    work = work.checked_add(1)?;
                    if work > limit {
                        return None;
                    }
                    let mut merged = stored.clone();
                    if candidate
                        .iter()
                        .all(|(edge, pair)| insert_pair(&mut merged, *edge, *pair))
                    {
                        merged.sort_unstable();
                        next.push(merged);
                    }
                }
            }
            combined = next;
        }
        configurations.extend(combined);
        if configurations.len() > limit {
            return None;
        }
    }
    let mut configurations = configurations.into_iter().collect::<Vec<_>>();
    configurations.sort_unstable();
    Some(configurations)
}

fn prune_mesh_endpoint_pair_support(
    assignments: &mut [Vec<MeshFaceBoundaryAssignment>],
    edge_candidates: &mut [Vec<[usize; 2]>],
) -> bool {
    'fixpoint: loop {
        let mut changed = false;
        for face in assignments.iter_mut() {
            let before = face.len();
            face.retain(|assignment| {
                mesh_assignment_endpoint_cycles_viable(assignment, edge_candidates)
            });
            if face.is_empty() {
                return false;
            }
            changed |= face.len() != before;
        }
        for edge in 0..edge_candidates.len() {
            if edge_candidates[edge].is_empty() {
                continue;
            }
            let incident_faces = assignments
                .iter()
                .enumerate()
                .filter_map(|(face, choices)| {
                    choices
                        .iter()
                        .any(|assignment| {
                            assignment
                                .boundaries
                                .iter()
                                .flatten()
                                .any(|use_| use_.edge == edge)
                        })
                        .then_some(face)
                })
                .collect::<Vec<_>>();
            let before = edge_candidates[edge].len();
            let snapshot = edge_candidates.to_vec();
            edge_candidates[edge].retain(|pair| {
                incident_faces.iter().all(|face| {
                    assignments[*face].iter().any(|assignment| {
                        assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .any(|use_| use_.edge == edge)
                            && mesh_assignment_endpoint_cycles_viable_with(
                                assignment,
                                &snapshot,
                                Some((edge, *pair)),
                            )
                            .unwrap_or(true)
                    })
                })
            });
            if edge_candidates[edge].is_empty() {
                return false;
            }
            if edge_candidates[edge].len() != before {
                continue 'fixpoint;
            }
        }
        if !changed {
            return true;
        }
    }
}

fn uses_canonical_edge_direction_gauge(
    boundaries: &[Vec<MeshBoundaryEdgeCandidate>],
    directions: &[Vec<bool>],
    oriented_edges: &HashSet<usize>,
) -> bool {
    let mut oriented = oriented_edges.clone();
    boundaries
        .iter()
        .zip(directions)
        .all(|(boundary, directions)| {
            boundary.iter().zip(directions).all(|(use_, direction)| {
                let first = oriented.insert(use_.edge);
                !first || use_.reversed.is_some() || !direction
            })
        })
}

impl MeshSelectionSearch<'_> {
    fn should_stop(&self) -> bool {
        self.ambiguous
            || self.exhausted
            || (self.stop_after_first_solution && self.solution.is_some())
    }

    fn remaining_equation_merge_capacity(&self, quotient: &mut MeshQuotient) -> Option<usize> {
        fn choice_component_reductions(
            choice: &[[usize; 2]],
            quotient: &mut MeshQuotient,
            possible: &mut UnionFind,
        ) -> HashMap<usize, usize> {
            let mut equations = HashMap::<usize, Vec<[usize; 2]>>::new();
            for [left, right] in choice {
                let left = quotient.union.find(*left);
                let right = quotient.union.find(*right);
                let component = possible.find(left);
                if component == possible.find(right) {
                    equations.entry(component).or_default().push([left, right]);
                }
            }
            equations
                .into_iter()
                .map(|(component, equations)| {
                    let mut roots = HashMap::new();
                    for [left, right] in &equations {
                        for root in [left, right] {
                            let next = roots.len();
                            roots.entry(*root).or_insert(next);
                        }
                    }
                    let mut local = UnionFind::new(roots.len());
                    for [left, right] in equations {
                        local.union(roots[&left], roots[&right]);
                    }
                    let remaining = (0..local.len())
                        .filter(|&node| local.find(node) == node)
                        .count();
                    (component, roots.len().saturating_sub(remaining))
                })
                .collect()
        }

        let node_count = quotient.union.len();
        let mut possible = UnionFind::new(node_count);
        for node in 0..node_count {
            let root = quotient.union.find(node);
            possible.union(node, root);
        }
        let before = (0..node_count)
            .filter(|&node| possible.find(node) == node)
            .count();
        for (face, selected) in self.selected.iter().enumerate() {
            if selected.is_some() {
                continue;
            }
            for [left, right] in &self.possible_face_equations[face] {
                possible.union(*left, *right);
            }
        }
        let after = (0..node_count)
            .filter(|&node| possible.find(node) == node)
            .count();
        let point_count = if self.vertex_points.is_empty() {
            quotient
                .domains
                .iter()
                .flat_map(|domain| domain.iter())
                .max()
                .map_or(0, |point| point + 1)
        } else {
            self.vertex_points.len()
        };
        let mut possible_domains = HashMap::<usize, HashSet<usize>>::new();
        let mut universal_components = HashSet::new();
        let mut possible_root_counts = HashMap::<usize, usize>::new();
        for node in 0..node_count {
            if quotient.union.find(node) != node {
                continue;
            }
            let component = possible.find(node);
            if quotient.domains[node].len() == point_count {
                universal_components.insert(component);
                possible_domains.remove(&component);
            } else if !universal_components.contains(&component) {
                possible_domains
                    .entry(component)
                    .or_default()
                    .extend(quotient.domains[node].iter());
            }
            *possible_root_counts.entry(component).or_default() += 1;
        }
        let mut component_merge_capacity = HashMap::<usize, usize>::new();
        let mut independent_capacity = 0usize;
        for (face, selected) in self.selected.iter().enumerate() {
            if selected.is_some() {
                continue;
            }
            let mut face_capacity = HashMap::<usize, usize>::new();
            let mut independent_face_capacity = 0usize;
            for choice in &self.possible_face_choices[face] {
                let reductions = choice_component_reductions(choice, quotient, &mut possible);
                independent_face_capacity = independent_face_capacity.max(
                    reductions
                        .values()
                        .copied()
                        .fold(0usize, usize::saturating_add),
                );
                for (component, reduction) in reductions {
                    face_capacity
                        .entry(component)
                        .and_modify(|capacity| *capacity = (*capacity).max(reduction))
                        .or_insert(reduction);
                }
            }
            independent_capacity = independent_capacity.saturating_add(independent_face_capacity);
            for (component, capacity) in face_capacity {
                *component_merge_capacity.entry(component).or_default() += capacity;
            }
        }
        let required_root_count = possible_root_counts
            .iter()
            .map(|(component, roots)| {
                roots
                    .saturating_sub(
                        component_merge_capacity
                            .get(component)
                            .copied()
                            .unwrap_or(0),
                    )
                    .max(1)
            })
            .sum::<usize>();
        if required_root_count > point_count {
            return None;
        }
        let required_count = |component: &usize| {
            possible_root_counts[component]
                .saturating_sub(
                    component_merge_capacity
                        .get(component)
                        .copied()
                        .unwrap_or(0),
                )
                .max(1)
        };
        let universal_required = universal_components
            .iter()
            .map(required_count)
            .fold(0usize, usize::saturating_add);
        let mut domains = possible_domains
            .into_iter()
            .flat_map(|(component, domain)| {
                let required = required_count(&component);
                std::iter::repeat_n(domain, required)
            })
            .collect::<Vec<_>>();
        if universal_required > point_count.saturating_sub(domains.len()) {
            return None;
        }
        domains.sort_unstable_by_key(HashSet::len);
        let domains = domains
            .into_iter()
            .map(|domain| domain.into_iter().collect::<Vec<_>>())
            .collect::<Vec<_>>();
        if !domains_have_distinct_matching(domains.iter().map(Vec::as_slice), point_count) {
            return None;
        }
        let mut singleton_component = HashMap::new();
        for node in 0..node_count {
            if quotient.union.find(node) != node || quotient.domains[node].len() != 1 {
                continue;
            }
            let point = *quotient.domains[node].iter().next()?;
            let component = possible.find(node);
            if singleton_component
                .insert(point, component)
                .is_some_and(|previous| previous != component)
            {
                return None;
            }
        }
        Some(before.saturating_sub(after).min(independent_capacity))
    }

    fn face_projection_signature(
        &self,
        face: usize,
        quotient: &mut MeshQuotient,
    ) -> MeshQuotientSignature {
        let mut roots = self.assignments[face]
            .iter()
            .flat_map(|assignment| &assignment.boundaries)
            .flatten()
            .flat_map(|use_| [use_.edge * 2, use_.edge * 2 + 1])
            .map(|node| quotient.union.find(node))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        roots.sort_unstable();
        let mut signature = roots
            .into_iter()
            .map(|root| {
                let mut domain = quotient.domains[root].iter().copied().collect::<Vec<_>>();
                domain.sort_unstable();
                (quotient.members[root].clone(), domain)
            })
            .collect::<Vec<_>>();
        signature.sort_unstable();
        signature
    }

    #[cfg(test)]
    fn propagate_forced_face_equations(&self, quotient: &mut MeshQuotient) -> bool {
        self.propagate_forced_face_equations_from(quotient, None)
    }

    fn propagate_forced_face_equations_from(
        &self,
        quotient: &mut MeshQuotient,
        changed_edges: Option<&HashSet<usize>>,
    ) -> bool {
        const MAX_FACE_EQUATION_OPTIONS: usize = 4_096;

        fn common_assignment_equations(
            quotient: &MeshQuotient,
            assignment: &MeshFaceBoundaryAssignment,
            edge_candidates: &[Vec<[usize; 2]>],
        ) -> Option<HashSet<[usize; 2]>> {
            fn port(use_: MeshBoundaryEdgeCandidate, reversed: bool, end: bool) -> Option<usize> {
                use_.edge.checked_mul(2)?.checked_add(usize::from(if end {
                    !reversed
                } else {
                    reversed
                }))
            }

            if assignment.boundaries.iter().any(Vec::is_empty) {
                return None;
            }
            let unknown = assignment
                .boundaries
                .iter()
                .flatten()
                .filter(|use_| use_.reversed.is_none())
                .count();
            let combinations = 1usize.checked_shl(unknown as u32)?;
            if combinations > MAX_FACE_EQUATION_OPTIONS {
                return None;
            }
            let mut before = quotient.clone();
            let mut nodes = assignment
                .boundaries
                .iter()
                .flatten()
                .flat_map(|use_| [use_.edge * 2, use_.edge * 2 + 1])
                .collect::<Vec<_>>();
            nodes.sort_unstable();
            nodes.dedup();
            let mut common = None::<HashSet<[usize; 2]>>;
            for mask in 0..combinations {
                let mut variable = 0usize;
                let directions = assignment
                    .boundaries
                    .iter()
                    .map(|boundary| {
                        boundary
                            .iter()
                            .map(|use_| {
                                use_.reversed.unwrap_or_else(|| {
                                    let shift = unknown - variable - 1;
                                    variable += 1;
                                    mask & (1usize << shift) != 0
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                let mut option = quotient.clone();
                let viable =
                    assignment
                        .boundaries
                        .iter()
                        .zip(&directions)
                        .all(|(boundary, directions)| {
                            (0..boundary.len()).all(|index| {
                                let next = (index + 1) % boundary.len();
                                let Some(left) = port(boundary[index], directions[index], true)
                                else {
                                    return false;
                                };
                                let Some(right) = port(boundary[next], directions[next], false)
                                else {
                                    return false;
                                };
                                let Some(root) = option.merge(left, right) else {
                                    return false;
                                };
                                option.propagate_component_edge_domains(root, edge_candidates)
                            })
                        });
                if !viable {
                    continue;
                }
                let mut option_equations = HashSet::new();
                for (at, left) in nodes.iter().enumerate() {
                    for right in &nodes[at + 1..] {
                        if before.union.find(*left) != before.union.find(*right)
                            && option.union.find(*left) == option.union.find(*right)
                        {
                            option_equations.insert([*left, *right]);
                        }
                    }
                }
                match &mut common {
                    Some(common) => {
                        common.retain(|equation| option_equations.contains(equation));
                    }
                    None => common = Some(option_equations),
                }
                if common.as_ref().is_some_and(HashSet::is_empty) {
                    break;
                }
            }
            common
        }

        fn unconditional_corner_equations(
            quotient: &mut MeshQuotient,
            assignments: &[MeshFaceBoundaryAssignment],
        ) -> Option<HashSet<[usize; 2]>> {
            fn ports(use_: MeshBoundaryEdgeCandidate, end: bool) -> Option<Vec<usize>> {
                let directions = use_
                    .reversed
                    .map_or_else(|| vec![false, true], |reversed| vec![reversed]);
                directions
                    .into_iter()
                    .map(|reversed| {
                        use_.edge.checked_mul(2)?.checked_add(usize::from(if end {
                            !reversed
                        } else {
                            reversed
                        }))
                    })
                    .collect()
            }

            let mut common = None::<HashSet<[usize; 2]>>;
            for assignment in assignments {
                let mut forced = HashSet::new();
                for boundary in &assignment.boundaries {
                    if boundary.is_empty() {
                        return None;
                    }
                    for index in 0..boundary.len() {
                        let left = ports(boundary[index], true)?;
                        let right = ports(boundary[(index + 1) % boundary.len()], false)?;
                        let mut alternatives = HashSet::new();
                        for left in &left {
                            for right in &right {
                                let left = quotient.union.find(*left);
                                let right = quotient.union.find(*right);
                                alternatives.insert(if left <= right {
                                    [left, right]
                                } else {
                                    [right, left]
                                });
                            }
                        }
                        if alternatives.len() == 1 {
                            if let Some(equation) = alternatives.into_iter().next() {
                                forced.insert(equation);
                            }
                        }
                    }
                }
                match &mut common {
                    Some(common) => common.retain(|equation| forced.contains(equation)),
                    None => common = Some(forced),
                }
            }
            common
        }

        let mut queue = self
            .selected
            .iter()
            .enumerate()
            .filter_map(|(face, selected)| {
                (selected.is_none()
                    && changed_edges.is_none_or(|changed_edges| {
                        self.assignments[face]
                            .iter()
                            .flat_map(|assignment| &assignment.boundaries)
                            .flatten()
                            .any(|use_| changed_edges.contains(&use_.edge))
                    }))
                .then_some(face)
            })
            .collect::<VecDeque<_>>();
        let mut queued = queue.iter().copied().collect::<HashSet<_>>();
        while let Some(face) = queue.pop_front() {
            queued.remove(&face);
            if self.selected[face].is_some() {
                continue;
            }
            let before = quotient.clone();
            let mut changed = false;
            let deterministic = self.assignments[face].len() == 1
                && self.assignments[face][0]
                    .boundaries
                    .iter()
                    .flatten()
                    .all(|use_| use_.reversed.is_some());
            let equations = if deterministic {
                let [choice] = self.possible_face_choices[face].as_slice() else {
                    return false;
                };
                choice.clone()
            } else {
                let cache_key = (face, self.face_projection_signature(face, quotient));
                let cached = self.face_equation_cache.borrow().get(&cache_key).cloned();
                if let Some(cached) = cached {
                    cached
                } else {
                    let structural_option_bound =
                        self.assignments[face]
                            .iter()
                            .try_fold(0usize, |total, assignment| {
                                let unknown = assignment
                                    .boundaries
                                    .iter()
                                    .flatten()
                                    .filter(|use_| use_.reversed.is_none())
                                    .count();
                                total.checked_add(1usize.checked_shl(unknown as u32)?)
                            });
                    let equations: Vec<[usize; 2]> = if structural_option_bound
                        .is_none_or(|bound| bound > MAX_FACE_EQUATION_OPTIONS)
                    {
                        let Some(common) =
                            unconditional_corner_equations(quotient, &self.assignments[face])
                        else {
                            return false;
                        };
                        common.into_iter().collect()
                    } else {
                        let mut common = None::<HashSet<[usize; 2]>>;
                        for assignment in &self.assignments[face] {
                            let Some(assignment_common) = common_assignment_equations(
                                quotient,
                                assignment,
                                self.edge_candidates,
                            ) else {
                                continue;
                            };
                            match &mut common {
                                Some(common) => {
                                    common.retain(|equation| assignment_common.contains(equation));
                                }
                                None => common = Some(assignment_common),
                            }
                        }
                        let Some(common) = common else {
                            return false;
                        };
                        common.into_iter().collect()
                    };
                    let mut cache = self.face_equation_cache.borrow_mut();
                    if cache.len() >= MAX_FACE_EQUATION_CACHE_ENTRIES {
                        cache.clear();
                    }
                    cache.insert(cache_key, equations.clone());
                    equations
                }
            };
            for [left, right] in equations {
                if quotient.union.find(left) == quotient.union.find(right) {
                    continue;
                }
                let Some(root) = quotient.merge(left, right) else {
                    return false;
                };
                if !quotient.propagate_component_edge_domains(root, self.edge_candidates) {
                    return false;
                }
                changed = true;
            }
            if !changed {
                continue;
            }
            let changed_edges = changed_quotient_edges(&before, quotient);
            for (dependent, assignments) in self.assignments.iter().enumerate() {
                if self.selected[dependent].is_none()
                    && dependent != face
                    && !queued.contains(&dependent)
                    && assignments
                        .iter()
                        .flat_map(|assignment| &assignment.boundaries)
                        .flatten()
                        .any(|use_| changed_edges.contains(&use_.edge))
                {
                    queued.insert(dependent);
                    queue.push_back(dependent);
                }
            }
        }
        true
    }

    fn selection_orientable(&self, selection: &[MeshFaceSelection]) -> bool {
        let mut constraints = Vec::<Vec<(usize, bool)>>::new();
        let mut edge_uses = HashMap::<usize, Vec<(usize, bool)>>::new();
        for (face, selected) in selection.iter().enumerate() {
            let Some((assignment_index, directions)) = selected else {
                continue;
            };
            let Some(assignment) = self.assignments[face].get(*assignment_index) else {
                return false;
            };
            if assignment.boundaries.len() != directions.len() {
                return false;
            }
            for (boundary, directions) in assignment.boundaries.iter().zip(directions) {
                if boundary.len() != directions.len() {
                    return false;
                }
                let node = constraints.len();
                constraints.push(Vec::new());
                for (use_, &direction) in boundary.iter().zip(directions) {
                    let reversed = use_.reversed.unwrap_or(direction);
                    if use_.reversed.is_some() && reversed != direction {
                        return false;
                    }
                    let uses = edge_uses.entry(use_.edge).or_default();
                    if uses.len() == 2 {
                        return false;
                    }
                    uses.push((node, reversed));
                }
            }
        }
        for uses in edge_uses.values() {
            let [(left_node, left_reversed), (right_node, right_reversed)] = uses.as_slice() else {
                continue;
            };
            let parity = left_reversed == right_reversed;
            if left_node == right_node {
                if parity {
                    return false;
                }
            } else {
                constraints[*left_node].push((*right_node, parity));
                constraints[*right_node].push((*left_node, parity));
            }
        }
        let mut flips = vec![None; constraints.len()];
        for root in 0..constraints.len() {
            if flips[root].is_some() {
                continue;
            }
            flips[root] = Some(false);
            let mut stack = vec![root];
            while let Some(node) = stack.pop() {
                let Some(flip) = flips[node] else {
                    return false;
                };
                for &(neighbor, parity) in &constraints[node] {
                    let required = flip ^ parity;
                    match flips[neighbor] {
                        Some(existing) if existing != required => return false,
                        Some(_) => {}
                        None => {
                            flips[neighbor] = Some(required);
                            stack.push(neighbor);
                        }
                    }
                }
            }
        }
        true
    }

    fn selected_orientable(&self) -> bool {
        self.selection_orientable(&self.selected)
    }

    fn fixed_remaining_faces_are_orientable(&self) -> bool {
        let mut completion = self.selected.clone();
        for (face, selected) in completion.iter_mut().enumerate() {
            if selected.is_some() {
                continue;
            }
            let [assignment] = self.assignments[face].as_slice() else {
                continue;
            };
            let Some(directions) = assignment
                .boundaries
                .iter()
                .map(|boundary| {
                    boundary
                        .iter()
                        .map(|use_| use_.reversed)
                        .collect::<Option<Vec<_>>>()
                })
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            *selected = Some((0, directions));
        }
        self.selection_orientable(&completion)
    }

    fn prepare_selected_branch(
        &self,
        quotient: &MeshQuotient,
        changed_edges: &HashSet<usize>,
    ) -> Option<MeshQuotient> {
        let mut measured = quotient.clone();
        if !self.propagate_forced_face_equations_from(&mut measured, Some(changed_edges)) {
            return None;
        }
        if !measured.merge_singleton_coordinate_roots(self.edge_candidates) {
            return None;
        }
        let root_count = measured.root_count();
        if root_count < self.vertex_points.len() {
            return None;
        }
        let remaining_merges = self.remaining_equation_merge_capacity(&mut measured)?;
        if root_count.saturating_sub(remaining_merges) > self.vertex_points.len() {
            measured.close_coordinate_roots(self.vertex_points.len(), self.edge_candidates)?;
        }
        if root_count == self.vertex_points.len()
            && !measured.point_assignment_exists(self.vertex_points.len(), self.edge_candidates)
        {
            return None;
        }
        self.fixed_remaining_faces_are_orientable()
            .then_some(measured)
    }

    fn search(&mut self, quotient: &MeshQuotient) {
        self.search_from(quotient, None);
    }

    fn search_from(&mut self, quotient: &MeshQuotient, changed_edges: Option<&HashSet<usize>>) {
        self.search_from_state(quotient, changed_edges, false);
    }

    fn search_from_state(
        &mut self,
        quotient: &MeshQuotient,
        changed_edges: Option<&HashSet<usize>>,
        prepared: bool,
    ) {
        const MAX_SELECTION_STATES: usize = 512;

        if self.should_stop() {
            return;
        }
        let mut measured = quotient.clone();
        if !prepared {
            if !self.propagate_forced_face_equations_from(&mut measured, changed_edges) {
                return;
            }
            if !measured.merge_singleton_coordinate_roots(self.edge_candidates) {
                return;
            }
            let root_count = measured.root_count();
            if root_count < self.vertex_points.len() {
                return;
            }
            let Some(remaining_merges) = self.remaining_equation_merge_capacity(&mut measured)
            else {
                return;
            };
            if root_count.saturating_sub(remaining_merges) > self.vertex_points.len()
                && measured
                    .close_coordinate_roots(self.vertex_points.len(), self.edge_candidates)
                    .is_none()
            {
                return;
            }
            if root_count == self.vertex_points.len()
                && !measured.point_assignment_exists(self.vertex_points.len(), self.edge_candidates)
            {
                return;
            }
            if !self.fixed_remaining_faces_are_orientable() {
                return;
            }
        }
        let selected_edges = self
            .selected
            .iter()
            .enumerate()
            .filter_map(|(face, selected)| {
                selected
                    .as_ref()
                    .and_then(|(index, _)| self.assignments[face].get(*index))
            })
            .flat_map(|assignment| &assignment.boundaries)
            .flatten()
            .map(|use_| use_.edge)
            .collect::<HashSet<_>>();
        let next = self
            .selected
            .iter()
            .enumerate()
            .filter(|(_, selected)| selected.is_none())
            .filter_map(|(face, _)| {
                self.face_work[face]?;
                let assignments = &self.assignments[face];
                if assignments.is_empty() {
                    return Some((0, 0, 0, 0, 0, face));
                }
                let direction_work = assignments
                    .iter()
                    .map(|assignment| {
                        let unknown = assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .filter(|use_| use_.reversed.is_none())
                            .count();
                        1usize.checked_shl(unknown as u32).unwrap_or(usize::MAX)
                    })
                    .fold(0usize, usize::saturating_add);
                let can_merge = assignments
                    .iter()
                    .any(|assignment| mesh_assignment_can_merge(assignment, &mut measured));
                let assignment = &assignments[0];
                let selected_incidence = assignment
                    .boundaries
                    .iter()
                    .flatten()
                    .filter(|use_| selected_edges.contains(&use_.edge))
                    .count();
                let constrained = assignment
                    .boundaries
                    .iter()
                    .flatten()
                    .filter(|use_| {
                        let left = measured.union.find(use_.edge * 2);
                        let right = measured.union.find(use_.edge * 2 + 1);
                        measured.domains[left].len() < self.vertex_points.len()
                            || measured.domains[right].len() < self.vertex_points.len()
                    })
                    .count();
                Some((
                    if can_merge { 1 } else { 2 },
                    assignments.len(),
                    direction_work,
                    usize::MAX - selected_incidence,
                    usize::MAX - constrained,
                    face,
                ))
            })
            .min();
        let Some((_, supported, _, _, _, face)) = next else {
            let mut quotient = measured.clone();
            let Some(root_points) =
                quotient.close_coordinate_roots(self.vertex_points.len(), self.edge_candidates)
            else {
                return;
            };
            let selected = self.selected.iter().cloned().collect::<Option<Vec<_>>>();
            let Some(selected) = selected else {
                return;
            };
            let assignment_indices = selected.iter().map(|(index, _)| *index).collect::<Vec<_>>();
            let directions = selected
                .into_iter()
                .map(|(_, directions)| directions)
                .collect::<Vec<_>>();
            let selected_assignments = self
                .assignments
                .iter()
                .zip(&assignment_indices)
                .map(|(assignments, &index)| assignments.get(index).cloned())
                .collect::<Option<Vec<_>>>();
            let Some(selected_assignments) = selected_assignments else {
                return;
            };
            let candidate = reconstruct_mesh_selection(
                self.edge_rows.to_vec(),
                self.vertex_points.to_vec(),
                &selected_assignments,
                &directions,
            )
            .and_then(|mut topology| {
                let mut use_counts = vec![0usize; topology.edge_rows.len()];
                for coedge in topology
                    .faces
                    .iter()
                    .flat_map(|face| &face.boundaries)
                    .flat_map(|boundary| &boundary.coedges)
                {
                    use_counts[coedge.edge_row] += 1;
                }
                if use_counts.iter().any(|count| *count > 2) {
                    return None;
                }
                if use_counts.iter().all(|count| *count == 2) {
                    orient_face_cycles(&mut topology.faces)?;
                }
                let edge_vertices = topology.edge_vertices()?;
                let mut point_assignment = vec![None; topology.logical_vertex_count];
                for (edge, vertices) in edge_vertices.into_iter().enumerate() {
                    for (port, vertex) in vertices.into_iter().enumerate() {
                        let root = quotient.union.find(edge * 2 + port);
                        let point = *root_points.get(&root)?;
                        match point_assignment[vertex] {
                            Some(stored) if stored != point => return None,
                            Some(_) => {}
                            None => point_assignment[vertex] = Some(point),
                        }
                    }
                    let points = <[usize; 2]>::try_from(
                        vertices
                            .map(|vertex| point_assignment[vertex])
                            .into_iter()
                            .collect::<Option<Vec<_>>>()?,
                    )
                    .ok()?;
                    let closed_ports =
                        quotient.union.find(edge * 2) == quotient.union.find(edge * 2 + 1);
                    if !mesh_edge_points_compatible(
                        closed_ports,
                        &self.edge_candidates[edge],
                        points,
                    ) {
                        return None;
                    }
                }
                Some((
                    topology,
                    point_assignment.into_iter().collect::<Option<Vec<_>>>()?,
                ))
            });
            if let Some(candidate) = candidate {
                match &self.solution {
                    Some(solution)
                        if *solution != candidate
                            && !mesh_candidates_equivalent(solution, &candidate) =>
                    {
                        self.ambiguous = true;
                    }
                    None => self.solution = Some(candidate),
                    Some(_) => {}
                }
            }
            return;
        };
        if supported == 0 {
            return;
        }
        let mut options = Vec::new();
        for assignment_index in 0..self.assignments[face].len() {
            let remaining = MAX_SELECTION_STATES
                .saturating_sub(self.states)
                .saturating_add(1)
                .saturating_sub(options.len());
            if remaining == 0 {
                break;
            }
            let assignment = &self.assignments[face][assignment_index];
            options.extend(
                measured
                    .assignment_options_limited(
                        assignment,
                        self.edge_candidates,
                        &selected_edges,
                        remaining,
                    )
                    .into_iter()
                    .map(|(directions, next_quotient)| {
                        (assignment_index, directions, next_quotient)
                    }),
            );
        }
        options.retain_mut(|(_, _, quotient)| quotient.root_count() >= self.vertex_points.len());
        if options.is_empty() {
            return;
        }
        options.sort_unstable_by_key(|(assignment, directions, quotient)| {
            let mut measured = quotient.clone();
            let root_count = measured.root_count();
            let domain_freedom = (0..measured.union.len())
                .filter(|&node| measured.union.find(node) == node)
                .map(|node| measured.domains[node].len())
                .fold(0usize, usize::saturating_add);
            (root_count, domain_freedom, *assignment, directions.clone())
        });
        let branching = options.len() > 1;
        for (assignment_index, directions, next_quotient) in options {
            let changed_edges = changed_quotient_edges(&measured, &next_quotient);
            self.selected[face] = Some((assignment_index, directions));
            if self.selected_orientable() {
                if let Some(next_quotient) =
                    self.prepare_selected_branch(&next_quotient, &changed_edges)
                {
                    if branching {
                        if self.states >= MAX_SELECTION_STATES {
                            self.exhausted = true;
                            self.selected[face] = None;
                            return;
                        }
                        self.states += 1;
                    }
                    // `prepare_selected_branch` has already applied the recursive
                    // entry preflight to this quotient.
                    self.search_from_state(&next_quotient, None, true);
                }
            }
            self.selected[face] = None;
            if self.should_stop() {
                return;
            }
        }
    }
}

fn canonicalize_mesh_vertex_labels(
    mut topology: StandardTopology,
    point_assignment: &[usize],
) -> Option<(StandardTopology, Vec<usize>)> {
    if point_assignment.len() != topology.logical_vertex_count
        || point_assignment.len() != topology.vertex_points.len()
    {
        return None;
    }
    let mut seen = vec![false; point_assignment.len()];
    for &point in point_assignment {
        let entry = seen.get_mut(point)?;
        if std::mem::replace(entry, true) {
            return None;
        }
    }
    for coedge in topology
        .faces
        .iter_mut()
        .flat_map(|face| &mut face.boundaries)
        .flat_map(|boundary| &mut boundary.coedges)
    {
        coedge.start_vertex = *point_assignment.get(coedge.start_vertex)?;
        coedge.end_vertex = *point_assignment.get(coedge.end_vertex)?;
    }
    let mut edge_vertices = vec![None; topology.edge_rows.len()];
    for coedge in topology
        .faces
        .iter()
        .flat_map(|face| &face.boundaries)
        .flat_map(|boundary| &boundary.coedges)
    {
        let vertices = if coedge.reversed {
            [coedge.end_vertex, coedge.start_vertex]
        } else {
            [coedge.start_vertex, coedge.end_vertex]
        };
        let stored = edge_vertices.get_mut(coedge.edge_row)?;
        match stored {
            Some(existing) if *existing != vertices => return None,
            Some(_) => {}
            None => *stored = Some(vertices),
        }
    }
    let reverse_edges = edge_vertices
        .into_iter()
        .map(|vertices| vertices.is_some_and(|vertices| vertices[0] > vertices[1]))
        .collect::<Vec<_>>();
    for coedge in topology
        .faces
        .iter_mut()
        .flat_map(|face| &mut face.boundaries)
        .flat_map(|boundary| &mut boundary.coedges)
    {
        if *reverse_edges.get(coedge.edge_row)? {
            coedge.reversed = !coedge.reversed;
        }
    }
    for boundary in topology
        .faces
        .iter_mut()
        .flat_map(|face| &mut face.boundaries)
    {
        let len = boundary.coedges.len();
        if len == 0 {
            return None;
        }
        let best = (0..len).min_by_key(|&start| {
            (0..len)
                .map(|offset| {
                    let coedge = boundary.coedges[(start + offset) % len];
                    (
                        coedge.edge_row,
                        coedge.reversed,
                        coedge.start_vertex,
                        coedge.end_vertex,
                    )
                })
                .collect::<Vec<_>>()
        })?;
        boundary.coedges.rotate_left(best);
    }
    Some((topology, (0..point_assignment.len()).collect::<Vec<_>>()))
}

fn mesh_candidates_equivalent(
    left: &(StandardTopology, Vec<usize>),
    right: &(StandardTopology, Vec<usize>),
) -> bool {
    canonicalize_mesh_vertex_labels(left.0.clone(), &left.1)
        == canonicalize_mesh_vertex_labels(right.0.clone(), &right.1)
}

fn mesh_assignment_can_merge(
    assignment: &MeshFaceBoundaryAssignment,
    quotient: &mut MeshQuotient,
) -> bool {
    fn possible_ports(use_: MeshBoundaryEdgeCandidate, end: bool) -> [Option<usize>; 2] {
        let port = |reversed: bool| {
            use_.edge
                .checked_mul(2)?
                .checked_add(usize::from(reversed != end))
        };
        match use_.reversed {
            Some(reversed) => [port(reversed), None],
            None => [port(false), port(true)],
        }
    }

    assignment.boundaries.iter().any(|boundary| {
        (0..boundary.len()).any(|index| {
            let left = possible_ports(boundary[index], true);
            let right = possible_ports(boundary[(index + 1) % boundary.len()], false);
            left.into_iter().flatten().any(|left| {
                right
                    .into_iter()
                    .flatten()
                    .any(|right| quotient.union.find(left) != quotient.union.find(right))
            })
        })
    })
}

fn mesh_edge_points_compatible(
    closed_ports: bool,
    candidates: &[[usize; 2]],
    points: [usize; 2],
) -> bool {
    (points[0] != points[1] || closed_ports)
        && (candidates.is_empty()
            || candidates
                .iter()
                .any(|candidate| same_unordered_pair(*candidate, points)))
}

/// Resolve standard trim assignments through their abstract physical-port
/// quotient before binding the quotient bijectively to coordinate rows.
#[must_use]
pub fn parse_standard_mesh_endpoint_candidates(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
) -> Option<(StandardTopology, Vec<usize>)> {
    const MAX_SELECTION_WORK: usize = 100_000;

    let (_, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, vertex_header) = parse_edge_tables(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    if edge_rows.len() != edge_faces.len() || edge_rows.len() != edge_candidates.len() {
        return None;
    }
    let mut assignments =
        standard_mesh_boundary_assignments_impl(bytes, edge_faces, Some(edge_candidates))?;
    if assignments.len() != face_count {
        return None;
    }
    deduplicate_mesh_quotient_assignments(&mut assignments);
    let mut edge_candidates = edge_candidates.to_vec();
    if !prune_mesh_endpoint_pair_support(&mut assignments, &mut edge_candidates) {
        return None;
    }
    // Unconstrained ports share the immutable universal domain. Quotient
    // intersections allocate only when a constraint narrows a component.
    let all_points = Arc::new((0..vertex_points.len()).collect::<HashSet<_>>());
    let mut domains = Vec::with_capacity(edge_rows.len() * 2);
    for candidates in &edge_candidates {
        let domain = if candidates.is_empty() {
            all_points.clone()
        } else {
            Arc::new(candidates.iter().flatten().copied().collect::<HashSet<_>>())
        };
        if domain.is_empty() || domain.iter().any(|point| *point >= vertex_points.len()) {
            return None;
        }
        domains.push(domain.clone());
        domains.push(domain);
    }
    let mut quotient = MeshQuotient {
        union: UnionFind::new(edge_rows.len() * 2),
        domains,
        members: (0..edge_rows.len() * 2).map(|node| vec![node]).collect(),
    };
    let port_identities = standard_edge_port_identities(bytes)?;
    if port_identities.len() != edge_rows.len() {
        return None;
    }
    let mut node_by_identity = HashMap::new();
    for (edge, ports) in port_identities.into_iter().enumerate() {
        for (port, identity) in ports.into_iter().enumerate() {
            let node = edge * 2 + port;
            if let Some(&previous) = node_by_identity.get(&identity) {
                quotient.merge(previous, node)?;
            } else {
                node_by_identity.insert(identity, node);
            }
        }
    }
    if !quotient.edge_domains_viable(&edge_candidates) {
        return None;
    }
    for face in &mut assignments {
        face.retain(|assignment| quotient.assignment_has_option(assignment, &edge_candidates));
        if face.is_empty() {
            return None;
        }
    }
    let face_work = assignments
        .iter()
        .map(|assignments| Some(assignments.len()))
        .collect::<Vec<_>>();
    let total_work = face_work
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .try_fold(0usize, usize::checked_add)?;
    if total_work > MAX_SELECTION_WORK {
        return None;
    }
    let mut search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work,
        edge_candidates: &edge_candidates,
        edge_rows: &edge_rows,
        vertex_points: &vertex_points,
        selected: vec![None; face_count],
        states: 0,
        solution: None,
        stop_after_first_solution: edge_candidates.iter().all(|pairs| pairs.len() == 1),
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    search.search(&quotient);
    (!search.ambiguous && !search.exhausted)
        .then_some(search.solution)
        .flatten()
}

/// Resolve geometric endpoint alternatives through face incidence before
/// applying the exact trim-mesh endpoint quotient. Endpoint graphs must close
/// every face; all surviving graphs must produce one topology modulo logical
/// vertex labels, intrinsic edge direction, and boundary-cycle start.
#[must_use]
pub fn parse_standard_mesh_incidence_candidates<F>(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    pair_solution_valid: F,
) -> Option<(StandardTopology, Vec<usize>)>
where
    F: Fn(&[[usize; 2]]) -> bool,
{
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
    let mut mesh_assignments =
        standard_mesh_boundary_assignments_impl(bytes, edge_faces, Some(edge_candidates))?;
    if mesh_assignments.len() != face_count {
        return None;
    }
    deduplicate_mesh_quotient_assignments(&mut mesh_assignments);
    let mut pair_domains = edge_candidates.to_vec();
    if !prune_mesh_endpoint_pair_support(&mut mesh_assignments, &mut pair_domains) {
        return None;
    }
    let complete_solution_valid = |pairs: &[[usize; 2]]| {
        if !pair_solution_valid(pairs) {
            return false;
        }
        let singleton = pairs
            .iter()
            .copied()
            .map(|pair| vec![pair])
            .collect::<Vec<_>>();
        parse_standard_mesh_endpoint_candidates(bytes, edge_faces, &singleton).is_some()
    };
    let pair_solutions = incidence_endpoint_pair_solutions(
        &edge_rows,
        &vertex_points,
        edge_faces,
        &pair_domains,
        face_count,
        Some(&mesh_assignments),
        &complete_solution_valid,
    )?;
    let mut solution = None;
    for pairs in pair_solutions {
        let singleton = pairs.into_iter().map(|pair| vec![pair]).collect::<Vec<_>>();
        let Some(candidate) =
            parse_standard_mesh_endpoint_candidates(bytes, edge_faces, &singleton)
        else {
            continue;
        };
        match &solution {
            Some(stored) if !mesh_candidates_equivalent(stored, &candidate) => return None,
            None => solution = Some(candidate),
            Some(_) => {}
        }
    }
    solution
}

fn parse_fbb_edge_tables(
    bytes: &[u8],
    position: usize,
) -> Option<(Vec<EdgeRow>, Vec<usize>, usize, usize)> {
    [3, 2, 1]
        .into_iter()
        .find_map(|handle_width| parse_fbb_edge_tables_width(bytes, position, handle_width))
}

fn parse_fbb_edge_tables_width(
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

fn largest_fbb_run(bytes: &[u8]) -> Option<(usize, usize, usize)> {
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

fn parse_edge_tables(bytes: &[u8], position: usize) -> Option<(Vec<EdgeRow>, usize)> {
    if let Some(result) = parse_edge_tables_at(bytes, position) {
        return Some(result);
    }
    parse_fbb_edge_tables(bytes, position)
        .filter(|(_, _, vertex_header, _)| parse_vertex_table(bytes, *vertex_header).is_some())
        .map(|(rows, _, vertex_header, _)| (rows, vertex_header))
}

fn parse_edge_tables_at(bytes: &[u8], position: usize) -> Option<(Vec<EdgeRow>, usize)> {
    parse_edge_tables_scoped_at(bytes, position)
        .map(|(rows, _, vertex_header)| (rows, vertex_header))
}

fn parse_edge_tables_scoped_at(
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

fn parse_trim_chain(
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
            .cloned();
        let Some((start, record)) = predecessor else {
            backtrack(&mut frames, &mut reversed);
            continue;
        };
        frames[frame].next_predecessor += 1;
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
    let mut handles = Vec::with_capacity(stored_count);
    for _ in 0..stored_count {
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
        independent_count: a,
        strip_lengths: lengths[..b].to_vec(),
        fan_lengths: lengths[b..].to_vec(),
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

#[derive(Debug, Clone)]
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
    use std::{
        cell::RefCell,
        collections::{HashMap, HashSet},
        sync::Arc,
    };

    use cadmpeg_ir::topology::BodyKind;

    use super::{
        bind_edge_port_candidates, canonicalize_mesh_vertex_labels, complete_duplicate_face_slots,
        deduplicate_mesh_quotient_assignments, face_endpoint_candidates_close,
        mesh_assignment_can_merge, mesh_assignment_endpoint_cycles_viable,
        mesh_candidates_equivalent, mesh_edge_points_compatible, mesh_face_endpoint_configurations,
        motif_port_points, parse_edge_tables_at, parse_edge_tables_scoped_at,
        parse_fbb_edge_tables_width, parse_trim_chain, parse_trim_record, possible_face_choices,
        possible_face_equations, propagate_edge_port_points, propagate_partial_edge_port_points,
        prune_edge_candidates_by_port_domains, prune_mesh_endpoint_pair_support,
        reconstruct_incidence, reconstruct_incidence_candidates, resolve_edge_faces_from_runs,
        same_unordered_pair, standard_face_count, unique_coordinate_bijection,
        unique_duplicate_face_assignment, uses_canonical_edge_direction_gauge, Boundary, CoedgeUse,
        EdgeBoundaryLayout, EdgeRow, FaceTopology, MeshBoundaryEdgeCandidate, MeshEdgeRun,
        MeshFaceBoundaryAssignment, MeshQuotient, MeshSelectionSearch, StandardTopology,
        TrimRecord, UnionFind, EDGE_DELIMITER, MAX_FACE_EQUATION_CACHE_ENTRIES,
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
        assert_eq!(records[0].independent_count, 1);
        assert!(records[0].strip_lengths.is_empty());
        assert!(records[0].fan_lengths.is_empty());
        assert!(parse_trim_chain(&bytes, bytes.len(), 2, 3).is_none());
    }

    #[test]
    fn forced_trim_chain_has_no_recursive_depth_limit() {
        const RECORD_COUNT: usize = 10_000;
        let packet = triangle_packet([0, 0, 0]);
        let bytes = packet.repeat(RECORD_COUNT);

        let records = parse_trim_chain(&bytes, bytes.len(), RECORD_COUNT, 2)
            .expect("forced trim packet chain");

        assert_eq!(records.len(), RECORD_COUNT);
        assert!(records.iter().all(|record| record.handles == [0, 0, 0]));
    }

    #[test]
    fn trim_packet_retains_primitive_partition_lengths() {
        let mut bytes = vec![
            0x01, 0x47, 0x01, 0x01, 0x01, 0xff, 0x0a, 0x00, 0x00, 0x00, 0x03, 0x04,
        ];
        for handle in 0u16..10 {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
        let [record] = parse_trim_chain(&bytes, bytes.len(), 1, 2)
            .expect("mixed primitive packet")
            .try_into()
            .expect("one packet");
        assert_eq!(record.independent_count, 1);
        assert_eq!(record.strip_lengths, [3]);
        assert_eq!(record.fan_lengths, [4]);
    }

    #[test]
    fn standard_edge_row_arity_uses_widened_count_form() {
        let mut bytes = Vec::new();
        for (kind, handles) in [(1, [10u16, 11]), (2, [20, 21])] {
            bytes.extend_from_slice(&[0x01, kind, 1, 0x02, 0xff]);
            bytes.extend_from_slice(&2u32.to_le_bytes());
            for handle in handles {
                bytes.extend_from_slice(&handle.to_be_bytes());
            }
            bytes.extend_from_slice(&EDGE_DELIMITER);
        }
        bytes.extend_from_slice(&[0x01, 0x06, 0]);

        let (rows, vertex_header) = parse_edge_tables_at(&bytes, 0).expect("widened row arity");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].handles, vec![10, 11]);
        assert_eq!(rows[1].handles, vec![20, 21]);
        assert_eq!(vertex_header, bytes.len() - 3);
    }

    #[test]
    fn coordinate_rows_canonicalize_logical_vertex_labels() {
        let topology = |start_vertex, end_vertex| StandardTopology {
            faces: vec![FaceTopology {
                boundaries: vec![Boundary {
                    coedges: vec![CoedgeUse {
                        edge_row: 0,
                        reversed: false,
                        start_vertex,
                        end_vertex,
                    }],
                }],
            }],
            edge_rows: vec![EdgeRow {
                kind: 1,
                handles: vec![0, 1],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            }],
            vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            logical_vertex_count: 2,
        };

        let left_candidate = (topology(0, 1), vec![1, 0]);
        let right_candidate = (topology(1, 0), vec![0, 1]);
        assert_ne!(left_candidate, right_candidate);
        assert!(mesh_candidates_equivalent(
            &left_candidate,
            &right_candidate
        ));
        let left = canonicalize_mesh_vertex_labels(left_candidate.0, &left_candidate.1);
        let right = canonicalize_mesh_vertex_labels(right_candidate.0, &right_candidate.1);

        assert_eq!(left, right);
        assert_eq!(left.expect("canonical topology").1, vec![0, 1]);

        let forward = canonicalize_mesh_vertex_labels(topology(0, 1), &[0, 1]);
        let mut reversed = topology(0, 1);
        reversed.faces[0].boundaries[0].coedges[0].reversed = true;
        let reversed = canonicalize_mesh_vertex_labels(reversed, &[0, 1]);
        assert_eq!(forward, reversed);
    }

    #[test]
    fn mesh_candidate_comparison_ignores_boundary_cycle_start() {
        let mut topology = StandardTopology {
            faces: vec![FaceTopology {
                boundaries: vec![Boundary {
                    coedges: vec![
                        CoedgeUse {
                            edge_row: 0,
                            reversed: false,
                            start_vertex: 0,
                            end_vertex: 1,
                        },
                        CoedgeUse {
                            edge_row: 1,
                            reversed: false,
                            start_vertex: 1,
                            end_vertex: 0,
                        },
                    ],
                }],
            }],
            edge_rows: vec![
                EdgeRow {
                    kind: 1,
                    handles: vec![0, 1],
                    boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
                },
                EdgeRow {
                    kind: 1,
                    handles: vec![1, 0],
                    boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
                },
            ],
            vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            logical_vertex_count: 2,
        };
        let left = (topology.clone(), vec![0, 1]);
        topology.faces[0].boundaries[0].coedges.rotate_left(1);
        let right = (topology, vec![0, 1]);

        assert_ne!(left, right);
        assert!(mesh_candidates_equivalent(&left, &right));
    }

    #[test]
    fn standard_face_population_ignores_shorter_fbb_marker_runs() {
        let row = [0x30, 0x04, 0x04, 0xff, 0xff, 0xff, 0xd2, 0xd2];
        let mut bytes = row.to_vec();
        bytes.push(0);
        bytes.extend_from_slice(&row);
        bytes.extend_from_slice(&row);
        bytes.extend_from_slice(&row);

        assert_eq!(standard_face_count(&bytes), Some(3));
    }

    #[test]
    fn standard_face_population_rejects_equal_largest_fbb_runs() {
        let row = [0x30, 0x04, 0x04, 0xff, 0xff, 0xff, 0xd2, 0xd2];
        let mut bytes = row.repeat(2);
        bytes.push(0);
        bytes.extend_from_slice(&row.repeat(2));

        assert_eq!(standard_face_count(&bytes), None);
    }

    fn trim(kind: u8, handles: [u32; 4]) -> TrimRecord {
        TrimRecord {
            triangles: Vec::new(),
            frame_vector: None,
            handles: handles.to_vec(),
            independent_count: 0,
            strip_lengths: vec![handles.len()],
            fan_lengths: Vec::new(),
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
            reconstruct_incidence_candidates(&rows, &points, &edge_faces, &candidates, None, 4)
                .expect("unique face-closing endpoint assignment");
        assert_eq!(topology.edge_vertices().expect("edge vertices")[0], [0, 1]);

        let ports = [[11, 10], [11, 12], [10, 12], [13, 10], [11, 13], [13, 12]];
        let topology = reconstruct_incidence_candidates(
            &rows,
            &points,
            &edge_faces,
            &candidates,
            Some(&ports),
            4,
        )
        .expect("unique face-closing assignment with deferred port orientation");
        assert_eq!(topology.edge_vertices().expect("edge vertices")[0], [1, 0]);
    }

    #[test]
    fn incidence_propagation_closes_degree_one_vertices_before_search() {
        let mut choices = vec![vec![[0, 1]], vec![[1, 2], [3, 4]], vec![[2, 0]]];
        let edge_faces = [[0, 0], [0, 0], [0, 0]];
        super::prune_incidence_choices(&mut choices, &edge_faces, 1, 5)
            .expect("face incidence is satisfiable");
        assert_eq!(choices, vec![vec![[0, 1]], vec![[1, 2]], vec![[2, 0]]]);
    }

    #[test]
    fn incidence_component_does_not_charge_a_forced_viable_pair() {
        let choices = vec![vec![[0, 0], [1, 1]]];
        let edge_faces = [[0, 0]];
        let face_edges = vec![vec![0]];
        let edges = [0];
        let mut search = super::IncidenceComponentSearch {
            choices: &choices,
            edge_faces: &edge_faces,
            face_edges: &face_edges,
            mesh_assignments: None,
            active: vec![true],
            edges: &edges,
            constraints: vec![(0, 0), (0, 1)],
            assignment: vec![None],
            degrees: vec![vec![0, 2]],
            solutions: Vec::new(),
            solution_filter: None,
            dead_states: HashSet::new(),
            states: 4_096,
            exhausted: false,
        };

        search.search();

        assert!(!search.exhausted);
        assert_eq!(search.states, 4_096);
        assert_eq!(search.solutions, vec![vec![(0, [0, 0])]]);
    }

    #[test]
    fn incidence_components_join_only_through_shared_face_vertices() {
        let choices = vec![
            vec![[0, 1], [0, 2]],
            vec![[1, 3], [2, 3]],
            vec![[4, 5], [4, 6]],
            vec![[7, 8]],
        ];
        let edge_faces = [[0, 0], [0, 0], [0, 0], [0, 0]];
        assert_eq!(
            super::incidence_choice_components(&choices, &edge_faces),
            vec![vec![0, 1], vec![2]]
        );
    }

    #[test]
    fn incidence_components_solve_coupled_face_vertex_closures() {
        let a = vec![[0, 2], [0, 12], [2, 12]];
        let b = vec![[1, 3], [1, 1969], [3, 1969]];
        let c = vec![
            [0, 1],
            [0, 2],
            [0, 3],
            [0, 12],
            [0, 1969],
            [1, 2],
            [1, 3],
            [1, 12],
            [1, 1969],
            [2, 3],
            [2, 12],
            [2, 1969],
            [3, 12],
            [3, 1969],
            [12, 1969],
        ];
        let choices = vec![
            a.clone(),
            b.clone(),
            a,
            b,
            c,
            vec![[2, 3]],
            vec![[2, 12]],
            vec![[12, 1969]],
            vec![[3, 1969]],
        ];
        let edge_faces = [
            [1, 0],
            [3, 0],
            [2, 1],
            [2, 3],
            [2, 0],
            [0, 0],
            [1, 1],
            [2, 2],
            [3, 3],
        ];
        let solutions = super::component_incidence_pair_solutions(
            &choices,
            &edge_faces,
            4,
            1970,
            None,
            &|_| true,
        )
        .expect("component closure solution");
        assert!(solutions
            .iter()
            .any(|solution| { solution[..5] == [[0, 2], [1, 3], [0, 12], [1, 1969], [0, 1]] }));
    }

    #[test]
    fn incidence_components_reject_degree_cycles_in_the_wrong_edge_order() {
        let choices = vec![
            vec![[0, 1]],
            vec![[1, 2], [2, 3]],
            vec![[2, 3], [1, 2]],
            vec![[3, 0]],
        ];
        let edge_faces = [[0, 0]; 4];
        let mesh_assignments = vec![vec![MeshFaceBoundaryAssignment {
            boundaries: vec![(0..4)
                .map(|edge| MeshBoundaryEdgeCandidate {
                    edge,
                    start: 0,
                    end: 0,
                    reversed: None,
                })
                .collect()],
        }]];

        let solutions = super::component_incidence_pair_solutions(
            &choices,
            &edge_faces,
            1,
            4,
            Some(&mesh_assignments),
            &|_| true,
        )
        .expect("ordered component solution");

        assert_eq!(solutions.len(), 1);
        assert_eq!(solutions[0], [[0, 1], [1, 2], [2, 3], [3, 0]]);
    }

    #[test]
    fn incidence_components_filter_complete_solutions_during_search() {
        let choices = vec![
            vec![[0, 1]],
            vec![[1, 2], [2, 3]],
            vec![[2, 3], [1, 2]],
            vec![[3, 0]],
        ];
        let edge_faces = [[0, 0]; 4];
        let solutions = super::component_incidence_pair_solutions(
            &choices,
            &edge_faces,
            1,
            4,
            None,
            &|pairs| pairs[1] == [2, 3],
        )
        .expect("filtered component solution");

        assert_eq!(solutions, vec![vec![[0, 1], [2, 3], [1, 2], [3, 0]]]);
    }

    #[test]
    fn quotient_assignments_ignore_span_allocation_with_identical_edge_order() {
        let use_ = |edge, start, end| MeshBoundaryEdgeCandidate {
            edge,
            start,
            end,
            reversed: None,
        };
        let mut faces = vec![vec![
            MeshFaceBoundaryAssignment {
                boundaries: vec![vec![
                    use_(0, 0, 1),
                    use_(1, 1, 2),
                    use_(2, 2, 3),
                    use_(3, 3, 4),
                ]],
            },
            MeshFaceBoundaryAssignment {
                boundaries: vec![vec![
                    use_(0, 0, 2),
                    use_(1, 2, 3),
                    use_(2, 3, 4),
                    use_(3, 4, 5),
                ]],
            },
            MeshFaceBoundaryAssignment {
                boundaries: vec![vec![
                    use_(3, 0, 1),
                    use_(2, 1, 2),
                    use_(1, 2, 3),
                    use_(0, 3, 4),
                ]],
            },
            MeshFaceBoundaryAssignment {
                boundaries: vec![vec![
                    use_(0, 0, 1),
                    use_(2, 1, 2),
                    use_(1, 2, 3),
                    use_(3, 3, 4),
                ]],
            },
        ]];
        deduplicate_mesh_quotient_assignments(&mut faces);
        assert_eq!(faces[0].len(), 2);
        assert_eq!(faces[0][0].boundaries[0][0].edge, 0);
        assert_eq!(faces[0][1].boundaries[0][1].edge, 2);
    }

    #[test]
    fn mesh_direction_search_fixes_each_new_edge_gauge_once() {
        let assignment = MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 1,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 1,
                    end: 2,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 2,
                    end: 3,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 2,
                    start: 3,
                    end: 0,
                    reversed: Some(true),
                },
            ]],
        };
        let already_oriented = HashSet::from([1]);

        assert!(uses_canonical_edge_direction_gauge(
            &assignment.boundaries,
            &[vec![false, true, true, true]],
            &already_oriented,
        ));
        assert!(!uses_canonical_edge_direction_gauge(
            &assignment.boundaries,
            &[vec![true, false, false, true]],
            &already_oriented,
        ));
    }

    #[test]
    fn quotient_merge_preserves_physical_edge_pair_correlation() {
        let mut quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: [vec![0], vec![0, 1], vec![0], vec![2]]
                .map(|domain| Arc::new(domain.into_iter().collect()))
                .into(),
            members: (0..4).map(|node| vec![node]).collect(),
        };
        quotient.merge(1, 2).expect("nonempty port intersection");
        assert!(!quotient.edge_domains_viable(&[vec![[0, 1]], vec![[0, 2]]]));
    }

    #[test]
    fn quotient_clones_share_unconstrained_point_domains() {
        let all = Arc::new((0..1_000).collect::<HashSet<_>>());
        let quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: vec![all.clone(), all.clone(), all.clone(), all.clone()],
            members: (0..4).map(|node| vec![node]).collect(),
        };

        let clone = quotient.clone();
        assert!(Arc::ptr_eq(&quotient.domains[0], &clone.domains[0]));
        assert!(Arc::ptr_eq(&quotient.domains[0], &quotient.domains[3]));
    }

    #[test]
    fn quotient_pair_domains_propagate_through_shared_components() {
        let mut quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: [vec![0, 1], vec![2], vec![0, 1], vec![3, 4]]
                .map(|domain| Arc::new(domain.into_iter().collect()))
                .into(),
            members: (0..4).map(|node| vec![node]).collect(),
        };
        let root = quotient.merge(0, 2).expect("shared endpoint component");

        assert!(quotient.edge_domains_viable(&[vec![[0, 2]], vec![[0, 3], [1, 4]],]));
        assert_eq!(*quotient.domains[root], HashSet::from([0]));
        assert_eq!(
            *quotient.domains[quotient.union.find(3)],
            HashSet::from([3])
        );
    }

    #[test]
    fn quotient_assignment_requires_one_consistent_closed_orientation() {
        let mut quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: [vec![0], vec![1], vec![2], vec![3]]
                .map(|domain| Arc::new(domain.into_iter().collect()))
                .into(),
            members: (0..4).map(|node| vec![node]).collect(),
        };
        let assignment = MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 1,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 1,
                    end: 2,
                    reversed: None,
                },
            ]],
        };
        assert!(!quotient.assignment_has_option(&assignment, &[vec![], vec![]]));
        Arc::make_mut(&mut quotient.domains[2]).insert(1);
        assert!(!quotient.assignment_has_option(&assignment, &[vec![], vec![]]));
        Arc::make_mut(&mut quotient.domains[3]).insert(0);
        assert!(quotient.assignment_has_option(&assignment, &[vec![], vec![]]));
    }

    #[test]
    fn fixed_boundary_option_has_no_recursive_depth_limit() {
        const EDGE_COUNT: usize = 10_000;
        let quotient = MeshQuotient {
            union: UnionFind::new(EDGE_COUNT * 2),
            domains: vec![Arc::new(HashSet::from([0])); EDGE_COUNT * 2],
            members: (0..EDGE_COUNT * 2).map(|node| vec![node]).collect(),
        };
        let assignment = MeshFaceBoundaryAssignment {
            boundaries: vec![(0..EDGE_COUNT)
                .map(|edge| MeshBoundaryEdgeCandidate {
                    edge,
                    start: edge,
                    end: (edge + 1) % EDGE_COUNT,
                    reversed: Some(false),
                })
                .collect()],
        };
        let candidates = vec![vec![[0, 0]]; EDGE_COUNT];

        assert!(quotient.assignment_has_option(&assignment, &candidates));
    }

    #[test]
    fn quotient_options_reject_an_interior_pair_contradiction() {
        let quotient = MeshQuotient {
            union: UnionFind::new(6),
            domains: [vec![0], vec![1, 2], vec![2], vec![3], vec![0, 3], vec![0]]
                .map(|domain| Arc::new(domain.into_iter().collect()))
                .into(),
            members: (0..6).map(|node| vec![node]).collect(),
        };
        let assignment = MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 1,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 1,
                    end: 2,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 2,
                    start: 2,
                    end: 3,
                    reversed: None,
                },
            ]],
        };
        let candidates = [vec![[0, 1]], vec![[2, 3]], vec![[0, 3]]];

        let options = quotient.assignment_options(&assignment, &candidates);

        assert!(!options
            .iter()
            .any(|(directions, _)| directions == &[vec![false, false, false]]));
        let unrestricted = [Vec::new(), Vec::new(), Vec::new()];
        let options = quotient.assignment_options(&assignment, &unrestricted);
        let limited =
            quotient.assignment_options_limited(&assignment, &unrestricted, &HashSet::new(), 1);
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].0, options[0].0);
        let unique =
            quotient.assignment_options_limited(&assignment, &unrestricted, &HashSet::new(), 4_096);
        assert!(unique
            .iter()
            .all(|option| options.iter().any(|candidate| candidate.0 == option.0)));
    }

    #[test]
    fn quotient_point_assignment_preserves_endpoint_pair_relations() {
        let quotient = || MeshQuotient {
            union: UnionFind::new(4),
            domains: [vec![0, 1], vec![2], vec![0, 1], vec![3]]
                .map(|domain| Arc::new(domain.into_iter().collect()))
                .into(),
            members: (0..4).map(|node| vec![node]).collect(),
        };
        assert!(quotient().point_assignment(4, &[vec![], vec![]]).is_none());
        assert!(quotient().point_assignment_exists(4, &[vec![], vec![]]));

        let assignment = quotient()
            .point_assignment(4, &[vec![[0, 2]], vec![[1, 3]]])
            .expect("edge-pair relations determine the coordinate bijection");
        assert_eq!(assignment[&0], 0);
        assert_eq!(assignment[&1], 2);
        assert_eq!(assignment[&2], 1);
        assert_eq!(assignment[&3], 3);
    }

    #[test]
    fn point_assignment_handles_deep_augmenting_paths_iteratively() {
        const ROOT_COUNT: usize = 10_000;
        let mut domains = (0..ROOT_COUNT - 1)
            .map(|root| Arc::new(HashSet::from([root, root + 1])))
            .collect::<Vec<_>>();
        domains.push(Arc::new(HashSet::from([0])));
        let mut quotient = MeshQuotient {
            union: UnionFind::new(ROOT_COUNT),
            domains,
            members: (0..ROOT_COUNT).map(|node| vec![node]).collect(),
        };

        let assignment = quotient
            .point_assignment(ROOT_COUNT, &[])
            .expect("forced coordinate bijection");

        assert_eq!(assignment.len(), ROOT_COUNT);
        assert_eq!(assignment[&(ROOT_COUNT - 1)], 0);
        assert!((0..ROOT_COUNT - 1).all(|root| assignment[&root] == root + 1));
    }

    #[test]
    fn quotient_point_existence_rejects_an_all_different_conflict() {
        let mut quotient = MeshQuotient {
            union: UnionFind::new(2),
            domains: vec![Arc::new(HashSet::from([0])), Arc::new(HashSet::from([0]))],
            members: vec![vec![0], vec![1]],
        };

        assert!(!quotient.point_assignment_exists(2, &[vec![]]));
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
        assert_eq!(topology.body_kinds(&[9]), Some(vec![BodyKind::Solid]));
        assert_eq!(topology.body_kinds(&[4, 5]), None);
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
    fn open_standard_edge_incidence_classifies_a_sheet_body() {
        let mut topology = StandardTopology {
            faces: vec![FaceTopology {
                boundaries: vec![Boundary {
                    coedges: vec![CoedgeUse {
                        edge_row: 0,
                        reversed: false,
                        start_vertex: 0,
                        end_vertex: 1,
                    }],
                }],
            }],
            edge_rows: vec![
                EdgeRow {
                    kind: 1,
                    handles: vec![0, 1],
                    boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
                },
                EdgeRow {
                    kind: 1,
                    handles: vec![2, 3],
                    boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
                },
            ],
            vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            logical_vertex_count: 2,
        };

        assert_eq!(topology.body_kinds(&[1]), None);
        topology.edge_rows.pop();
        assert_eq!(topology.body_kinds(&[1]), Some(vec![BodyKind::Sheet]));
    }

    #[test]
    fn solid_body_cycles_orient_independently_from_an_open_sheet_body() {
        let use_ = |edge_row| CoedgeUse {
            edge_row,
            reversed: false,
            start_vertex: edge_row,
            end_vertex: 1 - edge_row,
        };
        let mut topology = StandardTopology {
            faces: vec![
                FaceTopology {
                    boundaries: vec![Boundary {
                        coedges: vec![use_(0), use_(1)],
                    }],
                },
                FaceTopology {
                    boundaries: vec![Boundary {
                        coedges: vec![use_(0), use_(1)],
                    }],
                },
                FaceTopology {
                    boundaries: vec![Boundary {
                        coedges: vec![CoedgeUse {
                            edge_row: 2,
                            reversed: false,
                            start_vertex: 0,
                            end_vertex: 1,
                        }],
                    }],
                },
            ],
            edge_rows: (0..3)
                .map(|edge| EdgeRow {
                    kind: 1,
                    handles: vec![edge * 2, edge * 2 + 1],
                    boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
                })
                .collect(),
            vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            logical_vertex_count: 2,
        };

        assert_eq!(
            topology.body_kinds(&[2, 1]),
            Some(vec![BodyKind::Solid, BodyKind::Sheet])
        );
        assert_eq!(topology.face_components(), vec![vec![0, 1], vec![2]]);
        topology
            .orient_solid_body_cycles(&[2, 1])
            .expect("closed group orientation");

        for edge in 0..2 {
            assert_ne!(
                topology.faces[0].boundaries[0].coedges[edge].reversed,
                topology.faces[1].boundaries[0].coedges[1 - edge].reversed,
            );
        }
        assert!(!topology.faces[2].boundaries[0].coedges[0].reversed);
    }

    #[test]
    fn mesh_selection_rejects_an_odd_boundary_orientation_cycle() {
        let use_ = |edge| MeshBoundaryEdgeCandidate {
            edge,
            start: 0,
            end: 1,
            reversed: None,
        };
        let assignments = vec![
            vec![MeshFaceBoundaryAssignment {
                boundaries: vec![vec![use_(0), use_(2)]],
            }],
            vec![MeshFaceBoundaryAssignment {
                boundaries: vec![vec![use_(0), use_(1)]],
            }],
            vec![MeshFaceBoundaryAssignment {
                boundaries: vec![vec![use_(1), use_(2)]],
            }],
        ];
        let mut search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: possible_face_equations(&assignments),
            possible_face_choices: possible_face_choices(
                &assignments,
                &possible_face_equations(&assignments),
            ),
            face_work: vec![Some(1); 3],
            edge_candidates: &[],
            edge_rows: &[],
            vertex_points: &[],
            selected: vec![
                Some((0, vec![vec![false, false]])),
                Some((0, vec![vec![false, false]])),
                Some((0, vec![vec![false, false]])),
            ],
            states: 0,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };

        assert!(!search.selected_orientable());
        search.selected[2] = Some((0, vec![vec![false, true]]));
        assert!(search.selected_orientable());
    }

    #[test]
    fn mesh_selection_rejects_a_branch_with_no_orientable_remaining_face() {
        let use_ = |edge, reversed| MeshBoundaryEdgeCandidate {
            edge,
            start: 0,
            end: 1,
            reversed,
        };
        let assignments = vec![
            vec![MeshFaceBoundaryAssignment {
                boundaries: vec![vec![use_(0, None), use_(2, None)]],
            }],
            vec![MeshFaceBoundaryAssignment {
                boundaries: vec![vec![use_(0, None), use_(1, None)]],
            }],
            vec![MeshFaceBoundaryAssignment {
                boundaries: vec![vec![use_(1, Some(false)), use_(2, Some(false))]],
            }],
        ];
        let edge_candidates = vec![Vec::new(); 3];
        let mut search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: possible_face_equations(&assignments),
            possible_face_choices: possible_face_choices(
                &assignments,
                &possible_face_equations(&assignments),
            ),
            face_work: vec![Some(1); 3],
            edge_candidates: &edge_candidates,
            edge_rows: &[],
            vertex_points: &[],
            selected: vec![
                Some((0, vec![vec![false, false]])),
                Some((0, vec![vec![false, false]])),
                None,
            ],
            states: 0,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };
        assert!(!search.fixed_remaining_faces_are_orientable());
        search.selected[1] = Some((0, vec![vec![false, true]]));
        assert!(search.fixed_remaining_faces_are_orientable());
    }

    #[test]
    fn mesh_selection_checks_all_fixed_remaining_faces_together() {
        let use_ = |edge| MeshBoundaryEdgeCandidate {
            edge,
            start: 0,
            end: 1,
            reversed: Some(false),
        };
        let assignments = vec![
            vec![MeshFaceBoundaryAssignment {
                boundaries: vec![vec![use_(2), use_(0)]],
            }],
            vec![MeshFaceBoundaryAssignment {
                boundaries: vec![vec![use_(0), use_(1)]],
            }],
            vec![MeshFaceBoundaryAssignment {
                boundaries: vec![vec![use_(1), use_(2)]],
            }],
        ];
        let edge_candidates = vec![Vec::new(); 3];
        let search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: possible_face_equations(&assignments),
            possible_face_choices: possible_face_choices(
                &assignments,
                &possible_face_equations(&assignments),
            ),
            face_work: vec![Some(1); 3],
            edge_candidates: &edge_candidates,
            edge_rows: &[],
            vertex_points: &[],
            selected: vec![Some((0, vec![vec![false, false]])), None, None],
            states: 0,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };

        assert!(!search.fixed_remaining_faces_are_orientable());
    }

    #[test]
    fn mesh_assignment_distinguishes_quotient_work_from_direction_only_work() {
        let assignment = MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 1,
                    reversed: Some(false),
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 1,
                    end: 0,
                    reversed: Some(false),
                },
            ]],
        };
        let mut quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: vec![Arc::new(HashSet::from([0, 1])); 4],
            members: (0..4).map(|node| vec![node]).collect(),
        };

        assert!(mesh_assignment_can_merge(&assignment, &mut quotient));
        quotient.merge(1, 2).expect("first boundary corner");
        quotient.merge(3, 0).expect("second boundary corner");
        assert!(!mesh_assignment_can_merge(&assignment, &mut quotient));
    }

    #[test]
    fn remaining_merge_capacity_counts_distinct_quotient_equations() {
        let assignment = MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 1,
                    reversed: Some(false),
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 1,
                    end: 0,
                    reversed: Some(false),
                },
            ]],
        };
        let assignments = vec![vec![assignment.clone()], vec![assignment]];
        let edge_candidates = vec![Vec::new(); 2];
        let search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: possible_face_equations(&assignments),
            possible_face_choices: possible_face_choices(
                &assignments,
                &possible_face_equations(&assignments),
            ),
            face_work: vec![Some(1); 2],
            edge_candidates: &edge_candidates,
            edge_rows: &[],
            vertex_points: &[],
            selected: vec![None; 2],
            states: 0,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };
        let mut quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: vec![Arc::new(HashSet::from([0, 1])); 4],
            members: (0..4).map(|node| vec![node]).collect(),
        };

        assert_eq!(
            search.remaining_equation_merge_capacity(&mut quotient),
            Some(2)
        );
        quotient.merge(1, 2).expect("first repeated equation");
        assert_eq!(
            search.remaining_equation_merge_capacity(&mut quotient),
            Some(1)
        );
    }

    #[test]
    fn remaining_merge_capacity_respects_mutually_exclusive_orientations() {
        let assignment = MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 1,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 1,
                    end: 0,
                    reversed: None,
                },
            ]],
        };
        let assignments = vec![vec![assignment]];
        let equations = possible_face_equations(&assignments);
        let edge_candidates = vec![Vec::new(); 2];
        let search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_choices: possible_face_choices(&assignments, &equations),
            possible_face_equations: equations,
            face_work: vec![Some(1)],
            edge_candidates: &edge_candidates,
            edge_rows: &[],
            vertex_points: &[],
            selected: vec![None],
            states: 0,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };
        let mut quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: vec![Arc::new(HashSet::from([0, 1])); 4],
            members: (0..4).map(|node| vec![node]).collect(),
        };

        assert_eq!(
            search.remaining_equation_merge_capacity(&mut quotient),
            Some(2)
        );
    }

    #[test]
    fn remaining_equations_must_connect_equal_singleton_domains() {
        let assignments = vec![vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 0,
                reversed: Some(false),
            }]],
        }]];
        let edge_candidates = vec![Vec::new(); 2];
        let search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: possible_face_equations(&assignments),
            possible_face_choices: possible_face_choices(
                &assignments,
                &possible_face_equations(&assignments),
            ),
            face_work: vec![Some(1)],
            edge_candidates: &edge_candidates,
            edge_rows: &[],
            vertex_points: &[],
            selected: vec![None],
            states: 0,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };
        let mut quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: vec![
                Arc::new(HashSet::from([0])),
                Arc::new(HashSet::from([1])),
                Arc::new(HashSet::from([0])),
                Arc::new(HashSet::from([2])),
            ],
            members: (0..4).map(|node| vec![node]).collect(),
        };

        assert_eq!(
            search.remaining_equation_merge_capacity(&mut quotient),
            None
        );
    }

    #[test]
    fn remaining_equation_components_require_a_coordinate_matching() {
        let assignments = Vec::new();
        let edge_candidates = vec![Vec::new(); 2];
        let search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: Vec::new(),
            possible_face_choices: Vec::new(),
            face_work: Vec::new(),
            edge_candidates: &edge_candidates,
            edge_rows: &[],
            vertex_points: &[],
            selected: Vec::new(),
            states: 0,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };
        let mut quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: vec![
                Arc::new(HashSet::from([0, 1])),
                Arc::new(HashSet::from([0, 1])),
                Arc::new(HashSet::from([0, 1])),
                Arc::new(HashSet::from([2, 3])),
            ],
            members: (0..4).map(|node| vec![node]).collect(),
        };

        assert_eq!(
            search.remaining_equation_merge_capacity(&mut quotient),
            None
        );
    }

    #[test]
    fn coordinate_matching_reserves_unavoidable_roots_per_component() {
        let assignments = vec![Vec::new()];
        let edge_candidates = vec![Vec::new(); 2];
        let search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: vec![vec![[0, 1], [1, 2]]],
            possible_face_choices: vec![vec![vec![[0, 1]], vec![[1, 2]]]],
            face_work: vec![Some(1)],
            edge_candidates: &edge_candidates,
            edge_rows: &[],
            vertex_points: &[[0.0, 0.0, 0.0]; 3],
            selected: vec![None],
            states: 0,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };
        let mut quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: vec![
                Arc::new(HashSet::from([0])),
                Arc::new(HashSet::from([0])),
                Arc::new(HashSet::from([0])),
                Arc::new(HashSet::from([1, 2])),
            ],
            members: (0..4).map(|node| vec![node]).collect(),
        };

        assert_eq!(
            search.remaining_equation_merge_capacity(&mut quotient),
            None
        );
    }

    #[test]
    fn singleton_mesh_search_stops_after_its_first_complete_solution() {
        let assignments = Vec::new();
        let edge_candidates = Vec::new();
        let edge_rows = Vec::new();
        let vertex_points = Vec::new();
        let search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: Vec::new(),
            possible_face_choices: Vec::new(),
            face_work: Vec::new(),
            edge_candidates: &edge_candidates,
            edge_rows: &edge_rows,
            vertex_points: &vertex_points,
            selected: Vec::new(),
            states: 512,
            solution: Some((
                StandardTopology {
                    faces: Vec::new(),
                    edge_rows: Vec::new(),
                    vertex_points: Vec::new(),
                    logical_vertex_count: 0,
                },
                Vec::new(),
            )),
            stop_after_first_solution: true,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };

        assert!(search.should_stop());
    }

    #[test]
    fn forced_face_selection_does_not_consume_the_branch_budget() {
        let assignments = vec![vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 0,
                reversed: Some(false),
            }]],
        }]];
        let edge_candidates = vec![vec![[0, 0]]];
        let edge_rows = vec![EdgeRow {
            kind: 1,
            handles: vec![0],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        }];
        let mut search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: possible_face_equations(&assignments),
            possible_face_choices: possible_face_choices(
                &assignments,
                &possible_face_equations(&assignments),
            ),
            face_work: vec![Some(1)],
            edge_candidates: &edge_candidates,
            edge_rows: &edge_rows,
            vertex_points: &[[0.0, 0.0, 0.0]],
            selected: vec![None],
            states: 512,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };
        let quotient = MeshQuotient {
            union: UnionFind::new(2),
            domains: vec![Arc::new(HashSet::from([0])); 2],
            members: (0..2).map(|node| vec![node]).collect(),
        };

        search.search(&quotient);

        assert!(!search.exhausted);
        assert_eq!(search.states, 512);
    }

    #[test]
    fn overmerged_face_options_do_not_consume_the_branch_budget() {
        let assignments = vec![vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 1,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 1,
                    end: 0,
                    reversed: None,
                },
            ]],
        }]];
        let edge_candidates = vec![Vec::new(); 2];
        let edge_rows = vec![
            EdgeRow {
                kind: 1,
                handles: vec![0],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            };
            2
        ];
        let mut search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: possible_face_equations(&assignments),
            possible_face_choices: possible_face_choices(
                &assignments,
                &possible_face_equations(&assignments),
            ),
            face_work: vec![Some(1)],
            edge_candidates: &edge_candidates,
            edge_rows: &edge_rows,
            vertex_points: &[[0.0, 0.0, 0.0]; 3],
            selected: vec![None],
            states: 512,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };
        let quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: vec![Arc::new(HashSet::from([0, 1, 2])); 4],
            members: (0..4).map(|node| vec![node]).collect(),
        };

        search.search(&quotient);

        assert!(!search.exhausted);
        assert_eq!(search.states, 512);
        assert!(search.solution.is_none());
    }

    #[test]
    fn mesh_selection_merges_corner_equations_common_to_every_option() {
        let assignment = MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 1,
                    reversed: Some(false),
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 1,
                    end: 2,
                    reversed: Some(false),
                },
                MeshBoundaryEdgeCandidate {
                    edge: 2,
                    start: 2,
                    end: 3,
                    reversed: Some(false),
                },
            ]],
        };
        let assignments = vec![vec![assignment]];
        let candidates = vec![vec![], vec![], vec![]];
        let search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: possible_face_equations(&assignments),
            possible_face_choices: possible_face_choices(
                &assignments,
                &possible_face_equations(&assignments),
            ),
            face_work: vec![Some(1)],
            edge_candidates: &candidates,
            edge_rows: &[],
            vertex_points: &[],
            selected: vec![None],
            states: 0,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };
        let mut quotient = MeshQuotient {
            union: UnionFind::new(6),
            domains: (0..6).map(|_| Arc::new(HashSet::from([0, 1, 2]))).collect(),
            members: (0..6).map(|node| vec![node]).collect(),
        };

        assert!(search.propagate_forced_face_equations(&mut quotient));
        assert_eq!(quotient.union.find(1), quotient.union.find(2));
        assert_eq!(quotient.union.find(3), quotient.union.find(4));
        assert_eq!(quotient.union.find(5), quotient.union.find(0));
        assert_eq!(quotient.root_count(), 3);
    }

    #[test]
    fn mesh_selection_merges_equations_common_to_every_assignment() {
        let use_ = |edge, reversed| MeshBoundaryEdgeCandidate {
            edge,
            start: edge,
            end: edge + 1,
            reversed: Some(reversed),
        };
        let assignments = vec![vec![
            MeshFaceBoundaryAssignment {
                boundaries: vec![vec![use_(0, false), use_(1, false), use_(2, false)]],
            },
            MeshFaceBoundaryAssignment {
                boundaries: vec![vec![use_(0, false), use_(1, false), use_(2, true)]],
            },
        ]];
        let candidates = vec![vec![], vec![], vec![]];
        let search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: possible_face_equations(&assignments),
            possible_face_choices: possible_face_choices(
                &assignments,
                &possible_face_equations(&assignments),
            ),
            face_work: vec![Some(2)],
            edge_candidates: &candidates,
            edge_rows: &[],
            vertex_points: &[],
            selected: vec![None],
            states: 0,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };
        let mut quotient = MeshQuotient {
            union: UnionFind::new(6),
            domains: (0..6).map(|_| Arc::new(HashSet::from([0, 1, 2]))).collect(),
            members: (0..6).map(|node| vec![node]).collect(),
        };

        assert!(search.propagate_forced_face_equations(&mut quotient));
        assert_eq!(quotient.union.find(1), quotient.union.find(2));
        assert_eq!(quotient.root_count(), 5);
    }

    #[test]
    fn mesh_selection_common_equations_ignore_infeasible_assignments() {
        let use_ = |edge| MeshBoundaryEdgeCandidate {
            edge,
            start: edge,
            end: edge + 1,
            reversed: Some(false),
        };
        let assignments = vec![vec![
            MeshFaceBoundaryAssignment {
                boundaries: vec![vec![use_(0), use_(1)]],
            },
            MeshFaceBoundaryAssignment {
                boundaries: vec![vec![use_(0), use_(2)]],
            },
        ]];
        let candidates = vec![vec![]; 3];
        let search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: possible_face_equations(&assignments),
            possible_face_choices: possible_face_choices(
                &assignments,
                &possible_face_equations(&assignments),
            ),
            face_work: vec![Some(2)],
            edge_candidates: &candidates,
            edge_rows: &[],
            vertex_points: &[],
            selected: vec![None],
            states: 0,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };
        let mut quotient = MeshQuotient {
            union: UnionFind::new(6),
            domains: [1, 0, 0, 1, 2, 2]
                .into_iter()
                .map(|point| Arc::new(HashSet::from([point])))
                .collect(),
            members: (0..6).map(|node| vec![node]).collect(),
        };

        assert!(search.propagate_forced_face_equations(&mut quotient));
        assert_eq!(quotient.union.find(1), quotient.union.find(2));
        assert_eq!(quotient.union.find(3), quotient.union.find(0));
        assert_eq!(quotient.root_count(), 4);
    }

    #[test]
    fn mesh_selection_propagates_closed_ports_without_enumerating_directions() {
        let boundary = (0..13)
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: edge,
                end: (edge + 1) % 13,
                reversed: None,
            })
            .collect();
        let assignments = vec![vec![MeshFaceBoundaryAssignment {
            boundaries: vec![boundary],
        }]];
        let candidates = vec![vec![]; 13];
        let search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: possible_face_equations(&assignments),
            possible_face_choices: possible_face_choices(
                &assignments,
                &possible_face_equations(&assignments),
            ),
            face_work: vec![Some(1)],
            edge_candidates: &candidates,
            edge_rows: &[],
            vertex_points: &[],
            selected: vec![None],
            states: 0,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };
        let mut quotient = MeshQuotient {
            union: UnionFind::new(26),
            domains: (0..26).map(|_| Arc::new((0..13).collect())).collect(),
            members: (0..26).map(|node| vec![node]).collect(),
        };
        for edge in 0..13 {
            quotient.merge(edge * 2, edge * 2 + 1).expect("closed port");
        }

        assert_eq!(quotient.root_count(), 13);
        assert!(search.propagate_forced_face_equations(&mut quotient));
        assert_eq!(quotient.root_count(), 1);
    }

    #[test]
    fn face_equation_cache_ignores_unrelated_quotient_components() {
        let assignments = vec![vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 1,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 1,
                    end: 0,
                    reversed: None,
                },
            ]],
        }]];
        let candidates = vec![vec![]; 3];
        let search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: possible_face_equations(&assignments),
            possible_face_choices: possible_face_choices(
                &assignments,
                &possible_face_equations(&assignments),
            ),
            face_work: vec![Some(1)],
            edge_candidates: &candidates,
            edge_rows: &[],
            vertex_points: &[],
            selected: vec![None],
            states: 0,
            solution: None,
            stop_after_first_solution: false,
            ambiguous: false,
            exhausted: false,
            face_equation_cache: RefCell::default(),
        };
        let mut quotient = MeshQuotient {
            union: UnionFind::new(6),
            domains: (0..6).map(|_| Arc::new(HashSet::from([0, 1, 2]))).collect(),
            members: (0..6).map(|node| vec![node]).collect(),
        };

        assert!(search.propagate_forced_face_equations(&mut quotient));
        assert_eq!(search.face_equation_cache.borrow().len(), 1);
        quotient.merge(4, 5).expect("unrelated component merge");
        assert!(search.propagate_forced_face_equations(&mut quotient));
        assert_eq!(search.face_equation_cache.borrow().len(), 1);
        quotient
            .merge(0, 4)
            .expect("component joined to a face port");
        assert!(search.propagate_forced_face_equations(&mut quotient));
        assert_eq!(search.face_equation_cache.borrow().len(), 2);
        {
            let mut cache = search.face_equation_cache.borrow_mut();
            for key in 1..=MAX_FACE_EQUATION_CACHE_ENTRIES {
                cache.insert((key, Vec::new()), Vec::new());
            }
        }
        quotient.merge(1, 2).expect("new face-component merge");
        assert!(search.propagate_forced_face_equations(&mut quotient));
        assert_eq!(search.face_equation_cache.borrow().len(), 1);
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
    fn partial_endpoint_ports_propagate_known_components_only() {
        let ports = [
            Some([10, 11]),
            Some([11, 12]),
            None,
            Some([12, 13]),
            Some([13, 10]),
        ];
        let pairs = [Some([0, 1]), Some([1, 2]), Some([8, 9]), None, Some([3, 0])];

        assert_eq!(
            propagate_partial_edge_port_points(&ports, &pairs),
            Some(vec![
                Some([0, 1]),
                Some([1, 2]),
                Some([8, 9]),
                Some([2, 3]),
                Some([3, 0]),
            ])
        );
    }

    #[test]
    fn endpoint_port_propagation_requires_a_point_bijection() {
        assert_eq!(
            propagate_edge_port_points(&[[10, 11]], &[Some([0, 1])]),
            Some(vec![Some([0, 1])])
        );
        assert_eq!(
            propagate_edge_port_points(&[[10, 11], [10, 12]], &[Some([0, 1]), Some([0, 1])]),
            None
        );
        assert_eq!(
            propagate_edge_port_points(&[[10, 11]], &[Some([0, 0])]),
            None
        );
    }

    #[test]
    fn endpoint_port_propagation_closes_equal_port_edges() {
        let ports = [[10, 11], [11, 12], [10, 10]];
        let pairs = [Some([0, 1]), Some([1, 2]), None];

        assert_eq!(
            propagate_edge_port_points(&ports, &pairs),
            Some(vec![Some([0, 1]), Some([1, 2]), Some([0, 0])])
        );
    }

    #[test]
    fn equal_endpoint_ports_produce_closed_edge_candidates() {
        let ports = [[10, 10], [10, 11]];
        let candidates = [vec![[0, 0], [1, 1], [2, 2]], vec![[1, 3], [2, 4]]];
        assert_eq!(
            prune_edge_candidates_by_port_domains(&ports, &candidates),
            Some(vec![vec![[1, 1], [2, 2]], vec![[1, 3], [2, 4]]])
        );
        assert_eq!(
            prune_edge_candidates_by_port_domains(&[[10, 10]], &[vec![[0, 1], [0, 2]]]),
            None
        );
    }

    #[test]
    fn endpoint_port_domains_propagate_pair_correlation_to_a_fixpoint() {
        let ports = [[10, 11], [11, 12], [12, 13]];
        let candidates = [vec![[0, 1], [2, 3]], vec![[1, 4], [3, 5]], vec![[4, 6]]];

        assert_eq!(
            prune_edge_candidates_by_port_domains(&ports, &candidates),
            Some(vec![vec![[0, 1]], vec![[1, 4]], vec![[4, 6]]])
        );
    }

    #[test]
    fn mesh_endpoint_validation_accepts_equal_points_only_for_closed_ports() {
        assert!(mesh_edge_points_compatible(true, &[[2, 2]], [2, 2]));
        assert!(!mesh_edge_points_compatible(false, &[[2, 2]], [2, 2]));
        assert!(!mesh_edge_points_compatible(true, &[[1, 1]], [2, 2]));
    }

    #[test]
    fn quotient_merges_roots_forced_to_one_coordinate_identity() {
        let mut quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: [0, 1, 0, 2]
                .into_iter()
                .map(|point| Arc::new(HashSet::from([point])))
                .collect(),
            members: (0..4).map(|node| vec![node]).collect(),
        };

        assert!(quotient.merge_singleton_coordinate_roots(&[Vec::new(), Vec::new()]));
        assert_eq!(quotient.root_count(), 3);
        assert_eq!(quotient.union.find(0), quotient.union.find(2));
    }

    #[test]
    fn singleton_coordinate_root_merges_are_batched() {
        const ROOT_COUNT: usize = 10_000;
        let mut quotient = MeshQuotient {
            union: UnionFind::new(ROOT_COUNT),
            domains: vec![Arc::new(HashSet::from([0])); ROOT_COUNT],
            members: (0..ROOT_COUNT).map(|node| vec![node]).collect(),
        };
        let candidates = vec![Vec::new(); ROOT_COUNT / 2];

        assert!(quotient.merge_singleton_coordinate_roots(&candidates));
        assert_eq!(quotient.root_count(), 1);
    }

    #[test]
    fn quotient_closes_coordinate_roots_forced_by_joint_edge_pairs() {
        let all = Arc::new(HashSet::from([0, 1, 2]));
        let mut quotient = MeshQuotient {
            union: UnionFind::new(6),
            domains: vec![all.clone(); 6],
            members: (0..6).map(|node| vec![node]).collect(),
        };
        quotient.merge(1, 2).expect("shared first corner");
        quotient.merge(3, 4).expect("shared second corner");
        let candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]];

        let assignment = quotient
            .close_coordinate_roots(3, &candidates)
            .expect("unique joint coordinate closure");

        assert_eq!(quotient.root_count(), 3);
        assert_eq!(quotient.union.find(0), quotient.union.find(5));
        assert_eq!(assignment[&quotient.union.find(0)], 0);
        assert_eq!(assignment[&quotient.union.find(1)], 1);
        assert_eq!(assignment[&quotient.union.find(3)], 2);
    }

    #[test]
    fn quotient_closes_independent_coordinate_components_with_local_budgets() {
        const COMPONENT_COUNT: usize = 100;
        let point_count = COMPONENT_COUNT * 3;
        let mut quotient = MeshQuotient {
            union: UnionFind::new(COMPONENT_COUNT * 6),
            domains: (0..COMPONENT_COUNT)
                .flat_map(|component| {
                    let points = Arc::new(HashSet::from_iter(component * 3..component * 3 + 3));
                    std::iter::repeat_n(points, 6)
                })
                .collect(),
            members: (0..COMPONENT_COUNT * 6).map(|node| vec![node]).collect(),
        };
        let mut candidates = Vec::new();
        for component in 0..COMPONENT_COUNT {
            let node = component * 6;
            let point = component * 3;
            quotient
                .merge(node + 1, node + 2)
                .expect("shared first corner");
            quotient
                .merge(node + 3, node + 4)
                .expect("shared second corner");
            candidates.extend([
                vec![[point, point + 1]],
                vec![[point + 1, point + 2]],
                vec![[point, point + 2]],
            ]);
        }

        let assignment = quotient
            .close_coordinate_roots(point_count, &candidates)
            .expect("independent coordinate closures");

        assert_eq!(quotient.root_count(), point_count);
        assert_eq!(assignment.len(), point_count);
        for component in 0..COMPONENT_COUNT {
            let node = component * 6;
            assert_eq!(quotient.union.find(node), quotient.union.find(node + 5));
        }
    }

    #[test]
    fn quotient_closure_does_not_budget_forced_component_depth() {
        const ROOT_COUNT: usize = 10_000;
        let mut quotient = MeshQuotient {
            union: UnionFind::new(ROOT_COUNT),
            domains: vec![Arc::new(HashSet::from([0])); ROOT_COUNT],
            members: (0..ROOT_COUNT).map(|node| vec![node]).collect(),
        };
        let candidates = vec![vec![[0, 0]]; ROOT_COUNT / 2];

        let assignment = quotient
            .close_coordinate_roots(1, &candidates)
            .expect("forced coordinate component");

        assert_eq!(quotient.root_count(), 1);
        assert_eq!(assignment.values().copied().collect::<Vec<_>>(), [0]);
    }

    #[test]
    fn quotient_does_not_guess_an_ambiguous_coordinate_closure() {
        let all = Arc::new(HashSet::from([0, 1]));
        let mut quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: vec![all.clone(); 4],
            members: (0..4).map(|node| vec![node]).collect(),
        };
        quotient.merge(1, 2).expect("shared middle corner");

        assert!(quotient
            .close_coordinate_roots(2, &[vec![[0, 1]], vec![[0, 1]]])
            .is_none());
        assert_eq!(quotient.root_count(), 3);
    }

    #[test]
    fn quotient_closure_requires_every_coordinate_row_in_a_domain() {
        let mut quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: vec![Arc::new(HashSet::from([0])); 4],
            members: (0..4).map(|node| vec![node]).collect(),
        };
        quotient.merge(1, 2).expect("shared endpoint");

        assert!(quotient
            .close_coordinate_roots(2, &[vec![[0, 0]], vec![[0, 0]]])
            .is_none());
        assert_eq!(quotient.root_count(), 3);
    }

    #[test]
    fn quotient_accepts_diagonal_domain_for_closed_edge() {
        let mut quotient = MeshQuotient {
            union: UnionFind::new(2),
            domains: vec![Arc::new(HashSet::from([2])), Arc::new(HashSet::from([2]))],
            members: vec![vec![0], vec![1]],
        };
        quotient.merge(0, 1).expect("closed endpoint merge");
        assert!(quotient.edge_domains_viable(&[vec![[2, 2]]]));
        assert!(!quotient.edge_domains_viable(&[vec![[1, 2]]]));
    }

    #[test]
    fn quotient_point_assignment_accepts_a_closed_diagonal_edge() {
        let mut quotient = MeshQuotient {
            union: UnionFind::new(2),
            domains: vec![Arc::new(HashSet::from([0])); 2],
            members: vec![vec![0], vec![1]],
        };
        let root = quotient.merge(0, 1).expect("closed endpoint merge");

        assert_eq!(
            quotient.point_assignment(1, &[vec![[0, 0]]]),
            Some(HashMap::from([(root, 0)]))
        );
    }

    #[test]
    fn quotient_retains_diagonal_pairs_until_ports_are_merged() {
        let mut quotient = MeshQuotient {
            union: UnionFind::new(2),
            domains: vec![
                Arc::new(HashSet::from([1, 2])),
                Arc::new(HashSet::from([1, 2])),
            ],
            members: vec![vec![0], vec![1]],
        };

        assert!(quotient.edge_domains_viable(&[vec![[2, 2]]]));
        assert_eq!(
            quotient.domains,
            vec![Arc::new(HashSet::from([2])), Arc::new(HashSet::from([2]))]
        );
        quotient.merge(0, 1).expect("closed endpoint merge");
        assert!(quotient.edge_domains_viable(&[vec![[2, 2]]]));
    }

    #[test]
    fn closed_edge_is_a_single_coedge_boundary_on_each_incident_face() {
        let topology = reconstruct_incidence(
            vec![EdgeRow {
                kind: 0,
                handles: vec![7, 7],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            }],
            vec![[1.0, 0.0, 0.0]],
            &[[0, 1]],
            &[[0, 0]],
            2,
        )
        .expect("closed radial edge");
        assert!(topology
            .faces()
            .iter()
            .all(|face| face.boundaries.len() == 1 && face.boundaries[0].coedges.len() == 1));
        assert_ne!(
            topology.faces()[0].boundaries[0].coedges[0].reversed,
            topology.faces()[1].boundaries[0].coedges[0].reversed
        );
    }

    #[test]
    fn duplicate_face_reference_slot_is_completed_by_face_closure() {
        let rows = (0..3)
            .map(|handle| EdgeRow {
                kind: 0,
                handles: vec![handle],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            })
            .collect::<Vec<_>>();
        let faces = complete_duplicate_face_slots(
            &rows,
            &[[0, 1], [0, 1], [0, 0]],
            &[[0, 1], [1, 2], [2, 0]],
            2,
            None,
            Some(&[]),
        )
        .expect("unique face-closing slot assignment");

        assert_eq!(faces, vec![[0, 1], [0, 1], [0, 1]]);
    }

    #[test]
    fn mesh_assignment_endpoint_cycles_reject_crossed_edge_order() {
        let use_ = |edge| MeshBoundaryEdgeCandidate {
            edge,
            start: 0,
            end: 0,
            reversed: None,
        };
        let assignment = |edges: &[usize]| MeshFaceBoundaryAssignment {
            boundaries: vec![edges.iter().copied().map(use_).collect()],
        };
        let candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[2, 3]], vec![[3, 0]]];

        assert!(mesh_assignment_endpoint_cycles_viable(
            &assignment(&[0, 1, 2, 3]),
            &candidates,
        ));
        assert!(!mesh_assignment_endpoint_cycles_viable(
            &assignment(&[0, 2, 1, 3]),
            &candidates,
        ));
    }

    #[test]
    fn mesh_face_endpoint_configurations_preserve_pair_correlation() {
        let assignment = MeshFaceBoundaryAssignment {
            boundaries: vec![(0..4)
                .map(|edge| MeshBoundaryEdgeCandidate {
                    edge,
                    start: 0,
                    end: 0,
                    reversed: None,
                })
                .collect()],
        };
        let candidates = vec![
            vec![[0, 1]],
            vec![[1, 2], [2, 3]],
            vec![[2, 3], [1, 2]],
            vec![[3, 0]],
        ];
        let configurations =
            mesh_face_endpoint_configurations(&[assignment], &candidates, &[None; 4], 4_096)
                .expect("bounded face configurations");

        assert_eq!(
            configurations,
            vec![vec![(0, [0, 1]), (1, [1, 2]), (2, [2, 3]), (3, [0, 3])]],
        );
    }

    #[test]
    fn mesh_assignment_endpoint_cycles_preserve_unconstrained_boundaries() {
        let assignment = MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 0,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 0,
                    end: 0,
                    reversed: None,
                },
            ]],
        };
        assert!(mesh_assignment_endpoint_cycles_viable(
            &assignment,
            &[vec![[0, 1]], Vec::new()],
        ));
    }

    #[test]
    fn mesh_endpoint_pair_support_propagates_across_incident_faces() {
        let assignment = |edges: &[usize]| MeshFaceBoundaryAssignment {
            boundaries: vec![edges
                .iter()
                .copied()
                .map(|edge| MeshBoundaryEdgeCandidate {
                    edge,
                    start: 0,
                    end: 0,
                    reversed: None,
                })
                .collect()],
        };
        let mut assignments = vec![
            vec![assignment(&[0, 1, 2])],
            vec![assignment(&[0, 3, 4]), assignment(&[0, 5, 6])],
        ];
        let mut candidates = vec![
            vec![[0, 1], [0, 3]],
            vec![[1, 2]],
            vec![[2, 0]],
            vec![[1, 4]],
            vec![[4, 0]],
            vec![[3, 5]],
            vec![[5, 0]],
        ];

        assert!(prune_mesh_endpoint_pair_support(
            &mut assignments,
            &mut candidates,
        ));
        assert_eq!(candidates[0], vec![[0, 1]]);
        assert_eq!(assignments[1], vec![assignment(&[0, 3, 4])]);
    }

    #[test]
    fn duplicate_face_slot_requires_one_joint_carrier_and_mesh_assignment() {
        let serialized = [[0, 0], [0, 1], [1, 1]];
        let allowed = [vec![1, 2], Vec::new(), vec![0, 2]];
        let resolved = unique_duplicate_face_assignment(&serialized, &allowed, 3, |faces| {
            faces == [[0, 2], [0, 1], [1, 0]]
        })
        .expect("one complete assignment");
        assert_eq!(resolved, [[0, 2], [0, 1], [1, 0]]);

        assert!(unique_duplicate_face_assignment(&serialized, &allowed, 3, |_| true).is_none());
        assert!(unique_duplicate_face_assignment(
            &serialized,
            &[vec![3], Vec::new(), vec![0]],
            3,
            |_| true,
        )
        .is_none());
    }

    #[test]
    fn duplicate_face_slots_do_not_budget_forced_assignments() {
        const EDGE_COUNT: usize = 5_000;
        let serialized = vec![[0, 0]; EDGE_COUNT];
        let allowed = vec![vec![1, 1]; EDGE_COUNT];

        let resolved = unique_duplicate_face_assignment(&serialized, &allowed, 2, |_| true)
            .expect("forced duplicate-face assignments");

        assert_eq!(resolved, vec![[0, 1]; EDGE_COUNT]);
    }

    #[test]
    fn face_endpoint_candidates_require_one_closed_local_cycle() {
        let faces = [[0, 1], [0, 2], [0, 3]];
        assert!(face_endpoint_candidates_close(
            &faces,
            &[vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]],
            0,
        ));
        assert!(!face_endpoint_candidates_close(
            &faces,
            &[vec![[0, 1]], vec![[1, 2]], vec![[3, 4]]],
            0,
        ));
    }

    #[test]
    fn face_endpoint_candidates_do_not_budget_fixed_cycle_size() {
        const EDGE_COUNT: usize = 65_537;
        let faces = vec![[0, 0]; EDGE_COUNT];
        let candidates = (0..EDGE_COUNT)
            .map(|edge| vec![[edge, (edge + 1) % EDGE_COUNT]])
            .collect::<Vec<_>>();

        assert!(face_endpoint_candidates_close(&faces, &candidates, 0));
    }

    #[test]
    fn counted_edge_arities_are_bounded_by_remaining_bytes() {
        let oversized_row = [0x01, 0x01, 0x01, 0x02, 0xff, 0xff, 0xff, 0xff, 0xff];
        assert!(parse_edge_tables_scoped_at(&oversized_row, 0).is_none());
        assert!(parse_fbb_edge_tables_width(&oversized_row, 0, 3).is_none());
    }

    #[test]
    fn trim_primitive_counts_are_bounded_by_remaining_bytes() {
        let oversized_primitives = [
            0x01, 0x46, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00,
            0x00, 0x00,
        ];
        assert!(parse_trim_record(&oversized_primitives, 0, 2).is_none());
    }

    #[test]
    fn duplicate_face_completion_rejects_out_of_range_faces() {
        let rows = vec![EdgeRow {
            kind: 0,
            handles: vec![0, 1],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        }];
        assert!(
            complete_duplicate_face_slots(&rows, &[[0, 2]], &[[0, 1]], 2, None, None,).is_none()
        );
    }

    #[test]
    fn exact_mesh_occurrences_complete_duplicate_face_slot() {
        let run = |edge, face| MeshEdgeRun {
            edge,
            face,
            cycle: 0,
            start: 0,
            segment_count: 1,
            reversed: false,
        };
        let faces = resolve_edge_faces_from_runs(
            &[[1, 1], [2, 2], [3, 4]],
            &[run(0, 1), run(0, 5), run(1, 2), run(2, 3), run(2, 4)],
        )
        .expect("consistent exact face occurrences");

        assert_eq!(faces, vec![[1, 5], [2, 2], [3, 4]]);
    }

    #[test]
    fn equivalent_edge_rows_share_one_incidence_assignment_gauge() {
        let rows = vec![
            EdgeRow {
                kind: 0,
                handles: vec![0],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
            EdgeRow {
                kind: 0,
                handles: vec![1],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
            EdgeRow {
                kind: 0,
                handles: vec![2, 3],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
            EdgeRow {
                kind: 0,
                handles: vec![4, 5],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
        ];
        let faces = complete_duplicate_face_slots(
            &rows,
            &[[0, 1], [0, 1], [2, 2], [2, 2]],
            &[[0, 1], [1, 2], [2, 0], [0, 2]],
            3,
            Some(&[0, 1, 2, 2]),
            None,
        )
        .expect("one assignment modulo equivalent edge rows");

        let mut assigned = [faces[2][1], faces[3][1]];
        assigned.sort_unstable();
        assert_eq!(assigned, [0, 1]);
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
        let candidates = [vec![[0, 1]], vec![[2, 3]], vec![[0, 2]], vec![[1, 3]]];
        assert_eq!(
            bind_edge_port_candidates(&ports, &candidates),
            Some(vec![[0, 1], [2, 3], [0, 2], [1, 3]])
        );
    }

    #[test]
    fn native_edge_identities_reject_multiple_coordinate_bijections() {
        let ports = [[10, 11]];
        let candidates = [vec![[0, 1], [2, 3]]];
        assert_eq!(bind_edge_port_candidates(&ports, &candidates), None);
    }

    #[test]
    fn native_edge_identities_preserve_endpoint_equality() {
        assert_eq!(
            bind_edge_port_candidates(&[[10, 11]], &[vec![[0, 0]]]),
            None
        );
        assert_eq!(
            bind_edge_port_candidates(&[[10, 10]], &[vec![[0, 1]]]),
            None
        );
        assert_eq!(
            bind_edge_port_candidates(&[[10, 10]], &[vec![[0, 0]]]),
            Some(vec![[0, 0]])
        );
    }

    #[test]
    fn native_edge_identities_bind_independent_components_with_local_budgets() {
        const COMPONENT_COUNT: usize = 100;
        let ports = (0..COMPONENT_COUNT)
            .map(|component| {
                let port = u32::try_from(component * 2).expect("bounded port identity");
                [port, port + 1]
            })
            .collect::<Vec<_>>();
        let candidates = (0..COMPONENT_COUNT)
            .map(|component| vec![[component * 2, component * 2 + 1]])
            .collect::<Vec<_>>();

        let solution =
            bind_edge_port_candidates(&ports, &candidates).expect("independent port components");

        assert_eq!(solution.len(), COMPONENT_COUNT);
        assert!(solution
            .iter()
            .zip(&candidates)
            .all(|(pair, candidates)| same_unordered_pair(*pair, candidates[0])));
    }

    #[test]
    fn native_edge_identities_do_not_charge_forced_chain_depth() {
        const EDGE_COUNT: usize = 10_000;
        let ports = (0..EDGE_COUNT)
            .map(|edge| {
                let port = u32::try_from(edge).expect("bounded port identity");
                [port, port + 1]
            })
            .collect::<Vec<_>>();
        let candidates = (0..EDGE_COUNT)
            .map(|edge| vec![[edge, edge + 1]])
            .collect::<Vec<_>>();

        let solution =
            bind_edge_port_candidates(&ports, &candidates).expect("forced connected port chain");

        assert_eq!(
            solution,
            candidates.into_iter().flatten().collect::<Vec<_>>()
        );
    }

    #[test]
    fn duplicate_coordinate_rows_have_one_geometric_bijection() {
        let domains = [HashSet::from([0, 1]), HashSet::from([0, 1])];
        assert_eq!(
            unique_coordinate_bijection(&domains, &[[1.0, 2.0, 3.0], [1.0, 2.0, 3.0]]),
            Some(vec![0, 1])
        );
    }

    #[test]
    fn forced_coordinate_bijection_has_no_recursive_depth_limit() {
        const POINT_COUNT: usize = 10_000;
        let domains = (0..POINT_COUNT)
            .map(|point| HashSet::from([point]))
            .collect::<Vec<_>>();
        let points = (0..POINT_COUNT)
            .map(|point| {
                [
                    f64::from(u32::try_from(point).expect("bounded point index")),
                    0.0,
                    0.0,
                ]
            })
            .collect::<Vec<_>>();

        assert_eq!(
            unique_coordinate_bijection(&domains, &points),
            Some((0..POINT_COUNT).collect())
        );
    }

    #[test]
    fn coordinate_bijection_respects_duplicate_class_capacity() {
        let domains = [
            HashSet::from([0, 2]),
            HashSet::from([0, 1]),
            HashSet::from([0, 1]),
        ];
        let points = [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];

        assert_eq!(
            unique_coordinate_bijection(&domains, &points),
            Some(vec![2, 0, 1])
        );
    }

    #[test]
    fn distinct_coordinate_bijections_remain_ambiguous() {
        let domains = [HashSet::from([0, 1]), HashSet::from([0, 1])];
        assert_eq!(
            unique_coordinate_bijection(&domains, &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]),
            None
        );
    }
}
