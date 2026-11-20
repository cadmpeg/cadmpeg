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

#[test]
fn wide_station_fills_only_the_bounded_fallback() {
    use crate::wire::records::consolidated_records;
    let frame = |class, payload: Vec<u8>| {
        let mut bytes = vec![
            0xb2,
            3,
            class,
            u8::try_from(payload.len()).expect("short fixture frame"),
            5,
        ];
        bytes.extend(payload);
        bytes
    };
    let resolve = |index: usize, reference: AllocationReference| match reference.encoding {
        AllocationReferenceEncoding::TaggedU24 if reference.value == 0x0012_3456 => Some(1),
        AllocationReferenceEncoding::OwnedChild => {
            index.checked_add(1 + usize::try_from(reference.value).ok()?)
        }
        AllocationReferenceEncoding::BackwardDistance => {
            index.checked_sub(usize::try_from(reference.value).ok()?)
        }
        _ => None,
    };
    for t in [-0.1_f64, 0.0, 0.5, 1.0, 1.1] {
        let mut bytes = frame(0x5d, vec![0x0e, 0x56, 0x34, 0x12, 0]);
        bytes.extend(frame(0x05, vec![0x81, 3]));
        let mut incidence = vec![0x82, 9, 3, 0x80];
        incidence.extend(t.to_le_bytes());
        bytes.extend(frame(0x06, incidence));
        bytes.extend(frame(0x18, vec![0]));
        let records = consolidated_records(&bytes);
        assert_eq!(records.len(), 4);
        for range in [[0.0, 1.0], [1.0, 0.0]] {
            let domain = Domain {
                range,
                knots: None,
                line: true,
            };
            let mut refusal = LaneRefusals::new();
            assert_eq!(
                parameter(&bytes, &records, 0, 3, domain, &resolve, &mut refusal),
                None
            );
            assert_eq!(
                wide_parameter(&bytes, &records, 0, 3, domain, &resolve, &mut refusal),
                (0.0..=1.0).contains(&t).then_some(t)
            );
        }
    }
}
