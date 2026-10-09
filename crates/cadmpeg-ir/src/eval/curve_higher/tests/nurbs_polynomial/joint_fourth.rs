// SPDX-License-Identifier: Apache-2.0
use super::*;

const EPS_POLYNOMIAL_LAW: f64 = 128.0 * f64::EPSILON;

fn higher(scratch: &Scratch<'_, '_>, geometry: &SolvedCurveGeometry, parameter: f64)
    -> Result<CurveHigher, EvaluationFailure<()>>
{
    stored_higher(scratch, geometry, FiniteReal::new(parameter).unwrap(),
        Err(EvaluationFailure::NoValue), Err(EvaluationFailure::NoValue), crate::eval::ModelCurveRequest::Fourth)
}

#[test]
fn joint_polynomial_fourth_uses_one_selected_cox_triangle_and_original_third_arithmetic() {
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for degree in 4..=8 {
        let geometry = polynomial(degree, [0.0, 1.0], 1.0, false);
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            let scratch = Scratch::new(admission);
            for t in [0.0, 0.125, 0.5, 1.0] {
                let actual = higher(&scratch, &geometry, t).unwrap();
                assert_eq!(actual.third, third(&scratch, &geometry, t));
                let factor = f64::from(degree * (degree - 1) * (degree - 2) * (degree - 3));
                let expected = factor * t.powi(i32::try_from(degree - 4).unwrap());
                let fourth = actual.fourth.unwrap();
                assert!((fourth.x - expected).abs() <= EPS_POLYNOMIAL_LAW * expected.abs().max(1.0));
                assert_eq!((fourth.y, fourth.z), (0.0, 0.0));
            }
        }
    }
    // Blossom coefficients product(U[i+1]..U[i+4]) represent t^4.
    // Uniform knots with six poles have two actual selected spans, [0,1],[1,2].
    let geometry = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 4, (-4..=6).map(f64::from).collect::<Vec<_>>(),
        [0.0, 0.0, 0.0, 0.0, 24.0, 120.0].map(|x| Point3::new(x, 0.0, 0.0)).to_vec(),
        None, false).unwrap().unwrap());
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = Scratch::new(admission);
        for t in [0.0, 0.25, 1.0, 1.25, 2.0] {
            let actual = higher(&scratch, &geometry, t).unwrap();
            assert_eq!(actual.third, third(&scratch, &geometry, t));
            assert!((actual.third.unwrap().x - 24.0 * t).abs() <= EPS_POLYNOMIAL_LAW * 48.0);
            assert!((actual.fourth.unwrap().x - 24.0).abs() <= EPS_POLYNOMIAL_LAW * 24.0);
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn joint_polynomial_fourth_keeps_order_range_and_periodic_selection_independent() {
    let tiny = polynomial(4, [0.0, 2.0_f64.powi(-260)], 2.0_f64.powi(-1000), false);
    let overflowing = polynomial(4, [0.0, 1.0], f64::MAX, false);
    let wide = polynomial(4, [-f64::MAX, f64::MAX], f64::MAX, false);
    let periodic = polynomial(4, [0.0, 1.0], 1.0, true);
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = Scratch::new(admission);
        let actual = higher(&scratch, &tiny, 0.0).unwrap();
        assert_eq!(actual.third, Ok(FiniteVector3::ZERO));
        assert_eq!(actual.fourth.unwrap().x, 24.0 * 2.0_f64.powi(40));
        let actual = higher(&scratch, &overflowing, 0.0).unwrap();
        assert_eq!(actual.third, Ok(FiniteVector3::ZERO));
        assert_eq!(actual.fourth, Err(EvaluationFailure::NonFinite(())));
        // This raw Fourth recurrence cannot keep its nonzero coefficient
        // outside binary64; absence is not a sampled polynomial zero theorem.
        let actual = higher(&scratch, &wide, 0.0).unwrap();
        assert_eq!(actual.third, third(&scratch, &wide, 0.0));
        assert_eq!(actual.fourth, Err(EvaluationFailure::NoValue));
        for t in [-0.5, 0.5, 1.0, 1.5, 100.5] {
            let actual = higher(&scratch, &periodic, t).unwrap();
            assert_eq!(actual.third, third(&scratch, &periodic, t));
            assert_eq!(actual.fourth.unwrap().x, 24.0);
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn joint_polynomial_fourth_admits_actual_rows_growth_support_and_original_refusal() {
    let geometry = polynomial(4, [0.0, 1.0], 1.0, false);
    // Third rows3+4+5=12, Fourth rows2+3+4+5=14, one support5.
    // Original Third last row grows4->8: actual32 bytes of relocation work.
    // Third capacities4+4+8, Fourth empty reserves4+4+4+5 =>33 f64s.
    let bytes = 33 * u64::try_from(std::mem::size_of::<f64>()).unwrap();
    for (work, storage, items, dimension, operation) in [
        (62, bytes, 26, ResourceDimension::WorkUnits, "IR polynomial curve third support"),
        (63, bytes - 1, 26, ResourceDimension::MaterializedBytes, "IR scaled polynomial fourth basis"),
        (63, bytes, 25, ResourceDimension::CollectionItems, "IR scaled polynomial fourth basis"),
        (63, bytes, 26, ResourceDimension::WorkUnits, "success"),
    ] {
        let mut policy = DecodePolicy::service(); policy.limits.max_work_units = work;
        policy.limits.max_materialized_bytes = storage; policy.limits.max_collection_items = items;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx); let actual = higher(&scratch, &geometry, 0.25);
        if operation == "success" {
            let actual = actual.unwrap(); assert_eq!(actual.third.unwrap().x, 6.0);
            assert_eq!(actual.fourth.unwrap().x, 24.0); drop(scratch); ctx.finish_session().unwrap();
        } else {
            let Err(EvaluationFailure::ResourceLimit(original)) = actual else { panic!("actual next owner refuses"); };
            assert_eq!(original.dimension, dimension); assert_eq!(original.operation, operation);
            assert_eq!(higher(&scratch, &geometry, -1.0).err(), Some(EvaluationFailure::ResourceLimit(original)));
            drop(scratch); assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
        }
    }
    for (cap, fourth) in [(17, false), (30, true), (31, true)] {
        let work = WorkBudget::new(cap);
        let result = EvaluationAdmission::Standard.within_work_slice(&work, |admission| {
            let scratch = Scratch::new(admission);
            if fourth { higher(&scratch, &geometry, 0.25).map(|actual| actual.fourth) }
            else { third(&scratch, &geometry, 0.25).map(|third| {
                assert_eq!(third.x, 6.0); Err(EvaluationFailure::NoValue) }) }
        }).unwrap();
        if cap == 31 { assert_eq!(result.unwrap().x, 24.0); }
        else { assert_eq!(result, Err(EvaluationFailure::NoValue)); }
        assert_eq!(work.consumed(), cap);
    }
}

#[test]
fn joint_polynomial_fourth_preserves_real_subset_replica_placement_and_requested_lower_orders() {
    use crate::geometry::{Curve, CurveGeometry, ProceduralCurve};
    use crate::geometry::curve_payloads::SubsetCurveConstruction;
    use crate::ids::{CurveId, ProceduralCurveId};
    use crate::eval::ModelCurveRequest;
    let mut ir = crate::CadIr::empty();
    let source = CurveId::mint("test:model:curve#joint-polynomial-fourth-source").unwrap();
    let subset = CurveId::mint("test:model:curve#joint-polynomial-fourth-subset").unwrap();
    let replica = CurveId::mint("test:model:curve#joint-polynomial-fourth-replica").unwrap();
    ir.model.curves.push(Curve { id: source.clone(), geometry: CurveGeometry::Solved(
        polynomial(4, [0.0, 1.0], 1.0, false)), source_object: None });
    for (id, name, definition) in [
        (&subset, "subset", ProceduralCurveDefinition::Subset(
            SubsetCurveConstruction::try_new(source.clone(), [0.0, 1.0], false, None).unwrap())),
        (&replica, "replica", ProceduralCurveDefinition::Replica { source: subset.clone(), transform: Transform::affine([
            [2.0, 0.0, 0.0, 5.0], [0.0, 3.0, 0.0, 7.0], [0.0, 0.0, 4.0, 11.0],
        ]).unwrap() }),
    ] {
        let construction = ProceduralCurveId::mint(format!("test:model:construction#joint-polynomial-fourth-{name}")).unwrap();
        ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Procedural {
            construction: construction.clone(), cache: None }, source_object: None });
        ir.model.add_procedural_curve(&crate::document::admission::StandardAdmission, id,
            ProceduralCurve::new(construction, definition)).unwrap().unwrap();
    }
    let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        // Reversed Subset maps .75 to .25; its odd derivatives change sign.
        for (id, t, expected_third, expected_fourth) in [
            (&source, 0.25, 6.0, 24.0), (&subset, 0.75, -6.0, 24.0), (&replica, 0.75, -12.0, 48.0),
        ] {
            let evaluate = |request| crate::eval::model_curve_differential_by_id(admission,
                &index, id, t, request).unwrap();
            let actual = evaluate(ModelCurveRequest::Fourth);
            assert_eq!(actual.third.unwrap().x, expected_third);
            assert_eq!(actual.fourth.unwrap().x, expected_fourth);
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
    let geometry = place(place(polynomial(4, [0.0, 1.0], 1.0, false), 2.0), 3.0);
    // Two actual placement visits plus the31 coefficient/support operations.
    let work = WorkBudget::new(33);
    let actual = EvaluationAdmission::Standard.within_work_slice(&work, |admission|
        higher(&Scratch::new(admission), &geometry, 0.25)).unwrap();
    assert_eq!(actual.third.unwrap().x, 36.0); assert_eq!(actual.fourth.unwrap().x, 144.0);
    assert_eq!(work.consumed(), 33);
}
