// SPDX-License-Identifier: Apache-2.0
//! Geometry-backed boundary-role derivation shared by closed topology routes.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::LoopId;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::topology::FaceLoops;

const EPS_PLANAR_COORDINATE: f64 = 1.0e-10;

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

#[derive(Clone, Copy)]
struct Bounds {
    min_u: f64,
    min_v: f64,
    max_u: f64,
    max_v: f64,
}

impl Bounds {
    fn segment(left: Point2, right: Point2) -> Self {
        Self {
            min_u: left.u.min(right.u),
            min_v: left.v.min(right.v),
            max_u: left.u.max(right.u),
            max_v: left.v.max(right.v),
        }
    }
    fn union(self, other: Self) -> Self {
        Self {
            min_u: self.min_u.min(other.min_u),
            min_v: self.min_v.min(other.min_v),
            max_u: self.max_u.max(other.max_u),
            max_v: self.max_v.max(other.max_v),
        }
    }
    fn expanded(self, tolerance: f64) -> Self {
        Self {
            min_u: self.min_u - tolerance,
            min_v: self.min_v - tolerance,
            max_u: self.max_u + tolerance,
            max_v: self.max_v + tolerance,
        }
    }
    fn intersects(self, other: Self) -> bool {
        self.min_u <= other.max_u
            && other.min_u <= self.max_u
            && self.min_v <= other.max_v
            && other.min_v <= self.max_v
    }
}

struct SpatialNode {
    bounds: Bounds,
    range: std::ops::Range<usize>,
    children: Option<[usize; 2]>,
}

/// A balanced box hierarchy keeps exact geometry tests local to overlapping boxes.
struct SpatialIndex {
    items: Vec<(Bounds, usize)>,
    nodes: Vec<SpatialNode>,
}

impl SpatialIndex {
    fn new(ctx: &DecodeContext<'_>, items: Vec<(Bounds, usize)>) -> Result<Self, CodecError> {
        let mut index = Self {
            items,
            nodes: Vec::new(),
        };
        if index.items.is_empty() {
            return Ok(index);
        }
        let mut task_storage = ctx.reserve_scoped(0, "catia_boundary_index_tasks")?;
        let mut tasks = Vec::new();
        ctx.push_vec(
            &mut index.nodes,
            SpatialNode {
                bounds: index.items[0].0,
                range: 0..index.items.len(),
                children: None,
            },
            "catia_boundary_index_nodes",
        )?;
        ctx.push_scoped_vec(
            &mut task_storage,
            &mut tasks,
            0usize,
            "catia_boundary_index_tasks",
        )?;
        loop {
            let Some(node) = ctx.next_charged(
                &mut std::iter::from_fn(|| tasks.pop()),
                "catia_boundary_index_build",
            )?
            else {
                break;
            };
            let range = index.nodes[node].range.clone();
            let mut bounds = index.items[range.start].0;
            for (item, _) in
                ctx.admit_iter(&index.items[range.clone()], "catia_boundary_index_bounds")?
            {
                bounds = bounds.union(*item);
            }
            index.nodes[node].bounds = bounds;
            if range.len() <= 8 {
                continue;
            }
            let along_u = bounds.max_u - bounds.min_u >= bounds.max_v - bounds.min_v;
            if !ctx.is_sorted_by(
                &index.items[range.clone()],
                |(bounds, _)| {
                    if along_u {
                        &bounds.min_u
                    } else {
                        &bounds.min_v
                    }
                },
                f64::total_cmp,
                "catia_boundary_index_order",
            )? {
                ctx.stable_sort_by(
                    &mut index.items[range.clone()],
                    |(bounds, _)| {
                        if along_u {
                            &bounds.min_u
                        } else {
                            &bounds.min_v
                        }
                    },
                    f64::total_cmp,
                    "catia_boundary_index_sort",
                )?;
            }
            let middle = range.start + range.len() / 2;
            let left = index.nodes.len();
            for child in [range.start..middle, middle..range.end] {
                ctx.push_vec(
                    &mut index.nodes,
                    SpatialNode {
                        bounds,
                        range: child,
                        children: None,
                    },
                    "catia_boundary_index_nodes",
                )?;
            }
            index.nodes[node].children = Some([left, left + 1]);
            for child in [left + 1, left] {
                ctx.push_scoped_vec(
                    &mut task_storage,
                    &mut tasks,
                    child,
                    "catia_boundary_index_tasks",
                )?;
            }
        }
        Ok(index)
    }

    fn any_match(
        &self,
        ctx: &DecodeContext<'_>,
        query: Bounds,
        mut predicate: impl FnMut(usize) -> Result<bool, CodecError>,
    ) -> Result<bool, CodecError> {
        if self.nodes.is_empty() {
            return Ok(false);
        }
        let mut storage = ctx.reserve_scoped(0, "catia_boundary_query_stack")?;
        let mut stack = Vec::new();
        ctx.push_scoped_vec(
            &mut storage,
            &mut stack,
            0usize,
            "catia_boundary_query_stack",
        )?;
        loop {
            let Some(node) = ctx.next_charged(
                &mut std::iter::from_fn(|| stack.pop()),
                "catia_boundary_index_query",
            )?
            else {
                return Ok(false);
            };
            let node = &self.nodes[node];
            if !node.bounds.intersects(query) {
                continue;
            }
            if let Some(children) = node.children {
                for child in children.into_iter().rev() {
                    ctx.push_scoped_vec(
                        &mut storage,
                        &mut stack,
                        child,
                        "catia_boundary_query_stack",
                    )?;
                }
            } else if ctx.any_by(
                &self.items[node.range.clone()],
                |(bounds, id)| {
                    if bounds.intersects(query) {
                        predicate(*id)
                    } else {
                        Ok(false)
                    }
                },
                "catia_boundary_index_candidates",
            )? {
                return Ok(true);
            }
        }
    }
}

struct BoundaryEdge {
    polygon: usize,
    ordinal: usize,
    start: Point2,
    end: Point2,
}
struct BoundaryIndexes {
    edges: Vec<BoundaryEdge>,
    all_edges: SpatialIndex,
    polygons: Vec<SpatialIndex>,
}

impl BoundaryIndexes {
    fn new(ctx: &DecodeContext<'_>, polygons: &[Vec<Point2>]) -> Result<Self, CodecError> {
        let mut edges = Vec::new();
        let mut all_boxes = Vec::new();
        let mut indexes = Vec::new();
        for (polygon_id, polygon) in ctx
            .admit_iter(polygons, "catia_boundary_index_polygons")?
            .enumerate()
        {
            let mut boxes = Vec::new();
            for (ordinal, start) in ctx
                .admit_iter(polygon, "catia_boundary_index_edges")?
                .enumerate()
            {
                let end = polygon[(ordinal + 1) % polygon.len()];
                let bounds = Bounds::segment(*start, end);
                let id = edges.len();
                ctx.push_vec(
                    &mut edges,
                    BoundaryEdge {
                        polygon: polygon_id,
                        ordinal,
                        start: *start,
                        end,
                    },
                    "catia_boundary_index_edges",
                )?;
                ctx.push_vec(&mut boxes, (bounds, id), "catia_boundary_index_boxes")?;
                ctx.push_vec(&mut all_boxes, (bounds, id), "catia_boundary_index_boxes")?;
            }
            ctx.push_vec(
                &mut indexes,
                SpatialIndex::new(ctx, boxes)?,
                "catia_boundary_polygon_indexes",
            )?;
        }
        Ok(Self {
            edges,
            all_edges: SpatialIndex::new(ctx, all_boxes)?,
            polygons: indexes,
        })
    }

    fn intersect(
        &self,
        ctx: &DecodeContext<'_>,
        polygons: &[Vec<Point2>],
        tolerance: f64,
    ) -> Result<bool, CodecError> {
        ctx.any_by(
            self.edges.iter().enumerate(),
            |(id, edge)| {
                self.all_edges.any_match(
                    ctx,
                    Bounds::segment(edge.start, edge.end).expanded(tolerance),
                    |other| {
                        if other <= id {
                            return Ok(false);
                        }
                        let other = &self.edges[other];
                        if edge.polygon == other.polygon
                            && ((edge.ordinal + 1) % polygons[edge.polygon].len() == other.ordinal
                                || (other.ordinal + 1) % polygons[edge.polygon].len()
                                    == edge.ordinal)
                        {
                            return Ok(false);
                        }
                        Ok(segments_intersect_or_touch(
                            edge.start,
                            edge.end,
                            other.start,
                            other.end,
                            tolerance,
                        ))
                    },
                )
            },
            "catia_boundary_segment_edges",
        )
    }

    fn strictly_inside(
        &self,
        ctx: &DecodeContext<'_>,
        point: Point2,
        polygon: usize,
        tolerance: f64,
    ) -> Result<bool, CodecError> {
        let mut inside = false;
        let query = Bounds {
            min_u: point.u - tolerance,
            max_u: f64::INFINITY,
            min_v: point.v - tolerance,
            max_v: point.v + tolerance,
        };
        let touches = self.polygons[polygon].any_match(ctx, query, |id| {
            let edge = &self.edges[id];
            if point_on_segment(point, edge.start, edge.end, tolerance) {
                return Ok(true);
            }
            if (edge.start.v > point.v) != (edge.end.v > point.v) {
                let intersection = edge.start.u
                    + (point.v - edge.start.v) * (edge.end.u - edge.start.u)
                        / (edge.end.v - edge.start.v);
                if intersection > point.u {
                    inside = !inside;
                }
            }
            Ok(false)
        })?;
        Ok(!touches && inside)
    }
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
    let (indexes, _index_storage) = ctx.with_scoped_storage("catia_boundary_indexes", || {
        BoundaryIndexes::new(ctx, &polygons)
    })?;
    if indexes.intersect(ctx, &polygons, coordinate_tolerance)? {
        return unspecified();
    }
    if ctx.any_by(
        areas.iter().enumerate(),
        |(index, area)| Ok(index != outer && outer_area - area.abs() <= area_tolerance),
        "catia_boundary_outer_uniqueness",
    )? {
        return unspecified();
    }
    // Disjoint connected boundaries cannot cross between inside and outside.
    // Intersection admission also rejects tolerance contacts, so one vertex
    // determines each complete polygon's strict containment.
    if ctx.any_by(
        polygons.iter().enumerate(),
        |(index, polygon)| {
            Ok(index != outer
                && !indexes.strictly_inside(ctx, polygon[0], outer, coordinate_tolerance)?)
        },
        "catia_boundary_outer_containment",
    )? {
        return unspecified();
    }
    let (holes, _hole_storage) = ctx.with_scoped_storage("catia_boundary_hole_index", || {
        let mut boxes = Vec::new();
        for (index, polygon) in ctx
            .admit_iter(&indexes.polygons, "catia_boundary_hole_boxes")?
            .enumerate()
        {
            if index != outer {
                ctx.push_vec(
                    &mut boxes,
                    (polygon.nodes[0].bounds, index),
                    "catia_boundary_hole_boxes",
                )?;
            }
        }
        SpatialIndex::new(ctx, boxes)
    })?;
    if ctx.any_by(
        polygons.iter().enumerate(),
        |(index, polygon)| {
            if index == outer {
                return Ok(false);
            }
            let point = polygon[0];
            holes.any_match(
                ctx,
                Bounds::segment(point, point).expanded(coordinate_tolerance),
                |other| {
                    Ok(other != index
                        && indexes.strictly_inside(ctx, point, other, coordinate_tolerance)?)
                },
            )
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
    fn self_intersection_uses_indexed_candidates() {
        use cadmpeg_ir::math::Point2;
        let square = [
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(0.0, 1.0),
        ];
        let check = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            let polygons = vec![square.to_vec()];
            let (indexes, _storage) = ctx
                .with_scoped_storage("catia_test_boundary_index", || {
                    super::BoundaryIndexes::new(ctx, &polygons)
                })?;
            indexes.intersect(ctx, &polygons, 0.0)
        };
        let result = crate::test_support::with_service_context(check).expect("indexed square");
        assert!(!result);
        let refusal =
            crate::test_support::with_work_refusal("catia_boundary_index_candidates", |ctx| {
                let result = check(ctx);
                if let Err(cadmpeg_core::CodecError::ResourceLimit(ref limit)) = result {
                    assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                }
                result
            });
        assert!(
            matches!(refusal, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "catia_boundary_index_candidates")
        );
    }

    #[test]
    fn planar_boundary_comparisons_refuse_work() {
        let boundaries = rows(vec![
            square(0.0, 0.0, 10.0, 10.0),
            square(2.0, 2.0, 3.0, 3.0),
        ]);
        let result =
            crate::test_support::with_work_refusal("catia_boundary_index_candidates", |ctx| {
                let result = super::classify_planar_boundaries(ctx, &plane(), &boundaries);
                if let Err(cadmpeg_core::CodecError::ResourceLimit(ref limit)) = result {
                    assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                }
                result
            });
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "catia_boundary_index_candidates")
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
    #[test]
    fn boundary_index_admits_large_convex_outer_and_disjoint_holes() {
        let work = |point_count: u32| {
            let outer = (0..point_count)
                .map(|index| {
                    let angle = f64::from(index) * std::f64::consts::TAU / f64::from(point_count);
                    Point3::new(100.0 * angle.cos(), 100.0 * angle.sin(), 0.0)
                })
                .collect();
            let mut boundaries = vec![outer];
            let hole_count = point_count / 8;
            for index in 0..hole_count {
                let u = f64::from(index % 16) * 3.0 - 24.0;
                let v = f64::from(index / 16) * 3.0 - 12.0;
                boundaries.push(vec![
                    Point3::new(u, v, 0.0),
                    Point3::new(u + 1.0, v, 0.0),
                    Point3::new(u, v + 1.0, 0.0),
                ]);
            }
            let boundaries = rows(boundaries);
            let result = crate::test_support::with_work_limit(u64::MAX, |ctx| {
                let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                    cadmpeg_core::decode::ResourceDimension::WorkUnits,
                    "catia_test_boundary_complete",
                    None,
                );
                let result = super::classify_planar_boundaries(ctx, &plane(), &boundaries)?;
                assert_eq!(
                    result,
                    FaceLoops::classified(
                        loop_id(0),
                        (1..=hole_count)
                            .map(|index| loop_id(usize::try_from(index).expect("hole index")))
                            .collect()
                    )
                );
                ctx.charge_work(1, "catia_test_boundary_complete")
            });
            let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
                panic!("complete classification must reach the cost probe");
            };
            assert_eq!(limit.operation, "catia_test_boundary_complete");
            limit.used
        };
        // Doubling n raises n log^2(n) by less than three at these sizes.
        // Doubling both edge and hole populations exposes quadratic searches.
        assert!(work(1024) <= 3 * work(512));
    }

    #[test]
    fn boundary_indexes_match_exhaustive_geometry_predicates() {
        use cadmpeg_ir::math::Point2;
        let shapes = [
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(4.0, 0.0),
                Point2::new(4.0, 4.0),
                Point2::new(0.0, 4.0),
            ],
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(4.0, 0.0),
                Point2::new(2.0, 1.0),
                Point2::new(4.0, 4.0),
                Point2::new(0.0, 4.0),
            ],
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(4.0, 4.0),
                Point2::new(0.0, 4.0),
                Point2::new(4.0, 0.0),
            ],
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(0.0, 0.0),
                Point2::new(4.0, 0.0),
                Point2::new(0.0, 4.0),
            ],
        ];
        for tolerance in [0.0, super::EPS_PLANAR_COORDINATE, 0.25] {
            for shape in &shapes {
                let polygons = vec![shape.clone()];
                crate::test_support::with_service_context(|ctx| {
                    let (index, _storage) = ctx
                        .with_scoped_storage("catia_test_boundary_indexes", || {
                            super::BoundaryIndexes::new(ctx, &polygons)
                        })?;
                    let expected = (0..shape.len()).any(|left| {
                        ((left + 1)..shape.len()).any(|right| {
                            (left + 1) % shape.len() != right
                                && (right + 1) % shape.len() != left
                                && super::segments_intersect_or_touch(
                                    shape[left],
                                    shape[(left + 1) % shape.len()],
                                    shape[right],
                                    shape[(right + 1) % shape.len()],
                                    tolerance,
                                )
                        })
                    });
                    assert_eq!(index.intersect(ctx, &polygons, tolerance)?, expected);
                    for u in -2..11 {
                        for v in -2..11 {
                            let point = Point2::new(f64::from(u) * 0.5, f64::from(v) * 0.5);
                            let mut inside = false;
                            let strict = (0..shape.len()).all(|edge| {
                                let left = shape[edge];
                                let right = shape[(edge + 1) % shape.len()];
                                if super::point_on_segment(point, left, right, tolerance) {
                                    return false;
                                }
                                if (left.v > point.v) != (right.v > point.v) {
                                    let intersection = left.u
                                        + (point.v - left.v) * (right.u - left.u)
                                            / (right.v - left.v);
                                    if intersection > point.u {
                                        inside = !inside;
                                    }
                                }
                                true
                            });
                            assert_eq!(
                                index.strictly_inside(ctx, point, 0, tolerance)?,
                                strict && inside
                            );
                        }
                    }
                    Ok::<_, cadmpeg_core::CodecError>(())
                })
                .expect("indexed geometry matches exact predicates");
            }
        }
    }
}
