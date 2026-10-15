// SPDX-License-Identifier: Apache-2.0

use crate::materials::tests::appearance_loss_report;
use crate::materials::tests::material_losses;
use crate::materials::tests::opaque_appearance;

#[test]
fn appearance_loss_stands_when_no_asset_decodes() {
    let ir = cadmpeg_ir::CadIr::empty();
    let mut report = appearance_loss_report();
    crate::test_support::with_decode_context(|ctx| {
        crate::decode::reconcile_appearance_loss(ctx, &mut report, &ir, false).unwrap();
    });
    assert_eq!(material_losses(&report).len(), 1);
}

#[test]
fn appearance_loss_clears_when_an_unassigned_catalog_transfers() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.appearances = vec![opaque_appearance("2F0E19C1-0000-4000-8000-000000000001")];
    let mut report = appearance_loss_report();
    crate::test_support::with_decode_context(|ctx| {
        crate::decode::reconcile_appearance_loss(ctx, &mut report, &ir, false).unwrap();
    });
    assert!(material_losses(&report).is_empty());
}

#[test]
fn appearance_loss_counts_assets_whose_assignment_is_unresolved() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.appearances = vec![
        opaque_appearance("2F0E19C1-0000-4000-8000-000000000001"),
        opaque_appearance("2F0E19C1-0000-4000-8000-000000000002"),
    ];
    let mut report = appearance_loss_report();
    crate::test_support::with_decode_context(|ctx| {
        crate::decode::reconcile_appearance_loss(ctx, &mut report, &ir, true).unwrap();
    });
    let messages = material_losses(&report);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("2 Protein appearance asset(s)"));
}

#[test]
fn appearance_loss_clears_when_an_assignment_resolves() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let appearance = opaque_appearance("2F0E19C1-0000-4000-8000-000000000001");
    ir.model.appearance_bindings = vec![cadmpeg_ir::appearance::AppearanceBinding {
        id: "f3d:appearance:body#0_1:2F0E19C1-0000-4000-8000-000000000001"
            .try_into()
            .expect("valid identity"),
        target: cadmpeg_ir::appearance::AppearanceTarget::Body(
            cadmpeg_ir::ids::BodyId::mint("f3d:brep/a.smbh/brep:entity#1".to_owned())
                .expect("identity grammar"),
        ),
        appearance: appearance.id.clone(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::new(),
    }];
    ir.model.appearances = vec![appearance];
    let mut report = appearance_loss_report();
    crate::test_support::with_decode_context(|ctx| {
        crate::decode::reconcile_appearance_loss(ctx, &mut report, &ir, true).unwrap();
    });
    assert!(material_losses(&report).is_empty());
}

#[test]
fn unresolved_appearance_loss_search_preserves_work_refusal() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.appearances = vec![opaque_appearance("2F0E19C1-0000-4000-8000-000000000001")];
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "find F3D unresolved appearance loss",
        0,
        |ctx| {
            crate::decode::reconcile_appearance_loss(ctx, &mut appearance_loss_report(), &ir, true)
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "find F3D unresolved appearance loss")
    );
}
