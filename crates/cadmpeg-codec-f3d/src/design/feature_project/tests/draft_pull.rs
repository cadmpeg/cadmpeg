// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::direct_face::{
    DesignDraftFlags, DesignDraftOperation, DesignDraftTwoSided,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::identity::ReferenceRun;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::entity_selection::{
    DesignEntitySelectionFaceCandidate, DesignEntitySelectionOperand,
    DesignEntitySelectionOperandDraft,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::fillet::HistoricalBinding;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::features::direct_edit::{DraftAnchor, DraftSecondSide};
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::{Point3, Vector3};

const STREAM: &str = "f3d:test/Design/BulkStream.dat";

fn draft_scope(angle: f64, flipped: Option<bool>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#100"),
        DesignFeatureKind::Draft,
        100,
    );
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(7);
            draft.reference_members = ReferenceRun::unlocated(vec![101, 111, 102, 112]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let DesignScopePayloadMut::Draft(slot) = scope.payload_mut() {
        *slot = Some(DesignDraftOperation {
            angle: cadmpeg_ir::scalar::Angle::new(angle).unwrap(),
            angle_record_index: 90,
            angle_offset: 0,
            opposite_angle_record_index: 91,
            opposite_angle_offset: 0,
            opposite_angle: None,
            flags: flipped.map(|flipped| DesignDraftFlags {
                flipped,
                tangent_chain: false,
                two_sided: None,
                ..Default::default()
            }),
        });
    }
    scope
}

fn group(
    record_index: u32,
    member: u32,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    json::from_value(cadmpeg_core::json!({
        "id": format!("{STREAM}:design-construction-operand-group#{record_index}"),
        "scope_record_index": 100,
        "scope_reference_ordinal": 0,
        "record_index": record_index,
        "byte_offset": 1000,
        "class_tag": "332",
        "members": [member],
        "member_offsets": [1026],
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": role.raw(),
        "role_offset": 1054,
        "paired_class_tag": "259",
        "paired_byte_offset": 1125
    }))
    .unwrap()
}

fn groups() -> [DesignConstructionOperandGroup; 2] {
    [
        group(101, 111, DesignOperandRole::ROLE_0X10),
        group(102, 112, DesignOperandRole::ROLE_0X21),
    ]
}

fn selection(primary_identity: u64, face_slots: &[i64]) -> DesignEntitySelectionOperand {
    DesignEntitySelectionOperand::try_new(DesignEntitySelectionOperandDraft {
        id: format!("{STREAM}:design-entity-selection-operand#112"),
        scope_record_index: 100,
        group_record_index: 102,
        group_member_ordinal: 0,
        record_index: 112,
        byte_offset: 0,
        class_tag: DesignClassTag::try_from("000".to_owned()).unwrap(),
        asset_id: DesignRelaxedGuidText::try_from(
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
        )
        .unwrap(),
        asset_id_offset: 0,
        context_id: DesignRelaxedGuidText::try_from(
            "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
        )
        .unwrap(),
        context_id_offset: 0,
        serialization_form:
            crate::records::topology::entity_selection::SelectionSerializationForm::Zero,
        identity_record_index: 115,
        identity_record_offset: 0,
        primary_identity,
        primary_identity_offset: 21,
        secondary: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: face_slots
            .iter()
            .map(|face_slot| DesignEntitySelectionFaceCandidate {
                history_id: "history".into(),
                historical: HistoricalBinding {
                    kind: AsmHistoricalEntityKind::Coedge,
                    entity_ref: i64::try_from(primary_identity).unwrap(),
                    state_ids: vec![7, 6],
                },
                face_slot: *face_slot,
            })
            .collect(),
        resolved_edge_slot: None,
        next_record_index: 114,
        next_byte_offset: 29,
    })
    .unwrap()
}

struct Projected {
    anchor: DraftAnchor,
    outward: Option<bool>,
}

fn naming_space(
    scope: &DesignParameterScope,
    component_record_index: u64,
) -> crate::records::recipes::DesignComponentNamingSpace {
    let stream = crate::ids::native_stream(&scope.id).unwrap();
    crate::records::recipes::DesignComponentNamingSpace {
        id: format!("{stream}:design-component-naming-space#{component_record_index}"),
        byte_offset: component_record_index,
        component_record_index,
        context_uuid: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
            .to_owned()
            .try_into()
            .expect("GUID"),
        context_uuid_offset: component_record_index + 12,
        name: None,
        name_offset: None,
    }
}

fn project(
    scope: &DesignParameterScope,
    selections: &[DesignEntitySelectionOperand],
) -> Option<Projected> {
    let definition = super::super::draft::exact(
        scope,
        &super::super::ProjectInputs {
            scopes: std::slice::from_ref(scope),
            construction_groups: &groups(),
            entity_selection_operands: selections,
            face_operands: &[],
            histories: &[],
            design_types: &crate::test_support::origin_types::origin_design_types(
                "f3d:test/Design/MetaStream.dat",
                39,
            ),
            component_naming_spaces: &[naming_space(scope, 1)],
            ..Default::default()
        },
    )
    .unwrap()?;
    let FeatureDefinition::Operation(FeatureOperation::Draft {
        anchor, outward, ..
    }) = definition
    else {
        panic!("expected a Draft");
    };
    Some(Projected { anchor, outward })
}

/// A flipped axis-aligned pull keeps `+0.0` in its zero components.
fn assert_zero_components_are_unsigned(direction: Vector3) {
    for component in [direction.x, direction.y, direction.z] {
        assert!(
            component != 0.0 || !component.is_sign_negative(),
            "{direction:?}"
        );
    }
}

fn pull_direction(anchor: &DraftAnchor) -> Option<Vector3> {
    let DraftAnchor::NeutralPlane { pull, .. } = anchor else {
        panic!("expected a neutral-plane anchor");
    };
    pull.as_ref().map(|pull| *pull.direction)
}

#[test]
fn an_origin_plane_states_its_normal_as_the_pull_direction() {
    for (identity, normal, u_axis) in [
        (42, Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)),
        (43, Vector3::new(0.0, 1.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
        (44, Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
    ] {
        let projected =
            project(&draft_scope(0.25, Some(false)), &[selection(identity, &[])]).unwrap();
        assert_eq!(pull_direction(&projected.anchor), Some(normal));
        assert_eq!(projected.outward, Some(false));
        let DraftAnchor::NeutralPlane { plane, pull } = &projected.anchor else {
            panic!("expected a neutral-plane anchor");
        };
        assert_eq!(plane, &FaceSelection::Native(groups()[1].id.clone()));
        let pull = pull.as_ref().unwrap();
        assert_eq!(pull.plane, None);
        let frame = pull.frame.expect("origin-plane frame");
        assert_eq!(frame.origin().get(), Point3::new(0.0, 0.0, 0.0));
        assert_eq!(frame.normal().get(), normal);
        assert_eq!(frame.u_axis().get(), u_axis);
    }
}

#[test]
fn an_origin_plane_in_a_stream_of_several_components_keeps_no_frame() {
    let scope = draft_scope(0.25, Some(false));
    let space = |component_record_index| naming_space(&scope, component_record_index);
    for (spaces, framed) in [
        (Vec::new(), false),
        (vec![space(1)], true),
        (vec![space(1), space(2)], false),
    ] {
        let definition = super::super::draft::exact(
            &scope,
            &super::super::ProjectInputs {
                scopes: std::slice::from_ref(&scope),
                construction_groups: &groups(),
                entity_selection_operands: &[selection(42, &[])],
                face_operands: &[],
                histories: &[],
                design_types: &crate::test_support::origin_types::origin_design_types(
                    "f3d:test/Design/MetaStream.dat",
                    39,
                ),
                component_naming_spaces: &spaces,
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Draft { anchor, .. }) = definition
        else {
            panic!("expected a Draft");
        };
        let DraftAnchor::NeutralPlane { pull, .. } = anchor else {
            panic!("expected a neutral-plane anchor");
        };
        let pull = pull.unwrap();
        assert_eq!(pull.direction.get(), Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(pull.frame.is_some(), framed);
    }
}

#[test]
fn a_flip_reverses_a_stated_pull_and_keeps_the_angle_sense() {
    for (identity, flipped_normal) in [
        (42, Vector3::new(0.0, 0.0, -1.0)),
        (43, Vector3::new(0.0, -1.0, 0.0)),
        (44, Vector3::new(-1.0, 0.0, 0.0)),
    ] {
        let projected =
            project(&draft_scope(0.25, Some(true)), &[selection(identity, &[])]).unwrap();
        let pull = pull_direction(&projected.anchor).unwrap();
        assert_eq!(pull, flipped_normal);
        assert_zero_components_are_unsigned(pull);
        assert_eq!(projected.outward, Some(false));
    }

    let negative = project(&draft_scope(-0.25, Some(true)), &[selection(42, &[])]).unwrap();
    assert_eq!(negative.outward, Some(true));
}

#[test]
fn an_origin_identity_is_not_the_asm_entity_with_the_same_number() {
    for face_slots in [&[158][..], &[158, 159][..]] {
        let projected = project(
            &draft_scope(0.25, Some(false)),
            &[selection(42, face_slots)],
        )
        .unwrap();
        assert_eq!(
            pull_direction(&projected.anchor),
            Some(Vector3::new(0.0, 0.0, 1.0))
        );
    }
}

#[test]
fn a_flip_inverts_outward_when_no_pull_is_stated() {
    for (angle, flipped, expected) in [
        (0.25, Some(false), false),
        (0.25, Some(true), true),
        (-0.25, Some(false), true),
        (-0.25, Some(true), false),
        (0.25, None, false),
    ] {
        let projected = project(&draft_scope(angle, flipped), &[]).unwrap();
        assert_eq!(pull_direction(&projected.anchor), None);
        assert_eq!(projected.outward, Some(expected), "{angle} {flipped:?}");
    }
}

#[test]
fn an_identity_outside_the_origin_planes_keeps_the_historical_rule() {
    let mut scope = draft_scope(0.25, Some(false));
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(project(&scope, &[selection(45, &[])]).is_none());
    assert!(project(&scope, &[selection(41, &[])]).is_none());
}

fn work_plane_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#283"),
        DesignFeatureKind::WorkPlane,
        283,
    );
    if let DesignScopePayloadMut::WorkPlane(slot) = scope.payload_mut() {
        *slot = Some(crate::records::feature::scope::DesignWorkPlaneTransform {
            work_plane_transform: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.5],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap(),
            work_plane_transform_offset: 0,
            reference: None,
            work_plane_construction: None,
        });
    }
    scope
}

#[test]
fn a_flip_reverses_the_pull_of_a_work_plane() {
    let plane = work_plane_scope();
    for (flipped, z) in [(false, 1.0), (true, -1.0)] {
        let scope = draft_scope(0.25, Some(flipped));
        let definition = super::super::draft::exact(
            &scope,
            &super::super::ProjectInputs {
                scopes: &[scope.clone(), plane.clone()],
                construction_groups: &groups(),
                entity_selection_operands: &[selection(282, &[])],
                face_operands: &[],
                histories: &[],
                design_types: &[],
                component_naming_spaces: &[],
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Draft {
            anchor, outward, ..
        }) = definition
        else {
            panic!("expected a Draft");
        };
        let pull = pull_direction(&anchor).unwrap();
        assert_eq!(pull, Vector3::new(0.0, 0.0, z));
        assert_zero_components_are_unsigned(pull);
        assert_eq!(outward, Some(false));
    }
}

#[test]
fn a_flip_reverses_the_pull_of_a_parting_line() {
    let plane = work_plane_scope();
    let mut scope = draft_scope(0.25, Some(true));
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![101, 111, 102, 112, 103, 113]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let [faces, neutral] = groups();
    let tool = group(103, 113, DesignOperandRole::ROLE_0X21);
    let definition = super::super::draft::exact(
        &scope,
        &super::super::ProjectInputs {
            scopes: &[scope.clone(), plane],
            construction_groups: &[faces, neutral, tool],
            entity_selection_operands: &[selection(282, &[])],
            face_operands: &[],
            histories: &[],
            design_types: &[],
            component_naming_spaces: &[],
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    let FeatureDefinition::Operation(FeatureOperation::Draft {
        anchor: DraftAnchor::PartingLine { pull, .. },
        outward,
        ..
    }) = definition
    else {
        panic!("expected a parting-line Draft");
    };
    assert_eq!(*pull.direction, Vector3::new(0.0, 0.0, -1.0));
    assert_eq!(outward, Some(false));
}

fn two_sided_scope(
    first: f64,
    second: f64,
    two_sided: DesignDraftTwoSided,
    flipped: bool,
) -> DesignParameterScope {
    let mut scope = draft_scope(first, Some(flipped));
    if let DesignScopePayloadMut::Draft(Some(draft)) = scope.payload_mut() {
        draft.opposite_angle = Some(cadmpeg_ir::scalar::Angle::new(second).unwrap());
        draft.flags = Some(DesignDraftFlags {
            flipped,
            tangent_chain: false,
            two_sided: Some(two_sided),
            ..Default::default()
        });
    }
    scope
}

fn slope(angle: f64) -> cadmpeg_ir::scalar::SlopeAngle {
    cadmpeg_ir::scalar::SlopeAngle::new(angle).unwrap()
}

#[test]
fn the_first_lane_drafts_the_plane_normal_side_whatever_the_flip() {
    let (first, second) = (0.0873, 0.0524);
    let plane = work_plane_scope();
    for (two_sided, flipped, pull_z, angle, second_side) in [
        (
            DesignDraftTwoSided::TwoAngles,
            false,
            1.0,
            first,
            DraftSecondSide::Angle {
                angle: slope(second),
            },
        ),
        (
            DesignDraftTwoSided::TwoAngles,
            true,
            -1.0,
            -second,
            DraftSecondSide::Angle {
                angle: slope(-first),
            },
        ),
        (
            DesignDraftTwoSided::Symmetric,
            false,
            1.0,
            first,
            DraftSecondSide::Symmetric {},
        ),
        (
            DesignDraftTwoSided::Symmetric,
            true,
            -1.0,
            -first,
            DraftSecondSide::Symmetric {},
        ),
    ] {
        let opposite = if two_sided == DesignDraftTwoSided::Symmetric {
            first
        } else {
            second
        };
        let scope = two_sided_scope(first, opposite, two_sided, flipped);
        for (identity, scopes) in [
            (282, vec![scope.clone(), plane.clone()]),
            (42, vec![scope.clone()]),
        ] {
            let definition = super::super::draft::exact(
                &scope,
                &super::super::ProjectInputs {
                    scopes: &scopes,
                    construction_groups: &groups(),
                    entity_selection_operands: &[selection(identity, &[])],
                    face_operands: &[],
                    histories: &[],
                    design_types: &crate::test_support::origin_types::origin_design_types(
                        "f3d:test/Design/MetaStream.dat",
                        39,
                    ),
                    component_naming_spaces: &[],
                    ..Default::default()
                },
            )
            .unwrap()
            .unwrap();
            let FeatureDefinition::Operation(FeatureOperation::Draft {
                anchor,
                angle: projected_angle,
                outward,
                second_side: projected_second_side,
                ..
            }) = definition
            else {
                panic!("expected a Draft");
            };
            assert_eq!(
                pull_direction(&anchor),
                Some(Vector3::new(0.0, 0.0, pull_z))
            );
            assert_eq!(projected_angle, Some(slope(angle)));
            assert_eq!(outward, Some(angle < 0.0));
            assert_eq!(projected_second_side, Some(second_side));
        }
    }
}

#[test]
fn a_two_sided_draft_needs_a_neutral_plane_pull() {
    let one_sided = draft_scope(0.25, Some(false));
    let two_sided = two_sided_scope(0.25, 0.1, DesignDraftTwoSided::TwoAngles, false);
    for selections in [vec![], vec![selection(225, &[158])]] {
        assert!(project(&one_sided, &selections).is_some());
        assert!(project(&two_sided, &selections).is_none());
    }

    let plane = work_plane_scope();
    let [faces, neutral] = groups();
    let tool = group(103, 113, DesignOperandRole::ROLE_0X21);
    for (scope, expected) in [(one_sided, true), (two_sided, false)] {
        let mut scope = scope;
        scope
            .try_edit(|draft| {
                draft.reference_members =
                    ReferenceRun::unlocated(vec![101, 111, 102, 112, 103, 113]);
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let definition = super::super::draft::exact(
            &scope,
            &super::super::ProjectInputs {
                scopes: &[scope.clone(), plane.clone()],
                construction_groups: &[faces.clone(), neutral.clone(), tool.clone()],
                entity_selection_operands: &[selection(282, &[])],
                face_operands: &[],
                histories: &[],
                design_types: &[],
                component_naming_spaces: &[],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(definition.is_some(), expected);
    }
}

fn parting_scope(form: u32) -> DesignParameterScope {
    use crate::records::feature::direct_face::DesignDraftParting;
    use crate::records::identity::Located;
    let mut scope = draft_scope(0.25, Some(false));
    scope
        .try_edit(|draft| {
            draft.reference_members =
                ReferenceRun::unlocated(vec![101, 111, 102, 112, 103, 113, 104, 114]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let DesignScopePayloadMut::Draft(Some(operation)) = scope.payload_mut() {
        operation.opposite_angle = Some(cadmpeg_ir::scalar::Angle::new(0.25).unwrap());
        operation.flags = Some(DesignDraftFlags {
            parting: Some(DesignDraftParting {
                form,
                mode: if form == 2 { 3 } else { 1 },
                prefix_scalars: [
                    cadmpeg_ir::scalar::FiniteReal::new(1.5).unwrap(),
                    cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap(),
                ],
                boundary_flag: false,
                fixed_edges: (form == 3).then_some(Located {
                    value: 104,
                    offset: 74,
                }),
                pull: Located {
                    value: 102,
                    offset: if form == 2 { 75 } else { 85 },
                },
                tool: Located {
                    value: 103,
                    offset: if form == 2 { 86 } else { 96 },
                },
                faces: Located {
                    value: 101,
                    offset: if form == 2 { 101 } else { 111 },
                },
            }),
            ..Default::default()
        });
    }
    scope
}

#[test]
fn legacy_parting_groups_keep_the_work_plane_and_tool_selection() {
    let [faces, plane_group] = groups();
    let tool = group(103, 113, DesignOperandRole::ROLE_0X21);
    let expected_tool = FaceSelection::Native(tool.id.clone());
    let construction_groups = [faces, plane_group, tool];
    let plane = work_plane_scope();
    let expected_plane = crate::ids::neutral_feature_id(&plane);
    for (form, mode, flipped, opposite) in
        [(2, 1, false, 0.0), (2, 2, true, 0.0), (3, 2, true, 0.25)]
    {
        let mut scope = parting_scope(form);
        if let DesignScopePayloadMut::Draft(Some(operation)) = scope.payload_mut() {
            operation.opposite_angle = Some(cadmpeg_ir::scalar::Angle::new(opposite).unwrap());
            let flags = operation.flags.as_mut().unwrap();
            flags.flipped = flipped;
            flags.tangent_chain = true;
            let parting = flags.parting.as_mut().unwrap();
            parting.mode = mode;
            if form == 2 && mode == 1 {
                std::mem::swap(&mut parting.pull.value, &mut parting.tool.value);
            }
        }
        let operation = scope.draft_operation().unwrap();
        assert_eq!(operation.opposite_angle.unwrap().get(), opposite);
        let flags = operation.flags.unwrap();
        assert!(flags.tangent_chain);
        assert_eq!(flags.parting.unwrap().fixed_edges.is_some(), form == 3);
        let mut losses = Vec::new();
        let definition = super::super::draft::project(
            &scope,
            &super::super::ProjectInputs {
                scopes: &[scope.clone(), plane.clone()],
                construction_groups: &construction_groups,
                entity_selection_operands: &[selection(282, &[])],
                ..Default::default()
            },
            &mut losses,
        )
        .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Draft {
            anchor: DraftAnchor::PartingLine { tool, pull },
            angle,
            second_side: None,
            ..
        }) = definition
        else {
            panic!("expected the resolved parting-line Draft");
        };
        assert_eq!(tool, expected_tool);
        assert_eq!(pull.plane.as_ref(), Some(&expected_plane));
        assert_eq!(
            pull.direction.get(),
            Vector3::new(0.0, 0.0, if flipped { -1.0 } else { 1.0 })
        );
        assert_eq!(angle.unwrap().get(), 0.25);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "feature.draft-parting-controls-partial"
        );
    }
}

#[test]
fn parting_draft_retains_origin_pull_faces_and_tool_with_a_named_control_loss() {
    for form in [2, 3] {
        for flipped in [false, true] {
            let mut scope = parting_scope(form);
            if let DesignScopePayloadMut::Draft(Some(operation)) = scope.payload_mut() {
                operation.flags.as_mut().unwrap().flipped = flipped;
            }
            let mut losses = Vec::new();
            let definition = super::super::draft::project(
                &scope,
                &super::super::ProjectInputs {
                    scopes: std::slice::from_ref(&scope),
                    construction_groups: &[
                        group(101, 111, DesignOperandRole::ROLE_0X10),
                        group(102, 112, DesignOperandRole::ROLE_0X21),
                        group(103, 113, DesignOperandRole::BODIES_B),
                        group(104, 114, DesignOperandRole::BODIES_B),
                    ],
                    entity_selection_operands: &[selection(42, &[])],
                    design_types: &crate::test_support::origin_types::origin_design_types(
                        "f3d:test/Design/MetaStream.dat",
                        39,
                    ),
                    ..Default::default()
                },
                &mut losses,
            )
            .unwrap();
            let FeatureDefinition::Operation(FeatureOperation::Draft {
                faces: FaceSelection::Native(faces),
                anchor:
                    DraftAnchor::PartingLine {
                        tool: FaceSelection::Native(tool),
                        pull,
                    },
                angle: Some(angle),
                outward: Some(outward),
                second_side: None,
            }) = definition
            else {
                panic!("partial parting Draft")
            };
            assert!(faces.ends_with("#101"));
            assert!(tool.ends_with("#103"));
            assert!((angle.get() - 0.25).abs() < 1.0e-12);
            assert!(!outward);
            assert_eq!(
                pull.direction.get(),
                Vector3::new(0.0, 0.0, if flipped { -1.0 } else { 1.0 })
            );
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code.local_code(),
                "feature.draft-parting-controls-partial"
            );
        }
    }
}

#[test]
fn parting_draft_keeps_angles_when_pull_bindings_are_missing_duplicate_or_foreign() {
    let scope = parting_scope(2);
    let groups = [
        group(101, 111, DesignOperandRole::ROLE_0X10),
        group(102, 112, DesignOperandRole::ROLE_0X21),
        group(103, 113, DesignOperandRole::BODIES_B),
    ];
    let mut foreign = selection(42, &[]);
    foreign.id = "f3d:other/Design/BulkStream.dat:design-entity-selection-operand#112".into();
    for selections in [
        vec![],
        vec![selection(42, &[]), selection(42, &[])],
        vec![foreign],
    ] {
        let mut losses = Vec::new();
        let result = super::super::draft::project(
            &scope,
            &super::super::ProjectInputs {
                scopes: std::slice::from_ref(&scope),
                construction_groups: &groups,
                entity_selection_operands: &selections,
                design_types: &crate::test_support::origin_types::origin_design_types(
                    "f3d:test/Design/MetaStream.dat",
                    39,
                ),
                ..Default::default()
            },
            &mut losses,
        )
        .unwrap();
        assert!(matches!(
            result,
            FeatureDefinition::Operation(FeatureOperation::Draft {
                anchor: DraftAnchor::NeutralPlane { pull: None, .. },
                angle: Some(_),
                ..
            })
        ));
        assert!(losses
            .iter()
            .any(|loss| loss.code.local_code() == "feature.draft-neutral-anchor-assumed"));
    }
}

#[test]
fn missing_draft_definition_preserves_unique_owned_angle_and_omits_ambiguous_angles() {
    use crate::records::parameters::DesignParameterOwner;
    let mut scope = draft_scope(0.25, None);
    if let DesignScopePayloadMut::Draft(operation) = scope.payload_mut() {
        *operation = None;
    }
    let owner = |stream: &str, value: f64| -> DesignParameterOwner {
        json::from_value(cadmpeg_core::json!({
            "id": format!("{stream}:design-parameter-owner#111"), "byte_offset":0, "frame_length":104,"variant":0,"class_tag":"277",
            "record_index":111,"scope_record_index":100,"local_ordinal":0,"evaluated_value":value,
            "evaluated_value_offset":40,"parameter_record_index":112,"owned_ordinal":0,"companion_record_index":113
        })).unwrap()
    };
    for (owners, expected) in [
        (vec![], None),
        (vec![owner(STREAM, -0.375)], Some(-0.375)),
        (vec![owner(STREAM, 0.25), owner(STREAM, 0.5)], None),
        (vec![owner("f3d:foreign/Design/BulkStream.dat", 0.5)], None),
        (vec![owner(STREAM, 2.0)], None),
    ] {
        let mut losses = Vec::new();
        let result = super::super::draft::project(
            &scope,
            &super::super::ProjectInputs {
                owners: &owners,
                ..Default::default()
            },
            &mut losses,
        )
        .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Draft {
            angle,
            faces: FaceSelection::Native(faces),
            anchor: DraftAnchor::NeutralPlane { pull: None, .. },
            ..
        }) = result
        else {
            panic!("partial Draft")
        };
        assert_eq!(angle.is_some(), expected.is_some());
        if let (Some(angle), Some(expected)) = (angle, expected) {
            assert!((angle.get() - expected).abs() < 1.0e-12);
        }
        assert_eq!(faces, scope.id);
        assert!(losses
            .iter()
            .any(|loss| loss.code.local_code() == "feature.draft-neutral-anchor-assumed"));
        assert_eq!(
            losses
                .iter()
                .any(|loss| loss.code.local_code() == "feature.draft-slope-out-of-range"),
            owners
                .first()
                .is_some_and(|owner| owner.evaluated_value().get() == 2.0)
        );
    }
}
