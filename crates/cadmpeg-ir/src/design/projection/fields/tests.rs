// SPDX-License-Identifier: Apache-2.0

use super::{ArrangementField, FeatureField, Field, ParameterField};

/// The properties the committed CADIR schema gives `definition`, less `id`.
fn schema_keys(definition: &str) -> Vec<String> {
    let schema: serde_json::Value =
        serde_json::from_str(crate::CADIR_JSON_SCHEMA).expect("committed schema");
    schema["$defs"][definition]["properties"]
        .as_object()
        .expect("schema properties")
        .keys()
        .filter(|key| key.as_str() != "id")
        .cloned()
        .collect()
}

fn keys<F: Field>(fields: &[F]) -> Vec<String> {
    fields.iter().map(|field| field.key().to_owned()).collect()
}

#[test]
fn design_field_keys_are_the_wire_keys_of_the_design_records() {
    let sorted = |mut keys: Vec<String>| {
        keys.sort();
        keys
    };
    assert_eq!(
        sorted(keys(FeatureField::ALL)),
        sorted(schema_keys("Feature"))
    );
    assert_eq!(
        sorted(keys(ParameterField::ALL)),
        sorted(schema_keys("DesignParameter"))
    );
    assert_eq!(
        sorted(keys(ArrangementField::ALL)),
        sorted(schema_keys("Arrangement"))
    );
}

#[test]
fn a_model_feature_serializes_with_the_keys_of_its_field_enum() {
    use crate::features::selections::DistinctMembers;
    use crate::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    let feature = Feature {
        product_definition: Some(
            crate::ids::ProductDefinitionId::mint("test:model:product-definition#part").unwrap(),
        ),
        id: FeatureId::mint("test:model:feature#base").unwrap(),
        ordinal: 0,
        name: Some("Base".to_owned()),
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: Some("base".to_owned()),
        source_text: Some("text".to_owned()),
        source_content: crate::features::FeatureContent::default(),
        evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ),
        native_ref: Some("test:native#base".to_owned()),
    };
    let wire = crate::features::serialization::FeatureWriteWire::new(&feature, Some(&feature.id));
    let value = serde_json::to_value(&wire).unwrap();
    let mut wire_keys = value
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| key.as_str() != "id")
        .cloned()
        .collect::<Vec<_>>();
    wire_keys.sort();
    let mut field_keys = keys(super::ModelFeatureField::ALL);
    field_keys.sort();
    assert_eq!(field_keys, wire_keys);
}

#[test]
fn a_model_parameter_serializes_with_the_keys_of_its_field_enum() {
    use crate::features::parameters::{
        DesignParameter, DimensionDisplay, ParameterPmi, ParameterValue, PmiDimensionSubtype,
    };
    use crate::features::selections::DistinctMembers;
    use crate::features::{FeatureId, ParameterId};
    let parameter = DesignParameter {
        product_definition: Some(
            crate::ids::ProductDefinitionId::mint("test:model:product-definition#part").unwrap(),
        ),
        id: ParameterId::mint("test:model:parameter#width").unwrap(),
        owner: Some(FeatureId::mint("test:model:feature#base").unwrap()),
        ordinal: 0,
        name: "width".to_owned(),
        expression: "1".to_owned(),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Integer(1)),
        dependencies: DistinctMembers::default(),
        properties: std::collections::BTreeMap::default(),
        pmi: Some(ParameterPmi {
            subtype: PmiDimensionSubtype::Linear,
            precision: 0,
            display_text: None,
            basic: false,
            inspection: false,
            reference_only: false,
            native_ref: "test:native#pmi".to_owned(),
        }),
        native_ref: Some("test:native#width".to_owned()),
    };
    let value = serde_json::to_value(&parameter).unwrap();
    let mut wire_keys = value
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| key.as_str() != "id")
        .cloned()
        .collect::<Vec<_>>();
    wire_keys.sort();
    let mut field_keys = keys(super::ModelParameterField::ALL);
    field_keys.sort();
    assert_eq!(field_keys, wire_keys);
}

#[test]
fn a_model_configuration_serializes_with_the_keys_of_its_field_enum() {
    use crate::features::parameters::DesignConfiguration;
    use crate::features::selections::DistinctMembers;
    use crate::features::ConfigurationId;
    use crate::ids::BodyId;
    let configuration = DesignConfiguration {
        id: ConfigurationId::mint("test:model:configuration#open").unwrap(),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: Some("Open".to_owned()),
        material: Some("Steel".to_owned()),
        properties: std::collections::BTreeMap::new(),
        parameter_overrides: std::collections::BTreeMap::new(),
        bodies: Some(
            DistinctMembers::try_from(vec![BodyId::mint("test:model:body#1").unwrap()]).unwrap(),
        ),
        parameter_values: std::collections::BTreeMap::new(),
        feature_states: std::collections::BTreeMap::new(),
        native_ref: Some("test:native#open".to_owned()),
    };
    let value = serde_json::to_value(&configuration).unwrap();
    let mut wire_keys = value
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| key.as_str() != "id")
        .cloned()
        .collect::<Vec<_>>();
    wire_keys.sort();
    let mut field_keys = keys(super::ModelConfigurationField::ALL);
    field_keys.sort();
    assert_eq!(field_keys, wire_keys);
}
