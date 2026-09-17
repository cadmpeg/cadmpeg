// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::non_boolean_feature_definition;
use crate::native::attach::symbolic_thread_feature_definition;
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

#[test]
fn nx_symbolic_thread_retains_cosmetic_family_without_roles() {
    let definition = non_boolean_feature_definition("SYMBOLIC_THREAD", &[], None, None, None);
    let expected = FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
        face: FaceSelection::Unresolved,
        diameter: None,
        extent: None,
    });
    assert_eq!(definition, expected);
    assert_eq!(symbolic_thread_feature_definition(), definition);
}
