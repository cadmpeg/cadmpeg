// SPDX-License-Identifier: Apache-2.0

use super::super::*;

#[test]
fn wide_roster_ordinal_preserves_all_three_bytes() {
    assert_eq!(
        vertex_roster_reference(&[0x0e, 0x56, 0x34, 0x12, 0]),
        Some(AllocationReference {
            value: 0x0012_3456,
            encoding: AllocationReferenceEncoding::TaggedU24,
        })
    );
}

#[test]
fn wide_roster_reference_requires_complete_frame_and_terminal() {
    for bytes in [
        &[0x0e, 0x56, 0x34][..],
        &[0x0e, 0x56, 0x34, 0x12],
        &[0x0e, 0x56, 0x34, 0x12, 1],
        &[0x0e, 0x56, 0x34, 0x12, 0, 0],
    ] {
        assert_eq!(vertex_roster_reference(bytes), None);
    }
}

#[test]
fn vertex_roster_reference_preserves_existing_address_forms() {
    for (bytes, value, encoding) in [
        (&[3, 0][..], 0, AllocationReferenceEncoding::OwnedChild),
        (&[5, 0], 1, AllocationReferenceEncoding::BackwardDistance),
        (
            &[0x0a, 0x34, 0x12, 0],
            0x1234,
            AllocationReferenceEncoding::TaggedU16,
        ),
        (&[0x0e, 0], 3, AllocationReferenceEncoding::Selector2),
    ] {
        assert_eq!(
            vertex_roster_reference(bytes),
            Some(AllocationReference { value, encoding })
        );
    }
    let mut at = 0;
    assert_eq!(
        allocation_reference(&[0x0e, 0x56, 0x34, 0x12, 0], &mut at),
        Some(AllocationReference {
            value: 3,
            encoding: AllocationReferenceEncoding::Selector2
        })
    );
    assert_eq!(at, 1);
}
