use super::super::*;

#[test]
fn occurrence_scale_rejects_nonfinite_components_with_field_context() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for axis in 0..3 {
            let mut scale = [1.0; 3];
            scale[axis] = value;
            let deserializer = serde::de::value::SeqDeserializer::<_, serde::de::value::Error>::new(
                scale.into_iter(),
            );
            let error = deserialize_occurrence_scale(deserializer).unwrap_err();
            assert!(error.to_string().contains("scale"), "{error}");
        }
    }
}

#[test]
fn occurrence_scale_preserves_zero_and_negative_factors_on_the_wire() {
    let mut occurrence =
        super::occurrence("test:model:occurrence#scale", OccurrenceParent::Root, 0.0);
    let mut wire = serde_json::to_value(&occurrence).unwrap();
    assert_eq!(wire["scale"], serde_json::json!([1.0, 1.0, 1.0]));
    wire["scale"] = serde_json::json!([-2.0, 0.0, 3.0]);
    occurrence = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(occurrence.scale.map(FiniteReal::get), [-2.0, 0.0, 3.0]);
    assert_eq!(serde_json::to_value(&occurrence).unwrap(), wire);
    occurrence.scale[1] = FiniteReal::ONE;
    assert_eq!(occurrence.scale.map(FiniteReal::get), [-2.0, 1.0, 3.0]);
    wire["scale"] = serde_json::json!([null, 1.0, 1.0]);
    let error = serde_json::from_value::<Occurrence>(wire).unwrap_err();
    assert!(error.to_string().contains("scale"), "{error}");
}
