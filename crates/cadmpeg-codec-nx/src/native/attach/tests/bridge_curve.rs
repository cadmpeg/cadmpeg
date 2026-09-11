// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, UnresolvedFamily};

#[test]
fn nx_bridge_curve_retains_unresolved_curve_family() {
    let definition = super::non_boolean_feature_definition("BRIDGE_CURVE", &[], None, None, None);

    assert_eq!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::BridgeCurve
        })
    );
    assert_eq!(definition.body_output_family(), None);
}
