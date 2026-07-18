// SPDX-License-Identifier: Apache-2.0
//! Native half-edge graph assembly from curve topology rows.
//!
//! [`build`] resolves successors only when a curve and face identify one
//! candidate. It emits a [`Loop`] only when traversal closes on its starting
//! half-edge.
#![deny(clippy::disallowed_methods)]

use std::collections::{BTreeMap, BTreeSet};

use crate::curve::CurveTopologyRow;

/// A curve identifier paired with one of its two native sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HalfEdgeId {
    /// The owning curve's `crv_id` in the `crv_array` namespace.
    pub curve_id: u32,
    /// The half-edge side: `0` for the `F0`/`E0` suffix fields, `1` for
    /// `F1`/`E1`.
    pub side: u8,
}

/// A native half-edge, its face, and its uniquely resolved successor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HalfEdge {
    /// This half-edge's curve and side.
    pub id: HalfEdgeId,
    /// The `srf_array` face identifier this half-edge side bounds (the
    /// corresponding `F0`/`F1` suffix field).
    pub face_id: u32,
    /// The next half-edge on the same face, when exactly one candidate
    /// successor matched the row's `E0`/`E1` next-edge field on that face.
    /// `None` when the successor is absent or ambiguous.
    pub next: Option<HalfEdgeId>,
}

/// A closed ring of half-edges on one face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loop {
    /// The `srf_array` face identifier this loop bounds.
    pub face_id: u32,
    /// The ring of half-edges in traversal order, starting from the first
    /// half-edge encountered for this face.
    pub half_edges: Vec<HalfEdgeId>,
}

/// One connected component of non-null `srf_array` face references.
///
/// The component is native topology only. It is not an emitted shell because
/// curve geometry, face carriers, and vertex bindings are independent layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceComponent {
    /// Sorted nonzero face identifiers in the connected component.
    pub face_ids: Vec<u32>,
    /// Sorted curve identifiers whose two sides connect component faces.
    pub curve_ids: Vec<u32>,
}

/// One topological vertex represented by its incident half-edge orbit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologicalVertex {
    /// Deterministic one-based vertex identifier.
    pub id: u32,
    /// Sorted half-edges sharing this start vertex.
    pub half_edges: Vec<HalfEdgeId>,
}

/// Start/end vertex binding for one oriented half-edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HalfEdgeVertexIncidence {
    /// Bound oriented half-edge.
    pub half_edge: HalfEdgeId,
    /// Vertex orbit containing this half-edge.
    pub start_vertex_id: u32,
    /// Start vertex of the resolved successor half-edge.
    pub end_vertex_id: Option<u32>,
}

/// Build topological vertex orbits under `twin(previous(h))` and bind each
/// half-edge's start and end vertex.
pub fn vertex_orbits(edges: &[HalfEdge]) -> (Vec<TopologicalVertex>, Vec<HalfEdgeVertexIncidence>) {
    let by_id = edges
        .iter()
        .map(|edge| (edge.id, edge))
        .collect::<BTreeMap<_, _>>();
    let mut predecessors = BTreeMap::<HalfEdgeId, Vec<HalfEdgeId>>::new();
    for edge in edges {
        if let Some(next) = edge.next {
            predecessors.entry(next).or_default().push(edge.id);
        }
    }
    let vertex_step = |half_edge: HalfEdgeId| {
        let previous = predecessors.get(&half_edge)?;
        (previous.len() == 1).then_some(HalfEdgeId {
            curve_id: previous[0].curve_id,
            side: 1 - previous[0].side,
        })
    };
    let mut visited = BTreeSet::new();
    let mut vertices = Vec::new();
    for start in by_id.keys().copied() {
        if visited.contains(&start) {
            continue;
        }
        let mut orbit = BTreeSet::new();
        let mut current = Some(start);
        while let Some(half_edge) = current {
            if orbit.contains(&half_edge) || visited.contains(&half_edge) {
                break;
            }
            orbit.insert(half_edge);
            visited.insert(half_edge);
            current = vertex_step(half_edge).filter(|next| by_id.contains_key(next));
        }
        vertices.push(TopologicalVertex {
            id: u32::try_from(vertices.len() + 1).unwrap_or(u32::MAX),
            half_edges: orbit.into_iter().collect(),
        });
    }
    let start_vertex = vertices
        .iter()
        .flat_map(|vertex| {
            vertex
                .half_edges
                .iter()
                .map(move |half_edge| (*half_edge, vertex.id))
        })
        .collect::<BTreeMap<_, _>>();
    let incidence = edges
        .iter()
        .filter_map(|edge| {
            Some(HalfEdgeVertexIncidence {
                half_edge: edge.id,
                start_vertex_id: *start_vertex.get(&edge.id)?,
                end_vertex_id: edge.next.and_then(|next| start_vertex.get(&next).copied()),
            })
        })
        .collect();
    (vertices, incidence)
}

/// Group non-null face references connected by curve topology rows.
///
/// Face identifier zero is a boundary sentinel, never a shell face. A curve
/// contributes to a component when either of its sides names a nonzero face.
pub fn face_components(rows: &[CurveTopologyRow]) -> Vec<FaceComponent> {
    let mut adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    let mut face_curves = BTreeMap::<u32, BTreeSet<u32>>::new();
    for row in rows {
        let [left, right] = row.faces;
        for face in [left, right].into_iter().filter(|face| *face != 0) {
            adjacency.entry(face).or_default();
            face_curves.entry(face).or_default().insert(row.id);
        }
        if left != 0 && right != 0 && left != right {
            adjacency.entry(left).or_default().insert(right);
            adjacency.entry(right).or_default().insert(left);
        }
    }
    let mut seen = BTreeSet::new();
    let mut components = Vec::new();
    for start in adjacency.keys().copied().collect::<Vec<_>>() {
        if !seen.insert(start) {
            continue;
        }
        let mut pending = vec![start];
        let mut faces = BTreeSet::new();
        let mut curves = BTreeSet::new();
        while let Some(face) = pending.pop() {
            faces.insert(face);
            curves.extend(face_curves.get(&face).into_iter().flatten().copied());
            for neighbour in adjacency.get(&face).into_iter().flatten().copied() {
                if seen.insert(neighbour) {
                    pending.push(neighbour);
                }
            }
        }
        components.push(FaceComponent {
            face_ids: faces.into_iter().collect(),
            curve_ids: curves.into_iter().collect(),
        });
    }
    components
}

/// Build half-edges and closed loops from curve topology rows.
///
/// Ambiguous or missing successors remain `None` and cannot form loops.
pub fn build(rows: &[CurveTopologyRow]) -> (Vec<HalfEdge>, Vec<Loop>) {
    let mut face_sides: BTreeMap<u32, Vec<HalfEdgeId>> = BTreeMap::new();
    for row in rows {
        for side in 0..2 {
            face_sides
                .entry(row.faces[side])
                .or_default()
                .push(HalfEdgeId {
                    curve_id: row.id,
                    side: side as u8,
                });
        }
    }
    let mut edges = Vec::new();
    for row in rows {
        for side in 0..2 {
            let face_id = row.faces[side];
            let candidates = face_sides
                .get(&face_id)
                .into_iter()
                .flatten()
                .filter(|id| id.curve_id == row.next_edges[side])
                .copied()
                .collect::<Vec<_>>();
            edges.push(HalfEdge {
                id: HalfEdgeId {
                    curve_id: row.id,
                    side: side as u8,
                },
                face_id,
                next: (candidates.len() == 1)
                    .then(|| candidates.first().copied())
                    .flatten(),
            });
        }
    }
    edges.sort_by_key(|edge| edge.id);
    let by_id = edges
        .iter()
        .map(|edge| (edge.id, edge))
        .collect::<BTreeMap<_, _>>();
    let mut consumed = BTreeSet::new();
    let mut loops = Vec::new();
    for edge in &edges {
        if consumed.contains(&edge.id) {
            continue;
        }
        let mut ring = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = edge.id;
        loop {
            if !seen.insert(current) {
                if current == edge.id {
                    loops.push(Loop {
                        face_id: edge.face_id,
                        half_edges: ring.clone(),
                    });
                    consumed.extend(ring);
                }
                break;
            }
            ring.push(current);
            let Some(next) = by_id.get(&current).and_then(|entry| entry.next) else {
                break;
            };
            if by_id
                .get(&next)
                .is_none_or(|entry| entry.face_id != edge.face_id)
            {
                break;
            }
            current = next;
        }
    }
    (edges, loops)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(id: u32, next: u32) -> CurveTopologyRow {
        CurveTopologyRow {
            id,
            type_byte: 0,
            feature_id: 0,
            directions: [1, 1],
            faces: [10, 20],
            next_edges: [next, next],
            offset: 0,
        }
    }
    #[test]
    fn builds_closed_face_side_rings_without_guessing() {
        let (half_edges, loops) = build(&[row(1, 2), row(2, 3), row(3, 1)]);
        assert_eq!(half_edges.len(), 6);
        assert_eq!(loops.len(), 2);
        assert_eq!(loops[0].face_id, 10);
        assert_eq!(
            loops[0].half_edges,
            vec![
                HalfEdgeId {
                    curve_id: 1,
                    side: 0
                },
                HalfEdgeId {
                    curve_id: 2,
                    side: 0
                },
                HalfEdgeId {
                    curve_id: 3,
                    side: 0
                }
            ]
        );
    }
    #[test]
    fn withholds_ambiguous_successors() {
        let (half_edges, loops) = build(&[
            row(1, 2),
            row(2, 1),
            CurveTopologyRow {
                faces: [10, 30],
                ..row(2, 1)
            },
        ]);
        assert!(half_edges.iter().any(|edge| edge.id
            == HalfEdgeId {
                curve_id: 1,
                side: 0
            }
            && edge.next.is_none()));
        assert!(loops.is_empty());
    }
}
