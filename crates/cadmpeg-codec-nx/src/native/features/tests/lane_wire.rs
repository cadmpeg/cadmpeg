// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::native::features::extrude_32::FeatureExtrudePayload32Branch;
use crate::native::features::terminal_discriminator::FeatureOperationTerminalDiscriminator;

#[test]
fn input_identity_group_preserves_parallel_wire_and_requires_complete_members() {
    check_lane_wire::<FeatureInputBlockIdentityGroup>(
        r#"{"id":"group","data_block":"block","input_blocks":["input-a","input-b"],"operation_labels":["first","second"],"input_slots":[2,1],"source_offsets":[10,40]}"#,
        &[
            "input_blocks",
            "operation_labels",
            "input_slots",
            "source_offsets",
        ],
    );
}

#[test]
fn sketch_scalar_lane_preserves_parallel_wire_and_requires_complete_tokens() {
    check_lane_wire::<FeatureSketchPayloadScalarLane>(
        r#"{"id":"lane","operation_label":"operation","construction_payload":"payload","ordinal":0,"discriminator":[37,37,65,0,4,1,7,1,192,69,16,0,128,134,2,0,1,0],"values":[2.5,4.0],"raw_values":[[80,32,0,0],[80,128,0,0]],"value_payload_offsets":[18,22],"terminator_payload_offset":26,"source_offset":100,"value_source_offsets":[118,122],"terminator_source_offset":126}"#,
        &[
            "values",
            "raw_values",
            "value_payload_offsets",
            "value_source_offsets",
        ],
    );
}

#[test]
fn terminal_discriminator_preserves_wire_and_requires_complete_tokens() {
    check_lane_wire::<FeatureOperationTerminalDiscriminator>(
        r#"{"id":"lane","operation_label":"operation","type_indices":[7,8],"raw_type_indices":[[7],[8]],"type_index_source_offsets":[103,104],"flags":[1,2,3,4],"trailing_indices":[9,10],"raw_trailing_indices":[[9],[10]],"trailing_index_source_offsets":[118,119],"source_offset":100}"#,
        &[
            "type_indices",
            "raw_type_indices",
            "type_index_source_offsets",
            "trailing_indices",
            "raw_trailing_indices",
            "trailing_index_source_offsets",
        ],
    );
}

#[test]
fn terminal_discriminator_rejects_inconsistent_compact_tokens() {
    let json = r#"{"id":"lane","operation_label":"operation","type_indices":[4096,8],"raw_type_indices":[[144,0],[8]],"type_index_source_offsets":[103,105],"flags":[1,2,3,4],"trailing_indices":[9],"raw_trailing_indices":[[128,9]],"trailing_index_source_offsets":[119],"source_offset":100}"#;
    check_lane_wire::<FeatureOperationTerminalDiscriminator>(json, &[]);
    for (field, raw_field, raw) in [
        (
            "type_indices",
            "raw_type_indices",
            serde_json::json!([[144, 0, 0], [8]]),
        ),
        (
            "type_indices",
            "raw_type_indices",
            serde_json::json!([[255], [8]]),
        ),
        (
            "type_indices",
            "raw_type_indices",
            serde_json::json!([[144], [8]]),
        ),
        (
            "type_indices",
            "raw_type_indices",
            serde_json::json!([[7], [8]]),
        ),
        (
            "trailing_indices",
            "raw_trailing_indices",
            serde_json::json!([[10]]),
        ),
        (
            "trailing_indices",
            "raw_trailing_indices",
            serde_json::json!([[9, 0]]),
        ),
        (
            "trailing_indices",
            "raw_trailing_indices",
            serde_json::json!([[255]]),
        ),
    ] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire[raw_field] = raw;
        let error =
            serde_json::from_value::<FeatureOperationTerminalDiscriminator>(wire).unwrap_err();
        assert!(error.to_string().contains(field));
    }
}

#[test]
fn point_scalar_lane_preserves_wire_and_requires_six_complete_tokens() {
    check_lane_wire::<FeaturePointConstructionScalarLane>(
        r#"{"id":"lane","operation_label":"operation","construction_header":"header","data_blocks":["first","second"],"values":[1.0,2.0,3.0,4.0,5.0,6.0],"raw_values":[[47,240,0,0,0,0,0,0],[48,0,0,0,0,0,0,0],[48,8,0,0,0,0,0,0],[48,16,0,0,0,0,0,0],[48,20,0,0,0,0,0,0],[48,24,0,0,0,0,0,0]],"source_offsets":[100,110,120,200,210,220]}"#,
        &["values", "raw_values", "source_offsets"],
    );
}

#[test]
fn extrude_32_branch_preserves_wire_and_requires_complete_reference_tokens() {
    check_lane_wire::<FeatureExtrudePayload32Branch>(
        r#"{"id":"branch","operation_label":"operation","body_object_index":42,"scalar":1.0,"raw_scalar":[47,240,0,0,0,0,0,0],"atoms_be":[1031799040],"atom_source_offsets":[33],"atom_indices":[1],"atom_data_blocks":["block#1"],"first_indices":[2],"raw_first_indices":[[2]],"first_index_source_offsets":[39],"first_data_blocks":[null],"second_indices":[3],"raw_second_indices":[[3]],"second_index_source_offsets":[42],"second_data_blocks":["block#3"],"terminal_object_index":42,"raw_terminal_object_index":[42],"terminal_source_offset":45,"source_offset":20}"#,
        &[
            "atoms_be",
            "atom_source_offsets",
            "atom_indices",
            "atom_data_blocks",
            "first_indices",
            "raw_first_indices",
            "first_index_source_offsets",
            "first_data_blocks",
            "second_indices",
            "raw_second_indices",
            "second_index_source_offsets",
            "second_data_blocks",
        ],
    );
}

#[test]
fn block_dimensions_preserve_wire_and_require_three_complete_parameters() {
    check_lane_wire::<FeatureBlockDimensions>(
        r#"{"id":"dimensions","operation_label":"operation","construction":"construction","anchor_bindings":["binding"],"declarations":["d1","d2","d3"],"expressions":["e1","e2","e3"],"values":[1.0,2.0,3.0]}"#,
        &["declarations", "expressions", "values"],
    );
}

#[test]
fn boolean_operation_preserves_wire_and_requires_complete_tools() {
    check_lane_wire::<FeatureBooleanOperation>(
        r#"{"id":"boolean","operation_label":"operation","kind":"subtract","target_object_index":10,"raw_target_object_index":[10],"target_source_offset":100,"tool_object_indices":[20,30],"raw_tool_object_indices":[[20],[30]],"tool_source_offsets":[110,120],"source_offset":90}"#,
        &[
            "tool_object_indices",
            "raw_tool_object_indices",
            "tool_source_offsets",
        ],
    );
}

#[test]
fn boolean_operation_rejects_inconsistent_reference_tokens() {
    let json = r#"{"id":"boolean","operation_label":"operation","kind":"subtract","target_object_index":256,"raw_target_object_index":[144,1,0],"target_source_offset":100,"tool_object_indices":[20,256],"raw_tool_object_indices":[[240,20],[241,1,0]],"tool_source_offsets":[110,120],"source_offset":90}"#;
    check_lane_wire::<FeatureBooleanOperation>(json, &[]);
    for (field, value, error_field) in [
        (
            "target_object_index",
            serde_json::json!(257),
            "target_object_index",
        ),
        (
            "raw_target_object_index",
            serde_json::json!([144, 1, 0, 0]),
            "target_object_index",
        ),
        (
            "raw_target_object_index",
            serde_json::json!([255]),
            "target_object_index",
        ),
        (
            "tool_object_indices",
            serde_json::json!([21, 256]),
            "tool_object_indices",
        ),
        (
            "raw_tool_object_indices",
            serde_json::json!([[240, 20], [241, 0, 0]]),
            "tool_object_indices",
        ),
    ] {
        let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
        malformed[field] = value;
        let error = serde_json::from_value::<FeatureBooleanOperation>(malformed).unwrap_err();
        assert!(error.to_string().contains(error_field));
    }
}

#[test]
fn sketch_scalar_lane_rejects_inconsistent_or_zero_atoms() {
    let json = r#"{"id":"lane","operation_label":"operation","construction_payload":"payload","ordinal":0,"discriminator":[37,37,65,0,4,1,7,1,192,69,16,0,128,134,2,0,1,0],"values":[2.5],"raw_values":[[80,32,0,0]],"value_payload_offsets":[18],"terminator_payload_offset":22,"source_offset":100,"value_source_offsets":[118],"terminator_source_offset":122}"#;
    let original: serde_json::Value = serde_json::from_str(json).unwrap();
    for (value, raw) in [
        (4.0, vec![80, 32, 0, 0]),
        (2.5, vec![80, 32, 0, 0, 0]),
        (0.0, vec![0]),
    ] {
        let mut invalid = original.clone();
        invalid["values"][0] = serde_json::json!(value);
        invalid["raw_values"][0] = serde_json::json!(raw);
        let error = serde_json::from_value::<FeatureSketchPayloadScalarLane>(invalid).unwrap_err();
        assert!(error.to_string().contains("raw_values"));
    }
}

#[test]
fn datum_fixed_pair_preserves_values_and_raw_values_wire() {
    check_lane_wire::<FeatureDatumCsysPayloadFixedPair>(
        r#"{"id":"pair","operation_label":"operation","datum_csys_payload":"payload","ordinal":0,"values":[0.5,-0.5],"raw_values":[[64,0,0,0,0,0,0],[192,0,0,0,0,0,0]],"discriminator":[11,2,3,1,3,1,192,69,4,0,128,134,2,0,3],"payload_offset":0,"value_payload_offsets":[15,24],"source_offset":100,"value_source_offsets":[115,124]}"#,
        &[
            "values",
            "raw_values",
            "value_payload_offsets",
            "value_source_offsets",
        ],
    );
}

#[test]
fn q155_lanes_reject_mismatched_values_and_unknown_markers() {
    let pattern = r#"{"id":"lane","operation_label":"operation","construction_payload":"payload","ordinal":0,"values":[0.5],"markers":[48],"raw_values":[[64,0,0,0,0,0,0]],"payload_offset":0,"value_payload_offsets":[18],"source_offset":100,"value_source_offsets":[118]}"#;
    let draft = pattern.replace("construction_payload", "graph_payload");
    for (key, value) in [
        ("values", serde_json::json!([0.25])),
        ("markers", serde_json::json!([49])),
    ] {
        let mut invalid: serde_json::Value = serde_json::from_str(pattern).unwrap();
        invalid[key] = value.clone();
        assert!(serde_json::from_value::<FeaturePatternConstructionFixedLane>(invalid).is_err());
        let mut invalid: serde_json::Value = serde_json::from_str(&draft).unwrap();
        invalid[key] = value;
        assert!(serde_json::from_value::<FeatureDraftConstructionFixedLane>(invalid).is_err());
    }
}

#[test]
fn scaled_sketch_pair_preserves_values_and_raw_values_wire() {
    check_lane_wire::<FeatureSketchPayloadFixedPair>(
        r#"{"id":"pair","operation_label":"operation","construction_payload":"payload","ordinal":0,"values":[0.5,0.75],"raw_values":[[0,0,0,0,0,0,0],[8,0,0,0,0,0,0]],"discriminator":[4,224,72,14,2,3,128,132],"payload_offset":0,"value_payload_offsets":[8,17],"source_offset":100,"value_source_offsets":[108,117]}"#,
        &[
            "values",
            "raw_values",
            "value_payload_offsets",
            "value_source_offsets",
        ],
    );
}

#[test]
fn mixed_sketch_pair_preserves_interleaved_atom_wire() {
    check_lane_wire::<FeatureSketchPayloadMixedPair>(
        r#"{"id":"pair","operation_label":"operation","construction_payload":"payload","ordinal":0,"fixed_value":0.5,"binary32_value":3.25,"fixed_raw_value":[0,0,0,0,0,0,0],"binary32_raw_value":[80,80,0,0],"discriminator":[4,224,72,14,2,3,128,132],"payload_offset":0,"value_payload_offsets":[8,17],"source_offset":100,"value_source_offsets":[108,117]}"#,
        &[
            "fixed_raw_value",
            "binary32_raw_value",
            "value_payload_offsets",
            "value_source_offsets",
        ],
    );
}

#[test]
fn scaled_sketch_pair_rejects_values_inconsistent_with_its_marker() {
    let invalid = r#"{"id":"pair","operation_label":"operation","construction_payload":"payload","ordinal":0,"values":[0.5,-0.5],"raw_values":[[0,0,0,0,0,0,0],[0,0,0,0,0,0,0]],"discriminator":[4,224,72,14,2,3,128,132],"payload_offset":0,"value_payload_offsets":[8,17],"source_offset":100,"value_source_offsets":[108,117]}"#;
    assert!(
        serde_json::from_str::<FeatureSketchPayloadFixedPair>(invalid)
            .unwrap_err()
            .to_string()
            .contains("raw_values")
    );
}

#[test]
fn sketch_scalar_run_derives_payload_positions_across_split_source_blocks() {
    let json = r#"{"id":"lane","operation_label":"operation","construction_payload":"payload","ordinal":0,"discriminator":[37,37,65,0,4,1,7,1,192,69,16,0,128,134,2,0,1,0],"values":[2.5,4.0],"raw_values":[[80,32,0,0],[48,16,0,0,0,0,0,0]],"value_payload_offsets":[18,22],"terminator_payload_offset":30,"source_offset":100,"value_source_offsets":[118,900],"terminator_source_offset":908}"#;
    check_lane_wire::<FeatureSketchPayloadScalarLane>(
        json,
        &[
            "values",
            "raw_values",
            "value_payload_offsets",
            "value_source_offsets",
        ],
    );
    let original: serde_json::Value = serde_json::from_str(json).unwrap();
    for (field, value) in [
        ("value_payload_offsets", serde_json::json!([18, 23])),
        ("terminator_payload_offset", serde_json::json!(31)),
        ("discriminator", serde_json::json!([1, 2])),
        (
            "value_payload_offsets",
            serde_json::json!([u64::MAX - 1, u64::MAX]),
        ),
    ] {
        let mut invalid = original.clone();
        invalid[field] = value;
        let error = serde_json::from_value::<FeatureSketchPayloadScalarLane>(invalid).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }
}

#[test]
fn draft_and_pattern_runs_reject_payload_gaps_and_empty_atoms() {
    let fixed = r#"{"id":"lane","operation_label":"operation","construction_payload":"payload","ordinal":0,"values":[0.25,0.5],"markers":[48,176],"raw_values":[[32,0,0,0,0,0,0],[64,0,0,0,0,0,0]],"payload_offset":0,"value_payload_offsets":[18,27],"source_offset":100,"value_source_offsets":[118,900]}"#;
    assert!(
        serde_json::from_str::<FeaturePatternConstructionFixedLane>(fixed)
            .unwrap_err()
            .to_string()
            .contains("value_payload_offsets")
    );
    let draft_fixed = fixed.replace("construction_payload", "graph_payload");
    assert!(
        serde_json::from_str::<FeatureDraftConstructionFixedLane>(&draft_fixed)
            .unwrap_err()
            .to_string()
            .contains("value_payload_offsets")
    );
    let binary32 = r#"{"id":"lane","operation_label":"operation","graph_payload":"payload","ordinal":0,"discriminator":[144,24,69,1,4,1,3,1,192,69,4,0,128,134,2,0,3,0],"branch":3,"values":[2.5,4.0],"raw_values":[[80,32,0,0],[80,128,0,0]],"payload_offset":0,"value_payload_offsets":[18,23],"source_offset":100,"value_source_offsets":[118,900]}"#;
    assert!(
        serde_json::from_str::<FeatureDraftConstructionBinary32Lane>(binary32)
            .unwrap_err()
            .to_string()
            .contains("value_payload_offsets")
    );
    let mut empty: serde_json::Value = serde_json::from_str(binary32).unwrap();
    for field in [
        "values",
        "raw_values",
        "value_payload_offsets",
        "value_source_offsets",
    ] {
        empty[field] = serde_json::json!([]);
    }
    assert!(
        serde_json::from_value::<FeatureDraftConstructionBinary32Lane>(empty)
            .unwrap_err()
            .to_string()
            .contains("values")
    );
}

#[test]
fn extrude_32_branch_rejects_disagreeing_scalar_and_body_copies() {
    let original: serde_json::Value = serde_json::from_str(r#"{"id":"branch","operation_label":"operation","body_object_index":42,"scalar":1.0,"raw_scalar":[47,240,0,0,0,0,0,0],"atoms_be":[1031799040],"atom_source_offsets":[33],"atom_indices":[1],"atom_data_blocks":["block#1"],"first_indices":[2],"raw_first_indices":[[2]],"first_index_source_offsets":[39],"first_data_blocks":[null],"second_indices":[3],"raw_second_indices":[[3]],"second_index_source_offsets":[42],"second_data_blocks":["block#3"],"terminal_object_index":42,"raw_terminal_object_index":[42],"terminal_source_offset":45,"source_offset":20}"#).unwrap();
    for (field, value) in [
        ("atom_indices", serde_json::json!([2])),
        ("atoms_be", serde_json::json!([0x3dff_0100u32])),
        ("atoms_be", serde_json::json!([0x3d80_0101u32])),
        ("atoms_be", serde_json::json!([0x3c80_0100u32])),
        ("body_object_index", serde_json::json!(43)),
        ("scalar", serde_json::json!(2.0)),
        ("raw_terminal_object_index", serde_json::json!([43])),
        ("raw_terminal_object_index", serde_json::json!([255])),
        ("raw_terminal_object_index", serde_json::json!([42, 0])),
        ("raw_first_indices", serde_json::json!([[3]])),
        ("raw_first_indices", serde_json::json!([[255]])),
        ("raw_first_indices", serde_json::json!([[2, 0]])),
        ("raw_second_indices", serde_json::json!([[2]])),
        ("raw_second_indices", serde_json::json!([[255]])),
        ("raw_second_indices", serde_json::json!([[128]])),
    ] {
        let mut invalid = original.clone();
        invalid[field] = value;
        let error = serde_json::from_value::<FeatureExtrudePayload32Branch>(invalid).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }
}

#[test]
fn datum_plane_descriptor_derives_suffix_and_preserves_native_wire() {
    let json = r#"{"id":"descriptor","operation_label":"operation","datum_plane_header":"header","ordinal":0,"data_block":"block","identity":"012345678901234567890123456789","suffix":[63,65,1,255,2,1,97,98,99,100],"schema_index":1,"label":"abcd","source_offset":10}"#;
    let descriptor: FeatureDatumPlaneDescriptor = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&descriptor).unwrap(), json);
    for (field, value) in [
        ("schema_index", serde_json::json!(2)),
        ("label", serde_json::json!("other")),
        ("identity", serde_json::json!("")),
        ("suffix", serde_json::json!([63, 65])),
    ] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire[field] = value;
        let error = serde_json::from_value::<FeatureDatumPlaneDescriptor>(wire).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }
    let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
    wire["identity"] = serde_json::json!("012345678901234567890123456789?");
    wire["suffix"].as_array_mut().unwrap().remove(0);
    assert!(serde_json::from_value::<FeatureDatumPlaneDescriptor>(wire)
        .unwrap_err()
        .to_string()
        .contains("identity"));
}

#[test]
fn datum_csys_descriptor_preserves_wire_and_rejects_invalid_identity_or_position() {
    let json = r#"{"id":"descriptor","operation_label":"operation","construction":"construction","reference_ordinal":7,"data_block":"block","prefix":[2,1],"identity":"012345678901234567890123456789","suffix":[63,65],"source_offset":10,"identity_source_offset":12}"#;
    let descriptor: FeatureDatumCsysDescriptor = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&descriptor).unwrap(), json);
    for (field, value) in [
        ("reference_ordinal", serde_json::json!(4)),
        ("identity", serde_json::json!("abcd")),
        ("prefix", serde_json::json!([2, 1, 97])),
        ("source_offset", serde_json::json!(u64::MAX)),
        ("identity_source_offset", serde_json::json!(13)),
    ] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire[field] = value;
        let error = serde_json::from_value::<FeatureDatumCsysDescriptor>(wire).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }
}

#[test]
fn operation_body_reference_lanes_preserve_wire_and_enforce_each_grammar() {
    let compact = r#"{"id":"lane","operation_label":"operation","body_reference_ordinal":0,"body_object_index":110,"branch":28,"encoding":"compact_index","object_indices":[4096,28673],"raw_object_indices":[[144,0],[240,1]],"data_blocks":[null,"block"],"source_offsets":[111,113]}"#;
    let payload = r#"{"id":"lane","operation_label":"operation","body_reference_ordinal":0,"body_object_index":110,"branch":17,"encoding":"payload_object_index","object_indices":[1,256],"raw_object_indices":[[240,1],[241,1,0]],"data_blocks":[null,"block"],"source_offsets":[111,113]}"#;
    for json in [compact, payload] {
        let lane: super::FeatureOperationBodyReferenceLane = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&lane).unwrap(), json);
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["object_indices"][0] = serde_json::json!(2);
        let error =
            serde_json::from_value::<super::FeatureOperationBodyReferenceLane>(wire).unwrap_err();
        assert!(error.to_string().contains("object_indices"), "{error}");
    }
    for (json, raw) in [
        (compact, vec![144]),
        (compact, vec![144, 0, 0]),
        (compact, vec![255]),
        (payload, vec![1]),
        (payload, vec![128, 1]),
        (payload, vec![240]),
        (payload, vec![240, 1, 0]),
        (payload, vec![241, 0, 1]),
        (payload, vec![255]),
    ] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["raw_object_indices"][0] = serde_json::json!(raw);
        let error =
            serde_json::from_value::<super::FeatureOperationBodyReferenceLane>(wire).unwrap_err();
        assert!(error.to_string().contains("raw_object_indices"), "{error}");
    }
}

#[test]
fn operation_body_reference_lane_rejects_unknown_branch() {
    let json = serde_json::json!({
        "id": "lane", "operation_label": "operation", "body_reference_ordinal": 0,
        "body_object_index": 110, "branch": 0, "encoding": "compact_index",
        "object_indices": [1], "raw_object_indices": [[1]], "data_blocks": [null],
        "source_offsets": [100]
    });
    let error = serde_json::from_value::<FeatureOperationBodyReferenceLane>(json).unwrap_err();
    assert!(error.to_string().contains("branch"));
}

use crate::native::features::draft::FeatureDraftConstructionBinary32Lane;
use crate::native::features::draft::FeatureDraftConstructionFixedLane;

use crate::native::features::pattern::FeaturePatternConstructionFixedLane;

use crate::native::features::test_support::check_lane_wire;
