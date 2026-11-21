// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::feature_projection::enlarge_feature_definition;
use cadmpeg_ir::features::surface_inputs::SurfaceExtension;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

#[test]
fn nx_enlarge_preserves_surface_extension_family_without_roles() {
    assert!(matches!(
        enlarge_feature_definition(),
        FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
            faces: cadmpeg_ir::features::selections::FaceSelection::Unresolved,
            distance: None,
            method: SurfaceExtension::Unresolved,
        })
    ));
}
