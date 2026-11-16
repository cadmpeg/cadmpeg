// SPDX-License-Identifier: Apache-2.0

use super::derived_base_owner;

fn source_frame() -> Vec<u8> {
    let mut bytes = vec![0; 562];
    for at in [19, 24, 39, 54, 59, 74, 85] {
        bytes[at] = 1;
    }
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[70..74].copy_from_slice(&1u32.to_le_bytes());
    for (at, value) in [(25, 31u64), (40, 41), (60, 51), (75, 61), (86, 41)] {
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn source_pair_names_the_source_result_and_timeline_owner() {
    let source = super::derived_base_source(&source_frame(), 1000).unwrap();
    assert_eq!(source.parameter_body.value, 31);
    assert_eq!(source.result_body.value, 41);
    assert_eq!(source.owner_record.value, 51);
    assert_eq!(source.source_body.value, 61);
    assert_eq!(source.source_body.offset, 1075);
    assert_eq!(source.result_repeat_offset, 1086);
    assert!(source.frame_holds(1000, 1444));
    assert!(source.snapshot.is_none());
    assert!(super::derived_base_owner(&source_frame(), 1000).is_none());
}

#[test]
fn source_pair_requires_its_counts_local_references_and_repeated_result() {
    for at in [
        19, 20, 24, 33, 39, 48, 54, 55, 59, 68, 70, 74, 83, 85, 86, 94, 96,
    ] {
        let mut bytes = source_frame();
        bytes[at] ^= 3;
        assert!(super::derived_base_source(&bytes, 0).is_none(), "byte {at}");
    }
    assert!(super::derived_base_source(&source_frame()[..96], 0).is_none());
}

#[test]
fn source_pair_preserves_wide_body_references_and_checks_location_arithmetic() {
    let mut bytes = source_frame();
    bytes[75..83].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(
        super::derived_base_source(&bytes, 0)
            .unwrap()
            .source_body
            .value,
        u64::MAX
    );
    assert!(super::derived_base_source(&bytes, usize::MAX).is_none());
    let mut source = super::derived_base_source(&bytes, 1000).unwrap();
    source.source_body.offset += 1;
    assert!(!source.frame_holds(1000, 1444));
}

/// A derived base frame of `len` bytes that names `owner` at offset 59 and
/// again twelve bytes before its end, with a zero last byte.
fn frame(len: usize, owner: u64) -> Vec<u8> {
    let mut frame = vec![0; len];
    for at in [59, len - 12] {
        frame[at] = 1;
        frame[at + 1..at + 9].copy_from_slice(&owner.to_le_bytes());
    }
    frame
}

#[test]
fn a_derived_base_frame_names_its_owner_twice() {
    let owner = derived_base_owner(&frame(405, 8269), 1000).expect("owner");
    assert_eq!(owner.record_index.value, 8269);
    assert_eq!(owner.record_index.offset, 1060);
    assert_eq!(owner.repeat_offset, 1393);
}

#[test]
fn a_derived_base_owner_needs_two_equal_well_formed_references() {
    let mut differs = frame(405, 8269);
    differs[394] = 2;
    let mut unmarked = frame(405, 8269);
    unmarked[59] = 0;
    unmarked[393] = 0;
    let mut trailing = frame(405, 8269);
    trailing[404] = 1;
    let mut unclosed = frame(405, 8269);
    unclosed[68] = 1;
    unclosed[402] = 1;
    for (frame, reason) in [
        (differs, "the repeated reference names another record"),
        (unmarked, "the references lack their marker"),
        (trailing, "the last byte is not zero"),
        (unclosed, "the references lack their two zero bytes"),
        (
            frame(405, u64::from(u32::MAX) + 1),
            "the owner exceeds a record index",
        ),
        (
            frame(75, 8269)[..70].to_vec(),
            "the repeat overlaps the first reference",
        ),
        (
            frame(405, 8269)[..65].to_vec(),
            "the frame ends inside the first reference",
        ),
    ] {
        assert_eq!(derived_base_owner(&frame, 0), None, "{reason}");
    }
}
