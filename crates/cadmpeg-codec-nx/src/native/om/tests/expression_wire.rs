use super::super::{Expression, ExpressionDeclaration};

const DECLARATION: &str = r#"{"id":"declaration","object_id":8,"record":"record","name":"p0008_radius","parameter_index":8,"qualifier":"radius","source_entry":"entry","source_offset":12}"#;
const EXPRESSION: &str = r#"{"id":"expression","object_id":8,"record":"record","name":"p0008_radius","parameter_index":8,"qualifier":"radius","unit":"millimeter","expression":"2","source_entry":"entry","source_table":"table","source_offset":20}"#;

#[test]
fn expression_names_preserve_spelling_and_wire_field_order() {
    let declaration: ExpressionDeclaration = serde_json::from_str(DECLARATION).unwrap();
    assert_eq!(declaration.name.index(), 8);
    assert_eq!(declaration.name.qualifier(), Some("radius"));
    assert_eq!(serde_json::to_string(&declaration).unwrap(), DECLARATION);
    let expression: Expression = serde_json::from_str(EXPRESSION).unwrap();
    assert_eq!(expression.name.index(), Some(8));
    assert_eq!(expression.name.qualifier(), Some("radius"));
    assert_eq!(serde_json::to_string(&expression).unwrap(), EXPRESSION);
}

#[test]
fn expression_names_reject_conflicting_derived_wire_fields() {
    for (field, value) in [
        ("parameter_index", serde_json::json!(9)),
        ("qualifier", serde_json::json!("diameter")),
    ] {
        for wire in [DECLARATION, EXPRESSION] {
            let mut wire: serde_json::Value = serde_json::from_str(wire).unwrap();
            wire[field] = value.clone();
            let error = if wire["id"] == "declaration" {
                serde_json::from_value::<ExpressionDeclaration>(wire)
                    .unwrap_err()
                    .to_string()
            } else {
                serde_json::from_value::<Expression>(wire)
                    .unwrap_err()
                    .to_string()
            };
            assert!(error.contains(field), "{error}");
        }
    }
}

#[test]
fn expression_owner_requires_record_for_persistent_identity() {
    for field in ["record", "object_id"] {
        let mut wire: serde_json::Value = serde_json::from_str(EXPRESSION).unwrap();
        wire[field] = serde_json::Value::Null;
        let error = serde_json::from_value::<Expression>(wire.clone()).unwrap_err();
        assert!(error.to_string().contains("object_id and record"));
        wire.as_object_mut().unwrap().remove(field);
        let error = serde_json::from_value::<Expression>(wire).unwrap_err();
        assert!(error.to_string().contains("object_id and record"));
    }
    let mut wire: serde_json::Value = serde_json::from_str(EXPRESSION).unwrap();
    wire["record"] = serde_json::Value::Null;
    wire["object_id"] = serde_json::Value::Null;
    let expression = serde_json::from_value::<Expression>(wire.clone()).unwrap();
    assert!(expression.owner.is_none());
    assert_eq!(serde_json::to_value(expression).unwrap(), wire);
}

#[test]
fn expression_requires_nonempty_source_table() {
    let mut wire: serde_json::Value = serde_json::from_str(EXPRESSION).unwrap();
    wire["source_table"] = "".into();
    assert!(serde_json::from_value::<Expression>(wire.clone())
        .unwrap_err()
        .to_string()
        .contains("source_table"));
    wire.as_object_mut().unwrap().remove("source_table");
    assert!(serde_json::from_value::<Expression>(wire)
        .unwrap_err()
        .to_string()
        .contains("source_table"));
}

#[test]
fn expression_wire_rejects_nonfinite_rust_values() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut wire: super::super::ExpressionWire = serde_json::from_str(EXPRESSION).unwrap();
        wire.value = Some(value);
        assert!(Expression::try_from(wire).unwrap_err().contains("value"));
    }
    let mut wire: super::super::ExpressionWire = serde_json::from_str(EXPRESSION).unwrap();
    wire.value = Some(-0.0);
    let mut expression = Expression::try_from(wire).unwrap();
    assert_eq!(
        expression.value.unwrap().get().to_bits(),
        (-0.0_f64).to_bits()
    );
    expression.value = Some(super::super::finite_value::FiniteValue::try_from(-1.0).unwrap());
    let wire = serde_json::to_value(expression).unwrap();
    assert_eq!(wire["value"], serde_json::json!(-1.0));
}
