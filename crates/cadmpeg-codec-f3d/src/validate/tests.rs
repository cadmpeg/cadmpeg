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

use crate::records::topology::DesignConstructionOperandGroupFrame;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::records::feature::DesignScopePayload;
use crate::records::topology::DesignOperandRole;
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
    use crate::records::feature::{DesignComponentInsertConstruction, DesignParameterScope};
    use crate::records::DesignRecordHeader;

    let stream = "f3d:Design/BulkStream.dat";
    let scope_id = format!("{stream}:design-parameter-scope#100");
    let mut scope = DesignParameterScope::empty(
        &scope_id,
        crate::records::feature::DesignFeatureKind::ComponentInsert,
        169,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 100;
            draft.class_tag = crate::records::DesignClassTag::try_from("410".to_owned()).unwrap();
            draft.frame_length = 261;
            draft.kind_offset = 252;
            draft.reference_count_offset = 229;
            draft.reference_members = crate::records::ReferenceRun::from_columns(
                vec![167],
                vec![234],
                "reference_members",
            )
            .unwrap();
            draft.paired_class_tag =
                crate::records::DesignClassTag::try_from("261".to_owned()).unwrap();
            draft.paired_byte_offset = 361;
            draft.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
            draft.feature_ordinal_offset = 284;

            draft.previous_history_state_id_offset = Some(315);
        })
        .unwrap();
    if let crate::records::feature::DesignScopePayloadMut::ComponentInsert(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignComponentInsertConstruction {
            relation_record_index: 167,
            carrier_record_index: 166,
            occurrence_identity: Some(17),
            neutron_role: "cccccccc-dddd-eeee-ffff-000000000000".into(),
            neutron_role_offset: 259,
            placement: None,
        });
    }

    let mut ir = cadmpeg_ir::examples::unit_cube();
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_record_headers.extend([
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#167"),
                record_index: 167,
                class_tag: crate::records::DesignClassTag::try_from("310".to_owned()).unwrap(),
                byte_offset: 0,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#169"),
                record_index: 169,
                class_tag: crate::records::DesignClassTag::try_from("410".to_owned()).unwrap(),
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

    f3d_native_mut(&mut ir).design_parameter_scopes[0].paired_class_tag =
        crate::records::DesignClassTag::try_from("263".to_owned()).unwrap();
    let findings = crate::validate::validate_native(&ir);
    assert!(findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(scope_id.as_str())
            && finding.message == "Fusion Design parameter scope has an invalid paired frame"
    }));
}

#[test]
fn validation_accepts_only_the_class_397_symmetric_extent_frame() {
    use crate::records::feature::{
        DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeScope,
        DesignExtrudeStart, DesignFeatureKind, DesignParameterScope,
    };
    use crate::records::{DesignRecordHeader, ReferenceRun};

    let stream = "f3d:Design/BulkStream.dat";
    let scope_id = format!("{stream}:design-parameter-scope#0");
    let mut scope = DesignParameterScope::empty(&scope_id, DesignFeatureKind::Extrude, 3970);
    scope.class_tag = "397".to_owned().try_into().unwrap();
    scope.paired_class_tag = "262".to_owned().try_into().unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 473;
            draft.paired_byte_offset = 473;
            draft.reference_count_offset = 282;
            draft.reference_members = ReferenceRun::from_columns(
                vec![11, 22, 33, 44, 55, 66, 77, 88],
                vec![287, 298, 309, 320, 331, 342, 353, 364],
                "reference_members",
            )
            .unwrap();
            draft.kind_offset = 382;
            draft.feature_ordinal_offset = 395;
            draft.payload = DesignScopePayload::Extrude(Some(DesignExtrudeScope {
                extrude_prologue: Some(DesignExtrudePrologue::LegacyShifted {
                    operation_prefix_marker_offset: None,
                    operation: DesignExtrudeOperation::Cut,
                    operation_offset: 27,
                    direction_face_extend_values: [3, 2],
                    side_extent_discriminators: [1, 1],
                    side_extent_discriminator_offsets: [126, 139],
                    extent: Some(DesignExtrudeExtent::SymmetricDistance),
                    direction_face_extend_offsets: [31, 35],
                    direction_reversed: false,
                    direction_reversed_offset: 39,
                    solid_operation: true,
                    solid_operation_offset: 40,
                    start: DesignExtrudeStart::OffsetProfilePlane,
                    start_offset: 41,
                }),
                ..DesignExtrudeScope::default()
            }));
        })
        .unwrap();
    let mut ir = cadmpeg_ir::examples::unit_cube();
    {
        let mut native = f3d_native_mut(&mut ir);
        for record_index in [11, 22, 33, 44, 55, 66, 77, 88, 3970] {
            native.design_record_headers.push(DesignRecordHeader {
                id: format!("{stream}:design-record-header#{record_index}"),
                record_index,
                class_tag: "397".to_owned().try_into().unwrap(),
                byte_offset: 0,
            });
        }
        native.design_parameter_scopes.push(scope.clone());
    }
    let has_invalid_frame = |ir: &cadmpeg_ir::CadIr| {
        crate::validate::validate_native(ir).iter().any(|finding| {
            finding.entity.as_deref() == Some(scope_id.as_str())
                && finding.message == "Fusion Design parameter scope has an invalid paired frame"
        })
    };
    assert!(!has_invalid_frame(&ir));

    let mutations: &[fn(&mut DesignParameterScope)] = &[
        |scope| scope.class_tag = "338".to_owned().try_into().unwrap(),
        |scope| scope.paired_class_tag = "261".to_owned().try_into().unwrap(),
        |scope| {
            if let Some(DesignExtrudePrologue::LegacyShifted {
                side_extent_discriminators,
                ..
            }) = scope.extrude_prologue_mut()
            {
                *side_extent_discriminators = [1, 0];
            }
        },
        |scope| {
            if let Some(DesignExtrudePrologue::LegacyShifted {
                direction_face_extend_values,
                ..
            }) = scope.extrude_prologue_mut()
            {
                *direction_face_extend_values = [3, 1];
            }
        },
        |scope| {
            if let Some(DesignExtrudePrologue::LegacyShifted {
                side_extent_discriminator_offsets,
                ..
            }) = scope.extrude_prologue_mut()
            {
                *side_extent_discriminator_offsets = [116, 129];
            }
        },
        |scope| {
            if let Some(DesignExtrudePrologue::LegacyShifted { extent, .. }) =
                scope.extrude_prologue_mut()
            {
                *extent = Some(DesignExtrudeExtent::TwoSidedDistance);
            }
        },
    ];
    for (index, mutate) in mutations.iter().enumerate() {
        let mut invalid = scope.clone();
        mutate(&mut invalid);
        f3d_native_mut(&mut ir).design_parameter_scopes[0] = invalid;
        assert!(
            has_invalid_frame(&ir),
            "accepted invalid frame mutation {index}"
        );
    }
}

#[test]
fn validation_requires_timeline_items_to_resolve_through_the_type_table() {
    let meta_stream = "f3d:FusionAssetName[Active]/Design1/MetaStream.dat";
    let bulk_entry = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let design_type = |id: &str, type_guid: &str, entities: Vec<u64>| crate::records::SegmentType {
        id: id.into(),
        byte_offset: 0,
        type_guid: type_guid.to_owned().try_into().expect("type GUID"),
        type_guid_offset: 4,
        base_type_guid: (type_guid == crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID)
            .then(|| crate::records::RecordedValue {
                value: Some(
                    crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID
                        .to_owned()
                        .try_into()
                        .expect("base GUID"),
                ),
                offset: Some(8),
            }),
        version: if type_guid == crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID {
            crate::design::decode::meta::FEATURE_TIMELINE_TYPE_VERSIONS[1]
        } else {
            1
        },
        version_offset: 44,
        module: crate::records::DESIGN_MODULE_FUSION.into(),
        entities: crate::records::ReferenceRun::located(
            entities
                .into_iter()
                .map(|value| crate::records::Located { value, offset: 100 })
                .collect(),
        ),
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
        design_feature_timelines: vec![crate::records::DesignFeatureTimeline::try_new(
            crate::ids::native_design_feature_timeline_id(bulk_entry, 200),
            crate::records::DesignTimelineFrame::new(
                200,
                60,
                220,
                240,
                vec![crate::records::Located {
                    value: 101,
                    offset: 245,
                }],
            )
            .unwrap(),
            crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
            std::num::NonZeroU64::new(35).unwrap(),
            0,
            std::num::NonZeroU64::new(17).unwrap(),
        )
        .unwrap()],
        ..crate::native::F3dNative::default()
    };
    let mut ir = cadmpeg_ir::examples::unit_cube();
    native.store(ir.native.namespace_mut("f3d")).unwrap();
    let findings = crate::validate::validate_native(&ir);
    assert!(
        !findings.iter().any(|finding| {
            finding.message.contains("feature timeline")
                || finding.message.contains("feature-timeline")
        }),
        "{findings:#?}"
    );

    let mut duplicate_type_owner = native.clone();
    let mut entities = duplicate_type_owner.design_types[1]
        .entities
        .located_rows()
        .unwrap()
        .to_vec();
    entities.push(crate::records::Located {
        value: 35,
        offset: 108,
    });
    duplicate_type_owner.design_types[1].entities = crate::records::ReferenceRun::located(entities);
    duplicate_type_owner
        .store(ir.native.namespace_mut("f3d"))
        .unwrap();
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.entity.as_deref()
            == Some(
                duplicate_type_owner.design_feature_timelines[0]
                    .id()
                    .as_str(),
            )
            && finding.message == "Fusion Design feature timeline has an invalid typed frame"
    }));

    native.design_feature_timelines[0] = crate::records::DesignFeatureTimeline::try_new(
        native.design_feature_timelines[0].id().clone(),
        crate::records::DesignTimelineFrame::new(
            200,
            60,
            220,
            240,
            vec![crate::records::Located {
                value: 102,
                offset: 245,
            }],
        )
        .unwrap(),
        native.design_feature_timelines[0].class_tag.clone(),
        native.design_feature_timelines[0].record_index,
        native.design_feature_timelines[0].source_ordinal,
        native.design_feature_timelines[0].context_record_index,
    )
    .unwrap();
    native.store(ir.native.namespace_mut("f3d")).unwrap();
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.entity.as_deref() == Some(native.design_feature_timelines[0].id().as_str())
            && finding.message == "Fusion Design feature timeline has an invalid typed frame"
    }));
}

#[test]
fn validation_accepts_carrier_local_component_references() {
    use crate::records::feature::DesignComponentOccurrence;

    const COMPONENT: &str = "11111111-2222-4333-8444-555555555555";
    let occurrence = |record_index: u32,
                      byte_offset: u64,
                      component_record_index: u64,
                      occurrence_guid: &str| DesignComponentOccurrence {
        id: format!("f3d:Design/BulkStream.dat:design-component-occurrence#{record_index}"),
        class_tag: crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
        record_index,
        byte_offset,
        component_record_index,
        component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
        component_guid_offset: byte_offset + 48,
        occurrence_guid: occurrence_guid.to_owned().try_into().expect("GUID"),
        occurrence_guid_offset: byte_offset + 124,
        placement: crate::records::feature::DesignComponentOccurrencePlacement::Base,
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
    use crate::records::feature::{
        DesignCombineBodySelection, DesignCombineForm, DesignCombineOperation, DesignParameterScope,
    };
    use crate::records::topology::{
        DesignBodyRecipeOperand, DesignBodyRecipeReference, DesignOperandOwner,
    };
    use crate::records::{
        ConstructionRecipe, ConstructionRecipeKind, ConstructionRecipeSelector, DesignRecordHeader,
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
                crate::records::feature::DesignFeatureKind::Hole
            } else {
                crate::records::feature::DesignFeatureKind::Combine
            },
            scope_record_index,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members = crate::records::ReferenceRun::unlocated(if hole_scope {
                    vec![1, 2, 3, 4, 5, 6, operand_record_index]
                } else {
                    vec![1, 2, 3, 4, 5, operand_record_index]
                });
            })
            .unwrap();
        if let crate::records::feature::DesignScopePayloadMut::Combine(slot) = scope.payload_mut() {
            *slot = (!hole_scope).then_some(DesignCombineOperation {
                form: DesignCombineForm::Standard,
                operation: cadmpeg_ir::features::BooleanKind::Join,
                operation_offset: 0,
                keep_tools: false,
                keep_tools_offset: 0,
                target_record_index: if empty_legacy_tool {
                    operand_record_index + 1
                } else {
                    operand_record_index
                },
                tools: crate::records::feature::DesignCombineTools {
                    first: DesignCombineBodySelection {
                        record_index: if empty_legacy_tool {
                            operand_record_index
                        } else {
                            operand_record_index + 1
                        },
                        external_identity: None,
                    },
                    additional: vec![],
                },
            });
        }
        scopes.push(scope);
        headers.push(DesignRecordHeader {
            id: format!("{stream}:design-record-header#{operand_record_index}"),
            record_index: operand_record_index,
            class_tag: crate::records::DesignClassTag::try_from("365".to_owned()).unwrap(),
            byte_offset,
        });
        recipes.push(ConstructionRecipe {
            id: recipe_id.clone(),
            byte_offset: byte_offset + 220,
            record_index_offset: None,
            kind: ConstructionRecipeKind::Body,
            design: Some(crate::records::ConstructionRecipeDesign {
                id: crate::records::RecordedValue {
                    value: "301".into(),
                    offset: Some(byte_offset + 197),
                },
                selector: Some(ConstructionRecipeSelector {
                    value: operand_record_index + 4,
                    byte_offset: byte_offset + 200,
                }),
            }),
            recipe_index: ordinal,
            record_index: i32::try_from(operand_record_index + 3).unwrap(),
        });
        operands.push(
            DesignBodyRecipeOperand::try_new(
                crate::records::topology::DesignBodyRecipeOperandDraft {
                    id: format!("{stream}:design-body-recipe-operand#{operand_record_index}"),
                    scope_record_index,
                    owner: DesignOperandOwner::ScopeReference {
                        scope_reference_ordinal: if hole_scope { 6 } else { 5 },
                    },
                    record_index: operand_record_index,
                    byte_offset,
                    class_tag: crate::records::DesignClassTag::try_from("365".to_owned()).unwrap(),
                    asset_id: crate::records::DesignRelaxedGuidText::try_from(
                        "11111111-1111-4111-8111-111111111111".to_owned(),
                    )
                    .unwrap(),
                    asset_id_offset: byte_offset + if empty_legacy_tool { 44 } else { 56 },
                    context_id: crate::records::DesignRelaxedGuidText::try_from(
                        "22222222-2222-4222-8222-222222222222".to_owned(),
                    )
                    .unwrap(),
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
                    nested_record_index_offset: byte_offset
                        + if empty_legacy_tool { 26 } else { 38 },
                    recipe_id,
                    resolved_face_slot: None,
                    resolved_body_state_id: None,
                    resolved_body_slot: None,
                    resolved_body_face_slots: Vec::new(),
                    next_record_index: operand_record_index + 4,
                    next_byte_offset: byte_offset + 300,
                },
            )
            .unwrap(),
        );
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
    use crate::records::feature::DesignParameterScope;
    use crate::records::topology::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
    };
    use crate::records::DesignRecordHeader;

    let stream = "f3d:Design/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        crate::records::feature::DesignFeatureKind::Hole,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![100, 101, 200, 201]);
        })
        .unwrap();
    let group = |record_index: u32,
                 scope_reference_ordinal: u32,
                 member: u32,
                 byte_offset: u64,
                 role: DesignOperandRole| {
        let role_offset = byte_offset + 40;
        DesignConstructionOperandGroup::try_from(
            crate::records::topology::DesignConstructionOperandGroupDraft {
                id: format!("{stream}:design-construction-operand-group#{record_index}"),
                scope_record_index: 10,
                scope_reference_ordinal,
                record_index,
                byte_offset,
                class_tag: crate::records::DesignClassTag::try_from("277".to_owned()).unwrap(),
                members: vec![crate::records::Located {
                    value: member,
                    offset: byte_offset + 26,
                }],
                lost_edge_references: Vec::new(),
                frame: DesignConstructionOperandGroupFrame::try_from(
                    crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                        member_count_offset: byte_offset + 21,
                        auxiliary_records: Vec::new(),
                        auxiliary_paths: Vec::new(),
                        trailing_records: Vec::new(),
                        trailing_transforms: Vec::new(),
                        trailing_dual_transforms: Vec::new(),
                        trailing_flags: Vec::new(),
                        opaque_index: 1,
                        opaque_index_offset: role_offset + 18,
                        opaque_scalar: 0.0,
                        opaque_scalar_offset: role_offset + 22,
                        variant: false,
                    },
                )
                .unwrap(),
                operand_role: crate::records::topology::DesignConstructionOperandRole::Other(role),
                role_offset,
                paired_class_tag: crate::records::DesignClassTag::try_from("258".to_owned())
                    .unwrap(),
                paired_byte_offset: byte_offset + 80,
            },
        )
        .unwrap()
    };
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes.push(scope);
        native.design_construction_operand_groups.extend([
            group(100, 0, 101, 1_000, DesignOperandRole::BODIES_A),
            group(200, 2, 201, 2_000, DesignOperandRole::ROLE_0X5),
        ]);
        native.design_record_headers.extend([
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#100"),
                record_index: 100,
                class_tag: crate::records::DesignClassTag::try_from("277".to_owned()).unwrap(),
                byte_offset: 1_000,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#101"),
                record_index: 101,
                class_tag: crate::records::DesignClassTag::try_from("316".to_owned()).unwrap(),
                byte_offset: 1_100,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#200"),
                record_index: 200,
                class_tag: crate::records::DesignClassTag::try_from("277".to_owned()).unwrap(),
                byte_offset: 2_000,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#201"),
                record_index: 201,
                class_tag: crate::records::DesignClassTag::try_from("316".to_owned()).unwrap(),
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

    f3d_native_mut(&mut ir).design_construction_operand_groups[1].operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_B);
    assert!(crate::validate::validate_native(&ir)
        .iter()
        .any(invalid_frame));

    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes[0]
            .try_edit(|draft| {
                draft.payload = crate::records::feature::DesignFeatureKind::SurfaceTrim
                    .try_into()
                    .unwrap();
            })
            .unwrap();
        native.design_construction_operand_groups[1].operand_role =
            crate::records::topology::DesignConstructionOperandRole::Other(
                DesignOperandRole::ROLE_0X21,
            );
    }
    assert!(!crate::validate::validate_native(&ir)
        .iter()
        .any(invalid_frame));

    f3d_native_mut(&mut ir).design_construction_operand_groups[1].operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_B);
    assert!(crate::validate::validate_native(&ir)
        .iter()
        .any(invalid_frame));
}

#[test]
fn validation_checks_pipe_path_group_roles() {
    use crate::records::feature::{
        DesignExtrudeOperation, DesignParameterScope, DesignPathFeatureConstruction,
    };
    use crate::records::topology::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
    };
    use crate::records::DesignRecordHeader;

    let stream = "f3d:Design/BulkStream.dat";
    let scope_id = format!("{stream}:design-parameter-scope#10");
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let mut scope = DesignParameterScope::empty(
        &scope_id,
        crate::records::feature::DesignFeatureKind::Pipe,
        10,
    );
    {
        let value = Some(DesignPathFeatureConstruction::Pipe(
            crate::records::feature::DesignPipeConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 0,
                section_shape: crate::records::feature::DesignPipeSectionShape::Circular,
                section_shape_offset: 0,
                filled: true,
                filled_offset: 0,
                values: [1.0, 1.0, 0.6, 0.15],
                record_indexes: [11, 12, 13, 14],
                value_offsets: [0; 4],
            },
        ));
        scope
            .try_edit(|draft| {
                draft.payload = value.map_or_else(|| draft.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![1, 2, 3, 4, 20, 21]);
        })
        .unwrap();
    let path_group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::DesignConstructionOperandGroupDraft {
            id: format!("{stream}:design-construction-operand-group#20"),
            scope_record_index: 10,
            scope_reference_ordinal: 4,
            record_index: 20,
            byte_offset: 1_000,
            class_tag: crate::records::DesignClassTag::try_from("312".to_owned()).unwrap(),
            members: Vec::new(),
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1_021,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 1_058,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 1_062,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::DesignConstructionOperandRole::Other(
                DesignOperandRole::ROLE_0X5,
            ),
            role_offset: 1_040,
            paired_class_tag: crate::records::DesignClassTag::try_from("258".to_owned()).unwrap(),
            paired_byte_offset: 1_100,
        },
    )
    .unwrap();
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes.push(scope);
        native.design_construction_operand_groups.push(path_group);
        native.design_record_headers.extend([
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#20"),
                record_index: 20,
                class_tag: crate::records::DesignClassTag::try_from("312".to_owned()).unwrap(),
                byte_offset: 1_000,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#21"),
                record_index: 21,
                class_tag: crate::records::DesignClassTag::try_from("316".to_owned()).unwrap(),
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
        group
            .try_set_members(
                group
                    .members()
                    .iter()
                    .copied()
                    .chain([crate::records::Located {
                        value: 21,
                        offset: 1_026,
                    }])
                    .collect(),
            )
            .unwrap();
    }
    assert!(!has_role_finding(&ir));
    // The synthetic carrier is only a record header. No typed edge operand
    // exists yet, so the independent carrier finding remains.
    assert_eq!(group_native_finding_count(&ir), 1);

    f3d_native_mut(&mut ir).design_construction_operand_groups[0].operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_B);
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
        let source_id = native.sketch_points[0]
            .persistent_id()
            .expect("generated point identity");
        let mut form = native.sketch_points[1].record_form().clone();
        let crate::records::SketchPointRecordForm::Version11 { persistent_id, .. } = &mut form
        else {
            panic!("generated point has a version-11 record form");
        };
        *persistent_id = std::num::NonZeroU64::new(source_id).unwrap();
        native.sketch_points[1].try_set_record_form(form).unwrap();
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
        let source_id = native.sketch_points[0]
            .persistent_id()
            .expect("generated point identity");
        let mut form = native.sketch_points[1].record_form().clone();
        let crate::records::SketchPointRecordForm::Version11 { persistent_id, .. } = &mut form
        else {
            panic!("generated point has a version-11 record form");
        };
        *persistent_id = std::num::NonZeroU64::new(source_id).unwrap();
        native.sketch_points[1].try_set_record_form(form).unwrap();
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
        let source_id = native.sketch_points[0]
            .persistent_id()
            .expect("generated point identity");
        let mut form = native.sketch_points[1].record_form().clone();
        let crate::records::SketchPointRecordForm::Version11 { persistent_id, .. } = &mut form
        else {
            panic!("generated point has a version-11 record form");
        };
        *persistent_id = std::num::NonZeroU64::new(source_id).unwrap();
        native.sketch_points[1].try_set_record_form(form).unwrap();
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
        duplicate.entity_id =
            crate::records::DesignEntityId::from_parts("duplicate", duplicate.entity_id.suffix());
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
fn validation_accepts_user_design_parameter_frame() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let parameter =
        crate::records::DesignParameter::try_from(crate::records::DesignParameterDraft {
            id: "f3d:generated:design-parameter#0".into(),
            byte_offset: 100,
            class_tag: crate::records::DesignClassTag::try_from("305".to_owned()).unwrap(),
            record_index: 900,
            source_ordinal: 0,
            source: crate::records::DesignParameterSource::User {
                family_discriminator: crate::records::Located {
                    value: crate::records::DesignParameterDiscriminator::Code0,
                    offset: 122,
                },
            },
            expression: "60 mm".into(),
            expression_offset: 136,
            source_kind_offset: 166,

            unit: Some(crate::records::RecordedValue {
                value: "mm".into(),
                offset: Some(210),
            }),
            name: "Width".into(),
            name_offset: 220,
            evaluated_value: 6.0,
            evaluated_value_offset: 234,
        })
        .unwrap();
    f3d_native_mut(&mut ir).design_parameters.push(parameter);
    assert!(crate::validate::validate_native(&ir).is_empty());
}

#[test]
fn validation_accepts_legacy_owner_frames_and_ownerless_class_287_parameters() {
    use crate::records::{DesignParameterCompanion, DesignParameterOwner, DesignRecordHeader};

    const DESIGN_STREAM: &str = "Design/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let owned_parameter =
        crate::records::DesignParameter::try_from(crate::records::DesignParameterDraft {
            id: crate::ids::native_design_parameter_id(DESIGN_STREAM, 101),
            byte_offset: 1_068,
            class_tag: crate::records::DesignClassTag::try_from("305".to_owned()).unwrap(),
            record_index: 101,
            source_ordinal: 0,
            source: crate::records::DesignParameterSource::new(
                "Feature Input".into(),
                Some(100),
                None,
            )
            .unwrap(),
            expression: "6 cm".into(),
            expression_offset: 1_080,
            source_kind_offset: 1_100,

            unit: Some(crate::records::RecordedValue {
                value: "cm".into(),
                offset: Some(1_120),
            }),
            name: "Length".into(),
            name_offset: 1_130,
            evaluated_value: 6.0,
            evaluated_value_offset: 1_140,
        })
        .unwrap();
    let owner = DesignParameterOwner::try_from(crate::records::DesignParameterOwnerWire {
        id: crate::ids::native_design_parameter_owner_id(DESIGN_STREAM, 1_000),
        byte_offset: 1_000,
        frame_length: 68,
        class_tag: crate::records::DesignClassTag::try_from("268".to_owned()).unwrap(),
        record_index: 100,
        scope_record_index: 0,
        local_ordinal: 0,
        evaluated_value: 6.0,
        evaluated_value_offset: owned_parameter.evaluated_value_offset(),
        parameter_record_index: 101,
        owned_ordinal: 0,
        variant: None,
        companion_record_index: 102,
    })
    .unwrap();
    let companion = DesignParameterCompanion {
        id: crate::ids::native_design_parameter_companion_id(DESIGN_STREAM, 1_200),
        byte_offset: 1_200,
        class_tag: crate::records::DesignClassTag::try_from("258".to_owned()).unwrap(),
        record_index: 102,
        owner_record_index: 100,
        timestamp_micros: std::num::NonZeroU64::new(1).unwrap(),
        timestamp_micros_offset: 1_242,
        payload_byte_offset: 1_258,
        payload_byte_length: 0,
        owned_recipe_ids: Vec::new(),
    };
    let ownerless_parameter =
        crate::records::DesignParameter::try_from(crate::records::DesignParameterDraft {
            id: crate::ids::native_design_parameter_id(DESIGN_STREAM, 201),
            byte_offset: 1_400,
            class_tag: crate::records::DesignClassTag::try_from("287".to_owned()).unwrap(),
            record_index: 201,
            source_ordinal: 1,
            source: crate::records::DesignParameterSource::new(
                "Feature Input".into(),
                Some(200),
                None,
            )
            .unwrap(),
            expression: "OffsetX".into(),
            expression_offset: 1_440,
            source_kind_offset: 1_470,

            unit: None,
            name: "OffsetX".into(),
            name_offset: 1_490,
            evaluated_value: 0.0,
            evaluated_value_offset: 1_510,
        })
        .unwrap();
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
                class_tag: crate::records::DesignClassTag::try_from("268".to_owned()).unwrap(),
                byte_offset: 1_000,
            },
            DesignRecordHeader {
                id: crate::ids::native_scoped_id(DESIGN_STREAM, "record-header", 101),
                record_index: 101,
                class_tag: crate::records::DesignClassTag::try_from("305".to_owned()).unwrap(),
                byte_offset: 1_068,
            },
            DesignRecordHeader {
                id: crate::ids::native_scoped_id(DESIGN_STREAM, "record-header", 102),
                record_index: 102,
                class_tag: crate::records::DesignClassTag::try_from("258".to_owned()).unwrap(),
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
    use crate::records::feature::{
        DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
        DesignParameterScope,
    };
    use crate::records::topology::{DesignConstructionOperandGroup, DesignSketchProfileOperand};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let profile = DesignSketchProfileOperand::try_new(
        crate::records::topology::DesignSketchProfileOperandDraft {
            scope_reference_ordinal: 0,
            record_index: 20,
            byte_offset: 200,
            class_tag: crate::records::DesignClassTag::try_from("300".to_owned()).unwrap(),
            asset_id: crate::records::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 230,
            entity_id: crate::records::DesignEntityId::try_from("0_10".to_owned())
                .expect("valid entity identity"),
            entity_reference_offset: 250,
            region_selection: None,
            paired_class_tag: crate::records::DesignClassTag::try_from("260".to_owned()).unwrap(),
            paired_byte_offset: 300,
        },
    )
    .unwrap();
    let scope = DesignParameterScope::try_new(crate::records::feature::DesignParameterScopeDraft {
        id: "f3d:test:scope#10".into(),
        byte_offset: 100,
        class_tag: crate::records::DesignClassTag::try_from("301".to_owned()).unwrap(),
        record_index: 10,
        frame_length: 200,
        kind_offset: 210,
        payload: DesignScopePayload::Extrude(Some(crate::records::feature::DesignExtrudeScope {
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
            ..crate::records::feature::DesignExtrudeScope::default()
        })),
        feature_ordinal: std::num::NonZeroU32::MIN,
        feature_ordinal_offset: 220,
        history_state_id: None,

        previous_history_state_id: None,
        previous_history_state_id_offset: Some(228),
        reference_count_offset: 180,
        reference_members: crate::records::ReferenceRun::from_columns(
            vec![20, 30],
            vec![184, 195],
            "reference_members",
        )
        .unwrap(),
        unclosed_construction_operand_groups: Vec::new(),
        paired_class_tag: crate::records::DesignClassTag::try_from("261".to_owned()).unwrap(),
        paired_byte_offset: 300,
    })
    .unwrap();
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::DesignConstructionOperandGroupDraft {
            id: "f3d:test:operand-group#30".into(),
            scope_record_index: 10,
            scope_reference_ordinal: 1,
            record_index: 30,
            byte_offset: 400,
            class_tag: crate::records::DesignClassTag::try_from("302".to_owned()).unwrap(),
            members: vec![crate::records::Located {
                value: 20,
                offset: 424,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 420,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::Located {
                        value: 31,
                        offset: 440,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 468,
                    opaque_scalar: 0.5,
                    opaque_scalar_offset: 472,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::DesignConstructionOperandRole::ExtrudeProfile,
            role_offset: 450,

            paired_class_tag: crate::records::DesignClassTag::try_from("262".to_owned()).unwrap(),
            paired_byte_offset: 500,
        },
    )
    .unwrap();
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
    use crate::records::topology::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
        DesignConstructionOperandIdentity, DesignConstructionPersistentIdentity,
    };
    use crate::records::DesignRecordHeader;

    let stream = "f3d:Design/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::DesignConstructionOperandGroupDraft {
            id: format!("{stream}:operand-group#100"),
            scope_record_index: 10,
            scope_reference_ordinal: 0,
            record_index: 100,
            byte_offset: 1_000,
            class_tag: crate::records::DesignClassTag::try_from("271".to_owned()).unwrap(),
            members: Vec::new(),
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1_021,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::Located {
                        value: 101,
                        offset: 1_025,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 1_059,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 1_063,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::DesignConstructionOperandRole::Other(
                DesignOperandRole::from_raw(0),
            ),
            role_offset: 1_041,
            paired_class_tag: crate::records::DesignClassTag::try_from("261".to_owned()).unwrap(),
            paired_byte_offset: 1_050,
        },
    )
    .unwrap();
    let identity = DesignConstructionOperandIdentity::try_new(
        crate::records::topology::DesignConstructionOperandIdentityDraft {
            id: format!("{stream}:operand-identity#1100"),
            group_record_index: 100,
            wrappers: vec![crate::records::topology::DesignIdentityWrapper {
                record_index: 101,
                byte_offset: 1_100,
                class_tag: crate::records::DesignClassTag::try_from("384".to_owned()).unwrap(),
            }],
            following_record_index: 102,
            following_byte_offset: 1_124,
            following_class_tag: crate::records::DesignClassTag::try_from("395".to_owned())
                .unwrap(),
            tracking_path: None,
            persistent_identity: Some(
                DesignConstructionPersistentIdentity::try_new(
                    crate::records::topology::DesignConstructionPersistentIdentityDraft {
                        local_id: 167,
                        local_id_offset: 1_145,
                        asset_id: crate::records::DesignRelaxedGuidText::try_from(
                            "2d0697b6-f6c5-4f86-bb58-4a2f413c99d3".to_owned(),
                        )
                        .unwrap(),
                        asset_id_offset: 1_157,
                        context_id: crate::records::DesignRelaxedGuidText::try_from(
                            "9dea94a1-729a-4032-930b-d4ba4eaadb0c".to_owned(),
                        )
                        .unwrap(),
                        context_id_offset: 1_233,
                        tail_slot_present: false,
                        tail_slot_offset: 1_309,
                        next_record_index: 103,
                        next_byte_offset: 1_314,
                    },
                )
                .unwrap(),
            ),
        },
    )
    .unwrap();
    let wrapper = DesignRecordHeader {
        id: format!("{stream}:record-header#1100"),
        record_index: 101,
        class_tag: crate::records::DesignClassTag::try_from("384".to_owned()).unwrap(),
        byte_offset: 1_100,
    };
    let following = DesignRecordHeader {
        id: format!("{stream}:record-header#1124"),
        record_index: 102,
        class_tag: crate::records::DesignClassTag::try_from("395".to_owned()).unwrap(),
        byte_offset: 1_124,
    };
    let identity_id = identity.id.clone();
    let mut native = crate::native::F3dNative::default();
    native.design_construction_operand_groups.push(group);
    native.design_construction_operand_identities.push(identity);
    native.design_record_headers.extend([wrapper, following]);
    native.store(ir.native.namespace_mut("f3d")).unwrap();

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
        class_tag: crate::records::DesignClassTag::try_from("301".to_owned()).unwrap(),
        byte_offset: 1_315,
    });
    native.store(ir.native.namespace_mut("f3d")).unwrap();
    assert!(crate::validate::validate_native(&ir)
        .iter()
        .any(invalid_identity));
}

#[test]
fn validation_accepts_class_338_sketch_curve_entity_selection_frame() {
    use crate::records::topology::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
        DesignEntitySelectionOperand,
    };
    use crate::records::DesignRecordHeader;

    let stream = "f3d:Design/BulkStream.dat";
    let group_id = format!("{stream}:design-construction-operand-group#100");
    let operand_id = format!("{stream}:design-entity-selection-operand#1000");
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::DesignConstructionOperandGroupDraft {
            id: group_id,
            scope_record_index: 10,
            scope_reference_ordinal: 0,
            record_index: 100,
            byte_offset: 900,
            class_tag: crate::records::DesignClassTag::try_from("277".to_owned()).unwrap(),
            members: vec![crate::records::Located {
                value: 200,
                offset: 926,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 921,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 971,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 975,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::DesignConstructionOperandRole::ExtrudeProfile,
            role_offset: 953,
            paired_class_tag: crate::records::DesignClassTag::try_from("265".to_owned()).unwrap(),
            paired_byte_offset: 1024,
        },
    )
    .unwrap();
    let header = DesignRecordHeader {
        id: format!("{stream}:design-record-header#1000"),
        byte_offset: 1_000,
        class_tag: crate::records::DesignClassTag::try_from("338".to_owned()).unwrap(),
        record_index: 200,
    };
    let operand = DesignEntitySelectionOperand::try_new(
        crate::records::topology::DesignEntitySelectionOperandDraft {
            id: operand_id.clone(),
            scope_record_index: 10,
            group_record_index: 100,
            group_member_ordinal: 0,
            record_index: 200,
            byte_offset: 1_000,
            class_tag: crate::records::DesignClassTag::try_from("338".to_owned()).unwrap(),
            asset_id: crate::records::DesignRelaxedGuidText::try_from(
                "11111111-2222-4333-8444-555555555555".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 1_034,
            context_id: crate::records::DesignRelaxedGuidText::try_from(
                "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".to_owned(),
            )
            .unwrap(),
            context_id_offset: 1_100,
            identity_record_index: 203,
            identity_record_offset: 2_000,
            primary_identity: 949,
            primary_identity_offset: 2033,
            secondary: Some(crate::records::DesignSecondaryIdentity {
                identity: crate::records::Located {
                    value: 249,
                    offset: 2041,
                },
                curve_identity: None,
            }),
            historical_edge_candidates: Vec::new(),
            historical_face_candidates: Vec::new(),
            resolved_edge_slot: None,
            next_record_index: 204,
            next_byte_offset: 2049,
        },
    )
    .unwrap();
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
}
