// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::geometry::nurbs::NurbsCurve;
use crate::eval::ModelCurveRequest;
use cadmpeg_core::decode::WorkBudget;

fn linear(knots: [f64; 2], endpoint: f64, weights: Option<[f64; 2]>) -> SolvedCurveGeometry {
    // Fixture construction is outside each original evaluation session.
    SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 1,
        vec![knots[0], knots[0], knots[1], knots[1]],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(endpoint, 0.0, 0.0)],
        weights.map(Vec::from), false).unwrap().unwrap())
}

fn higher(scratch: &Scratch<'_, '_>, geometry: &SolvedCurveGeometry, parameter: f64)
    -> Result<CurveHigher, EvaluationFailure<()>>
{
    stored_higher(scratch, geometry, FiniteReal::new(parameter).unwrap(),
        Err(EvaluationFailure::NoValue), Err(EvaluationFailure::NoValue), ModelCurveRequest::Fourth)
}

#[test]
fn rational_linear_fourth_obeys_the_true_quotient_and_common_weight_scale() {
    for exponent in [-900, 0, 900] {
        let scale = 2.0_f64.powi(exponent);
        for reversed_weights in [false, true] {
            let weights = if reversed_weights { [2.0 * scale, scale] } else { [scale, 2.0 * scale] };
            let geometry = linear([0.0, 1.0], 1.0, Some(weights));
            for parameter in [0.0, 0.5, 1.0] {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = 0;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_collection_items = 0;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                for admission in [EvaluationAdmission::Decode(&ctx), EvaluationAdmission::Standard] {
                    let scratch = Scratch::new(admission);
                    let result = higher(&scratch, &geometry, parameter).unwrap();
                    // C=2t/(1+t), or C=t/(2-t) for reversed weights.
                    let denominator = if reversed_weights { 2.0 - parameter } else { 1.0 + parameter };
                    let expected = if reversed_weights { 48.0 } else { -48.0 } / denominator.powi(5);
                    let fourth = result.fourth.unwrap().get();
                    assert!((fourth.x - expected).abs() <= 32.0 * f64::EPSILON * expected.abs());
                    assert_eq!((fourth.y, fourth.z), (0.0, 0.0));
                    let third = stored_third(&scratch, &geometry, FiniteReal::new(parameter).unwrap(), Err(EvaluationFailure::NoValue));
                    assert_eq!(result.third, third);
                }
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[test]
fn rational_linear_fourth_retains_extended_span_zero_and_separate_final_range() {
    let tiny = linear([0.0, 2.0_f64.powi(-350)], 2.0_f64.powi(-1000), Some([1.0, 2.0]));
    let fourth_overflow = linear([0.0, 2.0_f64.powi(-600)], 2.0_f64.powi(-1000), Some([1.0, 2.0]));
    let subnormal = linear([0.0, 2.0], f64::from_bits(1), Some([1.0, 2.0]));
    let wide = linear([-f64::MAX, f64::MAX], 1.0, Some([1.0, 2.0]));
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let result = higher(&scratch, &tiny, 0.0).unwrap();
    assert_eq!(result.third.unwrap().get(), Vector3::new(12.0 * 2.0_f64.powi(50), 0.0, 0.0));
    assert_eq!(result.fourth.unwrap().get(), Vector3::new(-48.0 * 2.0_f64.powi(400), 0.0, 0.0));
    let result = higher(&scratch, &fourth_overflow, 0.0).unwrap();
    assert_eq!(result.third.unwrap().get(), Vector3::new(12.0 * 2.0_f64.powi(800), 0.0, 0.0));
    assert_eq!(result.fourth, Err(EvaluationFailure::NonFinite(())));
    assert_eq!(higher(&scratch, &subnormal, 0.0).unwrap().fourth.unwrap().get(), Vector3::new(-f64::from_bits(3), 0.0, 0.0));
    assert_eq!(higher(&scratch, &wide, 0.0).unwrap().fourth, Ok(FiniteVector3::ZERO));
    for weights in [None, Some([1.0, 1.0])] {
        let zero = linear([0.0, f64::from_bits(1)], f64::MAX, weights);
        let result = higher(&scratch, &zero, 0.0).unwrap();
        assert_eq!(result.third, Ok(FiniteVector3::ZERO));
        assert_eq!(result.fourth, Ok(FiniteVector3::ZERO));
    }
}

#[test]
fn linear_fourth_uses_one_real_span_prefix_and_retains_original_sticky_refusal() {
    let count = 1024_u32;
    let end = f64::from(count - 1);
    for rational in [false, true] {
        let geometry = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(), 1,
            [0.0, 0.0].into_iter().chain((1..count - 1).map(f64::from)).chain([end, end]).collect::<Vec<_>>(),
            (0..count).map(|i| Point3::new(f64::from(i), 0.0, 0.0)).collect::<Vec<_>>(),
            rational.then(|| (0..count).map(|i| if i % 2 == 0 { 1.0 } else { 2.0 }).collect()), false).unwrap().unwrap());
        // Binary search mids512,256,128,64,32,16,8,4,2,1 select [0,1].
        for cap in [0, 1, 2, 9, 10] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let scratch = Scratch::new(&ctx);
            let result = higher(&scratch, &geometry, 0.25);
            let original = if cap < 10 {
                let Err(EvaluationFailure::ResourceLimit(original)) = result else { panic!("next actual span comparison must refuse") };
                assert_eq!(original.operation, "IR B-spline span search");
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
                assert!(matches!(higher(&scratch, &geometry, 0.25), Err(EvaluationFailure::ResourceLimit(sticky)) if sticky == original));
                Some(original)
            } else {
                let result = result.unwrap();
                let expected = if rational { -48.0 / 1.25_f64.powi(5) } else { 0.0 };
                let actual = result.fourth.unwrap().x;
                assert!((actual - expected).abs() <= 32.0 * f64::EPSILON * expected.abs());
                None
            };
            drop(scratch);
            if let Some(original) = original { assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)); }
            else { ctx.finish_session().unwrap(); }
        }
    }
}

#[test]
fn rational_linear_fourth_maps_periodic_domain_and_each_stored_placement_once() {
    let periodic = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 1, vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)], Some(vec![1.0, 2.0]), true).unwrap().unwrap());
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let expected = higher(&scratch, &periodic, 0.5).unwrap().fourth;
    for parameter in [-0.5, 1.5, 100.5] { assert_eq!(higher(&scratch, &periodic, parameter).unwrap().fourth, expected); }
    assert_eq!(higher(&scratch, &periodic, 1.0).unwrap().fourth.unwrap().get(), Vector3::new(-48.0, 0.0, 0.0));
    let source = linear([0.0, 1.0], 1.0, Some([1.0, 2.0]));
    for parameter in [-0.5, 1.5] { assert!(matches!(higher(&scratch, &source, parameter), Err(EvaluationFailure::NoValue))); }
    let place = |basis, scale| SolvedCurveGeometry::Transformed(PlacedCurve::try_new(Box::new(basis),
        Transform::affine([[scale, 0.0, 0.0, 5.0], [0.0, 3.0, 0.0, 7.0], [0.0, 0.0, 4.0, 11.0]]).unwrap()).unwrap());
    let placed = place(place(source, 2.0), 3.0);
    for cap in [0, 1, 2] {
        let work = WorkBudget::new(cap);
        let result = EvaluationAdmission::Standard.within_work_slice(&work, |admission| higher(&Scratch::new(admission), &placed, 0.0));
        if cap < 2 { assert!(matches!(result, Err(EvaluationFailure::NoValue))); }
        else {
            let result = result.unwrap();
            assert_eq!(result.third.unwrap().get(), Vector3::new(72.0, 0.0, 0.0));
            assert_eq!(result.fourth.unwrap().get(), Vector3::new(-288.0, 0.0, 0.0));
        }
        assert_eq!(work.consumed(), cap);
    }
}
