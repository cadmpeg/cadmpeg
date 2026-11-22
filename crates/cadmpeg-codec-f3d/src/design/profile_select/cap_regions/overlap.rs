// SPDX-License-Identifier: Apache-2.0

use super::{
    boundary_use_samples, clearance, point_in_polygon, ring_bounds, Aabb2, CURVE_SAMPLES,
    MAX_SCANLINES,
};
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
    let bounds: Vec<Vec<Aabb2>> = regions
        .iter()
        .map(|rings| {
            rings
                .iter()
                .map(|ring| ring_bounds(ring).unwrap_or(Aabb2::EMPTY))
                .collect()
        })
        .collect();
    let contains = |region_index: usize, point: Point2| {
        let rings = &regions[region_index];
        let boxes = &bounds[region_index];
        if !boxes[0].contains(point) {
            return false;
        }
        if !point_in_polygon(point, rings[0]) || clearance(point, rings[0]) <= tolerance {
            return false;
        }
        rings[1..].iter().zip(&boxes[1..]).all(|(hole, hole_box)| {
            !hole_box.expanded(tolerance).contains(point)
                || (!point_in_polygon(point, hole) && clearance(point, hole) > tolerance)
        })
    };
    let mut heights = Vec::new();
    let mut rows = Vec::new();
    let mut crossings = Vec::new();
    for (first_index, first) in regions.iter().enumerate() {
        let first_outer = bounds[first_index][0];
        for (offset, second) in regions[first_index + 1..].iter().enumerate() {
            let second_index = first_index + 1 + offset;
            let Some(overlap) = first_outer.interior_overlap(bounds[second_index][0]) else {
                continue;
            };
            if points.iter().copied().any(|point| {
                overlap.contains(point)
                    && contains(first_index, point)
                    && contains(second_index, point)
            }) {
                return false;
            }
            heights.clear();
            for ring in first.iter().chain(second) {
                heights.extend(ring.iter().map(|point| point.v));
            }
            heights.sort_by(f64::total_cmp);
            heights.dedup();
            rows.clear();
            rows.extend(heights.windows(2).map(|pair| 0.5 * (pair[0] + pair[1])));
            let step = rows.len().div_ceil(MAX_SCANLINES).max(1);
            for &row in rows.iter().step_by(step) {
                if row < overlap.min_v || row > overlap.max_v {
                    continue;
                }
                crossings.clear();
                for (ring, ring_box) in first
                    .iter()
                    .zip(&bounds[first_index])
                    .chain(second.iter().zip(&bounds[second_index]))
                {
                    if row < ring_box.min_v || row > ring_box.max_v {
                        continue;
                    }
                    for index in 0..ring.len() {
                        let start = ring[index];
                        let end = ring[(index + 1) % ring.len()];
                        if (start.v - row) * (end.v - row) < 0.0 {
                            crossings.push(
                                start.u + (end.u - start.u) * (row - start.v) / (end.v - start.v),
                            );
                        }
                    }
                }
                crossings.sort_by(f64::total_cmp);
                for pair in crossings.windows(2) {
                    let mid_u = 0.5 * (pair[0] + pair[1]);
                    if mid_u < overlap.min_u || mid_u > overlap.max_u {
                        continue;
                    }
                    let point = Point2::new(mid_u, row);
                    if contains(first_index, point) && contains(second_index, point) {
                        return false;
                    }
                }
            }
        }
    }
    true
}
