// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::geometry::nurbs::NurbsCurve;
use cadmpeg_core::decode::WorkBudget;

fn polynomial(degree: u32, interval: [f64; 2], endpoint: f64, periodic: bool) -> SolvedCurveGeometry {
    // C(t)=endpoint*((t-a)/(b-a))^degree on this clamped span.
    // The fixture is built before the actual original evaluation context.
    let count = degree + 1;
    SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), degree,
        std::iter::repeat_n(interval[0], usize::try_from(count).unwrap())
            .chain(std::iter::repeat_n(interval[1], usize::try_from(count).unwrap())).collect::<Vec<_>>(),
        (0..count).map(|i| Point3::new(if i == degree { endpoint } else { 0.0 }, 0.0, 0.0)).collect::<Vec<_>>(),
        None, periodic).unwrap().unwrap())
}

fn third(scratch: &Scratch<'_, '_>, geometry: &SolvedCurveGeometry, parameter: f64)
    -> Result<FiniteVector3, EvaluationFailure<()>>
{
    stored_third(scratch, geometry, FiniteReal::new(parameter).unwrap(), Err(EvaluationFailure::NoValue))
}

#[test]
fn polynomial_third_uses_the_true_cubic_quartic_and_scaled_span_laws() {
    let cubic = polynomial(3, [0.0, 1.0], 1.0, false);
    let quartic = polynomial(4, [0.0, 1.0], 1.0, false);
    let tiny = polynomial(3, [0.0, 2.0_f64.powi(-350)], 2.0_f64.powi(-1000), false);
    let overflow = polynomial(3, [0.0, f64::from_bits(1)], f64::from_bits(1), false);
    let subnormal = polynomial(3, [0.0, 2.0], f64::from_bits(4), false);
    let wide = polynomial(3, [-f64::MAX, f64::MAX], f64::MAX, false);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Decode(&ctx), EvaluationAdmission::Standard] {
        let scratch = Scratch::new(admission);
        for parameter in [0.0, 0.25, 0.5, 1.0] {
            assert_eq!(third(&scratch, &cubic, parameter).unwrap().get(), Vector3::new(6.0, 0.0, 0.0));
            assert_eq!(third(&scratch, &quartic, parameter).unwrap().get(), Vector3::new(24.0 * parameter, 0.0, 0.0));
        }
        // 6*q/h^3=6*2^50. The basis Third alone would overflow.
        assert_eq!(third(&scratch, &tiny, 0.0).unwrap().get(), Vector3::new(6.0 * 2.0_f64.powi(50), 0.0, 0.0));
        assert_eq!(third(&scratch, &overflow, 0.0), Err(EvaluationFailure::NonFinite(())));
        // q=4*2^-1074, h=2: 6q/h^3=3*2^-1074 exactly.
        assert_eq!(third(&scratch, &subnormal, 0.0).unwrap().get(), Vector3::new(f64::from_bits(3), 0.0, 0.0));
        assert_eq!(third(&scratch, &wide, 0.0), Ok(FiniteVector3::ZERO));
    }
    ctx.finish_session().unwrap();
}

#[test]
fn low_degree_polynomial_third_is_zero_without_inventing_a_rational_theorem() {
    let rational = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        Some(vec![1.0, 1.0, 2.0]), false).unwrap().unwrap());
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Decode(&ctx), EvaluationAdmission::Standard] {
        let scratch = Scratch::new(admission);
        for degree in 0..=2 {
            let geometry = polynomial(degree, [0.0, f64::from_bits(1)], f64::MAX, false);
            assert_eq!(third(&scratch, &geometry, 0.0), Ok(FiniteVector3::ZERO));
        }
        // This rational law is 2t^2/(1+t^2): its Third is zero at
        // t=0, but is -4608/625 at t=.5. Degree alone proves no zero.
        assert_eq!(third(&scratch, &rational, 0.0), Ok(FiniteVector3::ZERO));
        let actual = third(&scratch, &rational, 0.5).unwrap().x;
        assert!((actual + 4608.0 / 625.0).abs() <= 32.0 * f64::EPSILON * actual.abs());
    }
    ctx.finish_session().unwrap();
}

#[test]
fn polynomial_third_preserves_each_real_work_storage_and_item_boundary() {
    let geometry = polynomial(3, [0.0, 1.0], 1.0, false);
    // The degree-zero base is inline. Recurrence rows advance2+3+4 times,
    // then the support advances4 times. Each f64 Vec grows to capacity4;
    // Scratch keeps all three admitted backing charges until its own drop.
    let bytes = 3 * 4 * u64::try_from(std::mem::size_of::<f64>()).unwrap();
    for (work, storage, items, dimension, operation) in [
        (12, bytes, 9, ResourceDimension::WorkUnits, "IR polynomial curve third support"),
        (13, bytes - 1, 9, ResourceDimension::MaterializedBytes, "IR scaled B-spline derivative basis"),
        (13, bytes, 8, ResourceDimension::CollectionItems, "IR scaled B-spline derivative basis"),
        (13, bytes, 9, ResourceDimension::WorkUnits, "success"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_materialized_bytes = storage;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = items;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let result = third(&scratch, &geometry, 0.0);
        let original = if operation == "success" {
            assert_eq!(result.unwrap().get(), Vector3::new(6.0, 0.0, 0.0));
            None
        } else {
            let Err(EvaluationFailure::ResourceLimit(original)) = result else { panic!("next actual operation must refuse"); };
            assert_eq!(original.dimension, dimension);
            assert_eq!(original.operation, operation);
            if dimension == ResourceDimension::WorkUnits {
                assert_eq!((original.limit, original.used, original.additional), (12, 12, 1));
            }
            assert_eq!(third(&scratch, &geometry, -1.0), Err(EvaluationFailure::ResourceLimit(original)));
            Some(original)
        };
        drop(scratch);
        match original {
            Some(original) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)),
            None => { ctx.finish_session().unwrap(); }
        }
    }
    // Storage is scoped; collection items remain monotone. This separate
    // reuse control provides9 recurrence slots plus12 genuinely new slots.
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 13;
    policy.limits.max_materialized_bytes = bytes;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 21;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    {
        let scratch = Scratch::new(&ctx);
        assert_eq!(third(&scratch, &geometry, 0.0).unwrap().get(), Vector3::new(6.0, 0.0, 0.0));
    }
    let scratch = Scratch::new(&ctx);
    let reused = scratch.temporary_vec::<f64>(12, "test reused polynomial scratch").unwrap();
    assert_eq!((reused.0.len(), reused.0.capacity()), (0, 12));
    drop(reused); // tuple backing is destroyed before its reservation.
    drop(scratch);
    ctx.finish_session().unwrap();
}

#[test]
fn polynomial_third_independent_work_and_periodic_domain_are_actual() {
    let geometry = polynomial(3, [0.0, 1.0], 1.0, true);
    for cap in [12, 13] {
        let budget = WorkBudget::new(cap);
        let result = EvaluationAdmission::Standard.within_work_slice(&budget, |admission| {
            third(&Scratch::new(admission), &geometry, 0.0)
        });
        if cap == 12 { assert_eq!(result, Err(EvaluationFailure::NoValue)); }
        else { assert_eq!(result.unwrap().get(), Vector3::new(6.0, 0.0, 0.0)); }
        assert_eq!(budget.consumed(), cap);
    }
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    for parameter in [-0.5, 0.5, 1.0, 1.5, 100.5] {
        assert_eq!(third(&scratch, &geometry, parameter).unwrap().get(), Vector3::new(6.0, 0.0, 0.0));
    }
    let ordinary = polynomial(3, [0.0, 1.0], 1.0, false);
    for parameter in [-0.5, 1.5] { assert_eq!(third(&scratch, &ordinary, parameter), Err(EvaluationFailure::NoValue)); }
}

#[test]
fn polynomial_third_admits_each_actual_span_comparison_before_any_rows() {
    let count = 1024_u32;
    let end = f64::from(count - 3);
    let geometry = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 3,
        std::iter::repeat_n(0.0, 4).chain((1..count - 3).map(f64::from))
            .chain(std::iter::repeat_n(end, 4)).collect::<Vec<_>>(),
        (0..count).map(|_| Point3::new(0.0, 0.0, 0.0)).collect::<Vec<_>>(), None, false).unwrap().unwrap());
    for cap in 0..=2 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let Err(EvaluationFailure::ResourceLimit(original)) = third(&scratch, &geometry, 0.25)
            else { panic!("the next real span comparison must refuse"); };
        assert_eq!(original.operation, "IR B-spline span search");
        assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
        drop(scratch);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
        let budget = WorkBudget::new(usize::try_from(cap).unwrap());
        let result = EvaluationAdmission::Standard.within_work_slice(&budget, |admission| {
            third(&Scratch::new(admission), &geometry, 0.25)
        });
        assert_eq!(result, Err(EvaluationFailure::NoValue));
        assert_eq!(budget.consumed(), usize::try_from(cap).unwrap());
    }
}

#[test]
fn model_polynomial_third_preserves_lower_orders_and_local_odd_placement() {
    use crate::geometry::{Curve, CurveGeometry, ProceduralCurve};
    use crate::geometry::curve_payloads::SubsetCurveConstruction;
    use crate::ids::{CurveId, ProceduralCurveId};
    let mut ir = crate::CadIr::empty();
    let source = CurveId::mint("test:model:curve#polynomial-source").unwrap();
    ir.model.curves.push(Curve { id: source.clone(), geometry: CurveGeometry::Solved(
        polynomial(3, [0.0, 1.0], 1.0, false)), source_object: None });
    let subset = CurveId::mint("test:model:curve#polynomial-subset").unwrap();
    let replica = CurveId::mint("test:model:curve#polynomial-replica").unwrap();
    for (id, name, definition) in [
        (&subset, "subset", ProceduralCurveDefinition::Subset(
            SubsetCurveConstruction::try_new(source.clone(), [0.0, 1.0], false, None).unwrap())),
        (&replica, "replica", ProceduralCurveDefinition::Replica { source: subset.clone(), transform: Transform::affine([
            [2.0, 0.0, 0.0, 5.0], [0.0, 3.0, 0.0, 7.0], [0.0, 0.0, 4.0, 11.0],
        ]).unwrap() }),
    ] {
        let construction = ProceduralCurveId::mint(format!("test:model:construction#polynomial-{name}")).unwrap();
        ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Procedural {
            construction: construction.clone(), cache: None }, source_object: None });
        ir.model.add_procedural_curve(&crate::document::admission::StandardAdmission, id,
            ProceduralCurve::new(construction, definition)).unwrap().unwrap();
    }
    let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Decode(&ctx), EvaluationAdmission::Standard] {
        for (id, expected) in [(&source, 6.0), (&replica, -12.0)] {
            let evaluate = |request| crate::eval::model_curve_differential_by_id(admission, &index, id, 0.0, request).unwrap();
            let result = evaluate(crate::eval::ModelCurveRequest::Third);
            assert_eq!(result.third.unwrap().get(), Vector3::new(expected, 0.0, 0.0));
            let second = evaluate(crate::eval::ModelCurveRequest::Second);
            assert_eq!(result.point, second.point);
            assert_eq!(result.tangent, second.tangent);
            assert_eq!(result.acceleration, second.acceleration);
            assert_eq!(second.third, Err(EvaluationFailure::NoValue));
            let first = evaluate(crate::eval::ModelCurveRequest::First);
            assert_eq!(first.point, result.point);
            assert_eq!(first.tangent, result.tangent);
            assert_eq!(first.acceleration, Err(EvaluationFailure::NoValue));
            let point = evaluate(crate::eval::ModelCurveRequest::Point);
            assert_eq!(point.point, result.point);
            assert_eq!(point.tangent, Err(EvaluationFailure::NoValue));
        }
    }
    ctx.finish_session().unwrap();
    let place = |basis, scale| SolvedCurveGeometry::Transformed(PlacedCurve::try_new(Box::new(basis),
        Transform::affine([[scale, 0.0, 0.0, 5.0], [0.0, 1.0, 0.0, 7.0], [0.0, 0.0, 1.0, 11.0]]).unwrap()).unwrap());
    let placed = place(place(polynomial(3, [0.0, 1.0], 1.0, false), 2.0), 3.0);
    assert_eq!(third(&Scratch::new(EvaluationAdmission::Standard), &placed, 0.25).unwrap().get(), Vector3::new(36.0, 0.0, 0.0));
}

#[test]
fn polynomial_third_reports_missing_extended_coefficients_without_fabricating_zero() {
    let least = f64::from_bits(1);
    let geometry = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 3,
        vec![-3.0, -2.0, -1.0, 0.0, least, 1.0, 2.0, 3.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0), Point3::new(least, 0.0, 0.0)], None, false).unwrap().unwrap());
    // The final basis term's Third at zero is6/((2+h)*(1+h)*h).
    // Multiplying its pole h gives a finite value near3. Its normalized
    // h-coordinate coefficients need nonzero terms below binary64 range.
    // The existing finite recurrence cannot supply those terms; retain a
    // truthful missing order instead of claiming their rounded zero exact.
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    assert_eq!(third(&scratch, &geometry, 0.0), Err(EvaluationFailure::NoValue));
    assert!(crate::eval::decode::curve_point_solved(EvaluationAdmission::Standard, &geometry, 0.0).is_ok());
}

mod fourth;
