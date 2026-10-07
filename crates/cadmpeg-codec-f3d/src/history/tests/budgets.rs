// SPDX-License-Identifier: Apache-2.0
//! History resource-budget unit tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::u64_from_index;

use crate::history::{
    history_topology_work_budget_exceeded, HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY,
};

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

fn one_delta_state() -> Vec<u8> {
    let mut bytes = super::super::DELTA.to_vec();
    for (tag, value) in [
        (0x04, 1_i32),
        (0x04, 1),
        (0x04, 0),
        (0x0c, -1),
        (0x0c, -1),
        (0x0c, 0),
        (0x0c, -1),
        (0x0c, 0),
    ] {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0x0b, 0x11]);
    bytes
}

fn one_board_state() -> Vec<u8> {
    let mut bytes = one_delta_state();
    bytes.pop();
    for (tag, value) in [
        (0x04, 1_i32),
        (0x0c, 0),
        (0x04, 1),
        (0x04, 1),
        (0x0c, -1),
        (0x0c, 2),
        (0x04, 0),
        (0x04, 0),
    ] {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x11);
    bytes
}

fn history_id_lengths() -> (u64, u64, u64, u64) {
    let history = crate::test_support::with_decode_context(|ctx| {
        crate::ids::native_scoped_id(ctx, "history", "asm-history", format_args!("{:010}", 0))
            .expect("test F3D native identity")
    });
    let state = crate::test_support::with_decode_context(|ctx| {
        crate::ids::native_scoped_id(ctx, "history", "asm-delta-state", format_args!("{:010}", 0))
            .expect("test F3D native identity")
    });
    let board = crate::test_support::with_decode_context(|ctx| {
        crate::ids::native_scoped_id(ctx, "history", "asm-bulletin-board", "0000000000:000000")
            .expect("test F3D native identity")
    });
    let change = crate::test_support::with_decode_context(|ctx| {
        crate::ids::native_scoped_id(
            ctx,
            "history",
            "asm-entity-change",
            "0000000000:000000:000000",
        )
        .expect("test F3D native identity")
    });
    (
        u64_from_index(history.len()),
        u64_from_index(state.len()),
        u64_from_index(board.len()),
        u64_from_index(change.len()),
    )
}

fn history_record_with_limits(
    bytes: &[u8],
    policy: &cadmpeg_core::decode::DecodePolicy,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, policy).unwrap();
    super::super::decode_history_records(
        &ctx,
        bytes,
        0,
        None,
        "history",
        "state",
        cadmpeg_asm::kernel_header::RefWidth::Four,
    )
    .unwrap_err()
}

fn one_framed_history_record() -> Vec<u8> {
    let mut bytes = b"\x0d\x01x\x0c".to_vec();
    bytes.extend_from_slice(&3_i32.to_le_bytes());
    bytes.push(0x11);
    bytes
}

fn one_state_history() -> crate::history_records::AsmHistory {
    use crate::history_records::{AsmDeltaState, AsmHistory, AsmTopologyCache};

    AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "state".into(),
            parent: "history".into(),
            byte_offset: 0,
            state_id: 0,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 0,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: AsmTopologyCache::Absent,
            transition: None,
        }],
    }
}

fn one_archived_state() -> crate::history_records::AsmHistory {
    use crate::history_records::{AsmHistoryRecord, AsmHistoryRecordFraming};

    let mut history = one_state_history();
    history.states[0].records.push(AsmHistoryRecord {
        id: "record".into(),
        parent: "state".into(),
        revision_id: Some(1),
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

fn one_insert_only_state() -> crate::history_records::AsmHistory {
    use crate::history_records::{
        AsmBulletinBoard, AsmEntityChange, AsmEntityChangeKind, AsmHistoryRecord,
        AsmHistoryRecordFraming,
    };

    let mut history = one_state_history();
    history.states[0].records.push(AsmHistoryRecord {
        id: "boundary".into(),
        parent: "state".into(),
        revision_id: None,
        byte_offset: 0,
        framing: AsmHistoryRecordFraming::Framed {
            index: 0,
            name: "End-of-ASM-data".into(),
            entity_references: Vec::new(),
        },
        raw_bytes: vec![0x11],
    });
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
    history
}

fn one_archived_state_with_delete() -> crate::history_records::AsmHistory {
    use crate::history_records::{AsmBulletinBoard, AsmEntityChange, AsmEntityChangeKind};

    let mut history = one_archived_state();
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
            kind: AsmEntityChangeKind::Delete { old: 1 },
        }],
    });
    history
}

fn archive_record() -> cadmpeg_asm::sab::Record {
    cadmpeg_asm::sab::Record {
        index: 0,
        name: "edge".into(),
        tokens: vec![cadmpeg_asm::sab::Token::Ref(-1)].into(),
        offset: 0,
        len: 0,
    }
}

fn archive_error(max_items: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::historical_record_archive(&ctx, &[], &[archive_record()], Default::default())
        .unwrap_err()
}

#[test]
fn history_active_revision_index_refuses_collection_limit() {
    let error = archive_error(0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D active record revisions")
    );
}

#[test]
fn history_active_revision_index_refuses_materialized_limit() {
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "index F3D active record revisions",
        0,
        |ctx| {
            super::super::historical_record_archive(
                ctx,
                &[],
                &[archive_record()],
                Default::default(),
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D active record revisions"
    ));
}

#[test]
fn history_active_record_archive_refuses_materialized_limit() {
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "retain F3D active record archive",
        0,
        |ctx| {
            super::super::historical_record_archive(
                ctx,
                &[],
                &[archive_record()],
                Default::default(),
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3D active record archive"
    ));
}

#[test]
fn history_active_record_ordinals_refuse_work() {
    let operation = "validate F3D active record ordinals";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            super::super::historical_record_archive(
                ctx,
                &[],
                &[archive_record()],
                Default::default(),
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_active_record_archive_refuses_collection_limit() {
    let error = archive_error(1);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D active record archive")
    );
}

#[test]
fn history_archived_token_copy_refuses_collection_limit() {
    let error = archive_error(2);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D archived record tokens")
    );
}

#[test]
fn history_record_name_copy_refuses_retained_limit() {
    // The live archive reservation charges this copy as MaterializedBytes.
    // The name outgrows the temporary peak left by the revision index, so
    // the copy itself raises the materialized usage.
    let operation = "copy F3D historical record text";
    let record = cadmpeg_asm::sab::Record {
        name: "e".repeat(4096),
        ..archive_record()
    };
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| {
            super::super::historical_record_archive(
                ctx,
                &[],
                std::slice::from_ref(&record),
                Default::default(),
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_token_text_copy_refuses_retained_limit() {
    // The live archive reservation charges this copy as MaterializedBytes.
    let operation = "copy F3D historical record text";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| {
            let record = cadmpeg_asm::sab::Record {
                name: String::new(),
                tokens: vec![cadmpeg_asm::sab::Token::Str("x".into())].into(),
                ..archive_record()
            };
            super::super::historical_record_archive(ctx, &[], &[record], Default::default())
                .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_archived_token_copy_refuses_work_limit() {
    let operation = "copy F3D archived record tokens";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            super::super::historical_record_archive(
                ctx,
                &[],
                &[archive_record()],
                Default::default(),
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_archived_token_vector_refuses_materialized_limit() {
    let operation = "copy F3D archived record tokens";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| {
            super::super::historical_record_archive(
                ctx,
                &[],
                &[archive_record()],
                Default::default(),
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_archived_token_arc_copy_refuses_materialized_limit() {
    let operation = "copy F3D archived record tokens";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        operation,
        1,
        |ctx| {
            super::super::historical_record_archive(
                ctx,
                &[],
                &[archive_record()],
                Default::default(),
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

fn table_error(max_items: u64) -> cadmpeg_core::CodecError {
    use crate::history_records::AsmEntityVersion;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut history = one_state_history();
    history.states[0].entity_versions.push(AsmEntityVersion {
        entity_ref: 0,
        record_ref: 0,
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let archive = super::super::HistoricalRecordArchive {
        records: std::collections::BTreeMap::from([(0, archive_record())]),
        _storage: ctx
            .reserve_scoped(0, "retain F3D active record archive")
            .unwrap(),
        _token_storage: ctx
            .reserve_scoped(0, "copy F3D archived record tokens")
            .unwrap(),
    };
    super::super::materialize_record_table(&ctx, &history.states[0], &archive).unwrap_err()
}

fn table_materialized_error(operation: &'static str) -> cadmpeg_core::CodecError {
    use crate::history_records::AsmEntityVersion;
    use cadmpeg_core::decode::ResourceDimension;

    crate::test_support::resource_refusal_at(
        ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| {
            let mut history = one_state_history();
            history.states[0].entity_versions.push(AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            });
            let archive = super::super::HistoricalRecordArchive {
                records: std::collections::BTreeMap::from([(0, archive_record())]),
                _storage: ctx.reserve_scoped(0, "retain F3D active record archive")?,
                _token_storage: ctx.reserve_scoped(0, "copy F3D archived record tokens")?,
            };
            super::super::materialize_record_table(ctx, &history.states[0], &archive).map(|_| ())
        },
    )
}

#[test]
fn history_record_presence_refuses_collection_limit() {
    let error = table_error(0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical record presence")
    );
}

#[test]
fn history_record_presence_refuses_materialized_limit() {
    let error = table_materialized_error("index F3D historical record presence");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D historical record presence"
    ));
}

#[test]
fn history_record_table_refuses_materialized_limit() {
    let error = table_materialized_error("materialize F3D historical record table");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "materialize F3D historical record table"
    ));
}

#[test]
fn history_record_table_refuses_collection_limit() {
    let error = table_error(1);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "materialize F3D historical record table")
    );
}

#[test]
fn history_topology_slot_index_refuses_materialized_limit() {
    let topology = crate::history_records::AsmHistoricalTopology {
        bodies: vec![1],
        ..Default::default()
    };
    let operation = "index F3D historical topology slots";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| super::super::topology_entity_slots(ctx, &topology).map(|_| ()),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_topology_slot_index_refuses_work_limit() {
    let topology = crate::history_records::AsmHistoricalTopology {
        bodies: vec![1],
        ..Default::default()
    };
    let operation = "index F3D historical topology slots";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| super::super::topology_entity_slots(ctx, &topology).map(|_| ()),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_topology_slot_index_refuses_collection_limit() {
    use crate::history_records::AsmHistoricalTopology;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let topology = AsmHistoricalTopology {
        bodies: vec![1],
        faces: vec![2],
        ..Default::default()
    };
    let error = super::super::topology_entity_slots(&ctx, &topology).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical topology slots")
    );
}

/// The adaptive ceiling includes all `CollectionItems` charged before the selected operation.
fn transition_error(operation: &str) -> cadmpeg_core::CodecError {
    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        0,
        |ctx| {
            use crate::history_records::{
                AsmEntityVersion, AsmHistoricalTopology, AsmTopologyCache,
            };

            let mut history = one_state_history();
            history.states[0].entity_versions.push(AsmEntityVersion {
                entity_ref: 1,
                record_ref: 1,
            });
            history.states[0].topology_cache = AsmTopologyCache::Complete(AsmHistoricalTopology {
                bodies: vec![1],
                ..Default::default()
            });
            super::super::bind_historical_transitions(ctx, &mut history.states)
        },
    )
}

macro_rules! historical_transition_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let error = transition_error($operation);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
        }
    };
}

historical_transition_limit_test!(
    history_transition_node_index_refuses_limit,
    "index F3D transition nodes"
);
historical_transition_limit_test!(
    history_transition_vector_refuses_limit,
    "collect F3D historical transitions"
);
historical_transition_limit_test!(
    history_transition_version_map_refuses_limit,
    "index F3D transition versions"
);
historical_transition_limit_test!(
    history_transition_version_keys_refuses_limit,
    "collect F3D transition version keys"
);
historical_transition_limit_test!(
    history_transition_entity_index_refuses_limit,
    "copy F3D transition entities"
);
historical_transition_limit_test!(
    history_transition_delta_refuses_limit,
    "collect F3D transition delta"
);

#[test]
fn history_transition_node_index_refuses_scoped_storage_limit() {
    let operation = "index F3D transition nodes";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| {
            let mut history = one_state_history();
            super::super::bind_historical_transitions(ctx, &mut history.states)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_transition_node_source_refuses_work_limit() {
    let operation = "scan F3D transition nodes";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            let mut history = one_state_history();
            super::super::bind_historical_transitions(ctx, &mut history.states)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_archived_count_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let history = one_archived_state();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::archived_active_record_count(&ctx, &history.states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D archived revisions")
    );
}

#[test]
fn history_insert_only_count_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let history = one_insert_only_state();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::insert_only_active_record_count(&ctx, &history.states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D insert-only revisions")
    );
}

fn historical_versions_error(operation: &str, deletion: bool) -> cadmpeg_core::CodecError {
    use crate::history_records::{AsmBulletinBoard, AsmEntityChange, AsmEntityChangeKind};

    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        0,
        |ctx| {
            let mut history = one_archived_state();
            if deletion {
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
                        kind: AsmEntityChangeKind::Delete { old: 1 },
                    }],
                });
            }
            super::super::bind_historical_entity_versions(ctx, &mut history.states)
        },
    )
}

macro_rules! historical_versions_limit_test {
    ($name:ident, $deletion:expr, $operation:literal) => {
        #[test]
        fn $name() {
            let error = historical_versions_error($operation, $deletion);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
        }
    };
}

historical_versions_limit_test!(
    history_version_archive_index_refuses_limit,
    false,
    "index F3D archived revision IDs"
);
historical_versions_limit_test!(
    history_version_node_index_refuses_limit,
    false,
    "index F3D history node ordinals"
);

#[test]
fn history_version_node_index_refuses_materialized_limit() {
    let operation = "index F3D history node ordinals";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| {
            let mut history = one_archived_state();
            super::super::bind_historical_entity_versions(ctx, &mut history.states)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}
historical_versions_limit_test!(
    history_version_seed_refuses_limit,
    false,
    "seed F3D history versions"
);
historical_versions_limit_test!(
    history_version_state_vector_refuses_limit,
    false,
    "materialize F3D state versions"
);
historical_versions_limit_test!(
    history_version_projection_index_refuses_limit,
    false,
    "index F3D state version projections"
);
historical_versions_limit_test!(
    history_version_restore_refuses_limit,
    true,
    "restore F3D historical version"
);

#[test]
fn history_snapshot_old_references_refuse_collection_limit() {
    use crate::history_records::{AsmBulletinBoard, AsmEntityChange, AsmEntityChangeKind};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

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
            kind: AsmEntityChangeKind::Delete { old: 1 },
        }],
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::bind_snapshot_revision_ids(&ctx, &mut history.states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D ASM old references")
    );
}

#[test]
fn history_graph_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::graph_is_coherent_charged(&ctx, &one_state_history()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ASM history states")
    );
}

#[test]
fn history_record_references_refuse_collection_limit() {
    let bytes = one_framed_history_record();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let error = history_record_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "frame F3D history references")
    );
}

#[test]
fn history_record_vector_refuses_collection_limit() {
    let bytes = one_framed_history_record();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 4;
    let error = history_record_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "frame F3D history record")
    );
}

#[test]
fn history_record_id_refuses_retained_limit() {
    let bytes = one_framed_history_record();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D native record ID",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(history_record_with_limits(&bytes, &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = history_record_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID")
    );
}

#[test]
fn history_record_parent_refuses_retained_limit() {
    let bytes = one_framed_history_record();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();

    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D history record parent",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();

            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(history_record_with_limits(&bytes, &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = history_record_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D history record parent")
    );
}

#[test]
fn opaque_history_record_id_refuses_retained_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D native record ID",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(history_record_with_limits(&[0xff], &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = history_record_with_limits(&[0xff], &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID")
    );
}

#[test]
fn opaque_history_record_parent_refuses_retained_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();

    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy opaque F3D history record parent",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();

            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(history_record_with_limits(&[0xff], &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = history_record_with_limits(&[0xff], &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy opaque F3D history record parent")
    );
}

#[test]
fn opaque_history_error_refuses_retained_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();

    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain opaque F3D history error",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();

            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(history_record_with_limits(&[0xff], &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = history_record_with_limits(&[0xff], &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain opaque F3D history error")
    );
}

#[test]
fn history_board_id_refuses_retained_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let (history, state, _, _) = history_id_lengths();
    policy.limits.max_retained_bytes = history + state;
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID")
    );
}

#[test]
fn history_change_vector_refuses_collection_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "admit F3D ASM entity change")
    );
}

#[test]
fn history_change_id_refuses_retained_limit() {
    let bytes = one_board_state();
    let operation = "retain F3D native record ID";
    // The history, state and board identities precede the change identity.
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        operation,
        3,
        |ctx| {
            super::super::decode(
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

#[test]
fn history_change_parent_refuses_retained_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();

    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D ASM change parent",
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
        if limit.operation == "copy F3D ASM change parent")
    );
}

#[test]
fn history_bulletin_board_parser_loops_refuse_work() {
    let bytes = one_board_state();
    for (operation, skip) in [
        ("read F3D ASM bulletin board entry", 0),
        ("read F3D ASM bulletin board entry", 1),
        ("read F3D ASM entity change entry", 0),
        ("read F3D ASM entity change entry", 1),
    ] {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            skip,
            |ctx| {
                super::super::decode(
                    ctx,
                    &bytes,
                    "history",
                    cadmpeg_asm::kernel_header::RefWidth::Four,
                    &ctx.policy().limits,
                )
                .map(|_| ())
            },
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
        ));
    }
}

#[test]
fn history_board_vector_refuses_collection_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "admit F3D ASM bulletin board")
    );
}

#[test]
fn history_board_parent_refuses_retained_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();

    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D ASM board parent",
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
        if limit.operation == "copy F3D ASM board parent")
    );
}

fn decode_with_limits(
    bytes: &[u8],
    policy: &cadmpeg_core::decode::DecodePolicy,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, policy).unwrap();
    super::super::decode(
        &ctx,
        bytes,
        "history",
        cadmpeg_asm::kernel_header::RefWidth::Four,
        &policy.limits,
    )
    .unwrap_err()
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
    let bytes = [super::super::DELTA, super::super::DELTA].concat();
    let operation = "collect F3D ASM delta offsets";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        0,
        |ctx| {
            super::super::decode(
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
            super::super::decode(
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

#[test]
fn history_record_collection_preserves_framed_output() {
    let bytes = one_framed_history_record();
    let records = crate::test_support::with_decode_context(|ctx| {
        super::super::decode_history_records(
            ctx,
            &bytes,
            0,
            None,
            "history",
            "state",
            cadmpeg_asm::kernel_header::RefWidth::Four,
        )
    })
    .unwrap();
    let [record] = records.as_slice() else {
        panic!("expected one framed record");
    };
    assert_eq!(record.name(), "x");
    assert_eq!(record.parent, "state");
    assert_eq!(record.byte_offset, 0);
    assert_eq!(record.raw_bytes, bytes);
    assert!(matches!(
        &record.framing,
        crate::history_records::AsmHistoryRecordFraming::Framed {
            index: 0,
            entity_references,
            ..
        } if entity_references == &[3]
    ));
}

#[test]
fn history_record_collection_refuses_work_limit() {
    let operation = "frame F3D history record";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            let bytes = one_framed_history_record();
            super::super::decode_history_records(
                ctx,
                &bytes,
                0,
                None,
                "history",
                "state",
                cadmpeg_asm::kernel_header::RefWidth::Four,
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_record_reference_count_refuses_work_limit() {
    let operation = "count F3D history record references";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            let bytes = one_framed_history_record();
            super::super::decode_history_records(
                ctx,
                &bytes,
                0,
                None,
                "history",
                "state",
                cadmpeg_asm::kernel_header::RefWidth::Four,
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_binding_work_budget_charges_state_record_cross_product() {
    let desktop = cadmpeg_core::decode::ResourceLimits::desktop();
    let desktop_entries = desktop.max_work_units / HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY;
    assert!(!history_topology_work_budget_exceeded(
        [usize::try_from(desktop_entries).expect("desktop entry budget fits usize")],
        &desktop
    ));
    assert!(history_topology_work_budget_exceeded(
        [usize::try_from(desktop_entries + 1).expect("desktop entry overflow fits usize")],
        &desktop
    ));
    assert!(history_topology_work_budget_exceeded(
        [usize::MAX, 1],
        &desktop
    ));

    let service = cadmpeg_core::decode::ResourceLimits::service();
    let service_entries = service.max_work_units / HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY;
    assert!(!history_topology_work_budget_exceeded(
        [usize::try_from(service_entries).expect("service entry budget fits usize")],
        &service
    ));
    assert!(history_topology_work_budget_exceeded(
        [usize::try_from(service_entries + 1).expect("service entry overflow fits usize")],
        &service
    ));
}

#[test]
fn history_complete_table_binding_refuses_materialized_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = super::super::HISTORY_TOPOLOGY_CACHE_BYTES_PER_ENTRY - 1;
    let ctx = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap()
        .0;
    let error = super::super::admit_complete_table_binding_budget(
        &ctx,
        [1_usize].into_iter(),
        &policy.limits,
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref refusal)
        if refusal.operation == "bind F3D complete history topology bytes")
    );
}

#[test]
fn history_complete_table_binding_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY - 1;
    let ctx = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap()
        .0;
    let error = super::super::admit_complete_table_binding_budget(
        &ctx,
        [1_usize].into_iter(),
        &policy.limits,
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref refusal)
        if refusal.operation == "bind F3D complete history topology work")
    );
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
        |ctx| super::super::graph_is_coherent_charged(ctx, history),
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
        |ctx| super::super::graph_is_coherent_charged(ctx, &one_state_history()),
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
            super::super::history_state_reaches(decode, &history, state, state.state_id).map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_archived_revision_map_refuses_materialized_limit() {
    use crate::history_records::AsmEntityChangeKind;

    let mut history = one_insert_only_state();
    history.states[0].bulletin_boards[0].changes[0].kind = AsmEntityChangeKind::Delete { old: 1 };
    let archive = crate::test_support::with_decode_context(|ctx| {
        super::super::historical_record_archive(
            ctx,
            &history.states,
            &[],
            std::collections::BTreeMap::from([(1, archive_record())]),
        )
        .map(|archive| archive.is_some())
    })
    .unwrap();
    assert!(archive);
    let operation = "index F3D archived record revisions";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| {
            super::super::historical_record_archive(
                ctx,
                &history.states,
                &[],
                std::collections::BTreeMap::from([(1, archive_record())]),
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_archived_frame_map_refuses_materialized_limit() {
    let operation = "retain F3D archived record archive";
    let frames = std::collections::BTreeMap::from([(1, archive_record())]);
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| super::super::historical_record_archive(ctx, &[], &[], frames.clone()).map(|_| ()),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_version_projection_map_refuses_materialized_limit() {
    let operation = "index F3D state version projections";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| {
            let mut history = one_archived_state();
            super::super::bind_historical_entity_versions(ctx, &mut history.states)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_archived_revision_state_scan_refuses_work() {
    let operation = "scan F3D archived history states";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            let mut history = one_archived_state();
            super::super::bind_historical_entity_versions(ctx, &mut history.states)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_archived_revision_record_scan_refuses_work() {
    let operation = "scan F3D archived state records";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            let mut history = one_archived_state();
            super::super::bind_historical_entity_versions(ctx, &mut history.states)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_node_source_scan_refuses_work() {
    let operation = "scan F3D history node ordinals";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            let mut history = one_archived_state();
            super::super::bind_historical_entity_versions(ctx, &mut history.states)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_version_bulletin_board_scan_refuses_work() {
    let operation = "scan F3D historical version bulletin boards";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            let mut history = one_archived_state_with_delete();
            super::super::bind_historical_entity_versions(ctx, &mut history.states)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_version_bulletin_change_scan_refuses_work() {
    let operation = "scan F3D historical version changes";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            let mut history = one_archived_state_with_delete();
            super::super::bind_historical_entity_versions(ctx, &mut history.states)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_version_binding_loop_refuses_work() {
    let operation = "bind F3D historical entity versions";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            let mut history = one_archived_state();
            super::super::bind_historical_entity_versions(ctx, &mut history.states)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_version_state_vector_refuses_retained_limit() {
    let operation = "materialize F3D state versions";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        operation,
        0,
        |ctx| {
            let mut history = one_archived_state();
            super::super::bind_historical_entity_versions(ctx, &mut history.states)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}
