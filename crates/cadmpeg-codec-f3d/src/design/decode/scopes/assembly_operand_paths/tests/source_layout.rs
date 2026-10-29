// SPDX-License-Identifier: Apache-2.0
//! Actual path fields and source-order admission.

use super::{identity_qualified_path_bytes, path_link, push_guid};
use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn paired_path_bytes() -> Vec<u8> {
    let plain = identity_qualified_path_bytes(*b"366", true);
    let mut bytes = plain[..plain.len() - 8].to_vec();
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    for unit in "43".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    push_guid(&mut bytes, "66666666-6666-6666-6666-666666666666");
    bytes
}

#[test]
fn native_path_keeps_ordered_identity_values_without_source_coordinates() {
    let bytes = paired_path_bytes();
    let arena = DecodeArena::new();
    let (_ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let parsed = super::super::exact_assembly_operand_path(
        source,
        0,
        65,
        bytes.len(),
        path_link(),
        Some(TYPED_OCCURRENCE_PATH_TYPE_GUID),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        serde_json::to_value(parsed.content).unwrap(),
        serde_json::json!({
            "link": {"locator_record_index":64, "locator_class_tag":"304",
                "wrapper_record_index":66, "wrapper_class_tag":"382"},
            "record_index":65, "class_tag":"366", "type_guid":TYPED_OCCURRENCE_PATH_TYPE_GUID,
            "occurrence_guids":["11111111-1111-1111-1111-111111111111"],
            "identity_guids":["22222222-2222-2222-2222-222222222222",
                "33333333-3333-3333-3333-333333333333", "44444444-4444-4444-4444-444444444444",
                "55555555-5555-5555-5555-555555555555"],
            "paired_occurrence_keys":["43", "66666666-6666-6666-6666-666666666666"]
        })
    );
}

#[test]
fn source_order_admission_rejects_reversed_actual_record_fields() {
    let first = super::path_bytes();
    let mut bytes = first.clone();
    bytes.extend_from_slice(&first);
    let arena = DecodeArena::new();
    let (_ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let early =
        super::super::exact_assembly_operand_path(source, 0, 65, first.len(), path_link(), None)
            .unwrap()
            .unwrap();
    let mut late = super::super::exact_assembly_operand_path(
        source,
        first.len(),
        65,
        bytes.len(),
        path_link(),
        None,
    )
    .unwrap()
    .unwrap();
    assert!(late.source.append(early.source).is_none());
}
