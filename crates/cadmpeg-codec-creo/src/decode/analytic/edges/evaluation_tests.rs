// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;

/// Highest degree whose basis is evaluated in inline storage is 15; this
/// degree needs a heap basis.
const DEGREE: usize = 16;

fn line(periodic: bool) -> CurveGeometry {
    let poles = DEGREE + 1;
    let mut knots = vec![0.0; poles];
    knots.extend(vec![1.0; poles]);
    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            u32::try_from(DEGREE).expect("fixture degree"),
            knots,
            (0..poles)
                .map(|pole| {
                    let x = if periodic {
                        0.0
                    } else {
                        f64::from(u32::try_from(pole).expect("fixture pole")) / 16.0
                    };
                    Point3::new(x, 0.0, 0.0)
                })
                .collect(),
            None,
            periodic,
        )
        .expect("finite spline"),
    ))
}

#[test]
fn nonperiodic_endpoint_recovery_reads_both_endpoints() {
    assert_eq!(
        crate::decode::with_service_context(|ctx| {
            super::nonperiodic_nurbs_endpoint_points(ctx, &line(false))
        })
        .expect("endpoint recovery"),
        Some([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]])
    );
}

#[test]
fn periodic_range_recovery_spans_the_unit_interval() {
    assert_eq!(
        crate::decode::with_service_context(|ctx| {
            super::full_periodic_nurbs_edge_parameter_range(ctx, &line(true), [0.0, 0.0, 0.0])
        })
        .expect("periodic range recovery"),
        Some([0.0, 1.0])
    );
}

#[test]
fn degree_one_parameter_search_propagates_evaluator_refusal() {
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("linear spline"),
    ));
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = &geometry else {
        panic!("spline fixture")
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(matches!(
        super::degree_one_nurbs_point_parameter(
            &ctx, &geometry, nurbs, [0.5, 0.0, 0.0], [0.0, 1.0], super::EPS_AGREE
        ),
        Err(CodecError::ResourceLimit(resource))
            if resource.operation == "geometry evaluation nesting"
    ));
    assert_eq!(
        crate::decode::with_service_context(|ctx| super::degree_one_nurbs_point_parameter(
            ctx,
            &geometry,
            nurbs,
            [0.5, 0.0, 0.0],
            [0.0, 1.0],
            super::EPS_AGREE
        ))
        .expect("service"),
        Some(0.5)
    );
}
