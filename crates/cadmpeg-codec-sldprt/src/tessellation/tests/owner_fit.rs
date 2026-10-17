// SPDX-License-Identifier: Apache-2.0
//! The owner fit's shortcuts state what the general measure states.

use super::test_nurbs_surface;
use crate::tessellation::{
    identity_point, polygon_boundary_within, surface_measure, surface_residual,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{
    ConeSurface, CylinderSurface, PlaneSurface, SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::planar::point_segment_distance;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::FinitePoint2;

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn identity_point_is_the_identity_transform_image_bit_for_bit() {
    let edges = [
        0.0,
        -0.0,
        f64::from_bits(1),
        -f64::from_bits(1),
        f64::from_bits(0x000f_ffff_ffff_ffff),
        -f64::from_bits(0x0008_0000_0000_0001),
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        1.0,
        -1.5,
        1.0e-300,
        1.0e300,
        f64::MAX,
        -f64::MAX,
    ];
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut coordinates = edges.to_vec();
    while coordinates.len() < 64 {
        let value = f64::from_bits(xorshift(&mut state));
        if value.is_finite() {
            coordinates.push(value);
        }
    }
    let bits = |point: Point3| [point.x, point.y, point.z].map(f64::to_bits);
    for &x in &coordinates {
        for &y in &edges {
            for &z in &coordinates[..16] {
                let point = FinitePoint3::new(Point3::new(x, y, z)).expect("finite point");
                let transformed = Transform::identity()
                    .apply_point(point.get())
                    .expect("the identity keeps a finite point finite")
                    .get();
                assert_eq!(
                    bits(identity_point(point)),
                    bits(transformed),
                    "{x:e} {y:e} {z:e}"
                );
            }
        }
    }
}

fn measured_boundary_within(polygon: &[Point2], point: Point2, tolerance: f64) -> bool {
    polygon.iter().enumerate().any(|(index, start)| {
        match [point, *start, polygon[(index + 1) % polygon.len()]].map(FinitePoint2::new) {
            [Some(point), Some(start), Some(end)] => {
                point_segment_distance(point, start, end) <= tolerance
            }
            _ => false,
        }
    })
}

#[test]
fn boxed_boundary_test_matches_measuring_every_edge() {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let unit = |state: &mut u64| {
        f64::from(u32::try_from(xorshift(state) >> 40).expect("24 bits")) / f64::from(1_u32 << 24)
    };
    let mut cases = 0_usize;
    let mut near = 0_usize;
    for scale in [1.0e-300, 1.0e-6, 1.0, 3.0e4, 1.0e150, 1.0e300] {
        for _ in 0..300 {
            let vertices = 3 + usize::try_from(xorshift(&mut state) % 5).expect("small");
            let mut polygon = (0..vertices)
                .map(|_| {
                    Point2::new(
                        (unit(&mut state) - 0.5) * scale,
                        (unit(&mut state) - 0.5) * scale,
                    )
                })
                .collect::<Vec<_>>();
            if xorshift(&mut state).is_multiple_of(4) {
                polygon[0] = Point2::new(polygon[1].u + scale * 1.0e-12, polygon[1].v);
            }
            if xorshift(&mut state).is_multiple_of(8) {
                polygon[1] = Point2::new(f64::NAN, 0.0);
            }
            let tolerance = match xorshift(&mut state) % 8 {
                0 => 0.0,
                1 => -0.0,
                2 => f64::NAN,
                3 => f64::INFINITY,
                4 => scale * 1.0e-12,
                _ => scale * f64::from(f32::EPSILON) * unit(&mut state),
            };
            let edge = usize::try_from(xorshift(&mut state)).expect("index") % vertices;
            let (start, end) = (polygon[edge], polygon[(edge + 1) % vertices]);
            let (du, dv) = (end.u - start.u, end.v - start.v);
            let length = du.hypot(dv);
            let (nu, nv) = if length > 0.0 && length.is_finite() {
                (-dv / length, du / length)
            } else {
                (0.0, 1.0)
            };
            let along = unit(&mut state) * 1.2 - 0.1;
            let offset = if tolerance.is_finite() {
                tolerance.abs() * (1.0 + (unit(&mut state) - 0.5) * 1.0e-6)
            } else {
                scale * unit(&mut state)
            };
            let points = [
                Point2::new(
                    start.u + along * du + offset * nu,
                    start.v + along * dv + offset * nv,
                ),
                Point2::new(start.u - offset, start.v),
                Point2::new(end.u, end.v + offset),
                Point2::new(
                    end.u + du / length * offset * (1.0 + 1.0e-9),
                    end.v + dv / length * offset * (1.0 + 1.0e-9),
                ),
                Point2::new(
                    (unit(&mut state) - 0.5) * 2.0 * scale,
                    (unit(&mut state) - 0.5) * 2.0 * scale,
                ),
            ];
            for point in points {
                let measured = measured_boundary_within(&polygon, point, tolerance);
                assert_eq!(
                    polygon_boundary_within(&polygon, point, tolerance),
                    measured,
                    "{polygon:?} {point:?} {tolerance:e}"
                );
                cases += 1;
                near += usize::from(measured);
            }
        }
    }
    assert!(
        near > cases / 10,
        "{near} of {cases} cases lie near the boundary"
    );
}

#[test]
fn surface_residual_is_the_measured_residual() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let axis = Vector3::new(0.0, 0.6, 0.8).unit().unwrap();
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let origin = Point3::new(0.25, -0.5, 1.0);
    let surfaces = [
        SolvedSurfaceGeometry::Plane(PlaneSurface::try_new(origin, axis, reference).unwrap()),
        SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(origin, axis, reference, 0.75).unwrap(),
        ),
        SolvedSurfaceGeometry::Sphere(
            SphereSurface::try_new(origin, axis, reference, 0.75).unwrap(),
        ),
        SolvedSurfaceGeometry::Torus(
            TorusSurface::try_new(origin, axis, reference, 1.0, 0.25).unwrap(),
        ),
        SolvedSurfaceGeometry::Cone(
            ConeSurface::try_new(
                origin,
                axis,
                reference,
                0.5,
                0.75,
                std::f64::consts::FRAC_PI_6,
            )
            .unwrap(),
        ),
        SolvedSurfaceGeometry::Nurbs(test_nurbs_surface()),
    ];
    let steps = |count: u32| (0..count).map(|index| -1.0 + 0.4 * f64::from(index));
    for surface in &surfaces {
        for x in steps(6) {
            for y in steps(6) {
                for z in steps(6) {
                    let point = Point3::new(x, y, z);
                    for tolerance in [1.0e-3, 0.5, 4.0] {
                        let measured = surface_measure(&ctx, surface, point, Some(tolerance))
                            .expect("measure within the test budget")
                            .map(|measure| measure.residual.to_bits());
                        let residual = surface_residual(&ctx, surface, point, tolerance)
                            .expect("residual within the test budget")
                            .map(f64::to_bits);
                        assert_eq!(residual, measured, "{surface:?} {point:?} {tolerance}");
                    }
                }
            }
        }
    }
}
