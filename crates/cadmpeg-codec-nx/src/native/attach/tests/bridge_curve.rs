// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::FeatureDefinition;

#[test]
fn nx_bridge_curve_retains_unresolved_curve_family() {
    let definition = super::non_boolean_feature_definition("BRIDGE_CURVE", &[], None, None, None);

    assert_eq!(definition, FeatureDefinition::BridgeCurveUnresolved);
    assert_eq!(definition.body_output_family(), None);
}
