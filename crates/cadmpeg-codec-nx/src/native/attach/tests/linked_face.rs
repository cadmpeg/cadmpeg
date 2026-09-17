// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::non_boolean_feature_definition;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, UnresolvedFamily};

#[test]
fn nx_linked_face_retains_body_affecting_family_without_roles() {
    let definition = non_boolean_feature_definition("LINKED_FACE", &[], None, None, None);

    assert_eq!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::LinkedFace
        })
    );
    assert_eq!(definition.body_output_family(), Some("linked face"));
}
