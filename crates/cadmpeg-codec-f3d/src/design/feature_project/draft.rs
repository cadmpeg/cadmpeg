// SPDX-License-Identifier: Apache-2.0
//! Draft projection.

use super::{
    draft_outward, project_face_selection, selected_work_planes, single_operand_group,
    unique_feature_match, ProjectInputs,
};
use crate::ids::native_stream;
use crate::records::{
    feature::scope::DesignParameterScope,
    topology::{
        construction::DesignConstructionOperandGroup, extrude_selection::DesignOperandRole,
        face::DesignFaceOperand,
    },
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Vector3;

/// Neutral first angle and second side of a Draft scope. The first source
/// lane drafts the portion on the neutral-plane normal side, and the neutral
/// first side lies in the pull direction, so a flipped two-sided scope swaps
/// its lanes and negates both.
fn draft_sides(
    construction: &crate::records::feature::direct_face::DesignDraftOperation,
    flipped: bool,
) -> Option<(
    cadmpeg_ir::scalar::SlopeAngle,
    Option<cadmpeg_ir::features::DraftSecondSide>,
)> {
    use crate::records::feature::direct_face::DesignDraftTwoSided;
    use cadmpeg_ir::features::DraftSecondSide;
    use cadmpeg_ir::scalar::SlopeAngle;

    let first = construction.angle.get();
    let Some(two_sided) = construction.flags.and_then(|flags| flags.two_sided) else {
        return Some((SlopeAngle::new(first)?, None));
    };
    let second = construction.opposite_angle?.get();
    Some(match (two_sided, flipped) {
        (DesignDraftTwoSided::Symmetric, false) => {
            (SlopeAngle::new(first)?, Some(DraftSecondSide::Symmetric {}))
        }
        (DesignDraftTwoSided::Symmetric, true) => (
            SlopeAngle::new(0.0 - first)?,
            Some(DraftSecondSide::Symmetric {}),
        ),
        (DesignDraftTwoSided::TwoAngles, false) => (
            SlopeAngle::new(first)?,
            Some(DraftSecondSide::Angle {
                angle: SlopeAngle::new(second)?,
            }),
        ),
        (DesignDraftTwoSided::TwoAngles, true) => (
            SlopeAngle::new(0.0 - second)?,
            Some(DraftSecondSide::Angle {
                angle: SlopeAngle::new(0.0 - first)?,
            }),
        ),
    })
}

fn draft_pull_vector(normal: Vector3, flipped: bool) -> Vector3 {
    if flipped {
        Vector3::new(0.0 - normal.x, 0.0 - normal.y, 0.0 - normal.z)
    } else {
        normal
    }
}

/// Ordinal and normal of the Design Geometry origin plane that a group member names.
///
/// The type table identifies the origin plane. An ASM entity can share its
/// number, and such historical candidates describe unrelated topology.
fn selected_origin_plane(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    design_types: &[crate::records::entity_header::SegmentType],
) -> Option<(usize, Vector3)> {
    let stream = native_stream(&scope.id)?;
    let [crate::records::identity::Located { value: member, .. }] = group.members() else {
        return None;
    };
    let selection = unique_feature_match(entity_selection_operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.group_record_index == group.record_index
            && operand.group_member_ordinal == 0
            && operand.record_index() == *member
    }))?;
    if selection.secondary().is_some() {
        return None;
    }
    let crate::design::decode::meta::OriginClassification::Origin(
        crate::design::decode::meta::OriginEntity::Plane(ordinal),
    ) = crate::design::decode::meta::classify_origin_entity(
        design_types,
        stream,
        selection.primary_identity,
    )
    else {
        return None;
    };
    crate::history::selection::design_geometry_origin_plane(ordinal)
        .map(|plane| (ordinal, plane.normal))
}

/// Return whether a scope's Design stream binds exactly one component, so the
/// component's origin planes lie in the design's frame.
fn scope_stream_has_one_component(
    scope: &DesignParameterScope,
    component_naming_spaces: &[crate::records::recipes::DesignComponentNamingSpace],
) -> bool {
    let Some(stream) = native_stream(&scope.id) else {
        return false;
    };
    component_naming_spaces
        .iter()
        .filter(|space| native_stream(&space.id) == Some(stream))
        .take(2)
        .count()
        == 1
}

pub(super) fn project_draft(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let &ProjectInputs {
        scopes,
        construction_groups: groups,
        entity_selection_operands,
        face_operands,
        histories,
        design_types,
        component_naming_spaces,
        ..
    } = inputs;

    let construction = or_none!(scope.draft_operation());
    let flipped = construction.flags.is_some_and(|flags| flags.flipped);
    let (angle, second_side) = or_none!(draft_sides(construction, flipped));
    let angle_outward = draft_outward(angle.get());
    let faces = or_none!(single_operand_group(
        groups,
        scope,
        DesignOperandRole::ROLE_0X10
    ));
    let mut role_groups = Vec::new();
    for group in groups.iter().filter(|group| {
        native_stream(&group.id) == native_stream(&scope.id)
            && group.scope_record_index == scope.record_index
            && group.role() == DesignOperandRole::ROLE_0X21
            && !group.members().is_empty()
    }) {
        role_groups.push(group);
    }
    let member_of_scope = |group: &DesignConstructionOperandGroup| {
        group
            .members()
            .iter()
            .map(|member| &member.value)
            .all(|member| {
                scope
                    .reference_members()
                    .values()
                    .any(|value| value == member)
            })
    };
    if !scope
        .reference_members()
        .values()
        .any(|value| value == &faces.record_index)
        || !member_of_scope(faces)
    {
        return Ok(None);
    }
    Ok(match role_groups.as_slice() {
        [neutral_plane]
            if member_of_scope(neutral_plane)
                && group_has_entity_selection(scope, neutral_plane, entity_selection_operands) =>
        {
            if let Some(neutral_plane) =
                selected_work_plane(scope, neutral_plane, entity_selection_operands, scopes)
            {
                let transform = or_none!(neutral_plane.work_plane_transform());
                let pull_direction = or_none!(cadmpeg_ir::units::UnitVector3::normalized(
                    draft_pull_vector(
                        Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
                        flipped,
                    )
                ));
                return Ok(Some(FeatureDefinition::Operation(
                    FeatureOperation::Draft {
                        faces: project_draft_face_selection(
                            scope,
                            faces,
                            face_operands,
                            histories,
                        )?,
                        anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                            plane: cadmpeg_ir::features::FaceSelection::Native(
                                crate::ids::neutral_feature_id(neutral_plane).into_string(),
                            ),
                            pull: Some(cadmpeg_ir::features::DraftPull::new(
                                cadmpeg_ir::features::FeatureDirection3::from(pull_direction),
                                Some(crate::ids::neutral_feature_id(neutral_plane)),
                            )),
                        },
                        angle: Some(angle),
                        outward: Some(angle_outward),
                        second_side,
                    },
                )));
            }
            if let Some((ordinal, normal)) = selected_origin_plane(
                scope,
                neutral_plane,
                entity_selection_operands,
                design_types,
            ) {
                let direction = or_none!(cadmpeg_ir::units::UnitVector3::normalized(
                    draft_pull_vector(normal, flipped)
                ));
                let mut pull = cadmpeg_ir::features::DraftPull::new(
                    cadmpeg_ir::features::FeatureDirection3::from(direction),
                    None,
                );
                if scope_stream_has_one_component(scope, component_naming_spaces) {
                    pull.frame =
                        crate::history::selection::design_geometry_origin_plane_frame(ordinal);
                }
                if !pull.frame_matches_direction() {
                    pull.frame = None;
                }
                return Ok(Some(FeatureDefinition::Operation(
                    FeatureOperation::Draft {
                        faces: project_draft_face_selection(
                            scope,
                            faces,
                            face_operands,
                            histories,
                        )?,
                        anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                            plane: cadmpeg_ir::features::FaceSelection::Native(
                                neutral_plane.id.clone(),
                            ),
                            pull: Some(pull),
                        },
                        angle: Some(angle),
                        outward: Some(angle_outward),
                        second_side,
                    },
                )));
            }
            if second_side.is_some() {
                return Ok(None);
            }
            let neutral_plane = selected_historical_face_selection(
                scope,
                neutral_plane,
                entity_selection_operands,
                histories,
                design_types,
            );
            let neutral_plane = or_none!(neutral_plane);
            Some(FeatureDefinition::Operation(FeatureOperation::Draft {
                faces: project_draft_face_selection(scope, faces, face_operands, histories)?,
                anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                    plane: neutral_plane,
                    pull: None,
                },
                angle: Some(angle),
                outward: Some(angle_outward != flipped),
                second_side: None,
            }))
        }
        [neutral_plane] if member_of_scope(neutral_plane) && second_side.is_none() => {
            Some(FeatureDefinition::Operation(FeatureOperation::Draft {
                faces: project_draft_face_selection(scope, faces, face_operands, histories)?,
                anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                    plane: project_draft_face_selection(
                        scope,
                        neutral_plane,
                        face_operands,
                        histories,
                    )?,
                    pull: None,
                },
                angle: Some(angle),
                outward: Some(angle_outward != flipped),
                second_side: None,
            }))
        }
        [first, second]
            if member_of_scope(first) && member_of_scope(second) && second_side.is_none() =>
        {
            let first_plane = selected_work_plane(scope, first, entity_selection_operands, scopes);
            let second_plane =
                selected_work_plane(scope, second, entity_selection_operands, scopes);
            let (parting_tool, pull_plane) = match (first_plane, second_plane) {
                (Some(plane), None)
                    if !group_has_entity_selection(scope, second, entity_selection_operands) =>
                {
                    (second, plane)
                }
                (None, Some(plane))
                    if !group_has_entity_selection(scope, first, entity_selection_operands) =>
                {
                    (first, plane)
                }
                _ => return Ok(None),
            };
            let transform = or_none!(pull_plane.work_plane_transform());
            let pull_direction = or_none!(cadmpeg_ir::units::UnitVector3::normalized(
                draft_pull_vector(
                    Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
                    flipped,
                )
            ));
            Some(FeatureDefinition::Operation(FeatureOperation::Draft {
                faces: project_draft_face_selection(scope, faces, face_operands, histories)?,
                anchor: cadmpeg_ir::features::DraftAnchor::PartingLine {
                    tool: project_draft_face_selection(
                        scope,
                        parting_tool,
                        face_operands,
                        histories,
                    )?,
                    pull: cadmpeg_ir::features::DraftPull::new(
                        cadmpeg_ir::features::FeatureDirection3::from(pull_direction),
                        Some(crate::ids::neutral_feature_id(pull_plane)),
                    ),
                },
                angle: Some(angle),
                outward: Some(angle_outward),
                second_side: None,
            }))
        }
        _ => None,
    })
}

fn selected_historical_face_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    histories: &[crate::history_records::AsmHistory],
    design_types: &[crate::records::entity_header::SegmentType],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let previous_state_id =
        crate::history::effective_scope_previous_history_state_id(scope, histories)?;
    let stream = native_stream(&scope.id)?;
    let [crate::records::identity::Located { value: member, .. }] = group.members() else {
        return None;
    };
    let mut selections = entity_selection_operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.group_record_index == group.record_index
            && operand.group_member_ordinal == 0
            && operand.record_index() == *member
    });
    let selection = selections.next()?;
    if selections.next().is_some() || selection.secondary().is_some() {
        return None;
    }
    if crate::design::decode::meta::classify_origin_entity(
        design_types,
        stream,
        selection.primary_identity,
    ) != crate::design::decode::meta::OriginClassification::NotOrigin
    {
        return None;
    }
    let mut face_slots = selection
        .historical_face_candidates
        .iter()
        .filter(|candidate| candidate.historical.state_ids.contains(&previous_state_id))
        .map(|candidate| candidate.face_slot);
    let face_slot = face_slots.next()?;
    if face_slots.any(|candidate| candidate != face_slot) {
        return None;
    }
    let feature = crate::ids::neutral_feature_id(scope);
    let prefix = crate::ids::feature_input_prefix(&feature, previous_state_id);
    Some(
        match cadmpeg_ir::features::FaceSelection::historical(
            crate::ids::feature_input_topology_id(&feature, previous_state_id),
            vec![crate::ids::history_input_face_id(&prefix, face_slot)],
            group.id.clone(),
        ) {
            Ok(selection) => selection,
            Err(_) => cadmpeg_ir::features::FaceSelection::Native(group.id.clone()),
        },
    )
}

fn project_draft_face_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Result<cadmpeg_ir::features::FaceSelection, CodecError> {
    let selection = project_face_selection(scope, group, face_operands, histories)?;
    if matches!(&selection, cadmpeg_ir::features::FaceSelection::Native(_)) {
        Ok(
            crate::design::face_resolve::resolved_explicit_bounded_face_group(
                group,
                face_operands,
            )?
            .unwrap_or(selection),
        )
    } else {
        Ok(selection)
    }
}

fn group_has_entity_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> bool {
    let Some(stream) = native_stream(&scope.id) else {
        return false;
    };
    group
        .members()
        .iter()
        .map(|member| &member.value)
        .enumerate()
        .any(|(ordinal, record_index)| {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return false;
            };
            entity_selection_operands.iter().any(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == scope.record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.record_index() == *record_index
            })
        })
}

fn selected_work_plane<'a>(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &'a [DesignParameterScope],
) -> Option<&'a DesignParameterScope> {
    let planes = selected_work_planes(scope, group, entity_selection_operands, scopes);
    let planes = planes?;
    let [plane] = planes.as_slice() else {
        return None;
    };
    Some(*plane)
}
