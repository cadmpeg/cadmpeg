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

#[test]
fn parameter_scope_parses_named_tail_with_empty_label() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"378");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "CylinderPrimitive");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);

    bytes.push(1);
    bytes.push(0x0f);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.push(1);
    bytes.push(0x0e);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 0, 0]);
    bytes.push(1);
    bytes.push(0x0d);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);

    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "378".into(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("empty-label named scope");
    assert_eq!(
        scope.kind(),
        crate::records::DesignFeatureKind::CylinderPrimitive
    );
    assert_eq!(scope.frame_length, paired_at as u64);
    assert_eq!(scope.previous_history_state_id, None);
    assert_eq!(scope.previous_history_state_id_offset, None);
}
