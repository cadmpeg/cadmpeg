// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::features::surfaces::SplitFaceMethod;

use cadmpeg_core::decode::u64_from_index;

use crate::design::feature_project::delete_face::project_delete_face;
use crate::design::feature_project::direct_face::project_split;
use crate::design::feature_project::selected_work_planes;
use crate::design::feature_project::split_face::project_split_face;
use crate::ids::feature_input_topology_id;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::{
    construction::DesignConstructionOperandGroupFrame, extrude_selection::DesignOperandRole,
};
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;

fn group(
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    members: Vec<u32>,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: format!("f3d:Design/BulkStream.dat:group#{record_index}"),
            scope_record_index,
            scope_reference_ordinal,
            record_index,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("262".to_owned())
                .unwrap(),

            members: members
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::identity::Located {
                    value,
                    offset: u64_from_index(index) * 11,
                })
                .collect(),
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(role),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "258".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap()
}

fn split_body_scope() -> DesignParameterScope {
    split_body_scope_referencing(vec![100, 101, 102, 103])
}

fn split_body_scope_referencing(references: Vec<u32>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::scope::DesignFeatureKind::Split,
        77,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn split_body_groups(tool_role: DesignOperandRole) -> [DesignConstructionOperandGroup; 2] {
    [
        group(77, 0, 100, vec![101], tool_role),
        group(77, 2, 102, vec![103], DesignOperandRole::BODIES_A),
    ]
}

fn split_body_face_tool() -> crate::records::topology::face::DesignFaceOperand {
    split_body_face_tool_at(101, 1)
}

fn split_body_face_tool_at(
    record_index: u32,
    scope_ordinal: u32,
) -> crate::records::topology::face::DesignFaceOperand {
    use crate::records::recipes::ConstructionRecipeKind;
    crate::records::topology::face::DesignFaceOperand::try_new(
        crate::records::topology::face::DesignFaceOperandDraft {
            id: format!("f3d:Design/BulkStream.dat:face-operand#{record_index}"),
            scope_record_index: 77,
            scope_reference_ordinal: scope_ordinal,
            group: Some(crate::records::topology::body_recipe::DesignOperandGroup {
                group_record_index: 100,
                group_member_ordinal: scope_ordinal - 1,
            }),
            record_index,
            byte_offset: 1200,
            class_tag: "297".to_owned().try_into().unwrap(),
            paired_byte_offset: 1250,
            paired_class_tag: "259".to_owned().try_into().unwrap(),
            recipe_record_index: record_index + 3,
            recipe_record_byte_offset: 1300,
            recipe_id: format!("f3d:Design/BulkStream.dat:recipe#{}", record_index + 3),
            recipe_prefix_offset: 1311,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_kind: ConstructionRecipeKind::Face,
            recipe_program_offset: 1350,
            recipe_program: vec![0, -1],
            recipe_nodes: Vec::new(),
            candidate_faces: Vec::new(),
            unreferenced_candidate_faces: Vec::new(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: Vec::new(),
            resolved_active_face: None,
            next_record_index: record_index + 4,
            next_byte_offset: 1411,
        },
    )
    .unwrap()
}

#[test]
fn split_body_path_tool_keeps_group_id() {
    let scope = split_body_scope();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X21);
    let definition = project_split(&scope, &[], &groups, &[], &[], &[], &mut Vec::new())
        .unwrap()
        .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::SplitBody {
            tool: cadmpeg_ir::features::direct_edit::SplitBodyTool::Faces(FaceSelection::Native(ref tool)), ..
        }) if tool == &groups[0].id
    ));
}

#[test]
fn split_body_face_tool_keeps_native_id() {
    let scope = split_body_scope();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X9);
    let tool = split_body_face_tool();
    let definition = project_split(
        &scope,
        &[],
        &groups,
        &[],
        std::slice::from_ref(&tool),
        &[],
        &mut Vec::new(),
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::SplitBody {
            tool: cadmpeg_ir::features::direct_edit::SplitBodyTool::Faces(FaceSelection::Native(ref native)), ..
        }) if native == &tool.id
    ));
}

#[test]
fn split_body_face_tool_group_resolves_every_member_or_retains_the_group() {
    use cadmpeg_ir::features::direct_edit::SplitBodyTool;
    let mut scope = split_body_scope_referencing(vec![100, 101, 201, 102, 103]);
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let groups = [
        group(77, 0, 100, vec![101, 201], DesignOperandRole::ROLE_0X9),
        group(77, 3, 102, vec![103], DesignOperandRole::BODIES_A),
    ];
    let mut operands = [split_body_face_tool(), split_body_face_tool_at(201, 2)];
    let slots = [42, 11];
    for (operand, slot) in operands.iter_mut().zip(slots) {
        let face = cadmpeg_ir::ids::FaceId::mint(format!("f3d:model:face#{slot}")).unwrap();
        operand.candidate_faces = vec![face.clone()];
        operand.preceding_candidate_faces = vec![face];
    }
    let mut losses = Vec::new();
    let definition = project_split(&scope, &[], &groups, &[], &operands, &[], &mut losses)
        .unwrap()
        .unwrap();
    let feature = crate::ids::neutral_feature_id(&scope);
    let prefix = crate::ids::feature_input_prefix(&feature, 7);
    let expected = slots.map(|slot| crate::ids::history_input_face_id(&prefix, slot));
    assert!(matches!(
        split_body_tool(&definition),
        SplitBodyTool::Faces(FaceSelection::Historical { faces, native, .. })
            if faces.as_slice() == expected.as_slice() && native.as_str() == groups[0].id.as_str()
    ));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureSplitBodyFaceToolGroupAssumed.kind()
    );

    operands[1].preceding_candidate_faces.clear();
    let definition = project_split(&scope, &[], &groups, &[], &operands, &[], &mut Vec::new())
        .unwrap()
        .unwrap();
    assert_eq!(
        split_body_tool(&definition),
        &SplitBodyTool::Faces(FaceSelection::Native(groups[0].id.clone()))
    );
    assert!(project_split(
        &scope,
        &[],
        &groups,
        &[],
        &operands[..1],
        &[],
        &mut Vec::new()
    )
    .unwrap()
    .is_none());
}

#[test]
fn split_body_admits_zero_terminal_face_tool_with_an_assumption() {
    let scope = split_body_scope();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X9);
    let mut tool = split_body_face_tool();
    tool.recipe_program = vec![0, 0];
    let mut losses = Vec::new();
    let definition = project_split(&scope, &[], &groups, &[], &[tool], &[], &mut losses)
        .unwrap()
        .unwrap();
    assert_eq!(
        split_body_tool(&definition),
        &cadmpeg_ir::features::direct_edit::SplitBodyTool::Faces(FaceSelection::Native(
            groups[0].id.clone()
        ))
    );
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureSplitBodyFaceToolGroupAssumed.kind()
    );
}

#[test]
fn split_body_historical_face_tool_keeps_native_id() {
    let mut scope = split_body_scope();
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X9);
    let mut tool = split_body_face_tool();
    tool.resolved_face_slots = vec![42];
    let definition = project_split(
        &scope,
        &[],
        &groups,
        &[],
        std::slice::from_ref(&tool),
        &[],
        &mut Vec::new(),
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::SplitBody {
            tool: cadmpeg_ir::features::direct_edit::SplitBodyTool::Faces(FaceSelection::Historical { ref native, .. }), ..
        }) if native.as_str() == tool.id
    ));
}

#[test]
fn delete_face_falls_back_to_group_id() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::scope::DesignFeatureKind::DeleteFace,
        77,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 258;
            draft.kind_offset = draft.byte_offset + 161;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 200]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = draft.kind_offset - 34;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let selected = group(77, 0, 100, vec![200], DesignOperandRole::ROLE_0X10);
    let definition = project_delete_face(
        &scope,
        std::slice::from_ref(&selected),
        &[],
        &mut Vec::new(),
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::DeleteFace {
            faces: FaceSelection::Native(ref native), ..
        }) if native == &selected.id
    ));

    for (frame_length, known) in [(259, true), (260, false)] {
        let mut longer = scope.clone();
        longer
            .try_edit(|draft| {
                draft.frame_length = frame_length;
                draft.paired_byte_offset = draft.byte_offset + frame_length;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let mut losses = Vec::new();
        let projected =
            project_delete_face(&longer, std::slice::from_ref(&selected), &[], &mut losses)
                .unwrap();
        assert!(matches!(
            projected,
            Some(FeatureDefinition::Operation(FeatureOperation::DeleteFace {
                faces: FaceSelection::Native(ref native), heal: true,
            })) if native == &selected.id
        ));
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss.code
                    == crate::loss::F3dLossCode::DeleteFaceFrameControlsAssumed.kind())
                .count(),
            usize::from(!known)
        );
    }

    for (frame_length, admitted) in [(254, true), (255, true)] {
        let mut legacy = scope.clone();
        legacy.class_tag =
            crate::records::references::DesignClassTag::try_from("399".to_owned()).unwrap();
        legacy.paired_class_tag =
            crate::records::references::DesignClassTag::try_from("257".to_owned()).unwrap();
        legacy
            .try_edit(|draft| {
                draft.frame_length = frame_length;
                draft.kind_offset = draft.byte_offset + 157;
                draft.reference_count_offset = draft.kind_offset - 34;
                draft.paired_byte_offset = draft.byte_offset + frame_length;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        assert_eq!(
            project_delete_face(
                &legacy,
                std::slice::from_ref(&selected),
                &[],
                &mut Vec::new()
            )
            .unwrap()
            .is_some(),
            admitted
        );
    }
    let mut extended = scope.clone();
    extended
        .try_edit(|draft| {
            draft.reference_count_offset = draft.byte_offset + 131;
            draft.frame_length = 262;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(project_delete_face(
        &extended,
        std::slice::from_ref(&selected),
        &[],
        &mut Vec::new()
    )
    .unwrap()
    .is_some());
}

#[test]
fn delete_face_with_typed_references_projects() {
    let scope = |count_offset: u64| {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#77",
            crate::records::feature::scope::DesignFeatureKind::DeleteFace,
            77,
        );
        scope
            .try_edit(|draft| {
                let start = draft.byte_offset;
                let kind = count_offset + 12 + 2 * 51;
                draft.kind_offset = start + kind;
                draft.frame_length = kind + 20 + 196;
                draft.feature_ordinal_offset = start + kind + 20;
                draft.paired_byte_offset = start + draft.frame_length;
                draft.reference_count_offset = start + count_offset;
                draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                    vec![100, 200],
                    vec![start + count_offset + 5, start + count_offset + 56],
                    "reference_members",
                )
                .unwrap();
            })
            .unwrap();
        scope
    };
    let selected = group(77, 0, 100, vec![200], DesignOperandRole::ROLE_0X10);
    for (count_offset, known) in [
        (163, true),
        (167, true),
        (159, false),
        (161, false),
        (165, false),
    ] {
        let mut losses = Vec::new();
        let projected = project_delete_face(
            &scope(count_offset),
            std::slice::from_ref(&selected),
            &[],
            &mut losses,
        )
        .unwrap();
        assert!(matches!(
            projected,
            Some(FeatureDefinition::Operation(FeatureOperation::DeleteFace {
                heal: true,
                ..
            }))
        ));
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss.code
                    == crate::loss::F3dLossCode::DeleteFaceFrameControlsAssumed.kind())
                .count(),
            usize::from(!known),
            "count offset {count_offset}"
        );
    }
}

#[test]
fn surface_delete_face_layouts_project_with_other_class_tags() {
    let selected = group(77, 0, 100, vec![200], DesignOperandRole::ROLE_0X10);
    for (base_frame_length, base_kind_offset) in
        [(246, 135), (247, 135), (250, 139), (257, 146), (258, 146)]
    {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#77",
            crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace,
            77,
        );
        scope.class_tag = "332".to_owned().try_into().unwrap();
        scope.paired_class_tag = "258".to_owned().try_into().unwrap();
        scope
            .try_edit(|draft| {
                draft.frame_length = base_frame_length + 22;
                draft.kind_offset = draft.byte_offset + base_kind_offset + 22;
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(vec![100, 200]);
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.reference_count_offset = draft.kind_offset - 34;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        let definition = project_delete_face(
            &scope,
            std::slice::from_ref(&selected),
            &[],
            &mut Vec::new(),
        )
        .unwrap()
        .unwrap();
        assert!(matches!(
            definition,
            FeatureDefinition::Operation(FeatureOperation::DeleteFace {
                faces: FaceSelection::Native(ref native), heal: false, ..
            }) if native == &selected.id
        ));
    }
}

#[test]
fn surface_delete_face_reports_unfamiliar_count_position_kind_position_and_tail() {
    let selected = group(77, 0, 100, vec![200], DesignOperandRole::ROLE_0X10);
    for (count_offset, tail_length, known) in [
        (123, 77, true),
        (127, 77, true),
        (131, 77, true),
        (134, 77, true),
        (138, 77, true),
        (142, 77, true),
        (123, 76, true),
        (123, 75, false),
        (128, 77, true),
    ] {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#77",
            crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace,
            77,
        );
        scope
            .try_edit(|draft| {
                draft.reference_count_offset = draft.byte_offset + count_offset;
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(vec![100, 200]);
                draft.frame_length = count_offset + 12 + 22 + 34 + tail_length;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        let mut losses = Vec::new();
        assert!(
            project_delete_face(&scope, std::slice::from_ref(&selected), &[], &mut losses)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss.code
                    == crate::loss::F3dLossCode::DeleteFaceFrameControlsAssumed.kind())
                .count(),
            usize::from(!known)
        );
        if count_offset == 123 && tail_length == 77 {
            scope.set_fixture_kind_offset(scope.kind_offset() + 9);
            losses.clear();
            assert!(
                project_delete_face(&scope, std::slice::from_ref(&selected), &[], &mut losses)
                    .unwrap()
                    .is_some()
            );
            assert!(losses
                .iter()
                .any(|loss| loss.code
                    == crate::loss::F3dLossCode::DeleteFaceFrameControlsAssumed.kind()));
        }
    }
}

fn split_face_fixture(
    tool_role: DesignOperandRole,
) -> (DesignParameterScope, [DesignConstructionOperandGroup; 2]) {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::scope::DesignFeatureKind::SplitFace,
        77,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 1000;
            draft.frame_length = 319;
            draft.paired_byte_offset = 1319;
            draft.reference_count_offset = 1168;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 101, 102, 103]);
            draft.layout_fixture_references();
            draft.feature_ordinal_offset = 1242;
        })
        .unwrap();
    let groups = [
        group(77, 0, 100, vec![101], tool_role),
        group(77, 2, 102, vec![103], DesignOperandRole::ROLE_0X10),
    ];
    (scope, groups)
}

fn set_split_face_operation(
    scope: &mut DesignParameterScope,
    operation: crate::records::feature::split_face::DesignSplitFaceOperation,
) {
    assert!(operation.scope_offsets_valid(scope.byte_offset(), scope.reference_count_offset()));
    scope
        .try_edit(|draft| {
            let crate::records::feature::scope::DesignScopePayload::SplitFace(payload) =
                &mut draft.payload
            else {
                panic!("expected a SplitFace scope payload");
            };
            *payload = Some(operation);
        })
        .unwrap();
    crate::test_support::split_face::assert_native_scope(scope);
}

fn split_face_native_operation(
    method: crate::records::feature::split_face::DesignSplitFaceMethod,
    mut direction_reference: crate::records::feature::split_face::DesignSplitFaceDirectionReference,
    direction: Option<crate::records::identity::Located<[cadmpeg_ir::scalar::FiniteReal; 3]>>,
    inputs: Vec<crate::records::identity::Located<u32>>,
) -> crate::records::feature::split_face::DesignSplitFaceOperation {
    use crate::records::feature::split_face::DesignSplitFaceMethod;

    let control = match method {
        DesignSplitFaceMethod::SurfaceIntersection { extend } => Some(
            crate::records::feature::split_face::DesignSplitFaceMethodControl {
                family: 1,
                projection: 0,
                extension: u8::from(extend),
            },
        ),
        DesignSplitFaceMethod::ClosestPoint => Some(
            crate::records::feature::split_face::DesignSplitFaceMethodControl {
                family: 1,
                projection: 1,
                extension: 0,
            },
        ),
        DesignSplitFaceMethod::AlongVector => Some(
            crate::records::feature::split_face::DesignSplitFaceMethodControl {
                family: 0,
                projection: 1,
                extension: 0,
            },
        ),
        DesignSplitFaceMethod::Unresolved => None,
    };
    if let crate::records::feature::split_face::DesignSplitFaceDirectionReference::Local {
        target,
        ..
    }
    | crate::records::feature::split_face::DesignSplitFaceDirectionReference::CrossSegment {
        target,
        ..
    }
    | crate::records::feature::split_face::DesignSplitFaceDirectionReference::CrossDocument {
        target,
        ..
    } = &mut direction_reference
    {
        target.offset += 1000;
    }
    crate::records::feature::split_face::DesignSplitFaceOperation {
        method_offset: if matches!(
            direction_reference,
            crate::records::feature::split_face::DesignSplitFaceDirectionReference::Null
        ) {
            1022
        } else {
            1032
        },
        control,
        direction_reference,
        carrier_frame: direction.as_ref().map(|_| {
            crate::records::feature::split_face::DesignSplitFaceCarrierFrame {
                primary_offset: 120,
                secondary_offset: 228,
                end_offset: 239,
                version: 1,
            }
        }),
        input_count: direction
            .as_ref()
            .map(|_| crate::records::identity::Located {
                value: u32::try_from(inputs.len()).unwrap(),
                offset: 213,
            }),
        inputs_complete: direction.is_some(),
        direction: direction.map(|value| crate::records::identity::Located {
            value: Some(value.value),
            offset: value.offset,
        }),
        inputs: inputs
            .into_iter()
            .map(|input| crate::records::identity::Located {
                offset: input.offset.saturating_sub(1),
                value:
                    crate::records::feature::split_face::DesignSplitFaceDirectionReference::Local {
                        target: crate::records::identity::Located {
                            value: u64::from(input.value),
                            offset: input.offset,
                        },
                        inline_type_guid: None,
                    },
            })
            .collect(),
    }
}

#[test]
fn role_nine_tool_group_projects_as_a_face_tool() {
    use cadmpeg_ir::features::direct_edit::SplitFaceTool;
    use cadmpeg_ir::features::selections::FaceSelection;

    let (scope, groups) = split_face_fixture(DesignOperandRole::ROLE_0X9);
    let projected = project_split_face(
        &scope,
        std::slice::from_ref(&scope),
        &groups,
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        projected.definition,
        FeatureDefinition::Operation(FeatureOperation::SplitFace {
            targets: FaceSelection::Native(_),
            tool: SplitFaceTool::Faces(FaceSelection::Native(_)),
            method: cadmpeg_ir::features::surfaces::SplitFaceMethod::Unresolved {},
        })
    ));
    assert!(projected.losses.iter().any(|loss| {
        loss.code == crate::loss::F3dLossCode::FeatureSplitFaceMethodUnresolved.kind()
    }));
}

#[test]
fn supported_split_face_methods_project_as_decoded() {
    use crate::records::feature::split_face::{
        DesignSplitFaceDirectionReference, DesignSplitFaceMethod,
    };
    use cadmpeg_ir::features::surfaces::SplitFaceMethod;

    for (native, expected) in [
        (
            DesignSplitFaceMethod::SurfaceIntersection { extend: false },
            SplitFaceMethod::SurfaceIntersection { extend: false },
        ),
        (
            DesignSplitFaceMethod::SurfaceIntersection { extend: true },
            SplitFaceMethod::SurfaceIntersection { extend: true },
        ),
        (
            DesignSplitFaceMethod::ClosestPoint,
            SplitFaceMethod::ClosestPoint {},
        ),
    ] {
        let (mut scope, groups) = split_face_fixture(DesignOperandRole::ROLE_0X9);
        set_split_face_operation(
            &mut scope,
            split_face_native_operation(
                native,
                DesignSplitFaceDirectionReference::Null,
                None,
                Vec::new(),
            ),
        );
        let projected = project_split_face(
            &scope,
            std::slice::from_ref(&scope),
            &groups,
            &[],
            &[],
            &[],
            &[],
        );
        let FeatureDefinition::Operation(FeatureOperation::SplitFace { method, .. }) =
            projected.definition
        else {
            panic!("expected a SplitFace operation");
        };
        assert_eq!(method, expected);
    }
}

#[test]
fn along_vector_keeps_the_solved_vector_and_unresolved_source_identity() {
    use crate::records::feature::split_face::{
        DesignSplitFaceDirectionReference, DesignSplitFaceMethod,
    };
    use crate::records::identity::{Located, ReferenceRun};
    use cadmpeg_ir::features::FeatureOperation;
    use cadmpeg_ir::math::Vector3;

    let (mut scope, groups) = split_face_fixture(DesignOperandRole::ROLE_0X9);
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![100, 101, 102, 103, 700, 701]);
            draft.layout_fixture_references();
            draft.frame_length = 341;
            draft.paired_byte_offset = 1341;
            draft.feature_ordinal_offset = 1264;
        })
        .unwrap();
    let finite = |value| cadmpeg_ir::scalar::FiniteReal::new(value).unwrap();
    set_split_face_operation(
        &mut scope,
        split_face_native_operation(
            DesignSplitFaceMethod::AlongVector,
            DesignSplitFaceDirectionReference::Local {
                target: Located {
                    value: 700,
                    offset: 22,
                },
                inline_type_guid: None,
            },
            Some(Located {
                value: [finite(2.5), finite(0.0), finite(0.0)],
                offset: 169,
            }),
            vec![Located {
                value: 701,
                offset: 218,
            }],
        ),
    );
    let projected = project_split_face(
        &scope,
        std::slice::from_ref(&scope),
        &groups,
        &[],
        &[],
        &[],
        &[],
    );
    let FeatureDefinition::Operation(FeatureOperation::SplitFace {
        method:
            SplitFaceMethod::AlongVector {
                direction,
                native_carrier,
                native_inputs,
                native_input_count,
                native_inputs_complete,
            },
        ..
    }) = projected.definition
    else {
        panic!("expected an AlongVector SplitFace operation");
    };
    assert_eq!(direction.unwrap().get(), Vector3::new(2.5, 0.0, 0.0));
    assert!(native_carrier.is_some_and(|value| value.ends_with("record#700")));
    assert_eq!(native_inputs.len(), 1);
    assert!(native_inputs[0].as_ref().unwrap().ends_with("record#701"));
    assert_eq!(native_input_count, Some(1));
    assert!(native_inputs_complete);
    assert!(projected.losses.iter().any(|loss| {
        loss.code == crate::loss::F3dLossCode::FeatureSplitFaceDirectionUnresolved.kind()
    }));
}

#[test]
fn split_face_conflicting_carrier_qualifier_charges_assumption_and_keeps_vector() {
    use crate::records::feature::split_face::{
        DesignSplitFaceDirectionReference, DesignSplitFaceMethod,
    };
    use crate::records::identity::Located;
    use cadmpeg_ir::features::surfaces::SplitFaceMethod;
    use cadmpeg_ir::features::FeatureOperation;
    for (guid, assumed) in [
        ("F2A7590D-6654-4674-B393-A2AEF4FEC48A", false),
        ("00000000-0000-0000-0000-000000000099", true),
    ] {
        let (mut scope, groups) = split_face_fixture(DesignOperandRole::ROLE_0X9);
        let finite = |value| cadmpeg_ir::scalar::FiniteReal::new(value).unwrap();
        let mut operation = split_face_native_operation(
            DesignSplitFaceMethod::AlongVector,
            DesignSplitFaceDirectionReference::Local {
                target: Located {
                    value: 700,
                    offset: 22,
                },
                inline_type_guid: Some(guid.into()),
            },
            Some(Located {
                value: [finite(2.5), finite(0.0), finite(0.0)],
                offset: 169,
            }),
            vec![],
        );
        operation.method_offset = 1072;
        operation.input_count = Some(Located {
            value: 0,
            offset: 213,
        });
        operation.inputs_complete = false;
        set_split_face_operation(&mut scope, operation);
        let projected = project_split_face(
            &scope,
            std::slice::from_ref(&scope),
            &groups,
            &[],
            &[],
            &[],
            &[],
        );
        assert_eq!(
            projected.losses.iter().any(|loss| loss.code
                == crate::loss::F3dLossCode::FeatureSplitFaceCarrierTypeAssumed.kind()),
            assumed
        );
        let FeatureDefinition::Operation(FeatureOperation::SplitFace {
            method:
                SplitFaceMethod::AlongVector {
                    direction,
                    native_carrier,
                    ..
                },
            ..
        }) = projected.definition
        else {
            panic!("AlongVector")
        };
        assert_eq!(
            direction.unwrap().get(),
            cadmpeg_ir::math::Vector3::new(2.5, 0.0, 0.0)
        );
        assert!(native_carrier.unwrap().ends_with(guid));
    }
}

#[test]
fn missing_direction_definition_keeps_the_reference_in_the_default_native_stream() {
    use crate::records::feature::split_face::{
        DesignSplitFaceDirectionReference, DesignSplitFaceMethod,
    };
    let (mut scope, groups) = split_face_fixture(DesignOperandRole::ROLE_0X9);
    scope
        .try_edit(|draft| draft.id = "f3d:design:scope#77".into())
        .unwrap();
    set_split_face_operation(
        &mut scope,
        split_face_native_operation(
            DesignSplitFaceMethod::AlongVector,
            DesignSplitFaceDirectionReference::Local {
                target: crate::records::identity::Located {
                    value: 700,
                    offset: 22,
                },
                inline_type_guid: None,
            },
            None,
            Vec::new(),
        ),
    );
    let projected = project_split_face(
        &scope,
        std::slice::from_ref(&scope),
        &groups,
        &[],
        &[],
        &[],
        &[],
    );
    let FeatureDefinition::Operation(FeatureOperation::SplitFace {
        method:
            cadmpeg_ir::features::surfaces::SplitFaceMethod::AlongVector {
                direction,
                native_carrier,
                ..
            },
        ..
    }) = projected.definition
    else {
        panic!("SplitFace along vector")
    };
    assert!(direction.is_none());
    assert_eq!(
        native_carrier.as_deref(),
        Some("f3d:design:design-record#700")
    );
}

#[test]
fn missing_split_face_definition_stays_typed_with_named_losses() {
    use cadmpeg_ir::features::direct_edit::SplitFaceTool;
    use cadmpeg_ir::features::selections::FaceSelection;
    use cadmpeg_ir::features::FeatureOperation;

    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::scope::DesignFeatureKind::SplitFace,
        77,
    );
    let projected = project_split_face(&scope, &[scope.clone()], &[], &[], &[], &[], &[]);
    assert!(matches!(
        projected.definition,
        FeatureDefinition::Operation(FeatureOperation::SplitFace {
            targets: FaceSelection::Native(_),
            tool: SplitFaceTool::Native(_),
            method: SplitFaceMethod::Unresolved {},
        })
    ));
    assert!(projected.losses.iter().any(|loss| {
        loss.code == crate::loss::F3dLossCode::FeatureSplitFaceGroupsUnresolved.kind()
    }));
}

fn work_plane(record_index: u32) -> DesignParameterScope {
    let mut plane = DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:scope#{record_index}"),
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        record_index,
    );
    plane.with_work_plane_transform(
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap(),
    );
    plane
}

fn compact_plane_selection(
    record_index: u32,
    group_member_ordinal: u32,
    primary_identity: u64,
) -> crate::records::topology::entity_selection::DesignEntitySelectionOperand {
    crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
        crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
            id: format!("f3d:Design/BulkStream.dat:entity-selection#{record_index}"),
            scope_record_index: 77,
            group_record_index: 100,
            group_member_ordinal,
            record_index,
            byte_offset: 0,
            class_tag: "372".to_owned().try_into().unwrap(),
            asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
                .to_owned()
                .try_into()
                .unwrap(),
            asset_id_offset: 0,
            context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
                .to_owned()
                .try_into()
                .unwrap(),
            context_id_offset: 0,
            serialization_form:
                crate::records::topology::entity_selection::SelectionSerializationForm::Zero,
            identity_record_index: record_index + 3,
            identity_record_offset: 0,
            primary_identity,
            primary_identity_offset: 21,
            secondary: None,
            historical_edge_candidates: Vec::new(),
            historical_face_candidates: Vec::new(),
            resolved_edge_slot: None,
            next_record_index: record_index + 4,
            next_byte_offset: 29,
        },
    )
    .unwrap()
}

fn selected_plane_fixture() -> (
    DesignParameterScope,
    DesignConstructionOperandGroup,
    crate::records::topology::entity_selection::DesignEntitySelectionOperand,
    DesignParameterScope,
) {
    let (scope, groups) = split_face_fixture(DesignOperandRole::ROLE_0X21);
    (
        scope,
        groups[0].clone(),
        compact_plane_selection(101, 0, 600),
        work_plane(601),
    )
}

#[test]
fn selected_work_planes_select_target_plane() {
    let (scope, group, selection, plane) = selected_plane_fixture();
    let selected = selected_work_planes(
        &scope,
        &group,
        std::slice::from_ref(&selection),
        std::slice::from_ref(&plane),
    )
    .unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].record_index, plane.record_index);
}

fn split_body_tool(
    definition: &FeatureDefinition,
) -> &cadmpeg_ir::features::direct_edit::SplitBodyTool {
    let FeatureDefinition::Operation(FeatureOperation::SplitBody { tool, .. }) = definition else {
        panic!("expected a SplitBody operation");
    };
    tool
}

#[test]
fn split_body_projects_tool_extension_independently_of_native_tool_selection() {
    let mut scope = split_body_scope();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X21);
    for extend_tool in [None, Some(false), Some(true)] {
        let crate::records::feature::scope::DesignScopePayloadMut::Split(split) =
            scope.payload_mut()
        else {
            panic!("Split scope payload");
        };
        split.extend_tool =
            extend_tool.map(|value| crate::records::identity::Located { value, offset: 25 });
        let definition = project_split(&scope, &[], &groups, &[], &[], &[], &mut Vec::new())
            .unwrap()
            .unwrap();
        assert_eq!(
            split_body_tool(&definition),
            &cadmpeg_ir::features::direct_edit::SplitBodyTool::Faces(FaceSelection::Native(
                groups[0].id.clone()
            )),
        );
        let wire = json::to_value(&definition).unwrap();
        let operation = &wire["value"];
        assert_eq!(
            operation["value"].get("extend_tool"),
            extend_tool.map(serde_json::Value::Bool).as_ref(),
        );
        assert_eq!(
            json::from_value::<FeatureDefinition>(wire).unwrap(),
            definition
        );
    }
}

#[test]
fn split_body_work_plane_tool_selects_the_plane_feature() {
    let scope = split_body_scope();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X21);
    let plane = work_plane(601);
    let definition = project_split(
        &scope,
        std::slice::from_ref(&plane),
        &groups,
        &[compact_plane_selection(101, 0, 600)],
        &[],
        &[],
        &mut Vec::new(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        split_body_tool(&definition),
        &cadmpeg_ir::features::direct_edit::SplitBodyTool::Plane {
            plane: crate::ids::neutral_feature_id(&plane),
        }
    );
}

#[test]
fn split_body_tool_without_exactly_one_work_plane_keeps_the_group() {
    let native = |group: &DesignConstructionOperandGroup| {
        cadmpeg_ir::features::direct_edit::SplitBodyTool::Faces(FaceSelection::Native(
            group.id.clone(),
        ))
    };
    let scope = split_body_scope();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X21);
    let planes = [work_plane(601), work_plane(701)];

    // The identity names the record before a scope that is not a WorkPlane.
    let definition = project_split(
        &scope,
        &planes,
        &groups,
        &[compact_plane_selection(101, 0, 601)],
        &[],
        &[],
        &mut Vec::new(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(split_body_tool(&definition), &native(&groups[0]));

    // Two WorkPlane members have no single splitting plane.
    let scope = split_body_scope_referencing(vec![100, 101, 102, 103, 104]);
    let groups = [
        group(77, 0, 100, vec![101, 102], DesignOperandRole::ROLE_0X21),
        group(77, 3, 103, vec![104], DesignOperandRole::BODIES_A),
    ];
    let selections = [
        compact_plane_selection(101, 0, 600),
        compact_plane_selection(102, 1, 700),
    ];
    let definition = project_split(
        &scope,
        &planes,
        &groups,
        &selections,
        &[],
        &[],
        &mut Vec::new(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(split_body_tool(&definition), &native(&groups[0]));
}

#[test]
fn a_split_body_plane_tool_is_a_dependency_of_the_split() {
    let mut plane = work_plane(601);
    plane.feature_ordinal = std::num::NonZeroU32::new(1).unwrap();
    let mut split = split_body_scope();
    split.feature_ordinal = std::num::NonZeroU32::new(2).unwrap();
    let scopes = [plane, split];
    let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);
    let groups = split_body_groups(DesignOperandRole::ROLE_0X21);
    let selections = [compact_plane_selection(101, 0, 600)];
    let projected = crate::test_support::with_decode_context(|ctx| {
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes: &scopes,
                timelines: &timelines,
                construction_groups: &groups,
                entity_selection_operands: &selections,
                ..Default::default()
            },
        )
    })
    .expect("exact synthetic feature timeline");
    let plane_feature = crate::ids::neutral_feature_id(&scopes[0]);
    let split_feature = projected
        .features
        .iter()
        .find(|feature| feature.id == crate::ids::neutral_feature_id(&scopes[1]))
        .expect("projected Split");
    assert_eq!(
        split_body_tool(split_feature.evaluation.definition()),
        &cadmpeg_ir::features::direct_edit::SplitBodyTool::Plane {
            plane: plane_feature.clone(),
        }
    );
    assert_eq!(split_feature.dependencies.as_slice(), [plane_feature]);
}

#[test]
fn direct_single_identity_split_face_member_projects_historical_edge_path() {
    let scope_record_index = 77;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::scope::DesignFeatureKind::SplitFace,
        scope_record_index,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 407;
            draft.reference_count_offset = draft.byte_offset + 168;
            draft.previous_history_state_id = Some(7);
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated((100..112).collect());
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();

    let groups = [
        group(
            scope_record_index,
            0,
            100,
            vec![101],
            DesignOperandRole::ROLE_0X21,
        ),
        group(
            scope_record_index,
            2,
            102,
            (103..112).collect(),
            DesignOperandRole::ROLE_0X10,
        ),
    ];
    let selections = [
        crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
            crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                id: "f3d:Design/BulkStream.dat:entity-selection#101".into(),
                scope_record_index,
                group_record_index: 100,
                group_member_ordinal: 0,
                record_index: 101,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                    .unwrap(),
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 0,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
                )
                .unwrap(),
                context_id_offset: 0,
                serialization_form:
                    crate::records::topology::entity_selection::SelectionSerializationForm::Zero,
                identity_record_index: 104,
                identity_record_offset: 0,
                primary_identity: 225,
                primary_identity_offset: 21,
                secondary: None,
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: Vec::new(),
                resolved_edge_slot: Some(42),
                next_record_index: 103,
                next_byte_offset: 29,
            },
        )
        .unwrap(),
    ];

    let definition = project_split_face(
        &scope,
        &[scope.clone()],
        &groups,
        &selections,
        &[],
        &[],
        &[],
    )
    .definition;
    let FeatureDefinition::Operation(FeatureOperation::SplitFace {
        tool:
            cadmpeg_ir::features::direct_edit::SplitFaceTool::Path(
                cadmpeg_ir::features::profiles::PathRef::HistoricalEdges {
                    state,
                    edges,
                    native,
                },
            ),
        ..
    }) = definition
    else {
        panic!("expected historical edge path");
    };
    let feature = crate::ids::neutral_feature_id(&scope);
    let prefix = crate::ids::history_input_prefix(&feature.key(), 7);
    assert_eq!(state, feature_input_topology_id(&feature, 7),);
    assert_eq!(
        edges.as_slice(),
        vec![crate::ids::history_input_edge_id(&prefix, 42)],
    );
    assert_eq!(native.as_str(), groups[0].id);
}

#[test]
fn along_vector_preserves_extreme_values_and_input_slots() {
    use crate::records::feature::split_face::{
        DesignSplitFaceDirectionReference as Reference, DesignSplitFaceMethod,
    };
    use crate::records::identity::Located;
    use cadmpeg_ir::features::surfaces::SplitFaceMethod;
    let finite = |value| cadmpeg_ir::scalar::FiniteReal::new(value).unwrap();
    for value in [1.0e200, 1.0e-200] {
        let (mut scope, groups) = split_face_fixture(DesignOperandRole::ROLE_0X9);
        let mut operation = split_face_native_operation(
            DesignSplitFaceMethod::AlongVector,
            Reference::Local {
                target: Located {
                    value: 700,
                    offset: 22,
                },
                inline_type_guid: None,
            },
            Some(Located {
                value: [finite(value), finite(0.0), finite(0.0)],
                offset: 169,
            }),
            vec![Located {
                value: 701,
                offset: 219,
            }],
        );
        operation.carrier_frame.as_mut().unwrap().secondary_offset = 230;
        operation.carrier_frame.as_mut().unwrap().end_offset = 241;
        operation.input_count.as_mut().unwrap().value = 3;
        operation.inputs.insert(
            0,
            Located {
                value: Reference::Null,
                offset: 217,
            },
        );
        operation.inputs.push(Located {
            value: Reference::Null,
            offset: 229,
        });
        set_split_face_operation(&mut scope, operation);
        let projected = project_split_face(
            &scope,
            std::slice::from_ref(&scope),
            &groups,
            &[],
            &[],
            &[],
            &[],
        );
        let FeatureDefinition::Operation(FeatureOperation::SplitFace {
            method:
                SplitFaceMethod::AlongVector {
                    direction,
                    native_inputs,
                    native_input_count,
                    native_inputs_complete,
                    ..
                },
            ..
        }) = projected.definition
        else {
            panic!("AlongVector")
        };
        assert_eq!(direction.unwrap().get().x, value);
        assert_eq!(native_input_count, Some(3));
        assert!(native_inputs_complete);
        assert_eq!(native_inputs.len(), 3);
        assert!(native_inputs[0].is_none());
        assert!(native_inputs[1].as_ref().unwrap().ends_with("record#701"));
        assert!(native_inputs[2].is_none());
    }
}

#[test]
fn along_vector_distinguishes_partial_and_complete_input_runs() {
    use crate::records::feature::split_face::{
        DesignSplitFaceDirectionReference as Reference, DesignSplitFaceMethod,
    };
    use crate::records::identity::Located;
    use cadmpeg_ir::features::surfaces::SplitFaceMethod;
    let finite = |value| cadmpeg_ir::scalar::FiniteReal::new(value).unwrap();
    for (count, complete) in [(1, true), (2, false)] {
        let (mut scope, groups) = split_face_fixture(DesignOperandRole::ROLE_0X9);
        let mut operation = split_face_native_operation(
            DesignSplitFaceMethod::AlongVector,
            Reference::Local {
                target: Located {
                    value: 700,
                    offset: 22,
                },
                inline_type_guid: None,
            },
            Some(Located {
                value: [finite(1.0), finite(0.0), finite(0.0)],
                offset: 169,
            }),
            vec![Located {
                value: 701,
                offset: 218,
            }],
        );
        operation.input_count.as_mut().unwrap().value = count;
        operation.inputs_complete = complete;
        set_split_face_operation(&mut scope, operation);
        let projected = project_split_face(
            &scope,
            std::slice::from_ref(&scope),
            &groups,
            &[],
            &[],
            &[],
            &[],
        );
        let FeatureDefinition::Operation(FeatureOperation::SplitFace {
            method:
                SplitFaceMethod::AlongVector {
                    native_inputs,
                    native_input_count,
                    native_inputs_complete,
                    ..
                },
            ..
        }) = projected.definition
        else {
            panic!("AlongVector")
        };
        assert_eq!(native_inputs.len(), 1);
        assert_eq!(native_input_count, Some(count));
        assert_eq!(native_inputs_complete, complete);
    }
}

#[test]
fn split_face_projects_inputs_when_the_direction_value_is_unavailable() {
    use crate::records::feature::split_face::{
        DesignSplitFaceDirectionReference, DesignSplitFaceMethod,
    };
    use crate::records::identity::Located;
    use cadmpeg_ir::features::surfaces::SplitFaceMethod;
    let (mut scope, groups) = split_face_fixture(DesignOperandRole::ROLE_0X9);
    let finite = |value| cadmpeg_ir::scalar::FiniteReal::new(value).unwrap();
    let mut operation = split_face_native_operation(
        DesignSplitFaceMethod::AlongVector,
        DesignSplitFaceDirectionReference::Local {
            target: Located {
                value: 700,
                offset: 22,
            },
            inline_type_guid: None,
        },
        Some(Located {
            value: [finite(2.5), finite(0.0), finite(0.0)],
            offset: 169,
        }),
        vec![Located {
            value: 88,
            offset: 218,
        }],
    );
    operation.direction.as_mut().unwrap().value = None;
    set_split_face_operation(&mut scope, operation);
    let projected = project_split_face(
        &scope,
        std::slice::from_ref(&scope),
        &groups,
        &[],
        &[],
        &[],
        &[],
    );
    let FeatureDefinition::Operation(FeatureOperation::SplitFace {
        method:
            SplitFaceMethod::AlongVector {
                direction,
                native_inputs,
                native_input_count,
                native_inputs_complete,
                ..
            },
        ..
    }) = projected.definition
    else {
        panic!("AlongVector")
    };
    assert_eq!(direction, None);
    assert_eq!(native_input_count, Some(1));
    assert!(native_inputs_complete);
    assert_eq!(native_inputs.len(), 1);
    assert!(native_inputs[0]
        .as_ref()
        .unwrap()
        .ends_with("design-record#88"));
}

fn split_body_tool_body(
    resolved: bool,
) -> crate::records::topology::body_recipe::DesignBodyRecipeOperand {
    use crate::records::topology::body_recipe::{
        DesignBodyRecipeOperand, DesignBodyRecipeOperandDraft, DesignBodyRecipeReference,
        DesignOperandOwner,
    };
    DesignBodyRecipeOperand::try_new(DesignBodyRecipeOperandDraft {
        id: "f3d:Design/BulkStream.dat:body-recipe#101".into(),
        scope_record_index: 77,
        owner: DesignOperandOwner::Group {
            group_record_index: 100,
            group_member_ordinal: 0,
        },
        record_index: 101,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned()).unwrap(),
        asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
        )
        .unwrap(),
        asset_id_offset: 56,
        context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
            "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
        )
        .unwrap(),
        context_id_offset: 132,
        selector_tail: None,
        references: vec![DesignBodyRecipeReference {
            design_reference: 326,
            design_reference_offset: 25,
            form: 33,
            form_offset: 33,
            candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 104,
        nested_record_index_offset: 38,
        recipe_id: "f3d:Design/BulkStream.dat:recipe#102".into(),
        resolved_face_slot: None,
        resolved_body_state_id: resolved.then_some(7),
        resolved_body_slot: resolved.then_some(3),
        resolved_body_face_slots: if resolved { vec![20, 21] } else { Vec::new() },
        next_record_index: 105,
        next_byte_offset: 256,
    })
    .unwrap()
}

#[test]
fn split_body_tool_body_selects_the_resolved_body_faces() {
    let scope = split_body_scope();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X21);
    let definition = project_split(
        &scope,
        &[],
        &groups,
        &[],
        &[],
        &[split_body_tool_body(true)],
        &mut Vec::new(),
    )
    .unwrap()
    .unwrap();
    let cadmpeg_ir::features::direct_edit::SplitBodyTool::Faces(FaceSelection::Historical {
        state,
        faces,
        native,
    }) = split_body_tool(&definition)
    else {
        panic!("expected historical tool faces");
    };
    assert!(state.as_str().ends_with(":7"));
    let faces: Vec<&str> = faces
        .iter()
        .map(cadmpeg_ir::ids::HistoricalFaceId::as_str)
        .collect();
    assert!(faces.len() == 2 && faces[0].ends_with(":7:20") && faces[1].ends_with(":7:21"));
    assert_eq!(native.as_str(), groups[0].id);

    let scope = split_body_scope_referencing(vec![100, 101, 102, 103, 104]);
    let mixed = [
        group(77, 0, 100, vec![101, 102], DesignOperandRole::ROLE_0X21),
        group(77, 3, 103, vec![104], DesignOperandRole::BODIES_A),
    ];
    let definition = project_split(
        &scope,
        &[],
        &mixed,
        &[],
        &[],
        &[split_body_tool_body(true)],
        &mut Vec::new(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        split_body_tool(&definition),
        &cadmpeg_ir::features::direct_edit::SplitBodyTool::Faces(FaceSelection::Native(
            mixed[0].id.clone()
        ))
    );
    let scope = split_body_scope();

    let definition = project_split(
        &scope,
        &[],
        &groups,
        &[],
        &[],
        &[split_body_tool_body(false)],
        &mut Vec::new(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        split_body_tool(&definition),
        &cadmpeg_ir::features::direct_edit::SplitBodyTool::Faces(FaceSelection::Native(
            groups[0].id.clone()
        ))
    );
}
