// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::admission::EvaluationAdmission;

const EPS_RATIONAL_PCURVE: f64 = 1.0e-9;
// Core budget materialized_allowance=min(policy,16MiB+1000*root_len).
const EMPTY_ROOT_MATERIALIZED_ALLOWANCE: u64 = 16 * 1024 * 1024;

fn elevated(sign: f64, width: f64) -> PcurveNurbs {
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    // Degree5 elevation of H=t,W=1+t: H_i=i/5,W_i=1+i/5.
    let weights = (0..=5).map(|i| sign * (1.0 + f64::from(i) / 5.0)).collect();
    let points = (0..=5).map(|i| {
        let q = f64::from(i) / f64::from(5 + i); Point2::new(q, -2.0 * q)
    }).collect();
    let result = PcurveNurbs::from_lanes(&ctx, 5,
        vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, width, width, width, width, width, width],
        points, Some(weights), false).unwrap().unwrap();
    ctx.finish_session().unwrap(); result
}

#[test]
fn general_rational_pcurve_orders_use_actual_lower_sums_and_captured_rows() {
    for sign in [-1.0, 1.0] {
        for width in [0.5, 1.0, 2.0] {
            let curve = elevated(sign, width);
            let policy = DecodePolicy::service(); let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
                for local in [0.0, 0.5, 1.0] {
                    let t = FiniteReal::new(local * width).unwrap();
                    let scratch = decode::Scratch::new(admission);
                    let poles = DifferentialPoles::Stored(curve.pole_rows());
                    let old = differential(&scratch, curve.degree(), curve.knots(), poles, t).unwrap();
                    for max_order in [2, 3, 4, 5] {
                        let actual = differential_requested(&scratch, curve.degree(), curve.knots(), poles, t, max_order).unwrap();
                        assert_eq!(actual.point, old.point); assert_eq!(actual.tangent, old.tangent);
                        assert_eq!(actual.acceleration, old.acceleration);
                        let w = 1.0 + local;
                        for (at, (value, expected)) in actual.higher.into_iter()
                            .zip([6.0 / w.powi(4), -24.0 / w.powi(5), 120.0 / w.powi(6)]).enumerate() {
                            if at + 3 > max_order { assert_eq!(value, Err(EvaluationFailure::NoValue)); }
                            else {
                                let expected = expected / width.powi(i32::try_from(at + 3).unwrap());
                                let value = value.unwrap().get();
                                assert!((value.u - expected).abs() <= EPS_RATIONAL_PCURVE);
                                assert!((value.v + 2.0 * expected).abs() <= EPS_RATIONAL_PCURVE);
                            }
                        }
                    }
                }
            }
            drop(ctx.reserve_scoped_limit(policy.limits.max_materialized_bytes.min(EMPTY_ROOT_MATERIALIZED_ALLOWANCE), "general rational scratch destroyed").unwrap());
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn rational_cubic_zero_current_basis_does_not_prove_zero_quotient_orders() {
    // Actual H=t³,W=1+t³. At0, C3=6,C4=C5=0, although
    // the old point/First/Second are zero. The last source pole matters.
    let points = [Point2::new(0.0, 0.0), Point2::new(0.0, 0.0),
        Point2::new(0.0, 0.0), Point2::new(0.5, -1.0)];
    let weights = [1.0, 1.0, 1.0, 2.0];
    let knots = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = decode::Scratch::new(admission);
        let poles = DifferentialPoles::Raw { points: &points, weights: Some(&weights) };
        let old = differential(&scratch, 3, &knots, poles, FiniteReal::ZERO).unwrap();
        assert_eq!(old.point.get(), Point2::new(0.0, 0.0));
        assert_eq!(old.tangent.unwrap().get(), Point2::new(0.0, 0.0));
        assert_eq!(old.acceleration.unwrap().get(), Point2::new(0.0, 0.0));
        let actual = differential_requested(&scratch, 3, &knots, poles, FiniteReal::ZERO, 5).unwrap();
        assert_eq!(actual.point, old.point); assert_eq!(actual.tangent, old.tangent);
        assert_eq!(actual.acceleration, old.acceleration);
        assert_eq!(actual.higher.map(|value| value.unwrap().get()),
            [Point2::new(6.0, -12.0), Point2::new(0.0, 0.0), Point2::new(0.0, 0.0)]);
        let constant = [Point2::new(f64::MAX, f64::from_bits(1)); 4];
        let actual = differential_requested(&scratch, 3, &knots,
            DifferentialPoles::Raw { points: &constant, weights: Some(&weights) }, FiniteReal::ZERO, 5).unwrap();
        assert_eq!(actual.point.get(), constant[0]);
        assert_eq!(actual.higher.map(|value| value.unwrap().get()), [Point2::new(0.0, 0.0); 3]);
    }
    ctx.finish_session().unwrap();
}

#[test]
fn rational_polar_higher_projects_actual_radial_and_axial_source_rows() {
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let points = [PolarNurbsPole { radial: Point2::new(0.0, 0.0), axial: 0.0 },
        PolarNurbsPole { radial: Point2::new(0.0, 0.0), axial: 0.0 },
        PolarNurbsPole { radial: Point2::new(0.0, 0.0), axial: 0.0 },
        PolarNurbsPole { radial: Point2::new(0.5, -1.0), axial: 1.5 }];
    let curve = PolarPcurveNurbs::from_lanes(&setup, 3,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], points.to_vec(),
        Some(vec![1.0, 1.0, 1.0, 2.0]), false).unwrap().unwrap();
    setup.finish_session().unwrap();
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        for (poles, third) in [(DifferentialPoles::PolarRadial(curve.pole_rows()), Point2::new(6.0, -12.0)),
            (DifferentialPoles::PolarAxial(curve.pole_rows()), Point2::new(18.0, 0.0))] {
            let scratch = decode::Scratch::new(admission);
            let old = differential(&scratch, curve.degree(), curve.knots(), poles, FiniteReal::ZERO).unwrap();
            let actual = differential_requested(&scratch, curve.degree(), curve.knots(), poles, FiniteReal::ZERO, 5).unwrap();
            assert_eq!(actual.point, old.point); assert_eq!(actual.tangent, old.tangent);
            assert_eq!(actual.acceleration, old.acceleration);
            assert_eq!(actual.higher.map(|value| value.unwrap().get()),
                [third, Point2::new(0.0, 0.0), Point2::new(0.0, 0.0)]);
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn general_rational_degree_two_keeps_original_local_caps_and_prior_refusal() {
    let policy = DecodePolicy::service(); let setup_arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(&[], &setup_arena, &policy).unwrap();
    let curve = PcurveNurbs::from_lanes(&setup, 2, vec![0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0), Point2::new(2.0, 0.0), Point2::new(3.0, 0.0)],
        Some(vec![1.0, 2.0, 1.0, 2.0]), false).unwrap().unwrap();
    setup.finish_session().unwrap();
    let mut policy = DecodePolicy::service(); policy.limits.max_collection_items = 11;
    policy.limits.max_materialized_bytes = 512; policy.limits.max_retained_bytes = 0;
    policy.limits.max_work_units = 256;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scratch = decode::Scratch::new(&ctx);
    let actual = differential_requested(&scratch, curve.degree(), curve.knots(),
        DifferentialPoles::Stored(curve.pole_rows()), FiniteReal::new(0.5).unwrap(), 5).unwrap();
    for (value, expected) in actual.higher.into_iter()
        .zip([185088.0 / 28561.0, -8472576.0 / 371293.0, 746926080.0 / 4826809.0]) {
        let value = value.unwrap().get(); assert!((value.u - expected).abs() <= EPS_RATIONAL_PCURVE);
        assert_eq!(value.v, 0.0);
    }
    drop(scratch); drop(ctx.reserve_scoped_limit(512, "actual degree2 requested scratch destroyed").unwrap());
    ctx.finish_session().unwrap();
    let mut policy = DecodePolicy::service(); policy.limits.max_work_units = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = ctx.charge_work_limit(1, "actual general pcurve prior refusal").unwrap_err();
    let scratch = decode::Scratch::new(&ctx);
    for max_order in [2, 3, 4, 5] {
        assert!(matches!(differential_requested(&scratch, curve.degree(), curve.knots(),
            DifferentialPoles::Stored(curve.pole_rows()), FiniteReal::new(0.5).unwrap(), max_order),
            Err(EvaluationFailure::ResourceLimit(limit)) if limit == original));
    }
    drop(scratch);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn general_rational_cubic_range_keeps_independent_requested_orders_and_old_lower() {
    let points = [Point2::new(0.0, 0.0), Point2::new(0.0, 0.0),
        Point2::new(0.0, 0.0), Point2::new(0.5, 0.0)];
    let weights = [1.0, 1.0, 1.0, 2.0];
    // q=(t/h)^3/(1+(t/h)^3). At0 q3=6/h³,q4=q5=0.
    // The least width gives true Third overflow; MAX gives true underflow.
    let cases = [(f64::from_bits(1), Err(EvaluationFailure::NonFinite(()))),
        (2.0_f64.powi(-200), Ok(FinitePoint2::new(Point2::new(6.0 * 2.0_f64.powi(600), 0.0)).unwrap())),
        (f64::MAX, Ok(FinitePoint2::from_coordinates(FiniteReal::ZERO, FiniteReal::ZERO)))];
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        for (width, third) in cases {
            let knots = [0.0, 0.0, 0.0, 0.0, width, width, width, width];
            let scratch = decode::Scratch::new(admission);
            let poles = DifferentialPoles::Raw { points: &points, weights: Some(&weights) };
            let old = differential(&scratch, 3, &knots, poles, FiniteReal::ZERO).unwrap();
            assert_eq!(old.point.get(), Point2::new(0.0, 0.0));
            assert_eq!(old.tangent.unwrap().get(), Point2::new(0.0, 0.0));
            assert_eq!(old.acceleration.unwrap().get(), Point2::new(0.0, 0.0));
            for max_order in [2, 3, 4, 5] {
                let actual = differential_requested(&scratch, 3, &knots, poles, FiniteReal::ZERO, max_order).unwrap();
                assert_eq!(actual.point, old.point); assert_eq!(actual.tangent, old.tangent);
                assert_eq!(actual.acceleration, old.acceleration);
                for (at, value) in actual.higher.into_iter().enumerate() {
                    assert_eq!(value, if at + 3 > max_order { Err(EvaluationFailure::NoValue) }
                        else if at == 0 { third } else {
                            Ok(FinitePoint2::from_coordinates(FiniteReal::ZERO, FiniteReal::ZERO))
                        });
                }
            }
        }
    }
    ctx.finish_session().unwrap();
}
