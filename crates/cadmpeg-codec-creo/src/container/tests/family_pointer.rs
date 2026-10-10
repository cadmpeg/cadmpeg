// SPDX-License-Identifier: Apache-2.0

use super::super::{family_table, FamilyTablePointer, FamilyTableRecord, Section};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

const POINTER_LABEL: &[u8] = b"drv_tbl_ptr\0";

fn assert_bounded_pointer(payload: &[u8], completion: &[u8], expected: Option<FamilyTablePointer>) {
    const PREFIX: &[u8] = b"prefix\n";
    const HEADER: &[u8] = b"#FamilyInf\n";
    let mut previous_refusal = None;
    for tail_len in [0, 1024] {
        let mut bytes = PREFIX.to_vec();
        bytes.extend_from_slice(HEADER);
        bytes.extend_from_slice(payload);
        let section_end = bytes.len();
        bytes.extend_from_slice(completion);
        bytes.extend_from_slice(POINTER_LABEL);
        bytes.push(0xe1);
        bytes.extend(std::iter::repeat_n(b'x', tail_len));
        let section = Section::scan_for_test("FamilyInf".into(), PREFIX.len(), section_end,
            None, &bytes).expect("bounded family section");
        let expected = expected.map(|pointer| FamilyTableRecord {
            pointer,
            offset: PREFIX.len() + HEADER.len() + POINTER_LABEL.len(),
        });
        let result = crate::test_support::assert_work_boundaries(
            &["creo named section selection", "find Creo family table"],
            |ctx| family_table(ctx, std::slice::from_ref(&section)),
        );
        assert_eq!(result, expected);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(family_table(&ctx, std::slice::from_ref(&section)).expect("borrowed pointer search"), expected);
        let refusal = crate::test_support::last_refusal_at(&[], ResourceDimension::WorkUnits,
            "find Creo family table", |ctx| family_table(ctx, std::slice::from_ref(&section)));
        let cadmpeg_core::CodecError::ResourceLimit(refusal) = refusal else {
            panic!("family pointer work boundary");
        };
        if let Some(previous) = &previous_refusal {
            assert_eq!(&refusal, previous, "foreign tail changes no admitted work");
        }
        previous_refusal = Some(refusal);
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
