// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::admission::EvaluationAdmission;
use crate::eval::{ContactRequest, EvaluationFailure};
use crate::index::{ModelIndex, StandardIndex};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, WorkBudget};
use cadmpeg_core::CodecError;

const EPS_REQUESTED_BLEND_POINT: f64 = 64.0 * f64::EPSILON;

fn fixture() -> (CadIr, SurfaceId) {
    variable_blend_eval_fixture(
        Point3::new(0.0, 0.0, 0.0),
        [(Point2::new(2.0, 0.0), Point2::new(2.0, 1.0)),
         (Point2::new(0.0, 2.0), Point2::new(1.0, 2.0))],
        [2.0, 4.0], Some(VariableBlendCrossSection::Circular {}),
    )
}

#[test]
fn actual_contact_support_and_tangent_do_not_request_a_second_support_visit() {
    let (ir, _) = fixture();
    let ProceduralSurfaceDefinition::VariableBlend(payload) = ir.model.procedural_surfaces[0].definition() else {
        panic!("actual variable blend fixture");
    };
    let side = &payload.construction().sides[0];
    let index = ModelIndex::build(&ir, StandardIndex);
    // Each actual stored plane requires one model carrier step. Only the
    // normal derivative reads the separately selected second-order support.
    for request in [ContactRequest::Support, ContactRequest::Tangent, ContactRequest::NormalDerivative] {
        for cap in [1, 2] {
            let work = WorkBudget::new(cap);
            let track = EvaluationAdmission::Standard.within_work_slice(&work, |admission| {
                crate::eval::variable_blend_contact_track(admission, &index, side, 0.5, request)
            }).unwrap();
            assert_eq!(track.point(), Point3::new(3.0, 0.5, 0.0));
            assert_eq!(track.normal(), Ok(Vector3::new(0.0, 0.0, 1.0)));
            if request == ContactRequest::Support {
                assert_eq!(track.tangent(), Err(EvaluationFailure::NoValue));
            } else { assert_eq!(track.tangent(), Ok(Vector3::new(2.0, 1.0, 0.0))); }
            if request == ContactRequest::NormalDerivative && cap == 2 {
                assert_eq!(track.normal_derivative, Ok(Vector3::new(0.0, 0.0, 0.0)));
                assert_eq!(work.consumed(), 2);
            } else {
                assert_eq!(track.normal_derivative, Err(EvaluationFailure::NoValue));
                assert_eq!(work.consumed(), 1);
            }
        }
    }
}

#[test]
fn actual_circular_point_reads_two_supports_and_one_radius_value() {
    let (ir, _) = fixture();
    let ProceduralSurfaceDefinition::VariableBlend(payload) = ir.model.procedural_surfaces[0].definition() else {
        panic!("actual variable blend fixture");
    };
    let index = ModelIndex::build(&ir, StandardIndex);
    let expected = 3.0 - 3.0 / 2.0_f64.sqrt();
    // Two stored support steps plus the actual TwoEnds radius step. The
    // separate radius derivative and support seconds are not point inputs.
    let work = WorkBudget::new(3);
    let point = EvaluationAdmission::Standard.within_work_slice(&work, |admission| {
        crate::eval::cacheless_circular_variable_blend_point(admission, &index, payload, 0.5, 0.5)
    }).unwrap();
    for (actual, expected) in [point.x, point.y, point.z].into_iter().zip([expected, 0.5, expected]) {
        assert!((actual - expected).abs() <= EPS_REQUESTED_BLEND_POINT);
    }
    assert_eq!(work.consumed(), 3);
    let refused = WorkBudget::new(2);
    assert_eq!(EvaluationAdmission::Standard.within_work_slice(&refused, |admission| {
        crate::eval::cacheless_circular_variable_blend_point(admission, &index, payload, 0.5, 0.5)
    }), Err(EvaluationFailure::NoValue));
    assert_eq!(refused.consumed(), 2);
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let decoded = crate::eval::cacheless_circular_variable_blend_point(EvaluationAdmission::Decode(&ctx), &index, payload, 0.5, 0.5).unwrap();
    assert_eq!(decoded, point);
    ctx.finish_session().unwrap();
}

#[test]
fn actual_contact_request_keeps_original_decode_refusal_on_reentry_and_finish() {
    let (ir, _) = fixture();
    let ProceduralSurfaceDefinition::VariableBlend(payload) = ir.model.procedural_surfaces[0].definition() else {
        panic!("actual variable blend fixture");
    };
    let index = ModelIndex::build(&ir, StandardIndex);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let side = &payload.construction().sides[0];
    let first = crate::eval::variable_blend_contact_track(EvaluationAdmission::Decode(&ctx), &index, side, 0.5, ContactRequest::Support);
    let Err(EvaluationFailure::ResourceLimit(original)) = first else { panic!("the actual support visit must refuse"); };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    for request in [ContactRequest::Support, ContactRequest::Tangent, ContactRequest::NormalDerivative] {
        assert!(matches!(crate::eval::variable_blend_contact_track(EvaluationAdmission::Decode(&ctx), &index, side, f64::NAN, request), Err(EvaluationFailure::ResourceLimit(limit)) if limit == original));
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}
