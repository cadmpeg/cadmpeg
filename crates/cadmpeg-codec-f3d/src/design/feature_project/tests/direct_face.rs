// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::direct_face::{
    DesignOffsetFacesOperation, DesignShellOperation, DesignThickenOperation,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::parameters::DesignParameterOwner;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::serde_erase::json;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::selections::{BodySelection, FaceSelection};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

fn scope(kind: DesignFeatureKind) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty("f3d:test:scope#12", kind, 12);
    match scope.payload_mut() {
        DesignScopePayloadMut::OffsetFaces(slot) => {
            *slot = Some(DesignOffsetFacesOperation {
                distance: cadmpeg_ir::scalar::FiniteReal::new(0.25).unwrap(),
                distance_record_index: 300,
                distance_offset: 0,
            });
        }
        DesignScopePayloadMut::Thicken(slot) => {
            *slot = Some(DesignThickenOperation {
                signed_thickness: cadmpeg_ir::scalar::FiniteReal::new(0.5),
                thickness_record_index: 300,
                thickness_offset: Some(0),
                symmetric: false,
                tangent_chain: true,
                ..Default::default()
            });
        }
        DesignScopePayloadMut::Shell(slot) => {
            *slot = Some(DesignShellOperation {
                thickness: cadmpeg_ir::scalar::PositiveReal::new(0.5).unwrap(),
                thickness_record_index: 300,
                thickness_offset: 0,
                outward: true,
                outward_offset: 0,
                wall_slots: None,
                prefix_flag: None,
                auxiliary_references: Vec::new(),
            });
        }
        _ => panic!("unsupported direct-face scope fixture"),
    }
    scope
}

pub(super) fn group(role: DesignOperandRole) -> DesignConstructionOperandGroup {
    json::from_value(cadmpeg_core::json!({
        "id": "f3d:test:group#100",
        "scope_record_index": 12,
        "scope_reference_ordinal": 0,
        "record_index": 100,
        "byte_offset": 1000,
        "class_tag": "332",
        "members": [200],
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

fn projected_operation(
    projected: Result<Option<FeatureDefinition>, CodecError>,
) -> FeatureOperation {
    let Some(FeatureDefinition::Operation(operation)) = projected.unwrap() else {
        panic!("expected a projected operation");
    };
    operation
}

#[test]
fn offset_faces_projects_native_group() {
    let scope = scope(DesignFeatureKind::OffsetFaces);
    let group = group(DesignOperandRole::ROLE_0X10);
    let FeatureOperation::MoveFace { faces, .. } = projected_operation(
        crate::design::feature_project::direct_face::project_offset_faces(
            &scope,
            &[],
            &[],
            std::slice::from_ref(&group),
        ),
    ) else {
        panic!("expected a face move");
    };
    assert_eq!(faces, FaceSelection::Native(group.id.clone()));
}

#[test]
fn thicken_projects_native_group() {
    let scope = scope(DesignFeatureKind::Thicken);
    let group = group(DesignOperandRole::ROLE_0X5);
    let FeatureOperation::Thicken { faces, .. } =
        projected_operation(Ok(super::super::thicken::project_thicken(
            &scope,
            &[],
            std::slice::from_ref(&group),
            &mut Vec::new(),
        )))
    else {
        panic!("expected a thicken");
    };
    assert_eq!(faces, FaceSelection::Native(group.id.clone()));
}

#[test]
fn thicken_retains_direct_scope_faces_with_declared_input_groups() {
    for missing_group in [false, true] {
        let mut scope = thicken_after_state(5);
        if let DesignScopePayloadMut::Thicken(Some(operation)) = scope.payload_mut() {
            operation.group_record_indices = if missing_group {
                vec![100, 110]
            } else {
                vec![100]
            };
        }
        let mut operand = resolved_member_operand();
        operand.group = None;
        let mut losses = Vec::new();
        let FeatureOperation::Thicken { faces, .. } =
            projected_operation(Ok(super::super::thicken::project_thicken(
                &scope,
                &[operand],
                &[group(DesignOperandRole::ROLE_0X12)],
                &mut losses,
            )))
        else {
            panic!("Thicken");
        };
        let expected = crate::ids::history_input_face_id(
            &crate::ids::feature_input_prefix(&crate::ids::neutral_feature_id(&scope), 5),
            27,
        );
        match faces {
            FaceSelection::Historical { faces, .. } => {
                assert!(!missing_group);
                assert_eq!(faces.as_slice(), [expected]);
            }
            FaceSelection::HistoricalPartial {
                faces, unresolved, ..
            } => {
                assert!(missing_group);
                assert_eq!(faces.as_slice(), [expected]);
                assert_eq!(unresolved.as_slice(), [scope.id]);
            }
            _ => panic!("direct historical face is retained"),
        }
        assert_eq!(losses.len(), usize::from(missing_group));
        if missing_group {
            assert_eq!(
                losses[0].code.local_code(),
                "feature.thicken-face-selection-assumed"
            );
        }
    }
}

#[test]
fn thicken_boolean_results_survive_missing_scalar_and_target_definitions() {
    use crate::records::feature::direct_face::{
        DesignThickenBoolean, DesignThickenBooleanOperation,
    };
    use cadmpeg_ir::features::direct_edit::ThickenSide;
    use cadmpeg_ir::features::operation_settings::{BooleanKind, SolidFeatureResult};

    for (stored, expected) in [
        (DesignThickenBooleanOperation::Join, BooleanKind::Join),
        (DesignThickenBooleanOperation::Cut, BooleanKind::Cut),
        (
            DesignThickenBooleanOperation::Intersect,
            BooleanKind::Intersect,
        ),
    ] {
        for has_target in [false, true] {
            let mut scope = scope(DesignFeatureKind::Thicken);
            if let DesignScopePayloadMut::Thicken(Some(operation)) = scope.payload_mut() {
                operation.group_record_indices = vec![100];
                operation.signed_thickness = None;
                operation.thickness_offset = None;
                operation.symmetric = true;
                operation.boolean = Some(DesignThickenBoolean {
                    operation: stored,
                    operation_offset: 21,
                    target_group_record_index: 110,
                });
            }
            let mut groups = vec![group(DesignOperandRole::ROLE_0X12)];
            let mut target = group(DesignOperandRole::BODIES_A);
            target.id = "f3d:test:group#110".into();
            target.record_index = 110;
            if has_target {
                groups.push(target.clone());
            }
            let mut losses = Vec::new();
            let FeatureOperation::Thicken {
                faces,
                thickness,
                side,
                result,
                tangent_chain,
            } = projected_operation(Ok(super::super::thicken::project_thicken(
                &scope,
                &[],
                &groups,
                &mut losses,
            )))
            else {
                panic!("Thicken");
            };
            assert_eq!(faces, FaceSelection::Native(groups[0].id.clone()));
            assert_eq!(thickness, None);
            assert_eq!(side, Some(ThickenSide::Both));
            assert_eq!(tangent_chain, Some(true));
            assert_eq!(
                result,
                Some(SolidFeatureResult::Boolean {
                    operation: expected,
                    targets: BodySelection::Native(if has_target { target.id } else { scope.id }),
                })
            );
            assert_eq!(losses.len(), usize::from(!has_target));
            if !has_target {
                assert_eq!(
                    losses[0].code.local_code(),
                    "feature.thicken-target-selection-assumed"
                );
            }
        }
    }
}

#[test]
fn thicken_unknown_frame_and_missing_input_groups_still_project() {
    let mut scope = scope(DesignFeatureKind::Thicken);
    if let DesignScopePayloadMut::Thicken(slot) = scope.payload_mut() {
        *slot = None;
    }
    let mut losses = Vec::new();
    let FeatureOperation::Thicken {
        faces,
        thickness,
        side,
        result,
        tangent_chain,
    } = projected_operation(Ok(super::super::thicken::project_thicken(
        &scope,
        &[],
        &[],
        &mut losses,
    )))
    else {
        panic!("Thicken");
    };
    assert_eq!(faces, FaceSelection::Native(scope.id.clone()));
    assert_eq!(
        (thickness, side, result, tangent_chain),
        (None, None, None, None)
    );
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.code.local_code())
            .collect::<Vec<_>>(),
        [
            "feature.thicken-face-selection-assumed",
            "feature.thicken-frame-assumed",
        ]
    );

    let mut scope = thicken_after_state(5);
    if let DesignScopePayloadMut::Thicken(Some(operation)) = scope.payload_mut() {
        operation.group_record_indices = vec![100, 110];
    }
    let FeatureOperation::Thicken { faces, .. } =
        projected_operation(Ok(super::super::thicken::project_thicken(
            &scope,
            &[resolved_member_operand()],
            &[group(DesignOperandRole::ROLE_0X12)],
            &mut Vec::new(),
        )))
    else {
        panic!("Thicken");
    };
    let FaceSelection::HistoricalPartial {
        faces, unresolved, ..
    } = faces
    else {
        panic!("partial faces");
    };
    assert_eq!(faces.as_slice().len(), 1);
    assert_eq!(unresolved.as_slice(), [scope.id]);
}

#[test]
fn thicken_zero_and_overflow_keep_result_and_chain_settings() {
    for value in [0.0, 1.5e307] {
        let mut scope = scope(DesignFeatureKind::Thicken);
        if let DesignScopePayloadMut::Thicken(Some(operation)) = scope.payload_mut() {
            operation.signed_thickness = cadmpeg_ir::scalar::FiniteReal::new(value);
            operation.symmetric = true;
        }
        let mut losses = Vec::new();
        let FeatureOperation::Thicken {
            thickness,
            result,
            tangent_chain,
            ..
        } = projected_operation(Ok(super::super::thicken::project_thicken(
            &scope,
            &[],
            &[group(DesignOperandRole::ROLE_0X5)],
            &mut losses,
        )))
        else {
            panic!("Thicken");
        };
        assert_eq!(thickness, None);
        assert_eq!(
            result,
            Some(cadmpeg_ir::features::operation_settings::SolidFeatureResult::NewBody {})
        );
        assert_eq!(tangent_chain, Some(true));
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "feature.thicken-thickness-unrepresentable"
        );
    }
}

#[test]
fn a_symmetric_thicken_adds_the_stored_thickness_on_each_side() {
    let group = group(DesignOperandRole::ROLE_0X5);
    let project = |symmetric: bool, signed_thickness: f64| {
        let mut scope = scope(DesignFeatureKind::Thicken);
        if let DesignScopePayloadMut::Thicken(Some(operation)) = scope.payload_mut() {
            operation.symmetric = symmetric;
            operation.signed_thickness = cadmpeg_ir::scalar::FiniteReal::new(signed_thickness);
        }
        let FeatureOperation::Thicken {
            thickness, side, ..
        } = projected_operation(Ok(super::super::thicken::project_thicken(
            &scope,
            &[],
            std::slice::from_ref(&group),
            &mut Vec::new(),
        )))
        else {
            panic!("expected a thicken");
        };
        (thickness.map(cadmpeg_ir::scalar::PositiveLength::get), side)
    };
    assert_eq!(
        project(true, 0.5),
        (
            Some(10.0),
            Some(cadmpeg_ir::features::direct_edit::ThickenSide::Both)
        )
    );
    assert_eq!(
        project(true, -0.5),
        (
            Some(10.0),
            Some(cadmpeg_ir::features::direct_edit::ThickenSide::Both)
        )
    );
    assert_eq!(
        project(false, 0.5),
        (
            Some(5.0),
            Some(cadmpeg_ir::features::direct_edit::ThickenSide::Forward)
        )
    );
    assert_eq!(
        project(false, -0.5),
        (
            Some(5.0),
            Some(cadmpeg_ir::features::direct_edit::ThickenSide::Reverse)
        )
    );

    let mut scope = scope(DesignFeatureKind::Thicken);
    if let DesignScopePayloadMut::Thicken(Some(operation)) = scope.payload_mut() {
        operation.symmetric = true;
        operation.signed_thickness = cadmpeg_ir::scalar::FiniteReal::new(1.5e307);
    }
    let FeatureOperation::Thicken {
        faces, thickness, ..
    } = projected_operation(Ok(super::super::thicken::project_thicken(
        &scope,
        &[],
        std::slice::from_ref(&group),
        &mut Vec::new(),
    )))
    else {
        panic!("expected a thicken");
    };
    assert_eq!(faces, FaceSelection::Native(group.id.clone()));
    assert!(thickness.is_none());
}

fn thicken_after_state(previous_state: i64) -> DesignParameterScope {
    let mut scope = scope(DesignFeatureKind::Thicken);
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(previous_state);
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn thicken_lists_a_whole_body_group_beside_resolved_faces_as_unresolved() {
    let scope = thicken_after_state(5);
    let faces_group = group(DesignOperandRole::ROLE_0X12);
    let mut whole_body = group(DesignOperandRole::ROLE_0X5);
    whole_body.id = "f3d:test:group#110".into();
    whole_body.record_index = 110;
    let groups = [faces_group, whole_body.clone()];
    let FeatureOperation::Thicken { faces, .. } =
        projected_operation(Ok(super::super::thicken::project_thicken(
            &scope,
            &[resolved_member_operand()],
            &groups,
            &mut Vec::new(),
        )))
    else {
        panic!("expected a thicken");
    };
    let mut rebound = faces.clone();
    super::super::thicken::list_thicken_whole_bodies(&mut rebound, &scope, &groups);
    assert_eq!(rebound, faces);
    let FaceSelection::HistoricalPartial {
        faces,
        unresolved,
        native,
        ..
    } = faces
    else {
        panic!("expected a partial historical selection, got {faces:?}");
    };
    assert_eq!(faces.as_slice().len(), 1);
    assert_eq!(unresolved.as_slice(), [whole_body.id]);
    assert_eq!(native, scope.id.as_str());
}

#[test]
fn thicken_keeps_resolved_faces_without_a_whole_body_group() {
    let scope = thicken_after_state(5);
    let FeatureOperation::Thicken { faces, .. } =
        projected_operation(Ok(super::super::thicken::project_thicken(
            &scope,
            &[resolved_member_operand()],
            &[group(DesignOperandRole::ROLE_0X12)],
            &mut Vec::new(),
        )))
    else {
        panic!("expected a thicken");
    };
    assert!(
        matches!(faces, FaceSelection::Historical { .. }),
        "{faces:?}"
    );
}

#[test]
fn shell_projects_native_face_group() {
    let scope = scope(DesignFeatureKind::Shell);
    let group = group(DesignOperandRole::ROLE_0X10);
    let FeatureOperation::Shell {
        bodies,
        removed_faces,
        ..
    } = projected_operation(crate::design::feature_project::direct_face::project_shell(
        &scope,
        &[],
        std::slice::from_ref(&group),
    ))
    else {
        panic!("expected a shell");
    };
    assert_eq!(removed_faces, FaceSelection::Native(group.id.clone()));
    assert_eq!(bodies, None);
}

#[test]
fn shell_projects_native_body_group() {
    let scope = scope(DesignFeatureKind::Shell);
    let group = group(DesignOperandRole::BODIES_A);
    let FeatureOperation::Shell {
        bodies,
        removed_faces,
        ..
    } = projected_operation(crate::design::feature_project::direct_face::project_shell(
        &scope,
        &[],
        std::slice::from_ref(&group),
    ))
    else {
        panic!("expected a shell");
    };
    assert_eq!(bodies, Some(BodySelection::Native(group.id.clone())));
    assert_eq!(removed_faces, FaceSelection::Faces(Vec::new()));
}

fn wall_slot_scope(
    inside: f64,
    outside: Option<f64>,
    rounded_corners: bool,
) -> DesignParameterScope {
    use crate::records::feature::direct_face::{DesignShellOutsideWall, DesignShellWallSlots};
    let mut scope = scope(DesignFeatureKind::Shell);
    if let DesignScopePayloadMut::Shell(Some(operation)) = scope.payload_mut() {
        operation.thickness = cadmpeg_ir::scalar::PositiveReal::new(inside).unwrap();
        operation.outward = false;
        operation.wall_slots = Some(DesignShellWallSlots {
            tangent_chain: false,
            rounded_corners,
            outside: outside.map(|thickness| DesignShellOutsideWall {
                thickness: cadmpeg_ir::scalar::PositiveReal::new(thickness).unwrap(),
                thickness_record_index: 301,
                thickness_offset: 0,
            }),
        });
    }
    scope
}

#[test]
fn wall_slots_project_thickness_direction_mode_and_corners() {
    use cadmpeg_ir::features::operation_settings::{ShellJoin, ShellMode};
    let group = group(DesignOperandRole::ROLE_0X10);
    let project = |scope: &DesignParameterScope| {
        let FeatureOperation::Shell {
            thickness,
            outward,
            mode,
            join,
            ..
        } = projected_operation(crate::design::feature_project::direct_face::project_shell(
            scope,
            &[],
            std::slice::from_ref(&group),
        ))
        else {
            panic!("expected a shell");
        };
        (thickness.unwrap().get(), outward, mode, join)
    };
    assert_eq!(
        project(&wall_slot_scope(0.2, None, false)),
        (2.0, Some(false), None, Some(ShellJoin::Intersection))
    );
    assert_eq!(
        project(&wall_slot_scope(0.2, None, true)),
        (2.0, Some(false), None, Some(ShellJoin::Arc))
    );
    assert_eq!(
        project(&wall_slot_scope(0.1, Some(0.1), false)),
        (
            2.0,
            None,
            Some(ShellMode::BothSides),
            Some(ShellJoin::Intersection)
        )
    );
}

#[test]
fn unequal_walls_project_both_sides_with_unresolved_thickness() {
    let group = group(DesignOperandRole::ROLE_0X10);
    let scope = wall_slot_scope(0.3, Some(0.1), false);
    let FeatureOperation::Shell {
        thickness,
        outward,
        mode,
        removed_faces,
        join,
        ..
    } = projected_operation(crate::design::feature_project::direct_face::project_shell(
        &scope,
        &[],
        std::slice::from_ref(&group),
    ))
    else {
        panic!("expected a Shell");
    };
    assert_eq!(thickness, None);
    assert_eq!(outward, None);
    assert_eq!(
        mode,
        Some(cadmpeg_ir::features::operation_settings::ShellMode::BothSides)
    );
    assert_eq!(removed_faces, FaceSelection::Native(group.id));
    assert_eq!(
        join,
        Some(cadmpeg_ir::features::operation_settings::ShellJoin::Intersection)
    );
}

#[test]
fn a_shell_without_wall_slots_states_no_corners() {
    let group = group(DesignOperandRole::ROLE_0X10);
    let scope = scope(DesignFeatureKind::Shell);
    let FeatureOperation::Shell { outward, join, .. } =
        projected_operation(crate::design::feature_project::direct_face::project_shell(
            &scope,
            &[],
            std::slice::from_ref(&group),
        ))
    else {
        panic!("expected a shell");
    };
    assert_eq!(outward, Some(true));
    assert_eq!(join, None);
}

fn with_table(mut scope: DesignParameterScope, table: Vec<u32>) -> DesignParameterScope {
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(table);
            draft.frame_length = 256;
            draft.paired_byte_offset = 256;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn a_shell_table_of_face_group_wall_and_members_reports_no_loss() {
    let group = group(DesignOperandRole::ROLE_0X10);
    let scope = with_table(scope(DesignFeatureKind::Shell), vec![100, 300, 200]);
    assert_eq!(
        super::super::shell_table_member_loss(&scope, std::slice::from_ref(&group)),
        None
    );
}

#[test]
fn a_shell_table_reference_outside_the_face_group_keeps_the_shell_and_reports_it() {
    let group = group(DesignOperandRole::ROLE_0X10);
    let scope = with_table(scope(DesignFeatureKind::Shell), vec![100, 300, 200, 400]);
    let loss = super::super::shell_table_member_loss(&scope, std::slice::from_ref(&group)).unwrap();
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::FeatureShellTableMembersAssumed.kind()
    );
    assert!(
        loss.message
            .contains("ordered references [200, 400] besides face group 100 and wall scalar 300"),
        "{}",
        loss.message
    );
    assert!(loss.message.contains("members [200]"), "{}", loss.message);
    let FeatureOperation::Shell {
        removed_faces,
        thickness,
        outward,
        ..
    } = projected_operation(crate::design::feature_project::direct_face::project_shell(
        &scope,
        &[],
        std::slice::from_ref(&group),
    ))
    else {
        panic!("expected a shell");
    };
    assert_eq!(removed_faces, FaceSelection::Native(group.id.clone()));
    assert_eq!(thickness.unwrap().get(), 5.0);
    assert_eq!(outward, Some(true));
}

#[test]
fn a_cornered_shell_table_takes_no_member_comparison() {
    let group = group(DesignOperandRole::ROLE_0X10);
    let scope = with_table(wall_slot_scope(0.2, None, false), vec![100, 200, 400, 300]);
    assert_eq!(
        super::super::shell_table_member_loss(&scope, std::slice::from_ref(&group)),
        None
    );
}

fn move_face_scope(transform: [[f64; 4]; 4]) -> DesignParameterScope {
    use crate::records::feature::direct_face::{DesignMoveForm, DesignMoveOperation};
    let mut scope =
        DesignParameterScope::empty("f3d:test:scope#12", DesignFeatureKind::MoveFace, 12);
    let DesignScopePayloadMut::MoveFace(slot) = scope.payload_mut() else {
        panic!("MoveFace payload");
    };
    *slot = Some(DesignMoveOperation {
        transform: transform.try_into().unwrap(),
        transform_offset: 448,
        transform_record_index: 300,
        form: DesignMoveForm::Form5,
        form_offset: 443,
        transform_reference_offset: None,
        prefix: None,
        face_group: Some(crate::records::identity::Located {
            value: 100,
            offset: 0,
        }),
    });
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 200, 300]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn move_face_projects_a_translation_of_its_group() {
    use cadmpeg_ir::features::direct_edit::FaceMotion;
    let scope = move_face_scope([
        [1., 0., 0., 0.],
        [0., 1., 0., -0.25],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ]);
    let group = group(DesignOperandRole::ROLE_0X10);
    let FeatureOperation::MoveFace {
        faces,
        motion: FaceMotion::Translate {
            direction,
            distance,
        },
    } = projected_operation(
        crate::design::feature_project::direct_face::project_move_face(
            &scope,
            std::slice::from_ref(&group),
            &mut Vec::new(),
        ),
    )
    else {
        panic!("expected a face translation");
    };
    assert_eq!(faces, FaceSelection::Native(group.id.clone()));
    assert_eq!(direction.y, -1.);
    assert_eq!(distance.get(), 2.5);
}

#[test]
fn move_face_projects_a_transform_stored_as_the_first_reference() {
    let mut scope = move_face_scope([
        [1., 0., 0., 0.],
        [0., 1., 0., -0.25],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ]);
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![300, 100, 200]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut faces = group(DesignOperandRole::ROLE_0X10);
    assert!(
        crate::design::feature_project::direct_face::project_move_face(
            &scope,
            std::slice::from_ref(&faces),
            &mut Vec::new(),
        )
        .unwrap()
        .is_some(),
        "the decoded motion survives an unresolved group ordinal"
    );
    faces.scope_reference_ordinal = 1;
    assert!(matches!(
        projected_operation(
            crate::design::feature_project::direct_face::project_move_face(
                &scope,
                std::slice::from_ref(&faces),
                &mut Vec::new(),
            )
        ),
        FeatureOperation::MoveFace { .. }
    ));
}

#[test]
fn move_face_keeps_motion_without_a_resolved_group() {
    let translation = [
        [1., 0., 0., 0.25],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let faces = group(DesignOperandRole::ROLE_0X10);
    let refused = |transform, groups: &[DesignConstructionOperandGroup]| {
        crate::design::feature_project::direct_face::project_move_face(
            &move_face_scope(transform),
            groups,
            &mut Vec::new(),
        )
        .unwrap()
        .is_none()
    };
    assert!(!refused(translation, std::slice::from_ref(&faces)));
    assert!(!refused(translation, &[]));
    assert!(!refused(translation, &[group(DesignOperandRole::BODIES_A)]));
    assert!(!refused(translation, &[faces.clone(), faces.clone()]));
    let mut other_member = faces.clone();
    other_member.record_index = 101;
    assert!(!refused(translation, &[other_member]));
    let identity = [
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    assert!(!refused(identity, std::slice::from_ref(&faces)));
}

#[test]
fn move_face_projects_a_rotation_about_the_axis_point_nearest_the_origin() {
    use cadmpeg_ir::features::direct_edit::FaceMotion;
    const EPS_ROTATION_FIELD: f64 = 1.0e-9;
    let (sine, cosine) = 5f64.to_radians().sin_cos();
    // A rotation about +X through (0, 1.5, 2) cm stores t = p - R p.
    let (y, z) = (1.5, 2.0);
    let five_degrees = [
        [1., 0., 0., 0.],
        [0., cosine, -sine, y - (cosine * y - sine * z)],
        [0., sine, cosine, z - (sine * y + cosine * z)],
        [0., 0., 0., 1.],
    ];
    let quarter_turn = [
        [0., 1., 0., 0.25],
        [-1., 0., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let faces = group(DesignOperandRole::ROLE_0X10);
    for (transform, origin, axis, degrees) in [
        (five_degrees, [0., 15., 20.], [1., 0., 0.], 5.),
        (quarter_turn, [1.25, -1.25, 0.], [0., 0., -1.], 90.),
    ] {
        let FeatureOperation::MoveFace {
            faces: selected,
            motion:
                FaceMotion::Rotate {
                    axis_origin,
                    axis_dir,
                    angle,
                },
        } = projected_operation(
            crate::design::feature_project::direct_face::project_move_face(
                &move_face_scope(transform),
                std::slice::from_ref(&faces),
                &mut Vec::new(),
            ),
        )
        else {
            panic!("expected a face rotation");
        };
        assert_eq!(selected, FaceSelection::Native(faces.id.clone()));
        let point = axis_origin.get();
        let direction = axis_dir.get();
        for (actual, expected) in [point.x, point.y, point.z]
            .into_iter()
            .zip(origin)
            .chain(
                [direction.x, direction.y, direction.z]
                    .into_iter()
                    .zip(axis),
            )
            .chain([(angle.get(), f64::to_radians(degrees))])
        {
            assert!(
                (actual - expected).abs() <= EPS_ROTATION_FIELD,
                "{actual} {expected}"
            );
        }
    }
}

#[test]
fn move_face_preserves_a_screw_motion() {
    let screw = [
        [0., 1., 0., 0.25],
        [-1., 0., 0., 0.],
        [0., 0., 1., 0.25],
        [0., 0., 0., 1.],
    ];
    assert!(
        crate::design::feature_project::direct_face::project_move_face(
            &move_face_scope(screw),
            std::slice::from_ref(&group(DesignOperandRole::ROLE_0X10)),
            &mut Vec::new(),
        )
        .unwrap()
        .is_some()
    );
}

#[test]
fn move_face_stays_native_for_a_reflection() {
    let faces = group(DesignOperandRole::ROLE_0X10);
    for transform in [
        [
            [-1., 0., 0., 0.25],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ],
        [
            [1., 0., 0., 0.],
            [0., 1., 0., 0.25],
            [0., 0., -1., 0.],
            [0., 0., 0., 1.],
        ],
    ] {
        assert!(
            crate::design::feature_project::direct_face::project_move_face(
                &move_face_scope(transform),
                std::slice::from_ref(&faces),
                &mut Vec::new(),
            )
            .unwrap()
            .is_none()
        );
    }
}

#[test]
fn move_transform_projects_signed_rotation_translation_and_unique_body_group() {
    use crate::records::feature::direct_face::{DesignMoveForm, DesignMoveOperation};
    let mut scope = DesignParameterScope::empty("f3d:test:scope#12", DesignFeatureKind::Move, 12);
    let DesignScopePayloadMut::Move(slot) = scope.payload_mut() else {
        panic!("Move payload");
    };
    *slot = Some(DesignMoveOperation {
        transform: [
            [0., 1., 0., 7.],
            [-1., 0., 0., 11.],
            [0., 0., 1., 13.],
            [0., 0., 0., 1.],
        ]
        .try_into()
        .unwrap(),
        transform_offset: 448,
        transform_record_index: 71,
        form: DesignMoveForm::Form1,
        form_offset: 443,
        transform_reference_offset: None,
        prefix: None,
        ..DesignMoveOperation::default()
    });
    let group = group(DesignOperandRole::BODIES_A);
    let Some(FeatureDefinition::Operation(FeatureOperation::MoveBody {
        bodies,
        translation,
        rotation: Some(rotation),
        copies,
    })) = crate::design::feature_project::body_ops::project_move(
        &scope,
        std::slice::from_ref(&group),
        &[],
        &mut Vec::new(),
    )
    else {
        panic!("MoveBody");
    };
    assert_eq!(bodies, BodySelection::Native(group.id.clone()));
    assert_eq!(
        translation.get(),
        cadmpeg_ir::math::Vector3::new(70., 110., 130.)
    );
    assert_eq!(
        rotation.origin.get(),
        cadmpeg_ir::math::Point3::new(0., 0., 0.)
    );
    assert_eq!(rotation.direction.z, -1.);
    assert!((rotation.angle.get() - std::f64::consts::FRAC_PI_2).abs() <= f64::EPSILON);
    assert_eq!(copies, 0);
    assert!(crate::design::feature_project::body_ops::project_move(
        &scope,
        &[],
        &[],
        &mut Vec::new()
    )
    .is_none());
    assert!(crate::design::feature_project::body_ops::project_move(
        &scope,
        &[group.clone(), group.clone()],
        &[],
        &mut Vec::new()
    )
    .is_none());
    scope
        .try_edit(|draft| {
            draft.reference_count_offset = 50;
            draft.frame_length = 350;
            draft.paired_byte_offset = 350;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 200, 71, 900, 901]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut losses = Vec::new();
    assert!(crate::design::feature_project::body_ops::project_move(
        &scope,
        std::slice::from_ref(&group),
        &[],
        &mut losses
    )
    .is_some());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureMoveScopeReferencesAssumed.kind()
    );
    let DesignScopePayloadMut::Move(Some(operation)) = scope.payload_mut() else {
        panic!("Move payload");
    };
    operation.transform = [
        [-1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ]
    .try_into()
    .unwrap();
    assert!(crate::design::feature_project::body_ops::project_move(
        &scope,
        &[group],
        &[],
        &mut Vec::new()
    )
    .is_none());
}

fn move_owner(record_index: u32, local_ordinal: u32, value: f64) -> DesignParameterOwner {
    owner_in("f3d:test", 12, record_index, local_ordinal, value)
}

fn owner_in(
    stream: &str,
    scope_record_index: u32,
    record_index: u32,
    local_ordinal: u32,
    value: f64,
) -> DesignParameterOwner {
    json::from_value(cadmpeg_core::json!({
        "id": format!("{stream}:design-parameter-owner#{record_index}"),
        "byte_offset": 2000 + record_index,
        "frame_length": 104,
        "class_tag": "351",
        "record_index": record_index,
        "scope_record_index": scope_record_index,
        "local_ordinal": local_ordinal,
        "evaluated_value": value,
        "evaluated_value_offset": 2040 + record_index,
        "parameter_record_index": record_index + 2,
        "owned_ordinal": 1,
        "variant": 0,
        "companion_record_index": record_index + 1,
    }))
    .unwrap()
}

/// A `Move` scope with body group 100, transform 71, and further references
/// 900, 901 and 902.
fn move_scope(
    form: crate::records::feature::direct_face::DesignMoveForm,
    transform: [[f64; 4]; 4],
    codes: Option<[u32; 2]>,
) -> DesignParameterScope {
    use crate::records::feature::direct_face::{
        DesignMoveOperation, DesignMovePrefix, DesignMoveReference,
    };
    let mut scope = DesignParameterScope::empty("f3d:test:scope#12", DesignFeatureKind::Move, 12);
    scope
        .try_edit(|draft| {
            draft.reference_count_offset = 50;
            draft.frame_length = 350;
            draft.paired_byte_offset = 350;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                100, 200, 71, 900, 901, 902,
            ]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let DesignScopePayloadMut::Move(slot) = scope.payload_mut() else {
        panic!("Move payload");
    };
    *slot = Some(DesignMoveOperation {
        transform: transform.try_into().unwrap(),
        transform_record_index: 71,
        form,
        prefix: codes.map(|codes| DesignMovePrefix {
            references: std::array::from_fn(|_| DesignMoveReference::Null),
            codes,
            matrix_omitted: false,
            leading: None,
        }),
        ..DesignMoveOperation::default()
    });
    scope
}

fn move_scope_reference_loss(
    scope: &DesignParameterScope,
    owners: &[DesignParameterOwner],
) -> Option<String> {
    let mut losses = Vec::new();
    assert!(crate::design::feature_project::body_ops::project_move(
        scope,
        std::slice::from_ref(&group(DesignOperandRole::BODIES_A)),
        owners,
        &mut losses
    )
    .is_some());
    assert!(losses.len() <= 1);
    losses.pop().map(|loss| {
        assert_eq!(
            loss.code,
            crate::loss::F3dLossCode::FeatureMoveScopeReferencesAssumed.kind()
        );
        loss.message
    })
}

const MOVE_TRANSLATION: [[f64; 4]; 4] = [
    [1., 0., 0., 0.7],
    [0., 1., 0., -0.3],
    [0., 0., 1., 0.2],
    [0., 0., 0., 1.],
];

#[test]
fn translation_distance_owners_that_equal_the_matrix_translation_are_classified() {
    use crate::records::feature::direct_face::DesignMoveForm;
    let owners = [
        move_owner(900, 0, 0.7),
        move_owner(901, 1, -0.3),
        move_owner(902, 2, 0.2),
    ];
    for codes in [None, Some([0, 0]), Some([0, 1])] {
        let scope = move_scope(DesignMoveForm::Form0, MOVE_TRANSLATION, codes);
        assert_eq!(
            move_scope_reference_loss(&scope, &owners),
            None,
            "{codes:?}"
        );
    }
}

#[test]
fn world_translation_distance_owners_agree_with_the_matrix_by_length() {
    use crate::records::feature::direct_face::DesignMoveForm;
    let component = [
        [1., 0., 0., -0.3],
        [0., 1., 0., -0.7],
        [0., 0., 1., 0.2],
        [0., 0., 0., 1.],
    ];
    let owners = [
        move_owner(900, 0, 0.7),
        move_owner(901, 1, -0.3),
        move_owner(902, 2, 0.2),
    ];
    let world = move_scope(DesignMoveForm::Form0, component, Some([0, 1]));
    assert_eq!(move_scope_reference_loss(&world, &owners), None);
    let local = move_scope(DesignMoveForm::Form0, component, Some([0, 0]));
    assert!(move_scope_reference_loss(&local, &owners)
        .unwrap()
        .contains("[900, 901, 902]"));
}

#[test]
fn translation_owners_that_disagree_with_the_matrix_stay_auxiliary() {
    use crate::records::feature::direct_face::DesignMoveForm;
    let scope = move_scope(DesignMoveForm::Form0, MOVE_TRANSLATION, Some([0, 1]));
    for owners in [
        vec![
            move_owner(900, 0, 0.7),
            move_owner(901, 1, -0.3),
            move_owner(902, 2, 0.5),
        ],
        vec![move_owner(900, 0, 0.7), move_owner(901, 1, -0.3)],
        vec![
            move_owner(900, 0, 0.7),
            move_owner(901, 1, -0.3),
            move_owner(902, 3, 0.2),
        ],
    ] {
        assert!(move_scope_reference_loss(&scope, &owners)
            .unwrap()
            .contains("[900, 901, 902]"));
    }
    let disagreeing = [
        move_owner(900, 0, 0.7),
        move_owner(901, 1, -0.3),
        move_owner(902, 2, 0.5),
    ];
    let message = move_scope_reference_loss(&scope, &disagreeing).unwrap();
    assert!(
        message.contains("parameter owners [900, 901, 902] disagree"),
        "{message}"
    );
    let rotation = move_scope(DesignMoveForm::Form1, MOVE_TRANSLATION, None);
    let owners = [
        move_owner(900, 0, 0.7),
        move_owner(901, 1, -0.3),
        move_owner(902, 2, 0.2),
    ];
    assert!(move_scope_reference_loss(&rotation, &owners)
        .unwrap()
        .contains("[900, 901, 902]"));
}

#[test]
fn world_distances_of_a_rotated_component_agree_with_the_component_translation() {
    use crate::records::feature::direct_face::DesignMoveForm;
    let component = [
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.5],
        [0., 0., 0., 1.],
    ];
    let owners = [
        move_owner(900, 0, 0.3),
        move_owner(901, 1, 0.4),
        move_owner(902, 2, 0.),
    ];
    let world = move_scope(DesignMoveForm::Form0, component, Some([0, 1]));
    assert_eq!(move_scope_reference_loss(&world, &owners), None);
}

#[test]
fn translation_owners_with_unknown_codes_or_outside_the_scope_stay_auxiliary() {
    use crate::records::feature::direct_face::DesignMoveForm;
    let unknown = move_scope(DesignMoveForm::Form0, MOVE_TRANSLATION, Some([0, 2]));
    let owners = [
        move_owner(900, 0, 0.7),
        move_owner(901, 1, -0.3),
        move_owner(902, 2, 0.2),
    ];
    let message = move_scope_reference_loss(&unknown, &owners).unwrap();
    assert!(message.contains("[900, 901, 902]") && !message.contains("disagree"));
    let scope = move_scope(DesignMoveForm::Form0, MOVE_TRANSLATION, None);
    for third in [
        owner_in("f3d:other", 12, 902, 2, 0.2),
        owner_in("f3d:test", 13, 902, 2, 0.2),
        move_owner(950, 2, 0.2),
        move_owner(902, 1, 0.2),
    ] {
        let owners = [move_owner(900, 0, 0.7), move_owner(901, 1, -0.3), third];
        let message = move_scope_reference_loss(&scope, &owners).unwrap();
        assert!(message.contains("[900, 901, 902]"), "{message}");
    }
}

#[test]
fn a_rotation_angle_owner_is_compared_in_radians_modulo_a_full_turn() {
    use crate::records::feature::direct_face::DesignMoveForm;
    let (sine, cosine) = std::f64::consts::FRAC_PI_4.sin_cos();
    let rotation = [
        [cosine, -sine, 0., 0.],
        [sine, cosine, 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let scope = move_scope(DesignMoveForm::Form1, rotation, None);
    for angle in [
        -std::f64::consts::FRAC_PI_4,
        std::f64::consts::TAU - std::f64::consts::FRAC_PI_4,
        std::f64::consts::TAU + std::f64::consts::FRAC_PI_4,
    ] {
        assert!(
            move_scope_reference_loss(&scope, &[move_owner(900, 0, angle)])
                .unwrap()
                .contains("[901, 902]")
        );
    }
    let identity = move_scope(
        DesignMoveForm::Form1,
        [
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ],
        None,
    );
    assert!(
        move_scope_reference_loss(&identity, &[move_owner(900, 0, 4.0e-5)])
            .unwrap()
            .contains("parameter owners [900] disagree")
    );
}

#[test]
fn a_rotation_angle_owner_that_equals_the_matrix_angle_is_classified() {
    use crate::records::feature::direct_face::DesignMoveForm;
    let (sine, cosine) = std::f64::consts::FRAC_PI_4.sin_cos();
    let rotation = [
        [cosine, -sine, 0., 0.],
        [sine, cosine, 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let scope = move_scope(DesignMoveForm::Form1, rotation, None);
    let agreed = [move_owner(900, 0, std::f64::consts::FRAC_PI_4)];
    assert!(move_scope_reference_loss(&scope, &agreed)
        .unwrap()
        .contains("[901, 902]"));
    let other = [move_owner(900, 0, std::f64::consts::FRAC_PI_6)];
    assert!(move_scope_reference_loss(&scope, &other)
        .unwrap()
        .contains("[900, 901, 902]"));
}

/// A bounded-face member of group 100 whose candidate lanes resolve face 27.
pub(super) fn resolved_member_operand() -> crate::records::topology::face::DesignFaceOperand {
    crate::records::topology::face::DesignFaceOperand::try_new(
        crate::records::topology::face::DesignFaceOperandDraft {
            id: "f3d:test:face-operand#200".into(),
            scope_record_index: 12,
            scope_reference_ordinal: 0,
            group: Some(crate::records::topology::body_recipe::DesignOperandGroup {
                group_record_index: 100,
                group_member_ordinal: 0,
            }),
            record_index: 200,
            byte_offset: 1200,
            class_tag: "297".to_owned().try_into().unwrap(),
            paired_byte_offset: 1250,
            paired_class_tag: "259".to_owned().try_into().unwrap(),
            recipe_record_index: 203,
            recipe_record_byte_offset: 1300,
            recipe_id: "f3d:test:recipe#203".into(),
            recipe_prefix_offset: 1311,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_kind: crate::records::recipes::ConstructionRecipeKind::BoundedFace,
            recipe_program_offset: 1350,
            recipe_program: vec![0, -1],
            recipe_nodes: Vec::new(),
            candidate_faces: Vec::new(),
            unreferenced_candidate_faces: Vec::new(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: vec![27],
            resolved_active_face: None,
            next_record_index: 220,
            next_byte_offset: 1411,
        },
    )
    .unwrap()
}

#[test]
fn offset_faces_keeps_its_group_when_a_member_resolves_a_face() {
    let scope = scope(DesignFeatureKind::OffsetFaces);
    let group = group(DesignOperandRole::ROLE_0X10);
    let operand = resolved_member_operand();
    let FeatureOperation::MoveFace { faces, .. } = projected_operation(
        crate::design::feature_project::direct_face::project_offset_faces(
            &scope,
            &[],
            std::slice::from_ref(&operand),
            std::slice::from_ref(&group),
        ),
    ) else {
        panic!("expected a face move");
    };
    assert_eq!(faces, FaceSelection::Native(group.id.clone()));
}

#[test]
fn shell_keeps_its_group_when_a_member_resolves_a_face() {
    let scope = scope(DesignFeatureKind::Shell);
    let group = group(DesignOperandRole::ROLE_0X10);
    let FeatureOperation::Shell { removed_faces, .. } =
        projected_operation(crate::design::feature_project::direct_face::project_shell(
            &scope,
            &[resolved_member_operand()],
            std::slice::from_ref(&group),
        ))
    else {
        panic!("expected a shell");
    };
    assert_eq!(removed_faces, FaceSelection::Native(group.id.clone()));
}

#[test]
fn shell_projects_preceding_faces_with_empty_active_candidates() {
    let mut scope = scope(DesignFeatureKind::Shell);
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(6);
            draft.previous_history_state_id = Some(5);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group = group(DesignOperandRole::ROLE_0X10);
    let operand = resolved_member_operand();
    assert!(operand.candidate_faces.is_empty());
    let FeatureOperation::Shell { removed_faces, .. } =
        projected_operation(crate::design::feature_project::direct_face::project_shell(
            &scope,
            &[operand],
            std::slice::from_ref(&group),
        ))
    else {
        panic!("expected a shell");
    };
    let feature = crate::ids::neutral_feature_id(&scope);
    let prefix = crate::ids::feature_input_prefix(&feature, 5);
    let expected = FaceSelection::historical(
        crate::ids::feature_input_topology_id(&feature, 5),
        vec![crate::ids::history_input_face_id(&prefix, 27)],
        group.id,
    )
    .unwrap();
    assert_eq!(removed_faces, expected);
}

#[test]
fn shell_keeps_a_group_with_an_unresolved_historical_member() {
    let mut scope = scope(DesignFeatureKind::Shell);
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(6);
            draft.previous_history_state_id = Some(5);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group = group(DesignOperandRole::ROLE_0X10);
    let mut operand = resolved_member_operand();
    operand.resolved_face_slots.clear();
    let FeatureOperation::Shell { removed_faces, .. } =
        projected_operation(crate::design::feature_project::direct_face::project_shell(
            &scope,
            &[operand],
            std::slice::from_ref(&group),
        ))
    else {
        panic!("expected a shell");
    };
    assert_eq!(removed_faces, FaceSelection::Native(group.id));
}
