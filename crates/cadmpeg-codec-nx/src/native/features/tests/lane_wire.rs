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
        r#"{"id":"lane","operation_label":"operation","values":[2.5,4.0],"raw_values":[[1,2,3,4,5,6,7,8],[8,7,6,5,4,3,2,1]],"first_witness_offsets":[10,18],"second_witness_offsets":[40,48]}"#,
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
        r#"{"id":"lane","operation_label":"operation","construction_payload":"payload","ordinal":0,"discriminator":[1,2],"values":[2.5,4.0],"raw_values":[[1,2,3,4],[5,6,7,8]],"value_payload_offsets":[2,6],"terminator_payload_offset":10,"source_offset":100,"value_source_offsets":[102,106],"terminator_source_offset":110}"#,
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
        r#"{"id":"lane","operation_label":"operation","graph_payload":"payload","ordinal":0,"discriminator":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18],"branch":3,"values":[2.5,4.0],"raw_values":[[1,2,3,4],[5,6,7,8]],"payload_offset":0,"value_payload_offsets":[18,22],"source_offset":100,"value_source_offsets":[118,122]}"#,
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
        r#"{"id":"lane","operation_label":"operation","declared_count":3,"selectors":[7,8],"raw_selectors":[[7],[8]],"ordinals":[1,2],"row_indices":[3,4],"instance_count":2,"trailing_object_indices":[9],"raw_trailing_object_indices":[[9]],"source_offset":100,"selector_source_offsets":[110,120],"trailing_object_index_source_offsets":[130]}"#,
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
    check_lane_wire::<FeatureIdenticalInstanceOutputLane>(
        r#"{"id":"lane","operation_label":"operation","leading_schema_index":4,"count_schema_index":5,"row_schema_indices":[6,7,8],"declared_count":3,"selectors":[7,8],"raw_selectors":[[7],[8]],"source_offset":100,"selector_source_offsets":[110,120]}"#,
        &["selectors", "raw_selectors", "selector_source_offsets"],
    );
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
        r#"{"id":"lane","operation_label":"operation","construction_header":"header","data_blocks":["first","second"],"values":[1.0,2.0,3.0,4.0,5.0,6.0],"raw_values":[[1,2,3,4,5,6,7,8],[2,3,4,5,6,7,8,9],[3,4,5,6,7,8,9,10],[4,5,6,7,8,9,10,11],[5,6,7,8,9,10,11,12],[6,7,8,9,10,11,12,13]],"source_offsets":[100,110,120,200,210,220]}"#,
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
        r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"scalar_rows","declared_count":3,"encodings":["binary32","binary64"],"values":[2.5,4.0],"raw_values":[[1,2,3,4],[1,2,3,4,5,6,7,8]],"selectors":[7,8],"raw_selectors":[[7],[8]],"source_offset":100,"value_source_offsets":[110,120],"selector_source_offsets":[114,128]}"#,
        columns,
    );
    check_lane_wire::<FeaturePatternTransformLane>(
        r#"{"id":"lane","operation_label":"operation","row_schema_index":3,"layout":"wide_rows","declared_count":2,"encodings":["binary64","binary64","binary64","binary64","exact_one"],"values":[2.5,4.0,5.0,6.0,1.0],"raw_values":[[1,2,3,4,5,6,7,8],[2,3,4,5,6,7,8,9],[3,4,5,6,7,8,9,10],[4,5,6,7,8,9,10,11],[1]],"selectors":[7],"raw_selectors":[[7]],"source_offset":100,"value_source_offsets":[110,118,126,134,142],"selector_source_offsets":[143]}"#,
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
