// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;

use super::topology_bridge;
use crate::brep::topology::{parse_bridge, MAGIC};

fn face_conflict_tripled(attr: u16) -> Vec<u8> {
    let mut bytes = vec![0, 0x0e];
    bytes.extend(attr.to_be_bytes());
    bytes.extend(7_u32.to_be_bytes());
    bytes.extend(0_u16.to_be_bytes());
    bytes.push(1);
    bytes.extend(MAGIC);
    for reference in [0_u16, 0, 20, 0, 100] {
        bytes.extend(reference.to_be_bytes());
        bytes.push(1);
    }
    bytes.push(0x2b);
    for _ in 0..5 {
        bytes.extend(0_u16.to_be_bytes());
        bytes.push(1);
    }
    bytes
}

fn face_conflict_scan(bytes: &[u8], deltas: bool) -> crate::brep::topology::Tables {
    crate::brep::topology::scan_records(bytes, deltas, false, None, None, None, None)
}

#[test]
fn face_conflict_identical_complete_frames_coalesce() {
    for frame in [topology_bridge(10, 20), face_conflict_tripled(10)] {
        assert!(crate::brep::topology::bridge_frame_bytes(&frame, 0).is_some());
        let repeated = [frame.clone(), frame].concat();
        let tables = face_conflict_scan(&repeated, true);
        assert_eq!(tables.bridges().len(), 1);
        assert!(tables.bridges().contains_key(&10));
    }
}

#[test]
fn face_conflict_distinct_trailing_fields_do_not_coalesce_a_matching_prefix() {
    for (frame, stride) in [(topology_bridge(10, 20), 2), (face_conflict_tripled(10), 3)] {
        let mut changed = frame.clone();
        let at = changed.len() - stride;
        changed[at..at + 2].copy_from_slice(&2_u16.to_be_bytes());
        assert_eq!(
            parse_bridge(&frame, 0, false).unwrap().refs,
            parse_bridge(&changed, 0, false).unwrap().refs
        );
        for frames in [
            [frame.clone(), changed.clone()],
            [changed.clone(), frame.clone()],
        ] {
            let tables = face_conflict_scan(&frames.concat(), true);
            assert!(!tables.bridges().contains_key(&10));
        }
    }
}

#[test]
fn face_conflict_partition_presence_wins_over_history() {
    let original = topology_bridge(10, 20);
    let mut partition = face_conflict_scan(&original, false);
    let history = face_conflict_scan(&face_conflict_tripled(10), true);
    partition.merge_deltas(history, Some(&HashSet::from([10])));
    assert_eq!(partition.bridges()[&10].sequence, 0);
    assert!(partition.bridge_frames[&10].partition);
}

#[test]
fn face_conflict_across_deltas_stays_withheld_after_later_repetition() {
    let frame = topology_bridge(10, 20);
    let mut changed = frame.clone();
    let at = changed.len() - 2;
    changed[at..].copy_from_slice(&2_u16.to_be_bytes());
    let mut current = crate::brep::topology::Tables::default();
    current.merge_deltas(face_conflict_scan(&frame, true), None);
    assert!(current.bridges().contains_key(&10));
    current.merge_deltas(face_conflict_scan(&changed, true), None);
    assert!(!current.bridges().contains_key(&10));
    current.merge_deltas(face_conflict_scan(&frame, true), None);
    assert!(!current.bridges().contains_key(&10));
}

#[test]
fn face_conflict_partition_ambiguity_does_not_resurrect_from_deltas() {
    let frame = topology_bridge(10, 20);
    let changed = topology_bridge(10, 21);
    let mut partition = face_conflict_scan(&[frame.clone(), changed].concat(), false);
    assert!(!partition.bridges().contains_key(&10));
    partition.merge_deltas(face_conflict_scan(&frame, true), Some(&HashSet::from([10])));
    assert!(!partition.bridges().contains_key(&10));
    assert!(partition.bridge_frames[&10].partition);
}

#[test]
fn face_conflict_unknown_frame_completeness_cannot_prove_duplicate_equality() {
    let frame = face_conflict_tripled(10);
    let p = crate::brep::topology::body_start(&frame, 0, 0x0e).unwrap();
    let prefix = frame[..=p + crate::layout::face_tripled::SENSE].to_vec();
    assert!(parse_bridge(&prefix, 0, false).is_some());
    assert!(crate::brep::topology::bridge_frame_bytes(&prefix, 0).is_none());
    assert!(face_conflict_scan(&prefix, true)
        .bridges()
        .contains_key(&10));
    assert!(
        !face_conflict_scan(&[prefix.clone(), prefix].concat(), true)
            .bridges()
            .contains_key(&10)
    );
}

#[test]
fn face_conflict_new_history_bridge_requires_existing_typed_selection() {
    let mut partition = face_conflict_scan(&topology_bridge(10, 20), false);
    partition.merge_deltas(face_conflict_scan(&face_conflict_tripled(11), true), None);
    assert!(!partition.bridges().contains_key(&11));
    partition.merge_deltas(
        face_conflict_scan(&face_conflict_tripled(11), true),
        Some(&HashSet::from([11])),
    );
    assert!(partition.bridges().contains_key(&10));
    assert!(partition.bridges().contains_key(&11));
}
