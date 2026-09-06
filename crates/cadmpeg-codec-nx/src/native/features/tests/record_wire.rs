// SPDX-License-Identifier: Apache-2.0

use super::{FeatureBodyReference, FeatureOperationObjectReference};

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
