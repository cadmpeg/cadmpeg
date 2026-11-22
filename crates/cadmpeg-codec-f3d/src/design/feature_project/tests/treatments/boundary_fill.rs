// SPDX-License-Identifier: Apache-2.0

use super::{localized_fillet_group, localized_fillet_group_with_role};
use super::{localized_fillet_scope, localized_fillet_scope_without_owners};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayload};
use crate::records::identity::ReferenceRun;
use crate::records::topology::construction::DesignConstructionOperandRole;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

#[test]
fn boundary_fill_projects_body_region_groups() {
    let scope = localized_fillet_scope();
    let group = localized_fillet_group;
    let mut fill_scope = scope.clone();
    fill_scope
        .try_edit(|draft| {
            draft.payload = DesignFeatureKind::BoundaryFill.try_into().unwrap();
            draft.reference_members = ReferenceRun::unlocated(vec![100, 200, 201, 300, 301, 400]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut tools = group(100, 0, vec![200, 201]);
    tools.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    let mut cell = group(300, 3, vec![301]);
    cell.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5);
    let projected = crate::test_support::with_decode_context(|ctx| {
        crate::design::feature_project::boundary_fill::project(
            &fill_scope,
            &[tools.clone(), cell.clone()],
            ctx,
            &mut Vec::new(),
        )
        .unwrap()
    });
    assert!(matches!(
        projected,
        Some(FeatureDefinition::Operation(FeatureOperation::BoundaryFill {
            tools: cadmpeg_ir::features::selections::BodySelection::Native(ref tool_selection),
            cells: ref cell_selections,
            ..
        })) if tool_selection == &tools.id
            && cell_selections.as_slice() == [cadmpeg_ir::features::boundary_fill::BoundaryFillCell::BodyRegion(
                cadmpeg_ir::features::selections::BodySelection::Native(cell.id.clone())
            )]
    ));
    let wire = serde_json::to_value(&projected).unwrap();
    assert_eq!(wire["value"]["value"]["plane_tools"], serde_json::json!([]));
    assert_eq!(
        wire["value"]["value"]["cells"][0],
        serde_json::json!({"kind":"body_region","value":{"kind":"native","value":cell.id}})
    );
    let mut missing_planes = wire;
    missing_planes["value"]["value"]
        .as_object_mut()
        .unwrap()
        .remove("plane_tools");
    assert!(serde_json::from_value::<FeatureDefinition>(missing_planes).is_err());
}

#[test]
fn boundary_fill_projects_recorded_cells_and_removal_control() {
    let mut fill_scope = localized_fillet_scope_without_owners();
    fill_scope.source_kind = None;
    let tools =
        localized_fillet_group_with_role(100, 0, vec![200, 201], DesignOperandRole::BODIES_A);
    fill_scope
        .try_edit(|draft| {
            use crate::records::feature::boundary_fill::{
                DesignBoundaryFill, DesignBoundaryFillOperands, DesignBoundaryFillRemoval,
            };
            use std::num::NonZeroU32;
            draft.payload = DesignFeatureKind::BoundaryFill.try_into().unwrap();
            let DesignScopePayload::BoundaryFill(value) = &mut draft.payload else {
                panic!("boundary fill scope");
            };
            value.prefix = Some(DesignBoundaryFill {
                operation_word: 4,
                operands: Some(DesignBoundaryFillOperands {
                    body_groups: vec![NonZeroU32::new(100)],
                    plane_tools: vec![NonZeroU32::new(301), None],
                    cells: vec![NonZeroU32::new(777), None],
                    settings: NonZeroU32::new(400).unwrap(),
                    removal: Some(DesignBoundaryFillRemoval {
                        references: [Vec::new(), Vec::new()],
                        remove_tools: false,
                    }),
                    suffix: cadmpeg_ir::native::bytes::NativeBytes::new(Vec::new()),
                }),
            });
            draft.reference_count_offset = draft.byte_offset + 140;
            draft.reference_members = ReferenceRun::unlocated(vec![100, 200, 201, 301, 400]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let native_wire = serde_json::to_value(&fill_scope).unwrap();
    assert_eq!(
        serde_json::from_value::<DesignParameterScope>(native_wire.clone()).unwrap(),
        fill_scope
    );
    let mut invalid_wire = native_wire;
    invalid_wire["boundary_fill"]["operands"]["suffix"] = serde_json::json!("00".repeat(140));
    assert!(serde_json::from_value::<DesignParameterScope>(invalid_wire).is_err());
    let projected = crate::test_support::with_decode_context(|ctx| {
        crate::design::feature_project::boundary_fill::project(
            &fill_scope,
            &[tools],
            ctx,
            &mut Vec::new(),
        )
        .unwrap()
        .unwrap()
    });
    let wire = serde_json::to_value(&projected).unwrap();
    assert_eq!(
        wire["value"]["value"]["tools"]["value"],
        "f3d:native/BulkStream.dat:construction-group#100"
    );
    let operation = &wire["value"]["value"];
    assert_eq!(operation["operation"], "new_body");
    assert_eq!(operation["remove_tools"], false);
    assert_eq!(
        operation["plane_tools"],
        serde_json::json!(["f3d:native/BulkStream.dat:design-record#301", null])
    );
    assert_eq!(
        operation["cells"],
        serde_json::json!([
            {"kind":"recorded","value":{"record":"f3d:native/BulkStream.dat:design-record#777"}},
            {"kind":"recorded","value":{}}
        ])
    );
    let mut null_record = wire.clone();
    null_record["value"]["value"]["cells"][1]["value"]["record"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<FeatureDefinition>(null_record).is_err());
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire).unwrap(),
        projected
    );
}
