// SPDX-License-Identifier: Apache-2.0
//! Geometry-backed boundary-role derivation shared by closed topology routes.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::LoopId;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::topology::FaceLoops;

const EPS_PLANAR_COORDINATE: f64 = 1.0e-10;

fn strictly_inside_planar_polygon(
    ctx: &DecodeContext<'_>,
    point: Point2,
    polygon: &[Point2],
    tolerance: f64,
) -> Result<bool, CodecError> {
    let mut inside = false;
    let strict = ctx.all_by(
        polygon
            .iter()
            .zip(polygon.iter().cycle().skip(1))
            .take(polygon.len()),
        |(left, right)| {
            let edge_u = right.u - left.u;
            let edge_v = right.v - left.v;
            let point_u = point.u - left.u;
            let point_v = point.v - left.v;
            let edge_length = edge_u.hypot(edge_v);
            let cross = edge_u * point_v - edge_v * point_u;
            let dot = point_u * (point.u - right.u) + point_v * (point.v - right.v);
            if edge_length > 0.0
                && cross.abs() <= tolerance * edge_length
                && dot <= tolerance * tolerance
            {
                return Ok(false);
            }
            if (left.v > point.v) != (right.v > point.v) {
                let intersection = left.u + (point.v - left.v) * edge_u / (right.v - left.v);
                if intersection > point.u {
                    inside = !inside;
                }
            }
            Ok(true)
        },
        "catia_boundary_containment_edges",
    )?;
    Ok(strict && inside)
}

fn point_on_segment(point: Point2, left: Point2, right: Point2, tolerance: f64) -> bool {
    let edge_u = right.u - left.u;
    let edge_v = right.v - left.v;
    let point_u = point.u - left.u;
    let point_v = point.v - left.v;
    let edge_length = edge_u.hypot(edge_v);
    edge_length > 0.0
        && (edge_u * point_v - edge_v * point_u).abs() <= tolerance * edge_length
        && point_u * (point.u - right.u) + point_v * (point.v - right.v) <= tolerance * tolerance
}

fn segments_intersect_or_touch(
    left_start: Point2,
    left_end: Point2,
    right_start: Point2,
    right_end: Point2,
    tolerance: f64,
) -> bool {
    let min_u = left_start
        .u
        .min(left_end.u)
        .max(right_start.u.min(right_end.u) - tolerance);
    let max_u = left_start
        .u
        .max(left_end.u)
        .min(right_start.u.max(right_end.u) + tolerance);
    let min_v = left_start
        .v
        .min(left_end.v)
        .max(right_start.v.min(right_end.v) - tolerance);
    let max_v = left_start
        .v
        .max(left_end.v)
        .min(right_start.v.max(right_end.v) + tolerance);
    if min_u > max_u || min_v > max_v {
        return false;
    }
    if [
        (right_start, left_start, left_end),
        (right_end, left_start, left_end),
        (left_start, right_start, right_end),
        (left_end, right_start, right_end),
    ]
    .into_iter()
    .any(|(point, start, end)| point_on_segment(point, start, end, tolerance))
    {
        return true;
    }
    let orientation = |first: Point2, second: Point2, third: Point2| {
        (second.u - first.u) * (third.v - first.v) - (second.v - first.v) * (third.u - first.u)
    };
    let left_left = orientation(left_start, left_end, right_start);
    let left_right = orientation(left_start, left_end, right_end);
    let right_left = orientation(right_start, right_end, left_start);
    let right_right = orientation(right_start, right_end, left_end);
    ((left_left > tolerance && left_right < -tolerance)
        || (left_left < -tolerance && left_right > tolerance))
        && ((right_left > tolerance && right_right < -tolerance)
            || (right_left < -tolerance && right_right > tolerance))
}

fn polygon_boundaries_intersect(
    ctx: &DecodeContext<'_>,
    left: &[Point2],
    right: &[Point2],
    tolerance: f64,
    same_polygon: bool,
) -> Result<bool, CodecError> {
    ctx.any_by(
        left.iter()
            .zip(left.iter().cycle().skip(1))
            .take(left.len())
            .enumerate(),
        |(left_index, (&left_start, &left_end))| {
            let first_right = if same_polygon { left_index + 1 } else { 0 };
            ctx.any_by(
                first_right..right.len(),
                |right_index| {
                    if same_polygon
                        && ((left_index + 1) % left.len() == right_index
                            || (right_index + 1) % right.len() == left_index)
                    {
                        return Ok(false);
                    }
                    let right_start = right[right_index];
                    let right_end = right[(right_index + 1) % right.len()];
                    Ok(segments_intersect_or_touch(
                        left_start,
                        left_end,
                        right_start,
                        right_end,
                        tolerance,
                    ))
                },
                "catia_boundary_segment_pairs",
            )
        },
        "catia_boundary_segment_edges",
    )
}

/// Classify complete planar boundary polygons by strict containment.
///
/// Each row pairs a loop id with the closed polygon of that boundary, so the
/// classification and the loop list are one value and cannot disagree in
/// length or order. A single boundary is the outer boundary by the face
/// invariant. Multiple boundaries are classified only when one unique largest
/// non-degenerate polygon strictly contains every other polygon. This
/// deliberately declines disjoint, touching, nested-hole, malformed, and
/// non-planar arrangements.
pub(crate) fn classify_planar_boundaries(
    ctx: &DecodeContext<'_>,
    surface: &SurfaceGeometry,
    rows: &[(LoopId, Vec<Point3>)],
) -> Result<FaceLoops, CodecError> {
    let unspecified = || -> Result<FaceLoops, CodecError> {
        let mut ids = Vec::new();
        for (id, _) in ctx.admit_iter(rows, "catia_boundary_unspecified_rows")? {
            let id = id.try_clone_for_decode(ctx, "catia_boundary_unspecified_id_copy")?;
            ctx.push_vec(&mut ids, id, "catia_boundary_unspecified_ids")?;
        }
        Ok(FaceLoops::unspecified(ids))
    };
    if let [(single, _)] = rows {
        let id = single.try_clone_for_decode(ctx, "catia_boundary_single_id_copy")?;
        return Ok(FaceLoops::classified(id, Vec::new()));
    }
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) = surface else {
        return unspecified();
    };
    let origin = plane_surface.origin().get();
    let u_axis = *plane_surface.frame().reference().as_raw();
    let v_axis = *plane_surface.frame().binormal().as_raw();
    let normal = *plane_surface.frame().axis().as_raw();
    let (projected, polygon_storage) =
        ctx.with_scoped_storage("catia_boundary_polygons", || {
            let mut polygons = Vec::new();
            let mut areas = Vec::new();
            let mut coordinate_scale = 1.0_f64;
            let complete = ctx.all_by(
                rows,
                |(_, boundary)| {
                    if boundary.len() < 3 {
                        return Ok(false);
                    }
                    let mut polygon = Vec::<Point2>::new();
                    let mut area = 0.0;
                    let complete = ctx.all_by(
                        boundary,
                        |point| {
                            let offset = point.vector_from(origin);
                            let u = offset.dot(u_axis);
                            let v = offset.dot(v_axis);
                            let distance = offset.dot(normal);
                            let scale = 1.0_f64.max(u.abs()).max(v.abs());
                            if !u.is_finite()
                                || !v.is_finite()
                                || !distance.is_finite()
                                || distance.abs() > EPS_PLANAR_COORDINATE * scale
                            {
                                return Ok(false);
                            }
                            coordinate_scale = coordinate_scale.max(u.abs()).max(v.abs());
                            if let Some(left) = polygon.last() {
                                area += left.u * v - u * left.v;
                            }
                            ctx.push_vec(
                                &mut polygon,
                                Point2::new(u, v),
                                "catia_boundary_polygon_points",
                            )?;
                            Ok(true)
                        },
                        "catia_boundary_projection_points",
                    )?;
                    if !complete {
                        return Ok(false);
                    }
                    if let (Some(left), Some(right)) = (polygon.last(), polygon.first()) {
                        area += left.u * right.v - right.u * left.v;
                    }
                    ctx.push_vec(&mut polygons, polygon, "catia_boundary_polygon_rows")?;
                    ctx.push_vec(&mut areas, area * 0.5, "catia_boundary_polygon_areas")?;
                    Ok(true)
                },
                "catia_boundary_projection_rows",
            )?;
            Ok::<_, CodecError>(complete.then_some((polygons, areas, coordinate_scale)))
        })?;
    let Some((polygons, areas, coordinate_scale)) = projected else {
        drop(polygon_storage);
        return unspecified();
    };
    let coordinate_tolerance = EPS_PLANAR_COORDINATE * coordinate_scale;
    let area_tolerance = coordinate_tolerance * coordinate_scale;
    let mut largest: Option<(usize, f64)> = None;
    if !ctx.all_by(
        areas.iter().enumerate(),
        |(index, area)| {
            if !area.is_finite() || area.abs() <= area_tolerance {
                return Ok(false);
            }
            let area = area.abs();
            if largest.is_none_or(|(_, prior)| !area.total_cmp(&prior).is_lt()) {
                largest = Some((index, area));
            }
            Ok(true)
        },
        "catia_boundary_area_search",
    )? {
        return unspecified();
    }
    let Some((outer, outer_area)) = largest else {
        return unspecified();
    };
    if ctx.any_by(
        polygons.iter().enumerate(),
        |(index, polygon)| {
            Ok(
                polygon_boundaries_intersect(ctx, polygon, polygon, coordinate_tolerance, true)?
                    || ctx.any_by(
                        &polygons[index + 1..],
                        |other| {
                            polygon_boundaries_intersect(
                                ctx,
                                polygon,
                                other,
                                coordinate_tolerance,
                                false,
                            )
                        },
                        "catia_boundary_polygon_pairs",
                    )?,
            )
        },
        "catia_boundary_polygon_intersections",
    )? {
        return unspecified();
    }
    if ctx.any_by(
        areas.iter().enumerate(),
        |(index, area)| Ok(index != outer && outer_area - area.abs() <= area_tolerance),
        "catia_boundary_outer_uniqueness",
    )? {
        return unspecified();
    }
    if ctx.any_by(
        polygons.iter().enumerate(),
        |(index, polygon)| {
            Ok(index != outer
                && ctx.any_by(
                    polygon,
                    |point| {
                        Ok(!strictly_inside_planar_polygon(
                            ctx,
                            *point,
                            &polygons[outer],
                            coordinate_tolerance,
                        )?)
                    },
                    "catia_boundary_outer_point_search",
                )?)
        },
        "catia_boundary_outer_containment",
    )? {
        return unspecified();
    }
    if ctx.any_by(
        polygons.iter().enumerate(),
        |(index, polygon)| {
            Ok(index != outer
                && ctx.any_by(
                    polygons.iter().enumerate(),
                    |(other_index, other)| {
                        Ok(other_index != outer
                            && other_index != index
                            && ctx.any_by(
                                polygon,
                                |point| {
                                    strictly_inside_planar_polygon(
                                        ctx,
                                        *point,
                                        other,
                                        coordinate_tolerance,
                                    )
                                },
                                "catia_boundary_hole_point_search",
                            )?)
                    },
                    "catia_boundary_hole_pairs",
                )?)
        },
        "catia_boundary_hole_containment",
    )? {
        return unspecified();
    }
    let Some((outer_id, _)) = rows.get(outer) else {
        return unspecified();
    };
    let mut inner = Vec::new();
    for (index, (id, _)) in ctx
        .admit_iter(rows, "catia_boundary_inner_rows")?
        .enumerate()
    {
        if index != outer {
            let id = id.try_clone_for_decode(ctx, "catia_boundary_inner_id_copy")?;
            ctx.push_vec(&mut inner, id, "catia_boundary_inner_ids")?;
        }
    }
    let outer_id = outer_id.try_clone_for_decode(ctx, "catia_boundary_outer_id_copy")?;
    Ok(FaceLoops::classified(outer_id, inner))
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::LoopId;
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::FaceLoops;

    fn classify_planar_boundaries(
        surface: &SurfaceGeometry,
        rows: &[(cadmpeg_ir::ids::LoopId, Vec<Point3>)],
    ) -> FaceLoops {
        crate::test_support::with_service_context(|ctx| {
            super::classify_planar_boundaries(ctx, surface, rows)
        })
        .expect("service context admits boundary classification")
    }

    fn plane() -> SurfaceGeometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        ))
    }

    fn loop_id(index: usize) -> LoopId {
        LoopId::mint(format!("catia:test:loop#{index}")).expect("identity grammar")
    }

    fn square(min_u: f64, min_v: f64, max_u: f64, max_v: f64) -> Vec<Point3> {
        [
            [min_u, min_v, 0.0],
            [max_u, min_v, 0.0],
            [max_u, max_v, 0.0],
            [min_u, max_v, 0.0],
        ]
        .into_iter()
        .map(|[u, v, w]| Point3::new(u, v, w))
        .collect()
    }

    fn rows(boundaries: Vec<Vec<Point3>>) -> Vec<(LoopId, Vec<Point3>)> {
        boundaries
            .into_iter()
            .enumerate()
            .map(|(index, boundary)| (loop_id(index), boundary))
            .collect()
    }

    #[test]
    fn self_intersection_visits_each_unordered_edge_pair_once() {
        use cadmpeg_ir::math::Point2;
        let square = [
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(0.0, 1.0),
        ];
        // Four outer visits and their end probe; six unordered pairs and
        // the four inner end probes. Adjacent pairs need only a fixed check.
        let result = crate::test_support::with_work_limit(15, |ctx| {
            super::polygon_boundaries_intersect(ctx, &square, &square, 0.0, true)
        })
        .expect("one visit per unordered pair and end probe");
        assert!(!result);
        let refusal =
            crate::test_support::with_work_refusal("catia_boundary_segment_pairs", |ctx| {
                let result = super::polygon_boundaries_intersect(ctx, &square, &square, 0.0, true);
                if let Err(cadmpeg_core::CodecError::ResourceLimit(ref limit)) = result {
                    assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                }
                result
            });
        assert!(
            matches!(refusal, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "catia_boundary_segment_pairs")
        );
    }

    #[test]
    fn planar_boundary_comparisons_refuse_work() {
        let boundaries = rows(vec![
            square(0.0, 0.0, 10.0, 10.0),
            square(2.0, 2.0, 3.0, 3.0),
        ]);
        let result =
            crate::test_support::with_work_refusal("catia_boundary_segment_pairs", |ctx| {
                let result = super::classify_planar_boundaries(ctx, &plane(), &boundaries);
                if let Err(cadmpeg_core::CodecError::ResourceLimit(ref limit)) = result {
                    assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                }
                result
            });
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "catia_boundary_segment_pairs")
        );
    }

    #[test]
    fn planar_boundary_points_refuse_before_nested_polygon_allocation() {
        let boundaries = rows(vec![square(0.0, 0.0, 1.0, 1.0), square(3.0, 0.0, 4.0, 1.0)]);
        let limited = crate::test_support::with_collection_limit(3, |ctx| {
            super::classify_planar_boundaries(ctx, &plane(), &boundaries)
        });
        assert!(matches!(
            limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        assert_eq!(
            classify_planar_boundaries(&plane(), &boundaries),
            FaceLoops::unspecified(vec![loop_id(0), loop_id(1)])
        );
    }

    #[test]
    fn planar_boundaries_decline_an_off_plane_hole() {
        let outer = square(0.0, 0.0, 10.0, 10.0);
        let mut inner = square(2.0, 2.0, 3.0, 3.0);
        for point in &mut inner {
            point.z = 10.0;
        }
        assert_eq!(
            classify_planar_boundaries(&plane(), &rows(vec![outer, inner])),
            FaceLoops::unspecified(vec![loop_id(0), loop_id(1)])
        );
    }

    #[test]
    fn one_boundary_is_outer() {
        assert_eq!(
            classify_planar_boundaries(&plane(), &rows(vec![square(0.0, 0.0, 1.0, 1.0)])),
            FaceLoops::classified(loop_id(0), Vec::new())
        );
    }

    #[test]
    fn containment_classifies_outer_and_hole_independent_of_order() {
        assert_eq!(
            classify_planar_boundaries(
                &plane(),
                &rows(vec![square(1.0, 1.0, 3.0, 3.0), square(0.0, 0.0, 5.0, 5.0)])
            ),
            FaceLoops::classified(loop_id(1), vec![loop_id(0)])
        );
    }

    #[test]
    fn disjoint_boundaries_remain_unspecified() {
        assert_eq!(
            classify_planar_boundaries(
                &plane(),
                &rows(vec![square(0.0, 0.0, 1.0, 1.0), square(3.0, 0.0, 4.0, 1.0)])
            ),
            FaceLoops::unspecified(vec![loop_id(0), loop_id(1)])
        );
    }

    #[test]
    fn overlapping_or_nested_holes_remain_unspecified() {
        let outer = square(0.0, 0.0, 10.0, 10.0);
        let unspecified = FaceLoops::unspecified(vec![loop_id(0), loop_id(1), loop_id(2)]);
        assert_eq!(
            classify_planar_boundaries(
                &plane(),
                &rows(vec![
                    outer.clone(),
                    square(1.0, 1.0, 5.0, 5.0),
                    square(4.0, 4.0, 8.0, 8.0)
                ])
            ),
            unspecified
        );
        assert_eq!(
            classify_planar_boundaries(
                &plane(),
                &rows(vec![
                    outer,
                    square(1.0, 1.0, 9.0, 9.0),
                    square(2.0, 2.0, 3.0, 3.0)
                ])
            ),
            unspecified
        );
    }
}
