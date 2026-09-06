// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_lane_wire<T>(json: &str, columns: &[&str])
where
    T: serde::de::DeserializeOwned + Serialize,
{
    let lane: T = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&lane).unwrap(), json);
    for column in columns {
        let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
        malformed[*column].as_array_mut().unwrap().pop();
        assert!(serde_json::from_value::<T>(malformed).is_err(), "{column}");
    }
}

#[test]
fn repeated_scalar_lane_preserves_parallel_wire_and_requires_complete_tokens() {
    check_lane_wire::<FeatureSimpleHoleRepeatedScalarLane>(
        r#"{"id":"lane","operation_label":"operation","values":[2.5,4.0],"raw_values":[[48,4,0,0,0,0,0,0],[48,16,0,0,0,0,0,0]],"first_witness_offsets":[10,18],"second_witness_offsets":[40,48]}"#,
        &[
            "values",
            "raw_values",
            "first_witness_offsets",
            "second_witness_offsets",
        ],
    );
}

#[test]
fn hole_group_preserves_parallel_wire_and_requires_complete_members() {
    check_lane_wire::<FeatureSimpleHoleConstructionGroup>(
        r#"{"id":"group","first_data_blocks":["a","b"],"second_data_blocks":["c","d"],"operation_labels":["first","second"],"scalar_lanes":["scalar-a","scalar-b"],"block_references":["refs-a","refs-b"]}"#,
        &["operation_labels", "scalar_lanes", "block_references"],
    );
}

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
        r#"{"id":"lane","operation_label":"operation","construction_payload":"payload","ordinal":0,"discriminator":[1,2],"values":[2.5,4.0],"raw_values":[[80,32,0,0],[80,128,0,0]],"value_payload_offsets":[2,6],"terminator_payload_offset":10,"source_offset":100,"value_source_offsets":[102,106],"terminator_source_offset":110}"#,
        &[
            "values",
            "raw_values",
            "value_payload_offsets",
            "value_source_offsets",
        ],
    );
}

#[test]
fn pattern_fixed_lane_preserves_parallel_wire_and_requires_complete_tokens() {
    check_lane_wire::<FeaturePatternConstructionFixedLane>(
        r#"{"id":"lane","operation_label":"operation","construction_payload":"payload","ordinal":0,"values":[0.25,0.5],"markers":[48,49],"raw_values":[[1,2,3,4,5,6,7],[7,6,5,4,3,2,1]],"payload_offset":0,"value_payload_offsets":[2,10],"source_offset":100,"value_source_offsets":[102,110]}"#,
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
fn draft_fixed_lane_preserves_parallel_wire_and_requires_complete_tokens() {
    check_lane_wire::<FeatureDraftConstructionFixedLane>(
        r#"{"id":"lane","operation_label":"operation","graph_payload":"payload","ordinal":0,"values":[0.25,0.5],"markers":[48,49],"raw_values":[[1,2,3,4,5,6,7],[7,6,5,4,3,2,1]],"payload_offset":0,"value_payload_offsets":[2,10],"source_offset":100,"value_source_offsets":[102,110]}"#,
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
fn multi_instance_lane_preserves_wire_and_requires_complete_rows_and_references() {
    check_lane_wire::<FeatureMultiInstanceOutputLane>(
        r#"{"id":"lane","operation_label":"operation","declared_count":3,"selectors":[7,8],"raw_selectors":[[7],[8]],"ordinals":[1,2],"row_indices":[2,3],"instance_count":2,"trailing_object_indices":[9],"raw_trailing_object_indices":[[9]],"source_offset":100,"selector_source_offsets":[110,120],"trailing_object_index_source_offsets":[130]}"#,
        &[
            "selectors",
            "raw_selectors",
            "ordinals",
            "row_indices",
            "selector_source_offsets",
            "trailing_object_indices",
            "raw_trailing_object_indices",
            "trailing_object_index_source_offsets",
        ],
    );
}

#[test]
fn identical_instance_lane_preserves_wire_and_requires_complete_selectors() {
    let json = r#"{"id":"lane","operation_label":"operation","leading_schema_index":4,"count_schema_index":5,"row_schema_indices":[6,7,8],"declared_count":3,"selectors":[7,8],"raw_selectors":[[7],[8]],"source_offset":100,"selector_source_offsets":[110,120]}"#;
    check_lane_wire::<FeatureIdenticalInstanceOutputLane>(
        json,
        &["selectors", "raw_selectors", "selector_source_offsets"],
    );
    for (field, value) in [
        ("count_schema_index", serde_json::json!(253)),
        ("row_schema_indices", serde_json::json!([6, 7, 9])),
    ] {
        let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
        malformed[field] = value;
        let error = serde_json::from_value::<FeatureIdenticalInstanceOutputLane>(malformed)
            .unwrap_err();
        assert!(error.to_string().contains(field));
    }
}

#[test]
fn terminal_discriminator_preserves_wire_and_requires_complete_tokens() {
    check_lane_wire::<FeatureOperationTerminalDiscriminator>(
        r#"{"id":"lane","operation_label":"operation","type_indices":[7,8],"raw_type_indices":[[7],[8]],"type_index_source_offsets":[110,120],"flags":[1,2,3,4],"trailing_indices":[9,10],"raw_trailing_indices":[[9],[10]],"trailing_index_source_offsets":[130,140],"source_offset":100}"#,
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
fn point_scalar_lane_preserves_wire_and_requires_six_complete_tokens() {
    check_lane_wire::<FeaturePointConstructionScalarLane>(
        r#"{"id":"lane","operation_label":"operation","construction_header":"header","data_blocks":["first","second"],"values":[1.0,2.0,3.0,4.0,5.0,6.0],"raw_values":[[47,240,0,0,0,0,0,0],[48,0,0,0,0,0,0,0],[48,8,0,0,0,0,0,0],[48,16,0,0,0,0,0,0],[48,20,0,0,0,0,0,0],[48,24,0,0,0,0,0,0]],"source_offsets":[100,110,120,200,210,220]}"#,
        &["values", "raw_values", "source_offsets"],
    );
}

#[test]
fn draft_index_lane_preserves_wire_and_groups_resolution_with_tokens() {
    check_lane_wire::<FeatureDraftConstructionIndexLane>(
        r#"{"id":"lane","operation_label":"operation","declared_count":3,"indices":[7,8],"raw_indices":[[7],[8]],"data_blocks":["first","second"],"source_offsets":[110,120]}"#,
        &["indices", "raw_indices", "data_blocks", "source_offsets"],
    );
    check_lane_wire::<FeatureDraftConstructionIndexLane>(
        r#"{"id":"lane","operation_label":"operation","declared_count":3,"indices":[7,8],"raw_indices":[[7],[8]],"source_offsets":[110,120]}"#,
        &["indices", "raw_indices", "source_offsets"],
    );
}

#[test]
fn pattern_transform_lane_preserves_wire_and_requires_complete_rows() {
    let columns = &[
        "encodings",
        "values",
        "raw_values",
        "selectors",
        "raw_selectors",
        "value_source_offsets",
        "selector_source_offsets",
    ];
    check_lane_wire::<FeaturePatternTransformLane>(
        r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"scalar_rows","declared_count":3,"encodings":["binary32","binary64"],"values":[2.5,4.0],"raw_values":[[80,32,0,0],[48,16,0,0,0,0,0,0]],"selectors":[7,8],"raw_selectors":[[7],[8]],"source_offset":100,"value_source_offsets":[110,120],"selector_source_offsets":[114,128]}"#,
        columns,
    );
    check_lane_wire::<FeaturePatternTransformLane>(
        r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"wide_rows","declared_count":2,"encodings":["binary64","binary64","binary64","binary64","exact_one"],"values":[2.5,4.0,5.0,6.0,1.0],"raw_values":[[48,4,0,0,0,0,0,0],[48,16,0,0,0,0,0,0],[48,20,0,0,0,0,0,0],[48,24,0,0,0,0,0,0],[1]],"selectors":[7],"raw_selectors":[[7]],"source_offset":100,"value_source_offsets":[110,118,126,134,142],"selector_source_offsets":[143]}"#,
        columns,
    );
}

#[test]
fn extrude_32_branch_preserves_wire_and_requires_complete_reference_tokens() {
    check_lane_wire::<FeatureExtrudePayload32Branch>(
        r#"{"id":"branch","operation_label":"operation","body_object_index":42,"scalar":1.0,"raw_scalar":[47,240,0,0,0,0,0,0],"atoms_be":[1031799040],"atom_source_offsets":[20],"atom_indices":[1],"atom_data_blocks":["block#1"],"first_indices":[2],"raw_first_indices":[[2]],"first_index_source_offsets":[21],"first_data_blocks":[null],"second_indices":[3],"raw_second_indices":[[3]],"second_index_source_offsets":[22],"second_data_blocks":["block#3"],"terminal_object_index":42,"raw_terminal_object_index":[42],"terminal_source_offset":23,"source_offset":20}"#,
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
fn draft_terminal_lane_derives_its_source_offset() {
    let json = r#"{"id":"lane","operation_label":"operation","indices":[128,129],"raw_indices":[[128,128],[128,129]],"tail":[1,2,3],"index_source_offsets":[100,102],"source_offset":100}"#;
    check_lane_wire::<FeatureDraftConstructionTerminalLane>(json, &[]);
    let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
    malformed["source_offset"] = serde_json::json!(101);
    let error = serde_json::from_value::<FeatureDraftConstructionTerminalLane>(malformed)
        .unwrap_err();
    assert!(error.to_string().contains("source_offset"));
}

#[test]
fn pattern_rows_reject_scalar_families_outside_their_layout() {
    let narrow = r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"scalar_rows","declared_count":2,"encodings":["exact_one"],"values":[1.0],"raw_values":[[1]],"selectors":[7],"raw_selectors":[[7]],"source_offset":100,"value_source_offsets":[110],"selector_source_offsets":[111]}"#;
    assert!(serde_json::from_str::<FeaturePatternTransformLane>(narrow).is_err());
    let wide = r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"wide_rows","declared_count":2,"encodings":["binary64","binary64","binary64","binary64","exact_one"],"values":[2.5,4.0,5.0,6.0,1.0],"raw_values":[[48,4,0,0,0,0,0,0],[48,16,0,0,0,0,0,0],[48,20,0,0,0,0,0,0],[48,24,0,0,0,0,0,0],[1]],"selectors":[7],"raw_selectors":[[7]],"source_offset":100,"value_source_offsets":[110,118,126,134,142],"selector_source_offsets":[143]}"#;
    let mut wrong_first: serde_json::Value = serde_json::from_str(wide).unwrap();
    wrong_first["encodings"][0] = serde_json::json!("binary32");
    wrong_first["raw_values"][0] = serde_json::json!([80,32,0,0]);
    assert!(serde_json::from_value::<FeaturePatternTransformLane>(wrong_first).is_err());
    let mut wrong_terminal: serde_json::Value = serde_json::from_str(wide).unwrap();
    wrong_terminal["encodings"][4] = serde_json::json!("binary64");
    wrong_terminal["raw_values"][4] = serde_json::json!([47,240,0,0,0,0,0,0]);
    assert!(serde_json::from_value::<FeaturePatternTransformLane>(wrong_terminal).is_err());
    assert!(serde_json::from_str::<FeaturePatternTransformLane>(&wide.replace("\"row_schema_index\":3", "\"row_schema_index\":0")).is_err());
}

#[test]
fn sketch_scalar_lane_rejects_inconsistent_or_zero_atoms() {
    let json = r#"{"id":"lane","operation_label":"operation","construction_payload":"payload","ordinal":0,"discriminator":[1,2],"values":[2.5],"raw_values":[[80,32,0,0]],"value_payload_offsets":[2],"terminator_payload_offset":6,"source_offset":100,"value_source_offsets":[102],"terminator_source_offset":106}"#;
    let original: serde_json::Value = serde_json::from_str(json).unwrap();
    for (value, raw) in [
        (4.0, vec![80,32,0,0]),
        (2.5, vec![80,32,0,0,0]),
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
fn draft_binary32_lane_rejects_inconsistent_or_wrong_width_atoms() {
    let json = r#"{"id":"lane","operation_label":"operation","graph_payload":"payload","ordinal":0,"discriminator":[144,24,69,1,4,1,3,1,192,69,4,0,128,134,2,0,3,0],"branch":3,"values":[2.5],"raw_values":[[80,32,0,0]],"payload_offset":0,"value_payload_offsets":[18],"source_offset":100,"value_source_offsets":[118]}"#;
    let original: serde_json::Value = serde_json::from_str(json).unwrap();
    for (value, raw) in [
        (4.0, vec![80,32,0,0]),
        (2.5, vec![48,4,0,0,0,0,0,0]),
        (0.0, vec![0,0,0,0]),
    ] {
        let mut invalid = original.clone();
        invalid["values"][0] = serde_json::json!(value);
        invalid["raw_values"][0] = serde_json::json!(raw);
        assert!(serde_json::from_value::<FeatureDraftConstructionBinary32Lane>(invalid).is_err());
    }
}

#[test]
fn repeated_scalar_lane_rejects_empty_and_inconsistent_atoms() {
    let empty = r#"{"id":"lane","operation_label":"operation","values":[],"raw_values":[],"first_witness_offsets":[],"second_witness_offsets":[]}"#;
    assert!(serde_json::from_str::<FeatureSimpleHoleRepeatedScalarLane>(empty).unwrap_err().to_string().contains("values"));
    let invalid = r#"{"id":"lane","operation_label":"operation","values":[4.0],"raw_values":[[48,4,0,0,0,0,0,0]],"first_witness_offsets":[10],"second_witness_offsets":[40]}"#;
    assert!(serde_json::from_str::<FeatureSimpleHoleRepeatedScalarLane>(invalid).unwrap_err().to_string().contains("raw_values"));
}
