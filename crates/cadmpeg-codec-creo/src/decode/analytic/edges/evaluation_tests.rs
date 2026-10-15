// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;

fn line(periodic: bool) -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(if periodic { 0.0 } else { 0.5 }, 0.0, 0.0),
                Point3::new(if periodic { 0.0 } else { 1.0 }, 0.0, 0.0),
            ],
            None,
            periodic,
        )
        .expect("fixture constructor admission")
        .expect("finite spline"),
    ))
}

fn context_test(test: impl FnOnce(&DecodeContext<'_>), cap: u64) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    test(&ctx);
}

fn basis_refusal<T>(run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>) {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "IR B-spline basis",
        run,
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(resource) if resource.operation == "IR B-spline basis")
    );
}

#[test]
fn nonperiodic_endpoint_recovery_propagates_evaluator_refusal() {
    basis_refusal(|ctx| super::nonperiodic_nurbs_endpoint_points(ctx, &line(false)));

    context_test(
        |ctx| {
            assert_eq!(
                super::nonperiodic_nurbs_endpoint_points(ctx, &line(false)).expect("service"),
                Some([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]])
            );
        },
        u64::MAX,
    );
}

#[test]
fn nonperiodic_range_recovery_propagates_evaluator_refusal() {
    basis_refusal(|ctx| {
        super::nonperiodic_nurbs_edge_parameter_range(
            ctx,
            &line(false),
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        )
    });
}

#[test]
fn nonperiodic_orientation_propagates_evaluator_refusal() {
    basis_refusal(|ctx| {
        super::orient_nonperiodic_nurbs_edge_carrier(
            ctx,
            &mut line(false),
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        )
    });
}

#[test]
fn periodic_range_recovery_propagates_evaluator_refusal() {
    basis_refusal(|ctx| {
        super::full_periodic_nurbs_edge_parameter_range(ctx, &line(true), [0.0, 0.0, 0.0])
    });

    context_test(
        |ctx| {
            assert_eq!(
                super::full_periodic_nurbs_edge_parameter_range(ctx, &line(true), [0.0, 0.0, 0.0])
                    .expect("service"),
                Some([0.0, 1.0])
            );
        },
        u64::MAX,
    );
}

#[test]
fn degree_one_parameter_search_propagates_evaluator_refusal() {
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("linear spline"),
    ));
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = &geometry else {
        panic!("spline fixture")
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(
        matches!(super::degree_one_nurbs_point_parameter(&ctx, &geometry, nurbs, [0.5, 0.0, 0.0], [0.0, 1.0], super::EPS_AGREE), Err(CodecError::ResourceLimit(resource)) if resource.operation == "geometry evaluation nesting")
    );
    context_test(
        |ctx| {
            assert_eq!(
                super::degree_one_nurbs_point_parameter(
                    ctx,
                    &geometry,
                    nurbs,
                    [0.5, 0.0, 0.0],
                    [0.0, 1.0],
                    super::EPS_AGREE
                )
                .expect("service"),
                Some(0.5)
            );
        },
        u64::MAX,
    );
}

#[test]
fn point_pair_alignment_needs_no_input_sized_work() {
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]
        .map(|point| super::super::vertices::finite_model_point(point).expect("finite fixture"));
    assert_eq!(super::point_pair_alignments(points, points), [true, false]);
}
