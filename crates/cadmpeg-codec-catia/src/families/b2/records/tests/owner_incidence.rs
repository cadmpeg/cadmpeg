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
        let counted = crate::test_support::with_service_context(|_ctx| {
            crate::families::b2::records::b2_counted_owners_from_records(&bytes, &records)
        });
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
    let relations = crate::test_support::with_service_context(|_ctx| {
        crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(&bytes, &records)
    });
    assert_eq!(relations.len(), 2);
    assert_eq!(relations[0].owner.source_index, 0);
    assert_eq!(relations[1].owner.source_index, 1);
}

#[test]
fn compact_face_with_double_three_terminal_binds_width_coded_counted_owner() {
    let mut bytes = crate::test_support::test_b2::b2_adjacent_face_counted_owner_stream();
    bytes[10] = 0x03;
    let records = crate::wire::records::consolidated_records(&bytes);
    let relations = crate::test_support::with_service_context(|_ctx| {
        crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(&bytes, &records)
    });
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].face_node.terminal, [0x03, 0x03]);
    assert_eq!(
        relations[0].face_node.target.checked_add(1),
        relations[0].owner.references.last().copied()
    );

    bytes[34] = 0x96;
    let records = crate::wire::records::consolidated_records(&bytes);
    let relations = crate::test_support::with_service_context(|_ctx| {
        crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(&bytes, &records)
    });
    assert!(relations.is_empty());
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
    let relations = crate::test_support::with_service_context(|_ctx| {
        crate::families::b2::records::b2_adjacent_face_owners_from_records(&bytes, &records)
    });
    assert_eq!(relations.len(), 2);
    assert_eq!(relations[0].owner.source_index, 0);
    assert_eq!(relations[1].owner.source_index, 1);
}

#[test]
fn counted_incidence_resolves_face_and_loop_sharing_construction_bridge() {
    let bytes = six_edge_incidence_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let relations = super::super::b2_adjacent_face_counted_owners_from_records(&bytes, &records);
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].face_node.pos, 0);
    assert_eq!(relations[0].face_node.terminal[1], 0x03);
    assert_eq!(relations[0].owner.references.last().copied(), Some(1));
}

#[test]
fn counted_incidence_rejects_owned_child_with_five_terminal() {
    let mut bytes = six_edge_incidence_stream();
    bytes[8] = 0x05;
    let records = crate::wire::records::consolidated_records(&bytes);
    let relations = super::super::b2_adjacent_face_counted_owners_from_records(&bytes, &records);
    assert!(relations.is_empty());
}

#[test]
fn counted_incidence_rejects_mismatched_reference_forms_and_bridge_class() {
    for (offset, replacement) in [(6, 0x05), (41, 0x03), (18, 0x30), (25, 0x30)] {
        let mut bytes = six_edge_incidence_stream();
        bytes[offset] = replacement;
        let records = crate::wire::records::consolidated_records(&bytes);
        let relations = crate::test_support::with_service_context(|_ctx| {
            crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(
                &bytes, &records,
            )
        });
        assert!(relations.is_empty(), "offset {offset}");
    }
}

#[test]
fn counted_incidence_rejects_unframed_gap_before_owned_bridge() {
    let mut bytes = six_edge_incidence_stream();
    bytes.insert(9, 0);
    let records = crate::wire::records::consolidated_records(&bytes);
    let relations = crate::test_support::with_service_context(|_ctx| {
        crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(&bytes, &records)
    });
    assert!(relations.is_empty());
}

fn six_edge_incidence_stream() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0xb2, 0x03, 0x5f, 0x04, 0x05, 0x82, 0x07, 0x0b, 0x03]);
    bytes.extend_from_slice(&[0xb2, 0x03, 0x30, 0x02, 0x05, 0x80, 0x01]);
    bytes.extend_from_slice(&[0xb2, 0x03, 0x37, 0x02, 0x05, 0x81, 0x05]);
    bytes.extend_from_slice(&[
        0xb2, 0x03, 0x62, 0x14, 0x05, 0x8d, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03,
        0x03, 0x03, 0x03, 0x05, 0x86, 0x41, 0x17, 0xdc, 0x03, 0x01,
    ]);
    bytes
}
