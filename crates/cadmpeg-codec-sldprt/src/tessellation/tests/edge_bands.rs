// SPDX-License-Identifier: Apache-2.0
//! Banded trim polygons answer what a scan of every edge answers.

use crate::tessellation::{
    is_simple_polygon, polygon_boundary_within, polygon_contains, polygon_strictly_contains,
    PolygonBox, TrimPolygon,
};
use cadmpeg_ir::math::Point2;

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn unit(state: &mut u64) -> f64 {
    f64::from(u32::try_from(xorshift(state) >> 40).expect("24 bits")) / f64::from(1_u32 << 24)
}

fn below(state: &mut u64, bound: u64) -> usize {
    usize::try_from(xorshift(state) % bound).expect("small")
}

/// A star-shaped polygon about `center`, with repeated vertices, flat edges
/// and long spokes mixed in.
fn star(state: &mut u64, center: Point2, scale: f64) -> Vec<Point2> {
    let count = 16 + below(state, 150);
    let mut points = Vec::with_capacity(count);
    for index in 0..count {
        let angle = (f64::from(u32::try_from(index).expect("small")) + unit(state) * 0.9)
            * std::f64::consts::TAU
            / f64::from(u32::try_from(count).expect("small"));
        let radius = scale
            * if below(state, 16) == 0 {
                0.05
            } else {
                0.3 + unit(state) * 0.7
            };
        points.push(Point2::new(
            center.u + radius * angle.cos(),
            center.v + radius * angle.sin(),
        ));
        match below(state, 12) {
            0 => points.push(points[points.len() - 1]),
            1 => {
                let last = points[points.len() - 1];
                points.push(Point2::new(last.u + scale * 0.01, last.v));
            }
            _ => {}
        }
    }
    points
}

/// Points on, near and away from the polygon: vertices, the levels of
/// vertices, edges offset by about the tolerance, and points that are not
/// finite.
fn probes(state: &mut u64, polygon: &[Point2], tolerance: f64, scale: f64) -> Vec<Point2> {
    let mut points = vec![
        Point2::new(f64::NAN, polygon[0].v),
        Point2::new(polygon[0].u, f64::INFINITY),
    ];
    let offset = if tolerance.is_finite() {
        tolerance.abs()
    } else {
        scale * 1.0e-3
    };
    for _ in 0..12 {
        let index = below(state, u64::try_from(polygon.len()).expect("small"));
        let start = polygon[index];
        let end = polygon[(index + 1) % polygon.len()];
        let along = unit(state) * 1.2 - 0.1;
        let (du, dv) = (end.u - start.u, end.v - start.v);
        let length = du.hypot(dv);
        let (nu, nv) = if length > 0.0 {
            (-dv / length, du / length)
        } else {
            (0.0, 1.0)
        };
        let side = offset * (1.0 + (unit(state) - 0.5) * 1.0e-6);
        points.extend([
            start,
            Point2::new(start.u + scale * (unit(state) - 0.5), start.v),
            Point2::new(
                start.u + along * du + side * nu,
                start.v + along * dv + side * nv,
            ),
            Point2::new(
                start.u + along * du - side * nu,
                start.v + along * dv - side * nv,
            ),
            Point2::new(start.u, start.v - side),
            Point2::new(
                polygon[0].u + (unit(state) - 0.5) * 2.5 * scale,
                polygon[0].v + (unit(state) - 0.5) * 2.5 * scale,
            ),
        ]);
    }
    points
}

#[test]
fn banded_queries_match_scanning_every_edge() {
    let mut state = 0x6a09_e667_f3bc_c908_u64;
    let (mut polygons, mut banded, mut cases, mut near, mut inside) = (0, 0, 0, 0, 0);
    let mut excluded = 0;
    for scale in [1.0e-250, 1.0e-6, 1.0, 3.0e4, 1.0e150, 1.0e300] {
        for _ in 0..20 {
            let center = Point2::new(
                (unit(&mut state) - 0.5) * 8.0 * scale,
                (unit(&mut state) - 0.5) * 8.0 * scale,
            );
            let points = star(&mut state, center, scale);
            let polygon = TrimPolygon::new(points.clone());
            polygons += 1;
            banded += usize::from(polygon.bands.is_some());
            for tolerance in [
                0.0,
                -0.0,
                scale * 1.0e-12,
                scale * f64::from(f32::EPSILON) * unit(&mut state),
                scale * 1.0e-3,
                -scale * 1.0e-6,
                f64::NAN,
                f64::INFINITY,
            ] {
                for point in probes(&mut state, &points, tolerance, scale) {
                    let within = polygon_boundary_within(&points, point, tolerance);
                    let contains = polygon_contains(&points, point, tolerance);
                    let context = || format!("{points:?} {point:?} {tolerance:e}");
                    assert_eq!(
                        polygon.boundary_within(point, tolerance),
                        within,
                        "{}",
                        context()
                    );
                    assert_eq!(
                        polygon.contains(point, tolerance),
                        contains,
                        "{}",
                        context()
                    );
                    let strictly = contains && !within;
                    assert_eq!(
                        polygon_strictly_contains(&points, point, tolerance),
                        strictly,
                        "{}",
                        context()
                    );
                    assert_eq!(
                        polygon.strictly_contains(point, tolerance),
                        strictly,
                        "{}",
                        context()
                    );
                    if PolygonBox::new(&points, tolerance)
                        .is_some_and(|bounds| bounds.excludes(point))
                    {
                        assert!(!contains && !within, "{}", context());
                        excluded += 1;
                    }
                    cases += 1;
                    near += usize::from(within);
                    inside += usize::from(contains && !within);
                }
            }
        }
    }
    assert!(
        banded * 4 > polygons * 3,
        "{banded} of {polygons} polygons are banded"
    );
    assert!(
        near * 10 > cases,
        "{near} of {cases} cases lie near the boundary"
    );
    assert!(inside * 10 > cases, "{inside} of {cases} cases lie inside");
    assert!(
        excluded * 50 > cases,
        "{excluded} of {cases} cases lie beyond the box"
    );
}

#[test]
fn points_beyond_a_triangle_box_are_neither_near_nor_inside() {
    let mut state = 0xbb67_ae85_84ca_a73b_u64;
    let (mut cases, mut excluded, mut close) = (0, 0, 0);
    for scale in [1.0e-150, 1.0e-6, 1.0, 3.0e4, 1.0e140] {
        for _ in 0..400 {
            let mut corner = || {
                Point2::new(
                    (unit(&mut state) - 0.5) * scale,
                    (unit(&mut state) - 0.5) * scale,
                )
            };
            let mut triangle = [corner(), corner(), corner()];
            if below(&mut state, 4) == 0 {
                triangle[2] = Point2::new(
                    triangle[0].u + (triangle[1].u - triangle[0].u) * 0.5,
                    triangle[0].v + (triangle[1].v - triangle[0].v) * 0.5 + scale * 1.0e-12,
                );
            }
            let tolerance = match below(&mut state, 4) {
                0 => 0.0,
                1 => scale * 1.0e-12,
                2 => scale * f64::from(f32::EPSILON),
                _ => -scale * 1.0e-9,
            };
            let bounds = PolygonBox::new(&triangle, tolerance).expect("boxed triangle");
            for _ in 0..16 {
                let corner = triangle[below(&mut state, 3)];
                let reach = tolerance.abs() * (1.0 + 4.0 * unit(&mut state))
                    + scale * 4.0e-9 * unit(&mut state);
                let point = match below(&mut state, 5) {
                    0 => Point2::new(corner.u + reach, corner.v),
                    1 => Point2::new(corner.u - reach, corner.v),
                    2 => Point2::new(corner.u, corner.v + reach),
                    3 => Point2::new(corner.u, corner.v - reach),
                    _ => Point2::new(
                        (unit(&mut state) - 0.5) * 3.0 * scale,
                        (unit(&mut state) - 0.5) * 3.0 * scale,
                    ),
                };
                let within = polygon_boundary_within(&triangle, point, tolerance);
                let contains = polygon_contains(&triangle, point, tolerance);
                cases += 1;
                if bounds.excludes(point) {
                    assert!(!within && !contains, "{triangle:?} {point:?} {tolerance:e}");
                    excluded += 1;
                } else if !contains {
                    close += 1;
                }
            }
        }
    }
    assert!(
        excluded * 4 > cases,
        "{excluded} of {cases} points are excluded"
    );
    assert!(
        close * 10 > cases,
        "{close} of {cases} points are outside but kept"
    );
}

#[test]
fn long_edges_take_fewer_bands_and_unbandable_polygons_are_scanned_whole() {
    let comb = (0..64)
        .flat_map(|index| {
            let u = f64::from(index);
            [Point2::new(u, 0.0), Point2::new(u + 0.5, 100.0)]
        })
        .chain([Point2::new(64.0, -1.0)])
        .collect::<Vec<_>>();
    let bands = TrimPolygon::new(comb.clone())
        .bands
        .expect("the comb fits in fewer bands");
    assert!(bands.starts.len() - 1 < comb.len() / 4);
    let flat = (0..32)
        .map(|index| Point2::new(f64::from(index), 1.0))
        .collect::<Vec<_>>();
    let mut not_finite = comb.clone();
    not_finite[3] = Point2::new(f64::NAN, 0.0);
    let small = comb[..12].to_vec();
    let points = [Point2::new(10.2, 50.0), Point2::new(3.0, 0.0)];
    for polygon in [&flat, &not_finite, &small] {
        assert!(TrimPolygon::new(polygon.clone()).bands.is_none());
    }
    for polygon in [comb, flat, not_finite, small] {
        let trim = TrimPolygon::new(polygon.clone());
        for point in points {
            assert_eq!(
                trim.contains(point, 1.0e-9),
                polygon_contains(&polygon, point, 1.0e-9)
            );
        }
    }
}

#[test]
fn remembered_simplicity_is_the_tested_simplicity() {
    let mut points = (0..64)
        .map(|index| {
            let angle = f64::from(index) * std::f64::consts::TAU / 64.0;
            Point2::new(angle.cos(), angle.sin())
        })
        .collect::<Vec<_>>();
    points[10] = Point2::new(points[9].u + 0.01, points[9].v);
    let polygon = TrimPolygon::new(points.clone());
    for tolerance in [1.0e-9, 1.0e-9, 0.2, 1.0e-9, 0.0, 0.2, 0.2] {
        assert_eq!(
            polygon.is_simple(tolerance),
            is_simple_polygon(&points, tolerance),
            "{tolerance}"
        );
    }
}
