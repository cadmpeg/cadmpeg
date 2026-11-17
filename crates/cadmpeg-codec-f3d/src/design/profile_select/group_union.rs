// SPDX-License-Identifier: Apache-2.0
//! A typed-reference region group whose first region is the union of its
//! further regions.

use cadmpeg_ir::features::{SketchProfileBoundaryUse, SketchProfileRegion};

/// Remove and return the region that a group's first stored region added
/// when the regions its further stored regions added are the atomic regions
/// of that first region.
///
/// `regions[group_start..first_end]` are the regions the first stored region
/// added, and `regions[first_end..]` the regions the further stored regions
/// added. The first region is removed only when it added exactly one region,
/// the further regions added at least two, no region has holes, and the
/// directed outer intervals of the further regions, after opposite uses of
/// the same interval cancel, are exactly the outer intervals of the first
/// region.
pub(super) fn remove_union_first_region(
    regions: &mut Vec<SketchProfileRegion>,
    group_start: usize,
    first_end: usize,
) -> Option<SketchProfileRegion> {
    if first_end.checked_sub(group_start) != Some(1) {
        return None;
    }
    let further = regions.get(first_end..)?;
    if further.len() < 2 {
        return None;
    }
    let first = outer_without_holes(regions.get(group_start)?)?;
    let mut remaining: Vec<&SketchProfileBoundaryUse> = Vec::new();
    for region in further {
        let outer = outer_without_holes(region)?;
        for use_ in outer {
            if let Some(opposite) = remaining.iter().position(|other| {
                other.entity == use_.entity
                    && other.parameter_range == use_.parameter_range
                    && other.reversed != use_.reversed
            }) {
                remaining.swap_remove(opposite);
            } else {
                remaining.push(use_);
            }
        }
    }
    if remaining.len() != first.len() {
        return None;
    }
    for use_ in first {
        let found = remaining.iter().position(|other| *other == use_)?;
        remaining.swap_remove(found);
    }
    Some(regions.remove(group_start))
}

fn outer_without_holes(region: &SketchProfileRegion) -> Option<&[SketchProfileBoundaryUse]> {
    match region {
        SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } if hole_boundaries.is_empty() => Some(outer_boundary),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
