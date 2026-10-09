// SPDX-License-Identifier: Apache-2.0

use super::super::{family_table, FamilyTablePointer, FamilyTableRecord, Section};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

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
        let section = Section::scan_for_test("FamilyInf".into(), PREFIX.len(), section_end,
            None, &bytes).expect("bounded family section");
        let expected = expected.map(|pointer| FamilyTableRecord {
            pointer,
            offset: PREFIX.len() + HEADER.len() + POINTER_LABEL.len(),
        });
        // One selected section visit plus the current core search bound.
        // All windows here are at least as long as the fixed label.
        let scan_work = u64::try_from(HEADER.len() + payload.len() + POINTER_LABEL.len())
            .expect("bounded fixture search");
        let work = 1 + scan_work;
        for allowed in 0..=work {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowed;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = family_table(&ctx, std::slice::from_ref(&section));
            let original = if allowed < work {
                let Err(CodecError::ResourceLimit(refusal)) = result else {
                    panic!("next actual selected-section operation must refuse");
                };
                let (operation, used, additional) = if allowed == 0 {
                    ("creo named section selection", 0, 1)
                } else {
                    ("find Creo family table", 1, scan_work)
                };
                assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                assert_eq!(refusal.operation, operation);
                assert_eq!((refusal.used, refusal.additional), (used, additional));
                refusal
            } else {
                assert_eq!(result.expect("bounded search excludes unrelated tail"), expected);
                let refusal = ctx.charge_work_limit(1, "after bounded family pointer")
                    .expect_err("exact source work cap");
                assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                assert_eq!((refusal.used, refusal.additional), (work, 1));
                refusal
            };
            assert!(matches!(family_table(&ctx, &[]),
                Err(CodecError::ResourceLimit(refusal)) if refusal == original));
            assert_eq!(ctx.resource_refusal(), Some(original));
        }
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
