// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::features::FeatureDefinition;

#[test]
fn nx_body_writing_sphere_retains_primitive_family_without_dimensions() {
    let mut source_properties = BTreeMap::new();
    source_properties.insert("body_write.0".to_string(), "witness".to_string());

    let definition =
        super::body_writing_unresolved_feature_definition("SPHERE", &source_properties);

    assert_eq!(definition, Some(FeatureDefinition::SphereUnresolved));
    assert_eq!(definition.unwrap().body_output_family(), Some("sphere"));
}

#[test]
fn nx_non_body_writing_sphere_remains_native_for_semantic_review() {
    let source_properties = BTreeMap::new();

    assert_eq!(
        super::body_writing_unresolved_feature_definition("SPHERE", &source_properties),
        None
    );
}
