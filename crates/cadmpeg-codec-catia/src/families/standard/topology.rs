//! `StandardTopology` container and face-cycle orientation for standard
//! nested CATIA V5 B-rep streams.

use crate::families::standard::fbb::{
    boundary_cycles, cover_cycle, largest_fbb_run, parse_fbb_edge_tables, parse_trim_chain,
    parse_vertex_table,
};
use crate::solve::matching::unique_coordinate_bijection;
use crate::solve::missing_edge::{standard_mesh_boundary_assignments, MeshFaceBoundaryAssignment};
use crate::solve::UnionFind;
use cadmpeg_ir::topology::BodyKind;
use std::collections::{HashMap, HashSet};

/// Reconstructed standard-nested (or FBB-only) topology: the counted spine's
/// face boundaries recovered from the trim-mesh triangle packets, plus the
/// physical edge rows and, for the standard family, the `05 08 01` vertex
/// coordinate table ([spec ?5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#5-standard-nested-v5_cfv2-topology-spine)).
#[derive(Debug, Clone, PartialEq)]
pub struct StandardTopology {
    pub(crate) faces: Vec<FaceTopology>,
    pub(crate) edge_rows: Vec<EdgeRow>,
    pub(crate) vertex_points: Vec<[f64; 3]>,
    pub(crate) logical_vertex_count: usize,
}

pub(crate) fn component_root(parents: &mut [usize], index: usize) -> usize {
    if parents[index] != index {
        parents[index] = component_root(parents, parents[index]);
    }
    parents[index]
}

pub(crate) fn union_components(parents: &mut [usize], left: usize, right: usize) {
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
    #[cfg(test)]
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
        let mut first_face_by_edge = vec![None; self.edge_rows.len()];
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
                first_face_by_edge[coedge.edge_row].get_or_insert(face);
                *uses[body].entry(coedge.edge_row).or_default() += 1;
            }
        }
        if bodies_by_edge.iter().any(|bodies| bodies.len() != 1) {
            return None;
        }
        let components = self.face_components();
        let mut component_by_face = vec![0usize; self.faces.len()];
        let mut components_by_body = vec![Vec::new(); face_groups.len()];
        for (component, faces) in components.iter().enumerate() {
            let body = body_by_face[faces[0]];
            components_by_body[body].push(component);
            for &face in faces {
                component_by_face[face] = component;
            }
        }
        let component_by_edge = first_face_by_edge
            .into_iter()
            .map(|face| face.map(|face| component_by_face[face]))
            .collect::<Vec<_>>();
        Some(
            uses.into_iter()
                .zip(components_by_body)
                .map(|(uses, components)| {
                    let closed_component_count = components
                        .iter()
                        .filter(|component| {
                            uses.keys()
                                .any(|edge| component_by_edge[*edge] == Some(**component))
                                && uses.iter().all(|(edge, count)| {
                                    component_by_edge[*edge] != Some(**component) || *count == 2
                                })
                        })
                        .count();
                    if uses.values().any(|count| *count > 2)
                        || (closed_component_count != 0
                            && closed_component_count != components.len())
                    {
                        BodyKind::General
                    } else if !components.is_empty() && closed_component_count == components.len() {
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
pub(crate) struct TrimRecord {
    pub(crate) triangles: Vec<[u32; 3]>,
    pub(crate) frame_vector: Option<[f64; 3]>,
    pub(crate) handles: Vec<u32>,
    pub(crate) independent_count: usize,
    pub(crate) strip_lengths: Vec<usize>,
    pub(crate) fan_lengths: Vec<usize>,
    pub(crate) kind: u8,
}

pub(crate) fn reconstruct_incidence(
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

pub(crate) fn reconstruct_incidence_with_edge_classes_and_mesh(
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

pub(crate) fn complete_duplicate_face_slots(
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

    pub(crate) fn search(
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
                    standard_mesh_boundary_assignments(bytes, &completed, None).is_some()
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

pub(crate) fn orient_face_cycles(faces: &mut [FaceTopology]) -> Option<()> {
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

pub(crate) fn incidence_cycles(
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

pub(crate) fn reconstruct(
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

pub(crate) fn reconstruct_mesh_selection(
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
