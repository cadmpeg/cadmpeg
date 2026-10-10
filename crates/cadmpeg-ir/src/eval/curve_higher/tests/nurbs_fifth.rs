// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::ModelCurveRequest;
use crate::geometry::nurbs::NurbsCurve;
use cadmpeg_core::decode::WorkBudget;

const EPS_FIFTH_LAW: f64 = 128.0 * f64::EPSILON;

fn curve(degree: u32, width: f64, endpoint: f64, weights: Option<f64>) -> SolvedCurveGeometry {
    let support = usize::try_from(degree + 1).unwrap();
    let mut knots = vec![0.0; support]; knots.extend(std::iter::repeat_n(width, support));
    let mut points = vec![Point3::new(0.0, 0.0, 0.0); support]; points[support - 1].x = endpoint;
    let weights = weights.map(|common| {
        let mut weights = vec![common; support]; weights[support - 1] = 2.0 * common; weights
    });
    SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(),
        degree, knots, points, weights, false).unwrap().unwrap())
}

fn higher(admission: EvaluationAdmission<'_, '_>, geometry: &SolvedCurveGeometry, t: f64, request: ModelCurveRequest)
    -> Result<CurveHigher, EvaluationFailure<()>>
{
    stored_higher(&Scratch::new(admission), geometry, FiniteReal::new(t).unwrap(),
        Err(EvaluationFailure::NoValue), Err(EvaluationFailure::NoValue), request)
}

fn falling(degree: u32, order: u32) -> f64 {
    if degree < order { 0.0 } else { f64::from((degree - order + 1..=degree).product::<u32>()) }
}

// W*C=H differentiated n times; this control does not use a quotient table.
fn rational_fifth(degree: u32, t: f64) -> f64 {
    let source = |order| if degree < order { 0.0 }
        else { falling(degree, order) * t.powi(i32::try_from(degree - order).unwrap()) };
    let mut derivatives = [0.0; 6];
    for n in 0..=5 {
        let mut correction = 0.0;
        let mut binomial = 1_u32;
        for k in 1..=n {
            binomial = binomial * (n + 1 - k) / k;
            correction += f64::from(binomial) * source(k) * derivatives[usize::try_from(n - k).unwrap()];
        }
        derivatives[usize::try_from(n).unwrap()] = (2.0 * source(n) - correction) / (1.0 + source(0));
    }
    derivatives[5]
}

#[test]
fn rational_fifth_uses_true_linear_quadratic_and_general_laws_without_changing_lower_orders() {
    for degree in 1..=8 {
        for common in [1.0, -2.0_f64.powi(-900), 2.0_f64.powi(900)] {
            let geometry = curve(degree, 1.0, 1.0, Some(common));
            let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
            let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
                for t in [0.0, 0.25, 0.5, 1.0] {
                    let actual = higher(admission, &geometry, t, ModelCurveRequest::Fifth).unwrap();
                    let expected = rational_fifth(degree, t);
                    assert!((actual.fifth.unwrap().x - expected).abs() <= EPS_FIFTH_LAW * expected.abs().max(1.0));
                    let lower = higher(admission, &geometry, t, ModelCurveRequest::Fourth).unwrap();
                    assert_eq!(actual.third, lower.third); assert_eq!(actual.fourth, lower.fourth);
                    assert_eq!(lower.fifth, Err(EvaluationFailure::NoValue));
                }
            }
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn polynomial_fifth_shares_the_real_cox_triangle_and_preserves_original_lower_arithmetic() {
    for degree in 1..=8 {
        let geometry = curve(degree, 1.0, 1.0, None);
        let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            for t in [0.0_f64, 0.125, 0.5, 1.0] {
                let actual = higher(admission, &geometry, t, ModelCurveRequest::Fifth).unwrap();
                let expected = if degree < 5 { 0.0 } else { falling(degree, 5) * t.powi(i32::try_from(degree - 5).unwrap()) };
                assert!((actual.fifth.unwrap().x - expected).abs() <= EPS_FIFTH_LAW * expected.abs().max(1.0));
                let lower = higher(admission, &geometry, t, ModelCurveRequest::Fourth).unwrap();
                assert_eq!(actual.third, lower.third); assert_eq!(actual.fourth, lower.fourth);
            }
        }
        ctx.finish_session().unwrap();
    }
    let tiny = curve(5, 2.0_f64.powi(-210), 2.0_f64.powi(-1000), None);
    let actual = higher(EvaluationAdmission::Standard, &tiny, 0.0, ModelCurveRequest::Fifth).unwrap();
    assert_eq!(actual.third, Ok(FiniteVector3::ZERO)); assert_eq!(actual.fourth, Ok(FiniteVector3::ZERO));
    assert_eq!(actual.fifth.unwrap().x, 120.0 * 2.0_f64.powi(50));
    let overflowing = curve(5, 1.0, f64::MAX, None);
    let actual = higher(EvaluationAdmission::Standard, &overflowing, 0.0, ModelCurveRequest::Fifth).unwrap();
    assert_eq!(actual.third, Ok(FiniteVector3::ZERO)); assert_eq!(actual.fourth, Ok(FiniteVector3::ZERO));
    assert_eq!(actual.fifth, Err(EvaluationFailure::NonFinite(())));
    let subnormal = curve(5, 2.0, f64::from_bits(1), None);
    assert_eq!(higher(EvaluationAdmission::Standard, &subnormal, 0.0, ModelCurveRequest::Fifth).unwrap().fifth.unwrap().x, f64::from_bits(4));
}

#[test]
fn fixed_linear_fifth_preserves_extended_span_and_independent_final_overflow() {
    let tiny = curve(1, 2.0_f64.powi(-210), 2.0_f64.powi(-1000), Some(1.0));
    let actual = higher(EvaluationAdmission::Standard, &tiny, 0.0, ModelCurveRequest::Fifth).unwrap();
    assert_eq!(actual.fifth.unwrap().x, 240.0 * 2.0_f64.powi(50));
    let overflowing = curve(1, 2.0_f64.powi(-420), 2.0_f64.powi(-1000), Some(1.0));
    let actual = higher(EvaluationAdmission::Standard, &overflowing, 0.0, ModelCurveRequest::Fifth).unwrap();
    assert!(actual.third.is_ok()); assert!(actual.fourth.is_ok());
    assert_eq!(actual.fifth, Err(EvaluationFailure::NonFinite(())));
    let geometry = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 1,
        vec![0.0, 0.0, f64::from_bits(1), f64::from_bits(1)],
        vec![Point3::new(f64::MAX, -0.0, f64::from_bits(1)); 2], Some(vec![1.0, 2.0]), false).unwrap().unwrap());
    assert_eq!(higher(EvaluationAdmission::Standard, &geometry, 0.0, ModelCurveRequest::Fifth).unwrap().fifth, Ok(FiniteVector3::ZERO));
}

#[test]
fn rational_fifth_does_not_replace_missing_required_fourth_coefficients_with_zero() {
    let h = 2.0_f64.powi(-400);
    let knots = vec![0.0, 0.0, 0.0, 0.0, 0.0, h, 1.0, 2.0, 3.0, 4.0, 4.0, 4.0, 4.0, 4.0];
    for constant in [false, true] {
        let mut points = vec![Point3::new(0.0, 0.0, 0.0); 9]; if !constant { points[4].x = 1.0; }
        let mut weights = vec![1.0; 9]; weights[4] = 2.0;
        let geometry = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(),
            4, knots.clone(), points, Some(weights), false).unwrap().unwrap());
        let actual = higher(EvaluationAdmission::Standard, &geometry, 0.0, ModelCurveRequest::Fifth).unwrap();
        let lower = higher(EvaluationAdmission::Standard, &geometry, 0.0, ModelCurveRequest::Fourth).unwrap();
        assert_eq!(actual.third, lower.third); assert_eq!(actual.fourth, lower.fourth);
        assert_eq!(actual.fifth, if constant { Ok(FiniteVector3::ZERO) } else { Err(EvaluationFailure::NoValue) });
    }
}

#[test]
fn rational_fifth_admits_real_six_lane_backing_support_and_original_refusal() {
    let geometry = curve(5, 1.0, 1.0, Some(1.0));
    // Two support6 buffers: initialization12 + degree rows5 + cells20 + poles6 =43.
    let bytes = u64::try_from(12 * std::mem::size_of::<[f64; 6]>()).unwrap();
    for trigger in 0..5 {
        let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
        policy.limits.max_work_units = 43 - u64::from(trigger == 0);
        policy.limits.max_materialized_bytes = bytes - u64::from(trigger == 1);
        policy.limits.max_collection_items = 12 - u64::from(trigger == 2);
        if trigger == 3 { policy.limits.max_recursion_depth = 0; }
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let actual = higher(EvaluationAdmission::Decode(&ctx), &geometry, 0.0, ModelCurveRequest::Fifth);
        if trigger < 4 {
            let Err(EvaluationFailure::ResourceLimit(original)) = actual else { panic!("next real operation refuses") };
            let (dimension, operation, limit, used, additional) = match trigger {
                0 => (ResourceDimension::WorkUnits, "IR requested curve homogeneous support", 42, 42, 1),
                1 => (ResourceDimension::MaterializedBytes, "IR requested curve basis storage", bytes - 1, bytes / 2, bytes / 2),
                2 => (ResourceDimension::CollectionItems, "IR requested curve basis storage", 11, 6, 6),
                _ => (ResourceDimension::RecursionDepth, "geometry evaluation nesting", 0, 0, 1),
            };
            assert_eq!((original.dimension, original.operation, original.limit, original.used, original.additional), (dimension, operation, limit, used, additional));
            assert!(matches!(higher(EvaluationAdmission::Decode(&ctx), &geometry, -1.0, ModelCurveRequest::Fifth), Err(EvaluationFailure::ResourceLimit(sticky)) if sticky == original));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
        } else {
            assert_eq!(actual.unwrap().fifth.unwrap().x, 240.0);
            drop(ctx.reserve_scoped_limit(bytes, "six lane backing destroyed").unwrap()); ctx.finish_session().unwrap();
        }
    }
    for cap in [42, 43] {
        let work = WorkBudget::new(cap);
        let actual = EvaluationAdmission::Standard.within_work_slice(&work, |admission| higher(admission, &geometry, 0.0, ModelCurveRequest::Fifth));
        if cap == 42 { assert!(matches!(actual, Err(EvaluationFailure::NoValue))); }
        else { assert_eq!(actual.unwrap().fifth.unwrap().x, 240.0); }
        assert_eq!(work.consumed(), cap);
    }
}

#[test]
fn polynomial_fifth_admits_actual_preceding_rows_recurrence_and_one_support_walk() {
    let geometry = curve(5, 1.0, 1.0, None);
    // Cox9 + Third(4+5+6)15 + Fourth(3+4+5+6)18 + Fifth(2+3+4+5+6)20 + support6 =68.
    // Decode keeps original two 4->8 Third relocations:64 additional byte visits.
    // Actual capacities: base4 + Third(4+8+8)20 + Fourth19 + Fifth23 =66 f64s.
    let bytes = 66 * u64::try_from(std::mem::size_of::<f64>()).unwrap();
    for (work, storage, items, dimension, operation) in [
        (131, bytes, 56, ResourceDimension::WorkUnits, "IR polynomial curve third support"),
        (132, bytes - 1, 56, ResourceDimension::MaterializedBytes, "IR scaled polynomial fifth basis"),
        (132, bytes, 55, ResourceDimension::CollectionItems, "IR scaled polynomial fifth basis"),
        (132, bytes, 56, ResourceDimension::WorkUnits, "success"),
    ] {
        let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
        policy.limits.max_work_units = work; policy.limits.max_materialized_bytes = storage; policy.limits.max_collection_items = items;
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let actual = higher(EvaluationAdmission::Decode(&ctx), &geometry, 0.0, ModelCurveRequest::Fifth);
        if operation == "success" {
            assert_eq!(actual.unwrap().fifth.unwrap().x, 120.0); ctx.finish_session().unwrap();
        } else {
            let Err(EvaluationFailure::ResourceLimit(original)) = actual else { panic!("real next owner refuses") };
            assert_eq!((original.dimension, original.operation), (dimension, operation));
            assert!(matches!(higher(EvaluationAdmission::Decode(&ctx), &geometry, -1.0, ModelCurveRequest::Fifth), Err(EvaluationFailure::ResourceLimit(sticky)) if sticky == original));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
        }
    }
    for cap in [67, 68] {
        let work = WorkBudget::new(cap);
        let actual = EvaluationAdmission::Standard.within_work_slice(&work, |admission|
            higher(admission, &geometry, 0.0, ModelCurveRequest::Fifth)).unwrap();
        assert_eq!(actual.fifth, if cap == 68 { Ok(FiniteVector3::new(Vector3::new(120.0, 0.0, 0.0)).unwrap()) } else { Err(EvaluationFailure::NoValue) });
        assert_eq!(work.consumed(), cap);
    }
}
