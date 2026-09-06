// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::geometry::{CompoundComponent, ProceduralSurfaceDefinition};

#[test]
fn compound_surface_wire_pairs_each_scalar_with_its_surface() {
    let definition = ProceduralSurfaceDefinition::Compound {
        components: vec![
            CompoundComponent {
                parameter: -0.5,
                component: "test:model:surface#0".into(),
            },
            CompoundComponent {
                parameter: 1.5,
                component: "test:model:surface#1".into(),
            },
        ],
    };
    let wire = serde_json::json!({
        "kind": "compound",
        "parameters": [-0.5, 1.5],
        "components": ["test:model:surface#0", "test:model:surface#1"],
    });
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire.clone()).unwrap(),
        definition
    );
    for parameters in [
        serde_json::json!([-0.5]),
        serde_json::json!([-0.5, 1.5, 2.5]),
    ] {
        let mut invalid = wire.clone();
        invalid["parameters"] = parameters;
        assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(invalid).is_err());
    }
}

#[test]
fn compound_curve_wire_pairs_each_scalar_with_its_curve() {
    use crate::geometry::ProceduralCurveDefinition;
    let definition = ProceduralCurveDefinition::Compound {
        parameters: vec![0.0, 0.5, 1.0],
        components: vec![
            CompoundComponent {
                parameter: -2.0,
                component: "test:model:curve#0".into(),
            },
            CompoundComponent {
                parameter: 4.0,
                component: "test:model:curve#1".into(),
            },
        ],
    };
    let wire = serde_json::json!({
        "kind": "compound",
        "parameters": [0.0, 0.5, 1.0],
        "component_parameters": [-2.0, 4.0],
        "components": ["test:model:curve#0", "test:model:curve#1"],
    });
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(wire.clone()).unwrap(),
        definition
    );
    for parameters in [
        serde_json::json!([-2.0]),
        serde_json::json!([-2.0, 4.0, 6.0]),
    ] {
        let mut invalid = wire.clone();
        invalid["component_parameters"] = parameters;
        assert!(serde_json::from_value::<ProceduralCurveDefinition>(invalid).is_err());
    }
}
