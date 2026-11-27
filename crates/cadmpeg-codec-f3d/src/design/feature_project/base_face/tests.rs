// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    design::test_support::name_scope_references,
    records::{
        entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData},
        feature::scope::DesignFeatureKind,
        identity::ReferenceRun,
        sheet_metal_settings::DesignSheetMetalSettings,
        topology::face::DesignFaceSourceGroup,
    },
};
use cadmpeg_core::serde_erase::json;

fn registration(version: u32) -> SegmentType {
    SegmentType::try_new(
        "f3d:test/Design1/MetaStream.dat:design-type#10".into(),
        SegmentTypeData {
            byte_offset: 10,
            type_guid: "7AFA0A65-B53C-472C-ACA4-5C7C0A65001C"
                .to_owned()
                .try_into()
                .unwrap(),
            type_guid_offset: 14,
            base_type_guid: BaseTypeGuid::Absent,
            version,
            version_offset: 50,
            module: "Fusion".into(),
            entities: ReferenceRun::unlocated(vec![10]),
        },
    )
    .unwrap()
}

fn settings() -> DesignSheetMetalSettings {
    json::from_value(cadmpeg_core::json!({
        "id": "f3d:test/Design1/BulkStream.dat:design-sheet-metal-settings#300",
        "record_index": 13, "byte_offset": 300, "paired_byte_offset": 400, "mask": 0,
        "inputs": { "version": "v2", "slots": [null, null, null, null, null, null, null] },
        "rule_record_index": 14, "presentation_record_index": 15, "owner_record_index": 10,
        "context_record_index": 16, "rule_thickness": 0.125, "rule_thickness_offset": 850,
    }))
    .unwrap()
}

#[test]
fn base_face_projects_a_registered_source_and_owned_rule() {
    let mut scope = DesignParameterScope::empty(
        "f3d:test/Design1/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Face,
        10,
    );
    name_scope_references(&mut scope, [12, 13]);
    let group: DesignFaceSourceGroup = json::from_value(cadmpeg_core::json!({
        "id": "f3d:test/Design1/BulkStream.dat:design-face-source-group#100",
        "scope_record_index": 10, "carrier_reference_ordinal": 0,
        "carrier_record_index": 12, "carrier_byte_offset": 100, "carrier_class_tag": "257",
        "carrier_frame_length": 200, "paired_record_index": 13,
        "paired_byte_offset": 300, "paired_class_tag": "258",
        "source_reference_offsets": [136], "source_members": [{
            "record_index": 20, "byte_offset": 420, "class_tag": "259",
            "persistent_identity": {
                "local_id": 1, "local_id_offset": 441,
                "asset_id": "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE", "asset_id_offset": 453,
                "context_id": "11111111-2222-4333-8444-555555555555", "context_id_offset": 529,
                "tail_slot_present": false, "tail_slot_offset": 605,
                "next_record_index": 21, "next_byte_offset": 610
            }
        }]
    }))
    .unwrap();
    let settings = settings();
    for version in [3, 4] {
        let inputs = ProjectInputs {
            design_types: &[registration(version)],
            face_source_groups: std::slice::from_ref(&group),
            sheet_metal_settings: std::slice::from_ref(&settings),
            ..Default::default()
        };
        let mut losses = Vec::new();
        assert_eq!(
            project(&scope, &inputs, &mut losses),
            Some(FeatureDefinition::Operation(
                FeatureOperation::SheetMetalBaseFlange {
                    profile: PlanarProfileRef::Native(group.id.clone()),
                    thickness: PositiveLength::new(1.25),
                    side: None,
                }
            ))
        );
        assert_eq!(
            losses.into_iter().map(|note| note.code).collect::<Vec<_>>(),
            [
                F3dLossCode::BaseFaceRegionsNative.kind(),
                F3dLossCode::BaseFaceMaterialSideUnresolved.kind(),
                F3dLossCode::BaseFaceResultChoiceUnresolved.kind(),
            ]
        );
    }
}

#[test]
fn base_face_requires_registered_scope_and_unique_owned_settings() {
    let mut scope = DesignParameterScope::empty(
        "f3d:test/Design1/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Face,
        10,
    );
    name_scope_references(&mut scope, [12, 13]);
    let mut inputs = ProjectInputs {
        design_types: &[registration(5)],
        sheet_metal_settings: &[settings(), settings()],
        ..Default::default()
    };
    let mut losses = Vec::new();
    assert!(project(&scope, &inputs, &mut losses).is_none());
    assert!(losses.is_empty());
    let admitted = [registration(4)];
    inputs.design_types = &admitted;
    assert_eq!(
        project(&scope, &inputs, &mut losses),
        Some(FeatureDefinition::Operation(
            FeatureOperation::SheetMetalBaseFlange {
                profile: PlanarProfileRef::Unresolved(scope.id.clone()),
                thickness: None,
                side: None,
            }
        ))
    );
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::BaseFaceThicknessUnresolved.kind()));
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::BaseFaceSourceCarrierUnresolved.kind()));
}
