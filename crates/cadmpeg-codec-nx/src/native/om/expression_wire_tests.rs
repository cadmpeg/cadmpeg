// SPDX-License-Identifier: Apache-2.0
//! Byte identity and native admission for borrowed expression wires.

use super::{Expression, ExpressionDeclaration, ExpressionDeclarationWire, ExpressionWire};

#[test]
fn expression_declaration_borrowed_wire_preserves_bytes() {
    let json = r#"{"id":"nx:om:expression-declaration#test","object_id":4,"record":"record","name":"p12_angle","parameter_index":12,"qualifier":"angle","literal":"literal","source_entry":"entry","source_offset":10}"#;
    let record: ExpressionDeclaration = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&ExpressionDeclarationWire::from(record.clone())).unwrap()
    );
}

#[test]
fn expression_declaration_native_limit_refuses_before_clone() {
    let json = r#"{"id":"nx:om:expression-declaration#test","object_id":4,"record":"record","name":"p12_angle","parameter_index":12,"qualifier":"angle","literal":"literal","source_entry":"entry","source_offset":10}"#;
    let record: ExpressionDeclaration = serde_json::from_str(json).unwrap();
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::from_str::<serde_json::Value>(json).unwrap(),
    );
}

#[test]
fn expression_borrowed_wire_preserves_bytes() {
    let json = r#"{"id":"nx:om:expression#test","object_id":4,"record":"record","declaration":"declaration","name":"p12_angle","parameter_index":12,"qualifier":"angle","unit":"millimeter","expression":"3.25","value":3.25,"source_entry":"entry","source_table":"table","source_offset":10}"#;
    let record: Expression = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&ExpressionWire::from(record.clone())).unwrap()
    );
}

#[test]
fn expression_native_limit_refuses_before_clone() {
    let json = r#"{"id":"nx:om:expression#test","object_id":4,"record":"record","declaration":"declaration","name":"p12_angle","parameter_index":12,"qualifier":"angle","unit":"millimeter","expression":"3.25","value":3.25,"source_entry":"entry","source_table":"table","source_offset":10}"#;
    let record: Expression = serde_json::from_str(json).unwrap();
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::from_str::<serde_json::Value>(json).unwrap(),
    );
}
