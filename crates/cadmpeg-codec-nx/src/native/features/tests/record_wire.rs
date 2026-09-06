// SPDX-License-Identifier: Apache-2.0

use super::{FeatureBodyReference, FeatureOperationBodyWrite, FeatureOperationObjectReference};

#[test]
fn object_reference_retains_tagged_and_untagged_wire() {
    for json in [
        r#"{"id":"reference","operation_label":"operation","operation_record":"record","ordinal":0,"tag":2,"object_index":3,"raw_object_index":[3],"object_index_source_offset":10,"byte_len":4,"source_offset":7}"#,
        r#"{"id":"reference","operation_label":"operation","operation_record":"record","ordinal":0,"object_index":3,"raw_object_index":[3],"object_index_source_offset":10,"byte_len":4,"source_offset":7}"#,
    ] {
        let reference: FeatureOperationObjectReference = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&reference).unwrap(), json);
    }
}

#[test]
fn body_reference_retains_primary_and_ordered_wire() {
    for json in [
        r#"{"id":"reference","operation_label":"operation","body_object_index":3,"raw_body_object_index":[3],"source_offset":10}"#,
        r#"{"id":"reference","operation_label":"operation","ordinal":0,"body_object_index":3,"raw_body_object_index":[3],"source_offset":10}"#,
    ] {
        let reference: FeatureBodyReference = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&reference).unwrap(), json);
    }
}

#[test]
fn body_write_retains_labeled_and_unlabeled_wire() {
    for json in [
        r#"{"id":"write","operation_label":"operation","operation_record":"record","ordinal":0,"body_identity":1,"group_node":2,"raw_group_node":[2],"group_node_source_offset":10,"endpoint_tag":16,"body_image_object_index":3,"raw_body_image_object_index":[3],"body_image_object_index_source_offset":11,"byte_len":4,"source_offset":8}"#,
        r#"{"id":"write","operation_record":"record","ordinal":0,"body_identity":1,"group_node":2,"raw_group_node":[2],"group_node_source_offset":10,"endpoint_tag":16,"body_image_object_index":3,"raw_body_image_object_index":[3],"body_image_object_index_source_offset":11,"byte_len":4,"source_offset":8}"#,
    ] {
        let write: FeatureOperationBodyWrite = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&write).unwrap(), json);
    }
}

#[test]
fn payload_scalar_preserves_each_payload_owner_key() {
    for json in [
        r#"{"id":"scalar","operation_label":"operation","datum_csys_payload":"payload","ordinal":0,"field_code":100,"value":2.0,"raw_value":[1,2,3,4,5,6,7,8],"payload_offset":10,"source_offset":20}"#,
        r#"{"id":"scalar","operation_label":"operation","construction_payload":"payload","ordinal":0,"field_code":100,"value":2.0,"raw_value":[1,2,3,4,5,6,7,8],"payload_offset":10,"source_offset":20}"#,
    ] {
        let scalar: super::FeaturePayloadScalar = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&scalar).unwrap(), json);
    }
}

#[test]
fn scalar_pair_preserves_payload_key_and_discriminator_presence() {
    for json in [
        r#"{"id":"pair","operation_label":"operation","datum_csys_payload":"payload","ordinal":0,"values":[1.0,2.0],"raw_values":[[1,2,3,4,5,6,7,8],[8,7,6,5,4,3,2,1]],"payload_offset":10,"value_payload_offsets":[11,19],"source_offset":30,"value_source_offsets":[31,39],"discriminator":[4]}"#,
        r#"{"id":"pair","operation_label":"operation","datum_plane_payload":"payload","ordinal":0,"values":[1.0,2.0],"raw_values":[[1,2,3,4,5,6,7,8],[8,7,6,5,4,3,2,1]],"payload_offset":10,"value_payload_offsets":[11,19],"source_offset":30,"value_source_offsets":[31,39]}"#,
        r#"{"id":"pair","operation_label":"operation","construction_payload":"payload","ordinal":0,"values":[1.0,2.0],"raw_values":[[1,2,3,4,5,6,7,8],[8,7,6,5,4,3,2,1]],"payload_offset":10,"value_payload_offsets":[11,19],"source_offset":30,"value_source_offsets":[31,39],"discriminator":[8,2,3,1,3,1]}"#,
        r#"{"id":"pair","operation_label":"operation","surface_construction_payload":"payload","ordinal":0,"values":[1.0,2.0],"raw_values":[[1,2,3,4,5,6,7,8],[8,7,6,5,4,3,2,1]],"payload_offset":10,"value_payload_offsets":[11,19],"source_offset":30,"value_source_offsets":[31,39],"discriminator":[]}"#,
    ] {
        let pair: super::FeaturePayloadScalarPair = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&pair).unwrap(), json);
    }
}

#[test]
fn construction_payload_preserves_each_ownership_form() {
    for owner in [
        r#""construction_inputs":"inputs""#,
        r#""operation_kind":"CPROJ","construction_references":["reference"]"#,
        r#""operation_kind":"CPROJ_CMB","construction_references":["reference"]"#,
        r#""reference_graph":"graph","group":"first""#,
        r#""reference_graph":"graph","group":"second""#,
        r#""operation_kind":"Pattern Feature","reference_layout":"canonical_graph","construction_references":["reference"]"#,
        r#""operation_kind":"Pattern Geometry","reference_layout":"compact_graph","construction_references":["reference"]"#,
        r#""index_lane":"lane""#,
        r#""construction":"construction""#,
    ] {
        let json = format!(r#"{{"id":"payload","operation_label":"operation",{owner},"data_blocks":["block"],"byte_len":8,"sha256":"hash","block_payload_offsets":[0],"block_byte_lengths":[8],"block_source_offsets":[10]}}"#);
        let payload: super::FeatureConstructionPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&payload).unwrap(), json);
    }
}

#[test]
fn construction_payload_rejects_untyped_operation_kinds() {
    let json = r#"{"id":"payload","operation_label":"operation","operation_kind":"other","construction_references":[],"data_blocks":[],"byte_len":0,"sha256":"hash","block_payload_offsets":[],"block_byte_lengths":[],"block_source_offsets":[]}"#;
    let error = serde_json::from_str::<super::FeatureConstructionPayload>(json).unwrap_err();
    assert!(error.to_string().contains("operation_kind"));
}

#[test]
fn datum_plane_payload_derives_terminal_index_count() {
    let json = r#"{"id":"payload","operation_label":"operation","datum_plane_header":"header","data_blocks":["block"],"byte_len":8,"sha256":"hash","block_payload_offsets":[0],"block_byte_lengths":[8],"block_source_offsets":[10],"index_lane_offset":2,"index_lane_declared_count":2,"index_lane_values":[1],"index_lane_raw_indices":[[1]],"index_lane_value_offsets":[4],"index_lane_trailer":0}"#;
    let payload: super::FeatureDatumPlanePayload = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&payload).unwrap(), json);
    for count in [0, 1, 3, 256] {
        let invalid = json.replace("\"index_lane_declared_count\":2", &format!("\"index_lane_declared_count\":{count}"));
        let error = serde_json::from_str::<super::FeatureDatumPlanePayload>(&invalid).unwrap_err();
        assert!(error.to_string().contains("index_lane_declared_count"));
    }
}

#[test]
fn symbolic_thread_text_frame_derives_marker() {
    let json = r#"{"id":"frame","symbolic_thread":"thread","ordinal":0,"marker":3,"value":"CUT","source_offset":10}"#;
    let frame: super::FeatureSymbolicThreadTextFrame = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&frame).unwrap(), json);
    for marker in [0, 4, 255] {
        let invalid = json.replace("\"marker\":3", &format!("\"marker\":{marker}"));
        let error = serde_json::from_str::<super::FeatureSymbolicThreadTextFrame>(&invalid).unwrap_err();
        assert!(error.to_string().contains("marker"));
    }
}
