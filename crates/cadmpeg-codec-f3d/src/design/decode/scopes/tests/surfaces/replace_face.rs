// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::scopes::replace_face::exact_replace_face_operation;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::feature::surface_ops::DesignReplaceFaceOperation;
use crate::test_support::indexed_header;

fn prefix(control: u8, replacements: &[u32], targets: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"777", 40);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(control);
    for run in [replacements, targets] {
        bytes.extend_from_slice(&u32::try_from(run.len()).unwrap().to_le_bytes());
        for value in run {
            bytes.push(1);
            bytes.extend_from_slice(&value.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
    }
    bytes.extend_from_slice(&36u32.to_le_bytes());
    for unit in "00000000-0000-0000-0000-000000000000".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 3]);
    bytes
}

fn read(bytes: &[u8]) -> Option<DesignReplaceFaceOperation> {
    let mut scope = DesignParameterScope::empty(
        "f3d:test:replace-face#40",
        DesignFeatureKind::ReplaceFace,
        40,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = u64::try_from(bytes.len()).unwrap() + 160;
            draft.reference_count_offset = u64::try_from(bytes.len()).unwrap();
            draft.paired_byte_offset = draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    crate::test_support::with_decode_context(|ctx| exact_replace_face_operation(ctx, bytes, &scope))
        .unwrap()
}

#[test]
fn replacement_prefix_preserves_boolean_and_both_ordered_group_runs() {
    for control in [0, 1] {
        let bytes = prefix(control, &[44], &[50]);
        assert_eq!(bytes.len(), 131);
        assert_eq!(&bytes[21..26], &[control, 1, 0, 0, 0]);
        assert_eq!(&bytes[37..41], &[1, 0, 0, 0]);
        assert_eq!(&bytes[52..56], &[36, 0, 0, 0]);
        let operation = read(&bytes).unwrap();
        assert_eq!(operation.tangent_chain, control == 1);
        assert_eq!(operation.flagged_prefix_word, None);
        assert_eq!(operation.replacement_group_record_indices, [44]);
        assert_eq!(operation.target_group_record_indices, [50]);
        assert_eq!(
            operation.trailing_identity.as_str(),
            "00000000-0000-0000-0000-000000000000"
        );
    }
    let operation = read(&prefix(1, &[44, 47], &[50, 53, 56])).unwrap();
    assert_eq!(operation.replacement_group_record_indices, [44, 47]);
    assert_eq!(operation.target_group_record_indices, [50, 53, 56]);
    let operation = read(&prefix(0, &[], &[])).unwrap();
    assert!(operation.replacement_group_record_indices.is_empty());
    assert!(operation.target_group_record_indices.is_empty());
    let mut bytes = prefix(0, &[44], &[50]);
    bytes[56] = b'A';
    assert_eq!(
        read(&bytes).unwrap().trailing_identity.as_str(),
        "A0000000-0000-0000-0000-000000000000"
    );
}

#[test]
fn replacement_flagged_prefix_preserves_word_control_and_group_order() {
    for control in [0, 1] {
        for word in [0u32, 0x1122_3344] {
            let mut bytes = prefix(control, &[44], &[50]);
            bytes[20] = 1;
            bytes.splice(21..21, word.to_le_bytes());
            assert_eq!(bytes.len(), 135);
            assert_eq!(&bytes[21..25], &word.to_le_bytes());
            assert_eq!(&bytes[25..30], &[control, 1, 0, 0, 0]);
            assert_eq!(&bytes[41..45], &[1, 0, 0, 0]);
            assert_eq!(&bytes[56..60], &[36, 0, 0, 0]);
            let operation = read(&bytes).unwrap();
            assert_eq!(operation.flagged_prefix_word, Some(word));
            assert_eq!(operation.tangent_chain, control == 1);
            assert_eq!(operation.replacement_group_record_indices, [44]);
            assert_eq!(operation.target_group_record_indices, [50]);
            assert_eq!(
                operation.trailing_identity.as_str(),
                "00000000-0000-0000-0000-000000000000"
            );
        }
    }
}

#[test]
fn replacement_flagged_prefix_requires_flag_padding_bounds_and_closed_identity() {
    let mut original = prefix(0, &[44], &[50]);
    original[20] = 1;
    original.splice(21..21, 0u32.to_le_bytes());
    for (at, value) in [
        (19, 1),
        (20, 2),
        (25, 2),
        (30, 0),
        (35, 1),
        (45, 0),
        (56, 35),
        (61, 1),
        (132, 1),
    ] {
        let mut bytes = original.clone();
        bytes[at] = value;
        assert!(read(&bytes).is_none(), "offset {at}");
    }
    for at in [26, 41] {
        let mut bytes = original.clone();
        bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read(&bytes).is_none());
    }
    for end in [20, 24, 25, 29, 40, 44, 55, 59, 131, 134] {
        assert!(read(&original[..end]).is_none());
    }
    original.push(0);
    assert!(read(&original).is_none());
}

#[test]
fn replacement_prefix_requires_bounded_counts_marked_references_and_closed_identity() {
    let original = prefix(0, &[44], &[50]);
    for (at, value) in [
        (11, 1),
        (21, 2),
        (26, 0),
        (31, 1),
        (41, 0),
        (46, 1),
        (52, 35),
        (57, 1),
        (128, 1),
    ] {
        let mut bytes = original.clone();
        bytes[at] = value;
        assert!(read(&bytes).is_none(), "offset {at}");
    }
    for at in [22, 37] {
        let mut bytes = original.clone();
        bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read(&bytes).is_none());
    }
    for end in [25, 36, 40, 51, 55, 127, 130] {
        assert!(read(&original[..end]).is_none());
    }
    let mut bytes = original;
    bytes.push(0);
    assert!(read(&bytes).is_none());
}
