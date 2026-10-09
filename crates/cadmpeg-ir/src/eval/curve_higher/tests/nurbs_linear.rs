// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::{Curve, CurveGeometry, ProceduralCurve};
use crate::ids::{CurveId, ProceduralCurveId};
use crate::index::{ModelIndex, StandardIndex};

fn linear(knots: [f64; 2], endpoint: f64, weights: Option<[f64; 2]>) -> SolvedCurveGeometry {
    // Fixture construction is outside each original evaluation session.
    SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 1,
        vec![knots[0], knots[0], knots[1], knots[1]],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(endpoint, 0.0, 0.0)],
        weights.map(Vec::from), false).unwrap().unwrap())
}

fn third(scratch: &Scratch<'_, '_>, geometry: &SolvedCurveGeometry, parameter: f64)
    -> Result<FiniteVector3, EvaluationFailure<()>>
{
    stored_third(scratch, geometry, FiniteReal::new(parameter).unwrap(), Err(EvaluationFailure::NoValue))
}

#[test]
fn degree_one_rational_third_uses_the_true_law_and_common_weight_scale() {
    // C(t)=2t/(1+t), C'''(t)=12/(1+t)^4.
    for exponent in [-900, 0, 900] {
        let scale = 2.0_f64.powi(exponent);
        let geometry = linear([0.0, 1.0], 1.0, Some([scale, 2.0 * scale]));
        for parameter in [0.0, 0.5, 1.0] {
            let expected = Vector3::new(12.0 / (1.0_f64 + parameter).powi(4), 0.0, 0.0);
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_work_units = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            for admission in [EvaluationAdmission::Decode(&ctx), EvaluationAdmission::Standard] {
                let scratch = Scratch::new(admission);
                let actual = third(&scratch, &geometry, parameter).unwrap().get();
                assert!((actual.x - expected.x).abs() <= 16.0 * f64::EPSILON * expected.x.abs());
                assert_eq!((actual.y, actual.z), (expected.y, expected.z));
            }
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn rational_third_keeps_tiny_and_wide_spans_and_true_final_range_errors() {
    let tiny = linear([0.0, 2.0_f64.powi(-350)], 2.0_f64.powi(-1000), Some([1.0, 2.0]));
    let wide = linear([-f64::MAX, f64::MAX], 1.0, Some([1.0, 2.0]));
    let least = f64::from_bits(1);
    let overflow = linear([0.0, least], least, Some([1.0, 2.0]));
    let large_point = linear([0.0, 1.0], f64::MAX, Some([1.0, 2.0]));
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    // At t=0 the law is12*endpoint/span^3 =12*2^50, with no finite span cube.
    assert_eq!(third(&scratch, &tiny, 0.0).unwrap().get(), Vector3::new(12.0 * 2.0_f64.powi(50), 0.0, 0.0));
    assert_eq!(third(&scratch, &wide, 0.0), Ok(FiniteVector3::ZERO));
    assert_eq!(third(&scratch, &overflow, 0.0), Err(EvaluationFailure::NonFinite(())));
    assert_eq!(third(&scratch, &large_point, 0.0), Err(EvaluationFailure::NonFinite(())));
    for weights in [None, Some([1.0, 1.0])] {
        let polynomial = linear([0.0, least], f64::MAX, weights);
        assert_eq!(third(&scratch, &polynomial, 0.0), Ok(FiniteVector3::ZERO));
    }
}

#[test]
fn rational_third_uses_the_actual_periodic_derivative_mapping_and_domain() {
    let geometry = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 1, vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        Some(vec![1.0, 2.0]), true).unwrap().unwrap());
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let expected = third(&scratch, &geometry, 0.5).unwrap();
    for parameter in [-0.5, 1.5, 100.5] {
        assert_eq!(third(&scratch, &geometry, parameter), Ok(expected));
    }
    // The existing derivative owner maps the periodic upper endpoint to lower.
    assert_eq!(third(&scratch, &geometry, 1.0).unwrap().get(), Vector3::new(12.0, 0.0, 0.0));
    let nonperiodic = linear([0.0, 1.0], 1.0, Some([1.0, 2.0]));
    for parameter in [-0.5, 1.5] {
        assert_eq!(third(&scratch, &nonperiodic, parameter), Err(EvaluationFailure::NoValue));
    }
}

#[test]
fn rational_third_admits_the_real_span_prefix_and_preserves_original_refusal() {
    let count = 1024_u32;
    let end = f64::from(count - 1);
    let geometry = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 1,
        [0.0, 0.0].into_iter().chain((1..count - 1).map(f64::from)).chain([end, end]).collect::<Vec<_>>(),
        (0..count).map(|i| Point3::new(f64::from(i), 0.0, 0.0)).collect::<Vec<_>>(),
        None, false).unwrap().unwrap());
    for cap in [0, 1, 2] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let Err(EvaluationFailure::ResourceLimit(original)) = third(&scratch, &geometry, 0.25)
            else { panic!("next actual span prefix must refuse"); };
        assert_eq!(original.operation, "IR B-spline span search");
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
        let unsupported = SolvedCurveGeometry::Unknown { record: None };
        assert_eq!(third(&scratch, &unsupported, 0.0), Err(EvaluationFailure::ResourceLimit(original)));
        drop(scratch);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
    }
}

#[test]
fn rational_third_crosses_each_stored_placement_once_and_keeps_analytic_final_frame() {
    let place = |basis, scale| SolvedCurveGeometry::Transformed(PlacedCurve::try_new(
        Box::new(basis), Transform::affine([
            [scale, 0.0, 0.0, 5.0], [0.0, 3.0, 0.0, 7.0], [0.0, 0.0, 4.0, 11.0],
        ]).unwrap()).unwrap());
    let geometry = place(place(linear([0.0, 1.0], 1.0, Some([1.0, 2.0])), 2.0), 3.0);
    let tangent = crate::eval::curve_tangent_solved(EvaluationAdmission::Standard, &geometry, 0.0);
    for cap in [0, 1, 2] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let actual = stored_third(&scratch, &geometry, FiniteReal::ZERO, tangent);
        let refused = if cap < 2 {
            let Err(EvaluationFailure::ResourceLimit(original)) = actual else { panic!("actual placement walk must refuse"); };
            assert_eq!(original.operation, "IR curve higher source traversal");
            assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
            Some(original)
        } else {
            assert_eq!(actual.unwrap().get(), Vector3::new(72.0, 0.0, 0.0));
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
fn model_rational_third_preserves_lower_orders_and_procedural_odd_chain() {
    use crate::geometry::curve_payloads::SubsetCurveConstruction;
    let mut ir = crate::CadIr::empty();
    let source = CurveId::mint("test:model:curve#rational-source").unwrap();
    ir.model.curves.push(Curve { id: source.clone(), geometry: CurveGeometry::Solved(
        linear([0.0, 1.0], 1.0, Some([1.0, 2.0]))), source_object: None });
    let subset = CurveId::mint("test:model:curve#rational-subset").unwrap();
    let replica = CurveId::mint("test:model:curve#rational-replica").unwrap();
    for id in [&subset, &replica] {
        let name = if id == &subset { "rational-subset" } else { "rational-replica" };
        let construction = ProceduralCurveId::mint(format!("test:model:construction#{name}")).unwrap();
        ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Procedural { construction: construction.clone(), cache: None }, source_object: None });
        let definition = if id == &subset {
            ProceduralCurveDefinition::Subset(SubsetCurveConstruction::try_new(source.clone(), [0.0, 1.0], false, None).unwrap())
        } else {
            ProceduralCurveDefinition::Replica { source: subset.clone(), transform: Transform::affine([
                [2.0, 0.0, 0.0, 5.0], [0.0, 3.0, 0.0, 7.0], [0.0, 0.0, 4.0, 11.0],
            ]).unwrap() }
        };
        ir.model.add_procedural_curve(&crate::document::admission::StandardAdmission, id,
            ProceduralCurve::new(construction, definition)).unwrap().unwrap();
    }
    let index = ModelIndex::build(&ir, StandardIndex);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Decode(&ctx), EvaluationAdmission::Standard] {
        for (id, expected) in [(&source, 12.0), (&replica, -1.5)] {
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
}
