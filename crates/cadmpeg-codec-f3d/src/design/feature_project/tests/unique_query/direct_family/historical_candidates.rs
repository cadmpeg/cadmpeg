// SPDX-License-Identifier: Apache-2.0
//! Draft candidate membership preserves state presence and equal repeated faces.

use super::{assert_refusal, fixtures};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use crate::records::topology::entity_selection::{
    DesignEntitySelectionFaceCandidate, DesignEntitySelectionOperand,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::fillet::HistoricalBinding;
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FaceSelection;

fn selection(candidates: Vec<DesignEntitySelectionFaceCandidate>) -> DesignEntitySelectionOperand {
    let mut selection = super::super::selection(101, 0, 600);
    selection.historical_face_candidates = candidates;
    selection
}

fn candidate(face: i64, states: Vec<i64>) -> DesignEntitySelectionFaceCandidate {
    DesignEntitySelectionFaceCandidate {
        history_id: "history".into(),
        historical: HistoricalBinding {
            kind: AsmHistoricalEntityKind::Coedge,
            entity_ref: 225,
            state_ids: states,
        },
        face_slot: face,
    }
}

fn assert_candidate_refusal(operation: &'static str) {
    let scope = fixtures::history_scope();
    let group = super::super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X21);
    let selections = [selection(vec![candidate(158, vec![7])])];
    let original = selections.clone();
    assert_refusal(operation, |ctx| {
        super::super::super::super::selected_historical_face_selection(
            ctx,
            &scope,
            &group,
            &selections,
            &[],
        )
    });
    assert_eq!(selections, original);
}

#[test]
fn draft_candidate_source_preserves_original_refusal() {
    assert_candidate_refusal("visit F3D Draft historical face candidate");
}

#[test]
fn draft_candidate_state_preserves_original_refusal() {
    assert_candidate_refusal("visit F3D Draft historical candidate state");
}

#[test]
fn draft_candidate_equal_faces_preserve_exact_historical_identity() {
    let scope = fixtures::history_scope();
    let group = super::super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X21);
    let mut states = vec![7];
    states.extend(std::iter::repeat_n(99, 4096));
    let selections = [selection(vec![
        candidate(999, vec![6]),
        candidate(158, states),
        candidate(158, vec![7]),
    ])];
    let original = selections.clone();
    crate::test_support::with_decode_context(|ctx| {
        let Some(FaceSelection::Historical {
            state,
            faces,
            native,
        }) = super::super::super::super::selected_historical_face_selection(
            ctx,
            &scope,
            &group,
            &selections,
            &[],
        )
        .unwrap()
        else {
            panic!("equal eligible faces must preserve historical projection");
        };
        let feature = crate::ids::neutral_feature_id(&scope);
        let prefix = crate::ids::history_input_prefix(&feature.key(), 7);
        assert_eq!(state, crate::ids::feature_input_topology_id(&feature, 7));
        assert_eq!(
            faces.as_slice(),
            &[crate::ids::history_input_face_id(&prefix, 158)]
        );
        assert_eq!(native.as_str(), group.id.as_str());
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(selections, original);
}

#[test]
fn draft_candidate_conflict_skips_unused_states_and_candidate_tail_without_storage() {
    let scope = fixtures::history_scope();
    let group = super::super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X21);
    let mut states = vec![7];
    states.extend(std::iter::repeat_n(99, 4096));
    let selections = [selection(vec![
        candidate(158, states.clone()),
        candidate(159, states),
        candidate(160, vec![99; 4096]),
    ])];
    let original = selections.clone();
    let scope_stream = crate::ids::native_stream(&scope.id).unwrap();
    let operand_stream = crate::ids::native_stream(&selections[0].id).unwrap();
    // Scope suffix, operand visit/suffix/equality/end, then two candidate
    // visits and their first matching state. No suffix is visited after conflict.
    let reached = u64::try_from(
        scope.id.len() - scope_stream.len() + 1 + selections[0].id.len() - operand_stream.len()
            + operand_stream.len()
            + 1
            + 4,
    )
    .unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = reached;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(
            super::super::super::super::selected_historical_face_selection(
                ctx,
                &scope,
                &group,
                &selections,
                &[]
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(original)) =
            ctx.charge_work(1, "observe Draft candidate reached work")
        else {
            panic!("exact reached work");
        };
        assert_eq!((original.used, original.additional), (reached, 1));
    });
    policy.limits.max_work_units -= 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            super::super::super::super::selected_historical_face_selection(
                ctx,
                &scope,
                &group,
                &selections,
                &[],
            )
        else {
            panic!("last reached state must refuse");
        };
        assert_eq!(
            original.operation,
            "visit F3D Draft historical candidate state"
        );
        assert_eq!((original.used, original.additional), (reached - 1, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
    assert_eq!(selections, original);
}

#[test]
fn draft_candidate_absent_state_preserves_no_projection_and_actual_end_work() {
    let scope = fixtures::history_scope();
    let group = super::super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X21);
    let selections = [selection(vec![
        candidate(158, vec![6]),
        candidate(159, Vec::new()),
    ])];
    let original = selections.clone();
    let scope_stream = crate::ids::native_stream(&scope.id).unwrap();
    let operand_stream = crate::ids::native_stream(&selections[0].id).unwrap();
    // Two candidate slots plus END; one state slot plus END, then empty END.
    let reached = u64::try_from(
        scope.id.len() - scope_stream.len() + 1 + selections[0].id.len() - operand_stream.len()
            + operand_stream.len()
            + 1
            + 6,
    )
    .unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = reached;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(
            super::super::super::super::selected_historical_face_selection(
                ctx,
                &scope,
                &group,
                &selections,
                &[]
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(original)) =
            ctx.charge_work(1, "observe absent candidate reached work")
        else {
            panic!("exact reached work");
        };
        assert_eq!((original.used, original.additional), (reached, 1));
    });
    policy.limits.max_work_units -= 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            super::super::super::super::selected_historical_face_selection(
                ctx,
                &scope,
                &group,
                &selections,
                &[],
            )
        else {
            panic!("candidate end probe must refuse");
        };
        assert_eq!(
            original.operation,
            "visit F3D Draft historical face candidate"
        );
        assert_eq!((original.used, original.additional), (reached - 1, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
    assert_eq!(selections, original);
}
