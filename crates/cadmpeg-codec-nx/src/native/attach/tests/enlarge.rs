// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SurfaceExtension};

#[test]
fn nx_enlarge_preserves_surface_extension_family_without_roles() {
    assert!(matches!(
        super::enlarge_feature_definition(),
        FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
            faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            distance: None,
            method: SurfaceExtension::Unresolved,
        })
    ));
}
