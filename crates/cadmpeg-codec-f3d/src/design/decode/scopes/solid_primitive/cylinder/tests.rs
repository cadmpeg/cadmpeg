use super::read_extended_prologue;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;

const FRAME: [[f64; 4]; 4] = [
    [0.0, 1.0, 0.0, 2.0],
    [-1.0, 0.0, 0.0, 3.0],
    [0.0, 0.0, 1.0, 4.0],
    [0.0, 0.0, 0.0, 1.0],
];

fn frame(length: usize, member_count: usize) -> (Vec<u8>, DesignParameterScope) {
    let prefixed = matches!(length, 356 | 506);
    let compact = length == 356;
    let shift = if prefixed { 4 } else { 0 };
    let start = 9;
    let mut bytes = vec![0; start + length];
    let window = &mut bytes[start..];
    window[..4].copy_from_slice(&3u32.to_le_bytes());
    window[4..7].copy_from_slice(b"321");
    window[7..11].copy_from_slice(&8u32.to_le_bytes());
    window[21 + shift] = 1;
    if prefixed {
        window[20] = 1;
    }
    window[22 + shift] = 2;
    let members = [20u32, 21, 22, 23, 24, 25, 26];
    window[26 + shift..28 + shift].copy_from_slice(&[1, 1]);
    window[28 + shift..32 + shift].copy_from_slice(&members[member_count - 1].to_le_bytes());
    for (offset, target) in [38, 49, 60]
        .into_iter()
        .zip(members[..member_count - 1].iter().rev())
    {
        window[offset + shift] = 1;
        window[offset + shift + 1..offset + shift + 5].copy_from_slice(&target.to_le_bytes());
    }
    let (count, guid) = if compact {
        window[75] = 1;
        (76, 99)
    } else {
        for (slot, value) in window[72 + shift..200 + shift]
            .chunks_exact_mut(8)
            .zip(FRAME.into_iter().flatten())
        {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        (
            if matches!(length, 506 | 507) {
                212
            } else {
                200
            },
            if length == 507 { 227 } else { 223 + shift },
        )
    };
    window[count] = 1;
    window[count + 4] = 1;
    window[count + 5..count + 9].copy_from_slice(&35u32.to_le_bytes());
    window[guid] = 36;
    for (slot, code_unit) in window[guid + 4..guid + 76]
        .chunks_exact_mut(2)
        .zip("00000000-0000-0000-0000-000000000000".encode_utf16())
    {
        slot.copy_from_slice(&code_unit.to_le_bytes());
    }
    let reference_count_offset = guid + 79;
    window[reference_count_offset] = u8::try_from(member_count).unwrap();
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#8",
        DesignFeatureKind::CylinderPrimitive,
        8,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = u64::try_from(start).unwrap();
            draft.paired_byte_offset = u64::try_from(start + length).unwrap();
            draft.frame_length = u64::try_from(length).unwrap();
            draft.reference_count_offset = u64::try_from(start + reference_count_offset).unwrap();
            draft.reference_members = ReferenceRun::unlocated(members[..member_count].to_vec());
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

#[test]
fn extended_cylinder_reads_counted_compact_and_expanded_frames() {
    for (length, member_count) in [(356, 5), (479, 5), (501, 7), (502, 7), (506, 7), (507, 7)] {
        let (bytes, scope) = frame(length, member_count);
        let prologue = read_extended_prologue(&bytes, &scope, 9).unwrap();
        assert_eq!(prologue.operation, DesignExtrudeOperation::Cut);
        assert_eq!(
            prologue.operation_offset,
            if matches!(length, 356 | 506) { 35 } else { 31 }
        );
        assert_eq!(
            prologue.construction.placement_record_index,
            if member_count == 5 { 21 } else { 23 }
        );
        assert_eq!(prologue.construction.reference.value, 35);
        assert_eq!(
            prologue
                .construction
                .local_frame
                .map(|local| local.value.rows()),
            (length != 356).then_some(FRAME),
        );
    }
}

fn write_typed_reference(window: &mut [u8], offset: usize, target: u32, guid: &str) {
    window[offset] = 1;
    window[offset + 1..offset + 9].copy_from_slice(&u64::from(target).to_le_bytes());
    window[offset + 9..offset + 13].copy_from_slice(&36u32.to_le_bytes());
    window[offset + 13..offset + 49].copy_from_slice(guid.as_bytes());
}

#[test]
fn extended_cylinder_reads_typed_roles_in_both_reference_runs() {
    let start = 9;
    let (mut bytes, mut scope) = frame(999, 5);
    let window = &mut bytes[start..];
    window[26..].fill(0);
    window[26] = 1;
    let types = [
        "5A1BF548-241F-46FD-9FB5-E4B05126EB9D",
        "7B6C3C2D-4096-4E6D-B225-699E8C2D9355",
        "D91D429C-1BB5-4640-B49F-69AF14F463E9",
        "D91D429C-1BB5-4640-B49F-69AF14F463E9",
        "92112554-B316-46D1-8AFD-C49D91252558",
    ];
    for (ordinal, (target, guid)) in (20..25).zip(types).enumerate() {
        write_typed_reference(window, 506 + 51 * ordinal, target, guid);
        if ordinal != 0 {
            write_typed_reference(window, 27 + 51 * (4 - ordinal), target, guid);
        }
    }
    for (slot, value) in window[232..360]
        .chunks_exact_mut(8)
        .zip(FRAME.into_iter().flatten())
    {
        slot.copy_from_slice(&value.to_le_bytes());
    }
    window[360] = 1;
    write_typed_reference(window, 364, 35, "D26351F0-5940-4D23-AA20-2C35475A6D9E");
    window[423] = 36;
    for (slot, code_unit) in window[427..499]
        .chunks_exact_mut(2)
        .zip("00000000-0000-0000-0000-000000000000".encode_utf16())
    {
        slot.copy_from_slice(&code_unit.to_le_bytes());
    }
    window[502] = 5;
    scope
        .try_edit(|draft| {
            draft.reference_count_offset = 511;
            draft.layout_fixture_references();
        })
        .unwrap();
    let prologue = read_extended_prologue(&bytes, &scope, start).unwrap();
    assert_eq!(prologue.operation, DesignExtrudeOperation::Cut);
    assert_eq!(prologue.construction.placement_record_index, 21);
    assert_eq!(prologue.construction.reference.value, 35);
    assert_eq!(
        prologue.construction.local_frame.unwrap().value.rows(),
        FRAME
    );
    for offset in [27 + 13, 364 + 13, 506 + 51 + 13] {
        let mut corrupt = bytes.clone();
        corrupt[start + offset] = b'0';
        assert!(read_extended_prologue(&corrupt, &scope, start).is_none());
    }
}

#[test]
fn extended_cylinder_rejects_corrupt_framing_and_nonrigid_matrix() {
    let (bytes, scope) = frame(506, 7);
    assert!(read_extended_prologue(&bytes[..bytes.len() - 1], &scope, 9).is_none());
    for (offset, value) in [
        (20, 2),
        (25, 0),
        (26, 8),
        (30, 0),
        (41, 1),
        (65, 0),
        (75, 1),
        (76 + 14, 0),
        (204, 1),
        (212, 2),
        (216, 0),
        (227, 35),
        (304, 1),
    ] {
        let mut changed = bytes.clone();
        changed[9 + offset] = value;
        assert!(
            read_extended_prologue(&changed, &scope, 9).is_none(),
            "{offset}"
        );
    }
}
