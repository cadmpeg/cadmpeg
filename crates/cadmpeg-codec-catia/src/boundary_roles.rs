// SPDX-License-Identifier: Apache-2.0
//! Geometry-backed boundary-role derivation shared by closed topology routes.

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::topology::LoopBoundaryRole;

const EPS_PLANE_AXES_ORTHO: f64 = 1.0e-8;
const EPS_PLANAR_COORDINATE: f64 = 1.0e-10;

fn strictly_inside_planar_polygon(point: Point2, polygon: &[Point2], tolerance: f64) -> bool {
    let mut inside = false;
    for (left, right) in polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
    {
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
            return false;
        }
        if (left.v > point.v) != (right.v > point.v) {
            let intersection = left.u + (point.v - left.v) * edge_u / (right.v - left.v);
            if intersection > point.u {
                inside = !inside;
            }
        }
    }
    inside
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
    left: &[Point2],
    right: &[Point2],
    tolerance: f64,
    same_polygon: bool,
) -> bool {
    for (left_index, (&left_start, &left_end)) in left
        .iter()
        .zip(left.iter().cycle().skip(1))
        .take(left.len())
        .enumerate()
    {
        for (right_index, (&right_start, &right_end)) in right
            .iter()
            .zip(right.iter().cycle().skip(1))
            .take(right.len())
            .enumerate()
        {
            if same_polygon
                && (left_index == right_index
                    || (left_index + 1) % left.len() == right_index
                    || (right_index + 1) % right.len() == left_index)
            {
                continue;
            }
            if segments_intersect_or_touch(left_start, left_end, right_start, right_end, tolerance)
            {
                return true;
            }
        }
    }
    false
}

/// Classify complete planar boundary polygons by strict containment.
///
/// A single boundary is the outer boundary by the face invariant. Multiple
/// boundaries are classified only when one unique largest non-degenerate
/// polygon strictly contains every other polygon. This deliberately declines
/// disjoint, touching, nested-hole, malformed, and non-planar arrangements.
pub(crate) fn classify_planar_boundary_roles(
    surface: &SurfaceGeometry,
    boundaries: &[Vec<Point3>],
) -> Vec<LoopBoundaryRole> {
    let unspecified = || {
        alloc_filled(
            boundaries.len(),
            LoopBoundaryRole::Unspecified,
            "catia planar boundary roles",
        )
        .unwrap_or_default()
    };
    if boundaries.len() == 1 {
        return vec![LoopBoundaryRole::Outer];
    }
    let SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = surface
    else {
        return unspecified();
    };
    let Some(normal) = normal.unit() else {
        return unspecified();
    };
    let Some(u_axis) = u_axis.unit() else {
        return unspecified();
    };
    if normal.dot(u_axis).abs() > EPS_PLANE_AXES_ORTHO {
        return unspecified();
    }
    let Some(v_axis) = normal.cross(u_axis).unit() else {
        return unspecified();
    };
    let polygons = boundaries
        .iter()
        .map(|boundary| {
            (boundary.len() >= 3).then(|| {
                boundary
                    .iter()
                    .map(|point| {
                        let offset = point.vector_from(*origin);
                        Point2::new(offset.dot(u_axis), offset.dot(v_axis))
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Option<Vec<_>>>();
    let Some(polygons) = polygons else {
        return unspecified();
    };
    let coordinate_scale = polygons
        .iter()
        .flat_map(|polygon| polygon.iter())
        .flat_map(|point| [point.u.abs(), point.v.abs()])
        .fold(1.0, f64::max);
    let coordinate_tolerance = EPS_PLANAR_COORDINATE * coordinate_scale;
    let area_tolerance = coordinate_tolerance * coordinate_scale;
    let areas = polygons
        .iter()
        .map(|polygon| {
            polygon
                .iter()
                .zip(polygon.iter().cycle().skip(1))
                .map(|(left, right)| left.u * right.v - right.u * left.v)
                .sum::<f64>()
                * 0.5
        })
        .collect::<Vec<_>>();
    if areas
        .iter()
        .any(|area| !area.is_finite() || area.abs() <= area_tolerance)
    {
        return unspecified();
    }
    if polygons.iter().enumerate().any(|(index, polygon)| {
        polygon_boundaries_intersect(polygon, polygon, coordinate_tolerance, true)
            || polygons.iter().skip(index + 1).any(|other| {
                polygon_boundaries_intersect(polygon, other, coordinate_tolerance, false)
            })
    }) {
        return unspecified();
    }
    let Some((outer, outer_area)) = areas
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.abs().total_cmp(&right.abs()))
    else {
        return unspecified();
    };
    let outer_area = outer_area.abs();
    if areas
        .iter()
        .enumerate()
        .any(|(index, area)| index != outer && outer_area - area.abs() <= area_tolerance)
    {
        return unspecified();
    }

    if polygons.iter().enumerate().any(|(index, polygon)| {
        index != outer
            && polygon.iter().any(|point| {
                !strictly_inside_planar_polygon(*point, &polygons[outer], coordinate_tolerance)
            })
    }) {
        return unspecified();
    }
    if polygons.iter().enumerate().any(|(index, polygon)| {
        index != outer
            && polygons.iter().enumerate().any(|(other_index, other)| {
                other_index != outer
                    && other_index != index
                    && polygon.iter().any(|point| {
                        strictly_inside_planar_polygon(*point, other, coordinate_tolerance)
                    })
            })
    }) {
        return unspecified();
    }
    let Ok(mut roles) = alloc_filled(
        boundaries.len(),
        LoopBoundaryRole::Inner,
        "catia planar boundary roles",
    ) else {
        return unspecified();
    };
    roles[outer] = LoopBoundaryRole::Outer;
    roles
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::LoopBoundaryRole;

    use super::classify_planar_boundary_roles;

    fn plane() -> SurfaceGeometry {
        SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        }
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

    #[test]
    fn one_boundary_is_outer() {
        assert_eq!(
            classify_planar_boundary_roles(&plane(), &[square(0.0, 0.0, 1.0, 1.0)]),
            vec![LoopBoundaryRole::Outer]
        );
    }

    #[test]
    fn containment_classifies_outer_and_hole_independent_of_order() {
        assert_eq!(
            classify_planar_boundary_roles(
                &plane(),
                &[square(1.0, 1.0, 3.0, 3.0), square(0.0, 0.0, 5.0, 5.0)]
            ),
            vec![LoopBoundaryRole::Inner, LoopBoundaryRole::Outer]
        );
    }

    #[test]
    fn disjoint_boundaries_remain_unspecified() {
        assert_eq!(
            classify_planar_boundary_roles(
                &plane(),
                &[square(0.0, 0.0, 1.0, 1.0), square(3.0, 0.0, 4.0, 1.0)]
            ),
            vec![LoopBoundaryRole::Unspecified, LoopBoundaryRole::Unspecified]
        );
    }

    #[test]
    fn overlapping_or_nested_holes_remain_unspecified() {
        let outer = square(0.0, 0.0, 10.0, 10.0);
        assert_eq!(
            classify_planar_boundary_roles(
                &plane(),
                &[
                    outer.clone(),
                    square(1.0, 1.0, 5.0, 5.0),
                    square(4.0, 4.0, 8.0, 8.0)
                ]
            ),
            vec![
                LoopBoundaryRole::Unspecified,
                LoopBoundaryRole::Unspecified,
                LoopBoundaryRole::Unspecified
            ]
        );
        assert_eq!(
            classify_planar_boundary_roles(
                &plane(),
                &[
                    outer,
                    square(1.0, 1.0, 9.0, 9.0),
                    square(2.0, 2.0, 3.0, 3.0)
                ]
            ),
            vec![
                LoopBoundaryRole::Unspecified,
                LoopBoundaryRole::Unspecified,
                LoopBoundaryRole::Unspecified
            ]
        );
    }
}
