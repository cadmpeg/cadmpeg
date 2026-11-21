// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::feature::scope::{DesignFeatureKind, DesignScopePayloadMut};
use cadmpeg_ir::codec::DecodeOptions;
use std::io::Cursor;

fn fixture() -> (
    DesignParameterScope,
    Vec<DesignConstructionOperandGroup>,
    Vec<DesignSketchPlacement>,
) {
    let bytes = crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_base_flange(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(bytes),
        &DecodeOptions::default(),
    )
    .unwrap();
    let native = decoded.ir().native.namespace("f3d").unwrap();
    let scopes = native
        .arena_as::<DesignParameterScope>("design_parameter_scopes")
        .unwrap();
    let scope = scopes
        .into_iter()
        .find(|scope| scope.kind() == DesignFeatureKind::BaseFlange)
        .unwrap();
    (
        scope,
        native
            .arena_as("design_construction_operand_groups")
            .unwrap(),
        native.arena_as("design_sketch_placements").unwrap(),
    )
}

fn unread_operation(scope: &mut DesignParameterScope) {
    let DesignScopePayloadMut::BaseFlange(slot) = scope.payload_mut() else {
        panic!("base flange");
    };
    slot.as_mut().unwrap().base_flange_operation = None;
}

#[test]
fn unread_operation_keeps_the_profile_and_owned_rule_thickness() {
    let (mut scope, groups, placements) = fixture();
    let operation = scope.base_flange_operation().unwrap().clone();
    let settings: DesignSheetMetalSettings = serde_json::from_value(serde_json::json!({
        "id": format!("{}:design-sheet-metal-settings#300", native_stream(&scope.id).unwrap()),
        "record_index": operation.settings_record_index, "byte_offset": 300, "paired_byte_offset": 400, "mask": 0,
        "inputs": {"version": "v2", "slots": [null, null, null, null, null, null, null]},
        "rule_record_index": 104, "presentation_record_index": 105, "owner_record_index": scope.record_index, "context_record_index": 106,
        "rule_thickness": 0.125, "rule_thickness_offset": 850,
    })).unwrap();
    unread_operation(&mut scope);
    let mut losses = Vec::new();
    let (definition, stored) =
        project(&scope, &groups, &placements, &[settings], &mut losses).unwrap();
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
        profile,
        thickness,
        side,
    }) = definition
    else {
        panic!("base flange");
    };
    assert!(matches!(profile, PlanarProfileRef::Sketch(_)));
    assert_eq!(thickness, PositiveLength::new(1.25));
    assert_eq!(side, None);
    assert!(!stored);
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        [F3dLossCode::BaseFlangeSideUnresolved.kind()]
    );
}

#[test]
fn unread_thickness_and_side_keep_the_readable_profile() {
    let (mut scope, groups, placements) = fixture();
    unread_operation(&mut scope);
    let mut losses = Vec::new();
    let (definition, stored) = project(&scope, &groups, &placements, &[], &mut losses).unwrap();
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
        profile,
        thickness,
        side,
    }) = definition
    else {
        panic!("base flange");
    };
    assert!(matches!(profile, PlanarProfileRef::Sketch(_)));
    assert_eq!(thickness, None);
    assert_eq!(side, None);
    assert!(!stored);
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        [
            F3dLossCode::BaseFlangeThicknessUnresolved.kind(),
            F3dLossCode::BaseFlangeSideUnresolved.kind()
        ]
    );
}

#[test]
fn unread_profile_keeps_fixed_form_thickness_and_side() {
    let (scope, _, _) = fixture();
    let mut losses = Vec::new();
    let (definition, stored) = project(&scope, &[], &[], &[], &mut losses).unwrap();
    assert!(
        matches!(definition, FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
        profile: PlanarProfileRef::Unresolved(_), thickness: Some(value), side: Some(SheetMetalThicknessSide::Forward),
    }) if value.get() == 2.0)
    );
    assert!(stored);
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        [F3dLossCode::BaseFlangeProfileUnresolved.kind()]
    );
}

#[test]
fn unread_operation_preserves_a_region_selection_with_its_sketch() {
    let (mut scope, groups, placements) = fixture();
    let expected_sketch = crate::ids::neutral_sketch_id(
        placements
            .iter()
            .find(|placement| placement.entity_id == scope.base_flange_profile().unwrap().entity_id)
            .unwrap(),
    );
    unread_operation(&mut scope);
    let DesignScopePayloadMut::BaseFlange(slot) = scope.payload_mut() else {
        panic!("base flange");
    };
    let profile = slot.as_mut().unwrap().base_flange_profile.as_mut().unwrap();
    profile.region_selection = Some(
        crate::records::topology::sketch_profile::DesignSketchProfileRegionSelection {
            record_index: 200,
            byte_offset: 20,
            class_tag: profile.class_tag.clone(),
            region_count_offset: 40,
            regions: Vec::new(),
            atomic_regions: Vec::new(),
            companion_class_tag: profile.class_tag.clone(),
            companion_byte_offset: 60,
        },
    );
    let mut losses = Vec::new();
    let (definition, _) = project(&scope, &groups, &placements, &[], &mut losses).unwrap();
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
        profile: PlanarProfileRef::SketchSelection { sketch, selections },
        ..
    }) = definition
    else {
        panic!("selected profile in its sketch");
    };
    assert_eq!(sketch, expected_sketch);
    assert_eq!(selections.as_slice(), [scope.id]);
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::BaseFlangeProfileUnresolved.kind()));
}
