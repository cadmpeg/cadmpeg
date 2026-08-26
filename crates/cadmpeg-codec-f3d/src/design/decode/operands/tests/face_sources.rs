// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports, clippy::default_trait_access, clippy::wildcard_imports)]

use super::prelude::*;
use crate::design::decode::operands::{
    face_source_carrier_layout, parse_face_source_carrier_prefix,
};

fn indexed_header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
}

fn marked_reference(bytes: &mut [u8], offset: usize, record_index: u32) {
    bytes[offset] = 1;
    bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
}

fn source_carrier(
    class_tag: &[u8; 3],
    record_index: u32,
    scope_record_index: u32,
    source_count: usize,
    scalar_offset: usize,
    discriminator: u32,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, class_tag, record_index);
    bytes.resize(scalar_offset + 16, 0);
    marked_reference(&mut bytes, 21, scope_record_index);
    bytes[32..36].copy_from_slice(&(source_count as u32).to_le_bytes());
    for ordinal in 0..source_count {
        let offset = 36 + ordinal * 11;
        marked_reference(&mut bytes, offset, 200 + ordinal as u32);
    }
    bytes[scalar_offset..scalar_offset + 4].copy_from_slice(&discriminator.to_le_bytes());
    bytes[scalar_offset + 4..scalar_offset + 12].copy_from_slice(&0.125f64.to_le_bytes());
    bytes[scalar_offset + 12..scalar_offset + 16].copy_from_slice(&discriminator.to_le_bytes());
    bytes
}

#[test]
fn face_source_carriers_use_generation_keyed_prefixes() {
    for (class_tag, source_count, scalar_offset, discriminator, paired_class_tag) in [
        (*b"398", 4, 80, 100, "462"),
        (*b"394", 2, 58, 109, "311"),
        (*b"356", 2, 58, 109, "309"),
    ] {
        let layout = face_source_carrier_layout(class_tag_str(&class_tag)).unwrap();
        assert_eq!(layout.source_count, source_count);
        assert_eq!(layout.scalar_offset, scalar_offset);
        assert_eq!(layout.scalar_discriminator, discriminator);
        assert_eq!(layout.paired_class_tag, paired_class_tag);

        let bytes = source_carrier(
            &class_tag,
            100,
            12,
            source_count,
            scalar_offset,
            discriminator,
        );
        let references = parse_face_source_carrier_prefix(&bytes, 0, 12, layout).unwrap();
        assert_eq!(
            references,
            (0..source_count)
                .map(|ordinal| (36 + ordinal * 11, 200 + ordinal as u32))
                .collect::<Vec<_>>()
        );
    }
}

fn class_tag_str(class_tag: &[u8; 3]) -> &str {
    std::str::from_utf8(class_tag).unwrap()
}

#[test]
fn face_source_carrier_prefix_rejects_wrong_count_and_discriminator() {
    let layout = face_source_carrier_layout("398").unwrap();
    let mut bytes = source_carrier(b"398", 100, 12, 4, 80, 100);

    bytes[32..36].copy_from_slice(&3u32.to_le_bytes());
    assert!(parse_face_source_carrier_prefix(&bytes, 0, 12, layout).is_none());

    let layout = face_source_carrier_layout("398").unwrap();
    let mut bytes = source_carrier(b"398", 100, 12, 4, 80, 100);
    bytes[80..84].copy_from_slice(&101u32.to_le_bytes());
    assert!(parse_face_source_carrier_prefix(&bytes, 0, 12, layout).is_none());
}
