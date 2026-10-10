// SPDX-License-Identifier: Apache-2.0

use super::super::{
    anchor_analytic_nurbs_endpoint_poles, elevate_bezier_homogeneous, reverse_nurbs,
    trim_nurbs_lanes, CompositeCurveError, CompositeEdge, DegreeElevationError,
};
use super::*;
use cadmpeg_core::decode::ResourceLimit;

fn rail() -> NurbsCurve {
    test_nurbs(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
    )
}

fn fixed<T>(result: Result<T, CodecError>, original: Option<ResourceLimit>) -> Option<T> {
    match original {
        Some(first) => {
            assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
            None
        }
        None => Some(result.expect("fixed recovery is free")),
    }
}

fn geometric<T>(
    result: Result<T, CompositeCurveError>,
    original: Option<ResourceLimit>,
) -> Option<Result<T, CompositeCurveError>> {
    match original {
        Some(first) => {
            let Err(error) = result else {
                panic!("original refusal must precede geometric recovery")
            };
            assert!(
                matches!(error.non_resource(), Err(CodecError::ResourceLimit(last)) if last == first)
            );
            None
        }
        None => Some(result),
    }
}

#[test]
fn composite_bezier_fixed_shape_recovery_preserves_original_refusal() {
    let controls = [[1.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]];
    crate::test_support::with_entry_context(|ctx, original| {
        for (source, source_degree, target_degree) in [
            (&[][..], usize::MAX, usize::MAX),
            (&controls[..1], 1, 2),
            (&controls[..], 1, 0),
        ] {
            let result = elevate_bezier_homogeneous(ctx, source, source_degree, target_degree);
            if let Some(value) = fixed(result, original) {
                assert!(value.is_none());
            }
        }
        assert_eq!(controls, [[1.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]]);
    });
}

#[test]
fn composite_empty_knot_insertion_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        if let Some(value) = fixed(insert_homogeneous_knot(ctx, &[], &[], 1, 0.5), original) {
            assert!(value.is_none());
        }
    });
}

#[test]
fn composite_invalid_trim_interval_preserves_original_refusal() {
    let curve = rail();
    let before = serde_json::to_value(&curve).unwrap();
    crate::test_support::with_entry_context(|ctx, original| {
        for interval in [
            [1.0, 0.0],
            [-1.0, 1.0],
            [f64::NAN, 1.0],
            [0.0, f64::INFINITY],
        ] {
            if let Some(value) = fixed(trim_nurbs_lanes(ctx, &curve, interval), original) {
                assert!(value.is_none());
            }
        }
        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    });
}

#[test]
fn composite_same_degree_elevation_preserves_original_refusal() {
    let mut curve = rail();
    let before = serde_json::to_value(&curve).unwrap();
    crate::test_support::with_entry_context(|ctx, original| {
        if let Some(value) = geometric(
            elevate_nurbs_to_degree(ctx, &mut curve, [0.0, 1.0], 1, None),
            original,
        ) {
            value.unwrap();
        }
        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    });
}

#[test]
fn composite_invalid_elevation_degree_preserves_original_refusal() {
    let mut curve = rail();
    let before = serde_json::to_value(&curve).unwrap();
    crate::test_support::with_entry_context(|ctx, original| {
        for target in [0, u32::MAX] {
            if let Some(value) = geometric(
                elevate_nurbs_to_degree(ctx, &mut curve, [0.0, 1.0], target, None),
                original,
            ) {
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
        }
        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    });
}

#[test]
fn composite_unclamped_elevation_interval_preserves_original_refusal() {
    let mut curve = rail();
    let before = serde_json::to_value(&curve).unwrap();
    crate::test_support::with_entry_context(|ctx, original| {
        if let Some(value) = geometric(
            elevate_nurbs_to_degree(ctx, &mut curve, [0.25, 0.75], 2, None),
            original,
        ) {
            assert!(matches!(
                value,
                Err(CompositeCurveError::Elevation(
                    DegreeElevationError::UnclampedKnots {
                        start: 0.25,
                        end: 0.75
                    }
                ))
            ));
        }
        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    });
}

#[test]
fn composite_invalid_reversal_interval_preserves_original_refusal() {
    let curve = rail();
    let before = serde_json::to_value(&curve).unwrap();
    crate::test_support::with_entry_context(|ctx, original| {
        for interval in [
            [1.0, 0.0],
            [-1.0, 1.0],
            [f64::NAN, 1.0],
            [0.0, f64::INFINITY],
        ] {
            if let Some(value) = geometric(reverse_nurbs(ctx, curve.clone(), interval), original) {
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
        }
        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    });
}

#[test]
fn composite_empty_concatenation_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        if let Some(value) = geometric(
            concatenate_nurbs(ctx, Vec::<(NurbsCurve, [f64; 2], ())>::new(), None),
            original,
        ) {
            assert!(matches!(value, Err(CompositeCurveError::EmptyChildList)));
        }
    });
}

#[test]
fn composite_absent_endpoint_tolerance_preserves_original_refusal() {
    let curve = rail();
    let before = serde_json::to_value(&curve).unwrap();
    let ir = CadIr::empty();
    let edge = CompositeEdge {
        start: VertexId::mint("test:model:vertex#start").unwrap(),
        end: VertexId::mint("test:model:vertex#end").unwrap(),
        param_range: Some([0.0, 1.0]),
    };
    crate::test_support::with_entry_context(|ctx, original| {
        let result = anchor_analytic_nurbs_endpoint_poles(
            ctx,
            curve.clone(),
            [0.0, 1.0],
            &ir,
            None,
            &edge,
            None,
        );
        if let Some(value) = fixed(result, original) {
            assert_eq!(serde_json::to_value(value.unwrap()).unwrap(), before);
        }
        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
        assert_eq!(ir, CadIr::empty());
    });
}
