// SPDX-License-Identifier: Apache-2.0

use super::{FeatureField, Field, ParameterField};

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
