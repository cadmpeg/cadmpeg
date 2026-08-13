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

use super::decode_sketch_visibility_member;

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
