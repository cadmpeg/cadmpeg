// SPDX-License-Identifier: Apache-2.0
//! Face boundary loops.

use super::geom::{cross, dot, sub, Slot};
use super::mesh::{next_in_triangle, Mesh};
use super::regions::Regions;
use super::MeshBrepError;

/// Boundary loops of one region, as ring-ordered half-edge lists.
pub(super) struct RegionLoops {
    pub(super) loops: Vec<Vec<usize>>,
    /// Index into `loops` of the outer loop: the only loop that runs
    /// counter-clockwise about the region normal. `None` when there is no such
    /// single loop (the face then lists its loops unclassified).
    pub(super) outer: Option<usize>,
}

impl Loops {
    /// Regions with loops but no unique counter-clockwise loop.
    pub(super) fn ambiguous_regions(&self) -> Vec<usize> {
        self.regions
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.loops.is_empty() && r.outer.is_none())
            .map(|(i, _)| i)
            .collect()
    }
}

/// All loops plus the boundary flag of every half-edge.
pub(super) struct Loops {
    pub(super) regions: Vec<RegionLoops>,
    /// Whether the half-edge has no twin in its own region.
    pub(super) boundary: Vec<bool>,
}

/// Trace the boundary of every region.
///
/// A boundary half-edge `a -> b` is followed by the next boundary half-edge
/// leaving `b` that the same region's triangle fan around `b` reaches. The fan
/// walk is purely combinatorial, so at a pinch vertex (several boundary
/// half-edges of one region leave it) each sector of the region closes its
/// own loop and loops never cross.
pub(super) fn trace(
    mesh: &Mesh,
    twin: &[Option<usize>],
    regions: &Regions,
) -> Result<Loops, MeshBrepError> {
    let half_edges = mesh.tris.len() * 3;
    let mut boundary = (0..half_edges).map(|_| false).collect::<Vec<_>>();
    for (he, flag) in boundary.iter_mut().enumerate() {
        *flag = match *twin.slot(he)? {
            None => true,
            Some(other) => regions.region_of.slot(he / 3)? != regions.region_of.slot(other / 3)?,
        };
    }
    let mut result: Vec<RegionLoops> = regions
        .list
        .iter()
        .map(|_| RegionLoops {
            loops: Vec::new(),
            outer: None,
        })
        .collect();
    let mut visited = (0..half_edges).map(|_| false).collect::<Vec<_>>();
    for start in 0..half_edges {
        if *visited.slot(start)? || !*boundary.slot(start)? {
            continue;
        }
        let mut ring = Vec::new();
        let mut current = start;
        loop {
            *visited.slot_mut(current)? = true;
            ring.push(current);
            current = next_boundary(current, &boundary, twin, half_edges)?;
            if current == start {
                break;
            }
            if *visited.slot(current)? {
                return Err(MeshBrepError::Internal(
                    "boundary walk is not a permutation",
                ));
            }
        }
        let region = *regions.region_of.slot(start / 3)?;
        result.slot_mut(region)?.loops.push(ring);
    }
    for (index, entry) in result.iter_mut().enumerate() {
        let normal = regions.list.slot(index)?.normal;
        let mut positive = None;
        let mut positives = 0_usize;
        for (loop_index, ring) in entry.loops.iter().enumerate() {
            if signed_area(mesh, ring, normal)? > 0.0 {
                positives += 1;
                positive = Some(loop_index);
            }
        }
        entry.outer = if positives == 1 { positive } else { None };
    }
    Ok(Loops {
        regions: result,
        boundary,
    })
}

fn next_boundary(
    he: usize,
    boundary: &[bool],
    twin: &[Option<usize>],
    limit: usize,
) -> Result<usize, MeshBrepError> {
    let mut current = he;
    for _ in 0..=limit {
        let candidate = next_in_triangle(current);
        if *boundary.slot(candidate)? {
            return Ok(candidate);
        }
        current = twin
            .slot(candidate)?
            .ok_or(MeshBrepError::Internal("interior half-edge without twin"))?;
    }
    Err(MeshBrepError::Internal("triangle fan does not terminate"))
}

/// Area of a loop about `normal`; positive when counter-clockwise.
fn signed_area(mesh: &Mesh, ring: &[usize], normal: [f64; 3]) -> Result<f64, MeshBrepError> {
    let Some(&first) = ring.first() else {
        return Ok(0.0);
    };
    let base = *mesh.pos.slot(mesh.from(first)?)?;
    let mut sum = [0.0; 3];
    for &he in ring {
        let a = sub(*mesh.pos.slot(mesh.from(he)?)?, base);
        let b = sub(*mesh.pos.slot(mesh.to(he)?)?, base);
        let c = cross(a, b);
        sum = [sum[0] + c[0], sum[1] + c[1], sum[2] + c[2]];
    }
    Ok(0.5 * dot(sum, normal))
}
