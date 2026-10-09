// SPDX-License-Identifier: Apache-2.0
//! Reached body-face proof, unread source tails and original refusal.

use super::{face_geometry, selection_fixture};
use crate::history::{body_recipe_face_body_candidates, external_bodies};
use crate::records::topology::body_recipe::DesignBodyRecipeOperand;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn operand(face_tail: usize, reference_tail: usize) -> DesignBodyRecipeOperand {
    let (_, _, operands, _) = selection_fixture();
    let mut draft = operands.into_iter().next().unwrap().into_draft();
    let face = cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#99").unwrap();
    for _ in 0..face_tail {
        draft.references[0].candidate_faces.push(face.clone());
    }
    for ordinal in 1..=reference_tail {
        let mut reference = draft.references[0].clone();
        reference.design_reference = 301 + u64::try_from(ordinal).unwrap();
        reference.design_reference_offset = 25 + 12 * u64::try_from(ordinal).unwrap();
        reference.form_offset = reference.design_reference_offset + 8;
        reference.candidate_faces = vec![face.clone()];
        draft.references.push(reference);
    }
    // Counted reference rows precede both the nested frame and UUIDs.
    let delta = 12 * u64::try_from(reference_tail).unwrap();
    draft.nested_record_index_offset += delta;
    draft.asset_id_offset += delta;
    draft.context_id_offset += delta;
    draft.next_byte_offset += delta;
    DesignBodyRecipeOperand::try_new(draft).unwrap()
}

fn proof_work(operand: &DesignBodyRecipeOperand) -> u64 {
    let (body, region, shell) = face_geometry();
    crate::test_support::with_decode_context(|ctx| {
        let external = external_bodies(
            ctx,
            std::slice::from_ref(&body),
            std::slice::from_ref(&region),
            std::slice::from_ref(&shell),
        ).unwrap();
        assert_eq!(body.id.as_str(), "f3d:brep:body#1");
        assert_eq!(body_recipe_face_body_candidates(ctx, operand, &body.id, &external).unwrap(), (true, true));
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(u64::MAX, "measure reached body proof")
        else { panic!("work measurement exceeds the service ceiling"); };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        original.used
    })
}

#[test]
fn selected_body_proof_does_not_visit_unread_face_tail() {
    assert_eq!(proof_work(&operand(0, 0)), proof_work(&operand(4096, 0)));
}

#[test]
fn selected_body_proof_does_not_visit_unread_reference_tail() {
    assert_eq!(proof_work(&operand(0, 0)), proof_work(&operand(0, 4096)));
}

#[test]
fn empty_body_face_query_is_free_and_keeps_original_refusal() {
    let mut draft = operand(0, 0).into_draft();
    draft.references.clear();
    draft.nested_record_index_offset -= 12;
    draft.asset_id_offset -= 12;
    let operand = DesignBodyRecipeOperand::try_new(draft).unwrap();
    let (body, _, _) = face_geometry();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let external = crate::history::ExternalBodies {
            body_by_face: Default::default(),
            bodies: Default::default(),
            _storage: ctx.reserve_scoped(0, "empty body fixture").unwrap(),
        };
        assert_eq!(body_recipe_face_body_candidates(ctx, &operand, &body.id, &external).unwrap(), (false, false));
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original body proof refusal")
        else { panic!("original body proof refusal"); };
        let Err(CodecError::ResourceLimit(sticky)) = body_recipe_face_body_candidates(ctx, &operand, &body.id, &external)
        else { panic!("empty body proof keeps the original refusal"); };
        assert_eq!(sticky, original);
    });
}

#[test]
fn body_face_query_preserves_absent_and_other_body_results() {
    let (body, region, shell) = face_geometry();
    let selected = cadmpeg_ir::ids::BodyId::mint("f3d:brep:body#2").unwrap();
    let active = operand(0, 0);
    let mut draft = operand(0, 0).into_draft();
    draft.references[0].candidate_faces = vec![cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#99").unwrap()];
    let absent = DesignBodyRecipeOperand::try_new(draft).unwrap();
    crate::test_support::with_decode_context(|ctx| {
        let external = external_bodies(ctx, std::slice::from_ref(&body), std::slice::from_ref(&region), std::slice::from_ref(&shell)).unwrap();
        assert_eq!(body_recipe_face_body_candidates(ctx, &active, &selected, &external).unwrap(), (true, false));
        assert_eq!(body_recipe_face_body_candidates(ctx, &absent, &body.id, &external).unwrap(), (false, false));
        assert_eq!(body_recipe_face_body_candidates(ctx, &active, &body.id, &external).unwrap(), (true, true));
        assert_eq!(body.id.as_str(), "f3d:brep:body#1");
        assert_eq!(ctx.resource_refusal(), None);
    });
}
