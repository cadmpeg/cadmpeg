// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::FeatureDefinition;

use super::*;

#[test]
fn nx_fill_hole_retains_body_affecting_family_without_roles() {
    let definition = super::non_boolean_feature_definition("FILL_HOLE", &[], None, None, None);

    assert_eq!(definition, FeatureDefinition::FillHoleUnresolved);
    assert_eq!(definition.body_output_family(), Some("fill hole"));
}
