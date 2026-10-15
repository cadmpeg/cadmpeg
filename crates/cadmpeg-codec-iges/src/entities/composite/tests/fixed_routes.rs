// SPDX-License-Identifier: Apache-2.0

use super::super::{
    anchor_analytic_nurbs_endpoint_poles, elevate_bezier_homogeneous, reverse_nurbs,
    trim_nurbs_lanes, CompositeCurveError, CompositeEdge, DegreeElevationError,
};
use super::*;

fn rail() -> NurbsCurve {
    test_nurbs(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
    )
}

fn with_zero_context(mut run: impl FnMut(&cadmpeg_core::decode::DecodeContext<'_>)) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        run(&ctx);
    }
    ctx.finish_session().unwrap();
}

#[test]
fn composite_absent_endpoint_tolerance_leaves_poles_unchanged() {
    let curve = rail();
    let before = serde_json::to_value(&curve).unwrap();
    let ir = CadIr::empty();
    let edge = CompositeEdge {
        start: VertexId::mint("test:model:vertex#start").unwrap(),
        end: VertexId::mint("test:model:vertex#end").unwrap(),
        param_range: Some([0.0, 1.0]),
    };
    with_zero_context(|ctx| {
        let result = anchor_analytic_nurbs_endpoint_poles(
            ctx,
            curve.clone(),
            [0.0, 1.0],
            &ir,
            None,
            &edge,
            None,
        );
        let value = result.expect("fixed recovery is free");
        assert_eq!(serde_json::to_value(value.unwrap()).unwrap(), before);

        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
        assert_eq!(ir, CadIr::empty());
    });
}

#[test]
fn composite_invalid_bezier_shapes_have_no_elevated_controls_or_work() {
    let controls = [[1.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]];
    with_zero_context(|ctx| {
        for (source, source_degree, target_degree) in [
            (&[][..], usize::MAX, usize::MAX),
            (&controls[..1], 1, 2),
            (&controls[..], 1, 0),
        ] {
            let result = elevate_bezier_homogeneous(ctx, source, source_degree, target_degree);
            let value = result.expect("fixed recovery is free");
            assert!(value.is_none());
        }
        assert_eq!(controls, [[1.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]]);
    });
}

#[test]
fn composite_empty_concatenation_needs_no_work() {
    with_zero_context(|ctx| {
        let value = concatenate_nurbs(ctx, Vec::<(NurbsCurve, [f64; 2], ())>::new(), None);
        assert!(matches!(value, Err(CompositeCurveError::EmptyChildList)));
    });
}

#[test]
fn composite_empty_knot_insertion_needs_no_work() {
    with_zero_context(|ctx| {
        let value = insert_homogeneous_knot(ctx, &[], &[], 1, 0.5).expect("fixed recovery is free");
        assert!(value.is_none());
    });
}

#[test]
fn composite_invalid_elevation_degree_rejects_without_work() {
    let mut curve = rail();
    let before = serde_json::to_value(&curve).unwrap();
    with_zero_context(|ctx| {
        for target in [0, u32::MAX] {
            let value = elevate_nurbs_to_degree(ctx, &mut curve, [0.0, 1.0], target, None);
            if target == 0 {
                assert!(matches!(
                    value,
                    Err(CompositeCurveError::Elevation(
                        DegreeElevationError::TargetBelowSource {
                            target: 0,
                            child: 1
                        }
                    ))
                ));
            } else {
                assert!(matches!(
                    value,
                    Err(CompositeCurveError::Elevation(
                        DegreeElevationError::TargetDegree {
                            degree: u32::MAX,
                            ..
                        }
                    ))
                ));
            }
        }
        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    });
}

#[test]
fn composite_invalid_reversal_interval_rejects_without_work() {
    let curve = rail();
    let before = serde_json::to_value(&curve).unwrap();
    with_zero_context(|ctx| {
        for interval in [
            [1.0, 0.0],
            [-1.0, 1.0],
            [f64::NAN, 1.0],
            [0.0, f64::INFINITY],
        ] {
            let value = reverse_nurbs(ctx, curve.clone(), interval);
            if interval[0] == -1.0 {
                assert!(matches!(
                    value,
                    Err(CompositeCurveError::ReversedChildIntervalOutsideDomain { .. })
                ));
            } else {
                assert!(matches!(
                    value,
                    Err(CompositeCurveError::ReversedChildInterval { .. })
                ));
            }
        }
        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    });
}

#[test]
fn composite_invalid_trim_interval_rejects_without_work() {
    let curve = rail();
    let before = serde_json::to_value(&curve).unwrap();
    with_zero_context(|ctx| {
        for interval in [
            [1.0, 0.0],
            [-1.0, 1.0],
            [f64::NAN, 1.0],
            [0.0, f64::INFINITY],
        ] {
            let value = trim_nurbs_lanes(ctx, &curve, interval).expect("fixed recovery is free");
            assert!(value.is_none());
        }
        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    });
}

#[test]
fn composite_same_degree_elevation_needs_no_work() {
    let mut curve = rail();
    let before = serde_json::to_value(&curve).unwrap();
    with_zero_context(|ctx| {
        let value = elevate_nurbs_to_degree(ctx, &mut curve, [0.0, 1.0], 1, None);
        value.unwrap();

        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    });
}

#[test]
fn composite_unclamped_elevation_interval_rejects_without_work() {
    let mut curve = rail();
    let before = serde_json::to_value(&curve).unwrap();
    with_zero_context(|ctx| {
        let value = elevate_nurbs_to_degree(ctx, &mut curve, [0.25, 0.75], 2, None);
        assert!(matches!(
            value,
            Err(CompositeCurveError::Elevation(
                DegreeElevationError::UnclampedKnots {
                    start: 0.25,
                    end: 0.75
                }
            ))
        ));

        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    });
}
