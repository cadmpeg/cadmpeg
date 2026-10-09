// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::admission::EvaluationAdmission;
use crate::geometry::PlacedCurve;
use crate::transform::Transform;
use cadmpeg_core::decode::WorkBudget;

fn curve(interval: [f64; 2], endpoint: f64, weights: [f64; 3], periodic: bool) -> SolvedCurveGeometry {
    SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 2,
        vec![interval[0], interval[0], interval[0], interval[1], interval[1], interval[1]],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0), Point3::new(endpoint, 0.0, 0.0)],
        Some(weights.to_vec()), periodic).unwrap().unwrap())
}

fn third(scratch: &decode::Scratch<'_, '_>, geometry: &SolvedCurveGeometry, t: f64)
    -> Result<FiniteVector3, EvaluationFailure<()>>
{
    crate::eval::curve_higher::stored_third(scratch, geometry, FiniteReal::new(t).unwrap(),
        Err(EvaluationFailure::NoValue))
}

#[test]
fn quadratic_rational_third_has_the_true_law_without_heap_or_variable_work() {
    for exponent in [-900, 0, 900] {
        for sign in [-1.0, 1.0] {
            let weight = sign * 2.0_f64.powi(exponent);
            let geometry = curve([0.0, 1.0], 1.0, [weight, weight, 2.0 * weight], false);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            for admission in [EvaluationAdmission::Decode(&ctx), EvaluationAdmission::Standard] {
                let scratch = decode::Scratch::new(admission);
                // C=2t^2/(1+t^2); differentiating three times gives
                // -48t(1-t^2)/(1+t^2)^4, including endpoint zeros.
                for t in [0.0_f64, 0.25, 0.5, 1.0] {
                    let expected = -48.0 * t * (1.0 - t * t) / (1.0 + t * t).powi(4);
                    let actual = third(&scratch, &geometry, t).unwrap().get();
                    assert!((actual.x - expected).abs() <= 32.0 * f64::EPSILON * expected.abs());
                    assert_eq!((actual.y, actual.z), (0.0, 0.0));
                }
            }
            let budget = WorkBudget::new(0);
            let actual = EvaluationAdmission::Standard.within_work_slice(&budget, |admission| {
                third(&decode::Scratch::new(admission), &geometry, 0.5)
            }).unwrap();
            assert!((actual.x + 4608.0 / 625.0).abs() <= 32.0 * f64::EPSILON * actual.x.abs());
            assert_eq!(budget.consumed(), 0);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn quadratic_third_retains_a_squared_coefficient_before_large_pole_amplification() {
    let parameter = 2.0_f64.powi(-600);
    let endpoint = 2.0_f64.powi(300);
    assert_eq!(parameter * parameter, 0.0);
    let geometry = curve([0.0, 1.0], endpoint, [1.0, 1.0, 2.0], false);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    // -48q*s*(1-s²)/(1+s²)^4 differs from -48*2^-300
    // below binary64 resolution; all normalized geometric orders are normal.
    let expected = -48.0 * 2.0_f64.powi(-300);
    for admission in [EvaluationAdmission::Decode(&ctx), EvaluationAdmission::Standard] {
        let actual = third(&decode::Scratch::new(admission), &geometry, parameter).unwrap().x;
        assert!((actual - expected).abs() <= 32.0 * f64::EPSILON * expected.abs());
    }
    let budget = WorkBudget::new(0);
    let actual = EvaluationAdmission::Standard.within_work_slice(&budget, |admission| {
        third(&decode::Scratch::new(admission), &geometry, parameter)
    }).unwrap().x;
    assert!((actual - expected).abs() <= 32.0 * f64::EPSILON * expected.abs());
    assert_eq!(budget.consumed(), 0);
    ctx.finish_session().unwrap();
}

#[test]
fn quadratic_third_keeps_scaled_spans_and_separate_intermediate_limits() {
    let h = 2.0_f64.powi(-350);
    let tiny = curve([0.0, h], 2.0_f64.powi(-1000), [1.0, 1.0, 2.0], false);
    let overflow_h = 2.0_f64.powi(-1000);
    let overflow = curve([0.0, overflow_h], overflow_h, [1.0, 1.0, 2.0], false);
    let wide = curve([-f64::MAX, f64::MAX], 1.0, [1.0, 1.0, 2.0], false);
    let intermediate = curve([0.0, 2.0], f64::MAX, [1.0, 1.0, 2.0], false);
    let scratch = decode::Scratch::new(EvaluationAdmission::Standard);
    // q/h^3=2^50, though h^3 and the unscaled derivative basis do not fit.
    let expected = -4608.0 / 625.0 * 2.0_f64.powi(50);
    let actual = third(&scratch, &tiny, h / 2.0).unwrap().x;
    assert!((actual - expected).abs() <= 32.0 * f64::EPSILON * expected.abs());
    assert_eq!(third(&scratch, &overflow, overflow_h / 2.0), Err(EvaluationFailure::NonFinite(())));
    assert_eq!(third(&scratch, &wide, 0.0), Ok(FiniteVector3::ZERO));
    // Normalized C'(.5)=1.28*MAX cannot be retained by this route.
    // The true parameter Third is -.9216*MAX, finite after division by2^3.
    // An intermediate failure is therefore missing Third, not final overflow.
    assert_eq!(third(&scratch, &intermediate, 1.0), Err(EvaluationFailure::NoValue));
    assert!(crate::eval::decode::curve_point_solved(EvaluationAdmission::Standard, &intermediate, 1.0).is_ok());
    let lost = curve([0.0, 1.0], 1.0, [1.0, 1.0, 2.0], false);
    assert_eq!(third(&scratch, &lost, f64::from_bits(1)), Err(EvaluationFailure::NoValue));
    assert!(crate::eval::decode::curve_point_solved(EvaluationAdmission::Standard, &lost, f64::from_bits(1)).is_ok());
    // The true Third is (-4608/625)*2^-24, finite, but normalized
    // p0=.4*2^-1074 rounds to zero before the span division amplifies it.
    let amplified = curve([0.0, h], f64::from_bits(1), [1.0, 1.0, 2.0], false);
    assert_eq!(third(&scratch, &amplified, h / 2.0), Err(EvaluationFailure::NoValue));
    assert!(crate::eval::decode::curve_point_solved(EvaluationAdmission::Standard, &amplified, h / 2.0).is_ok());
    // Equal actual weights establish a polynomial quadratic, even where
    // first/second intermediates or the knot width cube cannot fit.
    let polynomial = curve([0.0, f64::from_bits(1)], f64::MAX, [2.0; 3], false);
    assert_eq!(third(&scratch, &polynomial, 0.0), Ok(FiniteVector3::ZERO));
    let zero_weight = curve([0.0, 1.0], 1.0, [1.0, -1.0, 1.0], false);
    assert_eq!(third(&scratch, &zero_weight, 0.5), Err(EvaluationFailure::NoValue));
}

#[test]
fn quadratic_third_keeps_actual_periodic_mapping_and_nonperiodic_domain() {
    let scratch = decode::Scratch::new(EvaluationAdmission::Standard);
    let periodic = curve([0.0, 1.0], 1.0, [1.0, 1.0, 2.0], true);
    let expected = third(&scratch, &periodic, 0.5).unwrap();
    for t in [-0.5, 1.5, 100.5] { assert_eq!(third(&scratch, &periodic, t), Ok(expected)); }
    assert_eq!(third(&scratch, &periodic, 1.0), Ok(FiniteVector3::ZERO));
    let ordinary = curve([0.0, 1.0], 1.0, [1.0, 1.0, 2.0], false);
    for t in [-0.5, 1.5] { assert_eq!(third(&scratch, &ordinary, t), Err(EvaluationFailure::NoValue)); }
}

#[test]
fn quadratic_third_preserves_original_fuse_and_real_placement_steps() {
    let geometry = curve([0.0, 1.0], 1.0, [1.0, 1.0, 2.0], false);
    let place = |basis| SolvedCurveGeometry::Transformed(PlacedCurve::try_new(Box::new(basis),
        Transform::affine([[2.0, 0.0, 0.0, 5.0], [0.0, 3.0, 0.0, 7.0], [0.0, 0.0, 4.0, 11.0]]).unwrap()).unwrap());
    let placed = place(place(geometry.clone()));
    for cap in [0, 1, 2] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = decode::Scratch::new(&ctx);
        let actual = third(&scratch, &placed, 0.5);
        let refused = if cap < 2 {
            let Err(EvaluationFailure::ResourceLimit(original)) = actual else { panic!("actual placement step must refuse"); };
            assert_eq!(original.operation, "IR curve higher source traversal");
            assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
            assert_eq!(third(&scratch, &geometry, -0.5), Err(EvaluationFailure::ResourceLimit(original)));
            Some(original)
        } else {
            let actual = actual.unwrap().x;
            let expected = -4.0 * 4608.0 / 625.0;
            assert!((actual - expected).abs() <= 32.0 * f64::EPSILON * expected.abs());
            None
        };
        drop(scratch);
        match refused {
            Some(original) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)),
            None => { ctx.finish_session().unwrap(); }
        }
    }
}

#[test]
fn unsupported_rational_shapes_keep_true_lower_orders() {
    let geometries = [
        NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0); 4], Some(vec![1.0, 1.0, 1.0, 2.0]), false).unwrap().unwrap(),
        NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 2,
            vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0); 4], Some(vec![1.0, 1.0, 1.0, 2.0]), false).unwrap().unwrap(),
    ];
    let scratch = decode::Scratch::new(EvaluationAdmission::Standard);
    for curve in geometries {
        let geometry = SolvedCurveGeometry::Nurbs(curve);
        assert_eq!(third(&scratch, &geometry, 0.25), Err(EvaluationFailure::NoValue));
        assert!(crate::eval::decode::curve_point_solved(EvaluationAdmission::Standard, &geometry, 0.25).is_ok());
        assert!(crate::eval::curve_tangent_solved(EvaluationAdmission::Standard, &geometry, 0.25).is_ok());
        assert!(crate::eval::curve_second_derivative_solved(EvaluationAdmission::Standard, &geometry, 0.25).is_ok());
    }
}
