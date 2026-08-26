// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::FeatureDefinition;

#[test]
fn nx_extract_face_retains_family_without_unproven_roles() {
    let definition = super::non_boolean_feature_definition("EXTRACT_FACE", &[], None, None, None);

    assert_eq!(definition, FeatureDefinition::ExtractFaceUnresolved);
    assert_eq!(definition.body_output_family(), Some("extract face"));
}
