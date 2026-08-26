// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::FeatureDefinition;

#[test]
fn nx_through_curve_mesh_retains_surface_family_without_roles() {
    let definition =
        super::non_boolean_feature_definition("THRU_CURVE_MESH", &[], None, None, None);

    assert_eq!(definition, FeatureDefinition::ThroughCurveMeshUnresolved);
    assert_eq!(definition.body_output_family(), Some("through curve mesh"));
}
