// SPDX-License-Identifier: Apache-2.0
//! T-junction repair.
//!
//! Faces tessellated on their own meet along a curved edge with different
//! sample points: a vertex of one face lies on the edge of its neighbour
//! instead of at a vertex. Every such edge is open after welding. The repair
//! splits each unpaired edge at the unpaired-edge vertices that lie on it and
//! fans the triangle from its opposite corner, which keeps the winding.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::convert::{f64_from_index, truncate_f64_to_i64, truncate_f64_to_usize};

use super::geom::{corner, dot, norm, sub, Slot, NONE, V3};
use super::{adjacency, weld};
use super::{
    MeshBrepError, MAX_REPAIR_PROBES, MAX_REPAIR_ROUNDS, REPAIR_PROBES_PER_TRIANGLE,
    REPAIR_SAMPLE_CELL_FACTOR,
};

type Cell = [i64; 3];

/// A vertex found on an unpaired edge, ordered along that edge.
struct OnEdge {
    along: f64,
    vertex: usize,
}

/// Triangles after an edit and the number of edits made.
pub(super) struct Repaired {
    pub(super) tris: Vec<[usize; 3]>,
    pub(super) splits: usize,
}

fn cell_of(p: V3, inverse: f64) -> Result<Cell, MeshBrepError> {
    let mut cell = [0_i64; 3];
    for (slot, coordinate) in cell.iter_mut().zip(p) {
        *slot = truncate_f64_to_i64((coordinate * inverse).floor())
            .ok_or(MeshBrepError::CoordinateTooLarge)?;
    }
    Ok(cell)
}

fn lerp(a: V3, b: V3, t: f64) -> V3 {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Split the unpaired edges of `tris` at the vertices of `positions` that lie
/// within `tolerance` of them, strictly between the end points.
///
/// Each round splits one edge per triangle; rounds repeat until no edge splits
/// or [`MAX_REPAIR_ROUNDS`] passed. A round that would exceed the probe budget
/// ends the repair early: the remaining open edges stay open.
pub(super) fn split_t_junctions(
    positions: &[V3],
    tris: &[[usize; 3]],
    tolerance: f64,
) -> Result<Repaired, MeshBrepError> {
    let mut current = tris.to_vec();
    let mut splits = 0_usize;
    for _ in 0..MAX_REPAIR_ROUNDS {
        let Some(next) = round(positions, &current, tolerance)? else {
            break;
        };
        current = next.tris;
        splits += next.splits;
    }
    Ok(Repaired {
        tris: current,
        splits,
    })
}

/// One repair round; `None` when nothing splits.
fn round(
    positions: &[V3],
    tris: &[[usize; 3]],
    tolerance: f64,
) -> Result<Option<Repaired>, MeshBrepError> {
    let twin = adjacency::build(tris)?.twin;
    let open: Vec<usize> = twin
        .iter()
        .enumerate()
        .filter(|(_, t)| t.is_none())
        .map(|(he, _)| he)
        .collect();
    if open.is_empty() {
        return Ok(None);
    }
    let ends = |he: usize| -> Result<(usize, usize), MeshBrepError> {
        let tri = tris.slot(he / 3)?;
        Ok((corner(tri, he % 3)?, corner(tri, (he % 3 + 1) % 3)?))
    };
    let mut vertices: Vec<usize> = Vec::new();
    let mut lengths: Vec<f64> = Vec::new();
    for &he in &open {
        let (start, end) = ends(he)?;
        vertices.push(start);
        vertices.push(end);
        lengths.push(norm(sub(*positions.slot(start)?, *positions.slot(end)?)));
    }
    vertices.sort_unstable();
    vertices.dedup();
    lengths.sort_unstable_by(f64::total_cmp);
    let median = lengths.get(lengths.len() / 2).copied().unwrap_or(0.0);
    let cell_size = median.max(REPAIR_SAMPLE_CELL_FACTOR * tolerance);
    let inverse = 1.0 / cell_size;
    let mut grid: BTreeMap<Cell, Vec<usize>> = BTreeMap::new();
    for &v in &vertices {
        grid.entry(cell_of(*positions.slot(v)?, inverse)?)
            .or_default()
            .push(v);
    }

    let Some(mut budget) = tris
        .len()
        .checked_mul(REPAIR_PROBES_PER_TRIANGLE)
        .and_then(|probes| probes.checked_add(MAX_REPAIR_PROBES))
    else {
        return Ok(None);
    };
    let mut splits: BTreeMap<usize, Vec<OnEdge>> = BTreeMap::new();
    for &he in &open {
        let (start, end) = ends(he)?;
        let (pu, pv) = (*positions.slot(start)?, *positions.slot(end)?);
        let direction = sub(pv, pu);
        let length = norm(direction);
        if length <= 0.0 {
            continue;
        }
        // Sample spacing of half a cell: every point within half a cell of a
        // sample lies in one of the 27 cells around it.
        let Some(steps) = truncate_f64_to_usize((2.0 * length / cell_size).ceil()) else {
            return Ok(None);
        };
        let steps = steps.max(1);
        let steps_f = f64_from_index(steps).ok_or(MeshBrepError::Internal("step count"))?;
        let third = corner(tris.slot(he / 3)?, (he % 3 + 2) % 3)?;
        // A long edge touches more cells than there are candidate vertices:
        // test every vertex instead.
        let Some(sample_cost) = steps
            .checked_add(1)
            .and_then(|samples| samples.checked_mul(27))
        else {
            return Ok(None);
        };
        let gathered: Vec<usize>;
        let candidates: &[usize] = if sample_cost >= vertices.len() {
            if vertices.len() > budget {
                return Ok(None);
            }
            budget -= vertices.len();
            &vertices
        } else {
            if sample_cost > budget {
                return Ok(None);
            }
            budget -= sample_cost;
            let mut cells: BTreeSet<Cell> = BTreeSet::new();
            for k in 0..=steps {
                let k_f = f64_from_index(k).ok_or(MeshBrepError::Internal("step index"))?;
                let home = cell_of(lerp(pu, pv, k_f / steps_f), inverse)?;
                for dx in -1_i64..=1 {
                    for dy in -1_i64..=1 {
                        for dz in -1_i64..=1 {
                            let (Some(cx), Some(cy), Some(cz)) = (
                                home[0].checked_add(dx),
                                home[1].checked_add(dy),
                                home[2].checked_add(dz),
                            ) else {
                                continue;
                            };
                            cells.insert([cx, cy, cz]);
                        }
                    }
                }
            }
            gathered = cells
                .iter()
                .filter_map(|cell| grid.get(cell))
                .flatten()
                .copied()
                .collect();
            &gathered
        };
        let mut found: Vec<OnEdge> = Vec::new();
        for &w in candidates {
            if w == start || w == end || w == third {
                continue;
            }
            let offset = sub(*positions.slot(w)?, pu);
            let along = dot(offset, direction) / length;
            if along <= 0.0 || along >= length {
                continue;
            }
            let foot = lerp(pu, pv, along / length);
            if norm(sub(*positions.slot(w)?, foot)) <= tolerance {
                found.push(OnEdge { along, vertex: w });
            }
        }
        found.sort_unstable_by(|a, b| a.along.total_cmp(&b.along).then(a.vertex.cmp(&b.vertex)));
        found.dedup_by_key(|f| f.vertex);
        if !found.is_empty() {
            splits.insert(he, found);
        }
    }
    if splits.is_empty() {
        return Ok(None);
    }

    let mut next: Vec<[usize; 3]> = Vec::new();
    let mut split_count = 0_usize;
    for (t, tri) in tris.iter().enumerate() {
        let hit = (0..3).find_map(|k| splits.get(&(3 * t + k)).map(|s| (k, s)));
        let Some((k, points)) = hit else {
            next.push(*tri);
            continue;
        };
        let apex = corner(tri, (k + 2) % 3)?;
        let mut previous = corner(tri, k)?;
        for point in points {
            next.push([previous, point.vertex, apex]);
            previous = point.vertex;
        }
        next.push([previous, corner(tri, (k + 1) % 3)?, apex]);
        split_count += points.len();
    }
    Ok(Some(Repaired {
        tris: next,
        splits: split_count,
    }))
}

/// Merge the end points of unpaired edges that lie within `tolerance` of each
/// other, so that a crack narrower than the weld closes. Triangles that
/// collapse are dropped. Returns the triangles and the number of points
/// merged away, or `None` when nothing merges.
pub(super) fn close_cracks(
    positions: &[V3],
    tris: &[[usize; 3]],
    tolerance: f64,
) -> Result<Option<Repaired>, MeshBrepError> {
    let twin = adjacency::build(tris)?.twin;
    let mut boundary = vec![false; positions.len()];
    for (he, partner) in twin.iter().enumerate() {
        if partner.is_none() {
            let tri = tris.slot(he / 3)?;
            for k in [he % 3, (he % 3 + 1) % 3] {
                *boundary.slot_mut(corner(tri, k)?)? = true;
            }
        }
    }
    let welded = weld::weld_marked(positions, &boundary, tolerance)?;
    let mut representative = vec![NONE; welded.positions.len()];
    let mut merged = 0_usize;
    for (old, &new) in welded.map.iter().enumerate() {
        if new == NONE {
            continue;
        }
        let slot = representative.slot_mut(new)?;
        if *slot == NONE {
            *slot = old;
        } else {
            merged += 1;
        }
    }
    if merged == 0 {
        return Ok(None);
    }
    let remap = |v: usize| -> Result<usize, MeshBrepError> {
        let new = *welded.map.slot(v)?;
        if new == NONE {
            Ok(v)
        } else {
            Ok(*representative.slot(new)?)
        }
    };
    let mut out: Vec<[usize; 3]> = Vec::new();
    for tri in tris {
        let [a, b, c] = *tri;
        let moved = [remap(a)?, remap(b)?, remap(c)?];
        if moved[0] != moved[1] && moved[1] != moved[2] && moved[0] != moved[2] {
            out.push(moved);
        }
    }
    Ok(Some(Repaired {
        tris: out,
        splits: merged,
    }))
}
