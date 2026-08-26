// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

#[test]
fn nx_shell_preserves_family_without_assigning_construction_roles() {
    assert!(matches!(
        super::shell_feature_definition(),
        FeatureDefinition::Shell {
            bodies: None,
            removed_faces: FaceSelection::Unresolved,
            thickness: None,
            outward: None,
            mode: None,
            join: None,
            resolve_intersections: None,
            allow_self_intersections: None,
        }
    ));
}
