// SPDX-License-Identifier: Apache-2.0
//! A typed-reference region group whose first region is the union of its
//! further regions.

use crate::design::geometry::ProfileGeometryWork;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::profiles::{SketchProfileBoundaryUse, SketchProfileRegion};
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId};

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
    let mut remaining = union_outer_uses(further)?;
    if remaining.len() != first.len() {
        return None;
    }
    for use_ in first {
        let found = remaining.iter().position(|other| *other == use_)?;
        remaining.swap_remove(found);
    }
    Some(regions.remove(group_start))
}

/// Whether the further regions of a group are the atomic regions of its first
/// stored region when that region's loop resolves to no face of the Sketch.
///
/// `further` are the regions the further stored regions resolve to, each
/// distinct region counted once, and `first` names the curve of each entry of
/// the first region's single exterior loop with the entry's piece ordinal and
/// piece count. The further regions are at least two, no region has holes,
/// and the directed outer intervals of the further regions remain after
/// opposite uses of the same interval cancel. At least one interval remains,
/// and every remaining interval belongs to a curve that `first` names. The
/// piece that each entry of `first` names lies on the remaining intervals, as
/// a coincident curve does.
pub(super) fn further_regions_compose_first(
    further: &[SketchProfileRegion],
    first: &[(SketchEntityId, [u32; 2])],
    entities: &[SketchEntity],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let mut distinct: Vec<SketchProfileRegion> = Vec::new();
    for region in further {
        if !distinct.contains(region) {
            distinct.push(region.clone());
        }
    }
    if distinct.len() < 2 || first.is_empty() {
        return Ok(false);
    }
    let Some(remaining) = union_outer_uses(&distinct) else {
        return Ok(false);
    };
    if remaining.is_empty()
        || !remaining
            .iter()
            .all(|use_| first.iter().any(|(curve, _)| *curve == use_.entity))
    {
        return Ok(false);
    }
    let boundary = remaining.into_iter().cloned().collect::<Vec<_>>();
    crate::design::geometry::member_loop_union::first_region_pieces_lie_on_boundary(
        entities, first, &boundary, tolerance, work, ctx,
    )
}

/// The directed outer intervals of `regions` that remain after opposite uses
/// of the same curve interval cancel. `None` when a region has holes.
fn union_outer_uses(regions: &[SketchProfileRegion]) -> Option<Vec<&SketchProfileBoundaryUse>> {
    let mut remaining: Vec<&SketchProfileBoundaryUse> = Vec::new();
    for region in regions {
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
    Some(remaining)
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
