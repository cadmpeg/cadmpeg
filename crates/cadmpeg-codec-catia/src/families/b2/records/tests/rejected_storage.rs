// SPDX-License-Identifier: Apache-2.0
//! Rejected fixed reference-lane storage.

#[test]
fn rejected_reference_lists_allocate_no_retained_entries() {
    let mut bytes = crate::test_support::test_b2::b2_reference_list_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let payload = records[0].payload().expect("reference payload").start;
    bytes[payload] = 0x81;
    bytes[payload + 1] = 0;
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..64 {
            assert!(
                super::super::b2_reference_lists_from_records(ctx, &bytes, &records)
                    .expect("rejected reference scratch")
                    .is_empty()
            );
        }
    });
}
