// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::{model_curve_differential_by_id, ModelCurveRequest};
use cadmpeg_core::decode::WorkBudget;

#[test]
fn stored_fourth_uses_analytic_laws_and_preserves_separate_results() {
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let origin = Point3::new(0.0, 0.0, 0.0);
    let z = Vector3::new(0.0, 0.0, 1.0);
    let x = Vector3::new(1.0, 0.0, 0.0);
    let parameter = 0.5_f64;
    for (geometry, expected) in [
        (SolvedCurveGeometry::Line(LineCurve::try_new(origin, x).unwrap()), Vector3::new(0.0, 0.0, 0.0)),
        (SolvedCurveGeometry::Parabola(ParabolaCurve::try_new(origin, z, x, 2.0).unwrap()), Vector3::new(0.0, 0.0, 0.0)),
        (SolvedCurveGeometry::Circle(CircleCurve::try_new(origin, z, x, 2.0).unwrap()), Vector3::new(2.0 * parameter.cos(), 2.0 * parameter.sin(), 0.0)),
        (SolvedCurveGeometry::Ellipse(crate::geometry::analytic::EllipseCurve::try_new(origin, z, x, 2.0, 1.0).unwrap()), Vector3::new(2.0 * parameter.cos(), parameter.sin(), 0.0)),
        (SolvedCurveGeometry::Hyperbola(HyperbolaCurve::try_new(origin, z, x, 2.0, 3.0).unwrap()), Vector3::new(2.0 * parameter.cosh(), 3.0 * parameter.sinh(), 0.0)),
    ] {
        let tangent = crate::eval::curve_derivative_evaluation(&scratch, &geometry, parameter, crate::eval::CurveDerivative::First);
        let acceleration = crate::eval::curve_derivative_evaluation(&scratch, &geometry, parameter, crate::eval::CurveDerivative::Second);
        let actual = stored_higher(&scratch, &geometry, FiniteReal::new(parameter).unwrap(), tangent, acceleration, ModelCurveRequest::Fourth).unwrap();
        assert!((actual.fourth.unwrap().get() - expected).norm() <= 32.0 * f64::EPSILON * expected.norm().max(1.0));
        assert_eq!(actual.third, stored_third(&scratch, &geometry, FiniteReal::new(parameter).unwrap(), tangent));
        let lower = stored_higher(&scratch, &geometry, FiniteReal::new(parameter).unwrap(), tangent, acceleration, ModelCurveRequest::Third).unwrap();
        assert_eq!(lower.fourth, Err(EvaluationFailure::NoValue));
    }
    let circle = SolvedCurveGeometry::Circle(CircleCurve::try_new(origin, z, x, 2.0).unwrap());
    let acceleration = Ok(FiniteVector3::new(Vector3::new(-2.0, 0.0, 0.0)).unwrap());
    let separate = stored_higher(&scratch, &circle, FiniteReal::ZERO, Err(EvaluationFailure::NonFinite(())), acceleration, ModelCurveRequest::Fourth).unwrap();
    assert_eq!(separate.third, Err(EvaluationFailure::NonFinite(())));
    assert_eq!(separate.fourth.unwrap().get(), Vector3::new(2.0, 0.0, 0.0));
}

#[test]
fn stored_fourth_walks_two_placements_once_under_actual_decode_and_standard_caps() {
    let circle = SolvedCurveGeometry::Circle(CircleCurve::try_new(Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), 2.0).unwrap());
    let transform = Transform::affine([[2.0, 0.0, 0.0, 0.0], [0.0, 3.0, 0.0, 0.0], [0.0, 0.0, 4.0, 0.0]]).unwrap();
    let wrap = |basis| SolvedCurveGeometry::Transformed(PlacedCurve::try_new(Box::new(basis), transform).unwrap());
    let geometry = wrap(wrap(circle));
    let completed = Scratch::new(EvaluationAdmission::Standard);
    let tangent = crate::eval::curve_derivative_evaluation(&completed, &geometry, 0.0, crate::eval::CurveDerivative::First);
    let acceleration = crate::eval::curve_derivative_evaluation(&completed, &geometry, 0.0, crate::eval::CurveDerivative::Second);
    for cap in [0, 1, 2] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let result = stored_higher(&scratch, &geometry, FiniteReal::ZERO, tangent, acceleration, ModelCurveRequest::Fourth);
        let refused = if cap < 2 {
            let Err(EvaluationFailure::ResourceLimit(original)) = result else { panic!("next real placement must refuse") };
            assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.operation, "IR curve higher source traversal");
            assert!(matches!(stored_higher(&scratch, &geometry, FiniteReal::ZERO, tangent, acceleration, ModelCurveRequest::Fourth), Err(EvaluationFailure::ResourceLimit(sticky)) if sticky == original));
            Some(original)
        } else {
            let result = result.unwrap();
            assert_eq!(result.third.unwrap().get(), Vector3::new(0.0, -18.0, 0.0));
            assert_eq!(result.fourth.unwrap().get(), Vector3::new(8.0, 0.0, 0.0));
            None
        };
        drop(scratch);
        if let Some(original) = refused { assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)); }
        else { ctx.finish_session().unwrap(); }
    }
    for cap in [0, 1, 2] {
        let work = WorkBudget::new(cap);
        let result = EvaluationAdmission::Standard.within_work_slice(&work, |admission| {
            stored_higher(&Scratch::new(admission), &geometry, FiniteReal::ZERO, tangent, acceleration, ModelCurveRequest::Fourth)
        });
        if cap < 2 { assert!(matches!(result, Err(EvaluationFailure::NoValue))); }
        else { assert_eq!(result.unwrap().fourth.unwrap().get(), Vector3::new(8.0, 0.0, 0.0)); }
        assert_eq!(work.consumed(), cap);
    }
}

#[test]
fn fourth_chain_product_keeps_true_final_range_zero_and_even_sign() {
    let tiny = 2.0_f64.powi(-1000);
    let vector = FiniteVector3::new(Vector3::new(tiny, -tiny, 0.0)).unwrap();
    let large = FiniteReal::new(2.0_f64.powi(300)).unwrap();
    let expected = 2.0_f64.powi(200);
    assert_eq!(scale_fourth(vector, [large; 4]).unwrap().get(), Vector3::new(expected, -expected, 0.0));
    let negative = FiniteReal::new(-2.0).unwrap();
    let unit = FiniteVector3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap();
    assert_eq!(scale_fourth(unit, [negative; 4]).unwrap().get(), Vector3::new(16.0, 0.0, 0.0));
    let huge = FiniteReal::new(f64::MAX).unwrap();
    assert_eq!(scale_fourth(FiniteVector3::ZERO, [huge; 4]), Ok(FiniteVector3::ZERO));
    assert_eq!(scale_fourth(unit, [huge; 4]), Err(EvaluationFailure::NonFinite(())));
    assert_eq!(scale_fourth(unit, [huge, huge, FiniteReal::ZERO, huge]), Ok(FiniteVector3::ZERO));
}

#[test]
fn model_curve_fourth_preserves_replica_frame_and_subset_even_sign() {
    use crate::geometry::{Curve, CurveGeometry, ProceduralCurve};
    use crate::geometry::curve_payloads::SubsetCurveConstruction;
    use crate::ids::{CurveId, ProceduralCurveId};
    let mut ir = crate::CadIr::empty();
    let source = CurveId::mint("test:model:curve#fourth-source").unwrap();
    let subset = CurveId::mint("test:model:curve#fourth-subset").unwrap();
    let replica = CurveId::mint("test:model:curve#fourth-replica").unwrap();
    for id in [&source, &subset, &replica] {
        ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            CircleCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), 2.0).unwrap())), source_object: None });
    }
    let admission = crate::document::admission::StandardAdmission;
    ir.model.add_procedural_curve(&admission, &subset, ProceduralCurve::new(
        ProceduralCurveId::mint("test:model:construction#fourth-subset").unwrap(),
        ProceduralCurveDefinition::Subset(SubsetCurveConstruction::try_new(source, [0.0, 1.0], false, None).unwrap()),
    )).unwrap().unwrap();
    ir.model.add_procedural_curve(&admission, &replica, ProceduralCurve::new(
        ProceduralCurveId::mint("test:model:construction#fourth-replica").unwrap(),
        ProceduralCurveDefinition::Replica { source: subset, transform: Transform::affine([
            [2.0, 0.0, 0.0, 0.0], [0.0, 3.0, 0.0, 0.0], [0.0, 0.0, 4.0, 0.0],
        ]).unwrap() },
    )).unwrap().unwrap();
    let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
    let actual = model_curve_differential_by_id(EvaluationAdmission::Standard, &index, &replica, 0.0, ModelCurveRequest::Fourth).unwrap();
    let expected = Vector3::new(4.0 * 1.0_f64.cos(), 6.0 * 1.0_f64.sin(), 0.0);
    assert!((actual.fourth.unwrap().get() - expected).norm() <= 32.0 * f64::EPSILON * expected.norm());
    let lower = model_curve_differential_by_id(EvaluationAdmission::Standard, &index, &replica, 0.0, ModelCurveRequest::Third).unwrap();
    assert_eq!(actual.point, lower.point); assert_eq!(actual.tangent, lower.tangent);
    assert_eq!(actual.acceleration, lower.acceleration); assert_eq!(actual.third, lower.third);
    assert_eq!(lower.fourth, Err(EvaluationFailure::NoValue));
}

#[test]
fn model_helix_fourth_follows_taper_product_rule_and_native_domain() {
    use crate::geometry::{Curve, CurveGeometry, HelixCurveConstruction, HelixFrame, ProceduralCurve};
    use crate::ids::{CurveId, ProceduralCurveId};
    let mut ir = crate::CadIr::empty();
    let id = CurveId::mint("test:model:curve#fourth-helix").unwrap();
    let construction = ProceduralCurveId::mint("test:model:construction#fourth-helix").unwrap();
    ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Procedural { construction: construction.clone(), cache: None }, source_object: None });
    ir.model.add_procedural_curve(&crate::document::admission::StandardAdmission, &id,
        ProceduralCurve::new(construction, ProceduralCurveDefinition::Helix(HelixCurveConstruction::try_new(
            [0.25, 2.0], HelixFrame { center: Point3::new(1.0, -2.0, 3.0), major: Vector3::new(2.0, 0.0, 0.0),
                minor: Vector3::new(0.0, 2.0, 0.0), pitch: Vector3::new(0.0, 0.0, 3.0), axis: Vector3::new(0.0, 0.0, 1.0) }, 0.4, None,
        ).unwrap()))).unwrap().unwrap();
    let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
    let parameter = 0.75_f64;
    let slope = 0.4 / std::f64::consts::TAU;
    let radius = 2.0 * (1.0 + 0.4 * (parameter - 0.25) / std::f64::consts::TAU);
    let expected = Vector3::new(radius * parameter.cos() - 8.0 * slope * parameter.sin(),
        radius * parameter.sin() + 8.0 * slope * parameter.cos(), 0.0);
    let actual = model_curve_differential_by_id(EvaluationAdmission::Standard, &index, &id, parameter, ModelCurveRequest::Fourth).unwrap();
    assert!((actual.fourth.unwrap().get() - expected).norm() <= 32.0 * f64::EPSILON * expected.norm());
    for request in [ModelCurveRequest::Point, ModelCurveRequest::First, ModelCurveRequest::Second, ModelCurveRequest::Third] {
        let lower = model_curve_differential_by_id(EvaluationAdmission::Standard, &index, &id, parameter, request).unwrap();
        assert_eq!(actual.point, lower.point); assert_eq!(lower.fourth, Err(EvaluationFailure::NoValue));
    }
    assert!(matches!(model_curve_differential_by_id(EvaluationAdmission::Standard, &index, &id, 0.0, ModelCurveRequest::Fourth), Err(EvaluationFailure::NoValue)));
}
