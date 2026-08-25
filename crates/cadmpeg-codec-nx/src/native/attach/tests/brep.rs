// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::ids::BodyId;

use super::*;

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
