// SPDX-License-Identifier: Apache-2.0
//! A Canvas record run decodes its scope, image, asset and reference frame.
use super::{assert_valid, decode};
use crate::test_support::native_test::f3d_native;
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_canvas;

#[test]
fn canvas_record_run_projects_its_reference_image() {
    let decoded = decode(f3d_with_smbh_and_protein_with_generated_canvas(
        &synthetic_geometry_smbh(),
    ));
    let native = f3d_native(decoded.ir());
    assert_eq!(native.design_parameter_scopes.len(), 1);
    assert_eq!(
        native.design_parameter_scopes[0].kind(),
        crate::records::feature::scope::DesignFeatureKind::Canvas
    );
    assert_eq!(native.design_canvas_images.len(), 1);
    let image = &native.design_canvas_images[0];
    assert_eq!(image.asset_name(), "a.png");
    assert_eq!(decoded.ir().model.assets.len(), 1);
    let asset = &decoded.ir().model.assets[0];
    assert_eq!(
        asset.native_ref.as_deref(),
        Some(image.id.as_str()),
        "the asset keeps its image record as its source"
    );
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| {
            feature.id == crate::ids::neutral_feature_id(&native.design_parameter_scopes[0])
        })
        .expect("the Canvas scope owns a feature");
    assert!(matches!(
        feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::ReferenceImage { .. }
        )
    ));
    assert!(decoded.report().losses.iter().all(|loss| loss.code
        != cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
            .note(String::new())
            .code));
    assert_valid(&decoded);
}
