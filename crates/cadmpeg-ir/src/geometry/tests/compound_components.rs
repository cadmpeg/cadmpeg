// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::geometry::{CompoundComponent, ProceduralSurfaceDefinition};

#[test]
fn a_compound_surface_component_is_one_object_carrying_its_own_scalar() {
    let definition = ProceduralSurfaceDefinition::Compound(
        crate::geometry::surface_payloads::CompoundSurfacePayload::try_new(vec![
            CompoundComponent {
                parameter: -0.5,
                component: "test:model:surface#0".try_into().expect("valid identity"),
            },
            CompoundComponent {
                parameter: 1.5,
                component: "test:model:surface#1".try_into().expect("valid identity"),
            },
        ])
        .unwrap(),
    );
    let wire = serde_json::json!({
        "kind": "compound",
        "components": [
            {"parameter": -0.5, "component": "test:model:surface#0"},
            {"parameter": 1.5, "component": "test:model:surface#1"},
        ],
    });
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire.clone()).unwrap(),
        definition
    );
}

#[test]
fn a_compound_surface_carries_no_parallel_parameter_array() {
    let struct_of_arrays = serde_json::json!({
        "kind": "compound",
        "parameters": [-0.5, 1.5],
        "components": ["test:model:surface#0", "test:model:surface#1"],
    });
    let error = serde_json::from_value::<ProceduralSurfaceDefinition>(struct_of_arrays)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("expected struct CompoundComponent"),
        "{error}"
    );

    let stray = serde_json::json!({
        "kind": "compound",
        "parameters": [-0.5, 1.5],
        "components": [
            {"parameter": -0.5, "component": "test:model:surface#0"},
            {"parameter": 1.5, "component": "test:model:surface#1"},
        ],
    });
    let error = serde_json::from_value::<ProceduralSurfaceDefinition>(stray)
        .unwrap_err()
        .to_string();
    assert!(error.contains("parameters"), "{error}");
}

#[test]
fn a_compound_curve_component_is_one_object_carrying_its_own_scalar() {
    use crate::geometry::ProceduralCurveDefinition;
    let definition = ProceduralCurveDefinition::Compound(
        crate::geometry::CompoundCurveConstruction::try_new(
            vec![0.0, 0.5, 1.0],
            vec![
                CompoundComponent {
                    parameter: -2.0,
                    component: "test:model:curve#0".try_into().expect("valid identity"),
                },
                CompoundComponent {
                    parameter: 4.0,
                    component: "test:model:curve#1".try_into().expect("valid identity"),
                },
            ],
        )
        .unwrap(),
    );
    let wire = serde_json::json!({
        "kind": "compound",
        "parameters": [0.0, 0.5, 1.0],
        "components": [
            {"parameter": -2.0, "component": "test:model:curve#0"},
            {"parameter": 4.0, "component": "test:model:curve#1"},
        ],
    });
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(wire.clone()).unwrap(),
        definition
    );

    let mut stray = wire;
    stray["component_parameters"] = serde_json::json!([-2.0, 4.0]);
    let error = serde_json::from_value::<ProceduralCurveDefinition>(stray)
        .unwrap_err()
        .to_string();
    assert!(error.contains("component_parameters"), "{error}");
}

#[test]
fn a_compound_component_rejects_an_unknown_key_by_name() {
    let wire = serde_json::json!({
        "kind": "compound",
        "components": [
            {"parameter": -0.5, "component": "test:model:surface#0", "zz_bogus": 1},
        ],
    });
    let error = serde_json::from_value::<ProceduralSurfaceDefinition>(wire)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[test]
fn compound_curve_requires_components_and_finite_parameters() {
    use crate::geometry::{CompoundCurveConstruction, ProceduralCurveDefinition};
    let component = || CompoundComponent {
        parameter: -2.0,
        component: "test:model:curve#0".try_into().unwrap(),
    };
    assert!(CompoundCurveConstruction::try_new(Vec::new(), vec![component()]).is_ok());
    assert!(CompoundCurveConstruction::try_new(vec![0.0], Vec::new()).is_err());
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(CompoundCurveConstruction::try_new(vec![invalid], vec![component()]).is_err());
        let mut item = component();
        item.parameter = invalid;
        assert!(CompoundCurveConstruction::try_new(Vec::new(), vec![item]).is_err());
    }
    let empty = serde_json::json!({
        "kind": "compound", "parameters": [], "components": []
    });
    assert!(serde_json::from_value::<ProceduralCurveDefinition>(empty).is_err());
    let unordered = CompoundCurveConstruction::try_new(vec![2.0, -1.0], vec![component()]).unwrap();
    let wire = serde_json::to_value(&unordered).unwrap();
    assert_eq!(
        serde_json::from_value::<CompoundCurveConstruction>(wire).unwrap(),
        unordered
    );
}
