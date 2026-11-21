// SPDX-License-Identifier: Apache-2.0

use super::{boundary_use_samples, clearance, point_in_polygon, scanline_midpoints, CURVE_SAMPLES};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::features::profiles::SketchProfileRegion;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::SketchEntity;

pub(in crate::design::profile_select) fn regions_are_disjoint(
    regions: &[SketchProfileRegion],
    entities: &[SketchEntity],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let mut sampled = Vec::new();
    for region in regions {
        let SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } = region
        else {
            return Ok(false);
        };
        let mut rings = Vec::new();
        for boundary in std::iter::once(outer_boundary).chain(hole_boundaries) {
            let mut ring = Vec::new();
            for use_ in boundary {
                let Some(mut points) = boundary_use_samples(use_, entities, CURVE_SAMPLES, ctx)?
                else {
                    return Ok(false);
                };
                if use_.reversed {
                    points.reverse();
                }
                ring.extend(points);
            }
            rings.push(ring);
        }
        sampled.push(rings);
    }
    let rings: Vec<Vec<&[Point2]>> = sampled
        .iter()
        .map(|region| region.iter().map(Vec::as_slice).collect())
        .collect();
    Ok(interiors_are_disjoint(&rings, &[], tolerance))
}

pub(super) fn interiors_are_disjoint(
    regions: &[Vec<&[Point2]>],
    points: &[Point2],
    tolerance: f64,
) -> bool {
    if regions
        .iter()
        .any(|rings| rings.is_empty() || rings.iter().any(|ring| ring.len() < 3))
    {
        return false;
    }
    let contains = |rings: &[&[Point2]], point: Point2| {
        point_in_polygon(point, rings[0])
            && clearance(point, rings[0]) > tolerance
            && rings[1..]
                .iter()
                .all(|hole| !point_in_polygon(point, hole) && clearance(point, hole) > tolerance)
    };
    for (index, first) in regions.iter().enumerate() {
        for second in &regions[index + 1..] {
            let rings: Vec<_> = first.iter().chain(second).copied().collect();
            if points
                .iter()
                .copied()
                .chain(scanline_midpoints(&rings))
                .any(|point| contains(first, point) && contains(second, point))
            {
                return false;
            }
        }
    }
    true
}
