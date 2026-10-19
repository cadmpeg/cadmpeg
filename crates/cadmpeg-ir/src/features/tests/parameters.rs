use crate::{
    features::{DesignParameter, DistinctMembers, ParameterId, ParameterValue},
    scalar::FiniteReal,
};

#[test]
fn parameter_real_admission_preserves_finite_signed_wire_values() {
    for value in [-f64::MAX, -1.0, -0.0, 0.0, 1.0, f64::MAX] {
        let parameter = ParameterValue::Real(FiniteReal::new(value).unwrap());
        let wire = serde_json::json!({"kind": "real", "value": value});
        assert_eq!(serde_json::to_value(&parameter).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<ParameterValue>(wire).unwrap(),
            parameter
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(FiniteReal::new(value).is_none());
        let deserializer = serde::de::value::F64Deserializer::<serde::de::value::Error>::new(value);
        let error = super::super::deserialize_parameter_real(deserializer).unwrap_err();
        assert!(error.to_string().contains("value"));
    }
}

#[test]
fn parameter_dependencies_are_derived_only_from_ordered_expression_tokens() {
    use crate::features::{Expression, ExpressionToken};
    let first = ParameterId::mint("test:test:parameter#first").unwrap();
    let second = ParameterId::mint("test:test:parameter#second").unwrap();
    let expression = Expression {
        tokens: vec![
            ExpressionToken::Parameter(second.clone()),
            ExpressionToken::Text(" + ".into()),
            ExpressionToken::Parameter(first.clone()),
            ExpressionToken::Text(" + ".into()),
            ExpressionToken::Parameter(second.clone()),
        ],
    };
    assert_eq!(
        expression.dependencies().collect::<Vec<_>>(),
        vec![&second, &first, &second]
    );
    let mut wire = serde_json::json!({
        "id":"test:test:parameter#owner", "ordinal":0, "name":"value",
        "expression": expression, "driving":true,
    });
    let parameter: DesignParameter = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(parameter).unwrap(), wire);
    wire["dependencies"] = serde_json::json!([first, second]);
    assert!(serde_json::from_value::<DesignParameter>(wire)
        .unwrap_err()
        .to_string()
        .contains("dependencies"));
}

#[test]
fn reserved_parameter_dependencies_preserve_order_and_reject_repeats() {
    let first = ParameterId::mint("test:test:parameter#first").unwrap();
    let second = ParameterId::mint("test:test:parameter#second").unwrap();
    let ordered = DistinctMembers::try_from(vec![second.clone(), first.clone()])
        .expect("source-ordered members are distinct");
    assert_eq!(ordered.as_slice(), &[second.clone(), first.clone()]);
    assert!(DistinctMembers::try_from(vec![first.clone(), second, first]).is_err());
}
