// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::{model_curve_differential_by_id, ModelCurveRequest};
use cadmpeg_core::decode::WorkBudget;

#[test]
fn analytic_fifth_keeps_true_tangent_laws_and_independent_failures() {
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let origin = Point3::new(0.0, 0.0, 0.0);
    let z = Vector3::new(0.0, 0.0, 1.0);
    let x = Vector3::new(1.0, 0.0, 0.0);
    for geometry in [
        SolvedCurveGeometry::Line(LineCurve::try_new(origin, x).unwrap()),
        SolvedCurveGeometry::Parabola(ParabolaCurve::try_new(origin, z, x, 2.0).unwrap()),
        SolvedCurveGeometry::Circle(CircleCurve::try_new(origin, z, x, 2.0).unwrap()),
        SolvedCurveGeometry::Ellipse(crate::geometry::analytic::EllipseCurve::try_new(origin, z, x, 2.0, 1.0).unwrap()),
        SolvedCurveGeometry::Hyperbola(HyperbolaCurve::try_new(origin, z, x, 2.0, 3.0).unwrap()),
    ] {
        let tangent = crate::eval::curve_derivative_evaluation(&scratch, &geometry, 0.5, crate::eval::CurveDerivative::First);
        let acceleration = crate::eval::curve_derivative_evaluation(&scratch, &geometry, 0.5, crate::eval::CurveDerivative::Second);
        let evaluate = |request| stored_higher(&scratch, &geometry, FiniteReal::new(0.5).unwrap(), tangent, acceleration, request).unwrap();
        let actual = evaluate(ModelCurveRequest::Fifth);
        let expected = if matches!(geometry, SolvedCurveGeometry::Line(_) | SolvedCurveGeometry::Parabola(_)) {
            Ok(FiniteVector3::ZERO)
        } else { tangent };
        assert_eq!(actual.fifth, expected);
        let lower = evaluate(ModelCurveRequest::Fourth);
        assert_eq!(actual.third, lower.third); assert_eq!(actual.fourth, lower.fourth);
        assert_eq!(lower.fifth, Err(EvaluationFailure::NoValue));
    }
    let circle = SolvedCurveGeometry::Circle(CircleCurve::try_new(origin, z, x, 2.0).unwrap());
    let acceleration = Ok(FiniteVector3::new(Vector3::new(-2.0, 0.0, 0.0)).unwrap());
    let actual = stored_higher(&scratch, &circle, FiniteReal::ZERO, Err(EvaluationFailure::NonFinite(())),
        acceleration, ModelCurveRequest::Fifth).unwrap();
    assert_eq!(actual.fifth, Err(EvaluationFailure::NonFinite(())));
    assert_eq!(actual.fourth.unwrap().get(), Vector3::new(2.0, 0.0, 0.0));
}

#[test]
fn fifth_chain_product_preserves_six_factor_range_zero_and_odd_sign() {
    let tiny = 2.0_f64.powi(-1000);
    let vector = FiniteVector3::new(Vector3::new(tiny, -tiny, 0.0)).unwrap();
    let large = FiniteReal::new(2.0_f64.powi(300)).unwrap();
    let expected = 2.0_f64.powi(500);
    assert_eq!(scale_fifth(vector, [large; 5]).unwrap().get(), Vector3::new(expected, -expected, 0.0));
    let negative = FiniteReal::new(-2.0).unwrap();
    let unit = FiniteVector3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap();
    assert_eq!(scale_fifth(unit, [negative; 5]).unwrap().get(), Vector3::new(-32.0, 0.0, 0.0));
    let huge = FiniteReal::new(f64::MAX).unwrap();
    assert_eq!(scale_fifth(FiniteVector3::ZERO, [huge; 5]), Ok(FiniteVector3::ZERO));
    assert_eq!(scale_fifth(unit, [huge; 5]), Err(EvaluationFailure::NonFinite(())));
    assert_eq!(scale_fifth(unit, [huge, huge, FiniteReal::ZERO, huge, huge]), Ok(FiniteVector3::ZERO));
}

#[test]
fn analytic_fifth_walks_real_placements_once_in_both_admission_modes() {
    let circle = SolvedCurveGeometry::Circle(CircleCurve::try_new(Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), 2.0).unwrap());
    let transform = Transform::affine([[2.0, 0.0, 0.0, 0.0], [0.0, 3.0, 0.0, 0.0], [0.0, 0.0, 4.0, 0.0]]).unwrap();
    let wrap = |basis| SolvedCurveGeometry::Transformed(PlacedCurve::try_new(Box::new(basis), transform).unwrap());
    let geometry = wrap(wrap(circle));
    let complete = Scratch::new(EvaluationAdmission::Standard);
    let tangent = crate::eval::curve_derivative_evaluation(&complete, &geometry, 0.0, crate::eval::CurveDerivative::First);
    let acceleration = crate::eval::curve_derivative_evaluation(&complete, &geometry, 0.0, crate::eval::CurveDerivative::Second);
    for cap in [0, 1, 2] {
        let mut policy = DecodePolicy::service(); policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0; policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let evaluate = |admission| stored_higher(&Scratch::new(admission), &geometry, FiniteReal::ZERO,
            tangent, acceleration, ModelCurveRequest::Fifth);
        let actual = evaluate(EvaluationAdmission::Decode(&ctx));
        if cap < 2 {
            let Err(EvaluationFailure::ResourceLimit(original)) = actual else { panic!("next actual placement refuses") };
            assert_eq!(original.operation, "IR curve higher source traversal");
            assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
            assert!(matches!(evaluate(EvaluationAdmission::Decode(&ctx)), Err(EvaluationFailure::ResourceLimit(sticky)) if sticky == original));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
        } else { assert_eq!(actual.unwrap().fifth.unwrap().get(), Vector3::new(0.0, 18.0, 0.0)); ctx.finish_session().unwrap(); }
        let cap = usize::try_from(cap).unwrap(); let work = WorkBudget::new(cap);
        let actual = EvaluationAdmission::Standard.within_work_slice(&work, |admission| {
            stored_higher(&Scratch::new(admission), &geometry, FiniteReal::ZERO,
                tangent, acceleration, ModelCurveRequest::Fifth)
        });
        if cap < 2 { assert!(matches!(actual, Err(EvaluationFailure::NoValue))); }
        else { assert_eq!(actual.unwrap().fifth.unwrap().get(), Vector3::new(0.0, 18.0, 0.0)); }
        assert_eq!(work.consumed(), cap);
    }
}

#[test]
fn model_fifth_preserves_subset_odd_sign_replica_frame_and_helix_taper() {
    use crate::geometry::{Curve, CurveGeometry, HelixCurveConstruction, HelixFrame, ProceduralCurve};
    use crate::geometry::curve_payloads::SubsetCurveConstruction;
    use crate::ids::{CurveId, ProceduralCurveId};
    let mut ir = crate::CadIr::empty();
    let source = CurveId::mint("test:model:curve#fifth-source").unwrap();
    let subset = CurveId::mint("test:model:curve#fifth-subset").unwrap();
    let replica = CurveId::mint("test:model:curve#fifth-replica").unwrap();
    let helix = CurveId::mint("test:model:curve#fifth-helix").unwrap();
    ir.model.curves.push(Curve { id: source.clone(), geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), 2.0).unwrap())), source_object: None });
    for (id, name, definition) in [
        (&subset, "subset", ProceduralCurveDefinition::Subset(SubsetCurveConstruction::try_new(source.clone(), [0.0, 1.0], false, None).unwrap())),
        (&replica, "replica", ProceduralCurveDefinition::Replica { source: subset.clone(), transform: Transform::affine([
            [2.0, 0.0, 0.0, 0.0], [0.0, 3.0, 0.0, 0.0], [0.0, 0.0, 4.0, 0.0],
        ]).unwrap() }),
        (&helix, "helix", ProceduralCurveDefinition::Helix(HelixCurveConstruction::try_new([0.25, 2.0],
            HelixFrame { center: Point3::new(1.0, -2.0, 3.0), major: Vector3::new(2.0, 0.0, 0.0), minor: Vector3::new(0.0, 2.0, 0.0),
                pitch: Vector3::new(0.0, 0.0, 3.0), axis: Vector3::new(0.0, 0.0, 1.0) }, 0.4, None).unwrap())),
    ] {
        let construction = ProceduralCurveId::mint(format!("test:model:construction#fifth-{name}")).unwrap();
        ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Procedural { construction: construction.clone(), cache: None }, source_object: None });
        ir.model.add_procedural_curve(&crate::document::admission::StandardAdmission, id,
            ProceduralCurve::new(construction, definition)).unwrap().unwrap();
    }
    let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
    let slope = 0.4 / std::f64::consts::TAU;
    let parameter = 0.75_f64;
    let radius = 2.0 * (1.0 + 0.4 * (parameter - 0.25) / std::f64::consts::TAU);
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        for (id, t, expected) in [
            (&replica, 0.0, Vector3::new(4.0 * 1.0_f64.sin(), -6.0 * 1.0_f64.cos(), 0.0)),
            (&helix, parameter, Vector3::new(-radius * parameter.sin() + 10.0 * slope * parameter.cos(),
                radius * parameter.cos() + 10.0 * slope * parameter.sin(), 0.0)),
        ] {
            let evaluate = |request| model_curve_differential_by_id(admission, &index, id, t, request).unwrap();
            let actual = evaluate(ModelCurveRequest::Fifth);
            assert!((actual.fifth.unwrap().get() - expected).norm() <= 32.0 * f64::EPSILON * expected.norm());
            let lower = evaluate(ModelCurveRequest::Fourth);
            assert_eq!(actual.point, lower.point); assert_eq!(actual.tangent, lower.tangent);
            assert_eq!(actual.acceleration, lower.acceleration); assert_eq!(actual.third, lower.third); assert_eq!(actual.fourth, lower.fourth);
            assert_eq!(lower.fifth, Err(EvaluationFailure::NoValue));
        }
        assert!(matches!(model_curve_differential_by_id(admission, &index, &helix, 0.0, ModelCurveRequest::Fifth), Err(EvaluationFailure::NoValue)));
    }
    ctx.finish_session().unwrap();
}
