// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

use super::{
    decode_sketch_visibilities_in_stream, decode_sketch_visibility_member,
    CURRENT_SKETCH_CONTAINER_VERSION, SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID,
    SKETCH_CONTAINER_MEMBER_TYPE_GUID, SKETCH_CONTAINER_MEMBER_VERSION, SKETCH_CONTAINER_TYPE_GUID,
};

const ENTITY_SUFFIX: u64 = 201;

fn member(stream_ordinal: u32, visible: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"256");
    bytes.extend_from_slice(&ENTITY_SUFFIX.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.push(1);
    bytes.extend_from_slice(&203u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&stream_ordinal.to_le_bytes());
    bytes.push(0);
    bytes.push(visible);
    bytes.push(1);
    bytes
}

#[test]
fn sketch_visibility_member_decodes_both_boolean_values() {
    let hidden =
        decode_sketch_visibility_member(&member(1, 0), 0, ENTITY_SUFFIX).expect("hidden member");
    assert_eq!(hidden.stream_ordinal, 1);
    assert_eq!(hidden.stream_ordinal_offset, 30);
    assert_eq!(hidden.visible_offset, 35);
    assert!(!hidden.visible);

    let visible =
        decode_sketch_visibility_member(&member(513, 1), 0, ENTITY_SUFFIX).expect("visible member");
    assert_eq!(visible.stream_ordinal, 513);
    assert!(visible.visible);
}

#[test]
fn sketch_visibility_member_rejects_invalid_ordinal_or_owner() {
    assert!(decode_sketch_visibility_member(&member(1, 2), 0, ENTITY_SUFFIX).is_none());
    assert!(decode_sketch_visibility_member(&member(0, 1), 0, ENTITY_SUFFIX).is_none());
    assert!(decode_sketch_visibility_member(&member(1, 1), 0, ENTITY_SUFFIX + 1).is_none());

    let mut external_owner = member(1, 1);
    external_owner[28] = 1;
    assert!(decode_sketch_visibility_member(&external_owner, 0, ENTITY_SUFFIX).is_none());
}

#[test]
fn sketch_visibility_accepts_settled_container_header() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"256");
    bytes.extend_from_slice(&ENTITY_SUFFIX.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    bytes.push(0);
    bytes.extend_from_slice(&5u32.to_le_bytes());
    for unit in "0_201".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let mut visibility = member(1, 1);
    visibility[4..7].copy_from_slice(b"257");
    bytes.extend_from_slice(&visibility);

    let segment_type = |type_guid: &str,
                        base_type_guid: Option<&str>,
                        version: u32,
                        module: &str,
                        entity_ids: Vec<u64>| {
        crate::records::SegmentType {
            id: String::new(),
            byte_offset: 0,
            type_guid: type_guid.into(),
            type_guid_offset: 0,
            base_type_guid: base_type_guid.map(str::to_owned),
            base_type_guid_offset: base_type_guid.map(|_| 0),
            version,
            version_offset: 0,
            module: module.into(),
            entity_id_offsets: vec![0; entity_ids.len()],
            entity_ids,
        }
    };
    let metadata = crate::metastream::MetaStream {
        types: vec![
            segment_type(
                SKETCH_CONTAINER_TYPE_GUID,
                Some(SKETCH_CONTAINER_MEMBER_TYPE_GUID),
                CURRENT_SKETCH_CONTAINER_VERSION,
                DESIGN_MODULE_SKETCH,
                vec![ENTITY_SUFFIX],
            ),
            segment_type(
                SKETCH_CONTAINER_MEMBER_TYPE_GUID,
                Some(SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID),
                SKETCH_CONTAINER_MEMBER_VERSION,
                "Geometry",
                Vec::new(),
            ),
        ],
        records: vec![crate::metastream::RecordIndexEntry {
            entity_id: ENTITY_SUFFIX,
            bulk_offset: 0,
        }],
        secondary_records: Vec::new(),
    };

    let visibilities =
        decode_sketch_visibilities_in_stream(&bytes, &metadata).expect("settled header");
    assert_eq!(visibilities.len(), 1);
    assert_eq!(visibilities[0].0, ENTITY_SUFFIX);
    assert!(visibilities[0].1.visible);
}
