// SPDX-License-Identifier: Apache-2.0
use serde_json::json;

use crate::design::feature::FeatureOperation;
use crate::validate::design::fixture::{assert_finding, document};

fn operation(value: &serde_json::Value) -> FeatureOperation {
    serde_json::from_value(json!({"kind":"sheet_metal", "value":{
        "kind":value,"native_options":{},"source_settings":{}
    }}))
    .unwrap()
}

#[test]
fn attached_plate_requires_named_profile_and_attachment() {
    let mut ir = document();
    ir.design.features[0].definition.operation =
        operation(&json!({"kind":"attached_plate","value":{
            "profile":{"at":{"kind":"input"},"picks":{"kind":"unread"},"native":["profile"]},
            "thickness":{"kind":"literal","value":2.0},"side":"along",
            "attachment":{"at":{"kind":"input"},"picks":{"kind":"unread"},"native":["edges"]}
        }}));
    assert_finding(
        &ir,
        "attached sheet-metal selection requires resolved members",
    );
}

#[test]
fn unchanged_corner_requires_an_attached_bend() {
    let mut ir = document();
    ir.design.features[1].definition.operation =
        operation(&json!({"kind":"unchanged_corner","value":{
            "bend":ir.design.features[0].id
        }}));
    assert_finding(&ir, "requires a preceding attached bend");
    ir.design.features[1].definition.operation =
        operation(&json!({"kind":"unchanged_corner","value":{
            "bend":"test:design:feature#absent"
        }}));
    assert_finding(&ir, "requires a preceding attached bend");
}

fn named() -> serde_json::Value {
    json!({"at":{"kind":"input"},"picks":{"kind":"named","value":[{
        "target":{"kind":"native","value":{}},
        "native":{"codec":"test","value":{"kind":"text","value":"edge"}}
    }]}})
}

fn plate() -> serde_json::Value {
    json!({"kind":"attached_plate","value":{
        "profile":named(),"thickness":{"kind":"literal","value":2.0},
        "side":"along","attachment":named()
    }})
}

fn bend(plate: &str) -> serde_json::Value {
    json!({"kind":"attached_bend","value":{
        "plate":plate,"attachment":named(),"inside_radius":{"kind":"literal","value":1.0},
        "relief":{"form":"style_default","width":{"kind":"literal","value":2.0},
            "depth":{"kind":"literal","value":3.0},"minimum_remnant":{"kind":"literal","value":0.0},
            "seam_gap":{"kind":"literal","value":0.0}},
        "transition_form":"style_default","transition_radius":{"kind":"literal","value":0.0}
    }})
}

#[test]
fn attached_bend_accepts_positive_lengths_and_zero_clearances() {
    use crate::validate::design::fixture::assert_no_design_finding;
    let mut ir = document();
    ir.design.features[0].definition.operation = operation(&plate());
    ir.design.features[1].definition.operation =
        operation(&bend(ir.design.features[0].id.as_str()));
    assert_no_design_finding(&ir);
}

#[test]
fn attachment_references_require_the_input_history() {
    let mut ir = document();
    ir.design.features[1].definition.operation = operation(&plate());
    ir.design.features[0].definition.operation =
        operation(&bend(ir.design.features[1].id.as_str()));
    assert_finding(&ir, "requires a preceding attached plate");
}

#[test]
fn attached_dimensions_check_parameter_kind_range_and_negation() {
    use crate::design::parameter::ParameterValue;
    use crate::design::units::{Quantity, ValueKind};
    use crate::scalar::FiniteReal;
    use crate::validate::design::fixture::parameter;
    let mut ir = document();
    let mut length = parameter("thickness", ValueKind::Quantity(Quantity::LENGTH));
    length.value = Some(ParameterValue::Number(FiniteReal::new(0.0).unwrap()));
    let mut value = plate();
    value["value"]["thickness"] = json!({"kind":"parameter","value":length.id});
    ir.design.features[0].definition.operation = operation(&value);
    ir.design.parameters.push(length);
    assert_finding(&ir, "length is outside its permitted range");
    ir.design.parameters[0].kind = ValueKind::Boolean;
    assert_finding(&ir, "length binds a parameter of another kind");
    value["value"]["thickness"]["kind"] = json!("negated");
    ir.design.features[0].definition.operation = operation(&value);
    assert_finding(&ir, "length requires an unsigned dimension");
}

#[test]
fn attached_literals_require_positive_material_and_nonnegative_clearance() {
    for field in ["inside_radius", "transition_radius"] {
        let mut value = bend("test:design:feature#plate");
        value["value"][field]["value"] = json!(-1.0);
        assert!(
            serde_json::from_value::<crate::design::ops::sheet_metal::SheetMetalKind>(value)
                .is_err()
        );
    }
    for field in ["width", "depth", "minimum_remnant", "seam_gap"] {
        let mut value = bend("test:design:feature#plate");
        value["value"]["relief"][field]["value"] = json!(-1.0);
        assert!(
            serde_json::from_value::<crate::design::ops::sheet_metal::SheetMetalKind>(value)
                .is_err()
        );
    }
    let mut value = plate();
    value["value"]["thickness"]["value"] = json!(0.0);
    assert!(
        serde_json::from_value::<crate::design::ops::sheet_metal::SheetMetalKind>(value).is_err()
    );
}

#[test]
fn attached_selection_members_must_pick_edges() {
    let mut ir = document();
    let mut value = plate();
    value["value"]["attachment"]["picks"]["value"][0]["target"] =
        json!({"kind":"feature","value":ir.design.features[1].id});
    ir.design.features[0].definition.operation = operation(&value);
    assert_finding(&ir, "attachment requires edge picks");
}

#[test]
fn attached_wire_requires_every_operand() {
    for original in [
        plate(),
        bend("test:design:feature#plate"),
        json!({
            "kind":"unchanged_corner","value":{"bend":"test:design:feature#bend"}
        }),
    ] {
        let keys = original["value"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            let mut value = original.clone();
            value["value"].as_object_mut().unwrap().remove(&key);
            assert!(
                serde_json::from_value::<crate::design::ops::sheet_metal::SheetMetalKind>(value)
                    .is_err(),
                "missing {key}"
            );
        }
    }
}

#[test]
fn attachments_select_the_feature_input_state() {
    let mut ir = document();
    let mut value = plate();
    value["value"]["attachment"]["at"] = json!({"kind":"current"});
    ir.design.features[0].definition.operation = operation(&value);
    assert_finding(&ir, "attachment requires the input state");
}
