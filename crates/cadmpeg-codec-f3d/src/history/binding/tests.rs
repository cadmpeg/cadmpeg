// SPDX-License-Identifier: Apache-2.0
use super::{decode_slots, Binder, HistoricalRecordArchive, Slots, Statistics};
use cadmpeg_asm::sab::{Record, Token};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::collections::BTreeMap;

fn record(slot: i64, name: &str, tokens: Vec<Token>) -> Record {
    Record {
        index: usize::try_from(slot).unwrap(),
        name: name.into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    }
}

fn refs(slot: i64, name: &str, fields: &[(usize, i64)]) -> Record {
    let mut tokens = vec![Token::Ref(-1); 12];
    for &(field, reference) in fields {
        tokens[field] = Token::Ref(reference);
    }
    record(slot, name, tokens)
}

fn fixture() -> BTreeMap<i64, Record> {
    let mut records = vec![
        refs(0, "asmheader", &[]),
        refs(1, "body", &[(3, 2)]),
        refs(2, "lump", &[(4, 3), (5, 1)]),
        refs(3, "shell", &[(5, 10), (7, 2)]),
    ];
    for (base, next, partner) in [(10, 20, 23), (20, -1, 13)] {
        records.extend([
            refs(
                base,
                "face",
                &[(3, next), (4, base + 2), (5, 3), (7, base + 1)],
            ),
            record(
                base + 1,
                "plane-surface",
                vec![
                    Token::Ref(-1),
                    Token::Position([0.0, 0.0, 0.0]),
                    Token::Vector3([0.0, 0.0, 1.0]),
                    Token::Vector3([1.0, 0.0, 0.0]),
                ],
            ),
            refs(base + 2, "loop", &[(4, base + 3), (5, base)]),
            refs(
                base + 3,
                "coedge",
                &[
                    (3, base + 3),
                    (4, base + 3),
                    (5, partner),
                    (6, base + 4),
                    (8, base + 2),
                ],
            ),
            refs(
                base + 4,
                "edge",
                &[(3, base + 5), (5, base + 5), (7, base + 3), (8, base + 7)],
            ),
            refs(base + 5, "vertex", &[(3, base + 4), (4, base + 6)]),
            record(
                base + 6,
                "point",
                vec![Token::Ref(-1), Token::Position([0.0, 0.0, 0.0])],
            ),
            record(
                base + 7,
                "straight-curve",
                vec![
                    Token::Ref(-1),
                    Token::Position([0.0, 0.0, 0.0]),
                    Token::Vector3([1.0, 0.0, 0.0]),
                ],
            ),
        ]);
    }
    records
        .into_iter()
        .map(|record| (i64::try_from(record.index).unwrap(), record))
        .collect()
}

fn set_ref(records: &mut BTreeMap<i64, Record>, slot: i64, field: usize, reference: i64) {
    std::sync::Arc::make_mut(&mut records.get_mut(&slot).unwrap().tokens)[field] =
        Token::Ref(reference);
}

fn compare_step(
    a: &BTreeMap<i64, Record>,
    b: &BTreeMap<i64, Record>,
    changed: &Slots,
    saved_limit: Option<i64>,
) -> Statistics {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::desktop();
    let mut bytes = Vec::new();
    if let Some(limit) = saved_limit {
        bytes.extend_from_slice(b"ASM BinaryFile8");
        bytes.extend_from_slice(&23100_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 12]);
        bytes.extend_from_slice(&u64::try_from(limit).unwrap().to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut archive: HistoricalRecordArchive = a
        .iter()
        .map(|(&slot, record)| (slot, record.clone()))
        .collect();
    let versions_a = a.keys().map(|&slot| (slot, slot)).collect();
    let mut versions_b = BTreeMap::new();
    for (&slot, record) in b {
        let revision = if changed.contains(&slot) {
            slot + 1000
        } else {
            slot
        };
        archive.insert(revision, record.clone());
        versions_b.insert(slot, revision);
    }
    let mut binder = Binder::new(&archive, saved_limit);
    let mut stats = Statistics::default();
    binder
        .update(&a.keys().copied().collect(), &versions_a)
        .unwrap();
    binder.rebuild_units();
    let mut previous = binder.full(&ctx, &bytes, &mut stats).unwrap().unwrap();
    let full_previous = previous.clone();
    assert!(!previous.faces.is_empty());
    assert!(!previous.coedges.is_empty());
    let actual = binder
        .incremental(
            &ctx,
            &mut previous,
            changed,
            &versions_b,
            &bytes,
            &mut stats,
        )
        .unwrap();
    let expected = decode_slots(
        &ctx,
        &binder.live,
        &b.keys().copied().collect(),
        &bytes,
        &mut Statistics::default(),
        None,
    )
    .unwrap();
    assert_eq!(actual, expected);
    if let Some(expected) = expected.as_ref() {
        let old_snapshot = || crate::history_records::AsmStateSnapshot {
            state_id: 1,
            topology: expected,
            versions: &versions_b,
        };
        let new_snapshot = || crate::history_records::AsmStateSnapshot {
            state_id: 2,
            topology: &previous,
            versions: &versions_a,
        };
        let full_new_snapshot = || crate::history_records::AsmStateSnapshot {
            state_id: 2,
            topology: &full_previous,
            versions: &versions_a,
        };
        assert_eq!(
            super::transition_for_changes(
                old_snapshot(),
                new_snapshot(),
                changed,
                binder.changed_rows.as_ref()
            ),
            super::AsmHistoricalTransition::between(Some(old_snapshot()), full_new_snapshot()),
        );
    }
    stats
}

fn incremental_change(change: impl FnOnce(&mut BTreeMap<i64, Record>) -> Slots) {
    let a = fixture();
    let mut b = a.clone();
    let changed = change(&mut b);
    let stats = compare_step(&a, &b, &changed, None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}

#[test]
fn face_insertion() {
    let mut a = fixture();
    let b = a.clone();
    // The older state inserts a face that is absent in the newer state.
    for slot in 20..28 {
        a.remove(&slot);
    }
    set_ref(&mut a, 10, 3, -1);
    set_ref(&mut a, 13, 5, -1);
    let stats = compare_step(&a, &b, &(20..28).chain([10, 13]).collect(), None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}

#[test]
fn face_deletion() {
    incremental_change(|b| {
        for slot in 20..28 {
            b.remove(&slot);
        }
        set_ref(b, 10, 3, -1);
        set_ref(b, 13, 5, -1);
        (20..28).chain([10, 13]).collect()
    });
}

#[test]
fn neighbour_partner_change() {
    incremental_change(|b| {
        set_ref(b, 13, 5, -1);
        Slots::from([13])
    });
}

#[test]
fn carrier_update() {
    incremental_change(|b| {
        b.get_mut(&11).unwrap().tokens = vec![
            Token::Ref(-1),
            Token::Position([1.0, 2.0, 3.0]),
            Token::Vector3([0.0, 0.0, 1.0]),
            Token::Vector3([1.0, 0.0, 0.0]),
        ]
        .into();
        Slots::from([11])
    });
}

#[test]
fn shell_face_chain_change() {
    incremental_change(|b| {
        set_ref(b, 3, 5, 20);
        Slots::from([3])
    });
}

#[test]
fn uncovered_record_falls_back() {
    let a = fixture();
    let mut b = a.clone();
    b.insert(40, record(40, "unused", Vec::new()));
    let stats = compare_step(&a, &b, &Slots::from([40]), None);
    assert_eq!(stats.fallbacks.get("uncovered-change"), Some(&1));
}

#[test]
fn changed_definition_with_uncovered_reference_falls_back() {
    let mut a = fixture();
    a.insert(
        40,
        record(
            40,
            "unused",
            vec![
                Token::SubtypeOpen,
                Token::Ident("ref".into()),
                Token::Long(0),
                Token::SubtypeClose,
            ],
        ),
    );
    a.get_mut(&11).unwrap().tokens = vec![
        Token::SubtypeOpen,
        Token::Ident("example".into()),
        Token::SubtypeClose,
    ]
    .into();
    let mut b = a.clone();
    b.get_mut(&11).unwrap().tokens = vec![
        Token::SubtypeOpen,
        Token::Ident("other".into()),
        Token::SubtypeClose,
    ]
    .into();
    let stats = compare_step(&a, &b, &Slots::from([11]), None);
    assert_eq!(stats.fallbacks.get("uncovered-change"), Some(&1));
}

#[test]
fn subset_reference_uses_unchanged_external_definition() {
    let mut a = fixture();
    a.insert(
        40,
        record(
            40,
            "unused",
            vec![
                Token::SubtypeOpen,
                Token::Ident("example".into()),
                Token::SubtypeClose,
            ],
        ),
    );
    a.get_mut(&11).unwrap().tokens = vec![
        Token::SubtypeOpen,
        Token::Ident("ref".into()),
        Token::Long(0),
        Token::SubtypeClose,
    ]
    .into();
    let mut b = a.clone();
    set_ref(&mut b, 13, 5, -1);
    let stats = compare_step(&a, &b, &Slots::from([13]), None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}

#[test]
fn shared_attribute_falls_back() {
    let mut a = fixture();
    set_ref(&mut a, 1, 0, 40);
    set_ref(&mut a, 10, 0, 40);
    a.insert(40, record(40, "color-attrib", vec![Token::Ref(-1)]));
    let mut b = a.clone();
    set_ref(&mut b, 13, 5, -1);
    let stats = compare_step(&a, &b, &Slots::from([13]), None);
    assert_eq!(stats.fallbacks.get("global-dependency"), Some(&1));
}

#[test]
fn wire_change_falls_back() {
    let mut a = fixture();
    set_ref(&mut a, 3, 6, 30);
    a.extend([
        (30, refs(30, "wire", &[(4, 31)])),
        (31, refs(31, "coedge", &[(3, 31), (4, 31), (6, 32)])),
        (32, refs(32, "edge", &[(3, 15), (5, 15), (8, 17)])),
    ]);
    let mut b = a.clone();
    set_ref(&mut b, 32, 8, 27);
    let stats = compare_step(&a, &b, &Slots::from([32]), None);
    assert_eq!(stats.fallbacks.get("wire-change"), Some(&1));
}

#[test]
fn saved_edge_ownership_falls_back() {
    let a = fixture();
    let mut b = a.clone();
    for slot in [20, 22, 23] {
        b.remove(&slot);
    }
    set_ref(&mut b, 10, 3, -1);
    set_ref(&mut b, 13, 5, -1);
    set_ref(&mut b, 24, 7, -1);
    let stats = compare_step(&a, &b, &Slots::from([10, 13, 20, 22, 23, 24]), Some(30));
    assert_eq!(stats.fallbacks.get("saved-edge-ownership"), Some(&1));
}

#[test]
fn empty_shell_falls_back_before_scratch_retention() {
    let a = fixture();
    let mut b = a.clone();
    b.insert(4, refs(4, "shell", &[(7, 2)]));
    set_ref(&mut b, 3, 3, 4);
    let stats = compare_step(&a, &b, &Slots::from([3, 4]), None);
    assert_eq!(stats.fallbacks.get("empty-shell"), Some(&1));
}

#[test]
fn subset_decode_failure_falls_back() {
    let a = fixture();
    let mut b = a.clone();
    b.get_mut(&16).unwrap().tokens = vec![Token::Position([f64::NAN, 0.0, 0.0])].into();
    let stats = compare_step(&a, &b, &Slots::from([16]), None);
    assert_eq!(stats.fallbacks.get("subset-decode"), Some(&1));
}

#[test]
fn container_decode_failure_falls_back() {
    let a = fixture();
    let mut b = a.clone();
    b.insert(
        40,
        record(
            40,
            "unknown-attrib",
            vec![
                Token::Ref(-1),
                Token::Long(0),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(1),
                Token::Double(f64::NAN),
            ],
        ),
    );
    set_ref(&mut b, 1, 0, 40);
    let stats = compare_step(&a, &b, &Slots::from([1, 40]), None);
    assert_eq!(stats.fallbacks.get("container-decode"), Some(&1));
}

#[test]
fn container_with_incomplete_saved_edge_walk_falls_back() {
    let mut a = fixture();
    set_ref(&mut a, 14, 7, -1);
    let mut b = a.clone();
    set_ref(&mut b, 1, 5, 14);
    let stats = compare_step(&a, &b, &Slots::from([1]), Some(30));
    assert_eq!(stats.fallbacks.get("container-saved-edge"), Some(&1));
}

#[test]
fn inherited_attribute_insertion_updates_its_owner_unit() {
    incremental_change(|b| {
        b.insert(40, refs(40, "color-attrib", &[(4, 20)]));
        // A current attribute has an integer marker after the reserved ref.
        std::sync::Arc::make_mut(&mut b.get_mut(&40).unwrap().tokens)[1] = Token::Long(0);
        Slots::from([40])
    });
}

#[test]
fn orphan_container_falls_back() {
    let mut a = fixture();
    // This live shell is not owned by any body closure.
    a.insert(4, refs(4, "shell", &[(5, 10)]));
    let mut b = a.clone();
    set_ref(&mut b, 13, 5, -1);
    let stats = compare_step(&a, &b, &Slots::from([13]), None);
    assert_eq!(stats.fallbacks.get("orphan-container"), Some(&1));
}

#[test]
fn existing_saved_container_falls_back_for_container_edit() {
    let mut a = fixture();
    a.insert(30, refs(30, "edge", &[(3, 15), (5, 15), (8, 17)]));
    let mut b = a.clone();
    set_ref(&mut b, 3, 5, 20);
    let stats = compare_step(&a, &b, &Slots::from([3]), Some(40));
    assert_eq!(stats.fallbacks.get("saved-free-containers"), Some(&1));
}

#[test]
fn losing_face_admission_changes_saved_edge_ownership() {
    let a = fixture();
    let mut b = a.clone();
    set_ref(&mut b, 20, 5, -1);
    let stats = compare_step(&a, &b, &Slots::from([20]), Some(30));
    assert_eq!(stats.fallbacks.get("saved-edge-ownership"), Some(&1));
}

fn append_subtype(records: &mut BTreeMap<i64, Record>, slot: i64, name: &str, index: i64) {
    let record = records.get_mut(&slot).unwrap();
    let mut tokens = record.tokens.to_vec();
    tokens.extend([
        Token::SubtypeOpen,
        Token::Ident(name.into()),
        Token::Long(index),
        Token::SubtypeClose,
    ]);
    record.tokens = tokens.into();
}

fn assert_reference_dirty(a: &BTreeMap<i64, Record>, b: &BTreeMap<i64, Record>, changed: &Slots) {
    let mut archive: HistoricalRecordArchive = a.iter().map(|(&s, r)| (s, r.clone())).collect();
    let versions_a = a.keys().map(|&s| (s, s)).collect();
    let versions_b = b
        .iter()
        .map(|(&s, r)| {
            archive.insert(s + 1000, r.clone());
            (s, s + 1000)
        })
        .collect();
    let mut binder = Binder::new(&archive, None);
    binder
        .update(&a.keys().copied().collect(), &versions_a)
        .unwrap();
    assert!(binder
        .resolution_changes(changed, &versions_b)
        .contains(&21));
}

#[test]
fn inserted_definition_dirties_unchanged_reference_outside_changed_face() {
    let mut a = fixture();
    set_ref(&mut a, 13, 5, -1);
    set_ref(&mut a, 23, 5, -1);
    append_subtype(&mut a, 17, "example", 0);
    append_subtype(&mut a, 21, "ref", 0);
    let mut b = a.clone();
    append_subtype(&mut b, 11, "inserted", 0);
    let changed = Slots::from([11]);
    assert_reference_dirty(&a, &b, &changed);
    let stats = compare_step(&a, &b, &changed, None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}

#[test]
fn definition_content_change_dirties_unchanged_reference() {
    let mut a = fixture();
    append_subtype(&mut a, 11, "example", 0);
    append_subtype(&mut a, 21, "ref", 0);
    let mut b = a.clone();
    let tokens = std::sync::Arc::make_mut(&mut b.get_mut(&11).unwrap().tokens);
    tokens[6] = Token::Long(1);
    let changed = Slots::from([11]);
    assert_reference_dirty(&a, &b, &changed);
    let stats = compare_step(&a, &b, &changed, None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}

#[test]
fn definition_reference_dependencies_propagate_between_records() {
    let mut a = fixture();
    append_subtype(&mut a, 11, "first", 0);
    append_subtype(&mut a, 17, "second", 0);
    append_subtype(&mut a, 17, "ref", 0);
    append_subtype(&mut a, 21, "ref", 1);
    let mut b = a.clone();
    std::sync::Arc::make_mut(&mut b.get_mut(&11).unwrap().tokens)[6] = Token::Long(1);
    let changed = Slots::from([11]);
    assert_reference_dirty(&a, &b, &changed);
    let stats = compare_step(&a, &b, &changed, None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}

#[test]
fn container_owner_insertion_updates_closure_without_a_forward_link() {
    let a = fixture();
    let mut b = a.clone();
    b.insert(40, refs(40, "lump", &[(4, 3), (5, 1)]));
    set_ref(&mut b, 3, 7, 40);
    let stats = compare_step(&a, &b, &Slots::from([3, 40]), None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}
