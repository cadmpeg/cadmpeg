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
