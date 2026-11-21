// SPDX-License-Identifier: Apache-2.0

use super::{clearance, point_in_polygon, scanline_midpoints};
use cadmpeg_ir::math::Point2;

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
