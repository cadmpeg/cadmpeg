// SPDX-License-Identifier: Apache-2.0
use crate::features::{FeatureOperation, SheetMetalRelief, SheetMetalTransition};
use serde_json::json;

#[test]
fn relief_and_transition_preserve_evaluated_lengths_and_default_forms() {
    let relief_wire = json!({
        "form": "style_default", "width": 1.25, "depth": 2.5,
        "minimum_remnant": 0.0, "seam_gap": 0.75
    });
    let relief: SheetMetalRelief = serde_json::from_value(relief_wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(relief).unwrap(), relief_wire);
    let transition_wire = json!({"form": "style_default", "radius": 0.0});
    let transition: SheetMetalTransition = serde_json::from_value(transition_wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(transition).unwrap(), transition_wire);
    for field in ["width", "depth"] {
        for value in [0.0, -1.0] {
            let mut invalid = relief_wire.clone();
            invalid[field] = json!(value);
            assert!(serde_json::from_value::<SheetMetalRelief>(invalid).is_err());
        }
    }
    for field in ["minimum_remnant", "seam_gap"] {
        let mut invalid = relief_wire.clone();
        invalid[field] = json!(-1.0);
        assert!(serde_json::from_value::<SheetMetalRelief>(invalid).is_err());
    }
    for invalid in [
        json!({"form": "round", "radius": 1.0}),
        json!({"form": "style_default", "radius": -1.0}),
        json!({"form": "style_default", "radius": 1.0, "mode": 4}),
    ] {
        assert!(serde_json::from_value::<SheetMetalTransition>(invalid).is_err());
    }
}

#[test]
fn plate_and_bend_are_separate_wire_operations() {
    let common = json!({
        "attachment": {"kind": "unresolved"},
        "inside_radius": 0.75,
        "relief": {
            "form": "style_default", "width": 1.0, "depth": 2.0,
            "minimum_remnant": 0.0, "seam_gap": 0.0
        },
        "transition": {"form": "style_default", "radius": 1.5}
    });
    let mut plate = common.clone();
    plate["definition"] = json!("sheet_metal_attached_plate");
    plate["profile"] = json!({"kind": "sketch", "value": "test:model:sketch#profile"});
    plate["thickness"] = json!(1.0);
    plate["side"] = json!("forward");
    plate["op"] = json!("join");
    let mut bend = common;
    bend["definition"] = json!("sheet_metal_bend");
    bend["plate"] = json!("test:model:feature#plate");
    for wire in [plate, bend] {
        let operation: FeatureOperation = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&operation).unwrap(), wire);
        assert!(operation.body_output_family().is_some());
        if wire["definition"] == "sheet_metal_attached_plate" {
            let mut spatial = wire.clone();
            spatial["profile"] =
                json!({"kind": "spatial_sketch", "value": "test:model:sketch#spatial"});
            assert!(serde_json::from_value::<FeatureOperation>(spatial).is_err());
            let mut invalid_thickness = wire.clone();
            invalid_thickness["thickness"] = json!(0.0);
            assert!(serde_json::from_value::<FeatureOperation>(invalid_thickness).is_err());
        }
        let mut invalid = wire;
        invalid["inside_radius"] = json!(0.0);
        assert!(serde_json::from_value::<FeatureOperation>(invalid).is_err());
    }
}
