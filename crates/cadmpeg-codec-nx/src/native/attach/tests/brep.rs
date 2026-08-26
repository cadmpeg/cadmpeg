// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::ids::BodyId;

#[test]
fn nx_brep_projects_to_stored_geometry_only_with_unique_result_bodies() {
    let body = BodyId("body#1".into());
    assert!(matches!(
        super::brep_feature_definition(std::slice::from_ref(&body)),
        Some(FeatureDefinition::StoredGeometry)
    ));
    assert!(super::brep_feature_definition(&[]).is_none());
    assert!(super::brep_feature_definition(&[body.clone(), body]).is_none());
}

#[test]
fn nx_body_writing_brep_retains_unresolved_family() {
    let mut source_properties = BTreeMap::new();
    source_properties.insert("body_write.0".to_string(), "witness".to_string());

    assert_eq!(
        super::body_writing_unresolved_feature_definition("BREP", &source_properties),
        Some(FeatureDefinition::BrepUnresolved)
    );
}

#[test]
fn nx_non_body_writing_brep_remains_native_for_result_review() {
    assert_eq!(
        super::body_writing_unresolved_feature_definition("BREP", &BTreeMap::new()),
        None
    );
}
