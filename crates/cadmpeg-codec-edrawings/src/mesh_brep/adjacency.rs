// SPDX-License-Identifier: Apache-2.0
//! Half-edge pairing.

use super::geom::{corner, cross, dot, norm, sub, Slot};
use super::mesh::Mesh;
use super::{MeshBrepError, MIN_WEDGE_ANGLE_RAD};

/// Twin table and edge statistics.
pub(super) struct Adjacency {
    /// Opposite half-edge of a manifold, oppositely oriented pair.
    pub(super) twin: Vec<Option<usize>>,
    /// Undirected edges used by exactly one triangle.
    pub(super) open_edges: usize,
    /// Undirected edges used by more than two triangles or by a pair with
    /// equal direction. Their half-edges stay unpaired.
    pub(super) non_manifold_edges: usize,
}

/// Pair the half-edges of `tris`; half-edge `3 * t + k` runs from corner `k`
/// of triangle `t` to corner `(k + 1) % 3`.
pub(super) fn build(tris: &[[usize; 3]]) -> Result<Adjacency, MeshBrepError> {
    let half_edges = tris.len() * 3;
    let from = |he: usize| corner(tris.slot(he / 3)?, he % 3);
    let to = |he: usize| corner(tris.slot(he / 3)?, (he % 3 + 1) % 3);
    let mut entries: Vec<(usize, usize, usize)> = Vec::with_capacity(half_edges);
    for he in 0..half_edges {
        let (a, b) = (from(he)?, to(he)?);
        entries.push((a.min(b), a.max(b), he));
    }
    entries.sort_unstable();
    let mut adjacency = Adjacency {
        twin: (0..half_edges).map(|_| None).collect::<Vec<_>>(),
        open_edges: 0,
        non_manifold_edges: 0,
    };
    let mut start = 0;
    while start < entries.len() {
        let key = entries.get(start).map(|&(lo, hi, _)| (lo, hi));
        let mut end = start + 1;
        while entries.get(end).map(|&(lo, hi, _)| (lo, hi)) == key {
            end += 1;
        }
        let group = entries
            .get(start..end)
            .ok_or(MeshBrepError::Internal("edge group"))?;
        match group {
            [_] => adjacency.open_edges += 1,
            [(_, _, first), (_, _, second)] => {
                if from(*first)? == to(*second)? {
                    *adjacency.twin.slot_mut(*first)? = Some(*second);
                    *adjacency.twin.slot_mut(*second)? = Some(*first);
                } else {
                    adjacency.non_manifold_edges += 1;
                }
            }
            _ => adjacency.non_manifold_edges += 1,
        }
        start = end;
    }
    Ok(adjacency)
}

/// Pair the half-edges of every non-manifold edge whose triangles alternate in
/// direction around it, as at two solids that touch along an edge.
///
/// The triangles around the edge are ordered by their angle about it. In a
/// valid arrangement consecutive triangles run in opposite directions; the two
/// faces that bound each wedge of material become twins. Returns the number of
/// edges paired. An edge with a direction imbalance, equal angles, or two
/// consecutive triangles of the same direction stays unpaired.
pub(super) fn pair_around_edges(
    mesh: &Mesh,
    adjacency: &mut Adjacency,
) -> Result<usize, MeshBrepError> {
    let mut entries: Vec<(usize, usize, usize)> = Vec::new();
    for (he, twin) in adjacency.twin.iter().enumerate() {
        if twin.is_none() {
            let (a, b) = (mesh.from(he)?, mesh.to(he)?);
            entries.push((a.min(b), a.max(b), he));
        }
    }
    entries.sort_unstable();
    let mut paired = 0;
    let mut start = 0;
    while start < entries.len() {
        let key = entries.get(start).map(|&(lo, hi, _)| (lo, hi));
        let mut end = start + 1;
        while entries.get(end).map(|&(lo, hi, _)| (lo, hi)) == key {
            end += 1;
        }
        let group = entries
            .get(start..end)
            .ok_or(MeshBrepError::Internal("edge group"))?;
        start = end;
        let Some((lo, hi)) = key else {
            continue;
        };
        if group.len() < 4 || group.len() % 2 != 0 {
            continue;
        }
        let Some(order) = angular_order(mesh, lo, hi, group)? else {
            continue;
        };
        for (i, &(he, forward)) in order.iter().enumerate() {
            let (next, next_forward) = *order
                .get((i + 1) % order.len())
                .ok_or(MeshBrepError::Internal("angular order"))?;
            if !forward && next_forward {
                *adjacency.twin.slot_mut(he)? = Some(next);
                *adjacency.twin.slot_mut(next)? = Some(he);
            }
        }
        adjacency.non_manifold_edges = adjacency
            .non_manifold_edges
            .checked_sub(1)
            .ok_or(MeshBrepError::Internal("non-manifold count"))?;
        paired += 1;
    }
    Ok(paired)
}

/// The half-edges of one edge `lo..hi` with their direction flag, in
/// increasing angle about the edge; `None` when they do not alternate in
/// direction or two share an angle.
fn angular_order(
    mesh: &Mesh,
    lo: usize,
    hi: usize,
    group: &[(usize, usize, usize)],
) -> Result<Option<Vec<(usize, bool)>>, MeshBrepError> {
    let (p_lo, p_hi) = (*mesh.pos.slot(lo)?, *mesh.pos.slot(hi)?);
    let axis = sub(p_hi, p_lo);
    let length = norm(axis);
    if length <= 0.0 {
        return Ok(None);
    }
    let d = [axis[0] / length, axis[1] / length, axis[2] / length];
    let helper = if d[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let e1_raw = cross(d, helper);
    let e1_norm = norm(e1_raw);
    let e1 = [
        e1_raw[0] / e1_norm,
        e1_raw[1] / e1_norm,
        e1_raw[2] / e1_norm,
    ];
    let e2 = cross(d, e1);
    let mut items: Vec<(f64, usize, bool)> = Vec::new();
    let mut forward_count = 0;
    for &(_, _, he) in group {
        let forward = mesh.from(he)? == lo;
        if forward {
            forward_count += 1;
        }
        let tri = mesh.tris.slot(he / 3)?;
        let apex = *mesh.pos.slot(corner(tri, (he % 3 + 2) % 3)?)?;
        let mut n = sub(apex, p_lo);
        let along = dot(n, d);
        n = [
            n[0] - along * d[0],
            n[1] - along * d[1],
            n[2] - along * d[2],
        ];
        if norm(n) <= 0.0 {
            return Ok(None);
        }
        items.push((dot(n, e2).atan2(dot(n, e1)), he, forward));
    }
    if forward_count * 2 != group.len() {
        return Ok(None);
    }
    items.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    for (i, item) in items.iter().enumerate() {
        let next = items
            .get((i + 1) % items.len())
            .ok_or(MeshBrepError::Internal("angular order"))?;
        let gap = if i + 1 == items.len() {
            next.0 - item.0 + std::f64::consts::TAU
        } else {
            next.0 - item.0
        };
        if gap < MIN_WEDGE_ANGLE_RAD || item.2 == next.2 {
            return Ok(None);
        }
    }
    Ok(Some(items.into_iter().map(|(_, he, f)| (he, f)).collect()))
}
