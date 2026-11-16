// SPDX-License-Identifier: Apache-2.0

use super::AllocationJournal;
use crate::wire::records::{consolidated_records, consolidated_records_in_range_sources};

fn frame(class: u8, increment: u8) -> [u8; 5] {
    [0xb2, 0x03, class, 0, increment * 4 + 1]
}

#[test]
fn allocation_addresses_include_each_frames_compact_increment() {
    let mut data = frame(0x27, 63).to_vec();
    data.extend_from_slice(&[0xb3, 0x13, 0x18, 0, 0x04, 64]);
    data.extend_from_slice(&[0xb4, 0x83, 0x25, 0, 0x08, 0, 1]);
    let records = consolidated_records(&data);
    let journal = AllocationJournal::new(&records);
    assert_eq!(journal.address(0), Some(63));
    assert_eq!(journal.address(1), Some(127));
    assert_eq!(journal.address(2), Some(383));
    assert_eq!(journal.absolute_address(2), Some(383));
    assert_eq!(journal.resolve_address(2, 127), Some(&records[1]));
    assert_eq!(journal.resolve_weighted_distance(2, 256), Some(&records[1]));
    assert_eq!(journal.resolve_weighted_distance(1, 64), Some(&records[0]));
    assert!(journal.resolve_weighted_distance(2, u32::MAX).is_none());
    assert_eq!(journal.index(0, records[1].source_range().start), Some(1));
    assert!(journal.index(1, records[1].source_range().start).is_none());
}

#[test]
fn allocation_local_reference_forms_keep_their_own_units() {
    let data = [frame(0x27, 1), frame(0x18, 2), frame(0x25, 3)].concat();
    let records = consolidated_records(&data);
    let journal = AllocationJournal::new(&records);
    assert_eq!(journal.resolve_owned_child(0, 0), Some(&records[1]));
    assert_eq!(journal.resolve_owned_child(0, 1), Some(&records[2]));
    assert_eq!(journal.resolve_record_distance(2, 1), Some(&records[1]));
    assert!(journal.resolve_weighted_distance(2, 1).is_none());
    assert_eq!(journal.resolve_weighted_distance(2, 3), Some(&records[1]));
    assert!(journal.resolve_owned_child(2, 0).is_none());
    assert!(journal.resolve_owned_child(0, u32::MAX).is_none());
    assert!(journal.resolve_record_distance(0, 1).is_none());
}

#[test]
fn allocation_sources_and_logical_gaps_bound_every_reference() {
    let first = frame(0x27, 1);
    let second = frame(0x18, 1);
    let data = [first, second].concat();
    let boundary = first.len();
    let records = consolidated_records_in_range_sources(
        &data,
        [
            std::iter::once(0..boundary),
            std::iter::once(boundary..data.len()),
        ],
    );
    let journal = AllocationJournal::new(&records);
    assert_eq!(journal.address(0), Some(1));
    assert_eq!(journal.address(1), Some(1));
    assert_eq!(journal.resolve_address(1, 1), Some(&records[1]));
    assert!(journal.resolve_owned_child(0, 0).is_none());
    assert!(journal.resolve_record_distance(1, 1).is_none());
    assert!(journal.resolve_weighted_distance(1, 1).is_none());
    assert_eq!(journal.absolute_address(0), Some(1));
    assert_eq!(journal.absolute_address(1), Some(1));

    let mut gap = first.to_vec();
    gap.push(0);
    gap.extend_from_slice(&second);
    let records = consolidated_records(&gap);
    let journal = AllocationJournal::new(&records);
    assert_eq!(records.len(), 2);
    assert!(journal.resolve_owned_child(0, 0).is_none());
    assert!(journal.resolve_record_distance(1, 1).is_none());
    assert!(journal.resolve_weighted_distance(1, 1).is_none());
    assert_eq!(journal.absolute_address(0), Some(1));
    assert!(journal.absolute_address(1).is_none());
}

#[test]
fn allocation_duplicate_source_positions_withhold_both_indices() {
    let data = frame(0x18, 1);
    let records = consolidated_records(&data);
    let duplicate = [records[0].clone(), records[0].clone()];
    let journal = AllocationJournal::new(&duplicate);
    assert!(journal.index(0, records[0].source_range().start).is_none());
    assert!(journal.address(0).is_none());
    assert!(journal.address(1).is_none());
}

#[test]
fn allocation_physical_extents_preserve_one_logical_address_scope() {
    let data = [frame(0x27, 1), frame(0x18, 2), frame(0x25, 3)].concat();
    let split = frame(0x27, 1).len() + 2;
    let records = consolidated_records_in_range_sources(&data, [vec![0..split, split..data.len()]]);
    let journal = AllocationJournal::new(&records);
    assert_eq!(records.len(), 3);
    assert_eq!(journal.address(2), Some(6));
    assert_eq!(journal.resolve_weighted_distance(2, 3), Some(&records[1]));
}

#[test]
fn allocation_zero_increments_leave_duplicate_addresses_ambiguous() {
    let data = [frame(0x27, 1), frame(0x18, 0), frame(0x25, 2)].concat();
    let records = consolidated_records(&data);
    let journal = AllocationJournal::new(&records);
    assert!(journal.address(0).is_none());
    assert!(journal.address(1).is_none());
    assert_eq!(journal.address(2), Some(3));
    assert!(journal.resolve_address(2, 1).is_none());
    assert!(journal.resolve_weighted_distance(2, 2).is_none());
    assert_eq!(journal.resolve_owned_child(0, 0), Some(&records[1]));
}

#[test]
fn allocation_invalid_compact_headers_preserve_the_known_prefix() {
    let mut data = frame(0x27, 1).to_vec();
    data.extend_from_slice(&[0xb2, 0x03, 0x18, 0, 0x02]);
    data.extend_from_slice(&frame(0x25, 2));
    let records = consolidated_records(&data);
    let journal = AllocationJournal::new(&records);
    assert_eq!(records.len(), 3);
    assert_eq!(journal.address(0), Some(1));
    assert_eq!(journal.absolute_address(0), Some(1));
    assert_eq!(journal.resolve_address(0, 1), Some(&records[0]));
    for i in 1..records.len() {
        assert!(journal.address(i).is_none());
        assert!(journal.resolve_address(i, 1).is_none());
    }
    assert_eq!(journal.resolve_owned_child(0, 0), Some(&records[1]));
}
