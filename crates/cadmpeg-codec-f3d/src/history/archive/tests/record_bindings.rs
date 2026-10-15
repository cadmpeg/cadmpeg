// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_core::decode::u64_from_index;

use crate::history::archive::bind_historical_entity_versions;
use crate::history::archive::bind_snapshot_revision_ids;

use crate::history::archive::historical_record_archive;
use crate::history::archive::historical_transition;
use crate::history::archive::insert_only_active_record_count;
use crate::history::archive::materialize_record_table;

use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmEntityVersion,
    AsmHistoricalTopology, AsmHistoryRecord,
};

use crate::history::test_support::with_history_decode_context;

#[test]
fn historical_transition_separates_membership_and_revision_changes() {
    let state = |state_id, versions: &[(i64, i64)], topology| AsmDeltaState {
        id: format!("state-{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: versions
            .iter()
            .map(|&(entity_ref, record_ref)| AsmEntityVersion {
                entity_ref,
                record_ref,
            })
            .collect(),
        topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
        transition: None,
    };
    let previous = state(
        10,
        &[(1, 10), (4, 40), (8, 80)],
        AsmHistoricalTopology {
            bodies: vec![1],
            faces: vec![4],
            edges: vec![8],
            ..AsmHistoricalTopology::default()
        },
    );
    let current = state(
        11,
        &[(1, 11), (2, 2), (4, 40), (7, 70)],
        AsmHistoricalTopology {
            bodies: vec![1, 2],
            faces: vec![4],
            edges: vec![7],
            ..AsmHistoricalTopology::default()
        },
    );

    let transition = with_history_decode_context(|ctx| {
        historical_transition(ctx, &current, Some(&previous))
            .unwrap()
            .unwrap()
    });
    assert_eq!(transition.previous_state_id, Some(10));
    assert_eq!(transition.topology.bodies.inserted, [2]);
    assert_eq!(transition.topology.bodies.updated, [1]);
    assert!(transition.topology.faces.updated.is_empty());
    assert_eq!(transition.topology.edges.inserted, [7]);
    assert_eq!(transition.topology.edges.deleted, [8]);
    assert_eq!(transition.records.updated, [1]);
}

#[test]
fn snapshot_ordinals_bind_the_sorted_revision_interval() {
    let history_id = "history".to_string();
    let state_id = "state".to_string();
    let board_id = "board".to_string();
    let mut state = AsmDeltaState {
        id: state_id.clone(),
        parent: history_id,
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: vec![AsmBulletinBoard {
            id: board_id.clone(),
            parent: state_id.clone(),
            byte_offset: 0,
            owner_ref: 0,
            number: 2,
            changes: [7, 5, 6]
                .into_iter()
                .enumerate()
                .map(|(index, old_ref)| AsmEntityChange {
                    id: format!("change-{index}"),
                    parent: board_id.clone(),
                    byte_offset: u64_from_index(index),
                    kind: AsmEntityChangeKind::Update {
                        old: old_ref,
                        new: i64::try_from(index).expect("fixture value fits i64"),
                    },
                })
                .collect(),
        }],
        records: (0..3)
            .map(|index| AsmHistoryRecord {
                id: format!("record-{index}"),
                parent: state_id.clone(),
                revision_id: None,
                byte_offset: index,
                framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                    index,
                    name: "edge".into(),
                    entity_references: Vec::new(),
                },
                raw_bytes: vec![0x11],
            })
            .collect(),
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: None,
    };

    with_history_decode_context(|ctx| {
        bind_snapshot_revision_ids(ctx, std::slice::from_mut(&mut state)).unwrap()
    });

    assert_eq!(
        state
            .records
            .iter()
            .map(|record| record.revision_id)
            .collect::<Vec<_>>(),
        [Some(5), Some(6), Some(7)]
    );
}

fn snapshot_revision_scan_state() -> AsmDeltaState {
    let state_id = "snapshot-state".to_string();
    let board_id = "snapshot-board".to_string();
    AsmDeltaState {
        id: state_id.clone(),
        parent: "snapshot-history".into(),
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: vec![AsmBulletinBoard {
            id: board_id.clone(),
            parent: state_id.clone(),
            byte_offset: 0,
            owner_ref: 0,
            number: 1,
            changes: vec![AsmEntityChange {
                id: "snapshot-change".into(),
                parent: board_id,
                byte_offset: 0,
                kind: AsmEntityChangeKind::Delete { old: 1 },
            }],
        }],
        records: vec![AsmHistoryRecord {
            id: "snapshot-record".into(),
            parent: state_id,
            revision_id: None,
            byte_offset: 0,
            framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                index: 0,
                name: "edge".into(),
                entity_references: Vec::new(),
            },
            raw_bytes: vec![0x11],
        }],
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: None,
    }
}

#[test]
fn snapshot_revision_source_scans_refuse_work() {
    for operation in [
        "scan F3D snapshot reference states",
        "scan F3D snapshot reference boards",
        "scan F3D snapshot reference changes",
        "scan F3D snapshot record states",
        "scan F3D snapshot records",
    ] {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| {
                let mut state = snapshot_revision_scan_state();
                bind_snapshot_revision_ids(ctx, std::slice::from_mut(&mut state))
            },
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
        ));
    }
}

#[test]
fn insert_only_history_uses_the_active_record_table_as_revisions() {
    let state = |node_index, next_ref, inserted: &[i64]| {
        let state_id = format!("state-{node_index}");
        let board_id = format!("board-{node_index}");
        AsmDeltaState {
            id: state_id.clone(),
            parent: "history".into(),
            byte_offset: u64::try_from(node_index).expect("fixture reference is nonnegative"),
            state_id: 10 - node_index,
            version_flag: 1,
            state_flag: 0,
            previous_ref: (node_index > 0).then_some(node_index - 1),
            next_ref,
            node_index,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: vec![AsmBulletinBoard {
                id: board_id.clone(),
                parent: state_id.clone(),
                byte_offset: u64::try_from(node_index).expect("fixture reference is nonnegative"),
                owner_ref: 0,
                number: 2,
                changes: inserted
                    .iter()
                    .enumerate()
                    .map(|(index, new_ref)| AsmEntityChange {
                        id: format!("change-{node_index}-{index}"),
                        parent: board_id.clone(),
                        byte_offset: u64_from_index(index),
                        kind: AsmEntityChangeKind::Insert { new: *new_ref },
                    })
                    .collect(),
            }],
            records: vec![AsmHistoryRecord {
                id: format!("record-{node_index}"),
                parent: state_id,
                revision_id: None,
                byte_offset: u64::try_from(node_index).expect("fixture reference is nonnegative"),
                framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                    index: 0,
                    name: "End-of-ASM-History-Section".into(),
                    entity_references: Vec::new(),
                },
                raw_bytes: vec![0x11],
            }],
            entity_versions: Vec::new(),
            topology_cache: crate::history_records::AsmTopologyCache::Absent,
            transition: None,
        }
    };
    let mut states = vec![
        state(0, Some(1), &[1]),
        state(1, Some(2), &[2]),
        state(2, None, &[3]),
    ];

    with_history_decode_context(|ctx| {
        assert_eq!(
            insert_only_active_record_count(ctx, &states).unwrap(),
            Some(4)
        );
        bind_historical_entity_versions(ctx, &mut states).unwrap();
    });

    assert_eq!(
        states
            .iter()
            .map(|state| state.entity_versions.len())
            .collect::<Vec<_>>(),
        [4, 3, 2]
    );
    assert_eq!(
        states[1].entity_versions,
        [
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 2,
                record_ref: 2,
            },
            AsmEntityVersion {
                entity_ref: 3,
                record_ref: 3,
            },
        ]
    );
}

#[test]
fn insert_only_history_rejects_gaps_and_updates() {
    let mut state = AsmDeltaState {
        id: "state".into(),
        parent: "history".into(),
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: vec![AsmHistoryRecord {
            id: "record".into(),
            parent: "state".into(),
            revision_id: None,
            byte_offset: 0,
            framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                index: 0,
                name: "End-of-ASM-History-Section".into(),
                entity_references: Vec::new(),
            },
            raw_bytes: vec![0x11],
        }],
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: None,
    };
    let board = AsmBulletinBoard {
        id: "board".into(),
        parent: state.id.clone(),
        byte_offset: 0,
        owner_ref: 0,
        number: 2,
        changes: vec![
            AsmEntityChange {
                id: "gap-a".into(),
                parent: "board".into(),
                byte_offset: 0,
                kind: AsmEntityChangeKind::Insert { new: 1 },
            },
            AsmEntityChange {
                id: "gap-b".into(),
                parent: "board".into(),
                byte_offset: 0,
                kind: AsmEntityChangeKind::Insert { new: 3 },
            },
        ],
    };
    state.bulletin_boards.push(board);
    with_history_decode_context(|ctx| {
        assert_eq!(
            insert_only_active_record_count(ctx, &[state.clone()]).unwrap(),
            None
        )
    });
    state.bulletin_boards[0].changes[1].kind = AsmEntityChangeKind::Update { old: 2, new: 3 };
    with_history_decode_context(|ctx| {
        assert_eq!(
            insert_only_active_record_count(ctx, &[state]).unwrap(),
            None
        )
    });
}

#[test]
fn materialized_record_table_normalizes_revision_references() {
    let mut archived_bytes = vec![0x0d, 4];
    archived_bytes.extend_from_slice(b"edge");
    archived_bytes.push(0x0c);
    archived_bytes.extend_from_slice(&2i64.to_le_bytes());
    archived_bytes.push(0x11);
    let state_id = "state".to_string();
    let board_id = "board".to_string();
    let state = AsmDeltaState {
        id: state_id.clone(),
        parent: "history".into(),
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: vec![AsmBulletinBoard {
            id: board_id.clone(),
            parent: state_id.clone(),
            byte_offset: 0,
            owner_ref: 0,
            number: 2,
            changes: vec![AsmEntityChange {
                id: "change".into(),
                parent: board_id,
                byte_offset: 0,
                kind: AsmEntityChangeKind::Update { old: 2, new: 1 },
            }],
        }],
        records: vec![AsmHistoryRecord {
            id: "record".into(),
            parent: state_id,
            revision_id: Some(2),
            byte_offset: 0,
            framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                index: 0,
                name: "edge".into(),
                entity_references: vec![2],
            },
            raw_bytes: archived_bytes.clone(),
        }],
        entity_versions: vec![
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 1,
                record_ref: 2,
            },
        ],
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: None,
    };
    let active = ["asmheader", "edge"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| cadmpeg_asm::sab::Record {
            index,
            name: name.into(),

            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        })
        .collect::<Vec<_>>();

    let [framed]: [_; 1] = cadmpeg_asm::test_support::sab::frame(
        &archived_bytes,
        0,
        archived_bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("archived record frames")
    .try_into()
    .expect("one archived record");
    with_history_decode_context(|ctx| {
        let archive = historical_record_archive(
            ctx,
            std::slice::from_ref(&state),
            &active,
            std::collections::BTreeMap::from([(2, framed)]),
        )
        .expect("history archive budget")
        .expect("complete historical record archive");
        let table = materialize_record_table(ctx, &state, &archive)
            .expect("historical table budget")
            .expect("complete historical RecordTable");

        assert_eq!(table.records.len(), 2);
        assert_eq!(table.records[1].index, 1);
        assert_eq!(&*table.records[1].tokens, [cadmpeg_asm::sab::Token::Ref(1)]);
    });
}

#[test]
fn qualified_history_marker_remains_an_archived_record() {
    let mut archived_bytes = Vec::new();
    for part in ["End", "of", "ASM", "History"] {
        archived_bytes.extend_from_slice(&[0x0e, u8::try_from(part.len()).unwrap()]);
        archived_bytes.extend_from_slice(part.as_bytes());
    }
    archived_bytes.extend_from_slice(&[0x0d, 7]);
    archived_bytes.extend_from_slice(b"Section");
    archived_bytes.extend_from_slice(&[0x0d, 4]);
    archived_bytes.extend_from_slice(b"body");
    archived_bytes.push(0x0c);
    archived_bytes.extend_from_slice(&2i64.to_le_bytes());
    archived_bytes.push(0x11);
    let state_id = "state".to_string();
    let board_id = "board".to_string();
    let state = AsmDeltaState {
        id: state_id.clone(),
        parent: "history".into(),
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: vec![AsmBulletinBoard {
            id: board_id.clone(),
            parent: state_id.clone(),
            byte_offset: 0,
            owner_ref: 0,
            number: 2,
            changes: vec![AsmEntityChange {
                id: "change".into(),
                parent: board_id,
                byte_offset: 0,
                kind: AsmEntityChangeKind::Update { old: 2, new: 1 },
            }],
        }],
        records: vec![AsmHistoryRecord {
            id: "record".into(),
            parent: state_id,
            revision_id: Some(2),
            byte_offset: 0,
            framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                index: 0,
                name: "End-of-ASM-History-Section".into(),
                entity_references: vec![2],
            },
            raw_bytes: archived_bytes.clone(),
        }],
        entity_versions: vec![
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 1,
                record_ref: 2,
            },
        ],
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: None,
    };
    let active = ["asmheader", "body"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| cadmpeg_asm::sab::Record {
            index,
            name: name.into(),

            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        })
        .collect::<Vec<_>>();

    let [framed]: [_; 1] = cadmpeg_asm::test_support::sab::frame(
        &archived_bytes,
        0,
        archived_bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("archived record frames")
    .try_into()
    .expect("one archived record");
    with_history_decode_context(|ctx| {
        let archive = historical_record_archive(
            ctx,
            std::slice::from_ref(&state),
            &active,
            std::collections::BTreeMap::from([(2, framed)]),
        )
        .expect("history archive budget")
        .expect("qualified history marker is an archived record");
        let record = archive
            .records
            .get(&2)
            .expect("marker revision is retained");
        assert_eq!(record.name, "End-of-ASM-History-Section");
        assert_eq!(record.index, 1);
        assert!(record.tokens.contains(&cadmpeg_asm::sab::Token::Ref(1)));
    });
}

fn reverse_history_state_fixture() -> Vec<AsmDeltaState> {
    let state = |node_index, previous_ref, next_ref, old_ref, new_ref| {
        let board_id = format!("board-{node_index}");
        AsmDeltaState {
            id: format!("state-{node_index}"),
            parent: "history".into(),
            byte_offset: u64::try_from(node_index).expect("fixture reference is nonnegative"),
            state_id: 10 - node_index,
            version_flag: 1,
            state_flag: 0,
            previous_ref,
            next_ref,
            node_index,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: vec![AsmBulletinBoard {
                id: board_id.clone(),
                parent: format!("state-{node_index}"),
                byte_offset: u64::try_from(node_index).expect("fixture reference is nonnegative"),
                owner_ref: 0,
                number: 2,
                changes: vec![AsmEntityChange {
                    id: format!("change-{node_index}"),
                    parent: board_id,
                    byte_offset: u64::try_from(node_index)
                        .expect("fixture reference is nonnegative"),
                    kind: match (old_ref, new_ref) {
                        (Some(old), Some(new)) => AsmEntityChangeKind::Update { old, new },
                        (None, Some(new)) => AsmEntityChangeKind::Insert { new },
                        (Some(old), None) => AsmEntityChangeKind::Delete { old },
                        (None, None) => unreachable!(),
                    },
                }],
            }],
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: crate::history_records::AsmTopologyCache::Absent,
            transition: None,
        }
    };
    let mut states = vec![
        state(0, None, Some(1), Some(3), Some(1)),
        state(1, Some(0), Some(2), Some(4), Some(1)),
        state(2, Some(1), Some(3), None, Some(2)),
        state(3, Some(2), None, None, Some(1)),
    ];
    states[0].records = [3, 4]
        .map(|revision_id| AsmHistoryRecord {
            id: format!("record-{revision_id}"),
            parent: states[0].id.clone(),
            revision_id: Some(revision_id),
            byte_offset: 0,
            framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                index: u64::try_from(revision_id).expect("fixture reference is nonnegative") - 3,
                name: "edge".into(),
                entity_references: Vec::new(),
            },
            raw_bytes: vec![0x11],
        })
        .into();

    states
}

fn reverse_history_delete_fixture() -> Vec<AsmDeltaState> {
    let mut states = reverse_history_state_fixture();
    states[0].bulletin_boards[0].changes[0].kind = AsmEntityChangeKind::Delete { old: 4 };
    states
}

#[test]
fn reverse_history_builds_complete_entity_version_maps() {
    let mut states = reverse_history_state_fixture();

    with_history_decode_context(|ctx| bind_historical_entity_versions(ctx, &mut states).unwrap());

    assert_eq!(
        states
            .iter()
            .map(|state| state.entity_versions.len())
            .collect::<Vec<_>>(),
        [3, 3, 3, 2]
    );
    assert_eq!(
        states[1].entity_versions,
        [
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 1,
                record_ref: 3,
            },
            AsmEntityVersion {
                entity_ref: 2,
                record_ref: 2,
            },
        ]
    );
    assert_eq!(states[2].entity_versions[1].record_ref, 4);
}

#[test]
fn reverse_history_update_revision_search_propagates_work_refusal() {
    use cadmpeg_core::decode::ResourceDimension;

    let operation = "find archived F3D revision for update";
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| bind_historical_entity_versions(ctx, &mut reverse_history_state_fixture()),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn reverse_history_delete_revision_search_propagates_work_refusal() {
    use cadmpeg_core::decode::ResourceDimension;

    let operation = "find archived F3D revision for delete";
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| bind_historical_entity_versions(ctx, &mut reverse_history_delete_fixture()),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn reverse_history_update_key_comparison_propagates_work_refusal() {
    use cadmpeg_core::decode::ResourceDimension;

    let operation = "update F3D historical version";
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| bind_historical_entity_versions(ctx, &mut reverse_history_state_fixture()),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn reverse_history_delete_insertion_propagates_collection_refusal() {
    use cadmpeg_core::decode::ResourceDimension;

    let operation = "restore F3D historical version";
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::CollectionItems,
        operation,
        0,
        |ctx| bind_historical_entity_versions(ctx, &mut reverse_history_delete_fixture()),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}
