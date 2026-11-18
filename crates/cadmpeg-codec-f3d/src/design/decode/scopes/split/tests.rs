// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::feature::scope::DesignFeatureKind;

fn extension_fixture(property_block_present: bool, extend: u8) -> (Vec<u8>, usize) {
    let mut bytes = b"\x03\0\0\0".to_vec();
    bytes.extend(b"339");
    bytes.extend(77u32.to_le_bytes());
    bytes.extend([0; 9]);
    bytes.push(u8::from(property_block_present));
    if property_block_present {
        bytes.extend(0u32.to_le_bytes());
    }
    let offset = bytes.len();
    bytes.push(extend);
    bytes.extend(1u32.to_le_bytes());
    (bytes, offset)
}

#[test]
fn split_tool_extension_decodes_both_boolean_values() {
    for property_block_present in [false, true] {
        for extend in [false, true] {
            let start = 12;
            let (fixture, offset) = extension_fixture(property_block_present, u8::from(extend));
            let mut bytes = vec![0xa5; start];
            bytes.extend(fixture);
            if property_block_present {
                bytes.extend([1, 13, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            } else {
                bytes.push(0);
            }
            assert_eq!(
                read_extension(&bytes, u64_from_index(start), u64_from_index(bytes.len())),
                Some((
                    Located {
                        value: extend,
                        offset: u64_from_index(start + offset),
                    },
                    property_block_present,
                )),
            );
        }
    }
}

#[test]
fn split_tool_extension_refuses_invalid_or_out_of_bounds_lanes() {
    let (mut invalid_flag, _) = extension_fixture(true, 2);
    invalid_flag.push(0);
    let (mut wrong_word, flag) = extension_fixture(true, 1);
    wrong_word[flag + 1] = 2;
    wrong_word.push(0);
    let (mut wrong_reference, _) = extension_fixture(true, 1);
    wrong_reference.extend([1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let (mut truncated_reference, _) = extension_fixture(true, 1);
    truncated_reference.extend([1, 13]);
    let (missing_reference, _) = extension_fixture(true, 1);
    for bytes in [
        invalid_flag,
        wrong_word,
        wrong_reference,
        truncated_reference,
        missing_reference,
    ] {
        assert_eq!(read_extension(&bytes, 0, u64_from_index(bytes.len())), None);
    }

    let (mut bytes, _) = extension_fixture(true, 1);
    let end = u64_from_index(bytes.len());
    bytes.extend([0; 16]);
    assert_eq!(read_extension(&bytes, 0, end), None);
}

#[test]
fn split_tool_extension_preserves_absent_block_value_with_a_loss() {
    let (mut bytes, offset) = extension_fixture(false, 0);
    bytes.push(0);
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        DesignFeatureKind::Split,
        77,
    );
    scope
        .try_edit(|draft| {
            draft.reference_count_offset = u64_from_index(bytes.len());
            draft.layout_fixture_references();
            draft.frame_length = draft.kind_offset + 96;
            draft.paired_byte_offset = draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut losses = Vec::new();
    decode_split_tool_extension(&bytes, &mut scope, &mut losses);
    let value = serde_json::to_value(&scope).unwrap();
    assert_eq!(
        value["split_tool_extension"],
        serde_json::json!({"value": false, "offset": offset})
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureSplitToolExtensionAssumed.kind()
    );
    let decoded: DesignParameterScope = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded, scope);

    let mut wrong_offset = value;
    wrong_offset["split_tool_extension"]["offset"] = serde_json::json!(offset + 1);
    assert!(serde_json::from_value::<DesignParameterScope>(wrong_offset).is_err());

    bytes[offset] = 2;
    losses.clear();
    decode_split_tool_extension(&bytes, &mut scope, &mut losses);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureSplitToolExtensionUnresolved.kind()
    );
    assert!(serde_json::to_value(&scope)
        .unwrap()
        .get("split_tool_extension")
        .is_none());
}
