// SPDX-License-Identifier: Apache-2.0

use super::derived_base_owner;

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
