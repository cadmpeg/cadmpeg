// SPDX-License-Identifier: Apache-2.0
//! Finite source intervals survive without topology edges.

#[test]
fn carrier_interval_round_trips_and_rejects_invalid_endpoints() {
    let mut curve = crate::examples::unit_cube().unwrap().model.curves.remove(0);
    curve.parameter_range = crate::topology::IncreasingParameterInterval::new([2.0, 5.0]);
    let mut value = serde_json::to_value(&curve).unwrap();
    let recovered: crate::geometry::Curve = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(recovered, curve);
    for range in [
        serde_json::Value::Null,
        serde_json::json!([2.0, 2.0]),
        serde_json::json!([5.0, 2.0]),
        serde_json::json!([2.0, null]),
    ] {
        value["parameter_range"] = range;
        assert!(serde_json::from_value::<crate::geometry::Curve>(value.clone()).is_err());
    }
    value.as_object_mut().unwrap().remove("parameter_range");
    assert!(serde_json::from_value::<crate::geometry::Curve>(value)
        .unwrap()
        .parameter_range
        .is_none());
}

#[test]
fn carrier_interval_survives_complete_document_serialization() {
    let mut document = crate::examples::unit_cube().unwrap();
    let interval = crate::topology::IncreasingParameterInterval::new([2.0, 5.0]);
    document.model.curves[0].parameter_range = interval;
    let value = serde_json::to_value(&document).unwrap();
    assert_eq!(
        value["model"]["curves"][0]["parameter_range"],
        serde_json::json!([2.0, 5.0])
    );
    let recovered: crate::CadIr = serde_json::from_value(value).unwrap();
    assert_eq!(recovered.model.curves[0].parameter_range, interval);
}
