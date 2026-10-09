// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::admission::EvaluationAdmission;
use cadmpeg_core::decode::WorkBudget;

fn endpoint_curve(degree: u32, width: f64, endpoint: f64, common: f64) -> SolvedCurveGeometry {
    let support = usize::try_from(degree + 1).unwrap();
    let mut knots = vec![0.0; support];
    knots.extend(std::iter::repeat_n(width, support));
    let mut points = vec![Point3::new(0.0, 0.0, 0.0); support];
    points[support - 1].x = endpoint;
    let mut weights = vec![common; support];
    weights[support - 1] = 2.0 * common;
    SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), degree, knots, points,
        Some(weights), false).unwrap().unwrap())
}

fn third(admission: EvaluationAdmission<'_, '_>, geometry: &SolvedCurveGeometry, t: f64)
    -> Result<FiniteVector3, EvaluationFailure<()>>
{
    crate::eval::curve_higher::stored_third(&decode::Scratch::new(admission), geometry,
        FiniteReal::new(t).unwrap(), Err(EvaluationFailure::NoValue))
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() <= 64.0 * f64::EPSILON * expected.abs(),
        "{actual:?} versus {expected:?}");
}

#[test]
fn general_cubic_rational_third_follows_the_analytic_law_without_variable_work() {
    for common in [-1.0, 1.0].into_iter().flat_map(|sign|
        [-900, 0, 900].map(|exponent| sign * 2.0_f64.powi(exponent)))
    {
        let geometry = endpoint_curve(3, 1.0, 1.0, common);
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        // C=2s^3/(1+s^3); its Third is
        // 12*(1-16s^3+10s^6)/(1+s^3)^4.
        for (t, expected) in [(0.0, 12.0), (0.5, -512.0 / 81.0), (1.0, -15.0 / 4.0)] {
            for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
                let actual = third(admission, &geometry, t).unwrap().get();
                close(actual.x, expected);
                assert_eq!((actual.y, actual.z), (0.0, 0.0));
            }
        }
        let budget = WorkBudget::new(0);
        let actual = EvaluationAdmission::Standard.within_work_slice(&budget, |admission|
            third(admission, &geometry, 0.5)).unwrap();
        close(actual.x, -512.0 / 81.0);
        assert_eq!(budget.consumed(), 0);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn general_cubic_third_uses_final_span_scaling_and_reports_actual_coefficient_loss() {
    let width = 2.0_f64.powi(-350);
    let tiny = endpoint_curve(3, width, 2.0_f64.powi(-1000), 1.0);
    close(third(EvaluationAdmission::Standard, &tiny, width / 2.0).unwrap().x,
        (-512.0 / 81.0) * 2.0_f64.powi(50));
    let amplified = endpoint_curve(3, width, f64::from_bits(1), 1.0);
    close(third(EvaluationAdmission::Standard, &amplified, width / 2.0).unwrap().x,
        (-512.0 / 81.0) * 2.0_f64.powi(-24));
    let overflow = endpoint_curve(3, f64::from_bits(1), f64::from_bits(1), 1.0);
    assert_eq!(third(EvaluationAdmission::Standard, &overflow, 0.0),
        Err(EvaluationFailure::NonFinite(())));
    let lost = endpoint_curve(3, 1.0, 2.0_f64.powi(900), 1.0);
    // The finite-row representation loses nonzero s^2 here; it cannot
    // certify the resulting pole-amplified Third from a rounded zero.
    assert_eq!(third(EvaluationAdmission::Standard, &lost, 2.0_f64.powi(-600)),
        Err(EvaluationFailure::NoValue));
    assert!(crate::eval::decode::curve_point_solved(EvaluationAdmission::Standard,
        &lost, 2.0_f64.powi(-600)).is_ok());
}

#[test]
fn general_rational_third_selects_real_multispan_support_and_keeps_zero_denominator_absent() {
    let geometry = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 2,
        vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        Some(vec![1.0, 1.0, 1.0, 2.0]), false).unwrap().unwrap());
    assert_eq!(third(EvaluationAdmission::Standard, &geometry, 0.25), Ok(FiniteVector3::ZERO));
    // On the second span C=2s^2/(1+s^2), s=2(t-.5).
    close(third(EvaluationAdmission::Standard, &geometry, 0.75).unwrap().x, -36864.0 / 625.0);
    let zero = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 3,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0); 4], Some(vec![1.0, -1.0, 1.0, -1.0]),
        false).unwrap().unwrap());
    // Constant source coordinates do not override undefined W(.5)=0.
    assert_eq!(third(EvaluationAdmission::Standard, &zero, 0.5), Err(EvaluationFailure::NoValue));
    let budget = WorkBudget::new(0);
    assert_eq!(EvaluationAdmission::Standard.within_work_slice(&budget, |admission|
        third(admission, &geometry, 0.75)), Err(EvaluationFailure::NoValue));
    assert_eq!(budget.consumed(), 0);
}

#[test]
fn general_degree_four_third_has_exact_backing_and_actual_visit_boundaries() {
    let geometry = endpoint_curve(4, 1.0, 1.0, 1.0);
    let bytes = u64::try_from(10 * std::mem::size_of::<[f64; 4]>()).unwrap();
    // Two five-slot initializations10 + four rows4 + cells2+3+4+5=14
    // + five actual source-pole advances5 =33. Fresh exact capacities move0.
    // Direct differentiation of C=2s^4/(1+s^4) at s=.5 gives
    // 8s*(6-60s^4+30s^8)/(1+s^4)^4 =620544/83521.
    close(third(EvaluationAdmission::Standard, &geometry, 0.5).unwrap().x,
        620544.0 / 83521.0);
    for trigger in 0..5 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = bytes - u64::from(trigger == 0);
        policy.limits.max_collection_items = 10 - u64::from(trigger == 1);
        policy.limits.max_work_units = 33 - u64::from(trigger == 2);
        policy.limits.max_retained_bytes = 0;
        if trigger == 3 { policy.limits.max_recursion_depth = 0; }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = third(EvaluationAdmission::Decode(&ctx), &geometry, 0.0);
        if trigger < 4 {
            let Err(EvaluationFailure::ResourceLimit(original)) = result else {
                panic!("one-below real requested storage/work/depth must refuse");
            };
            assert_eq!(original.dimension, match trigger {
                0 => ResourceDimension::MaterializedBytes,
                1 => ResourceDimension::CollectionItems,
                2 => ResourceDimension::WorkUnits,
                _ => ResourceDimension::RecursionDepth,
            });
            assert_eq!(original.operation, match trigger {
                0 | 1 => "IR requested curve basis storage",
                2 => "IR requested curve homogeneous support",
                _ => "geometry evaluation nesting",
            });
            assert_eq!((original.limit, original.used, original.additional), match trigger {
                0 => (bytes - 1, bytes / 2, bytes / 2),
                1 => (9, 5, 5),
                2 => (32, 32, 1),
                _ => (0, 0, 1),
            });
            assert_eq!(third(EvaluationAdmission::Decode(&ctx), &geometry, -1.0),
                Err(EvaluationFailure::ResourceLimit(original)));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
        } else {
            // C=2s^4/(1+s^4) has Third0 at s0.
            assert_eq!(result, Ok(FiniteVector3::ZERO));
            drop(ctx.reserve_scoped_limit(bytes, "requested basis backing released").unwrap());
            ctx.finish_session().unwrap();
        }
    }
    for cap in [32, 33] {
        let budget = WorkBudget::new(cap);
        let result = EvaluationAdmission::Standard.within_work_slice(&budget, |admission|
            third(admission, &geometry, 0.0));
        if cap == 32 { assert_eq!(result, Err(EvaluationFailure::NoValue)); }
        else { assert_eq!(result, Ok(FiniteVector3::ZERO)); }
        assert_eq!(budget.consumed(), cap);
    }
}
