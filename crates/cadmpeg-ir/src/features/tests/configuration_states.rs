// SPDX-License-Identifier: Apache-2.0
use crate::features::{
    ConfigurationFeatureState, DistinctMembers, FeatureDefinition, FeatureOperation, FinitePoint3,
};
use crate::ids::BodyId;
use crate::math::Point3;

#[test]
fn configuration_states_store_only_suppression_activation_or_a_structural_difference() {
    let body = BodyId::mint("test:model:body#one").unwrap();
    assert!(DistinctMembers::try_from(vec![body.clone(), body]).is_err());
    let definition = FeatureDefinition::Operation(FeatureOperation::DatumPoint {
        position: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
        construction: None,
    });
    for state in [
        ConfigurationFeatureState::Suppressed {},
        ConfigurationFeatureState::Active {},
        ConfigurationFeatureState::Definition {
            definition: Box::new(definition),
        },
    ] {
        let wire = serde_json::to_value(&state).unwrap();
        if state.definition().is_some() {
            assert_eq!(
                wire,
                serde_json::json!({
                    "kind": "definition",
                    "definition": {
                        "definition": "datum_point",
                        "position": { "x": 0.0, "y": 0.0, "z": 0.0 }
                    }
                })
            );
        }
        assert_eq!(
            serde_json::from_value::<ConfigurationFeatureState>(wire).unwrap(),
            state
        );
    }
    assert_eq!(
        serde_json::to_value(ConfigurationFeatureState::Active {}).unwrap(),
        serde_json::json!({"kind":"active"})
    );
    assert_eq!(
        serde_json::to_value(ConfigurationFeatureState::Suppressed {}).unwrap(),
        serde_json::json!({"kind":"suppressed"})
    );
}

#[test]
fn configuration_states_reject_copied_outputs_dependencies_and_definitions() {
    for kind in ["active", "suppressed"] {
        for (key, value) in [
            ("outputs", serde_json::json!(["test:model:body#one"])),
            (
                "dependencies",
                serde_json::json!(["test:model:feature#earlier"]),
            ),
            ("definition", serde_json::json!({"kind":"stored_geometry"})),
        ] {
            let mut wire = serde_json::json!({"kind":kind});
            wire[key] = value;
            let error = serde_json::from_value::<ConfigurationFeatureState>(wire).unwrap_err();
            assert!(error.to_string().contains(key), "{error}");
        }
    }
}
