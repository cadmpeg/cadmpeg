// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::FeatureDefinition;

use super::*;

#[test]
fn nx_cylinder_retains_body_affecting_family_without_dimensions() {
    let definition = super::non_boolean_feature_definition("CYLINDER", &[], None, None, None);

    assert_eq!(definition, FeatureDefinition::CylinderUnresolved);
    assert_eq!(definition.body_output_family(), Some("cylinder"));
}
