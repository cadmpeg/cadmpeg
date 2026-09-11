// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, UnresolvedFamily};

#[test]
fn nx_cylinder_retains_body_affecting_family_without_dimensions() {
    let definition = super::non_boolean_feature_definition("CYLINDER", &[], None, None, None);

    assert_eq!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Cylinder
        })
    );
    assert_eq!(definition.body_output_family(), Some("cylinder"));
}
