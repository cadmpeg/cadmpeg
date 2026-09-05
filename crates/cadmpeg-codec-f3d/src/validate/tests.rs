// SPDX-License-Identifier: Apache-2.0
//! Native-validation unit tests for Fusion Design records.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::records::DesignScopePayload;
use crate::test_support::*;
use crate::F3dCodec;

fn recipe_reference() -> crate::records::DesignRecipeReference {
    crate::records::DesignRecipeReference {
        selector: 1,
        selector_offset: 20,
        token: "3".into(),
        token_offset: 28,
        design_reference: 329,
        design_reference_offset: 37,
        candidate_faces: vec![
            cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#10").expect("identity grammar")
        ],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    }
}

#[test]
fn finalized_recipe_reference_validation_ignores_only_derived_candidates() {
    let actual = recipe_reference();
    let mut expected = actual.clone();
    expected.candidate_faces =
        vec![cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#20").expect("identity grammar")];
    assert!(super::recipe_reference_frames_match(
        &[actual.clone()],
        &[expected.clone()],
        true,
    ));
    assert!(!super::recipe_reference_frames_match(
        &[actual.clone()],
        &[expected.clone()],
        false,
    ));

    expected.design_reference += 1;
    assert!(!super::recipe_reference_frames_match(
        &[actual],
        &[expected],
        true,
    ));
}

#[test]
fn validation_accepts_class_410_component_insert_identity_frame() {
    use crate::records::{
        DesignComponentInsertConstruction, DesignParameterScope, DesignRecordHeader,
    };

    let stream = "f3d:Design/BulkStream.dat";
    let scope_id = format!("{stream}:design-parameter-scope#100");
    let mut scope = DesignParameterScope::empty(
        &scope_id,
        crate::records::DesignFeatureKind::ComponentInsert,
        169,
    );
    scope.byte_offset = 100;
    scope.class_tag = "410".into();
    scope.frame_length = 261;
    scope.kind_offset = 252;
    scope.reference_count_offset = 229;
    scope.reference_members = vec![167];
    scope.reference_member_offsets = vec![234];
    scope.paired_class_tag = "261".into();
    scope.paired_byte_offset = 361;
    scope.feature_ordinal = 1;
    scope.feature_ordinal_offset = 284;
    scope.history_state_id_offset = 244;
    scope.previous_history_state_id_offset = Some(315);
    if let crate::records::DesignScopePayload::ComponentInsert(slot) = &mut scope.payload {
        *slot = Some(DesignComponentInsertConstruction {
            relation_record_index: 167,
            carrier_record_index: 166,
            occurrence_identity: Some(17),
            neutron_role: "cccccccc-dddd-eeee-ffff-000000000000".into(),
            neutron_role_offset: 259,
            transform: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            transform_offset: None,
            carrier_transform_offset: None,
        });
    }

    let mut ir = cadmpeg_ir::examples::unit_cube();
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_record_headers.extend([
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#167"),
                record_index: 167,
                class_tag: "310".into(),
                byte_offset: 0,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#169"),
                record_index: 169,
                class_tag: "410".into(),
                byte_offset: 100,
            },
        ]);
        native.design_parameter_scopes.push(scope);
    }

    let findings = crate::validate::validate_native(&ir);
    assert!(!findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(scope_id.as_str())
            && finding.message == "Fusion Design parameter scope has an invalid paired frame"
    }));

    f3d_native_mut(&mut ir).design_parameter_scopes[0].paired_class_tag = "263".into();
    let findings = crate::validate::validate_native(&ir);
    assert!(findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(scope_id.as_str())
            && finding.message == "Fusion Design parameter scope has an invalid paired frame"
    }));
}

#[test]
fn validation_requires_timeline_items_to_resolve_through_the_type_table() {
    let meta_stream = "f3d:FusionAssetName[Active]/Design1/MetaStream.dat";
    let bulk_entry = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let design_type = |id: &str, type_guid: &str, entities: Vec<u64>| crate::records::SegmentType {
        id: id.into(),
        byte_offset: 0,
        type_guid: type_guid.into(),
        type_guid_offset: 4,
        base_type_guid: (type_guid == crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID)
            .then(|| crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID.into()),
        base_type_guid_offset: (type_guid
            == crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID)
            .then_some(8),
        version: if type_guid == crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID {
            crate::design::decode::meta::FEATURE_TIMELINE_TYPE_VERSIONS[1]
        } else {
            1
        },
        version_offset: 44,
        module: crate::records::DESIGN_MODULE_FUSION.into(),
        entity_id_offsets: vec![100; entities.len()],
        entity_ids: entities,
    };
    let mut native = crate::native::F3dNative {
        design_types: vec![
            design_type(
                &format!("{meta_stream}:design-type#0"),
                crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
                vec![35],
            ),
            design_type(
                &format!("{meta_stream}:design-type#1"),
                "11111111-2222-3333-4444-555555555555",
                vec![17, 101],
            ),
        ],
        design_feature_timelines: vec![crate::records::DesignFeatureTimeline {
            id: crate::ids::native_design_feature_timeline_id(bulk_entry, 200),
            byte_offset: 200,
            class_tag: "256".into(),
            record_index: 35,
            source_ordinal: 0,
            frame_length: 60,
            context_record_index: 17,
            context_record_index_offset: 220,
            item_count_offset: 240,
            item_record_indices: vec![101],
            item_record_index_offsets: vec![245],
        }],
        ..crate::native::F3dNative::default()
    };
    let mut ir = cadmpeg_ir::examples::unit_cube();
    native
        .store(ir.native.namespace_mut("f3d", std::num::NonZeroU32::MIN))
        .unwrap();
    let findings = crate::validate::validate_native(&ir);
    assert!(
        !findings.iter().any(|finding| {
            finding.message.contains("feature timeline")
                || finding.message.contains("feature-timeline")
        }),
        "{findings:#?}"
    );

    let mut duplicate_type_owner = native.clone();
    duplicate_type_owner.design_types[1].entity_ids.push(35);
    duplicate_type_owner.design_types[1]
        .entity_id_offsets
        .push(108);
    duplicate_type_owner
        .store(ir.native.namespace_mut("f3d", std::num::NonZeroU32::MIN))
        .unwrap();
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.entity.as_deref()
            == Some(duplicate_type_owner.design_feature_timelines[0].id.as_str())
            && finding.message == "Fusion Design feature timeline has an invalid typed frame"
    }));

    let mut invalid_offsets = native.clone();
    invalid_offsets.design_feature_timelines[0].item_record_index_offsets[0] = 244;
    invalid_offsets
        .store(ir.native.namespace_mut("f3d", std::num::NonZeroU32::MIN))
        .unwrap();
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.entity.as_deref() == Some(invalid_offsets.design_feature_timelines[0].id.as_str())
            && finding.message == "Fusion Design feature timeline has an invalid typed frame"
    }));

    native.design_feature_timelines[0].item_record_indices[0] = 102;
    native
        .store(ir.native.namespace_mut("f3d", std::num::NonZeroU32::MIN))
        .unwrap();
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.entity.as_deref() == Some(native.design_feature_timelines[0].id.as_str())
            && finding.message == "Fusion Design feature timeline has an invalid typed frame"
    }));
}

#[test]
fn validation_accepts_carrier_local_component_references() {
    use crate::records::DesignComponentOccurrence;

    const COMPONENT: &str = "11111111-2222-4333-8444-555555555555";
    let occurrence = |record_index: u32,
                      byte_offset: u64,
                      component_record_index: u64,
                      occurrence_guid: &str| DesignComponentOccurrence {
        id: format!("f3d:Design/BulkStream.dat:design-component-occurrence#{record_index}"),
        class_tag: "256".into(),
        record_index,
        byte_offset,
        component_record_index,
        component_guid: COMPONENT.into(),
        component_guid_offset: byte_offset + 48,
        occurrence_guid: occurrence_guid.into(),
        occurrence_guid_offset: byte_offset + 124,
        occurrence_ordinal: 1,
        transform: None,
        transform_offset: None,
    };
    let mut ir = cadmpeg_ir::examples::unit_cube();
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_component_occurrences.extend([
            occurrence(100, 1_000, 700, "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"),
            occurrence(101, 2_000, 701, "aaaaaaaa-bbbb-4ccc-8ddd-ffffffffffff"),
        ]);
    }

    let findings = crate::validate::validate_native(&ir);
    assert!(!findings.iter().any(|finding| {
        finding.message == "Fusion Design component occurrence has an invalid fixed frame"
    }));
}

#[test]
fn validation_scopes_direct_body_operand_ordinals_by_owning_scope() {
    use crate::records::{
        ConstructionRecipe, ConstructionRecipeKind, ConstructionRecipeSelector,
        DesignBodyRecipeOperand, DesignBodyRecipeReference, DesignCombineBodySelection,
        DesignCombineForm, DesignCombineOperation, DesignExtrudeOperation, DesignOperandOwner,
        DesignParameterScope, DesignRecordHeader,
    };

    let stream = "f3d:Design/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let mut scopes = Vec::new();
    let mut headers = Vec::new();
    let mut recipes = Vec::new();
    let mut operands = Vec::new();
    for ordinal in 0..3u32 {
        let scope_record_index = 10 + ordinal;
        let operand_record_index = 100 + ordinal * 10;
        let byte_offset = 1_000 + u64::from(ordinal) * 1_000;
        let recipe_id = format!("{stream}:construction-recipe#{ordinal}");
        let empty_legacy_tool = ordinal == 0;
        let hole_scope = ordinal == 2;
        let mut scope = DesignParameterScope::empty(
            &format!("{stream}:design-parameter-scope#{scope_record_index}"),
            if hole_scope {
                crate::records::DesignFeatureKind::Hole
            } else {
                crate::records::DesignFeatureKind::Combine
            },
            scope_record_index,
        );
        scope.reference_members = if hole_scope {
            vec![1, 2, 3, 4, 5, 6, operand_record_index]
        } else {
            vec![1, 2, 3, 4, 5, operand_record_index]
        };
        if let crate::records::DesignScopePayload::Combine(slot) = &mut scope.payload {
            *slot = (!hole_scope).then_some(DesignCombineOperation {
                form: DesignCombineForm::Standard,
                operation: DesignExtrudeOperation::Join,
                operation_offset: 0,
                keep_tools: false,
                keep_tools_offset: 0,
                target: DesignCombineBodySelection {
                    record_index: if empty_legacy_tool {
                        operand_record_index + 1
                    } else {
                        operand_record_index
                    },
                    external_identity: None,
                },
                tools: vec![DesignCombineBodySelection {
                    record_index: if empty_legacy_tool {
                        operand_record_index
                    } else {
                        operand_record_index + 1
                    },
                    external_identity: None,
                }],
            });
        }
        scopes.push(scope);
        headers.push(DesignRecordHeader {
            id: format!("{stream}:design-record-header#{operand_record_index}"),
            record_index: operand_record_index,
            class_tag: "365".into(),
            byte_offset,
        });
        recipes.push(ConstructionRecipe {
            id: recipe_id.clone(),
            byte_offset: byte_offset + 220,
            record_index_offset: None,
            kind: ConstructionRecipeKind::Body,
            design_id: Some("301".into()),
            design_id_offset: Some(byte_offset + 197),
            design_selector: Some(ConstructionRecipeSelector {
                value: operand_record_index + 4,
                byte_offset: byte_offset + 200,
            }),
            recipe_index: ordinal,
            record_index: i32::try_from(operand_record_index + 3).unwrap(),
        });
        operands.push(DesignBodyRecipeOperand {
            id: format!("{stream}:design-body-recipe-operand#{operand_record_index}"),
            scope_record_index,
            owner: DesignOperandOwner::ScopeReference {
                scope_reference_ordinal: if hole_scope { 6 } else { 5 },
            },
            record_index: operand_record_index,
            byte_offset,
            class_tag: "365".into(),
            asset_id: "11111111-1111-4111-8111-111111111111".into(),
            asset_id_offset: byte_offset + if empty_legacy_tool { 44 } else { 56 },
            context_id: "22222222-2222-4222-8222-222222222222".into(),
            context_id_offset: byte_offset + if empty_legacy_tool { 124 } else { 136 },
            selector_tail: None,

            references: if empty_legacy_tool {
                Vec::new()
            } else {
                vec![DesignBodyRecipeReference {
                    design_reference: u64::from(300 + ordinal),
                    design_reference_offset: byte_offset + 25,
                    form: 3,
                    form_offset: byte_offset + 33,
                    candidate_faces: Vec::new(),
                    preceding_candidate_faces: Vec::new(),
                    preceding_body_slots: Vec::new(),
                }]
            },
            nested_record_index: u64::from(operand_record_index + 3),
            nested_record_index_offset: byte_offset + if empty_legacy_tool { 26 } else { 38 },
            recipe_id,
            resolved_face_slot: None,
            resolved_body_state_id: None,
            resolved_body_slot: None,
            resolved_body_face_slots: Vec::new(),
            next_record_index: operand_record_index + 4,
            next_byte_offset: byte_offset + 300,
        });
    }
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes = scopes;
        native.design_record_headers = headers;
        native.construction_recipes = recipes;
        native.design_body_recipe_operands = operands;
    }

    let findings = crate::validate::validate_native(&ir);
    let invalid_operands = findings
        .iter()
        .filter(|finding| {
            finding.message == "Fusion Design body recipe operand has an invalid nested frame"
        })
        .collect::<Vec<_>>();
    assert!(invalid_operands.is_empty(), "{invalid_operands:#?}");
}

#[test]
fn validation_accepts_hole_and_surface_trim_construction_group_roles() {
    use crate::records::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame, DesignParameterScope,
        DesignRecordHeader,
    };

    let stream = "f3d:Design/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        crate::records::DesignFeatureKind::Hole,
        10,
    );
    scope.reference_members = vec![100, 101, 200, 201];
    let group = |record_index: u32,
                 scope_reference_ordinal: u32,
                 member: u32,
                 byte_offset: u64,
                 role: u64| {
        let role_offset = byte_offset + 40;
        DesignConstructionOperandGroup {
            id: format!("{stream}:design-construction-operand-group#{record_index}"),
            scope_record_index: 10,
            scope_reference_ordinal,
            record_index,
            byte_offset,
            class_tag: "277".into(),
            members: vec![member],
            lost_edge_references: Vec::new(),
            member_offsets: vec![byte_offset + 26],
            frame: DesignConstructionOperandGroupFrame {
                member_count_offset: byte_offset + 21,
                auxiliary_record_indices: Vec::new(),
                auxiliary_record_offsets: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_record_indices: Vec::new(),
                trailing_record_offsets: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: role_offset + 18,
                opaque_scalar: 0.0,
                opaque_scalar_offset: role_offset + 22,
                variant: false,
            },
            role,
            extrude_role: None,
            role_offset,
            paired_class_tag: "258".into(),
            paired_byte_offset: byte_offset + 80,
        }
    };
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes.push(scope);
        native.design_construction_operand_groups.extend([
            group(100, 0, 101, 1_000, 0x0000_0004_0000_0000),
            group(200, 2, 201, 2_000, 0x0000_0005_0000_0000),
        ]);
        native.design_record_headers.extend([
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#100"),
                record_index: 100,
                class_tag: "277".into(),
                byte_offset: 1_000,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#101"),
                record_index: 101,
                class_tag: "316".into(),
                byte_offset: 1_100,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#200"),
                record_index: 200,
                class_tag: "277".into(),
                byte_offset: 2_000,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#201"),
                record_index: 201,
                class_tag: "316".into(),
                byte_offset: 2_100,
            },
        ]);
    }

    let invalid_frame = |finding: &cadmpeg_ir::Finding| {
        finding.message == "Fusion Design construction operand group has an invalid frame"
    };
    assert!(!crate::validate::validate_native(&ir)
        .iter()
        .any(invalid_frame));

    f3d_native_mut(&mut ir).design_construction_operand_groups[1].role = 0x0000_0008_0000_0000;
    assert!(crate::validate::validate_native(&ir)
        .iter()
        .any(invalid_frame));

    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes[0].payload =
            crate::records::DesignFeatureKind::SurfaceTrim.into();
        native.design_construction_operand_groups[1].role = 0x0000_0021_0000_0000;
    }
    assert!(!crate::validate::validate_native(&ir)
        .iter()
        .any(invalid_frame));

    f3d_native_mut(&mut ir).design_construction_operand_groups[1].role = 0x0000_0008_0000_0000;
    assert!(crate::validate::validate_native(&ir)
        .iter()
        .any(invalid_frame));
}

#[test]
fn validation_checks_pipe_path_group_roles() {
    use crate::records::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
        DesignExtrudeOperation, DesignParameterScope, DesignPathFeatureConstruction,
        DesignRecordHeader,
    };

    let stream = "f3d:Design/BulkStream.dat";
    let scope_id = format!("{stream}:design-parameter-scope#10");
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let mut scope =
        DesignParameterScope::empty(&scope_id, crate::records::DesignFeatureKind::Pipe, 10);
    {
        let value = Some(DesignPathFeatureConstruction::Pipe(crate::records::DesignPipeConstruction {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 0,
            section_shape: crate::records::DesignPipeSectionShape::Circular,
            section_shape_offset: 0,
            filled: true,
            filled_offset: 0,
            values: [1.0, 1.0, 0.6, 0.15],
            record_indexes: [11, 12, 13, 14],
            value_offsets: [0; 4],
        }));
        scope.payload = value.map_or_else(|| scope.kind().into(), Into::into);
    }
    scope.reference_members = vec![1, 2, 3, 4, 20, 21];
    let path_group = DesignConstructionOperandGroup {
        id: format!("{stream}:design-construction-operand-group#20"),
        scope_record_index: 10,
        scope_reference_ordinal: 4,
        record_index: 20,
        byte_offset: 1_000,
        class_tag: "312".into(),
        members: Vec::new(),
        lost_edge_references: Vec::new(),
        member_offsets: Vec::new(),
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset: 1_021,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 1_058,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 1_062,
            variant: false,
        },
        role: 0x0000_0005_0000_0000,
        extrude_role: None,
        role_offset: 1_040,
        paired_class_tag: "258".into(),
        paired_byte_offset: 1_100,
    };
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes.push(scope);
        native.design_construction_operand_groups.push(path_group);
        native.design_record_headers.extend([
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#20"),
                record_index: 20,
                class_tag: "312".into(),
                byte_offset: 1_000,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#21"),
                record_index: 21,
                class_tag: "316".into(),
                byte_offset: 1_200,
            },
        ]);
    }

    let has_role_finding = |ir: &cadmpeg_ir::CadIr| {
        crate::validate::validate_native(ir).iter().any(|finding| {
            finding.entity.as_deref() == Some(scope_id.as_str())
                && finding.message
                    == "Fusion Design path-feature operand roles conflict with its construction"
        })
    };
    assert!(has_role_finding(&ir));

    let group_native_finding_count = |ir: &cadmpeg_ir::CadIr| {
        crate::validate::validate_native(ir)
            .iter()
            .filter(|finding| {
                finding.entity.as_deref()
                    == Some("f3d:Design/BulkStream.dat:design-construction-operand-group#20")
                    && finding.check == cadmpeg_ir::Check::NativeLinks
            })
            .count()
    };
    // The empty group violates both its counted frame and its typed-member
    // carrier invariant. The validator reports those independent failures.
    assert_eq!(group_native_finding_count(&ir), 2);

    {
        let mut native = f3d_native_mut(&mut ir);
        let group = &mut native.design_construction_operand_groups[0];
        group.members.push(21);
        group.member_offsets.push(1_026);
    }
    assert!(!has_role_finding(&ir));
    // The synthetic carrier is only a record header. No typed edge operand
    // exists yet, so the independent carrier finding remains.
    assert_eq!(group_native_finding_count(&ir), 1);

    f3d_native_mut(&mut ir).design_construction_operand_groups[0].role = 0x0000_0008_0000_0000;
    assert_eq!(group_native_finding_count(&ir), 2);
}

#[test]
fn validation_rejects_duplicate_sketch_geometry_persistent_identities() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let (point_id, curve_id) = {
        let mut native = f3d_native_mut(&mut ir);
        assert!(native.sketch_points.len() >= 2);
        assert!(native.sketch_curve_identities.len() >= 2);
        let persistent_id = native.sketch_points[0].persistent_id();
        native.sketch_points[1].set_persistent_id(persistent_id);
        native.sketch_points[0].owner_reference = Some(100);
        native.sketch_points[1].owner_reference = Some(100);
        native.sketch_curve_identities[1].primary_id = native.sketch_curve_identities[0].primary_id;
        native.sketch_curve_identities[1].secondary_id =
            native.sketch_curve_identities[0].secondary_id;
        native.sketch_curve_identities[0].owner_reference = Some(100);
        native.sketch_curve_identities[1].owner_reference = Some(100);
        (
            native.sketch_points[1].id.clone(),
            native.sketch_curve_identities[1].id.clone(),
        )
    };

    let findings = crate::validate::validate_native(&ir);
    assert!(findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some(point_id.as_str())
            && finding.message.contains("persistent identity")
    }));
    assert!(findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some(curve_id.as_str())
            && finding.message.contains("persistent identity")
    }));
}

#[test]
fn validation_accepts_sketch_geometry_persistent_identities_reused_by_another_owner() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let (point_id, curve_id) = {
        let mut native = f3d_native_mut(&mut ir);
        assert!(native.sketch_points.len() >= 2);
        assert!(native.sketch_curve_identities.len() >= 2);
        let persistent_id = native.sketch_points[0].persistent_id();
        native.sketch_points[1].set_persistent_id(persistent_id);
        native.sketch_points[0].owner_reference = Some(100);
        native.sketch_points[1].owner_reference = Some(101);
        native.sketch_curve_identities[1].primary_id = native.sketch_curve_identities[0].primary_id;
        native.sketch_curve_identities[1].secondary_id =
            native.sketch_curve_identities[0].secondary_id;
        native.sketch_curve_identities[0].owner_reference = Some(100);
        native.sketch_curve_identities[1].owner_reference = Some(101);
        (
            native.sketch_points[1].id.clone(),
            native.sketch_curve_identities[1].id.clone(),
        )
    };

    assert!(
        !crate::validate::validate_native(&ir).iter().any(|finding| {
            finding.check == cadmpeg_ir::Check::NativeLinks
                && (finding.entity.as_deref() == Some(point_id.as_str())
                    || finding.entity.as_deref() == Some(curve_id.as_str()))
                && finding.message.contains("persistent identity")
        })
    );
}

#[test]
fn validation_accepts_sketch_geometry_identities_with_unknown_owner() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    {
        let mut native = f3d_native_mut(&mut ir);
        assert!(native.sketch_points.len() >= 2);
        assert!(native.sketch_curve_identities.len() >= 2);
        let persistent_id = native.sketch_points[0].persistent_id();
        native.sketch_points[1].set_persistent_id(persistent_id);
        native.sketch_points[0].owner_reference = None;
        native.sketch_points[1].owner_reference = None;
        native.sketch_curve_identities[1].primary_id = native.sketch_curve_identities[0].primary_id;
        native.sketch_curve_identities[1].secondary_id =
            native.sketch_curve_identities[0].secondary_id;
        native.sketch_curve_identities[0].owner_reference = None;
        native.sketch_curve_identities[1].owner_reference = None;
    }

    assert!(
        !crate::validate::validate_native(&ir).iter().any(|finding| {
            finding.check == cadmpeg_ir::Check::NativeLinks
                && finding.message.contains("persistent identity")
        })
    );
}

#[test]
fn validation_rejects_aliased_sketch_geometry_records() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let curve_id = {
        let mut native = f3d_native_mut(&mut ir);
        let point_record_index = native.sketch_points[0].record_index;
        native.sketch_curve_identities[0].record_index = point_record_index;
        native.sketch_curve_identities[0].id.clone()
    };

    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some(curve_id.as_str())
            && finding
                .message
                .contains("aliases another typed indexed record")
    }));
}

#[test]
fn validation_rejects_duplicate_design_entity_suffixes() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let duplicate_id = {
        let mut native = f3d_native_mut(&mut ir);
        let mut duplicate = native
            .design_entity_headers
            .first()
            .expect("generated Design entity header")
            .clone();
        duplicate.id.push_str("-duplicate");
        duplicate.entity_id.push_str(":duplicate");
        let id = duplicate.entity_id.clone();
        native.design_entity_headers.push(duplicate);
        id
    };

    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some(duplicate_id.as_str())
            && finding.message.contains("entity suffix is duplicated")
    }));
}

#[test]
fn validation_rejects_invalid_design_parameter_family_and_owner() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let parameter = crate::records::DesignParameter {
        id: "generated:design-parameter#0".into(),
        byte_offset: 100,
        class_tag: "305".into(),
        record_index: 900,
        family_discriminator: Some(crate::records::Located { value: 0, offset: 122 }),
        source_ordinal: 0,
        owner: crate::records::DesignParameterOwnerKind::from_kind(
            crate::records::DesignParameterKind::User,
            None,
        ),
        expression: "60 mm".into(),
        expression_offset: 136,
        source_kind: "User Parameter".into(),
        source_kind_offset: 166,

        unit: Some("mm".into()),
        unit_offset: Some(210),
        name: "Width".into(),
        name_offset: 220,
        evaluated_value: 6.0,
        evaluated_value_offset: 234,
    };
    f3d_native_mut(&mut ir).design_parameters.push(parameter);
    assert!(crate::validate::validate_native(&ir).is_empty());

    f3d_native_mut(&mut ir).design_parameters[0].family_discriminator = Some(crate::records::Located { value: 7, offset: 122 });
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some("generated:design-parameter#0")
            && finding.message.contains("family discriminator")
    }));
    f3d_native_mut(&mut ir).design_parameters[0].family_discriminator = Some(crate::records::Located { value: 0, offset: 122 });

    {
        let parameter = &mut f3d_native_mut(&mut ir).design_parameters[0];
        parameter.family_discriminator = None;
    }
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some("generated:design-parameter#0")
            && finding.message.contains("family discriminator")
    }));
    {
        let parameter = &mut f3d_native_mut(&mut ir).design_parameters[0];
        parameter.family_discriminator = Some(crate::records::Located { value: 0, offset: 122 });
    }

    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameters[0].owner = crate::records::DesignParameterOwnerKind::Feature {
            owner_record_index: 1234,
        };
    }
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some("generated:design-parameter#0")
    }));
}

#[test]
fn validation_accepts_legacy_owner_frames_and_ownerless_class_287_parameters() {
    use crate::records::{
        DesignParameter, DesignParameterCompanion, DesignParameterKind, DesignParameterOwner,
        DesignRecordHeader,
    };

    const DESIGN_STREAM: &str = "Design/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let owned_parameter = DesignParameter {
        id: crate::ids::native_design_parameter_id(DESIGN_STREAM, 101),
        byte_offset: 1_068,
        class_tag: "305".into(),
        record_index: 101,
        family_discriminator: None,
        source_ordinal: 0,
        owner: crate::records::DesignParameterOwnerKind::from_kind(
            DesignParameterKind::Feature,
            Some(100),
        ),
        expression: "6 cm".into(),
        expression_offset: 1_080,
        source_kind: "Feature Input".into(),
        source_kind_offset: 1_100,

        unit: Some("cm".into()),
        unit_offset: Some(1_120),
        name: "Length".into(),
        name_offset: 1_130,
        evaluated_value: 6.0,
        evaluated_value_offset: 1_140,
    };
    let owner = DesignParameterOwner {
        id: crate::ids::native_design_parameter_owner_id(DESIGN_STREAM, 1_000),
        byte_offset: 1_000,
        frame_length: 68,
        class_tag: "268".into(),
        record_index: 100,
        scope_record_index: 0,
        local_ordinal: 0,
        evaluated_value: 6.0,
        evaluated_value_offset: owned_parameter.evaluated_value_offset,
        parameter_record_index: 101,
        owned_ordinal: 0,
        variant: None,
        companion_record_index: 102,
    };
    let companion = DesignParameterCompanion {
        id: crate::ids::native_design_parameter_companion_id(DESIGN_STREAM, 1_200),
        byte_offset: 1_200,
        class_tag: "258".into(),
        record_index: 102,
        owner_record_index: 100,
        timestamp_micros: 1,
        timestamp_micros_offset: 1_242,
        payload_byte_offset: 1_258,
        payload_byte_length: 0,
        owned_recipe_ids: Vec::new(),
    };
    let ownerless_parameter = DesignParameter {
        id: crate::ids::native_design_parameter_id(DESIGN_STREAM, 201),
        byte_offset: 1_400,
        class_tag: "287".into(),
        record_index: 201,
        family_discriminator: None,
        source_ordinal: 1,
        owner: crate::records::DesignParameterOwnerKind::from_kind(
            DesignParameterKind::Feature,
            Some(200),
        ),
        expression: "OffsetX".into(),
        expression_offset: 1_440,
        source_kind: "Feature Input".into(),
        source_kind_offset: 1_470,

        unit: None,
        unit_offset: None,
        name: "OffsetX".into(),
        name_offset: 1_490,
        evaluated_value: 0.0,
        evaluated_value_offset: 1_510,
    };
    {
        let mut native = f3d_native_mut(&mut ir);
        native
            .design_parameters
            .extend([owned_parameter, ownerless_parameter]);
        native.design_parameter_owners.push(owner);
        native.design_parameter_companions.push(companion);
        native.design_record_headers.extend([
            DesignRecordHeader {
                id: crate::ids::native_scoped_id(DESIGN_STREAM, "record-header", 100),
                record_index: 100,
                class_tag: "268".into(),
                byte_offset: 1_000,
            },
            DesignRecordHeader {
                id: crate::ids::native_scoped_id(DESIGN_STREAM, "record-header", 101),
                record_index: 101,
                class_tag: "305".into(),
                byte_offset: 1_068,
            },
            DesignRecordHeader {
                id: crate::ids::native_scoped_id(DESIGN_STREAM, "record-header", 102),
                record_index: 102,
                class_tag: "258".into(),
                byte_offset: 1_200,
            },
        ]);
    }

    let findings = crate::validate::validate_native(&ir);
    assert!(
        findings.iter().all(|finding| {
            !finding
                .message
                .contains("Fusion Design parameter owner has an invalid frame")
                && !finding
                    .message
                    .contains("Fusion Design parameter has an invalid frame")
        }),
        "{findings:#?}"
    );
}

#[test]
fn validation_accepts_grouped_and_direct_extrude_profiles() {
    use crate::records::{
        DesignConstructionOperandGroup, DesignExtrudeExtent, DesignExtrudeOperandRole,
        DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart, DesignParameterScope,
        DesignSketchProfileOperand,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let profile = DesignSketchProfileOperand {
        scope_reference_ordinal: 0,
        record_index: 20,
        byte_offset: 200,
        class_tag: "300".into(),
        asset_id: "asset".into(),
        asset_id_offset: 230,
        entity_id: "0_10".into(),
        entity_suffix: 10,
        entity_reference_offset: 250,
        region_selection: None,
        paired_class_tag: "260".into(),
        paired_byte_offset: 300,
    };
    let scope = DesignParameterScope {
        id: "f3d:test:scope#10".into(),
        byte_offset: 100,
        class_tag: "301".into(),
        record_index: 10,
        frame_length: 200,
        kind_offset: 210,
        payload: DesignScopePayload::Extrude(Some(crate::records::DesignExtrudeScope {
            extrude_prologue: Some(DesignExtrudePrologue::ReferenceAware {
                reference: None,
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 128,
                direction_face_extend_values: [1, 2],
                side_extent_discriminators: [1, 0],
                side_extent_discriminator_offsets: [177, 190],
                first_side_target_ordinal: None,
                extent: DesignExtrudeExtent::OneSidedDistance,
                direction_face_extend_offsets: [132, 136],
                direction_reversed: false,
                direction_reversed_offset: 140,
                solid_operation: true,
                solid_operation_offset: 141,
                start: DesignExtrudeStart::ProfilePlane,
                start_offset: 142,
            }),
            extrude_profile: Some(profile),
            ..crate::records::DesignExtrudeScope::default()
        })),
        feature_ordinal: 1,
        feature_ordinal_offset: 220,
        history_state_id: None,
        history_state_id_offset: 224,
        previous_history_state_id: None,
        previous_history_state_id_offset: Some(228),
        reference_count_offset: 180,
        reference_members: vec![20, 30],
        reference_member_offsets: vec![184, 195],
        unclosed_construction_operand_groups: Vec::new(),
        paired_class_tag: "261".into(),
        paired_byte_offset: 300,
    };
    let group = DesignConstructionOperandGroup {
        id: "f3d:test:operand-group#30".into(),
        scope_record_index: 10,
        scope_reference_ordinal: 1,
        record_index: 30,
        byte_offset: 400,
        class_tag: "302".into(),
        members: vec![20],
        lost_edge_references: Vec::new(),
        member_offsets: vec![424],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 420,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![31],
            trailing_record_offsets: vec![440],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 460,
            opaque_scalar: 0.5,
            opaque_scalar_offset: 464,
            variant: false,
        },
        role: 0x0000_0041_0000_0000,
        extrude_role: Some(DesignExtrudeOperandRole::Profile),
        role_offset: 450,

        paired_class_tag: "262".into(),
        paired_byte_offset: 500,
    };
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes.push(scope);
        native
            .design_construction_operand_groups
            .push(group.clone());
    }
    let profile_message = |finding: &cadmpeg_ir::Finding| {
        finding.message == "Fusion Design Extrude profile conflicts with its profile operand group"
    };
    let findings = crate::validate::validate_native(&ir);
    assert!(!findings.iter().any(profile_message));
    assert!(!findings
        .iter()
        .any(|finding| finding.message.contains("no counted selection group")));

    f3d_native_mut(&mut ir)
        .design_construction_operand_groups
        .push(group);
    assert!(crate::validate::validate_native(&ir)
        .iter()
        .any(profile_message));

    let profile = f3d_native_mut(&mut ir).design_parameter_scopes[0]
        .extrude_mut()
        .unwrap()
        .extrude_profile
        .take();
    assert!(!crate::validate::validate_native(&ir)
        .iter()
        .any(profile_message));
    f3d_native_mut(&mut ir).design_parameter_scopes[0]
        .extrude_mut()
        .unwrap()
        .extrude_profile = profile;

    f3d_native_mut(&mut ir)
        .design_construction_operand_groups
        .clear();
    assert!(!crate::validate::validate_native(&ir)
        .iter()
        .any(profile_message));

    f3d_native_mut(&mut ir).design_parameter_scopes[0]
        .extrude_profile_mut()
        .expect("test Extrude profile")
        .scope_reference_ordinal = 1;
    assert!(crate::validate::validate_native(&ir)
        .iter()
        .any(profile_message));
}

#[test]
fn validation_accepts_unindexed_construction_identity_terminal() {
    use crate::records::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
        DesignConstructionOperandIdentity, DesignConstructionPersistentIdentity,
        DesignRecordHeader,
    };

    let stream = "f3d:Design/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let group = DesignConstructionOperandGroup {
        id: format!("{stream}:operand-group#100"),
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 1_000,
        class_tag: "271".into(),
        members: Vec::new(),
        lost_edge_references: Vec::new(),
        member_offsets: Vec::new(),
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset: 1_021,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![101],
            trailing_record_offsets: vec![1_025],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 1_029,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 1_033,
            variant: false,
        },
        role: 0,
        extrude_role: None,
        role_offset: 1_041,
        paired_class_tag: "261".into(),
        paired_byte_offset: 1_050,
    };
    let identity = DesignConstructionOperandIdentity {
        id: format!("{stream}:operand-identity#1100"),
        group_record_index: 100,
        wrapper_record_indices: vec![101],
        wrapper_byte_offsets: vec![1_100],
        wrapper_class_tags: vec!["384".into()],
        following_record_index: 102,
        following_byte_offset: 1_124,
        following_class_tag: "395".into(),
        tracking_path: None,
        persistent_identity: Some(DesignConstructionPersistentIdentity {
            local_id: 167,
            local_id_offset: 1_145,
            asset_id: "2d0697b6-f6c5-4f86-bb58-4a2f413c99d3".into(),
            asset_id_offset: 1_157,
            context_id: "9dea94a1-729a-4032-930b-d4ba4eaadb0c".into(),
            context_id_offset: 1_233,
            tail_slot_present: false,
            tail_slot_offset: 1_309,
            next_record_index: 103,
            next_byte_offset: 1_314,
        }),
    };
    let wrapper = DesignRecordHeader {
        id: format!("{stream}:record-header#1100"),
        record_index: 101,
        class_tag: "384".into(),
        byte_offset: 1_100,
    };
    let following = DesignRecordHeader {
        id: format!("{stream}:record-header#1124"),
        record_index: 102,
        class_tag: "395".into(),
        byte_offset: 1_124,
    };
    let identity_id = identity.id.clone();
    let mut native = crate::native::F3dNative::default();
    native.design_construction_operand_groups.push(group);
    native.design_construction_operand_identities.push(identity);
    native.design_record_headers.extend([wrapper, following]);
    native
        .store(ir.native.namespace_mut("f3d", std::num::NonZeroU32::MIN))
        .unwrap();

    let invalid_identity = |finding: &cadmpeg_ir::Finding| {
        finding.entity.as_deref() == Some(identity_id.as_str())
            && finding.message.contains("invalid nested frame")
    };
    assert!(!crate::validate::validate_native(&ir)
        .iter()
        .any(invalid_identity));

    let mut native = crate::native::F3dNative::load(ir.native.namespace("f3d").unwrap()).unwrap();
    native.design_record_headers.push(DesignRecordHeader {
        id: format!("{stream}:record-header#1315"),
        record_index: 103,
        class_tag: "301".into(),
        byte_offset: 1_315,
    });
    native
        .store(ir.native.namespace_mut("f3d", std::num::NonZeroU32::MIN))
        .unwrap();
    assert!(crate::validate::validate_native(&ir)
        .iter()
        .any(invalid_identity));
}

#[test]
fn validation_accepts_class_338_sketch_curve_entity_selection_frame() {
    use crate::records::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
        DesignEntitySelectionOperand, DesignRecordHeader,
    };

    let stream = "f3d:Design/BulkStream.dat";
    let group_id = format!("{stream}:design-construction-operand-group#100");
    let operand_id = format!("{stream}:design-entity-selection-operand#1000");
    let group = DesignConstructionOperandGroup {
        id: group_id,
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 900,
        class_tag: "277".into(),
        members: vec![200],
        lost_edge_references: Vec::new(),
        member_offsets: vec![926],
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset: 921,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 971,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 975,
            variant: false,
        },
        role: 0x41_0000_0000,
        extrude_role: Some(crate::records::DesignExtrudeOperandRole::Profile),
        role_offset: 953,
        paired_class_tag: "265".into(),
        paired_byte_offset: 1024,
    };
    let header = DesignRecordHeader {
        id: format!("{stream}:design-record-header#1000"),
        byte_offset: 1_000,
        class_tag: "338".into(),
        record_index: 200,
    };
    let operand = DesignEntitySelectionOperand {
        id: operand_id.clone(),
        scope_record_index: 10,
        group_record_index: 100,
        group_member_ordinal: 0,
        record_index: 200,
        byte_offset: 1_000,
        class_tag: "338".into(),
        asset_id: "11111111-2222-4333-8444-555555555555".into(),
        asset_id_offset: 1_034,
        context_id: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into(),
        context_id_offset: 1_100,
        identity_record_index: 203,
        identity_record_offset: 2_000,
        primary_identity: 949,
        primary_identity_offset: 2_033,
        secondary_identity: Some(249),
        secondary_identity_offset: Some(2_041),
        curve_secondary_identity: None,
        curve_secondary_identity_offset: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: 204,
        next_byte_offset: 2_049,
    };
    let mut ir = cadmpeg_ir::examples::unit_cube();
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_construction_operand_groups.push(group);
        native.design_record_headers.push(header);
        native.design_entity_selection_operands.push(operand);
    }

    let invalid_entity_selection = |finding: &cadmpeg_ir::Finding| {
        finding.entity.as_deref() == Some(operand_id.as_str())
            && finding.message
                == "Fusion Design entity-selection operand has an invalid nested frame"
    };
    assert!(!crate::validate::validate_native(&ir)
        .iter()
        .any(invalid_entity_selection));

    f3d_native_mut(&mut ir).design_entity_selection_operands[0].next_byte_offset = 2_048;
    assert!(crate::validate::validate_native(&ir)
        .iter()
        .any(invalid_entity_selection));
}
