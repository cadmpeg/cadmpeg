// SPDX-License-Identifier: Apache-2.0

use super::*;

use crate::native::features::test_support::check_lane_wire;

#[test]
fn draft_fixed_lane_preserves_parallel_wire_and_requires_complete_tokens() {
    check_lane_wire::<FeatureDraftConstructionFixedLane>(
        r#"{"id":"lane","operation_label":"operation","graph_payload":"payload","ordinal":0,"values":[0.25,0.5],"markers":[48,176],"raw_values":[[32,0,0,0,0,0,0],[64,0,0,0,0,0,0]],"payload_offset":0,"value_payload_offsets":[18,26],"source_offset":100,"value_source_offsets":[118,126]}"#,
        &[
            "values",
            "markers",
            "raw_values",
            "value_payload_offsets",
            "value_source_offsets",
        ],
    );
}

#[test]
fn draft_binary32_lane_preserves_parallel_wire_and_requires_complete_tokens() {
    check_lane_wire::<FeatureDraftConstructionBinary32Lane>(
        r#"{"id":"lane","operation_label":"operation","graph_payload":"payload","ordinal":0,"discriminator":[144,24,69,1,4,1,3,1,192,69,4,0,128,134,2,0,3,0],"branch":3,"values":[2.5,4.0],"raw_values":[[80,32,0,0],[80,128,0,0]],"payload_offset":0,"value_payload_offsets":[18,22],"source_offset":100,"value_source_offsets":[118,122]}"#,
        &[
            "values",
            "raw_values",
            "value_payload_offsets",
            "value_source_offsets",
        ],
    );
}

#[test]
fn draft_index_lane_preserves_wire_and_groups_resolution_with_tokens() {
    check_lane_wire::<FeatureDraftConstructionIndexLane>(
        r#"{"id":"lane","operation_label":"operation","declared_count":3,"indices":[7,8],"raw_indices":[[7],[8]],"data_blocks":["first","second"],"source_offsets":[110,111]}"#,
        &["indices", "raw_indices", "data_blocks", "source_offsets"],
    );
    check_lane_wire::<FeatureDraftConstructionIndexLane>(
        r#"{"id":"lane","operation_label":"operation","declared_count":3,"indices":[7,8],"raw_indices":[[7],[8]],"source_offsets":[110,111]}"#,
        &["indices", "raw_indices", "source_offsets"],
    );
}

#[test]
fn draft_index_lane_checks_compact_token_grammar() {
    let json = r#"{"id":"lane","operation_label":"operation","declared_count":3,"indices":[4096,7],"raw_indices":[[144,0],[128,7]],"source_offsets":[110,112]}"#;
    let lane: FeatureDraftConstructionIndexLane = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&lane).unwrap(), json);
    for raw in [vec![255], vec![144], vec![144, 0, 0], vec![1]] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["raw_indices"][0] = serde_json::json!(raw);
        let error = serde_json::from_value::<FeatureDraftConstructionIndexLane>(wire).unwrap_err();
        assert!(error.to_string().contains("indices[0]"));
    }
}

#[test]
fn draft_index_lane_requires_nonempty_byte_counted_members() {
    for count in [0, 1, 254, 255] {
        let wire = serde_json::json!({
            "id": "lane", "operation_label": "operation", "declared_count": count + 1,
            "indices": vec![1; count], "raw_indices": vec![vec![1]; count],
            "source_offsets": (100..100 + count).collect::<Vec<_>>(),
        });
        assert_eq!(
            serde_json::from_value::<FeatureDraftConstructionIndexLane>(wire).is_ok(),
            matches!(count, 1 | 254)
        );
    }
}

#[test]
fn draft_terminal_lane_derives_its_source_offset() {
    let json = r#"{"id":"lane","operation_label":"operation","indices":[128,129],"raw_indices":[[128,128],[128,129]],"tail":[1,2,3],"index_source_offsets":[100,102],"source_offset":100}"#;
    check_lane_wire::<FeatureDraftConstructionTerminalLane>(json, &[]);
    let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
    malformed["source_offset"] = serde_json::json!(101);
    let error =
        serde_json::from_value::<FeatureDraftConstructionTerminalLane>(malformed).unwrap_err();
    assert!(error.to_string().contains("source_offset"));
}

#[test]
fn draft_terminal_lane_requires_two_byte_compact_indices() {
    let json = r#"{"id":"lane","operation_label":"operation","indices":[4096,1],"raw_indices":[[144,0],[128,1]],"tail":[1,2,3],"index_source_offsets":[100,102],"source_offset":100}"#;
    check_lane_wire::<FeatureDraftConstructionTerminalLane>(json, &[]);
    for (value, raw) in [
        (4096, [255, 0]),
        (4096, [127, 0]),
        (1, [1, 0]),
        (1, [128, 2]),
    ] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["indices"][0] = serde_json::json!(value);
        wire["raw_indices"][0] = serde_json::json!(raw);
        let error =
            serde_json::from_value::<FeatureDraftConstructionTerminalLane>(wire).unwrap_err();
        assert!(error.to_string().contains("indices[0]"));
    }
}

#[test]
fn draft_binary32_lane_rejects_inconsistent_or_wrong_width_atoms() {
    let json = r#"{"id":"lane","operation_label":"operation","graph_payload":"payload","ordinal":0,"discriminator":[144,24,69,1,4,1,3,1,192,69,4,0,128,134,2,0,3,0],"branch":3,"values":[2.5],"raw_values":[[80,32,0,0]],"payload_offset":0,"value_payload_offsets":[18],"source_offset":100,"value_source_offsets":[118]}"#;
    let original: serde_json::Value = serde_json::from_str(json).unwrap();
    for (value, raw) in [
        (4.0, vec![80, 32, 0, 0]),
        (2.5, vec![48, 4, 0, 0, 0, 0, 0, 0]),
        (0.0, vec![0, 0, 0, 0]),
    ] {
        let mut invalid = original.clone();
        invalid["values"][0] = serde_json::json!(value);
        invalid["raw_values"][0] = serde_json::json!(raw);
        assert!(serde_json::from_value::<FeatureDraftConstructionBinary32Lane>(invalid).is_err());
    }
}

#[test]
fn draft_identity_frame_derives_prefix_form_and_preserves_wire() {
    let json = r#"{"id":"frame","operation_label":"operation","draft_construction_payload":"payload","ordinal":0,"prefix":[65,129,84,240,56,2,1],"form":{"kind":"indexed_branch","first_index":340,"second_index":56,"branch":2},"identity":"abc123","payload_offset":1,"identity_payload_offset":8,"source_offset":100,"identity_source_offset":500}"#;
    let frame: FeatureDraftConstructionIdentityFrame = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&frame).unwrap(), json);
    for (field, value, expected) in [
        ("identity", serde_json::json!(""), "identity"),
        ("identity", serde_json::json!("ABC123"), "identity"),
        (
            "prefix",
            serde_json::json!([65, 129, 84, 240, 56, 3, 1]),
            "form",
        ),
        (
            "prefix",
            serde_json::json!([65, 129, 84, 240, 56, 2, 1, 0]),
            "prefix",
        ),
        (
            "identity_payload_offset",
            serde_json::json!(9),
            "identity_payload_offset",
        ),
        (
            "payload_offset",
            serde_json::json!(u64::MAX),
            "payload_offset",
        ),
    ] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire[field] = value;
        let error =
            serde_json::from_value::<FeatureDraftConstructionIdentityFrame>(wire).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn draft_terminal_lane_rejects_detached_second_index_and_frame_overflow() {
    let json = r#"{"id":"lane","operation_label":"operation","indices":[128,129],"raw_indices":[[128,128],[128,129]],"tail":[1,2,3],"index_source_offsets":[100,102],"source_offset":100}"#;
    for offsets in [[100, 101], [100, 103], [100, u64::MAX]] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["index_source_offsets"] = serde_json::json!(offsets);
        let error =
            serde_json::from_value::<FeatureDraftConstructionTerminalLane>(wire).unwrap_err();
        assert!(error.to_string().contains("index_source_offsets"));
    }
    let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
    let origin = u64::MAX - 19;
    wire["source_offset"] = serde_json::json!(origin);
    wire["index_source_offsets"] = serde_json::json!([origin, origin + 2]);
    let lane: FeatureDraftConstructionTerminalLane = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(lane).unwrap(), wire);
    wire["source_offset"] = serde_json::json!(origin + 1);
    wire["index_source_offsets"] = serde_json::json!([origin + 1, origin + 3]);
    let error = serde_json::from_value::<FeatureDraftConstructionTerminalLane>(wire).unwrap_err();
    assert!(error.to_string().contains("source_offset"));
}

#[test]
fn draft_index_lane_rejects_detached_offsets_and_frame_overflow() {
    let json = r#"{"id":"lane","operation_label":"operation","declared_count":3,"indices":[7,8],"raw_indices":[[128,7],[8]],"source_offsets":[110,112]}"#;
    let lane: FeatureDraftConstructionIndexLane = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&lane).unwrap(), json);
    for offsets in [[110, 113], [23, 25], [u64::MAX - 4, u64::MAX - 2]] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["source_offsets"] = serde_json::json!(offsets);
        let error = serde_json::from_value::<FeatureDraftConstructionIndexLane>(wire).unwrap_err();
        assert!(error.to_string().contains("source_offsets"));
    }
    let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
    wire["source_offsets"] = serde_json::json!([u64::MAX - 5, u64::MAX - 3]);
    assert!(serde_json::from_value::<FeatureDraftConstructionIndexLane>(wire).is_ok());
}
