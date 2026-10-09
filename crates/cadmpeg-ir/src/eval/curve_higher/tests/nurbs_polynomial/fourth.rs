// SPDX-License-Identifier: Apache-2.0
use super::*;

fn higher(scratch: &Scratch<'_, '_>, geometry: &SolvedCurveGeometry, parameter: f64)
    -> Result<CurveHigher, EvaluationFailure<()>>
{
    stored_higher(scratch, geometry, FiniteReal::new(parameter).unwrap(),
        Err(EvaluationFailure::NoValue), Err(EvaluationFailure::NoValue), crate::eval::ModelCurveRequest::Fourth)
}

#[test]
fn polynomial_degree_zero_through_three_has_true_fourth_zero_with_the_same_selected_span() {
    for degree in 0..=3 {
        let geometry = polynomial(degree, [0.0, 1.0], 1.0, true);
        let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            let scratch = Scratch::new(admission);
            for t in [-0.5, 0.0, 0.25, 1.0, 1.5, 100.5] {
                let result = higher(&scratch, &geometry, t).unwrap();
                assert_eq!(result.fourth, Ok(FiniteVector3::ZERO));
                assert_eq!(result.third, third(&scratch, &geometry, t));
            }
        }
        ctx.finish_session().unwrap();
    }
    let geometry = polynomial(3, [0.0, 1.0], 1.0, false);
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    for t in [-0.5, 1.5] {
        let result = higher(&scratch, &geometry, t).unwrap();
        assert_eq!(result.third, Err(EvaluationFailure::NoValue));
        assert_eq!(result.fourth, Err(EvaluationFailure::NoValue));
    }
    for degree in 0..=2 {
        let geometry = polynomial(degree, [0.0, f64::from_bits(1)], f64::MAX, false);
        let mut policy = DecodePolicy::service(); policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0; policy.limits.max_collection_items = 0;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = higher(&Scratch::new(&ctx), &geometry, 0.0).unwrap();
        assert_eq!(result.third, Ok(FiniteVector3::ZERO)); assert_eq!(result.fourth, Ok(FiniteVector3::ZERO));
        ctx.finish_session().unwrap();
    }
}

#[test]
fn true_polynomial_fourth_zero_is_independent_of_third_overflow_or_missing_coefficients() {
    let least = f64::from_bits(1);
    let overflowing = polynomial(3, [0.0, 1.0], f64::MAX, false);
    let missing = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 3,
        vec![-3.0, -2.0, -1.0, 0.0, least, 1.0, 2.0, 3.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0), Point3::new(least, 0.0, 0.0)], None, false).unwrap().unwrap());
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = Scratch::new(admission);
        // C=MAX*t^3 gives C3=6*MAX outside range, C4=0 exactly.
        assert_eq!(higher(&scratch, &overflowing, 0.0).unwrap().third, Err(EvaluationFailure::NonFinite(())));
        assert_eq!(higher(&scratch, &overflowing, 0.0).unwrap().fourth, Ok(FiniteVector3::ZERO));
        // The old finite normalized recurrence cannot recover this selected Third.
        // The actual source is still polynomial degree3, hence Fourth0.
        let result = higher(&scratch, &missing, 0.0).unwrap();
        assert_eq!(result.third, Err(EvaluationFailure::NoValue)); assert_eq!(result.fourth, Ok(FiniteVector3::ZERO));
    }
    ctx.finish_session().unwrap();
}

#[test]
fn polynomial_fourth_zero_preserves_actual_third_work_storage_items_and_sticky_refusal() {
    let geometry = polynomial(3, [0.0, 1.0], 1.0, false);
    let bytes = 3 * 4 * u64::try_from(std::mem::size_of::<f64>()).unwrap();
    for (work, storage, items, dimension, operation) in [
        (12, bytes, 9, ResourceDimension::WorkUnits, "IR polynomial curve third support"),
        (13, bytes - 1, 9, ResourceDimension::MaterializedBytes, "IR scaled B-spline derivative basis"),
        (13, bytes, 8, ResourceDimension::CollectionItems, "IR scaled B-spline derivative basis"),
        (13, bytes, 9, ResourceDimension::WorkUnits, "success"),
    ] {
        let mut policy = DecodePolicy::service(); policy.limits.max_work_units = work;
        policy.limits.max_materialized_bytes = storage; policy.limits.max_collection_items = items;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx); let result = higher(&scratch, &geometry, 0.0);
        if operation == "success" {
            let result = result.unwrap(); assert_eq!(result.third.unwrap().x, 6.0);
            assert_eq!(result.fourth, Ok(FiniteVector3::ZERO)); drop(scratch); ctx.finish_session().unwrap();
        } else {
            let Err(EvaluationFailure::ResourceLimit(original)) = result else { panic!("real Third owner must refuse"); };
            assert_eq!(original.dimension, dimension); assert_eq!(original.operation, operation);
            assert_eq!(higher(&scratch, &geometry, -1.0).err(), Some(EvaluationFailure::ResourceLimit(original)));
            drop(scratch); assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
        }
    }
    for cap in [12, 13] {
        let work = WorkBudget::new(cap);
        let result = EvaluationAdmission::Standard.within_work_slice(&work, |admission|
            higher(&Scratch::new(admission), &geometry, 0.0));
        if cap == 12 { let result = result.unwrap();
            assert_eq!(result.third, Err(EvaluationFailure::NoValue));
            assert_eq!(result.fourth, Ok(FiniteVector3::ZERO)); }
        else { assert_eq!(result.unwrap().fourth, Ok(FiniteVector3::ZERO)); }
        assert_eq!(work.consumed(), cap);
    }
}

#[test]
fn model_polynomial_fourth_zero_keeps_subset_replica_and_lower_order_selection() {
    use crate::geometry::{Curve, CurveGeometry, ProceduralCurve};
    use crate::geometry::curve_payloads::SubsetCurveConstruction;
    use crate::ids::{CurveId, ProceduralCurveId};
    use crate::eval::ModelCurveRequest;
    let mut ir = crate::CadIr::empty();
    let source = CurveId::mint("test:model:curve#polynomial-fourth-source").unwrap();
    let subset = CurveId::mint("test:model:curve#polynomial-fourth-subset").unwrap();
    let replica = CurveId::mint("test:model:curve#polynomial-fourth-replica").unwrap();
    ir.model.curves.push(Curve { id: source.clone(), geometry: CurveGeometry::Solved(
        polynomial(3, [0.0, 1.0], 1.0, false)), source_object: None });
    for (id, name, definition) in [
        (&subset, "subset", ProceduralCurveDefinition::Subset(
            SubsetCurveConstruction::try_new(source.clone(), [0.0, 1.0], false, None).unwrap())),
        (&replica, "replica", ProceduralCurveDefinition::Replica { source: subset.clone(), transform: Transform::affine([
            [2.0, 0.0, 0.0, 5.0], [0.0, 3.0, 0.0, 7.0], [0.0, 0.0, 4.0, 11.0],
        ]).unwrap() }),
    ] {
        let construction = ProceduralCurveId::mint(format!("test:model:construction#polynomial-fourth-{name}")).unwrap();
        ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Procedural {
            construction: construction.clone(), cache: None }, source_object: None });
        ir.model.add_procedural_curve(&crate::document::admission::StandardAdmission, id,
            ProceduralCurve::new(construction, definition)).unwrap().unwrap();
    }
    let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        for (id, expected_third) in [(&source, 6.0), (&replica, -12.0)] {
            let evaluate = |request| crate::eval::model_curve_differential_by_id(admission,
                &index, id, 0.0, request).unwrap();
            let actual = evaluate(ModelCurveRequest::Fourth);
            assert_eq!(actual.third.unwrap().x, expected_third); assert_eq!(actual.fourth, Ok(FiniteVector3::ZERO));
            let old = evaluate(ModelCurveRequest::Third);
            assert_eq!(actual.point, old.point); assert_eq!(actual.tangent, old.tangent);
            assert_eq!(actual.acceleration, old.acceleration); assert_eq!(actual.third, old.third);
            for request in [ModelCurveRequest::Point, ModelCurveRequest::First, ModelCurveRequest::Second, ModelCurveRequest::Third] {
                assert_eq!(evaluate(request).fourth, Err(EvaluationFailure::NoValue));
            }
        }
    }
    ctx.finish_session().unwrap();
    let place = |basis, scale| SolvedCurveGeometry::Transformed(PlacedCurve::try_new(Box::new(basis),
        Transform::affine([[scale, 0.0, 0.0, 5.0], [0.0, 1.0, 0.0, 7.0], [0.0, 0.0, 1.0, 11.0]]).unwrap()).unwrap());
    let geometry = place(place(polynomial(3, [0.0, 1.0], 1.0, false), 2.0), 3.0);
    // The two actual placement visits plus original polynomial13 cost.
    for cap in [14, 15] {
        let work = WorkBudget::new(cap);
        let result = EvaluationAdmission::Standard.within_work_slice(&work, |admission|
            higher(&Scratch::new(admission), &geometry, 0.25));
        if cap == 14 { let result = result.unwrap();
            assert_eq!(result.third, Err(EvaluationFailure::NoValue));
            assert_eq!(result.fourth, Ok(FiniteVector3::ZERO)); }
        else { let result = result.unwrap(); assert_eq!(result.third.unwrap().x, 36.0);
            assert_eq!(result.fourth, Ok(FiniteVector3::ZERO)); }
        assert_eq!(work.consumed(), cap);
    }
}
