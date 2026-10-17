// SPDX-License-Identifier: Apache-2.0
//! Pole-box rejection before NURBS point inversion.

use super::test_nurbs_surface;
use crate::tessellation::{outside_pole_box, surface_measure};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::Point3;

const FIT_TOLERANCE: f64 = 1.0e-3;

fn weighted_test_surface(weights: [f64; 9]) -> NurbsSurface {
    let rows = test_nurbs_surface()
        .control_grid()
        .into_iter()
        .map(|row| row.into_iter().map(FinitePoint3::get).collect())
        .collect();
    let weights = weights.chunks(3).map(<[f64]>::to_vec).collect();
    let axis = || NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false);
    NurbsSurface::from_lanes(
        axis(),
        axis(),
        NurbsSurfaceLanes::new(rows, Some(weights)),
        false,
    )
    .expect("valid weighted test NURBS surface")
}

/// Whether the inversion `surface_measure` runs fits `point`, without the box.
fn inversion_fits(ctx: &DecodeContext<'_>, surface: &NurbsSurface, point: Point3) -> bool {
    let Some(parameters) =
        crate::brep::evaluation::nurbs_surface_parameter_near_point(ctx, surface, point, None)
            .expect("inversion within the test budget")
    else {
        return false;
    };
    crate::brep::evaluation::nurbs_surface_partials(ctx, surface, parameters.u, parameters.v)
        .expect("partials within the test budget")
        .is_some_and(|partials| point.distance(partials.point.get()) <= FIT_TOLERANCE)
}

fn grid_points() -> impl Iterator<Item = Point3> {
    let steps =
        |start: f64, count: u32| (0..count).map(move |index| start + 0.5 * f64::from(index));
    steps(-0.75, 8).flat_map(move |x| {
        steps(-0.75, 8).flat_map(move |y| steps(-0.5, 5).map(move |z| Point3::new(x, y, z)))
    })
}

#[test]
fn a_point_outside_the_pole_box_never_fits_the_surface() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    for surface in [
        test_nurbs_surface(),
        weighted_test_surface([1.0, 0.5, 1.0, 2.0, 0.25, 2.0, 1.0, 0.5, 1.0]),
    ] {
        let mut outside = 0_usize;
        for point in grid_points() {
            if outside_pole_box(&surface, point, FIT_TOLERANCE) {
                outside += 1;
                assert!(!inversion_fits(&ctx, &surface, point), "{point:?}");
            }
        }
        assert!(outside > 0);
        for (u, v) in [(0.0, 0.0), (0.3, 0.8), (0.5, 0.5), (1.0, 0.25)] {
            let point = cadmpeg_ir::eval::decode::nurbs_surface_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &surface,
                u,
                v,
            )
            .expect("surface point")
            .get();
            assert!(!outside_pole_box(&surface, point, FIT_TOLERANCE));
            let measure = surface_measure(
                &ctx,
                &SolvedSurfaceGeometry::Nurbs(surface.clone()),
                point,
                Some(FIT_TOLERANCE),
            )
            .expect("measure within the test budget");
            assert!(measure.is_some_and(|measure| measure.residual <= FIT_TOLERANCE));
        }
        let far = Point3::new(0.5, 0.5, 2.0);
        assert!(outside_pole_box(&surface, far, FIT_TOLERANCE));
        let beyond_corner = Point3::new(-1.1 * FIT_TOLERANCE, 0.0, 0.0);
        assert!(outside_pole_box(&surface, beyond_corner, FIT_TOLERANCE));
        assert!(!inversion_fits(&ctx, &surface, beyond_corner));
        let near_corner = Point3::new(-0.9 * FIT_TOLERANCE, 0.0, 0.0);
        assert!(!outside_pole_box(&surface, near_corner, FIT_TOLERANCE));
        assert!(inversion_fits(&ctx, &surface, near_corner));
    }
}

#[test]
fn a_weight_of_zero_or_less_gives_no_pole_box() {
    let surface = weighted_test_surface([1.0, 0.5, 1.0, 2.0, -0.25, 2.0, 1.0, 0.5, 1.0]);
    assert!(!outside_pole_box(
        &surface,
        Point3::new(100.0, 100.0, 100.0),
        FIT_TOLERANCE
    ));
}
