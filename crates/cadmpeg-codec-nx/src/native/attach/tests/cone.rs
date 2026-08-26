// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::features::FeatureDefinition;

#[test]
fn nx_cone_retains_body_family_without_dimensions() {
    let mut source_properties = BTreeMap::new();
    source_properties.insert("body_write.0".to_string(), "witness".to_string());

    let definition = super::body_writing_unresolved_feature_definition("CONE", &source_properties);

    assert_eq!(definition, Some(FeatureDefinition::ConeUnresolved));
    assert_eq!(definition.unwrap().body_output_family(), Some("cone"));
}

#[test]
fn nx_non_body_writing_cone_remains_native_for_semantic_review() {
    let source_properties = BTreeMap::new();

    assert_eq!(
        super::body_writing_unresolved_feature_definition("CONE", &source_properties),
        None
    );
}
