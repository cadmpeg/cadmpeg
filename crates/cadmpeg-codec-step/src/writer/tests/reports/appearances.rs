// SPDX-License-Identifier: Apache-2.0
//! Appearance asset and assignment report contracts.

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepSchema, StepWriteOptions};
use cadmpeg_ir::examples::unit_cube;

#[test]
fn writer_reports_dangling_appearance_binding() {
    use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let binding = "test:model:appearance-binding#dangling";
    let appearance = AppearanceId::mint("test:model:appearance#missing").expect("identity grammar");
    ir.model.appearance_bindings.push(AppearanceBinding {
        native_ref: None,
        id: binding.try_into().expect("valid identity"),
        target: AppearanceTarget::Body(ir.model.bodies[0].id.clone()),
        appearance: appearance.clone(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::default(),
    });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the representable geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::AppearanceBindingMissingAsset.kind()
            && loss.message.contains(binding)
            && loss.message.contains(appearance.as_str())
    }));
}

#[test]
fn writer_reports_appearance_without_base_color() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let appearance =
        AppearanceId::mint("test:model:appearance#colorless").expect("identity grammar");
    let binding = "test:model:appearance-binding#colorless";
    ir.model.appearances.push(Appearance {
        native_ref: None,
        id: appearance.clone(),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        native_ref: None,
        id: binding.try_into().expect("valid identity"),
        target: AppearanceTarget::Face(ir.model.faces[0].id.clone()),
        appearance: appearance.clone(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::default(),
    });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the representable geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::AppearanceBindingNoBaseColor.kind()
            && loss.message.contains(binding)
            && loss.message.contains(appearance.as_str())
    }));
}
