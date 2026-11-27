// SPDX-License-Identifier: Apache-2.0
use crate::features::sheet_metal_forms::{SheetMetalRelief, SheetMetalTransition};
use crate::features::FeatureOperation;
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;

#[test]
fn relief_and_transition_preserve_evaluated_lengths_and_default_forms() {
    let relief_wire = json!({
        "form": "style_default", "width": 1.25, "depth": 2.5,
        "minimum_remnant": 0.0, "seam_gap": 0.75
    });
    let relief: SheetMetalRelief = json::from_value(relief_wire.clone()).unwrap();
    assert_eq!(json::to_value(relief).unwrap(), relief_wire);
    let transition_wire = json!({"form": "style_default", "radius": 0.0});
    let transition: SheetMetalTransition = json::from_value(transition_wire.clone()).unwrap();
    assert_eq!(json::to_value(transition).unwrap(), transition_wire);
    for field in ["width", "depth"] {
        for value in [0.0, -1.0] {
            let mut invalid = relief_wire.clone();
            invalid[field] = json!(value);
            assert!(json::from_value::<SheetMetalRelief>(invalid).is_err());
        }
    }
    for field in ["minimum_remnant", "seam_gap"] {
        let mut invalid = relief_wire.clone();
        invalid[field] = json!(-1.0);
        assert!(json::from_value::<SheetMetalRelief>(invalid).is_err());
    }
    for invalid in [
        json!({"form": "round", "radius": 1.0}),
        json!({"form": "style_default", "radius": -1.0}),
        json!({"form": "style_default", "radius": 1.0, "mode": 4}),
    ] {
        assert!(json::from_value::<SheetMetalTransition>(invalid).is_err());
    }
}

#[test]
fn plate_and_bend_are_separate_wire_operations() {
    let mut bend = json!({
        "attachment": {"kind": "unresolved"},
        "inside_radius": 0.75,
        "relief": {
            "form": "style_default", "width": 1.0, "depth": 2.0,
            "minimum_remnant": 0.0, "seam_gap": 0.0
        },
        "transition": {"form": "style_default", "radius": 1.5}
    });
    let plate = json!({"kind": "sheet_metal_attached_plate", "value": {
        "profile": {"kind": "sketch", "value": "test:model:sketch#profile"},
        "thickness": 1.0,
        "side": "forward",
        "attachment": {"kind": "unresolved"}
    }});
    bend["plate"] = json!("test:model:feature#plate");
    let bend_wire = json!({"kind": "sheet_metal_bend", "value": bend});
    for wire in [plate, bend_wire] {
        let operation: FeatureOperation = json::from_value(wire.clone()).unwrap();
        assert_eq!(json::to_value(&operation).unwrap(), wire);
        assert!(operation.body_output_family().is_some());
        if wire["kind"] == "sheet_metal_attached_plate" {
            let mut spatial = wire.clone();
            spatial["value"]["profile"] =
                json!({"kind": "spatial_sketch", "value": "test:model:sketch#spatial"});
            assert!(json::from_value::<FeatureOperation>(spatial).is_err());
            let mut invalid_thickness = wire.clone();
            invalid_thickness["value"]["thickness"] = json!(0.0);
            assert!(json::from_value::<FeatureOperation>(invalid_thickness).is_err());
            for field in ["op", "inside_radius", "relief", "transition"] {
                let mut invalid = wire.clone();
                invalid["value"][field] = if field == "op" {
                    json!("join")
                } else {
                    bend[field].clone()
                };
                assert!(json::from_value::<FeatureOperation>(invalid).is_err());
            }
        } else {
            let mut invalid = wire;
            invalid["value"]["inside_radius"] = json!(0.0);
            assert!(json::from_value::<FeatureOperation>(invalid).is_err());
        }
    }
}

#[test]
fn unchanged_corner_requires_an_explicit_effect_and_preserves_its_bend() {
    let wire = json!({"kind": "sheet_metal_corner", "value": {
        "bend": "test:model:feature#bend",
        "effect": "unchanged"
    }});
    let operation: FeatureOperation = json::from_value(wire.clone()).unwrap();
    assert_eq!(json::to_value(&operation).unwrap(), wire);
    for effect in [json!(null), json!("unknown"), json!("relief")] {
        let mut invalid = wire.clone();
        invalid["value"]["effect"] = effect;
        assert!(json::from_value::<FeatureOperation>(invalid).is_err());
    }
    let mut missing = wire;
    missing["value"].as_object_mut().unwrap().remove("effect");
    assert!(json::from_value::<FeatureOperation>(missing).is_err());
}
