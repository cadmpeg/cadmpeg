// SPDX-License-Identifier: Apache-2.0
//! History resource-budget unit tests.
#![allow(clippy::unwrap_used)]

use crate::history::test_support::{decode_with_limits, one_delta_state, one_state_history};
#[test]
fn face_operand_recipe_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let recipe = crate::records::recipes::ConstructionRecipe {
        id: "recipe".into(),
        byte_offset: 0,
        kind: crate::records::recipes::ConstructionRecipeKind::Face,
        design: None,
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 1,
            offset: 0,
        }),
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::history::bind_face_operand_history_candidates(
        &ctx,
        &mut [],
        &[],
        &[],
        &[recipe],
        &[],
        &std::collections::HashMap::new(),
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D face operand recipe records")
    );
}

#[test]
fn history_graph_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::history::graph_is_coherent_charged(&ctx, &one_state_history()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ASM history states")
    );
}

#[test]
fn history_id_refuses_retained_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let error = decode_with_limits(&[], &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID")
    );
}

#[test]
fn history_state_id_refuses_retained_limit() {
    let bytes = one_delta_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();

    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D native record ID",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();

            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(decode_with_limits(&bytes, &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID")
    );
}

#[test]
fn history_state_vector_refuses_collection_limit() {
    let bytes = one_delta_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "admit F3D ASM delta state")
    );
}

#[test]
fn history_parent_copy_refuses_retained_limit() {
    let bytes = one_delta_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();

    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D ASM history parent",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();

            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(decode_with_limits(&bytes, &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D ASM history parent")
    );
}

#[test]
fn history_delta_offsets_refuse_collection_limit() {
    let bytes = [crate::history::DELTA, crate::history::DELTA].concat();
    let operation = "collect F3D ASM delta offsets";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        0,
        |ctx| {
            crate::history::decode(
                ctx,
                &bytes,
                "history",
                cadmpeg_asm::kernel_header::RefWidth::Four,
                &cadmpeg_core::decode::DecodePolicy::service().limits,
            )
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

fn history_decode_work_refusal(bytes: &[u8], operation: &'static str) -> cadmpeg_core::CodecError {
    let limits = cadmpeg_core::decode::ResourceLimits::service();
    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            crate::history::decode(
                ctx,
                bytes,
                "history",
                cadmpeg_asm::kernel_header::RefWidth::Four,
                &limits,
            )
            .map(|_| ())
        },
    )
}

#[test]
fn history_preamble_search_refuses_work_limit() {
    let error = history_decode_work_refusal(&one_delta_state(), "find F3D ASM preamble");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "find F3D ASM preamble"
    ));
}

#[test]
fn history_delta_marker_search_refuses_work_limit() {
    let error = history_decode_work_refusal(&one_delta_state(), "find F3D ASM delta markers");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "find F3D ASM delta markers"
    ));
}

#[test]
fn history_delta_offset_scan_refuses_work_limit() {
    let error = history_decode_work_refusal(&one_delta_state(), "scan F3D ASM delta offsets");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "scan F3D ASM delta offsets"
    ));
}

fn one_graph_history_with_board_and_record() -> crate::history_records::AsmHistory {
    use crate::history_records::{
        AsmBulletinBoard, AsmEntityChange, AsmEntityChangeKind, AsmHistoryRecord,
        AsmHistoryRecordFraming,
    };

    let mut history = one_state_history();
    history.states[0].bulletin_boards.push(AsmBulletinBoard {
        id: "board".into(),
        parent: "state".into(),
        byte_offset: 0,
        owner_ref: 0,
        number: 1,
        changes: vec![AsmEntityChange {
            id: "change".into(),
            parent: "board".into(),
            byte_offset: 0,
            kind: AsmEntityChangeKind::Insert { new: 1 },
        }],
    });
    history.states[0].records.push(AsmHistoryRecord {
        id: "record".into(),
        parent: "state".into(),
        revision_id: None,
        byte_offset: 0,
        framing: AsmHistoryRecordFraming::Framed {
            index: 0,
            name: "edge".into(),
            entity_references: Vec::new(),
        },
        raw_bytes: vec![0x11],
    });
    history
}

fn graph_work_refusal(
    operation: &'static str,
    history: &crate::history_records::AsmHistory,
) -> cadmpeg_core::CodecError {
    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| crate::history::graph_is_coherent_charged(ctx, history),
    )
}

#[test]
fn history_graph_state_iteration_refuses_work_limit() {
    let operation = "scan F3D ASM history states";
    let error = graph_work_refusal(operation, &one_state_history());
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_graph_state_index_refuses_scoped_storage_limit() {
    let operation = "index F3D ASM history states";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| crate::history::graph_is_coherent_charged(ctx, &one_state_history()),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_graph_parent_comparison_refuses_work_limit() {
    let operation = "compare F3D ASM history state parent";
    let error = graph_work_refusal(operation, &one_state_history());
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_graph_tail_count_refuses_work_limit() {
    let operation = "scan F3D ASM history chain ends";
    let error = graph_work_refusal(operation, &one_state_history());
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_graph_chain_step_refuses_work_limit() {
    let operation = "visit F3D ASM history chain link";
    let error = graph_work_refusal(operation, &one_state_history());
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_graph_bulletin_board_iteration_refuses_work_limit() {
    let operation = "scan F3D ASM history bulletin boards";
    let error = graph_work_refusal(operation, &one_graph_history_with_board_and_record());
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_graph_board_parent_comparison_refuses_work_limit() {
    let operation = "compare F3D ASM history board parent";
    let error = graph_work_refusal(operation, &one_graph_history_with_board_and_record());
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_graph_board_change_scan_refuses_work_limit() {
    let operation = "scan F3D ASM history board changes";
    let error = graph_work_refusal(operation, &one_graph_history_with_board_and_record());
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_graph_change_parent_comparison_refuses_work_limit() {
    let operation = "compare F3D ASM history change parent";
    let error = graph_work_refusal(operation, &one_graph_history_with_board_and_record());
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_graph_record_scan_refuses_work_limit() {
    let operation = "scan F3D ASM history records";
    let error = graph_work_refusal(operation, &one_graph_history_with_board_and_record());
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_graph_record_parent_comparison_refuses_work_limit() {
    let operation = "compare F3D ASM history record parent";
    let error = graph_work_refusal(operation, &one_graph_history_with_board_and_record());
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_state_reach_range_refuses_work_limit() {
    let history = one_state_history();
    let state = &history.states[0];
    let operation = "walk F3D history state chain";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |decode| {
            crate::history::history_state_reaches(decode, &history, state, state.state_id)
                .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}
