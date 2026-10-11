// SPDX-License-Identifier: Apache-2.0
//! Admission of stored derivatives and placed surface partials.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::eval::{
    curve_second_derivative, curve_second_derivative_solved, curve_tangent_solved,
    surface_partials, surface_second_partials, EvaluationFailure,
};
use crate::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use crate::math::{Point3, Vector3};

#[test]
fn stored_curve_derivatives_admit_actual_scratch_and_work() {
    let fixture = cadmpeg_test_support::service_decode_context();
    let solved = SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            &fixture,
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.5, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
            ],
            None,
            false,
        )
        .unwrap()
        .unwrap(),
    );
    let geometry = CurveGeometry::Solved(solved.clone());
    for trigger in 0..4 {
        for order in 0..3 {
            let mut policy = DecodePolicy::service();
            let dimension = match trigger {
                0 => {
                    policy.limits.max_materialized_bytes = 0;
                    ResourceDimension::MaterializedBytes
                }
                1 => {
                    policy.limits.max_collection_items = 0;
                    ResourceDimension::CollectionItems
                }
                2 => {
                    policy.limits.max_work_units = 0;
                    ResourceDimension::WorkUnits
                }
                _ => {
                    policy.limits.max_retained_bytes = 0;
                    ResourceDimension::RetainedBytes
                }
            };
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = match order {
                0 => curve_tangent_solved(&ctx, &solved, 0.5),
                1 => curve_second_derivative_solved(&ctx, &solved, 0.5),
                _ => curve_second_derivative(&ctx, &geometry, 0.5),
            };
            if trigger == 3 {
                let expected = if order == 0 {
                    Vector3::new(1.0, 0.0, 0.0)
                } else {
                    Vector3::new(0.0, 0.0, 0.0)
                };
                assert_eq!(result.unwrap().get(), expected);
                assert!(ctx.finish_session().is_ok());
            } else {
                let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
                    panic!("refusal must remain a resource error");
                };
                assert_eq!(first.dimension, dimension);
                assert_eq!((first.limit, first.used), (0, 0));
                assert!(first.additional > 0);
                assert_eq!(
                    curve_second_derivative(&ctx, &geometry, f64::NAN),
                    Err(EvaluationFailure::ResourceLimit(first))
                );
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
                );
            }
        }
    }
}

#[test]
fn stored_surface_partial_entries_admit_actual_scratch_and_work() {
    let fixture = cadmpeg_test_support::service_decode_context();
    let axis = || NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false);
    let poles = [0.0, 0.5, 1.0]
        .into_iter()
        .map(|x| {
            [0.0, 0.5, 1.0]
                .into_iter()
                .map(|y| Point3::new(x, y, 0.0))
                .collect()
        })
        .collect();
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::from_lanes(
            &fixture,
            axis(),
            axis(),
            NurbsSurfaceLanes::new(poles, None),
            false,
        )
        .unwrap()
        .unwrap(),
    ));
    for trigger in 0..4 {
        for second in [false, true] {
            let mut policy = DecodePolicy::service();
            let dimension = match trigger {
                0 => {
                    policy.limits.max_materialized_bytes = 0;
                    ResourceDimension::MaterializedBytes
                }
                1 => {
                    policy.limits.max_collection_items = 0;
                    ResourceDimension::CollectionItems
                }
                2 => {
                    policy.limits.max_work_units = 0;
                    ResourceDimension::WorkUnits
                }
                _ => {
                    policy.limits.max_retained_bytes = 0;
                    ResourceDimension::RetainedBytes
                }
            };
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = if second {
                surface_second_partials(&ctx, &surface, 0.5, 0.5).map(|value| value.point)
            } else {
                surface_partials(&ctx, &surface, 0.5, 0.5).map(|value| value.point)
            };
            if trigger == 3 {
                assert_eq!(result.unwrap().get(), Point3::new(0.5, 0.5, 0.0));
                assert!(ctx.finish_session().is_ok());
            } else {
                let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
                    panic!("refusal must remain a resource error");
                };
                assert_eq!(first.dimension, dimension);
                assert_eq!((first.limit, first.used), (0, 0));
                assert!(first.additional > 0);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
                );
            }
        }
    }
}

#[test]
fn placed_surface_partials_preserve_work_and_depth_refusals() {
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
    for depth in [false, true] {
        for second in [false, true] {
            let mut policy = DecodePolicy::service();
            let dimension = if depth {
                policy.limits.max_recursion_depth = 0;
                ResourceDimension::RecursionDepth
            } else {
                policy.limits.max_work_units = 0;
                ResourceDimension::WorkUnits
            };
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
            assert_eq!(first.dimension, dimension);
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
}
