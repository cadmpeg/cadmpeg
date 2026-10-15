// SPDX-License-Identifier: Apache-2.0

use crate::eval::test_support::with_policy;
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::{CurveGeometry, SolvedCurveGeometry};
use crate::math::Point3;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn curve() -> NurbsCurve {
    NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
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
    .expect("fixture constructor admission")
    .expect("finite line spline")
}

fn heap_curve() -> NurbsCurve {
    NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        4,
        vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.25, 0.0, 0.0),
            Point3::new(0.5, 0.0, 0.0),
            Point3::new(0.75, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("admitted high-degree fixture")
    .expect("finite high-degree line")
}

/// Degree 16: the smallest support that exceeds the inline window.
fn heap_support_curve() -> NurbsCurve {
    const DEGREE: u32 = 16;
    let poles = usize::try_from(DEGREE).expect("degree") + 1;
    let knots: Vec<f64> = std::iter::repeat_n(0.0, poles)
        .chain(std::iter::repeat_n(1.0, poles))
        .collect();
    let points = (0..poles)
        .map(|index| {
            Point3::new(
                f64::from(u32::try_from(index).expect("pole")) / 16.0,
                0.0,
                0.0,
            )
        })
        .collect();
    NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        DEGREE,
        knots,
        points,
        None,
        false,
    )
    .expect("admitted high-degree fixture")
    .expect("finite high-degree line")
}

#[test]
fn decode_curve_point_matches_standard_evaluation() {
    let curve = heap_curve();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        crate::eval::decode::outer_refusal(crate::eval::decode::nurbs_curve_point_at(
            &ctx, &curve, 0.5
        ))
        .map_err(CodecError::from)
        .expect("inline scratch"),
        crate::eval::decode::nurbs_curve_point_at(
            crate::eval::admission::EvaluationAdmission::Standard,
            &curve,
            0.5
        )
    );
}

#[test]
fn admitted_surface_point_keeps_both_axis_bases_inline() {
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    let surface = NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(
            4,
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            false,
        ),
        NurbsSurfaceAxis::new(
            4,
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            false,
        ),
        NurbsSurfaceLanes::new(
            (0..5)
                .map(|u| {
                    (0..5)
                        .map(|v| Point3::new(f64::from(u) * 0.25, f64::from(v) * 0.25, 0.0))
                        .collect()
                })
                .collect(),
            None,
        ),
        false,
    )
    .expect("fixture constructor admission")
    .expect("finite plane spline");
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        crate::eval::decode::outer_refusal(crate::eval::decode::surface_point(
            &ctx, &geometry, 0.5, 0.5
        ))
        .map_err(CodecError::from)
        .expect("inline scratch"),
        crate::eval::decode::surface_point(
            crate::eval::admission::EvaluationAdmission::Standard,
            &geometry,
            0.5,
            0.5
        )
    );
}

#[test]
fn admitted_curve_point_refuses_recursive_frame() {
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve()));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(
        matches!(crate::eval::decode::outer_refusal(crate::eval::decode::curve_point(&ctx, &geometry, 0.5)).map_err(CodecError::from),
        Err(CodecError::ResourceLimit(resource)) if resource.dimension == ResourceDimension::RecursionDepth
            && resource.operation == "geometry evaluation nesting")
    );
}

#[test]
fn reusable_nurbs_evaluator_refuses_depth() {
    let curve = heap_support_curve();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut evaluator = super::NurbsPointEvaluator::new(&ctx, &curve)
        .map_err(CodecError::from)
        .expect("scratch storage");
    assert!(
        matches!(evaluator.point(&ctx, 0.5).map_err(CodecError::from), Err(CodecError::ResourceLimit(resource)) if resource.operation == "geometry evaluation nesting")
    );
}

#[test]
fn reusable_nurbs_evaluator_matches_point_evaluation_above_inline_support() {
    let curve = heap_support_curve();
    with_policy(DecodePolicy::service(), |ctx| {
        let mut evaluator = super::NurbsPointEvaluator::new(ctx, &curve)
            .map_err(CodecError::from)
            .expect("heap basis");
        for index in 0..10_000 {
            let parameter = f64::from(index % 101) / 100.0;
            assert_eq!(
                evaluator
                    .point(ctx, parameter)
                    .map_err(CodecError::from)
                    .expect("reused storage"),
                crate::eval::decode::nurbs_curve_point_at(
                    crate::eval::admission::EvaluationAdmission::Standard,
                    &curve,
                    parameter
                )
            );
        }
    });
}

#[test]
fn reusable_nurbs_evaluator_keeps_constant_and_linear_spans_inline() {
    for degree in [0, 1] {
        for rational in [false, true] {
            let count = usize::try_from(degree).expect("constant or linear degree") + 1;
            let knots: Vec<f64> = (0..count)
                .map(|_| 0.0)
                .chain((0..count).map(|_| 1.0))
                .collect();
            let points = (0..count)
                .map(|index| {
                    Point3::new(
                        f64::from(u32::try_from(index).expect("two poles")),
                        0.0,
                        0.0,
                    )
                })
                .collect();
            let curve = NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                degree,
                knots,
                points,
                rational.then(|| vec![1.0; count]),
                false,
            )
            .expect("fixture constructor admission")
            .expect("fixed span");
            let mut policy = DecodePolicy::service();
            policy.limits.max_recursion_depth = 0;
            with_policy(policy, |ctx| {
                let mut evaluator = super::NurbsPointEvaluator::new(ctx, &curve)
                    .map_err(CodecError::from)
                    .expect("inline basis");
                for index in 0..10_000 {
                    let parameter = f64::from(index % 101) / 100.0;
                    assert_eq!(
                        evaluator
                            .point(ctx, parameter)
                            .map_err(CodecError::from)
                            .expect("fixed arithmetic"),
                        crate::eval::decode::outer_refusal(
                            crate::eval::decode::nurbs_curve_point_at(ctx, &curve, parameter)
                        )
                        .map_err(CodecError::from)
                        .expect("inline prior path")
                    );
                }
            });
        }
    }
}

#[test]
fn standard_evaluation_shares_scratch_and_basis_algorithms() {
    use crate::eval::admission::EvaluationAdmission;
    use std::cell::Cell;
    use std::rc::Rc;
    struct Counted(Rc<Cell<usize>>);
    impl Clone for Counted {
        fn clone(&self) -> Self {
            self.0.set(self.0.get() + 1);
            Self(Rc::clone(&self.0))
        }
    }
    let scratch = super::Scratch::new(EvaluationAdmission::Standard);
    let clones = Rc::new(Cell::new(0));
    let values = scratch.filled(3, Counted(Rc::clone(&clones))).unwrap();
    assert_eq!(values.len(), 3);
    assert_eq!(clones.get(), 3);
    let reads = Cell::new(0);
    assert!(scratch
        .collect((0..3).map(|index| {
            reads.set(reads.get() + 1);
            (index != 1).then_some(index)
        }))
        .is_none());
    assert_eq!(reads.get(), 2);
    let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let span = crate::eval::basis::bspline_span(scratch.admission, &knots, 2, 3, 0.5)
        .unwrap()
        .unwrap();
    assert_eq!(span, 2);
    assert_eq!(
        &*crate::eval::basis::bspline_basis(&scratch, &knots, 2, span, 0.5).unwrap(),
        &[0.25, 0.5, 0.25]
    );
    assert_eq!(
        &*crate::eval::basis::bspline_basis_derivative(&scratch, &knots, 2, span, 0.5).unwrap(),
        &[-1.0, 0.0, 1.0]
    );
    assert_eq!(scratch.work(usize::MAX), Some(()));
    assert_eq!(scratch.unless_refused(), Ok(()));
}

#[test]
fn standard_evaluation_depth_is_explicit_and_releases_frames() {
    use crate::eval::admission::EvaluationAdmission;
    let scratch = super::Scratch::new(EvaluationAdmission::Standard);
    let first = scratch.enter().unwrap();
    let second = scratch.enter().unwrap();
    assert_eq!(scratch.independent_depth.get(), 2);
    drop(first);
    assert_eq!(scratch.independent_depth.get(), 1);
    drop(second);
    assert_eq!(scratch.independent_depth.get(), 0);
    let mut guards = Vec::new();
    for _ in 0..256 {
        guards.push(scratch.enter().unwrap());
    }
    assert!(scratch.enter().is_none());
    let original = scratch.refused().unwrap().into_limit();
    assert_eq!(original.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(original.limit, 256);
    assert_eq!(original.used, 256);
    assert_eq!(original.additional, 1);
    assert_eq!(
        original.operation,
        "independent geometry evaluation nesting"
    );
    while let Some(guard) = guards.pop() {
        drop(guard);
    }
    drop(guards);
    assert_eq!(scratch.independent_depth.get(), 0);
    assert_eq!(scratch.unless_refused(), Err(original.into()));
}

#[test]
fn evaluation_scratch_depth_is_shared_across_caller_contexts() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = super::Scratch::new(&ctx);
    let second = super::Scratch::new(&ctx);
    let frame = first.enter().unwrap();
    assert!(second.enter().is_none());
    let original = second.refused().unwrap().into_limit();
    assert_eq!(original.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(original.limit, 1);
    assert_eq!(original.used, 1);
    assert_eq!(original.additional, 1);
    assert_eq!(original.operation, "geometry evaluation nesting");
    assert_eq!(first.refused(), Some(original.into()));
    drop(frame);
    assert_eq!(first.unless_refused(), Err(original.into()));
    assert_eq!(second.unless_refused(), Err(original.into()));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
}

#[test]
fn geometry_entry_returns_the_original_refusal_in_its_single_result() {
    use crate::eval::EvaluationFailure;
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve()));
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    with_policy(policy, |ctx| {
        let failure = super::curve_point(ctx, &geometry, 0.5).unwrap_err();
        let EvaluationFailure::ResourceLimit(limit) = failure else {
            panic!("session depth refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
        assert_eq!((limit.limit, limit.used, limit.additional), (0, 0, 1));
        assert_eq!(limit.operation, "geometry evaluation nesting");
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert_eq!(
            super::outer_refusal::<crate::features::FinitePoint3, Point3>(Err(failure)),
            Err(limit)
        );
        assert_eq!(super::curve_point(ctx, &geometry, 0.5), Err(failure));
    });
}

#[test]
fn a_prior_session_refusal_precedes_no_value_at_scratch_and_entries() {
    use crate::eval::EvaluationFailure;
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use crate::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
    use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use crate::math::Point2;
    let curve = curve();
    let solved_curve = SolvedCurveGeometry::Nurbs(curve.clone());
    let geometry = CurveGeometry::Solved(solved_curve.clone());
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(0.5, 0.0),
                Point2::new(1.0, 0.0),
            ],
            Some(vec![1.0; 3]),
            false,
        )
        .unwrap()
        .unwrap(),
    };
    let surface = NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
            None,
        ),
        false,
    )
    .unwrap()
    .unwrap();
    let solved_surface = SolvedSurfaceGeometry::Nurbs(surface.clone());
    let surface_geometry = SurfaceGeometry::Solved(solved_surface.clone());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scratch = super::Scratch::new(&ctx);
    let original = ctx
        .enter_nested_limit("external geometry refusal")
        .expect_err("zero depth refuses");
    assert_eq!(scratch.refused(), Some(original.into()));
    assert_eq!(scratch.unless_refused(), Err(original.into()));
    assert_eq!(
        scratch.failure::<()>(EvaluationFailure::NoValue),
        EvaluationFailure::ResourceLimit(original)
    );
    assert_eq!(
        scratch.settle::<(), ()>(Ok(())),
        Err(EvaluationFailure::ResourceLimit(original))
    );
    assert_eq!(
        scratch.settle::<(), ()>(Err(EvaluationFailure::NoValue)),
        Err(EvaluationFailure::ResourceLimit(original))
    );
    assert_eq!(
        super::outer_refusal(super::curve_point(&ctx, &geometry, f64::NAN)),
        Err(original)
    );
    assert_eq!(
        super::outer_refusal(super::nurbs_curve_point_at(&ctx, &curve, f64::NAN)),
        Err(original)
    );
    assert_eq!(
        super::outer_refusal(super::curve_point_solved(&ctx, &solved_curve, f64::NAN)),
        Err(original)
    );
    assert_eq!(
        super::outer_refusal(super::curve_tangent(&ctx, &geometry, f64::NAN)),
        Err(original)
    );
    assert_eq!(
        super::outer_refusal(super::pcurve_uv(&ctx, &pcurve, f64::NAN)),
        Err(original)
    );
    assert_eq!(
        super::outer_refusal(super::surface_point(
            &ctx,
            &surface_geometry,
            f64::NAN,
            f64::NAN
        )),
        Err(original)
    );
    assert_eq!(
        super::outer_refusal(super::surface_point_solved(
            &ctx,
            &solved_surface,
            f64::NAN,
            f64::NAN
        )),
        Err(original)
    );
    assert_eq!(
        super::outer_refusal(super::nurbs_surface_point(
            &ctx,
            &surface,
            f64::NAN,
            f64::NAN
        )),
        Err(original)
    );
    assert!(
        matches!(crate::eval::nurbs_pcurve_uv(&ctx, 0, &[], &[], None, f64::NAN),
        Err(EvaluationFailure::ResourceLimit(limit)) if limit == original)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
}
