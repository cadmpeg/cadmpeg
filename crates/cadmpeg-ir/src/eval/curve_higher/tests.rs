// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::admission::EvaluationAdmission;
use crate::geometry::analytic::{CircleCurve, HyperbolaCurve, LineCurve, ParabolaCurve};
use crate::geometry::PlacedCurve;
use crate::math::{Point3, Vector3};
use crate::transform::Transform;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn stored_analytic_third_follows_the_actual_curve_law() {
    let origin = Point3::new(0.0, 0.0, 0.0);
    let z = Vector3::new(0.0, 0.0, 1.0);
    let x = Vector3::new(1.0, 0.0, 0.0);
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    for (geometry, expected) in [
        (SolvedCurveGeometry::Line(LineCurve::try_new(origin, x).unwrap()), FiniteVector3::ZERO),
        (SolvedCurveGeometry::Parabola(ParabolaCurve::try_new(origin, z, x, 2.0).unwrap()), FiniteVector3::ZERO),
        (SolvedCurveGeometry::Circle(CircleCurve::try_new(origin, z, x, 2.0).unwrap()), FiniteVector3::new(Vector3::new(0.0, -2.0, 0.0)).unwrap()),
        (SolvedCurveGeometry::Hyperbola(HyperbolaCurve::try_new(origin, z, x, 2.0, 3.0).unwrap()), FiniteVector3::new(Vector3::new(0.0, 3.0, 0.0)).unwrap()),
    ] {
        let tangent = crate::eval::curve_derivative_evaluation(&scratch, &geometry, 0.0, crate::eval::CurveDerivative::First);
        assert_eq!(stored_third(&scratch, &geometry, tangent), Ok(expected));
    }
}

#[test]
fn stored_third_admits_each_real_placement_walk_and_preserves_the_original_fuse() {
    let circle = SolvedCurveGeometry::Circle(CircleCurve::try_new(
        Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), 2.0,
    ).unwrap());
    let wrap = |basis| SolvedCurveGeometry::Transformed(PlacedCurve::try_new(Box::new(basis), Transform::identity()).unwrap());
    let geometry = wrap(wrap(circle));
    // The actual final placed tangent is already owned by the differential.
    // This control isolates the two real classification source-pointer visits.
    let tangent = crate::eval::curve_tangent_solved(EvaluationAdmission::Standard, &geometry, 0.0);
    assert_eq!(tangent.unwrap().get(), Vector3::new(0.0, 2.0, 0.0));
    for cap in [0, 1, 2] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let result = stored_third(&scratch, &geometry, tangent);
        let refused = if cap < 2 {
            let Err(EvaluationFailure::ResourceLimit(original)) = result else { panic!("next actual source walk must refuse") };
            assert_eq!(original.operation, "IR curve higher source traversal");
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
            assert_eq!(stored_third(&scratch, &geometry, Err(EvaluationFailure::NoValue)), Err(EvaluationFailure::ResourceLimit(original)));
            Some(original)
        } else {
            assert_eq!(result, tangent.map(FiniteVector3::negated));
            assert_eq!(ctx.resource_refusal(), None);
            None
        };
        drop(scratch);
        match refused {
            Some(original) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)),
            None => { ctx.finish_session().unwrap(); }
        }
    }
}

#[test]
fn third_chain_factors_preserve_finite_final_products_zero_and_odd_sign() {
    let factors = [FiniteReal::new(1e150).unwrap(); 3];
    let vector = FiniteVector3::new(Vector3::new(1e-200, -1e-200, 0.0)).unwrap();
    let result = scale_third(vector, factors[0], factors[1], factors[2]).unwrap();
    // Each chain factor is 10^150 and each nonzero coordinate is 10^-200.
    // The product has order10^250 although the three factors alone overflow.
    let relative = result.x / 1e250;
    assert!((relative - 1.0).abs() <= 16.0 * f64::EPSILON);
    assert_eq!(result.y, -result.x);
    assert_eq!(result.z, 0.0);
    let huge = FiniteReal::new(f64::MAX).unwrap();
    assert_eq!(scale_third(FiniteVector3::ZERO, huge, huge, huge), Ok(FiniteVector3::ZERO));
    let negative = FiniteReal::new(-2.0).unwrap();
    assert_eq!(scale_third(FiniteVector3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap(), negative, negative, negative).unwrap().get(), Vector3::new(-8.0, 0.0, 0.0));
}

#[test]
fn model_curve_third_keeps_replica_placement_and_subset_odd_sign() {
    use crate::geometry::{Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition};
    use crate::geometry::curve_payloads::SubsetCurveConstruction;
    use crate::ids::{CurveId, ProceduralCurveId};
    let mut ir = crate::CadIr::empty();
    let source = CurveId::mint("test:model:curve#third-source").unwrap();
    let subset = CurveId::mint("test:model:curve#third-subset").unwrap();
    let replica = CurveId::mint("test:model:curve#third-replica").unwrap();
    for id in [&source, &subset, &replica] {
        ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Solved(
            SolvedCurveGeometry::Circle(CircleCurve::try_new(Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), 2.0).unwrap())), source_object: None });
    }
    let admission = crate::document::admission::StandardAdmission;
    ir.model.add_procedural_curve(&admission, &subset, ProceduralCurve::new(
        ProceduralCurveId::mint("test:model:construction#third-subset").unwrap(),
        ProceduralCurveDefinition::Subset(SubsetCurveConstruction::try_new(source, [0.0, 1.0], false, None).unwrap()),
    )).unwrap().unwrap();
    ir.model.add_procedural_curve(&admission, &replica, ProceduralCurve::new(
        ProceduralCurveId::mint("test:model:construction#third-replica").unwrap(),
        ProceduralCurveDefinition::Replica { source: subset, transform: Transform::affine([
            [2.0, 0.0, 0.0, 0.0], [0.0, 3.0, 0.0, 0.0], [0.0, 0.0, 4.0, 0.0],
        ]).unwrap() },
    )).unwrap().unwrap();
    let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
    let third = crate::eval::model_curve_differential_by_id(EvaluationAdmission::Standard,
        &index, &replica, 0.0, crate::eval::ModelCurveRequest::Third).unwrap();
    let expected = Vector3::new(-4.0 * 1.0_f64.sin(), 6.0 * 1.0_f64.cos(), 0.0);
    assert!((third.third.unwrap().get() - expected).norm() <= 16.0 * f64::EPSILON * expected.norm());
    let lower = crate::eval::model_curve_differential_by_id(EvaluationAdmission::Standard,
        &index, &replica, 0.0, crate::eval::ModelCurveRequest::Second).unwrap();
    assert_eq!(third.point, lower.point);
    assert_eq!(third.tangent, lower.tangent);
    assert_eq!(third.acceleration, lower.acceleration);
    assert_eq!(lower.third, Err(EvaluationFailure::NoValue));
}

#[test]
fn model_helix_third_uses_taper_product_rule_only_when_requested() {
    use crate::geometry::{Curve, CurveGeometry, HelixCurveConstruction, HelixFrame, ProceduralCurve};
    use crate::ids::{CurveId, ProceduralCurveId};
    let mut ir = crate::CadIr::empty();
    let id = CurveId::mint("test:model:curve#third-helix").unwrap();
    let construction = ProceduralCurveId::mint("test:model:construction#third-helix").unwrap();
    ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Procedural {
        construction: construction.clone(), cache: None,
    }, source_object: None });
    ir.model.add_procedural_curve(&crate::document::admission::StandardAdmission, &id,
        ProceduralCurve::new(construction, ProceduralCurveDefinition::Helix(HelixCurveConstruction::try_new(
            [0.25, 2.0], HelixFrame { center: Point3::new(1.0, -2.0, 3.0),
                major: Vector3::new(2.0, 0.0, 0.0), minor: Vector3::new(0.0, 2.0, 0.0),
                pitch: Vector3::new(0.0, 0.0, 3.0), axis: Vector3::new(0.0, 0.0, 1.0) }, 0.4, None,
        ).unwrap()))).unwrap().unwrap();
    let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
    let parameter = 0.75_f64;
    let scale_first = 0.4 / std::f64::consts::TAU;
    let scale = 1.0 + 0.4 * (parameter - 0.25) / std::f64::consts::TAU;
    let expected = Vector3::new(
        2.0 * (scale * parameter.sin() - 3.0 * scale_first * parameter.cos()),
        2.0 * (-scale * parameter.cos() - 3.0 * scale_first * parameter.sin()), 0.0);
    let actual = crate::eval::model_curve_differential_by_id(EvaluationAdmission::Standard,
        &index, &id, parameter, crate::eval::ModelCurveRequest::Third).unwrap();
    assert!((actual.third.unwrap().get() - expected).norm() <= 32.0 * f64::EPSILON * expected.norm());
    let point = crate::eval::model_curve_differential_by_id(EvaluationAdmission::Standard,
        &index, &id, parameter, crate::eval::ModelCurveRequest::Point).unwrap();
    assert_eq!(actual.point, point.point);
    assert_eq!(point.third, Err(EvaluationFailure::NoValue));
}
