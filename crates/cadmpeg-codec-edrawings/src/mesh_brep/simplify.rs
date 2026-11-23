// SPDX-License-Identifier: Apache-2.0
//! Choice of the vertices that survive as B-rep vertices.

use super::geom::{point_segment_distance, Slot};
use super::loops::Loops;
use super::mesh::Mesh;
use super::{MeshBrepError, COLLINEAR_TOLERANCE_MM};

/// Vertices on any face boundary.
pub(super) fn boundary_vertices(mesh: &Mesh, loops: &Loops) -> Result<Vec<bool>, MeshBrepError> {
    let mut on_boundary = vec![false; mesh.pos.len()];
    for (he, &flag) in loops.boundary.iter().enumerate() {
        if flag {
            *on_boundary.slot_mut(mesh.from(he)?)? = true;
        }
    }
    Ok(on_boundary)
}

/// Decide which boundary vertices stay.
///
/// A vertex that exactly two edges meet at is a candidate for removal: both
/// edges then lie between the same two faces (or bound the same lone face) on
/// both sides, so every face drops it consistently. A maximal chain of such
/// vertices is merged into one edge only where every dropped vertex lies
/// within [`COLLINEAR_TOLERANCE_MM`] of the merged straight segment; a
/// Douglas-Peucker pass keeps the vertices that violate it. Vertices of any
/// other degree (corners, pinch points, junctions) always stay.
pub(super) fn select_kept(
    mesh: &Mesh,
    twin: &[Option<usize>],
    loops: &Loops,
    on_boundary: &[bool],
) -> Result<Vec<bool>, MeshBrepError> {
    let vertex_count = mesh.pos.len();
    let mut degree = (0..vertex_count).map(|_| 0_u32).collect::<Vec<_>>();
    for (he, &flag) in loops.boundary.iter().enumerate() {
        if !flag {
            continue;
        }
        let counted = match *twin.slot(he)? {
            Some(other) => he < other,
            None => true,
        };
        if counted {
            *degree.slot_mut(mesh.from(he)?)? += 1;
            *degree.slot_mut(mesh.to(he)?)? += 1;
        }
    }
    let mut keep = Vec::with_capacity(vertex_count);
    for (&on, &deg) in on_boundary.iter().zip(&degree) {
        keep.push(on && deg != 2);
    }
    let mut decided = (0..vertex_count).map(|_| false).collect::<Vec<_>>();
    for region in &loops.regions {
        for ring in &region.loops {
            let mut verts = Vec::with_capacity(ring.len());
            for &he in ring {
                verts.push(mesh.from(he)?);
            }
            simplify_ring(mesh, &verts, &mut keep, &mut decided)?;
        }
    }
    Ok(keep)
}

fn simplify_ring(
    mesh: &Mesh,
    verts: &[usize],
    keep: &mut [bool],
    decided: &mut [bool],
) -> Result<(), MeshBrepError> {
    let len = verts.len();
    let mut anchor = None;
    for (position, &vertex) in verts.iter().enumerate() {
        if *keep.slot(vertex)? {
            anchor = Some(position);
            break;
        }
    }
    let first = match anchor {
        Some(position) => position,
        None => {
            // Every vertex is a removal candidate: keep one so the loop has
            // an edge to carry.
            let vertex = *verts.first().ok_or(MeshBrepError::Internal("empty loop"))?;
            *keep.slot_mut(vertex)? = true;
            0
        }
    };
    let mut walked = 0;
    let mut at = first;
    while walked < len {
        let mut chain = vec![*verts.slot(at)?];
        let mut next = (at + 1) % len;
        loop {
            let vertex = *verts.slot(next)?;
            chain.push(vertex);
            if *keep.slot(vertex)? || chain.len() > len + 1 {
                break;
            }
            next = (next + 1) % len;
        }
        walked += chain.len() - 1;
        at = next;
        let interior = chain
            .get(1..chain.len() - 1)
            .ok_or(MeshBrepError::Internal("chain interior"))?;
        let Some(&probe) = interior.first() else {
            continue;
        };
        if *decided.slot(probe)? {
            continue;
        }
        for &vertex in interior {
            *decided.slot_mut(vertex)? = true;
        }
        douglas_peucker(mesh, &chain, keep)?;
    }
    Ok(())
}

/// Keep the chain vertices that stray more than the tolerance from the
/// segment joining the chain's retained neighbours.
fn douglas_peucker(mesh: &Mesh, chain: &[usize], keep: &mut [bool]) -> Result<(), MeshBrepError> {
    let mut stack = vec![(0_usize, chain.len() - 1)];
    while let Some((low, high)) = stack.pop() {
        let a = *mesh.pos.slot(*chain.slot(low)?)?;
        let b = *mesh.pos.slot(*chain.slot(high)?)?;
        let mut worst = None;
        let mut worst_distance = COLLINEAR_TOLERANCE_MM;
        for index in low + 1..high {
            let p = *mesh.pos.slot(*chain.slot(index)?)?;
            let distance = point_segment_distance(p, a, b);
            if distance > worst_distance {
                worst_distance = distance;
                worst = Some(index);
            }
        }
        if let Some(index) = worst {
            *keep.slot_mut(*chain.slot(index)?)? = true;
            stack.push((low, index));
            stack.push((index, high));
        }
    }
    Ok(())
}
