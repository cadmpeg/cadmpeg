// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use crate::examples::unit_cube;
use crate::features::{
    ConfigurationFeatureState as State, ConfigurationId, DesignConfiguration, DesignParameter,
    Expression, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    ParameterId, ParameterValue,
};
use crate::scalar::{Angle, Length};
use crate::validate::validate_neutral;
use crate::CadIr;

fn configuration() -> DesignConfiguration {
    DesignConfiguration {
        id: ConfigurationId::mint("test:model:configuration#alternate").unwrap(),
        ordinal: 0,
        active: false,
        source_index: Some(7),
        name: Some("Alternate".into()),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies: None,
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    }
}

fn feature(id: &str, ordinal: u64) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint(id).unwrap(),
        ordinal,
        name: None,
        suppressed: Some(false),
        needs_recompute: false,
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        native_ref: None,
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::StoredGeometry {},
        )),
    }
}

#[test]
fn configuration_body_membership_round_trips_and_validates() {
    let mut ir = unit_cube().unwrap();
    let parameter = ParameterId::mint("test:model:parameter#width").unwrap();
    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        id: parameter.clone(),
        owner: None,
        ordinal: 0,
        name: "width".into(),
        expression: "10 mm".into(),
        driving: true,
        display: None,
        value: Some(ParameterValue::Length(Length::new(10.0).unwrap())),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    let mut alternative = configuration();
    alternative.bodies = Some(vec![ir.model.bodies[0].id.clone()].try_into().unwrap());
    alternative
        .parameter_overrides
        .insert(parameter.clone(), "25 mm".into());
    ir.model.configurations.push(alternative);
    ir.finalize();
    assert!(validate_neutral(&ir, vec![]).unwrap().is_ok());
    let round_trip = CadIr::from_json(&serde_json::to_string(&ir).unwrap()).unwrap();
    assert_eq!(round_trip.model.configurations, ir.model.configurations);
    assert_eq!(
        round_trip.model.configurations[0].parameter_overrides[&parameter]
            .literal_text()
            .as_deref(),
        Some("25 mm")
    );

    let missing = ParameterId::mint("test:model:parameter#missing").unwrap();
    ir.model.configurations[0]
        .parameter_overrides
        .insert(missing.clone(), "30 mm".into());
    assert!(validate_neutral(&ir, vec![])
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.message.contains("configuration parameter override")));
    ir.model.configurations[0]
        .parameter_overrides
        .remove(&missing);
    ir.model.configurations[0].parameter_values.insert(
        parameter.clone(),
        ParameterValue::Angle(Angle::new(1.0).unwrap()),
    );
    assert!(validate_neutral(&ir, vec![])
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.message == "configuration parameter value is invalid"));
    ir.model.configurations[0].parameter_values.clear();
    ir.model.configurations[0].bodies = Some(
        vec![crate::ids::BodyId::mint("test:model:body#missing").unwrap()]
            .try_into()
            .unwrap(),
    );
    assert!(validate_neutral(&ir, vec![])
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.message.contains("missing configuration body")));
}

#[test]
fn active_configuration_uses_the_base_state_without_copied_overrides() {
    let mut ir = CadIr::empty();
    let mut base = configuration();
    base.active = true;
    ir.model.configurations.push(base);
    assert!(validate_neutral(&ir, vec![]).unwrap().is_ok());
    let owner = feature("test:model:feature#owner", 0);
    let owner_id = owner.id.clone();
    ir.model.features.push(owner);
    ir.model.configurations[0]
        .feature_states
        .insert(owner_id, State::Active {});
    assert!(validate_neutral(&ir, vec![])
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.message.contains("base state without overrides")));
    ir.model.configurations[0].feature_states.clear();
    ir.model.features[0].suppressed = Some(true);
    assert!(
        validate_neutral(&ir, vec![]).unwrap().is_ok(),
        "base suppression is canonical"
    );
}

#[test]
fn configuration_structural_references_resolve_independently_of_order_and_use_effective_suppression(
) {
    let mut ir = CadIr::empty();
    let first = feature("test:model:feature#first", 0);
    let second = feature("test:model:feature#second", 1);
    let first_id = first.id.clone();
    let second_id = second.id.clone();
    ir.model.features = vec![first, second];
    let mut alternative = configuration();
    alternative.feature_states.insert(
        second_id.clone(),
        State::Definition {
            definition: Box::new(FeatureDefinition::Operation(
                FeatureOperation::DerivedGeometry {
                    source: first_id.clone(),
                },
            )),
        },
    );
    ir.model.configurations.push(alternative);
    assert!(validate_neutral(&ir, vec![]).unwrap().is_ok());
    ir.model.configurations[0]
        .feature_states
        .insert(first_id.clone(), State::Suppressed {});
    assert!(validate_neutral(&ir, vec![])
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.message.contains("closure uses suppressed feature")));
    ir.model.configurations[0].feature_states.remove(&first_id);
    assert!(validate_neutral(&ir, vec![]).unwrap().is_ok());
    ir.model.configurations[0].feature_states.clear();
    ir.model.configurations[0].feature_states.insert(
        first_id,
        State::Definition {
            definition: Box::new(FeatureDefinition::Operation(
                FeatureOperation::DerivedGeometry { source: second_id },
            )),
        },
    );
    assert!(validate_neutral(&ir, vec![]).unwrap().is_ok());
}

#[test]
fn configuration_expression_references_and_feature_state_targets_must_resolve() {
    let mut ir = CadIr::empty();
    let mut alternative = configuration();
    alternative.feature_states.insert(
        FeatureId::mint("test:model:feature#missing").unwrap(),
        State::Suppressed {},
    );
    let missing = ParameterId::mint("test:model:parameter#missing").unwrap();
    alternative.parameter_overrides.insert(
        missing.clone(),
        Expression {
            tokens: vec![crate::features::ExpressionToken::Parameter(missing)],
        },
    );
    ir.model.configurations.push(alternative);
    let report = validate_neutral(&ir, vec![]).unwrap();
    for message in [
        "configuration feature state",
        "configuration parameter override",
        "configuration expression parameter",
    ] {
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.message.contains(message)),
            "{message}: {:?}",
            report.findings
        );
    }
}

#[test]
fn configuration_identity_censuses_reject_duplicate_order_source_index_and_active_state() {
    let mut ir = CadIr::empty();
    let mut first = configuration();
    first.active = true;
    let mut second = first.clone();
    second.id = ConfigurationId::mint("test:model:configuration#second").unwrap();
    ir.model.configurations = vec![first, second];
    let report = validate_neutral(&ir, vec![]).unwrap();
    for message in [
        "repeats configuration ordinal",
        "repeats configuration source index",
        "multiple active configurations",
    ] {
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.message.contains(message)));
    }
}

#[test]
fn configuration_name_preserves_resolution_state_and_refuses_stated_nulls() {
    let configuration: DesignConfiguration =
        serde_json::from_value(serde_json::json!({"id":"test:model:configuration#one"})).unwrap();
    assert!(configuration.name.is_none());
    assert!(!configuration.active);
    let wire = serde_json::to_value(configuration).unwrap();
    assert!(wire.get("name").is_none());
    assert!(wire.get("active").is_none());
    for key in ["name", "bodies"] {
        let error = serde_json::from_value::<DesignConfiguration>(
            serde_json::json!({"id":"test:model:configuration#one",key:null}),
        )
        .unwrap_err();
        assert!(error.to_string().contains("does not state null"), "{error}");
    }
}

#[test]
fn configuration_suppression_is_read_from_difference_states_and_refuses_the_deleted_key() {
    let mut ir = CadIr::empty();
    let owner = feature("test:model:feature#owner", 0);
    let owner_id = owner.id.clone();
    ir.model.features.push(owner);
    let mut alternative = configuration();
    alternative
        .feature_states
        .insert(owner_id.clone(), State::Suppressed {});
    ir.model.configurations.push(alternative);
    let mut wire = serde_json::to_value(&ir).unwrap();
    assert_eq!(
        wire["model"]["configurations"][0]["feature_states"][owner_id.as_str()],
        serde_json::json!({"kind":"suppressed"})
    );
    let restored = serde_json::from_value::<CadIr>(wire.clone()).unwrap();
    assert_eq!(
        restored.model.configurations[0]
            .suppressed_features()
            .collect::<Vec<_>>(),
        vec![&owner_id]
    );
    wire["model"]["configurations"][0]["suppressed_features"] = serde_json::json!([owner_id]);
    assert!(serde_json::from_value::<CadIr>(wire)
        .unwrap_err()
        .to_string()
        .contains("unknown field `suppressed_features`"));
}

#[test]
fn inactive_dimension_resolution_uses_its_override_cache_and_never_the_base_cache() {
    use crate::features::{BooleanOp, Dimension, PrimitiveSolid, PrimitiveSolidKind};
    let mut ir = CadIr::empty();
    let id = ParameterId::mint("test:model:parameter#width").unwrap();
    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        id: id.clone(),
        owner: None,
        ordinal: 0,
        name: "width".into(),
        expression: "2 mm".into(),
        driving: true,
        display: None,
        value: Some(ParameterValue::Length(Length::new(2.0).unwrap())),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    let mut box_feature = feature("test:model:feature#box", 0);
    box_feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Primitive {
            placement: None,
            solid: PrimitiveSolid::new(PrimitiveSolidKind::Box {
                length: Dimension::Parameter(id.clone()),
                width: crate::features::Dimension::Literal(Length::new(3.0).unwrap()),
                height: crate::features::Dimension::Literal(Length::new(4.0).unwrap()),
            })
            .unwrap(),
            op: BooleanOp::NewBody,
        }));
    ir.model.features.push(box_feature);
    let mut alternative = configuration();
    alternative
        .parameter_overrides
        .insert(id.clone(), "-2 mm".into());
    assert_eq!(
        alternative.dimension_value(&ir, &Dimension::<Length>::Parameter(id.clone())),
        None
    );
    ir.model.configurations.push(alternative);
    assert!(
        validate_neutral(&ir, vec![]).unwrap().is_ok(),
        "unevaluated inactive dimension is legal"
    );
    ir.model.configurations[0].parameter_values.insert(
        id.clone(),
        ParameterValue::Length(Length::new(-2.0).unwrap()),
    );
    let report = validate_neutral(&ir, vec![]).unwrap();
    assert!(report
        .findings
        .iter()
        .any(
            |finding| finding.entity.as_deref() == Some(ir.model.configurations[0].id.as_str())
                && finding.message.contains("resolved dimensions violate")
        ));
    assert_eq!(
        ir.dimension_value(&Dimension::<Length>::Parameter(id)),
        Some(Length::new(2.0).unwrap())
    );
}
