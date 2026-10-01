// SPDX-License-Identifier: Apache-2.0

use crate::eval::test_support::with_policy;
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::{CurveGeometry, SolvedCurveGeometry};
use crate::math::Point3;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn curve() -> NurbsCurve {
    NurbsCurve::from_lanes(
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
    let surface = NurbsSurface::from_lanes(
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
    )
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
        nurbs: PcurveNurbs::from_lanes(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(0.5, 0.0),
                Point2::new(1.0, 0.0),
            ],
            Some(vec![1.0, 1.0, 1.0]),
            false,
        )
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
            let knots = (0..count)
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
                degree,
                knots,
                points,
                rational.then(|| vec![1.0; count]),
                false,
            )
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
        NurbsCurve::from_lanes(
            curve.degree(),
            curve.knots().to_vec(),
            curve
                .control_points()
                .into_iter()
                .map(crate::features::FinitePoint3::get)
                .collect(),
            Some(vec![1.0; 3]),
            false,
        )
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
            PolylineCurve::new(samples, 0.0).expect("polyline"),
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
        nurbs: PolarPcurveNurbs::from_lanes(
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
        )
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
fn uncharged_scratch_reports_an_allocation_refusal_instead_of_no_value() {
    use crate::eval::EvaluationFailure;
    use cadmpeg_core::decode::ResourceFailure;

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
    let scratch = super::Scratch::new(&ctx);
    assert!(scratch
        .filled(usize::MAX, 0_u8, "IR test scratch")
        .is_none());
    let refusal = scratch
        .refused()
        .expect("the allocation refusal is recorded");
    assert_eq!(refusal.reason, ResourceFailure::AllocationFailed);
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
