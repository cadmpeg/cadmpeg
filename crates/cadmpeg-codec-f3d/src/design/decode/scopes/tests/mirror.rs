// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::compact_feature_reference;
use super::prelude::*;

fn indexed_header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) -> usize {
    let start = bytes.len();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
    start
}

fn utf16(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(
        &u32::try_from(value.encode_utf16().count())
            .expect("test GUID length fits u32")
            .to_le_bytes(),
    );
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
}

#[test]
fn compact_mirror_reference_uses_the_identity_record_lane() {
    let record_index = 40;
    let reference = 17_u32;
    let mut bytes = Vec::new();
    let start = indexed_header(&mut bytes, *b"320", record_index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&(record_index + 3).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    utf16(&mut bytes, "dfa12ed5-41e3-47c2-947d-286843e235df");
    utf16(&mut bytes, "15afb570-2968-417f-8485-96c81b2d332f");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"259", record_index);
    indexed_header(&mut bytes, *b"306", record_index + 1);
    indexed_header(&mut bytes, *b"291", record_index + 2);
    let identity = indexed_header(&mut bytes, *b"428", record_index + 3);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&reference.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"457", record_index + 4);
    let header = DesignRecordHeader {
        id: String::new(),
        record_index,
        class_tag: "320".into(),
        byte_offset: start as u64,
    };

    assert_eq!(
        compact_feature_reference(&bytes, &header),
        Some((reference, (identity + 21) as u64))
    );
    bytes[identity + 20] = 1;
    assert_eq!(compact_feature_reference(&bytes, &header), None);
}
