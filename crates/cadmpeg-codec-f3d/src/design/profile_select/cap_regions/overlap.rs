// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::TAU;

use super::{
    boundary_use_samples, clearance, point_in_polygon, ring_bounds, Aabb2, CURVE_SAMPLES,
    MAX_SCANLINES,
};
use crate::design::geometry::sketch_geometry_point;
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::features::profiles::SketchProfileRegion;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntity, SketchGeometryDefinition};

/// The sampled rings of each region, the exterior ring first. `None` when a
/// region is not trimmed or a boundary use cannot be evaluated.
fn sampled_rings(
    regions: &[SketchProfileRegion],
    entities: &[SketchEntity],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Vec<Vec<Point2>>>>, CodecError> {
    let mut sampled = Vec::new();
    for region in regions {
        let SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } = region
        else {
            return Ok(None);
        };
        let mut rings = Vec::new();
        for boundary in std::iter::once(outer_boundary).chain(hole_boundaries) {
            let mut ring = Vec::new();
            for use_ in boundary {
                let Some(mut points) = boundary_use_samples(use_, entities, CURVE_SAMPLES, ctx)?
                else {
                    return Ok(None);
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
    Ok(Some(sampled))
}

pub(in crate::design::profile_select) fn regions_are_disjoint(
    regions: &[SketchProfileRegion],
    entities: &[SketchEntity],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(sampled) = sampled_rings(regions, entities, ctx)? else {
        return Ok(false);
    };
    let rings: Vec<Vec<&[Point2]>> = sampled
        .iter()
        .map(|region| region.iter().map(Vec::as_slice).collect())
        .collect();
    Ok(interiors_are_disjoint(&rings, &[], tolerance))
}

/// Whether a point of the line, circle, or circular arc `curve` lies in the
/// interior of one of the trimmed `regions`. The point is inside the sampled
/// exterior ring, outside each sampled hole ring, and farther from every ring
/// of that region than its longest sampled chord. A boundary arc stays within
/// its chord length of the chord that samples it, so the sampled rings and
/// the region boundary put such a point on the same side. `false` for any
/// other curve, a region that is not trimmed, or a curve or boundary that
/// cannot be evaluated.
pub(in crate::design::profile_select) fn curve_enters_region_interior(
    curve: &SketchEntity,
    regions: &[SketchProfileRegion],
    entities: &[SketchEntity],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let [from, to] = match curve.geometry.definition() {
        SketchGeometryDefinition::Line { .. } => [0.0, 1.0],
        SketchGeometryDefinition::Circle { .. } => [0.0, TAU],
        SketchGeometryDefinition::Arc {
            start_angle,
            end_angle,
            ..
        } => [start_angle.get(), end_angle.get()],
        _ => return Ok(false),
    };
    let Some(sampled) = sampled_rings(regions, entities, ctx)? else {
        return Ok(false);
    };
    let mut points = Vec::new();
    for k in 0..=CURVE_SAMPLES {
        let parameter = from + (to - from) * f64::from(k) / f64::from(CURVE_SAMPLES);
        let Some(point) = sketch_geometry_point(&curve.geometry, parameter, ctx)? else {
            return Ok(false);
        };
        points.push(point);
    }
    Ok(sampled.iter().any(|rings| {
        let Some((outer, holes)) = rings.split_first() else {
            return false;
        };
        let margin = rings
            .iter()
            .flat_map(|ring| {
                (0..ring.len()).map(|index| {
                    let next = ring[(index + 1) % ring.len()];
                    let point = ring[index];
                    (next.u - point.u).hypot(next.v - point.v)
                })
            })
            .fold(tolerance, f64::max);
        points.iter().any(|point| {
            point_in_polygon(*point, outer)
                && holes.iter().all(|hole| !point_in_polygon(*point, hole))
                && rings.iter().all(|ring| clearance(*point, ring) > margin)
        })
    }))
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
