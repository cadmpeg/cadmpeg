// SPDX-License-Identifier: Apache-2.0
//! Byte identity and native admission for borrowed OM record wires.

use super::{
    DataBlockControlClassReference, DataBlockControlClassReferenceWire, DataBlockControlForm,
    DataBlockControlFormWire, DataBlockReference, DataBlockReferenceWire, ObjectRecord,
    ObjectRecordWire, StoreHeader, StoreHeaderWire,
};

fn check<T: serde::Serialize + serde::de::DeserializeOwned>(wire: &str) -> T {
    let record: T = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_vec(&record).unwrap(), wire.as_bytes());
    record
}

#[test]
fn object_record_borrowed_wire_preserves_bytes() {
    let wire = format!(
        "{{\"id\":\"nx:om:object-record#0\",\"object_id\":2,\"object_id_source_offset\":4,\"section_ordinal\":0,\"record_ordinal\":0,\"section_offset\":0,\"byte_len\":4,\"sha256\":\"{}\",\"dependencies\":[\"a\"],\"dependents\":[\"b\"],\"source_entry\":\"entry\",\"source_offset\":4}}",
        "0".repeat(64)
    );
    let record = check::<ObjectRecord>(&wire);
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&ObjectRecordWire::from(record.clone())).unwrap()
    );
}

#[test]
fn object_record_native_limit_refuses_before_clone() {
    let wire = format!(
        "{{\"id\":\"nx:om:object-record#0\",\"object_id\":2,\"object_id_source_offset\":4,\"section_ordinal\":0,\"record_ordinal\":0,\"section_offset\":0,\"byte_len\":4,\"sha256\":\"{}\",\"source_entry\":\"entry\",\"source_offset\":4}}",
        "0".repeat(64)
    );
    let record = check::<ObjectRecord>(&wire);
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::from_str::<serde_json::Value>(&wire).unwrap(),
    );
}

#[test]
fn control_form_borrowed_wire_preserves_bytes() {
    for wire in [
        r#"{"id":"nx:om:control-form#0","data_block":"block","kind":"zero_prefixed","value_count":2,"byte_len":8,"source_offset":0}"#,
        r#"{"id":"nx:om:control-form#0","data_block":"block","kind":"product_anchored","value_count":2,"leading_value_width":2,"leading_value":0,"byte_len":26,"source_offset":0}"#,
    ] {
        let record = check::<DataBlockControlForm>(wire);
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&DataBlockControlFormWire::from(record.clone())).unwrap()
        );
    }
}

#[test]
fn control_form_native_limit_refuses_before_clone() {
    let wire = r#"{"id":"nx:om:control-form#0","data_block":"block","kind":"zero_prefixed","value_count":2,"byte_len":8,"source_offset":0}"#;
    let record = check::<DataBlockControlForm>(wire);
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::from_str::<serde_json::Value>(wire).unwrap(),
    );
}

#[test]
fn control_class_borrowed_wire_preserves_bytes() {
    let wire = r#"{"id":"nx:om:control-class#0","data_block":"block","ordinal":0,"class_ordinal":1,"class_definition":"class","class_name":"UGS::Part","source_offset":4}"#;
    let record = check::<DataBlockControlClassReference>(wire);
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&DataBlockControlClassReferenceWire::from(record.clone())).unwrap()
    );
}

#[test]
fn control_class_native_limit_refuses_before_clone() {
    let wire = r#"{"id":"nx:om:control-class#0","data_block":"block","ordinal":0,"class_ordinal":1,"class_definition":"class","class_name":"UGS::Part","source_offset":4}"#;
    let record = check::<DataBlockControlClassReference>(wire);
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::from_str::<serde_json::Value>(wire).unwrap(),
    );
}

#[test]
fn block_reference_borrowed_wire_preserves_bytes() {
    let wire = r#"{"id":"nx:om:block-reference#0","data_block":"block","ordinal":0,"object_id":0,"raw_object_id":[0],"target_record":"record","target_expression_declaration":"declaration","source_offset":4}"#;
    let record = check::<DataBlockReference>(wire);
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&DataBlockReferenceWire::from(record.clone())).unwrap()
    );
}

#[test]
fn block_reference_native_limit_refuses_before_clone() {
    let wire = r#"{"id":"nx:om:block-reference#0","data_block":"block","ordinal":0,"object_id":0,"raw_object_id":[0],"target_record":"record","target_expression_declaration":"declaration","source_offset":4}"#;
    let record = check::<DataBlockReference>(wire);
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::from_str::<serde_json::Value>(wire).unwrap(),
    );
}

#[test]
fn store_header_borrowed_wire_preserves_bytes() {
    for (wire, expected_id) in [
        (
            r#"{"object_id":null,"id":"nx:om:header#0","section_ordinal":0,"version":"NX 1","source_entry":"entry","source_offset":4}"#,
            None,
        ),
        (
            r#"{"object_id":2,"id":"nx:om:header#0","section_ordinal":0,"version":"NX 1","source_entry":"entry","source_offset":4}"#,
            Some(2),
        ),
    ] {
        let record = check::<StoreHeader>(wire);
        assert_eq!(record.header().id, "nx:om:header#0");
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&StoreHeaderWire::from(record.clone())).unwrap()
        );
        assert_eq!(
            matches!(record, StoreHeader::Fixed(_)),
            expected_id.is_some()
        );
    }
}

#[test]
fn store_header_native_limit_refuses_before_clone() {
    let wire = r#"{"object_id":2,"id":"nx:om:header#0","section_ordinal":0,"version":"NX 1","source_entry":"entry","source_offset":4}"#;
    let record = check::<StoreHeader>(wire);
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::from_str::<serde_json::Value>(wire).unwrap(),
    );
}

#[test]
fn data_block_reference_wire_preserves_feature_token_and_rejects_mismatch() {
    for (value, raw) in [
        (0, vec![0]),
        (0, vec![0x80, 0]),
        (0, vec![0x90, 0, 0]),
        (6466, vec![0x90, 0x19, 0x42]),
    ] {
        let wire = serde_json::json!({"id":"reference", "data_block":"block", "ordinal":0,
            "object_id":value, "raw_object_id":raw, "source_offset":12});
        let record: super::DataBlockReference = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }
    for (value, raw) in [
        (1, vec![0]),
        (0, vec![0xff]),
        (0, vec![0xf0, 0]),
        (0, vec![0x90, 0]),
        (0, vec![0, 0]),
    ] {
        let wire = serde_json::json!({"id":"reference", "data_block":"block", "ordinal":0,
            "object_id":value, "raw_object_id":raw, "source_offset":12});
        assert!(serde_json::from_value::<super::DataBlockReference>(wire)
            .unwrap_err()
            .to_string()
            .contains("object_id/raw_object_id"));
    }
}
