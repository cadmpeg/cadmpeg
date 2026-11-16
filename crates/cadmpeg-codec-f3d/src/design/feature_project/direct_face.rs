// SPDX-License-Identifier: Apache-2.0
//! Replace, split, move and offset face projection, and shell.

use super::{
    design_length, direct_face_selection, face_scope_frame, matrix_axis_angle,
    project_face_selection, selected_work_planes, single_operand_group, unique_feature_match,
    DESIGN_LENGTH_TO_MILLIMETRE,
};
use crate::design::face_resolve::{
    resolved_body_recipe_selection, resolved_historical_face_group,
    resolved_historical_face_operand,
};
use crate::ids::native_stream;
use crate::records::{
    feature::scope::DesignParameterScope,
    parameters::DesignParameter,
    recipes::ConstructionRecipeKind,
    topology::{
        body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
        extrude_selection::DesignOperandRole, face::DesignFaceOperand,
    },
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Vector3;

const EPS_FEATURE_PROJECT_PROJECT_OFFSET_FACES_E9: f64 = 1.0e-9;

const EPS_MOVE_FACE_MOTION_ROUNDING: f64 = 32.0 * f64::EPSILON;

fn resolved_split_face_path(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Option<cadmpeg_ir::features::PathRef> {
    use cadmpeg_ir::features::PathRef;

    let previous_state_id =
        crate::history::effective_scope_previous_history_state_id(scope, histories)?;
    let stream = native_stream(&scope.id)?;
    let feature = crate::ids::neutral_feature_id(scope);
    let prefix = crate::ids::feature_input_prefix(&feature, previous_state_id);
    let mut edge_slots = Vec::new();
    for (ordinal, member) in group
        .members()
        .iter()
        .map(|member| &member.value)
        .enumerate()
    {
        let ordinal = u32::try_from(ordinal).ok()?;
        let mut selections = entity_selection_operands.iter().filter(|selection| {
            native_stream(&selection.id) == Some(stream)
                && selection.scope_record_index == scope.record_index
                && selection.group_record_index == group.record_index
                && selection.group_member_ordinal == ordinal
                && selection.record_index() == *member
        });
        let selection = selections.next()?;
        if selections.next().is_some() || selection.secondary().is_some() {
            return None;
        }
        let edge_slot = selection.resolved_edge_slot?;
        if edge_slots.contains(&edge_slot) {
            return None;
        }
        edge_slots.push(edge_slot);
    }
    let mut edges = Vec::new();
    for edge_slot in edge_slots {
        let edge = crate::ids::history_input_edge_id(&prefix, edge_slot);
        edges.push(edge);
    }
    PathRef::historical_edges(
        crate::ids::feature_input_topology_id(&feature, previous_state_id),
        edges,
        group.id.clone(),
    )
    .ok()
}

pub(in crate::design) fn project_offset_faces(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    operands: &[DesignFaceOperand],
    groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FaceMotion, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::scalar::Length;

    let parameter_distance = match parameters {
        [] => None,
        [(_, distance)] if distance.source_kind() == "distance" => {
            Some(or_none!(design_length(distance)))
        }
        _ => return Ok(None),
    };
    let fixed_distance = match &scope.payload() {
        crate::records::feature::scope::DesignScopePayload::OffsetFaces(value) => {
            match value.as_ref() {
                Some(value) => Some(or_none!(Length::new(value.distance.get() * 10.0))),
                None => None,
            }
        }
        _ => return Ok(None),
    };
    let distance = match (parameter_distance, fixed_distance) {
        (Some(parameter), Some(fixed))
            if (parameter.get() - fixed.get()).abs()
                <= EPS_FEATURE_PROJECT_PROJECT_OFFSET_FACES_E9 =>
        {
            parameter
        }
        (Some(distance), None) | (None, Some(distance)) => distance,
        _ => return Ok(None),
    };
    let faces =
        if let Some(group) = single_operand_group(groups, scope, DesignOperandRole::ROLE_0X10) {
            cadmpeg_ir::features::FaceSelection::Native(group.id.clone())
        } else {
            or_none!(direct_face_selection(scope, operands))
        };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::MoveFace {
            faces,
            motion: FaceMotion::Offset { distance },
        },
    )))
}

pub(in crate::design) fn project_shell(
    scope: &DesignParameterScope,
    operands: &[DesignFaceOperand],
    groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition, FeatureOperation};

    let crate::records::feature::scope::DesignScopePayload::Shell(Some(operation)) =
        &scope.payload()
    else {
        return Ok(None);
    };
    let inside = operation.thickness.get();
    let slots = operation.wall_slots.as_ref();
    let (wall, outward, mode) = match slots.and_then(|slots| slots.outside.as_ref()) {
        None => (Some(inside), Some(operation.outward), None),
        Some(outside) if outside.thickness.get().to_bits() == inside.to_bits() => (
            Some(inside + outside.thickness.get()),
            None,
            Some(cadmpeg_ir::features::ShellMode::BothSides),
        ),
        Some(_) => (None, None, Some(cadmpeg_ir::features::ShellMode::BothSides)),
    };
    let join = slots.map(|slots| {
        if slots.rounded_corners {
            cadmpeg_ir::features::ShellJoin::Arc
        } else {
            cadmpeg_ir::features::ShellJoin::Intersection
        }
    });
    let bodies = single_operand_group(groups, scope, DesignOperandRole::BODIES_A)
        .map(|group| BodySelection::Native(group.id.clone()));
    let removed_faces =
        if let Some(group) = single_operand_group(groups, scope, DesignOperandRole::ROLE_0X10) {
            FaceSelection::Native(group.id.clone())
        } else if bodies.is_some() {
            FaceSelection::Faces(Vec::new())
        } else if wall.is_none() {
            direct_face_selection(scope, operands).unwrap_or(FaceSelection::Unresolved)
        } else {
            or_none!(direct_face_selection(scope, operands))
        };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Shell {
            bodies,
            removed_faces,
            thickness: match wall {
                Some(wall) => Some(or_none!(cadmpeg_ir::scalar::PositiveLength::new(
                    wall * 10.0
                ))),
                None => None,
            },
            outward,
            mode,
            join,
            resolve_intersections: None,
            allow_self_intersections: None,
        },
    )))
}

pub(super) fn project_replace_face(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    body_recipe_operands: &[DesignBodyRecipeOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::ReplaceFace
        || scope.class_tag.as_str() != "301"
        || scope.paired_class_tag.as_str() != "258"
        || scope.frame_length() != 290
        || scope.reference_members().len() != 4
    {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let mut groups = construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    });
    let (Some(first), Some(second), None) = (groups.next(), groups.next(), groups.next()) else {
        return None;
    };
    let (replacement_group, target_group) =
        if first.scope_reference_ordinal <= second.scope_reference_ordinal {
            (first, second)
        } else {
            (second, first)
        };
    let references = scope.reference_members().values_array::<4>();
    let references = references?.map(|value| *value);
    if replacement_group.scope_reference_ordinal != 0
        || replacement_group.record_index != references[0]
        || replacement_group.role() != DesignOperandRole::ROLE_0X9
        || !replacement_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq(references[1..2].iter().copied())
        || target_group.scope_reference_ordinal != 2
        || target_group.record_index != references[2]
        || target_group.role() != DesignOperandRole::ROLE_0X10
        || !target_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq(references[3..4].iter().copied())
    {
        return None;
    }
    let replacements =
        resolved_body_recipe_selection(scope, replacement_group, body_recipe_operands)?;
    let targets = resolved_historical_face_group(
        scope,
        scope.previous_history_state_id(),
        target_group,
        face_operands,
    )?;
    Some(FeatureDefinition::Operation(
        FeatureOperation::ReplaceFace {
            operands: cadmpeg_ir::features::ReplaceFaceOperands::new(targets, replacements).ok()?,
        },
    ))
}

pub(in crate::design) fn project_split(
    scope: &DesignParameterScope,
    scopes: &[DesignParameterScope],
    construction_groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        BodySelection, FaceSelection, FeatureDefinition, FeatureOperation, SplitBodyTool,
    };

    let parsed = (|| {
        if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Split
            || scope.reference_members().len() < 4
        {
            return None;
        }
        let stream = native_stream(&scope.id)?;
        let mut groups = construction_groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        });
        let (Some(first), Some(second), None) = (groups.next(), groups.next(), groups.next())
        else {
            return None;
        };
        let (tool_group, targets) =
            if first.scope_reference_ordinal <= second.scope_reference_ordinal {
                (first, second)
            } else {
                (second, first)
            };
        let target_ordinal = tool_group.members().len().checked_add(1)?;
        let tool_members = scope.reference_members().values_in(1..target_ordinal)?;
        let target_record_index = *scope.reference_members().values().nth(target_ordinal)?;
        let target_members = scope
            .reference_members()
            .values_in(target_ordinal.checked_add(1)?..scope.reference_members().len())?;
        if tool_group.scope_reference_ordinal != 0
            || tool_group.record_index != *scope.reference_members().values().next()?
            || tool_group.members().is_empty()
            || !tool_group
                .members()
                .iter()
                .map(|member| member.value)
                .eq(tool_members.copied())
            || usize::try_from(targets.scope_reference_ordinal).ok()? != target_ordinal
            || targets.record_index != target_record_index
            || targets.role() != DesignOperandRole::BODIES_A
            || targets.members().is_empty()
            || !targets
                .members()
                .iter()
                .map(|member| member.value)
                .eq(target_members.copied())
        {
            return None;
        }
        Some((stream, tool_group, targets))
    })();
    let Some((stream, tool_group, targets)) = parsed else {
        return Ok(None);
    };
    let tool = match tool_group.role() {
        DesignOperandRole::ROLE_0X9 => {
            let [crate::records::identity::Located {
                value: tool_record_index,
                ..
            }] = tool_group.members()
            else {
                return Ok(None);
            };
            let tool = unique_feature_match(face_operands.iter().filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == scope.record_index
                    && operand.scope_reference_ordinal == 1
                    && operand.record_index() == *tool_record_index
                    && operand.recipe_kind == ConstructionRecipeKind::Face
                    && operand.recipe_program.as_slice() == [0, -1]
                    && operand.recipe_nodes.is_empty()
            }));
            let Some(tool) = tool else {
                return Ok(None);
            };
            let mut tools = if let Some(selection) = resolved_historical_face_operand(scope, tool) {
                selection
            } else {
                direct_face_selection(scope, face_operands)
                    .unwrap_or_else(|| FaceSelection::Native(String::new()))
            };
            match &mut tools {
                FaceSelection::Resolved { native, .. } | FaceSelection::Native(native) => {
                    native.clone_from(&tool.id);
                }
                FaceSelection::Historical { native, .. }
                | FaceSelection::HistoricalPartial { native, .. } => {
                    *native = or_none!(cadmpeg_core::text::NonBlankString::new(tool.id.clone()));
                }
                _ => {}
            }
            SplitBodyTool::Faces(tools)
        }
        DesignOperandRole::ROLE_0X21 => {
            match selected_work_planes(scope, tool_group, entity_selection_operands, scopes)
                .as_deref()
            {
                Some([plane]) => SplitBodyTool::Plane {
                    plane: crate::ids::neutral_feature_id(plane),
                },
                _ => SplitBodyTool::Faces(FaceSelection::Native(tool_group.id.clone())),
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SplitBody {
            targets: BodySelection::Native(targets.id.clone()),
            tool,
        },
    )))
}

pub(super) fn project_split_face(
    scope: &DesignParameterScope,
    scopes: &[DesignParameterScope],
    construction_groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, PathRef, SplitFaceTool,
    };

    let reference_count = scope.reference_members().len();
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SplitFace
        || reference_count < 4
    {
        return Ok(None);
    }
    let (count_offset, tail_length) = or_none!(face_scope_frame(scope));
    if !matches!(count_offset, 168 | 172 | 176) || !matches!(tail_length, 77 | 78 | 87) {
        return Ok(None);
    }
    let stream = or_none!(native_stream(&scope.id));
    let mut groups = construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    });
    let (Some(first), Some(second), None) = (groups.next(), groups.next(), groups.next()) else {
        return Ok(None);
    };
    let (tool, targets) = if first.scope_reference_ordinal <= second.scope_reference_ordinal {
        (first, second)
    } else {
        (second, first)
    };
    let target_ordinal = or_none!(tool.members().len().checked_add(1));
    if tool.scope_reference_ordinal != 0
        || tool.record_index != *or_none!(scope.reference_members().values().next())
        || tool.role() != DesignOperandRole::ROLE_0X21
        || tool.members().is_empty()
        || !tool
            .members()
            .iter()
            .map(|member| member.value)
            .eq(or_none!(scope.reference_members().values_in(1..target_ordinal)).copied())
        || or_none!(usize::try_from(targets.scope_reference_ordinal).ok()) != target_ordinal
        || targets.record_index != *or_none!(scope.reference_members().values().nth(target_ordinal))
        || targets.role() != DesignOperandRole::ROLE_0X10
        || targets.members().is_empty()
        || !targets.members().iter().map(|member| member.value).eq(scope
            .reference_members()
            .values()
            .skip(target_ordinal + 1)
            .copied())
    {
        return Ok(None);
    }
    let target_selection = project_face_selection(scope, targets, face_operands, histories)?;
    let tool = if let Some(path) =
        resolved_split_face_path(scope, tool, entity_selection_operands, histories)
    {
        SplitFaceTool::Path(path)
    } else if let Some(planes) =
        selected_work_planes(scope, tool, entity_selection_operands, scopes)
    {
        let mut selected = Vec::new();
        for plane in planes {
            let id = crate::ids::neutral_feature_id(plane);
            selected.push(id);
        }
        if selected.len() == 1 {
            SplitFaceTool::Plane {
                plane: or_none!(selected.pop()),
            }
        } else {
            SplitFaceTool::Planes {
                planes: or_none!(selected.try_into().ok()),
            }
        }
    } else {
        SplitFaceTool::Path(PathRef::Native(tool.id.clone()))
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SplitFace {
            targets: if matches!(target_selection, FaceSelection::Native(_)) {
                FaceSelection::Native(targets.id.clone())
            } else {
                target_selection
            },
            tool,
        },
    )))
}

pub(super) fn project_move_face(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

    let operation = or_none!(scope.move_operation());
    if !operation.preserves_orientation() {
        return Ok(None);
    }
    let motion = or_none!(move_face_motion(operation.transform.as_ref()));
    losses.push(crate::loss::F3dLossCode::FeatureMoveFaceMatrixAssumed.note(format!(
        "MoveFace scope {} reads transform record {} as a row-major input-to-output face motion in model coordinates with centimetre translation (DR-401).",
        scope.id, operation.transform_record_index
    )));
    let group_ordinal = usize::from(
        scope.reference_members().values().next() == Some(&operation.transform_record_index),
    );
    let members_start = group_ordinal + 1;
    let group = single_operand_group(construction_groups, scope, DesignOperandRole::ROLE_0X10)
        .filter(|group| {
            operation
                .face_group
                .as_ref()
                .is_some_and(|marked| marked.value == group.record_index)
                && usize::try_from(group.scope_reference_ordinal).ok() == Some(group_ordinal)
                && scope.reference_members().values().nth(group_ordinal)
                    == Some(&group.record_index)
                && group.members().iter().map(|member| member.value).eq(scope
                    .reference_members()
                    .values()
                    .skip(members_start)
                    .take(group.members().len())
                    .copied())
                && !group
                    .members()
                    .iter()
                    .any(|member| member.value == operation.transform_record_index)
                && scope
                    .reference_members()
                    .values()
                    .filter(|index| **index == operation.transform_record_index)
                    .count()
                    == 1
        });
    let faces = if let Some(group) = group {
        let members_end = members_start + group.members().len();
        let unclassified = scope
            .reference_members()
            .values()
            .copied()
            .enumerate()
            .filter(|(ordinal, index)| {
                *ordinal != group_ordinal
                    && !(members_start..members_end).contains(ordinal)
                    && *index != operation.transform_record_index
            })
            .map(|(_, index)| index)
            .collect::<Vec<_>>();
        if !unclassified.is_empty() {
            losses.push(crate::loss::F3dLossCode::FeatureMoveFaceScopeReferencesAssumed.note(format!(
                "MoveFace scope {} applies its stored matrix to face group {}; unclassified ordered references {unclassified:?} are assumed to leave that motion unchanged (DR-401).",
                scope.id, group.record_index
            )));
        }
        FaceSelection::Native(group.id.clone())
    } else {
        losses.push(crate::loss::F3dLossCode::FeatureMoveFaceSelectionUnresolved.note(format!(
            "MoveFace scope {} retains its decoded motion with a scope-native face selection; its ordered references {:?} do not bind one complete face group (DR-402).",
            scope.id, scope.reference_members()
        )));
        losses.push(crate::loss::F3dLossCode::FeatureMoveFaceScopeReferencesAssumed.note(format!(
            "MoveFace scope {} applies its matrix while the effects of unclassified ordered references {:?} are assumed to be evaluated into that matrix (DR-401).",
            scope.id, scope.reference_members()
        )));
        FaceSelection::Native(scope.id.clone())
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::MoveFace { faces, motion },
    )))
}

fn move_face_motion(transform: &[[f64; 4]; 4]) -> Option<cadmpeg_ir::features::FaceMotion> {
    use cadmpeg_ir::features::{FaceMotion, FeatureDirection3, FinitePoint3};

    let translation = Vector3::new(
        transform[0][3] * DESIGN_LENGTH_TO_MILLIMETRE,
        transform[1][3] * DESIGN_LENGTH_TO_MILLIMETRE,
        transform[2][3] * DESIGN_LENGTH_TO_MILLIMETRE,
    );
    if let Some(rotation) = matrix_axis_angle(transform) {
        let axis = rotation.direction.get();
        if translation.dot(axis) == 0.0 {
            let cotangent = (rotation.angle.get() * 0.5).tan().recip();
            let origin = (translation + axis.cross(translation).scale(cotangent)).scale(0.5);
            if let Some(axis_origin) =
                FinitePoint3::new(cadmpeg_ir::math::Point3::new(origin.x, origin.y, origin.z))
            {
                if move_face_rotation_preserves_matrix(transform, translation, &rotation, origin) {
                    return Some(FaceMotion::Rotate {
                        axis_origin,
                        axis_dir: rotation.direction,
                        angle: rotation.angle,
                    });
                }
            }
        }
    } else if (0..3).all(|row| {
        (0..3).all(|column| transform[row][column] == if row == column { 1.0 } else { 0.0 })
    }) {
        if let (Some(direction), Some(distance)) = (
            cadmpeg_ir::units::UnitVector3::normalized(translation),
            cadmpeg_ir::scalar::Length::new(translation.norm()),
        ) {
            let direction = FeatureDirection3::from(direction);
            let rebuilt = direction.get().scale(distance.get());
            if [
                (translation.x, rebuilt.x),
                (translation.y, rebuilt.y),
                (translation.z, rebuilt.z),
            ]
            .into_iter()
            .all(|(source, rebuilt)| move_face_component_preserved(source, rebuilt))
            {
                return Some(FaceMotion::Translate {
                    direction,
                    distance,
                });
            }
        }
    }
    let mut rows = [transform[0], transform[1], transform[2]];
    for (row, component) in rows
        .iter_mut()
        .zip([translation.x, translation.y, translation.z])
    {
        row[3] = component;
    }
    Some(FaceMotion::Transform {
        transform: cadmpeg_ir::transform::Transform::affine(rows)?,
    })
}

fn move_face_rotation_preserves_matrix(
    transform: &[[f64; 4]; 4],
    translation: Vector3,
    rotation: &cadmpeg_ir::features::AxisAngle,
    origin: Vector3,
) -> bool {
    let axis = rotation.direction.get();
    let coordinates = [axis.x, axis.y, axis.z];
    let (sine, cosine) = rotation.angle.get().sin_cos();
    let cross = [
        [0.0, -axis.z, axis.y],
        [axis.z, 0.0, -axis.x],
        [-axis.y, axis.x, 0.0],
    ];
    let basis = std::array::from_fn::<_, 3, _>(|row| {
        std::array::from_fn::<_, 3, _>(|column| {
            let diagonal = if row == column { cosine } else { 0.0 };
            diagonal
                + (1.0 - cosine) * coordinates[row] * coordinates[column]
                + sine * cross[row][column]
        })
    });
    let point = [origin.x, origin.y, origin.z];
    let target = [translation.x, translation.y, translation.z];
    (0..3).all(|row| {
        (0..3)
            .all(|column| move_face_component_preserved(transform[row][column], basis[row][column]))
            && move_face_component_preserved(
                target[row],
                point[row]
                    - (0..3)
                        .map(|column| basis[row][column] * point[column])
                        .sum::<f64>(),
            )
    })
}

fn move_face_component_preserved(source: f64, rebuilt: f64) -> bool {
    if source == 0.0 {
        rebuilt.abs() <= EPS_MOVE_FACE_MOTION_ROUNDING
    } else {
        rebuilt != 0.0 && (source - rebuilt).abs() <= EPS_MOVE_FACE_MOTION_ROUNDING * source.abs()
    }
}
