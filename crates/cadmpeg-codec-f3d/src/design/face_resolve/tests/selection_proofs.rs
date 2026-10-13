// SPDX-License-Identifier: Apache-2.0
use super::{face, loft_scope, start_geometry_fixture};
use crate::design::face_resolve::{resolved_direct_face_selection, resolved_face_group};
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::features::FaceSelection;
use cadmpeg_ir::ids::FaceId;

fn second_operand(first: &DesignFaceOperand) -> DesignFaceOperand {
    let mut draft = first.clone().into_draft();
    draft.record_index = 201;
    draft.recipe_record_index = 204;
    draft.next_record_index = 203;
    draft.id = "f3d:test:face-operand#201".to_owned();
    DesignFaceOperand::try_new(draft).unwrap()
}

fn two_members(group: &mut crate::records::topology::construction::DesignConstructionOperandGroup) {
    group
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 200,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 201,
                offset: 11,
            },
        ])
        .unwrap();
}

#[test]
fn grouped_generated_and_canonical_faces_copy_only_first_distinct_ids() {
    let (mut first, mut group, _) = start_geometry_fixture();
    two_members(&mut group);
    first.resolved_face_slots = vec![0, 10, -10, i64::MIN, i64::MAX];
    let mut second = second_operand(&first);
    second.candidate_faces = first
        .resolved_face_slots
        .iter()
        .map(|slot| face(*slot))
        .collect();
    let expected = second.candidate_faces.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(
        expected.iter().map(|id| id.as_str().len()).sum::<usize>()
            + 8 * std::mem::size_of::<FaceId>()
            + group.id.len(),
    )
    .unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            resolved_face_group(ctx, &group, &[first, second]).unwrap(),
            Some(FaceSelection::Resolved {
                faces: expected,
                native: group.id.clone()
            })
        );
    });
}

#[test]
fn grouped_noncanonical_slot_text_keeps_exact_native_identity() {
    for native in [
        "f3d:brep:entity#+10",
        "f3d:brep:entity#010",
        "foreign:model:face#10",
    ] {
        let (mut first, mut group, _) = start_geometry_fixture();
        two_members(&mut group);
        first.resolved_face_slots = vec![10];
        let mut second = second_operand(&first);
        second.candidate_faces = vec![FaceId::mint(native).unwrap()];
        crate::test_support::with_decode_context(|ctx| {
            assert_eq!(
                resolved_face_group(ctx, &group, &[first, second]).unwrap(),
                Some(FaceSelection::Resolved {
                    faces: vec![face(10), FaceId::mint(native).unwrap()],
                    native: group.id.clone()
                })
            );
        });
    }
}

#[test]
fn grouped_missing_late_member_keeps_all_identity_text_borrowed() {
    let (mut first, mut group, _) = start_geometry_fixture();
    two_members(&mut group);
    first.resolved_active_face =
        Some(FaceId::mint(format!("synthetic:test:face#{}", "x".repeat(4096))).unwrap());
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(resolved_face_group(ctx, &group, &[first]).unwrap(), None);
    });
}

#[test]
fn direct_face_multiset_preserves_multiplicity_and_sorts_only_accepted_output() {
    for (second_slots, agrees) in [(vec![20, 10, 10], true), (vec![10, 20, 20], false)] {
        let (mut first, _, _) = start_geometry_fixture();
        first.group = None;
        first.resolved_face_slots = vec![10, 10, 20];
        let mut second = second_operand(&first);
        second.scope_reference_ordinal = 1;
        second.resolved_face_slots = second_slots;
        let mut scope = loft_scope();
        scope
            .try_edit(|draft| {
                draft.record_index = 100;
                draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                    vec![200, 201],
                    vec![14, 25],
                    "reference_members",
                )
                .unwrap();
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        let mut policy = DecodePolicy::service();
        if !agrees {
            policy.limits.max_retained_bytes = 0;
        }
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let expected = agrees.then(|| FaceSelection::Resolved {
                faces: vec![face(10), face(10), face(20)],
                native: scope.id.clone(),
            });
            assert_eq!(
                resolved_direct_face_selection(ctx, &scope, &[first, second]).unwrap(),
                expected
            );
        });
    }
}
