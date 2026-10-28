// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use super::*;
use crate::container::{outer_container_declarations, outer_container_for_extent};

fn source() -> (Vec<u8>, InnerDir, u64) {
    let (data, graph) = crate::test_support::test_container::outer_container_catpart(b"graph");
    let outer = crate::container::parse_outer_stream_directory(&data).expect("complete directory");
    (data, outer, graph)
}

#[test]
fn full_uuid_metadata_names_a_separate_content_stream() {
    let (data, outer, graph) = source();
    let declarations = outer_container_declarations(&data, &outer);
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].class_name, "CATPrtCont");
    assert_eq!(
        declarations[0].stream_name,
        "Cont_4bbc295c_1048_62eb7b6f_1825"
    );
    assert!(outer_container_for_extent(&outer, &declarations, graph, 5).is_some());
    assert!(outer_container_for_extent(&outer, &declarations, graph - 1, 6).is_none());
    let metadata = outer
        .descriptors
        .iter()
        .find(|d| d.name == "4bbc295c_1048_62eb7b6f_1825")
        .unwrap();
    assert!(outer_container_for_extent(
        &outer,
        &declarations,
        u64::from(metadata.extents[0].phys_off),
        1
    )
    .is_none());
}

#[test]
fn duplicate_content_descriptors_or_metadata_do_not_resolve() {
    let (data, outer, _) = source();
    for index in [1, 2] {
        let mut duplicate = outer.clone();
        duplicate
            .descriptors
            .push(duplicate.descriptors[index].clone());
        assert!(outer_container_declarations(&data, &duplicate).is_empty());
    }
}

#[test]
fn the_length_cell_and_full_uuid_are_required() {
    let (mut data, outer, _) = source();
    let directory = index_from_u32(
        View::u32_be_at(&data, crate::layout::outer_header::DIRECTORY_OFFSET).unwrap(),
    );
    let ds = directory + outer.descriptors[1].desc_offset;
    let prefix = ds - ("4bbc295c_1048_62eb7b6f_1825".len() * 2 + 5);
    data[prefix] -= 2;
    assert!(outer_container_declarations(&data, &outer).is_empty());
}

#[test]
fn malformed_or_duplicate_content_properties_do_not_resolve() {
    let (data, outer, _) = source();
    let metadata = &outer.descriptors[1];
    let range = contiguous_descriptor_range(metadata, 0, data.len()).unwrap();
    let original = &data[range];
    assert_eq!(
        content_name(original, |name| name.starts_with("Cont_")).as_deref(),
        Some("Cont_4bbc295c_1048_62eb7b6f_1825")
    );
    for (index, value) in [(3, 0), (11, 4), (21, 0), (25, 0), (original.len() - 1, 1)] {
        let mut malformed = original.to_vec();
        malformed[index] = value;
        assert!(
            content_name(&malformed, |name| name.starts_with("Cont_")).is_none(),
            "offset {index}"
        );
    }
    let mut duplicate = original.to_vec();
    duplicate.extend_from_slice(&original[114..]);
    let size = u32::try_from(duplicate.len()).unwrap();
    duplicate[..4].copy_from_slice(&size.to_be_bytes());
    duplicate[8..12].copy_from_slice(&4u32.to_be_bytes());
    assert!(content_name(&duplicate, |name| name.starts_with("Cont_")).is_none());
}

#[test]
fn property_identities_do_not_assign_the_content_role() {
    let (mut data, outer, _) = source();
    let range = contiguous_descriptor_range(&outer.descriptors[1], 0, data.len()).unwrap();
    let metadata = &mut data[range];
    metadata[4..8].copy_from_slice(&11u32.to_be_bytes());
    for (offset, identity) in [(16usize, 14u32), (64, 13), (118, 12)] {
        metadata[offset..offset + 4].copy_from_slice(&identity.to_be_bytes());
    }
    assert_eq!(outer_container_declarations(&data, &outer).len(), 1);
}

#[test]
fn a_named_but_missing_content_stream_does_not_resolve() {
    let (data, mut outer, _) = source();
    outer.descriptors.pop();
    assert!(outer_container_declarations(&data, &outer).is_empty());
}

#[test]
fn two_record_metadata_has_the_same_explicit_content_link() {
    let (data, outer, _) = source();
    let range = contiguous_descriptor_range(&outer.descriptors[1], 0, data.len()).unwrap();
    let original = &data[range];
    let mut compact = original[..12].to_vec();
    compact.extend_from_slice(&original[60..]);
    let size = u32::try_from(compact.len()).unwrap();
    compact[..4].copy_from_slice(&size.to_be_bytes());
    compact[8..12].copy_from_slice(&2u32.to_be_bytes());
    assert_eq!(
        content_name(&compact, |name| name == "Cont_4bbc295c_1048_62eb7b6f_1825").as_deref(),
        Some("Cont_4bbc295c_1048_62eb7b6f_1825")
    );
}

fn text_record(units: impl IntoIterator<Item = u16>) -> Vec<u8> {
    let value: Vec<u8> = units
        .into_iter()
        .chain(std::iter::once(0))
        .flat_map(u16::to_be_bytes)
        .collect();
    let width = u32::try_from(value.len()).unwrap();
    let mut record = (14 + width).to_be_bytes().to_vec();
    record.extend_from_slice(&16u32.to_be_bytes());
    record.extend_from_slice(&2u16.to_be_bytes());
    record.extend_from_slice(&width.to_be_bytes());
    record.extend(value);
    record
}

fn table_with_leading(leading: &[Vec<u8>], properties: &[u8], count: u32) -> Vec<u8> {
    let (data, outer, _) = source();
    let range = contiguous_descriptor_range(&outer.descriptors[1], 0, data.len()).unwrap();
    let mut table = data[range][..12].to_vec();
    for record in leading {
        table.extend_from_slice(record);
    }
    table.extend_from_slice(properties);
    let size = u32::try_from(table.len()).unwrap();
    table[..4].copy_from_slice(&size.to_be_bytes());
    table[8..12].copy_from_slice(&count.to_be_bytes());
    table
}

#[test]
fn leading_container_names_select_no_content() {
    let (data, outer, _) = source();
    let range = contiguous_descriptor_range(&outer.descriptors[1], 0, data.len()).unwrap();
    let original = &data[range];
    let leading = text_record("Cont_0_51ac_66443b70_3eab9".encode_utf16());
    let target = |name: &str| name == "Cont_4bbc295c_1048_62eb7b6f_1825";
    for names in [
        vec![leading.clone()],
        vec![leading.clone(), leading.clone()],
    ] {
        let count = u32::try_from(names.len()).unwrap() + 3;
        let table = table_with_leading(&names, &original[12..], count);
        assert_eq!(
            content_name(&table, target).as_deref(),
            Some("Cont_4bbc295c_1048_62eb7b6f_1825")
        );
    }
    let prefixed =
        text_record(std::iter::once(3).chain("Cont_0_51ac_66443b70_3eab9".encode_utf16()));
    for name in [
        prefixed,
        text_record("CATFeatCont".encode_utf16()),
        text_record("Cont_".encode_utf16()),
    ] {
        let table = table_with_leading(&[name], &original[12..], 4);
        assert!(content_name(&table, target).is_none());
    }
    let without_sets = table_with_leading(&[leading], &original[60..], 3);
    assert!(content_name(&without_sets, target).is_none());
}

#[test]
fn changing_uuid_word_zero_cannot_keep_the_old_owner() {
    let (mut data, outer, _) = source();
    let marker = b"\x03\0\xf7\0\x03\0\0\0";
    let at = data
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .unwrap()
        + marker.len();
    data[at] ^= 1;
    assert!(outer_container_declarations(&data, &outer).is_empty());
}

#[test]
fn an_overlapping_descriptor_candidate_cannot_own_a_name_prefix() {
    let (data, mut outer, _) = source();
    let mut overlap = outer.descriptors[2].clone();
    overlap.desc_offset -= 1;
    outer.descriptors.push(overlap);
    assert!(outer_container_declarations(&data, &outer).is_empty());
}

#[test]
fn a_name_wider_than_the_directory_length_field_is_rejected_before_lookup() {
    let (data, outer, _) = source();
    let range = contiguous_descriptor_range(&outer.descriptors[1], 0, data.len()).unwrap();
    let original = &data[range];
    let mut table = original[..12].to_vec();
    table.extend_from_slice(&original[60..114]);
    let mut value = Vec::new();
    for _ in 0..32767 {
        value.extend_from_slice(&[0, b'x']);
    }
    value.extend_from_slice(&[0, 0]);
    let width = u32::try_from(value.len()).unwrap();
    table.extend_from_slice(&(14 + width).to_be_bytes());
    table.extend_from_slice(&16u32.to_be_bytes());
    table.extend_from_slice(&2u16.to_be_bytes());
    table.extend_from_slice(&width.to_be_bytes());
    table.extend(value);
    let size = u32::try_from(table.len()).unwrap();
    table[..4].copy_from_slice(&size.to_be_bytes());
    table[8..12].copy_from_slice(&2u32.to_be_bytes());
    assert!(content_name(&table, |_| panic!(
        "u16 directory length cannot name this target"
    ))
    .is_none());
}
