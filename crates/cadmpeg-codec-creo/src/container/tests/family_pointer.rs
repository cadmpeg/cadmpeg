// SPDX-License-Identifier: Apache-2.0
//! A family pointer belongs to its declared section.

use super::super::{family_table, FamilyTablePointer, FamilyTableRecord, Section};

const POINTER_LABEL: &[u8] = b"drv_tbl_ptr\0";

fn assert_bounded_pointer(payload: &[u8], completion: &[u8], expected: Option<FamilyTablePointer>) {
    const PREFIX: &[u8] = b"prefix\n";
    const HEADER: &[u8] = b"#FamilyInf\n";
    for tail_len in [0, 1024] {
        let mut bytes = PREFIX.to_vec();
        bytes.extend_from_slice(HEADER);
        bytes.extend_from_slice(payload);
        let section_end = bytes.len();
        bytes.extend_from_slice(completion);
        bytes.extend_from_slice(POINTER_LABEL);
        bytes.push(0xe1);
        bytes.extend(std::iter::repeat_n(b'x', tail_len));
        let section = Section::scan("FamilyInf".into(), PREFIX.len(), section_end, None, &bytes)
            .expect("bounded family section");
        let expected = expected.map(|pointer| FamilyTableRecord {
            pointer,
            offset: PREFIX.len() + HEADER.len() + POINTER_LABEL.len(),
        });
        assert_eq!(family_table(&[section]), expected);
    }
}

#[test]
fn family_pointer_search_stops_at_section_end() {
    assert_bounded_pointer(b"opaque missing pointer", &[], None);
    assert_bounded_pointer(&POINTER_LABEL[..POINTER_LABEL.len() - 1], &[0, 0xe1], None);
    assert_bounded_pointer(POINTER_LABEL, &[0xe1], None);
}

#[test]
fn family_pointer_reference_cannot_consume_foreign_section_bytes() {
    let mut partial = POINTER_LABEL.to_vec();
    partial.extend_from_slice(&[0xf7, 0x80]);
    assert_bounded_pointer(&partial, &[0x80], None);
    let mut invalid = POINTER_LABEL.to_vec();
    invalid.extend_from_slice(&[0xf7, 0x80, 0x01]);
    assert_bounded_pointer(&invalid, &[], None);
}

#[test]
fn bounded_family_pointer_preserves_null_entity_and_absolute_offset() {
    for (value, expected) in [
        (&[0xe1][..], FamilyTablePointer::Null),
        (&[0xf7, 7][..], FamilyTablePointer::Entity(7)),
        (&[0xf7, 0x80, 0x80][..], FamilyTablePointer::Entity(128)),
    ] {
        let mut payload = POINTER_LABEL.to_vec();
        payload.extend_from_slice(value);
        assert_bounded_pointer(&payload, &[], Some(expected));
    }
}
