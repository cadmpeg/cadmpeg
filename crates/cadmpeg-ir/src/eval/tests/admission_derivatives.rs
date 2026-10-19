// SPDX-License-Identifier: Apache-2.0
//! Placed surface partial depth refusals and heap-support surface partials.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::eval::{surface_partials, surface_second_partials, EvaluationFailure};
use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use crate::math::{Point3, Vector3};

#[test]
fn placed_surface_partials_return_the_session_depth_refusal() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
        crate::geometry::PlacedSurface::try_new(
            Box::new(SolvedSurfaceGeometry::Plane(
                crate::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            crate::transform::Transform::affine([
                [2.0, 0.0, 0.0, 0.0],
                [0.0, 2.0, 0.0, 0.0],
                [0.0, 0.0, 2.0, 0.0],
            ])
            .unwrap(),
        )
        .unwrap(),
    ));
    for second in [false, true] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = if second {
            surface_second_partials(&ctx, &surface, 0.5, 0.5).map(|value| value.point)
        } else {
            surface_partials(&ctx, &surface, 0.5, 0.5).map(|value| value.point)
        };
        let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
            panic!("placed step must refuse");
        };
        assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
        assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
        assert_eq!(
            surface_partials(&ctx, &surface, f64::NAN, 0.5),
            Err(EvaluationFailure::ResourceLimit(first))
        );
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
        );
    }
}

#[test]
fn degree_sixteen_plane_partials_are_exact() {
    let plane = crate::eval::test_support::high_degree_plane();
    let ctx = cadmpeg_test_support::service_decode_context();
    let partials = crate::eval::nurbs_surface_partials(&ctx, &plane, 0.5, 0.5).unwrap();
    assert_eq!(partials.point, Point3::new(0.5, 0.5, 0.0));
    assert_eq!(partials.du, Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, 1.0, 0.0));
    let second = crate::eval::nurbs_surface_second_partials(&ctx, &plane, 0.5, 0.5).unwrap();
    for value in [second.duu, second.duv, second.dvv] {
        assert_eq!(value, Vector3::new(0.0, 0.0, 0.0));
    }
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(plane));
    assert_eq!(
        surface_partials(&ctx, &geometry, 0.5, 0.5).unwrap().point,
        Point3::new(0.5, 0.5, 0.0)
    );
    assert_eq!(
        surface_second_partials(&ctx, &geometry, 0.5, 0.5)
            .unwrap()
            .point,
        Point3::new(0.5, 0.5, 0.0)
    );
    ctx.finish_session().unwrap();
}
