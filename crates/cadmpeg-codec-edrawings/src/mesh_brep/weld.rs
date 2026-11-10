// SPDX-License-Identifier: Apache-2.0
//! Deterministic spatial-grid vertex welding.

use std::collections::BTreeMap;

use cadmpeg_core::convert::truncate_f64_to_i64;

use super::geom::{sub, Slot, NONE, V3};
use super::MeshBrepError;

/// Welded vertex table.
pub(super) struct Welded {
    /// Representative positions, numbered in order of first use.
    pub(super) positions: Vec<V3>,
    /// Input index to welded index; `NONE` for unreferenced input points.
    pub(super) map: Vec<usize>,
}

type Cell = [i64; 3];

fn cell_of(p: V3, inverse: f64) -> Result<Cell, MeshBrepError> {
    let mut cell = [0_i64; 3];
    for (slot, coordinate) in cell.iter_mut().zip(p) {
        *slot = truncate_f64_to_i64((coordinate * inverse).floor())
            .ok_or(MeshBrepError::CoordinateTooLarge)?;
    }
    Ok(cell)
}

/// Weld every point referenced by `triangles` to the lowest-numbered
/// representative within `tolerance` (Euclidean). Points are visited in input
/// order and the 27 grid cells around each point are searched, so two points
/// closer than the tolerance weld even when they straddle a cell boundary.
pub(super) fn weld(
    positions: &[V3],
    triangles: &[[u32; 3]],
    tolerance: f64,
) -> Result<Welded, MeshBrepError> {
    let mut used = vec![false; positions.len()];
    for triangle in triangles {
        for &index in triangle {
            let slot = usize::try_from(index)
                .ok()
                .and_then(|i| used.get_mut(i))
                .ok_or(MeshBrepError::Internal("triangle index out of range"))?;
            *slot = true;
        }
    }
    weld_marked(positions, &used, tolerance)
}

/// Weld the points flagged in `used` to the lowest-numbered representative
/// within `tolerance`; unflagged points map to `NONE`.
pub(super) fn weld_marked(
    positions: &[V3],
    used: &[bool],
    tolerance: f64,
) -> Result<Welded, MeshBrepError> {
    let inverse = 1.0 / tolerance;
    let limit = tolerance * tolerance;
    let mut grid: BTreeMap<Cell, Vec<usize>> = BTreeMap::new();
    let mut welded = Welded {
        positions: Vec::new(),
        map: vec![NONE; positions.len()],
    };
    for (index, (&position, &is_used)) in positions.iter().zip(used).enumerate() {
        if !is_used {
            continue;
        }
        let cell = cell_of(position, inverse)?;
        let mut best = NONE;
        for dx in -1_i64..=1 {
            for dy in -1_i64..=1 {
                for dz in -1_i64..=1 {
                    let (Some(x), Some(y), Some(z)) = (
                        cell[0].checked_add(dx),
                        cell[1].checked_add(dy),
                        cell[2].checked_add(dz),
                    ) else {
                        continue;
                    };
                    let Some(members) = grid.get(&[x, y, z]) else {
                        continue;
                    };
                    for &candidate in members {
                        let delta = sub(*welded.positions.slot(candidate)?, position);
                        let distance2 =
                            delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2];
                        if distance2 <= limit && candidate < best {
                            best = candidate;
                        }
                    }
                }
            }
        }
        let target = if best == NONE {
            let id = welded.positions.len();
            welded.positions.push(position);
            grid.entry(cell).or_default().push(id);
            id
        } else {
            best
        };
        *welded.map.slot_mut(index)? = target;
    }
    Ok(welded)
}
