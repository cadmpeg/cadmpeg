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
            std::slice::from_ref(&body),
            std::slice::from_ref(&region),
            std::slice::from_ref(&shell),
        );
        assert_eq!(body.id.as_str(), "f3d:brep:body#1");
        assert_eq!(
            body_recipe_face_body_candidates(ctx, operand, &body.id, &external).unwrap(),
            (true, true)
        );
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(original)) =
            ctx.charge_work(u64::MAX, "measure reached body proof")
        else {
            panic!("work measurement exceeds the service ceiling");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(ctx.resource_refusal(), Some(original));
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
fn empty_body_face_query_is_free() {
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
        let external = external_bodies(&[], &[], &[]);
        assert_eq!(
            body_recipe_face_body_candidates(ctx, &operand, &body.id, &external).unwrap(),
            (false, false)
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn body_face_query_preserves_absent_and_other_body_results() {
    let (body, region, shell) = face_geometry();
    let selected = cadmpeg_ir::ids::BodyId::mint("f3d:brep:body#2").unwrap();
    let active = operand(0, 0);
    let mut draft = operand(0, 0).into_draft();
    draft.references[0].candidate_faces =
        vec![cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#99").unwrap()];
    let absent = DesignBodyRecipeOperand::try_new(draft).unwrap();
    crate::test_support::with_decode_context(|ctx| {
        let external = external_bodies(
            std::slice::from_ref(&body),
            std::slice::from_ref(&region),
            std::slice::from_ref(&shell),
        );
        assert_eq!(
            body_recipe_face_body_candidates(ctx, &active, &selected, &external).unwrap(),
            (true, false)
        );
        assert_eq!(
            body_recipe_face_body_candidates(ctx, &absent, &body.id, &external).unwrap(),
            (false, false)
        );
        assert_eq!(
            body_recipe_face_body_candidates(ctx, &active, &body.id, &external).unwrap(),
            (true, true)
        );
        assert_eq!(body.id.as_str(), "f3d:brep:body#1");
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn empty_direct_body_fallback_skips_metadata_and_face_indexes() {
    use crate::history::{
        direct_body_recipe_candidate, BodySelectionIndex, FeatureBodySelectionInputs,
    };
    let (_, _, mut operands, _) = selection_fixture();
    let mut draft = operands.remove(0).into_draft();
    draft.references[0].candidate_faces.clear();
    operands.insert(0, DesignBodyRecipeOperand::try_new(draft).unwrap());
    let (body, region, shell) = face_geometry();
    for operation in [
        "index F3D external body metadata",
        "scan F3D external body shells",
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let inputs = FeatureBodySelectionInputs {
                scopes: &[],
                groups: &[],
                body_recipe_operands: &operands,
                construction_recipes: &[],
                persistent_design_links: &[],
                histories: &[],
                bodies: std::slice::from_ref(&body),
                regions: std::slice::from_ref(&region),
                shells: std::slice::from_ref(&shell),
            };
            let index = BodySelectionIndex::new(&inputs);
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                ResourceDimension::WorkUnits,
                operation,
                None,
            );
            assert!(direct_body_recipe_candidate(ctx, &operands[0], &index)
                .unwrap()
                .is_none());
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn empty_external_intersection_skips_unread_reference_tail() {
    use crate::history::unique_external_body_candidate;
    let work = |tail: usize| {
        let empty = operand(0, 0);
        let mut draft = empty.into_draft();
        draft.references[0].candidate_faces.clear();
        let mut reference = draft.references[0].clone();
        reference.candidate_faces = vec![crate::ids::brep_face_id(99)];
        for ordinal in 1..=tail {
            reference.design_reference = 301 + u64::try_from(ordinal).unwrap();
            reference.design_reference_offset = 25 + 12 * u64::try_from(ordinal).unwrap();
            reference.form_offset = reference.design_reference_offset + 8;
            draft.references.push(reference.clone());
        }
        let delta = 12 * u64::try_from(tail).unwrap();
        draft.nested_record_index_offset += delta;
        draft.asset_id_offset += delta;
        draft.context_id_offset += delta;
        draft.next_byte_offset += delta;
        let operand = DesignBodyRecipeOperand::try_new(draft).unwrap();
        crate::test_support::with_decode_context(|ctx| {
            let external = external_bodies(&[], &[], &[]);
            assert!(
                unique_external_body_candidate(ctx, &operand, None, &external)
                    .unwrap()
                    .is_none()
            );
            let CodecError::ResourceLimit(limit) = ctx
                .charge_work(u64::MAX, "measure intersection work")
                .unwrap_err()
            else {
                panic!("work measurement");
            };
            limit.used
        })
    };
    assert_eq!(work(1), work(4096));
}

#[test]
fn disjoint_external_intersection_skips_unread_reference_tail() {
    use crate::history::unique_external_body_candidate;
    use cadmpeg_ir::ids::{BodyId, RegionId, ShellId};
    use cadmpeg_ir::topology::Shell;
    let work = |tail: usize| {
        let mut draft = operand(0, tail + 1).into_draft();
        draft.references[1].candidate_faces = vec![crate::ids::brep_face_id(8)];
        let operand = DesignBodyRecipeOperand::try_new(draft).unwrap();
        let (body, region, shell) = face_geometry();
        let mut second_body = body.clone();
        second_body.id = BodyId::mint("f3d:brep:body#2").unwrap();
        second_body.regions = vec![RegionId::mint("f3d:model:region#2").unwrap()];
        let mut second_region = region.clone();
        second_region.id = second_body.regions[0].clone();
        second_region.body = second_body.id.clone();
        second_region.shells = vec![ShellId::mint("f3d:model:shell#2").unwrap()];
        let second_shell = Shell::with_face(
            second_region.shells[0].clone(),
            second_region.id.clone(),
            crate::ids::brep_face_id(8),
        );
        let bodies = [body, second_body];
        let regions = [region, second_region];
        let shells = [shell, second_shell];
        crate::test_support::with_decode_context(|ctx| {
            let external = external_bodies(&bodies, &regions, &shells);
            assert!(
                unique_external_body_candidate(ctx, &operand, None, &external)
                    .unwrap()
                    .is_none()
            );
            let CodecError::ResourceLimit(limit) = ctx
                .charge_work(u64::MAX, "measure disjoint intersection work")
                .unwrap_err()
            else {
                panic!("work measurement");
            };
            limit.used
        })
    };
    assert_eq!(work(1), work(4096));
}
