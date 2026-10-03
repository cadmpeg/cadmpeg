// SPDX-License-Identifier: Apache-2.0

use crate::eval::test_support::with_policy;
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::{CurveGeometry, SolvedCurveGeometry};
use crate::math::Point3;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn curve() -> NurbsCurve {
    NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.5, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
        None,
        false,
    ).expect("fixture constructor admission")
    .expect("finite line spline")
}

#[test]
fn admitted_curve_point_refuses_each_scratch_collection() {
    let curve = curve();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(
        matches!(super::nurbs_curve_point_at_for_decode(&ctx, &curve, 0.5).map_err(CodecError::from),
        Err(CodecError::ResourceLimit(resource)) if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "IR B-spline basis")
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        super::nurbs_curve_point_at_for_decode(&ctx, &curve, 0.5)
            .map_err(CodecError::from)
            .expect("exact scratch cap"),
        crate::eval::nurbs_curve_point_at(&curve, 0.5)
    );
}

#[test]
fn admitted_curve_point_refuses_basis_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 8;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(
        matches!(super::nurbs_curve_point_at_for_decode(&ctx, &curve(), 0.5).map_err(CodecError::from),
        Err(CodecError::ResourceLimit(resource)) if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "IR B-spline basis work")
    );
}

#[test]
fn admitted_curve_tangent_refuses_point_copy() {
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve()));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(
        matches!(super::curve_tangent_for_decode(&ctx, &geometry, 0.5).map_err(CodecError::from),
        Err(CodecError::ResourceLimit(resource)) if resource.operation == "IR NURBS derivative points")
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        super::curve_tangent_for_decode(&ctx, &geometry, 0.5)
            .map_err(CodecError::from)
            .expect("service scratch"),
        crate::eval::curve_tangent(&geometry, 0.5)
    );
}

#[test]
fn admitted_surface_point_refuses_both_axis_bases() {
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    let surface = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), 
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            (0..3)
                .map(|u| {
                    (0..3)
                        .map(|v| Point3::new(f64::from(u) * 0.5, f64::from(v) * 0.5, 0.0))
                        .collect()
                })
                .collect(),
            None,
        ),
        false,
    ).expect("fixture constructor admission")
    .expect("finite plane spline");
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface));
    for cap in [2, 5] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(
            matches!(super::surface_point_for_decode(&ctx, &geometry, 0.5, 0.5).map_err(CodecError::from),
            Err(CodecError::ResourceLimit(resource)) if resource.operation == "IR B-spline basis")
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 6;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        super::surface_point_for_decode(&ctx, &geometry, 0.5, 0.5)
            .map_err(CodecError::from)
            .expect("exact cap"),
        crate::eval::surface_point(&geometry, 0.5, 0.5)
    );
}

#[test]
fn admitted_pcurve_point_refuses_weights_poles_and_derivative_bases() {
    use crate::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
    use crate::math::Point2;
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(&cadmpeg_test_support::service_decode_context(), 
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(0.5, 0.0),
                Point2::new(1.0, 0.0),
            ],
            Some(vec![1.0, 1.0, 1.0]),
            false,
        ).expect("fixture pcurve construction admission")
        .expect("finite rational line pcurve"),
    };
    for (cap, operation) in [
        (2, "IR NURBS pcurve weights"),
        (5, "IR B-spline basis"),
        (10, "IR B-spline derivative basis"),
        (13, "IR B-spline second derivative basis"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(
            matches!(super::pcurve_uv_for_decode(&ctx, &pcurve, 0.5).map_err(CodecError::from),
            Err(CodecError::ResourceLimit(resource)) if resource.operation == operation)
        );
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        super::pcurve_uv_for_decode(&ctx, &pcurve, 0.5)
            .map_err(CodecError::from)
            .expect("service scratch"),
        crate::eval::pcurve_uv(&pcurve, 0.5)
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
        matches!(super::curve_point_for_decode(&ctx, &geometry, 0.5).map_err(CodecError::from),
        Err(CodecError::ResourceLimit(resource)) if resource.dimension == ResourceDimension::RecursionDepth
            && resource.operation == "geometry evaluation nesting")
    );
}

#[test]
fn reusable_nurbs_evaluator_admits_once_and_matches_point_evaluation() {
    let curve = curve();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(
        matches!(super::NurbsPointEvaluator::new(&ctx, &curve).map_err(CodecError::from),
        Err(CodecError::ResourceLimit(resource)) if resource.operation == "IR B-spline basis")
    );
    let arena = DecodeArena::new();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut evaluator = super::NurbsPointEvaluator::new(&ctx, &curve)
        .map_err(CodecError::from)
        .expect("exact scratch cap");
    for index in 0..10_000 {
        let parameter = f64::from(index % 101) / 100.0;
        assert_eq!(
            evaluator
                .point(&ctx, parameter)
                .map_err(CodecError::from)
                .expect("reused storage"),
            crate::eval::nurbs_curve_point_at(&curve, parameter)
        );
    }
}

#[test]
fn reusable_nurbs_evaluator_refuses_work_and_depth() {
    let curve = curve();
    for (work, depth, operation) in [
        (8, 128, "IR B-spline basis work"),
        (u64::MAX, 0, "geometry evaluation nesting"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_recursion_depth = depth;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut evaluator = super::NurbsPointEvaluator::new(&ctx, &curve)
            .map_err(CodecError::from)
            .expect("scratch storage");
        assert!(
            matches!(evaluator.point(&ctx, 0.5).map_err(CodecError::from), Err(CodecError::ResourceLimit(resource)) if resource.operation == operation)
        );
    }
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
            let curve = NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 
                degree,
                knots,
                points,
                rational.then(|| vec![1.0; count]),
                false,
            ).expect("fixture constructor admission")
            .expect("fixed span");
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_work_units = 0;
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
                        super::nurbs_curve_point_at_for_decode(ctx, &curve, parameter)
                            .map_err(CodecError::from)
                            .expect("inline prior path")
                    );
                }
            });
        }
    }
}

#[test]
fn admitted_curve_tangent_refuses_rational_weight_copy() {
    let curve = curve();
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 
            curve.degree(),
            curve.knots().to_vec(),
            curve
                .control_points()
                .into_iter()
                .map(crate::features::FinitePoint3::get)
                .collect(),
            Some(vec![1.0; 3]),
            false,
        ).expect("fixture constructor admission")
        .expect("rational curve"),
    ));
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 5;
    assert!(
        matches!(with_policy(policy, |ctx| super::curve_tangent_for_decode(ctx, &geometry, 0.5).map_err(CodecError::from)),
        Err(CodecError::ResourceLimit(resource)) if resource.operation == "IR NURBS derivative weights")
    );
    assert_eq!(
        with_policy(DecodePolicy::service(), |ctx| {
            super::curve_tangent_for_decode(ctx, &geometry, 0.5).map_err(CodecError::from)
        })
        .expect("service"),
        crate::eval::curve_tangent(&geometry, 0.5)
    );
}

#[test]
fn admitted_polyline_tangent_refuses_points_and_parameters() {
    use crate::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
    for parameterized in [false, true] {
        let points = vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 1.0, 0.0),
        ];
        let samples = if parameterized {
            PolylineSamples::Parameterized {
                vertices: points
                    .into_iter()
                    .enumerate()
                    .map(|(index, point)| PolylineVertex {
                        parameter: f64::from(u32::try_from(index).expect("three points")),
                        point,
                    })
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("nonempty vertices"),
            }
        } else {
            PolylineSamples::Unparameterized {
                points: points.try_into().expect("nonempty points"),
            }
        };
        let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Polyline(
            PolylineCurve::new(samples, 0.0, &cadmpeg_test_support::service_decode_context()).expect("polyline construction admission").expect("polyline"),
        ));
        for (cap, operation) in [
            (2, "IR polyline derivative points"),
            (5, "IR polyline derivative parameters"),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            assert!(
                matches!(with_policy(policy, |ctx| super::curve_tangent_for_decode(ctx, &geometry, 0.5).map_err(CodecError::from)),
                Err(CodecError::ResourceLimit(resource)) if resource.operation == operation)
            );
        }
        assert_eq!(
            with_policy(DecodePolicy::service(), |ctx| {
                super::curve_tangent_for_decode(ctx, &geometry, 0.5).map_err(CodecError::from)
            })
            .expect("service"),
            crate::eval::curve_tangent(&geometry, 0.5)
        );
    }
}

#[test]
fn admitted_polar_pcurve_refuses_weight_copy() {
    use crate::geometry::pcurve::{PcurveGeometry, PolarNurbsPole, PolarPcurveNurbs};
    use crate::math::Point2;
    let geometry = PcurveGeometry::PolarNurbs {
        nurbs: PolarPcurveNurbs::from_lanes(&cadmpeg_test_support::service_decode_context(), 
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                PolarNurbsPole {
                    radial: Point2::new(1.0, 0.0),
                    axial: 0.0,
                },
                PolarNurbsPole {
                    radial: Point2::new(1.0, 0.5),
                    axial: 0.5,
                },
                PolarNurbsPole {
                    radial: Point2::new(1.0, 1.0),
                    axial: 1.0,
                },
            ],
            Some(vec![1.0; 3]),
            false,
        ).expect("fixture pcurve construction admission")
        .expect("polar pcurve"),
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    assert!(
        matches!(with_policy(policy, |ctx| super::pcurve_uv_for_decode(ctx, &geometry, 0.5).map_err(CodecError::from)),
        Err(CodecError::ResourceLimit(resource)) if resource.operation == "IR polar NURBS weights")
    );
    assert_eq!(
        with_policy(DecodePolicy::service(), |ctx| super::pcurve_uv_for_decode(
            ctx, &geometry, 0.5
        )
        .map_err(CodecError::from))
        .expect("service"),
        crate::eval::pcurve_uv(&geometry, 0.5)
    );
}

#[test]
fn scratch_fill_admits_each_clone_before_it_runs() {
    use std::cell::Cell;
    use std::rc::Rc;
    struct Counted {
        value: u8,
        clones: Rc<Cell<u64>>,
    }
    impl Clone for Counted {
        fn clone(&self) -> Self {
            self.clones.set(self.clones.get() + 1);
            Self { value: self.value, clones: Rc::clone(&self.clones) }
        }
    }
    for allowance in 0..=3 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowance;
        policy.limits.max_materialized_bytes = 200;
        policy.limits.max_collection_items = 3;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = super::Scratch::new(&ctx);
        let clones = Rc::new(Cell::new(0));
        let result = scratch.filled(3, Counted { value: 7, clones: Rc::clone(&clones) }, "scratch fill storage", "scratch fill clones");
        assert_eq!(clones.get(), allowance);
        if allowance < 3 {
            assert!(result.is_none());
            let original = scratch.refused().unwrap();
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.operation, "scratch fill clones");
            drop(scratch);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
        } else {
            let result = result.unwrap();
            assert_eq!(result.iter().map(|value| value.value).collect::<Vec<_>>(), vec![7, 7, 7]);
            drop(result);
            drop(scratch);
            let storage = ctx.reserve_scoped_limit(200, "fill storage released").unwrap();
            drop(storage);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn scratch_collect_admits_each_iterator_read_before_it_runs() {
    use std::cell::Cell;
    for allowance in 0..=3 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowance;
        policy.limits.max_materialized_bytes = 200;
        policy.limits.max_collection_items = 3;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = super::Scratch::new(&ctx);
        let reads = Cell::new(0);
        let result = scratch.collect((0..3).map(|value| { reads.set(reads.get() + 1); Some(value) }), "scratch collect storage", "scratch collect reads");
        assert_eq!(reads.get(), allowance);
        if allowance < 3 {
            assert!(result.is_none());
            let original = scratch.refused().unwrap();
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.operation, "scratch collect reads");
            drop(scratch);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
        } else {
            assert_eq!(result.unwrap(), vec![0, 1, 2]);
            drop(scratch);
            let storage = ctx.reserve_scoped_limit(200, "collect storage released").unwrap();
            drop(storage);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn scratch_collect_stops_at_absence_and_observes_a_fused_empty_session() {
    use std::cell::Cell;
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scratch = super::Scratch::new(&ctx);
    let reads = Cell::new(0);
    let result = scratch.collect((0..3).map(|value| { reads.set(reads.get() + 1); (value != 1).then_some(value) }), "scratch absent storage", "scratch absent reads");
    assert_eq!(result, None);
    assert_eq!(reads.get(), 2);
    assert_eq!(scratch.refused(), None);
    drop(scratch);
    let original = ctx.charge_work_limit(1, "original empty scratch refusal").unwrap_err();
    let scratch = super::Scratch::new(&ctx);
    assert_eq!(scratch.collect(std::iter::empty::<Option<u8>>(), "empty storage", "empty reads"), None);
    assert_eq!(scratch.refused(), Some(original));
    drop(scratch);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn uncharged_scratch_reports_an_allocation_refusal_instead_of_no_value() {
    use crate::eval::EvaluationFailure;
    use cadmpeg_core::decode::ResourceFailure;

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
    let scratch = super::Scratch::new(&ctx);
    assert!(scratch
        .filled(usize::MAX, 0_u8, "IR test scratch", "IR test scratch work")
        .is_none());
    let refusal = scratch
        .refused()
        .expect("the allocation refusal is recorded");
    assert_eq!(refusal.reason, ResourceFailure::BudgetExceeded);
    assert_eq!(refusal.operation, "IR test scratch");
    assert!(matches!(
        scratch.settle::<(), ()>(Ok(())),
        Err(EvaluationFailure::ResourceLimit(limit)) if limit == refusal
    ));
    assert!(scratch.work(1, "IR test scratch work").is_none());
}

#[test]
fn decode_evaluation_scratch_charges_scoped_bytes_and_releases_them() {
    let curve = curve();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let bytes = 4 * std::mem::size_of::<f64>();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = u64::try_from(bytes).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let point = super::nurbs_curve_point_at_for_decode(&ctx, &curve, 0.5).unwrap();
    assert_eq!(point, crate::eval::nurbs_curve_point_at(&curve, 0.5));
    let reservation = ctx
        .reserve_scoped(u64::try_from(bytes).unwrap(), "reuse basis storage")
        .unwrap();
    drop(reservation);
}

#[test]
fn decode_evaluation_refuses_scoped_basis_storage() {
    let curve = curve();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes =
        u64::try_from(3 * std::mem::size_of::<f64>() - 1).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(super::nurbs_curve_point_at_for_decode(&ctx, &curve, 0.5),
        Err(limit) if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "IR B-spline basis")
    );
}

#[test]
fn scratch_completion_observes_refusals_from_other_context_operations() {
    use crate::eval::EvaluationFailure;
    for dimension in [ResourceDimension::WorkUnits, ResourceDimension::MaterializedBytes, ResourceDimension::RetainedBytes, ResourceDimension::CollectionItems, ResourceDimension::RecursionDepth] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = super::Scratch::new(&ctx);
        let original = match dimension {
            ResourceDimension::WorkUnits => ctx.charge_work_limit(1, "external geometry refusal").unwrap_err(),
            ResourceDimension::MaterializedBytes => ctx.reserve_scoped_limit(1, "external geometry refusal").unwrap_err(),
            ResourceDimension::RetainedBytes => ctx.charge_retained_limit(1, "external geometry refusal").unwrap_err(),
            ResourceDimension::CollectionItems => ctx.charge_collection_items_limit(1, "external geometry refusal").unwrap_err(),
            ResourceDimension::RecursionDepth => ctx.enter_nested_limit("external geometry refusal").err().unwrap(),
            _ => unreachable!(),
        };
        assert_eq!(scratch.refused(), Some(original));
        assert_eq!(scratch.unless_refused(), Err(original));
        assert_eq!(scratch.failure::<()>(EvaluationFailure::NoValue), EvaluationFailure::ResourceLimit(original));
        assert_eq!(scratch.settle::<(), ()>(Ok(())), Err(EvaluationFailure::ResourceLimit(original)));
        assert_eq!(scratch.settle::<(), ()>(Err(EvaluationFailure::NoValue)), Err(EvaluationFailure::ResourceLimit(original)));
        assert_eq!(scratch.finish(()), Err(original));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
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
    let values = scratch.filled(3, Counted(Rc::clone(&clones)), "standard fill", "standard clone").unwrap();
    assert_eq!(values.len(), 3);
    assert_eq!(clones.get(), 3);
    let reads = Cell::new(0);
    assert!(scratch.collect((0..3).map(|index| {
        reads.set(reads.get() + 1);
        (index != 1).then_some(index)
    }), "standard collect", "standard read").is_none());
    assert_eq!(reads.get(), 2);
    let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let span = crate::eval::basis::bspline_span(scratch.admission, &knots, 2, 3, 0.5).unwrap().unwrap();
    assert_eq!(span, 2);
    assert_eq!(&*crate::eval::basis::bspline_basis(&scratch, &knots, 2, span, 0.5).unwrap(), &[0.25, 0.5, 0.25]);
    assert_eq!(crate::eval::basis::bspline_basis_derivative(&scratch, &knots, 2, span, 0.5).unwrap(), [-1.0, 0.0, 1.0]);
    assert_eq!(scratch.work(usize::MAX, "standard work"), Some(()));
    assert_eq!(scratch.finish(7), Ok(7));
}

#[test]
fn standard_evaluation_preserves_allocation_refusal() {
    use crate::eval::admission::EvaluationAdmission;
    use cadmpeg_core::decode::ResourceFailure;
    let scratch = super::Scratch::new(EvaluationAdmission::Standard);
    let mut values = vec![7_u8];
    assert_eq!(scratch.reserve(&mut values, usize::MAX, "standard allocation"), None);
    assert_eq!(values, [7]);
    let original = scratch.refused().unwrap();
    assert_eq!(original.reason, ResourceFailure::AllocationFailed);
    assert_eq!(original.operation, "standard allocation");
    assert_eq!(scratch.work(0, "after allocation refusal"), None);
    assert_eq!(scratch.finish(7), Err(original));
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
    for _ in 0..256 { guards.push(scratch.enter().unwrap()); }
    assert!(scratch.enter().is_none());
    let original = scratch.refused().unwrap();
    assert_eq!(original.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(original.limit, 256);
    assert_eq!(original.used, 256);
    assert_eq!(original.additional, 1);
    assert_eq!(original.operation, "independent geometry evaluation nesting");
    while let Some(guard) = guards.pop() { drop(guard); }
    drop(guards);
    assert_eq!(scratch.independent_depth.get(), 0);
    assert_eq!(scratch.finish(7), Err(original));
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
    let original = second.refused().unwrap();
    assert_eq!(original.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(original.limit, 1);
    assert_eq!(original.used, 1);
    assert_eq!(original.additional, 1);
    assert_eq!(original.operation, "geometry evaluation nesting");
    assert_eq!(first.refused(), Some(original));
    drop(frame);
    assert_eq!(first.finish(7), Err(original));
    assert_eq!(second.finish(7), Err(original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn geometry_entries_use_explicit_standard_storage() {
    use crate::eval::admission::EvaluationAdmission;
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use crate::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
    use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use crate::math::{Point2, Vector3};
    let curve = curve();
    let solved_curve = SolvedCurveGeometry::Nurbs(curve.clone());
    let geometry = CurveGeometry::Solved(solved_curve.clone());
    let ctx = cadmpeg_test_support::service_decode_context();
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(&ctx, 2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(0.5, 0.0), Point2::new(1.0, 0.0)],
            Some(vec![1.0; 3]), false).unwrap().unwrap(),
    };
    let surface = NurbsSurface::from_lanes(&ctx,
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(vec![
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
        ], None), false).unwrap().unwrap();
    let solved_surface = SolvedSurfaceGeometry::Nurbs(surface.clone());
    let surface_geometry = SurfaceGeometry::Solved(solved_surface.clone());
    let standard = EvaluationAdmission::Standard;
    let point = Point3::new(0.5, 0.0, 0.0);
    let surface_point = Point3::new(0.25, 0.75, 0.0);
    assert_eq!(super::curve_point_for_decode(standard, &geometry, 0.5).unwrap().unwrap().get(), point);
    assert_eq!(super::nurbs_curve_point_at_for_decode(standard, &curve, 0.5).unwrap().unwrap().get(), point);
    assert_eq!(super::curve_point_solved_for_decode(standard, &solved_curve, 0.5).unwrap().unwrap().get(), point);
    assert_eq!(super::curve_tangent_for_decode(standard, &geometry, 0.5).unwrap().unwrap().get(), Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(super::pcurve_uv_for_decode(standard, &pcurve, 0.5).unwrap().unwrap().get(), Point2::new(0.5, 0.0));
    assert_eq!(super::surface_point_for_decode(standard, &surface_geometry, 0.25, 0.75).unwrap().unwrap().get(), surface_point);
    assert_eq!(super::surface_point_solved_for_decode(standard, &solved_surface, 0.25, 0.75).unwrap().unwrap().get(), surface_point);
    assert_eq!(super::nurbs_surface_point_for_decode(standard, &surface, 0.25, 0.75).unwrap().unwrap().get(), surface_point);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = ctx.charge_work_limit(1, "original entry refusal").unwrap_err();
    assert_eq!(super::curve_point_for_decode(&ctx, &geometry, f64::NAN), Err(original));
    assert_eq!(super::nurbs_curve_point_at_for_decode(&ctx, &curve, f64::NAN), Err(original));
    assert_eq!(super::curve_point_solved_for_decode(&ctx, &solved_curve, f64::NAN), Err(original));
    assert_eq!(super::curve_tangent_for_decode(&ctx, &geometry, f64::NAN), Err(original));
    assert_eq!(super::pcurve_uv_for_decode(&ctx, &pcurve, f64::NAN), Err(original));
    assert_eq!(super::surface_point_for_decode(&ctx, &surface_geometry, f64::NAN, f64::NAN), Err(original));
    assert_eq!(super::surface_point_solved_for_decode(&ctx, &solved_surface, f64::NAN, f64::NAN), Err(original));
    assert_eq!(super::nurbs_surface_point_for_decode(&ctx, &surface, f64::NAN, f64::NAN), Err(original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}
