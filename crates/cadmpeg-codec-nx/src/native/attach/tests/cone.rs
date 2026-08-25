// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::FeatureDefinition;

use super::*;

#[test]
fn nx_cone_retains_body_family_without_dimensions() {
    let definition = super::non_boolean_feature_definition("CONE", &[], None, None, None);

    assert_eq!(definition, FeatureDefinition::ConeUnresolved);
    assert_eq!(definition.body_output_family(), Some("cone"));
}
