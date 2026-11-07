// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::sab::{Record, Token};

fn tagged(out: &mut Vec<u8>, tag: u8, value: i64) {
    out.push(tag);
    out.extend_from_slice(&value.to_le_bytes());
}

type Board<'a> = (i64, i64, &'a [(i64, i64)]);

fn chain(entries: &[Board<'_>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for &(owner, number, changes) in entries {
        tagged(&mut bytes, 0x04, 1);
        tagged(&mut bytes, 0x0c, owner);
        tagged(&mut bytes, 0x04, number);
        for &(old, new) in changes {
            tagged(&mut bytes, 0x04, 1);
            tagged(&mut bytes, 0x0c, old);
            tagged(&mut bytes, 0x0c, new);
        }
        tagged(&mut bytes, 0x04, 0);
    }
    tagged(&mut bytes, 0x04, 0);
    bytes
}

fn walk(bytes: &[u8]) -> Option<(Vec<BulletinBoardItem>, usize)> {
    let mut items = Vec::new();
    let end = walk_bulletin_boards(bytes, 0, RefWidth::Eight, |item| items.push(item))?;
    Some((items, end))
}

#[test]
fn walk_classifies_insert_delete_and_update() {
    let bytes = chain(&[(3, 7, &[(-1, 4), (5, -1), (6, 8)])]);
    let (items, end) = walk(&bytes).unwrap();
    assert_eq!(end, bytes.len());
    assert_eq!(
        items,
        [
            BulletinBoardItem::Board {
                offset: 0,
                owner_ref: 3,
                number: 7
            },
            BulletinBoardItem::Change {
                offset: 27,
                kind: EntityChange::Insert { new: 4 }
            },
            BulletinBoardItem::Change {
                offset: 54,
                kind: EntityChange::Delete { old: 5 }
            },
            BulletinBoardItem::Change {
                offset: 81,
                kind: EntityChange::Update { old: 6, new: 8 }
            },
        ]
    );
}

#[test]
fn walk_refuses_a_change_with_two_null_references() {
    assert!(walk(&chain(&[(0, 1, &[(-1, -1)])])).is_none());
}

#[test]
fn walk_refuses_a_truncated_chain() {
    let bytes = chain(&[(0, 1, &[(-1, 2)])]);
    assert!(walk(&bytes[..bytes.len() - 1]).is_none());
}

#[test]
fn walk_returns_an_immediate_record_delimiter_unchanged() {
    let mut items = 0;
    assert_eq!(
        walk_bulletin_boards(&[0x11], 0, RefWidth::Four, |_| items += 1),
        Some(0)
    );
    assert_eq!(items, 0);
}

#[test]
fn walk_reads_four_byte_references() {
    let mut bytes = Vec::new();
    for (tag, value) in [(0x04, 1_i32), (0x0c, 2), (0x04, 3), (0x04, 0), (0x04, 0)] {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut items = Vec::new();
    let end = walk_bulletin_boards(&bytes, 0, RefWidth::Four, |item| items.push(item));
    assert_eq!(end, Some(bytes.len()));
    assert_eq!(
        items,
        [BulletinBoardItem::Board {
            offset: 0,
            owner_ref: 2,
            number: 3
        }]
    );
}

fn record(index: usize, refs: &[i64]) -> Record {
    Record {
        index,
        name: "face".into(),
        tokens: refs
            .iter()
            .map(|&r| Token::Ref(r))
            .collect::<Vec<_>>()
            .into(),
        offset: 0,
        len: 0,
    }
}

fn materialize(versions: &[(i64, i64)], records: &[(i64, Record)]) -> Option<Vec<Record>> {
    let versions: BTreeMap<i64, i64> = versions.iter().copied().collect();
    materialize_record_table(&versions, |revision| {
        records
            .iter()
            .find(|(r, _)| *r == revision)
            .map(|(_, record)| record)
    })
}

#[test]
fn materialize_orders_records_by_index() {
    let records = [(10, record(1, &[0, -1])), (11, record(0, &[]))];
    let table = materialize(&[(1, 10), (0, 11)], &records).unwrap();
    let indices: Vec<_> = table.iter().map(|r| r.index).collect();
    assert_eq!(indices, [0, 1]);
}

#[test]
fn materialize_refuses_a_reference_outside_the_slot_set() {
    let records = [(0, record(0, &[5]))];
    assert!(materialize(&[(0, 0)], &records).is_none());
}

#[test]
fn materialize_refuses_an_index_that_differs_from_its_slot() {
    let records = [(0, record(1, &[]))];
    assert!(materialize(&[(0, 0)], &records).is_none());
}

#[test]
fn materialize_refuses_an_empty_map_and_a_missing_revision() {
    assert!(materialize(&[], &[]).is_none());
    assert!(materialize(&[(0, 9)], &[]).is_none());
}

fn state(
    node_index: i64,
    previous_ref: Option<i64>,
    next_ref: Option<i64>,
    changes: &[EntityChange],
) -> DeltaStateView<'static> {
    DeltaStateView {
        node_index,
        previous_ref,
        next_ref,
        changes: changes.to_vec(),
        records: Vec::new(),
    }
}

/// States in a single chain, head first, with node indices 0, 1, 2, ...
fn chain_of(changes: &[&[EntityChange]]) -> Vec<DeltaStateView<'static>> {
    let last = changes.len() - 1;
    changes
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let node = i64::try_from(i).unwrap();
            state(
                node,
                (i > 0).then_some(node - 1),
                (i < last).then_some(node + 1),
                c,
            )
        })
        .collect()
}

type VisitedMap = (usize, Vec<(i64, i64)>);

fn walk_states(
    states: &[DeltaStateView<'_>],
    active_count: i64,
    archived_ids: &[i64],
) -> (bool, Vec<VisitedMap>) {
    let mut seen = Vec::new();
    let closed = walk_entity_versions(states, active_count, archived_ids, |ordinal, versions| {
        seen.push((ordinal, versions.iter().map(|(&k, &v)| (k, v)).collect()));
    });
    (closed, seen)
}

#[test]
fn walk_closes_a_chain_that_removes_every_slot_but_zero() {
    let states = chain_of(&[
        &[EntityChange::Insert { new: 2 }],
        &[EntityChange::Insert { new: 1 }],
    ]);
    let (closed, seen) = walk_states(&states, 3, &[]);
    assert!(closed);
    assert_eq!(
        seen,
        [(0, vec![(0, 0), (1, 1), (2, 2)]), (1, vec![(0, 0), (1, 1)]),]
    );
}

#[test]
fn walk_visits_a_state_before_its_changes_apply() {
    let states = chain_of(&[&[EntityChange::Insert { new: 1 }]]);
    let (closed, seen) = walk_states(&states, 2, &[]);
    assert!(closed);
    assert_eq!(seen, [(0, vec![(0, 0), (1, 1)])]);
}

#[test]
fn walk_update_maps_the_new_slot_to_the_old_revision() {
    let states = chain_of(&[
        &[EntityChange::Update { old: 5, new: 1 }],
        &[EntityChange::Insert { new: 1 }],
    ]);
    let (closed, seen) = walk_states(&states, 2, &[5]);
    assert!(closed);
    assert_eq!(seen[1], (1, vec![(0, 0), (1, 5)]));
}

#[test]
fn walk_update_refuses_an_absent_new_slot_and_an_unarchived_old_revision() {
    let absent = chain_of(&[&[EntityChange::Update { old: 5, new: 7 }]]);
    assert!(!walk_states(&absent, 1, &[5]).0);
    let unarchived = chain_of(&[&[EntityChange::Update { old: 5, new: 1 }]]);
    assert!(!walk_states(&unarchived, 2, &[4]).0);
}

#[test]
fn walk_delete_restores_the_old_slot_to_itself() {
    let states = chain_of(&[
        &[EntityChange::Delete { old: 3 }],
        &[EntityChange::Insert { new: 3 }],
    ]);
    let (closed, seen) = walk_states(&states, 1, &[3]);
    assert!(closed);
    assert_eq!(seen[1], (1, vec![(0, 0), (3, 3)]));
}

#[test]
fn walk_delete_refuses_a_present_slot_and_an_unarchived_revision() {
    let present = chain_of(&[&[EntityChange::Delete { old: 0 }]]);
    assert!(!walk_states(&present, 1, &[0]).0);
    let unarchived = chain_of(&[&[EntityChange::Delete { old: 3 }]]);
    assert!(!walk_states(&unarchived, 1, &[4]).0);
}

#[test]
fn walk_insert_refuses_an_absent_slot() {
    let states = chain_of(&[&[EntityChange::Insert { new: 4 }]]);
    assert!(!walk_states(&states, 2, &[]).0);
}

#[test]
fn walk_refuses_duplicate_node_indices_without_visiting() {
    let states = vec![state(0, None, Some(0), &[]), state(0, Some(0), None, &[])];
    let (closed, seen) = walk_states(&states, 1, &[]);
    assert!(!closed);
    assert!(seen.is_empty());
}

#[test]
fn walk_requires_exactly_one_head() {
    let no_head = vec![
        state(0, Some(1), Some(1), &[]),
        state(1, Some(0), Some(0), &[]),
    ];
    assert!(!walk_states(&no_head, 1, &[]).0);
    let two_heads = vec![state(0, None, None, &[]), state(1, None, None, &[])];
    let (closed, seen) = walk_states(&two_heads, 1, &[]);
    assert!(!closed);
    assert!(seen.is_empty());
}

#[test]
fn walk_refuses_a_revisited_node() {
    let states = vec![
        state(0, None, Some(1), &[]),
        state(1, Some(0), Some(0), &[]),
    ];
    let (closed, seen) = walk_states(&states, 1, &[]);
    assert!(!closed);
    assert_eq!(seen.len(), 2);
}

#[test]
fn walk_refuses_an_unreached_state_and_a_dangling_next_ref() {
    let unreached = vec![state(0, None, None, &[]), state(1, Some(0), None, &[])];
    assert!(!walk_states(&unreached, 1, &[]).0);
    let dangling = vec![state(0, None, Some(9), &[])];
    assert!(!walk_states(&dangling, 1, &[]).0);
}

#[test]
fn walk_refuses_a_final_map_other_than_zero_to_zero() {
    let unreduced = chain_of(&[&[]]);
    let (closed, seen) = walk_states(&unreduced, 2, &[]);
    assert!(!closed);
    assert_eq!(seen.len(), 1);
    let wrong_slot = chain_of(&[&[EntityChange::Insert { new: 0 }]]);
    assert!(!walk_states(&wrong_slot, 2, &[]).0);
    let remapped = chain_of(&[&[EntityChange::Update { old: 5, new: 0 }]]);
    assert!(!walk_states(&remapped, 1, &[5]).0);
}

fn archived<'a>(
    revision_id: i64,
    byte_offset: u64,
    raw: &'a [u8],
    name: &'a str,
) -> StateRecordView<'a> {
    StateRecordView {
        revision_id: Some(revision_id),
        byte_offset,
        raw_bytes: raw,
        name,
    }
}

fn boundary(name: &'static str) -> StateRecordView<'static> {
    StateRecordView {
        revision_id: None,
        byte_offset: 0,
        raw_bytes: &[],
        name,
    }
}

fn with_records<'a>(
    mut view: DeltaStateView<'a>,
    records: Vec<StateRecordView<'a>>,
) -> DeltaStateView<'a> {
    view.records = records;
    view
}

#[test]
fn archived_count_is_the_first_of_a_run_of_revision_ids() {
    let states = [with_records(
        state(0, None, None, &[]),
        vec![archived(4, 0, &[], "a"), archived(3, 0, &[], "b")],
    )];
    assert_eq!(archived_active_record_count(&states).unwrap(), Some(3));
    let gap = [with_records(
        state(0, None, None, &[]),
        vec![archived(3, 0, &[], "a"), archived(5, 0, &[], "b")],
    )];
    assert_eq!(archived_active_record_count(&gap).unwrap(), None);
    let zero = [with_records(
        state(0, None, None, &[]),
        vec![archived(0, 0, &[], "a")],
    )];
    assert_eq!(archived_active_record_count(&zero).unwrap(), None);
    assert_eq!(archived_active_record_count(&[]).unwrap(), None);
}

#[test]
fn insert_only_count_covers_every_slot_once() {
    let covering = [with_records(
        state(
            0,
            None,
            None,
            &[
                EntityChange::Insert { new: 2 },
                EntityChange::Insert { new: 1 },
            ],
        ),
        vec![boundary("End-of-ASM-data")],
    )];
    assert_eq!(insert_only_active_record_count(&covering), Some(3));
    let gap = [with_records(
        state(0, None, None, &[EntityChange::Insert { new: 2 }]),
        vec![boundary("End-of-ASM-History-Section")],
    )];
    assert_eq!(insert_only_active_record_count(&gap), None);
    let repeated = [with_records(
        state(
            0,
            None,
            None,
            &[
                EntityChange::Insert { new: 1 },
                EntityChange::Insert { new: 1 },
            ],
        ),
        vec![boundary("End-of-ASM-data")],
    )];
    assert_eq!(insert_only_active_record_count(&repeated), None);
    let update = [with_records(
        state(0, None, None, &[EntityChange::Update { old: 1, new: 1 }]),
        vec![boundary("End-of-ASM-data")],
    )];
    assert_eq!(insert_only_active_record_count(&update), None);
    let other_record = [with_records(
        state(0, None, None, &[EntityChange::Insert { new: 1 }]),
        vec![boundary("face")],
    )];
    assert_eq!(insert_only_active_record_count(&other_record), None);
    let no_record = [state(0, None, None, &[EntityChange::Insert { new: 1 }])];
    assert_eq!(insert_only_active_record_count(&no_record), None);
}

fn record_bytes(name: &str) -> Vec<u8> {
    let mut out = vec![0x0d, u8::try_from(name.len()).unwrap()];
    out.extend_from_slice(name.as_bytes());
    out.push(0x11);
    out
}

/// A `BinaryFile8` stream with an empty history partition flag, and the
/// `(offset, length)` of each record after the header.
fn stream(names: &[&str]) -> (Vec<u8>, Vec<(usize, usize)>) {
    let mut bytes = b"ASM BinaryFile8".to_vec();
    bytes.resize(crate::layout::asmheader_binaryfile8::LEN, 0);
    for _ in 0..3 {
        bytes.extend_from_slice(&[0x07, 0]);
    }
    for _ in 0..3 {
        bytes.push(0x06);
        bytes.extend_from_slice(&1.0f64.to_le_bytes());
    }
    let mut extents = Vec::new();
    for name in names {
        let record = record_bytes(name);
        extents.push((bytes.len(), record.len()));
        bytes.extend_from_slice(&record);
    }
    (bytes, extents)
}

fn archive(
    states: &[DeltaStateView<'_>],
    bytes: &[u8],
) -> Result<Option<RecordArchiveFrames>, CodecError> {
    crate::test_support::with_service_context(bytes, |ctx| {
        decode_record_archive(ctx, states, bytes, RefWidth::Eight)
    })?
}

/// Two active records, then archived records with revisions 2 and 3.
fn archived_fixture() -> (Vec<u8>, Vec<(usize, usize)>) {
    stream(&["face", "edge", "loop", "coedge"])
}

fn archived_states<'a>(
    bytes: &'a [u8],
    extents: &[(usize, usize)],
    names: [&'a str; 2],
) -> Vec<DeltaStateView<'a>> {
    let records = (0..2)
        .map(|i| {
            let (offset, len) = extents[2 + i];
            archived(
                2 + i64::try_from(i).unwrap(),
                u64::try_from(offset).unwrap(),
                &bytes[offset..offset + len],
                names[i],
            )
        })
        .collect();
    vec![with_records(state(0, None, None, &[]), records)]
}

#[test]
fn archive_frames_active_and_archived_records() {
    let (bytes, extents) = archived_fixture();
    let states = archived_states(&bytes, &extents, ["loop", "coedge"]);
    let frames = archive(&states, &bytes).unwrap().unwrap();
    let active: Vec<_> = frames
        .active
        .iter()
        .map(|r| (r.index, r.name.as_str()))
        .collect();
    assert_eq!(active, [(0, "face"), (1, "edge")]);
    assert_eq!(frames.archived.len(), 2);
    assert_eq!(frames.archived[&2].name, "loop");
    assert_eq!(frames.archived[&3].name, "coedge");
}

#[test]
fn archive_frames_an_insert_only_history_of_exactly_the_active_count() {
    let (bytes, _) = stream(&["face", "edge", "loop"]);
    let states = [with_records(
        state(
            0,
            None,
            None,
            &[
                EntityChange::Insert { new: 1 },
                EntityChange::Insert { new: 2 },
            ],
        ),
        vec![boundary("End-of-ASM-data")],
    )];
    let frames = archive(&states, &bytes).unwrap().unwrap();
    assert_eq!(frames.active.len(), 3);
    assert!(frames.archived.is_empty());
    let (longer, _) = stream(&["face", "edge", "loop", "coedge"]);
    assert!(archive(&states, &longer).unwrap().is_none());
}

#[test]
fn archive_refuses_a_stream_without_a_record_start() {
    let states = [state(0, None, None, &[])];
    assert!(archive(&states, b"not an ASM stream").unwrap().is_none());
}

#[test]
fn archive_refuses_a_history_with_no_active_count() {
    let (bytes, _) = stream(&["face", "edge"]);
    assert!(archive(&[state(0, None, None, &[])], &bytes)
        .unwrap()
        .is_none());
}

#[test]
fn archive_refuses_a_stream_that_does_not_frame() {
    let (mut bytes, extents) = archived_fixture();
    let states_bytes = bytes.clone();
    let states = archived_states(&states_bytes, &extents, ["loop", "coedge"]);
    bytes.extend_from_slice(&[0x20, 0x20]);
    assert!(archive(&states, &bytes).unwrap().is_none());
}

#[test]
fn archive_refuses_fewer_framed_records_than_the_active_count() {
    let (bytes, _) = stream(&["face", "edge"]);
    let raw = record_bytes("loop");
    let states = [with_records(
        state(0, None, None, &[]),
        vec![archived(5, 0, &raw, "loop")],
    )];
    assert!(archive(&states, &bytes).unwrap().is_none());
}

#[test]
fn archive_refuses_raw_bytes_that_differ_from_the_stream() {
    let (bytes, extents) = archived_fixture();
    let mut states = archived_states(&bytes, &extents, ["loop", "coedge"]);
    let other = record_bytes("lump");
    states[0].records[0].raw_bytes = &other;
    assert!(archive(&states, &bytes).unwrap().is_none());
    let (offset, _) = extents[2];
    states[0].records[0].raw_bytes = &bytes[offset..offset + 3];
    assert!(archive(&states, &bytes).unwrap().is_none());
}

#[test]
fn archive_refuses_an_offset_beyond_the_stream() {
    let (bytes, extents) = archived_fixture();
    let mut states = archived_states(&bytes, &extents, ["loop", "coedge"]);
    states[0].records[0].byte_offset = u64::MAX;
    assert!(archive(&states, &bytes).unwrap().is_none());
}

#[test]
fn archive_refuses_a_record_name_that_differs() {
    let (bytes, extents) = archived_fixture();
    let states = archived_states(&bytes, &extents, ["loop", "vertex"]);
    assert!(archive(&states, &bytes).unwrap().is_none());
}

#[test]
fn archive_refuses_raw_bytes_that_frame_to_more_than_one_record() {
    let (bytes, extents) = archived_fixture();
    let (offset, len) = extents[2];
    let both = &bytes[offset..offset + len + extents[3].1];
    let states = [with_records(
        state(0, None, None, &[]),
        vec![
            archived(2, u64::try_from(offset).unwrap(), both, "loop"),
            archived(
                3,
                u64::try_from(extents[3].0).unwrap(),
                &bytes[extents[3].0..extents[3].0 + extents[3].1],
                "coedge",
            ),
        ],
    )];
    assert!(archive(&states, &bytes).unwrap().is_none());
}

#[test]
fn archive_refuses_a_repeated_revision_id() {
    let (bytes, extents) = archived_fixture();
    let mut states = archived_states(&bytes, &extents, ["loop", "coedge"]);
    states[0].records[1].revision_id = Some(2);
    assert!(archive(&states, &bytes).unwrap().is_none());
}
