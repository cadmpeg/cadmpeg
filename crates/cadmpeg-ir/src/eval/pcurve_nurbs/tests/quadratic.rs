// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::admission::EvaluationAdmission;

const EPS_QUADRATIC_PCURVE: f64 = 1.0e-9;

fn quadratic(sign: f64) -> PcurveNurbs {
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = PcurveNurbs::from_lanes(&ctx, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0 / 3.0, -2.0 / 3.0), Point2::new(0.5, -1.0)],
        Some(vec![sign, 1.5 * sign, 2.0 * sign]), false).unwrap().unwrap();
    ctx.finish_session().unwrap(); result
}

#[test]
fn requested_quadratic_rational_pcurve_has_true_orders_and_identical_lower_results() {
    // Degree elevation of t/(1+t): H=t and W=1+t. Binary64 1/3
    // perturbs H by at most the rounding of its authored middle pole.
    for sign in [-1.0, 1.0] {
        let curve = quadratic(sign);
        let policy = DecodePolicy::service(); let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            let scratch = decode::Scratch::new(admission);
            let poles = DifferentialPoles::Stored(curve.pole_rows());
            for t in [0.0, 0.5, 1.0] {
                let parameter = FiniteReal::new(t).unwrap();
                let old = differential(&scratch, curve.degree(), curve.knots(), poles, parameter).unwrap();
                for max_order in [2, 3, 4, 5] {
                    let actual = differential_requested(&scratch, curve.degree(), curve.knots(), poles, parameter, max_order).unwrap();
                    assert_eq!(actual.point, old.point); assert_eq!(actual.tangent, old.tangent);
                    assert_eq!(actual.acceleration, old.acceleration);
                    let w = 1.0 + t;
                    for (at, (value, expected)) in actual.higher.into_iter()
                        .zip([6.0 / w.powi(4), -24.0 / w.powi(5), 120.0 / w.powi(6)]).enumerate() {
                        if at + 3 > max_order { assert_eq!(value, Err(EvaluationFailure::NoValue)); }
                        else {
                            let value = value.unwrap().get();
                            assert!((value.u - expected).abs() <= EPS_QUADRATIC_PCURVE);
                            assert!((value.v + 2.0 * expected).abs() <= EPS_QUADRATIC_PCURVE);
                        }
                    }
                }
            }
        }
        ctx.finish_session().unwrap();
    }
}

#[test]
fn fixed_quadratic_pcurve_owner_is_free_and_keeps_original_fuse() {
    let curve = quadratic(1.0);
    let mut policy = DecodePolicy::service(); policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0; policy.limits.max_collection_items = 0;
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = decode::Scratch::new(admission);
        for max_order in [2, 3, 4, 5] {
            // This is the actual fixed owner over stored source rows, without
            // a dummy seed. The old lower operation owns its separate costs.
            let actual = super::super::higher::quadratic(&scratch, curve.knots(),
                DifferentialPoles::Stored(curve.pole_rows()), FiniteReal::ZERO, max_order).expect("actual fixed quadratic carrier");
            for (at, (value, expected)) in actual.into_iter().zip([6.0, -24.0, 120.0]).enumerate() {
                if at + 3 > max_order { assert_eq!(value, Err(EvaluationFailure::NoValue)); }
                else {
                    let value = value.unwrap().get();
                    assert!((value.u - expected).abs() <= EPS_QUADRATIC_PCURVE);
                    assert!((value.v + 2.0 * expected).abs() <= EPS_QUADRATIC_PCURVE);
                }
            }
        }
    }
    let original = ctx.charge_work_limit(1, "actual quadratic pcurve prior refusal").unwrap_err();
    let scratch = decode::Scratch::new(&ctx);
    for max_order in [3, 4, 5] {
        let actual = super::super::higher::quadratic(&scratch, curve.knots(),
            DifferentialPoles::Stored(curve.pole_rows()), FiniteReal::ZERO, max_order).expect("actual fixed quadratic carrier");
        for (at, value) in actual.into_iter().enumerate() {
            assert_eq!(value, if at + 3 <= max_order { Err(EvaluationFailure::ResourceLimit(original)) }
                else { Err(EvaluationFailure::NoValue) });
        }
        assert!(matches!(differential_requested(&scratch, curve.degree(), curve.knots(),
            DifferentialPoles::Stored(curve.pole_rows()), FiniteReal::ZERO, max_order),
            Err(EvaluationFailure::ResourceLimit(limit)) if limit == original));
    }
    drop(scratch);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn other_rational_pcurve_shapes_keep_lower_results_and_missing_higher() {
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let curve = PcurveNurbs::from_lanes(&ctx, 2, vec![0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0), Point2::new(2.0, 0.0), Point2::new(3.0, 0.0)],
        Some(vec![1.0, 2.0, 1.0, 2.0]), false).unwrap().unwrap();
    ctx.finish_session().unwrap();
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = decode::Scratch::new(admission);
        let poles = DifferentialPoles::Stored(curve.pole_rows());
        let parameter = FiniteReal::new(0.5).unwrap();
        let old = differential(&scratch, curve.degree(), curve.knots(), poles, parameter).unwrap();
        let actual = differential_requested(&scratch, curve.degree(), curve.knots(), poles, parameter, 5).unwrap();
        assert_eq!(actual.point, old.point); assert_eq!(actual.tangent, old.tangent); assert_eq!(actual.acceleration, old.acceleration);
        // On this original first span H=4t-2t², W=1+2t-1.5t².
        // Differentiating W*C=H at t=.5 gives these exact rational controls.
        for (value, expected) in actual.higher.into_iter()
            .zip([185088.0 / 28561.0, -8472576.0 / 371293.0, 746926080.0 / 4826809.0]) {
            let value = value.unwrap().get();
            assert!((value.u - expected).abs() <= EPS_QUADRATIC_PCURVE);
            assert_eq!(value.v, 0.0);
        }
    }
    ctx.finish_session().unwrap();
}
