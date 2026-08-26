// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::FeatureDefinition;

#[test]
fn nx_extract_datum_axis_retains_unresolved_axis_family() {
    for kind in ["DATUM_AXIS", "EXTRACT_DATUM_AXIS"] {
        let definition = super::non_boolean_feature_definition(kind, &[], None, None, None);

        assert_eq!(definition, FeatureDefinition::DatumAxisUnresolved);
        assert_eq!(definition.body_output_family(), None);
    }
}
