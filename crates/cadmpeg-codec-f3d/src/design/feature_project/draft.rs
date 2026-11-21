use super::{
    draft_outward, draft_pull_vector, draft_sides, group_has_entity_selection,
    project_draft_face_selection, scope_stream_has_one_component,
    selected_historical_face_selection, selected_origin_plane, selected_work_plane,
    single_operand_group, ProjectInputs,
};
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::direct_face::{
    DesignDraftFlags, DesignDraftOperation, DesignDraftParting,
};
use crate::records::{
    feature::scope::DesignParameterScope,
    topology::{
        construction::DesignConstructionOperandGroup, extrude_selection::DesignOperandRole,
    },
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DraftAnchor, DraftPull, FaceSelection, FeatureDefinition, FeatureOperation,
};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::SlopeAngle;

pub(super) fn project(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<FeatureDefinition, CodecError> {
    if let Some(construction) = scope.draft_operation() {
        if let Some(flags) = construction.flags {
            if let Some(controls) = flags.parting {
                let mut groups = nonempty_anchor_groups(scope, inputs.construction_groups)
                    .map(|group| group.record_index);
                if matches!(
                    (groups.next(), groups.next(), groups.next()),
                    (Some(first), Some(second), None)
                        if [first, second] == [controls.pull.value, controls.tool.value]
                            || [second, first] == [controls.pull.value, controls.tool.value]
                ) {
                    if let Some(definition) = exact(scope, inputs)? {
                        losses.push(F3dLossCode::DraftPartingControlsPartial.note(format!(
                            "Draft scope {} uses the group roles and target types to read its two anchor groups as the legacy work-plane pull and face tool. The native construction retains the remaining parting controls (DR-812).", scope.id
                        )));
                        return Ok(definition);
                    }
                }
                return parting(scope, inputs, construction, flags, controls, losses);
            }
        }
    }
    if let Some(definition) = exact(scope, inputs)? {
        return Ok(definition);
    }
    let faces = single_operand_group(
        inputs.construction_groups,
        scope,
        DesignOperandRole::ROLE_0X10,
    )
    .filter(|group| scope_group(scope, group));
    let plane = single_operand_group(
        inputs.construction_groups,
        scope,
        DesignOperandRole::ROLE_0X21,
    )
    .filter(|group| scope_group(scope, group));
    let fixed_angle = scope
        .draft_operation()
        .map(|operation| operation.angle.get());
    let owner_angle = || {
        let stream = native_stream(&scope.id)?;
        let mut owners = inputs.owners.iter().filter(|owner| {
            native_stream(owner.id()) == Some(stream)
                && owner.scope_record_index() == scope.record_index
                && owner.local_ordinal() == 0
                && scope
                    .reference_members()
                    .values()
                    .any(|member| *member == owner.record_index())
        });
        let owner = owners.next()?;
        owners
            .next()
            .is_none()
            .then_some(owner.evaluated_value().get())
    };
    let raw_angle = fixed_angle.or_else(owner_angle);
    let angle = raw_angle.and_then(SlopeAngle::new);
    if raw_angle.is_some() && angle.is_none() {
        losses.push(F3dLossCode::DraftSlopeOutOfRange.note(format!(
            "Draft scope {} retains its signed angle in native data because it lies outside the neutral slope-angle domain.", scope.id
        )));
    }
    losses.push(F3dLossCode::DraftNeutralAnchorAssumed.note(format!(
        "Draft scope {} retains its face and anchor selectors as a partial neutral-plane Draft because its complete anchor construction is unresolved (DR-811).", scope.id
    )));
    Ok(FeatureDefinition::Operation(FeatureOperation::Draft {
        faces: face_selection(scope, faces, inputs)?,
        anchor: DraftAnchor::NeutralPlane {
            plane: FaceSelection::Native(
                plane.map_or_else(|| scope.id.clone(), |group| group.id.clone()),
            ),
            pull: None,
        },
        angle,
        outward: None,
        second_side: None,
    }))
}

fn nonempty_anchor_groups<'a>(
    scope: &'a DesignParameterScope,
    groups: &'a [DesignConstructionOperandGroup],
) -> impl Iterator<Item = &'a DesignConstructionOperandGroup> {
    groups.iter().filter(|group| {
        native_stream(&group.id) == native_stream(&scope.id)
            && group.scope_record_index == scope.record_index
            && group.role() == DesignOperandRole::ROLE_0X21
            && !group.members().is_empty()
    })
}

fn scope_group(scope: &DesignParameterScope, group: &DesignConstructionOperandGroup) -> bool {
    native_stream(&scope.id).is_some_and(|stream| native_stream(&group.id) == Some(stream))
        && group.scope_record_index == scope.record_index
        && scope
            .reference_members()
            .values()
            .any(|member| *member == group.record_index)
        && group.members().iter().all(|member| {
            scope
                .reference_members()
                .values()
                .any(|value| *value == member.value)
        })
}

fn face_selection(
    scope: &DesignParameterScope,
    group: Option<&DesignConstructionOperandGroup>,
    inputs: &ProjectInputs<'_>,
) -> Result<FaceSelection, CodecError> {
    group.map_or_else(
        || Ok(FaceSelection::Native(scope.id.clone())),
        |group| project_draft_face_selection(scope, group, inputs.face_operands, inputs.histories),
    )
}

fn parting(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    construction: &DesignDraftOperation,
    flags: DesignDraftFlags,
    parting: DesignDraftParting,
    losses: &mut Vec<LossNote>,
) -> Result<FeatureDefinition, CodecError> {
    let group = |record_index, role| {
        super::unique_feature_match(inputs.construction_groups.iter().filter(|group| {
            group.record_index == record_index
                && group.role() == role
                && !group.members().is_empty()
                && scope_group(scope, group)
        }))
    };
    let faces = group(parting.faces.value, DesignOperandRole::ROLE_0X10);
    let tool = group(parting.tool.value, DesignOperandRole::BODIES_B);
    let pull_group = group(parting.pull.value, DesignOperandRole::ROLE_0X21);
    let pull = pull_group.and_then(|group| {
        if let Some(plane) = selected_work_plane(
            scope,
            group,
            inputs.entity_selection_operands,
            inputs.scopes,
        ) {
            let transform = plane.work_plane_transform()?;
            let normal = Vector3::new(transform[0][2], transform[1][2], transform[2][2]);
            let direction = cadmpeg_ir::units::UnitVector3::normalized(draft_pull_vector(
                normal,
                flags.flipped,
            ))?;
            return Some(DraftPull::new(
                direction.into(),
                Some(crate::ids::neutral_feature_id(plane)),
            ));
        }
        let (ordinal, normal) = selected_origin_plane(
            scope,
            group,
            inputs.entity_selection_operands,
            inputs.design_types,
        )?;
        let direction =
            cadmpeg_ir::units::UnitVector3::normalized(draft_pull_vector(normal, flags.flipped))?;
        let mut pull = DraftPull::new(direction.into(), None);
        if scope_stream_has_one_component(scope, inputs.component_naming_spaces) {
            pull.frame = crate::history::selection::design_geometry_origin_plane_frame(ordinal);
        }
        Some(pull)
    });
    let anchor = if let Some(pull) = pull {
        DraftAnchor::PartingLine {
            tool: FaceSelection::Native(
                tool.map_or_else(|| scope.id.clone(), |group| group.id.clone()),
            ),
            pull,
        }
    } else {
        losses.push(F3dLossCode::DraftNeutralAnchorAssumed.note(format!(
            "Draft scope {} retains the parting pull selector as an unresolved neutral-plane anchor because its direction is unresolved (DR-811).", scope.id
        )));
        DraftAnchor::NeutralPlane {
            plane: FaceSelection::Native(
                pull_group.map_or_else(|| scope.id.clone(), |group| group.id.clone()),
            ),
            pull: None,
        }
    };
    losses.push(F3dLossCode::DraftPartingControlsPartial.note(format!(
        "Draft scope {} reads anchor kind {} as a parting-line construction and retains mode {}, boundary flag {}, auxiliary scalars, both signed angles and fixed-edge selectors in its native construction; the neutral Draft carries its first angle and resolved pull (DR-812).", scope.id, parting.form, parting.mode, parting.boundary_flag
    )));
    let angle = SlopeAngle::new(construction.angle.get());
    if angle.is_none() {
        losses.push(F3dLossCode::DraftSlopeOutOfRange.note(format!(
            "Draft scope {} retains signed angle {} in its native construction because it lies outside the neutral slope-angle domain.", scope.id, construction.angle.get()
        )));
    }
    Ok(FeatureDefinition::Operation(FeatureOperation::Draft {
        faces: face_selection(scope, faces, inputs)?,
        anchor,
        angle,
        outward: angle.map(|angle| draft_outward(angle.get())),
        second_side: None,
    }))
}

pub(super) fn exact(
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
    let role_groups: Vec<_> = nonempty_anchor_groups(scope, groups).collect();
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
