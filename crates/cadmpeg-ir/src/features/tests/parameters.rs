use crate::features::{DesignParameter, DistinctMembers, FiniteReal, ParameterId, ParameterValue};

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
fn parameter_dependencies_reject_duplicates_and_preserve_source_order() {
    let first = ParameterId::mint("test:parameter#first").unwrap();
    let second = ParameterId::mint("test:parameter#second").unwrap();
    assert!(DistinctMembers::try_from(vec![first.clone(), first.clone()]).is_err());
    let mut dependencies = DistinctMembers::try_from(vec![second.clone(), first.clone()]).unwrap();
    assert!(!dependencies.insert(first.clone()));
    assert_eq!(dependencies.as_slice(), &[second.clone(), first.clone()]);
    dependencies.retain(|id| id == &first);
    assert_eq!(dependencies.as_slice(), std::slice::from_ref(&first));
    dependencies.clear();
    assert!(dependencies.is_empty());

    let mut wire = serde_json::json!({
        "id":"test:parameter#owner", "ordinal":0, "name":"value", "expression":"first+second",
        "dependencies":[second, first]
    });
    let parameter: DesignParameter = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(parameter).unwrap(), wire);
    wire["dependencies"] = serde_json::json!([first, first]);
    let error = serde_json::from_value::<DesignParameter>(wire.clone()).unwrap_err();
    assert!(error.to_string().contains("dependencies"));
    wire.as_object_mut().unwrap().remove("dependencies");
    let parameter: DesignParameter = serde_json::from_value(wire.clone()).unwrap();
    assert!(parameter.dependencies.is_empty());
    assert_eq!(serde_json::to_value(parameter).unwrap(), wire);
}
