// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::admission::EvaluationAdmission;

const EPS_PCURVE_HIGHER: f64 = 1.0e-9;

fn quintic() -> PcurveNurbs {
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let points = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0].map(|value| Point2::new(value, -2.0 * value));
    let result = PcurveNurbs::from_lanes(&ctx, 5, vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        1.0, 1.0, 1.0, 1.0, 1.0, 1.0], points.to_vec(), None, false).unwrap().unwrap();
    ctx.finish_session().unwrap(); result
}

#[test]
fn requested_polynomial_pcurve_orders_use_real_quintic_poles_and_keep_lower_results() {
    // Bernstein poles 2^i give q(t)=(1+t)^5, from binomial expansion.
    let curve = quintic();
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        for parameter in [0.0, 0.5] {
            let parameter = FiniteReal::new(parameter).unwrap();
            for max_order in [2, 3, 4, 5] {
                let scratch = decode::Scratch::new(admission);
                let poles = DifferentialPoles::Stored(curve.pole_rows());
                let old = differential(&scratch, curve.degree(), curve.knots(), poles, parameter).unwrap();
                let actual = differential_requested(&scratch, curve.degree(), curve.knots(), poles, parameter, max_order).unwrap();
                assert_eq!(actual.point, old.point); assert_eq!(actual.tangent, old.tangent);
                assert_eq!(actual.acceleration, old.acceleration);
                let t = parameter.get();
                let expected = [60.0 * (1.0 + t).powi(2), 120.0 * (1.0 + t), 120.0];
                for (at, derivative) in actual.higher.into_iter().enumerate() {
                    if at + 3 > max_order { assert_eq!(derivative, Err(EvaluationFailure::NoValue)); }
                    else {
                        let actual = derivative.unwrap().get();
                        assert!((actual.u - expected[at]).abs() <= EPS_PCURVE_HIGHER);
                        assert!((actual.v + 2.0 * expected[at]).abs() <= EPS_PCURVE_HIGHER);
                    }
                }
            }
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn requested_degree_two_pcurve_preserves_original_local_caps_without_full_pole_copy() {
    let PcurveGeometry::Nurbs { nurbs } = large_line(false, false) else { panic!("planar pcurve"); };
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 11; policy.limits.max_materialized_bytes = 512;
    policy.limits.max_retained_bytes = 0; policy.limits.max_work_units = 256;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = decode::Scratch::new(admission);
        let actual = differential_requested(&scratch, nurbs.degree(), nurbs.knots(),
            DifferentialPoles::Stored(nurbs.pole_rows()), FiniteReal::new(0.5).unwrap(), 5).unwrap();
        assert_eq!(actual.point.get(), Point2::new(0.5, 0.0));
        assert_eq!(actual.higher, [Ok(FinitePoint2::from_coordinates(FiniteReal::ZERO, FiniteReal::ZERO)); 3]);
    }
    drop(ctx.reserve_scoped_limit(512, "requested polynomial scratch released").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn requested_rational_pcurve_missing_higher_keeps_actual_lower_and_original_fuse() {
    let policy = DecodePolicy::service(); let setup_arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(&[], &setup_arena, &policy).unwrap();
    let curve = PcurveNurbs::from_lanes(&setup, 1, vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(0.5, 0.0)], Some(vec![1.0, 2.0]), false).unwrap().unwrap();
    setup.finish_session().unwrap();
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = decode::Scratch::new(admission);
        let poles = DifferentialPoles::Stored(curve.pole_rows());
        let parameter = FiniteReal::new(0.5).unwrap();
        let old = differential(&scratch, curve.degree(), curve.knots(), poles, parameter).unwrap();
        let actual = differential_requested(&scratch, curve.degree(), curve.knots(), poles, parameter, 5).unwrap();
        assert_eq!(actual.point, old.point); assert_eq!(actual.tangent, old.tangent);
        assert_eq!(actual.acceleration, old.acceleration); assert_eq!(actual.higher, [Err(EvaluationFailure::NoValue); 3]);
    }
    ctx.finish_session().unwrap();
    let mut policy = DecodePolicy::service(); policy.limits.max_work_units = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = ctx.charge_work_limit(1, "actual prior polynomial pcurve refusal").unwrap_err();
    let scratch = decode::Scratch::new(&ctx);
    for max_order in [2, 3, 4, 5] {
        assert!(matches!(differential_requested(&scratch, curve.degree(), curve.knots(),
            DifferentialPoles::Stored(curve.pole_rows()), FiniteReal::new(0.5).unwrap(), max_order),
            Err(EvaluationFailure::ResourceLimit(limit)) if limit == original));
    }
    drop(scratch);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}
