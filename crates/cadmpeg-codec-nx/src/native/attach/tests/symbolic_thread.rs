// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

#[test]
fn nx_symbolic_thread_retains_cosmetic_family_without_roles() {
    let definition =
        super::non_boolean_feature_definition("SYMBOLIC_THREAD", &[], None, None, None);
    let expected = FeatureDefinition::CosmeticThread {
        face: FaceSelection::Unresolved,
        diameter: None,
        extent: None,
    };
    assert_eq!(definition, expected);
    assert_eq!(super::symbolic_thread_feature_definition(), definition);
}
