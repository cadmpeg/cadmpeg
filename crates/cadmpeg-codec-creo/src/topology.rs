// SPDX-License-Identifier: Apache-2.0
//! Native half-edge graph assembly from curve topology rows.
//!
//! [`build`] resolves successors only when a curve and face identify one
//! candidate. It emits a [`Loop`] only when traversal closes on its starting
//! half-edge.

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
    let mut edges = Vec::with_capacity(rows.len() * 2);
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
