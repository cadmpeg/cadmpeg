use super::{SheetMetalBendLinesConstruction, SheetMetalLineBend};
use serde_json::json;

#[test]
fn bend_construction_omits_absent_optional_inputs() {
    let value = json!({"bends": [{}]});
    let construction: SheetMetalBendLinesConstruction =
        serde_json::from_value(value.clone()).unwrap();
    assert_eq!(construction.bends, vec![SheetMetalLineBend::default()]);
    assert_eq!(construction.stationary, None);
    assert_eq!(construction.inside_radius, None);
    assert_eq!(serde_json::to_value(construction).unwrap(), value);
}

#[test]
fn bend_construction_round_trips_present_inputs() {
    let value = json!({
        "bends": [{
            "line": {"kind": "native", "value": "test:line"},
            "angle": -0.75
        }],
        "stationary": {"kind": "native", "value": "test:stationary"},
        "inside_radius": 2.5
    });
    let construction: SheetMetalBendLinesConstruction =
        serde_json::from_value(value.clone()).unwrap();
    assert_eq!(construction.bends[0].angle.unwrap().get(), -0.75);
    assert_eq!(construction.inside_radius.unwrap().get(), 2.5);
    assert_eq!(serde_json::to_value(construction).unwrap(), value);
}

#[test]
fn bend_optional_inputs_refuse_named_nulls() {
    for key in ["stationary", "inside_radius"] {
        let mut value = json!({"bends": []});
        value[key] = serde_json::Value::Null;
        let error = serde_json::from_value::<SheetMetalBendLinesConstruction>(value)
            .unwrap_err()
            .to_string();
        assert!(error.contains(key), "{error}");
    }
    for key in ["line", "angle"] {
        let mut value = json!({});
        value[key] = serde_json::Value::Null;
        let error = serde_json::from_value::<SheetMetalLineBend>(value)
            .unwrap_err()
            .to_string();
        assert!(error.contains(key), "{error}");
    }
}

#[test]
fn fold_constructions_distinguish_absent_present_and_null_inputs() {
    use super::{SheetMetalRefoldConstruction, SheetMetalUnfoldConstruction};
    let empty: SheetMetalUnfoldConstruction = serde_json::from_value(json!({})).unwrap();
    assert_eq!(empty, SheetMetalUnfoldConstruction::default());
    assert_eq!(serde_json::to_value(empty).unwrap(), json!({}));
    let value = json!({
        "stationary": {"kind": "native", "value": "test:stationary"},
        "bends": {"kind": "native", "value": "test:bends"},
        "all_bends": false,
        "neutral_factor": 1.0
    });
    let construction: SheetMetalUnfoldConstruction = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(construction.all_bends, Some(false));
    assert_eq!(construction.neutral_factor.unwrap().get(), 1.0);
    assert_eq!(serde_json::to_value(construction).unwrap(), value);
    for key in ["stationary", "bends", "all_bends", "neutral_factor"] {
        let mut value = json!({});
        value[key] = serde_json::Value::Null;
        let error = serde_json::from_value::<SheetMetalUnfoldConstruction>(value)
            .unwrap_err()
            .to_string();
        assert!(error.contains(key), "{error}");
    }
    let empty: SheetMetalRefoldConstruction = serde_json::from_value(json!({})).unwrap();
    assert_eq!(empty, SheetMetalRefoldConstruction::default());
    assert_eq!(serde_json::to_value(empty).unwrap(), json!({}));
    let value = json!({"unfold": "test:model:feature#unfold"});
    let construction: SheetMetalRefoldConstruction = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(construction).unwrap(), value);
    let error = serde_json::from_value::<SheetMetalRefoldConstruction>(json!({"unfold": null}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("unfold"), "{error}");
}
