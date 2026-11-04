// SPDX-License-Identifier: Apache-2.0
//! An unreferenced sheet-metal settings frame projects as a Native feature.
use super::{assert_valid, decode};
use crate::test_support::native_test::f3d_native;
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh_and_flat_pattern_settings;

#[test]
fn unreferenced_sheet_metal_settings_project_native() {
    let decoded = decode(f3d_with_smbh_and_flat_pattern_settings(
        &synthetic_geometry_smbh(),
    ));
    let native = f3d_native(decoded.ir());
    assert_eq!(native.design_sheet_metal_settings.len(), 1);
    let settings = &native.design_sheet_metal_settings[0];
    assert_eq!(settings.record_index, 50);
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(settings.id()))
        .expect("the unreferenced settings frame owns a feature");
    assert!(matches!(
        feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native { kind, .. }
        ) if kind.as_str() == "sheet_metal_settings"
    ));
    assert!(decoded.report().losses.iter().all(|loss| loss.code
        != cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
            .note(String::new())
            .code));
    assert_valid(&decoded);
}
