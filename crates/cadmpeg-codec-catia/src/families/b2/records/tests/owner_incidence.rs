// SPDX-License-Identifier: Apache-2.0
//! Counted incidence grammar and fixed-nine dialect separation.

#[test]
fn fixed_nine_packets_are_not_counted_allocation_programs() {
    for bytes in [
        crate::test_support::test_b2::b2_owner_packet_stream(),
        crate::test_support::test_b2::b2_width_coded_owner_packet_stream(),
        crate::test_support::test_b2::b2_all_compact_owner_packet_stream(),
    ] {
        let records = crate::wire::records::consolidated_records(&bytes);
        assert_eq!(
            crate::families::b2::records::b2_owner_packets_from_records(&bytes, &records).count(),
            1
        );
        let counted = crate::test_support::with_service_context(|ctx| {
            crate::families::b2::records::b2_counted_owners_from_records(ctx, &bytes, &records)
        })
        .expect("fixed packet fits service limits");
        assert!(counted.is_empty());
    }
}

#[test]
fn counted_incidence_keeps_distinct_sources_at_the_same_physical_offsets() {
    let bytes = crate::test_support::test_b2::b2_adjacent_face_counted_owner_stream();
    let records = crate::wire::records::consolidated_records_in_range_sources(
        &bytes,
        [
            std::iter::once(0..bytes.len()),
            std::iter::once(0..bytes.len()),
        ],
    );
    let relations = crate::test_support::with_service_context(|ctx| {
        crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(
            ctx, &bytes, &records,
        )
    })
    .expect("both sources fit service limits");
    assert_eq!(relations.len(), 2);
    assert_eq!(relations[0].owner.source_index, 0);
    assert_eq!(relations[1].owner.source_index, 1);
}

#[test]
fn fixed_incidence_keeps_distinct_sources_at_the_same_physical_offsets() {
    let bytes = crate::test_support::test_b2::b2_adjacent_face_owner_stream();
    let records = crate::wire::records::consolidated_records_in_range_sources(
        &bytes,
        [
            std::iter::once(0..bytes.len()),
            std::iter::once(0..bytes.len()),
        ],
    );
    let relations = crate::test_support::with_service_context(|ctx| {
        crate::families::b2::records::b2_adjacent_face_owners_from_records(ctx, &bytes, &records)
    })
    .expect("both sources fit service limits");
    assert_eq!(relations.len(), 2);
    assert_eq!(relations[0].owner.source_index, 0);
    assert_eq!(relations[1].owner.source_index, 1);
}
