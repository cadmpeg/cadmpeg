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
