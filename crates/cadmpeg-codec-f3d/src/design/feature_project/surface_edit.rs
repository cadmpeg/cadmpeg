// SPDX-License-Identifier: Apache-2.0
//! Surface trim, stitch, ruled surface and boundary fill projection.

use crate::design::edge_resolve::resolved_edge_group;
use crate::design::face_resolve::design_angle;
use crate::design::face_resolve::resolved_body_recipe_selection;
use crate::design::feature_project::design_positive_length;
use crate::design::feature_project::merge_edge_selections;

use crate::design::feature_project::unique_feature_match;
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;
use crate::records::topology::body_recipe::DesignBodyRecipeOperand;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::edge_identity::DesignEdgeIdentityOperand;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(in crate::design) fn project_surface_stitch(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::selections::FaceSelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let operation = scope.surface_stitch_operation()?;
    let input_end = scope.reference_members().len().checked_sub(2)?;
    let mut matching = Vec::new();
    for group in groups.iter().filter(|group| {
        native_stream(&group.id) == native_stream(&scope.id)
            && group.scope_record_index == scope.record_index
    }) {
        matching.push(group);
    }
    matching.sort_by(|left, right| {
        left.scope_reference_ordinal
            .cmp(&right.scope_reference_ordinal)
    });
    if matching.len().checked_mul(2)? != input_end
        || matching
            .iter()
            .enumerate()
            .zip(
                scope
                    .reference_members()
                    .values()
                    .step_by(2)
                    .zip(scope.reference_members().values().skip(1).step_by(2)),
            )
            .any(|((ordinal, group), (group_reference, member_reference))| {
                u32::try_from(ordinal * 2) != Ok(group.scope_reference_ordinal)
                    || group.record_index != *group_reference
                    || !group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .eq([*member_reference])
                    || group.role() != DesignOperandRole::ROLE_0X5
            })
    {
        return None;
    }
    Some(FeatureDefinition::Operation(
        FeatureOperation::KnitSurface {
            faces: FaceSelection::Native(scope.id.clone()),
            merge_entities: Some(true),
            create_solid: Some(true),
            gap_tolerance: Some(cadmpeg_ir::scalar::NonNegativeLength::new(
                operation.gap_tolerance.get() * 10.0,
            )?),
        },
    ))
}

pub(super) fn project_ruled_surface(
    scope: &DesignParameterScope,
    owners: &[crate::records::parameters::DesignParameterOwner],
    parameters: &[DesignParameter],
    groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use crate::records::feature::surface_ops::{
        DesignRuledSurfaceCorner, DesignRuledSurfaceMethod,
    };
    use cadmpeg_ir::features::selections::FaceSelection;
    use cadmpeg_ir::features::surface_inputs::{RuledSurfaceCorner, RuledSurfaceMode};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let operation = or_none!(scope.ruled_surface_operation());
    let stream = or_none!(native_stream(&scope.id));
    let parameter = |owner_record_index, source_kind: &str| {
        let mut matching = owners.iter().filter(|owner| {
            native_stream(owner.id()) == Some(stream)
                && owner.scope_record_index() == scope.record_index
                && owner.record_index() == owner_record_index
        });
        let owner = matching.next()?;
        if matching.next().is_some() {
            return None;
        }
        parameters.iter().find(|parameter| {
            native_stream(&parameter.id) == Some(stream)
                && parameter.record_index == owner.parameter_record_index()
                && parameter.source_kind() == source_kind
        })
    };
    let distance = or_none!(design_positive_length(or_none!(parameter(
        operation.distance_owner_record_index,
        "ruledDistance",
    ))));
    let angle = or_none!(design_angle(or_none!(parameter(
        operation.angle_owner_record_index,
        "ruledAngle"
    ))));
    let mode = match operation.method {
        DesignRuledSurfaceMethod::Tangent => RuledSurfaceMode::Tangent { distance },
        DesignRuledSurfaceMethod::Normal => RuledSurfaceMode::Normal { distance },
        DesignRuledSurfaceMethod::Direction => return Ok(None),
    };
    let mut ordered_groups = Vec::new();
    for record_index in &operation.edge_group_record_indices {
        let mut matching = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == *record_index
        });
        let group = or_none!(matching.next());
        if matching.next().is_some()
            || group.role() != DesignOperandRole::BODIES_B
            || group.members().len() != 1
        {
            return Ok(None);
        }
        let reference_ordinal = or_none!(usize::try_from(group.scope_reference_ordinal).ok());
        if scope.reference_members().values().nth(reference_ordinal) != Some(record_index)
            || scope
                .reference_members()
                .values()
                .nth(reference_ordinal + 1)
                != group.members().first().map(|member| &member.value)
        {
            return Ok(None);
        }
        ordered_groups.push(group);
    }
    let mut selections = Vec::new();
    for group in &ordered_groups {
        let selection = resolved_edge_group(
            group,
            groups,
            edge_operands,
            edge_identity_operands,
            scope.previous_history_state_id(),
            &crate::ids::neutral_feature_id(scope),
            ctx,
        )?;
        selections.push(selection);
    }
    let edges = merge_edge_selections(scope, &selections);
    let support_native = scope.id.clone();
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::RuledSurface {
            edges,
            support_faces: FaceSelection::Native(support_native),
            mode,
            angle: Some(angle),
            alternate_face: Some(operation.alternate_face),
            corner: Some(match operation.corner {
                DesignRuledSurfaceCorner::Rounded => RuledSurfaceCorner::Rounded,
                DesignRuledSurfaceCorner::Mitered => RuledSurfaceCorner::Mitered,
            }),
        },
    )))
}

pub(super) fn project_surface_trim(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    body_recipe_operands: &[DesignBodyRecipeOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::profiles::PathRef;
    use cadmpeg_ir::features::selections::FaceSelection;
    use cadmpeg_ir::features::surface_inputs::TrimRegion;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SurfaceTrim {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let references: Vec<u32> = scope.reference_members().values().copied().collect();
    let mut groups = construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    });
    let (Some(first), Some(second), None) = (groups.next(), groups.next(), groups.next()) else {
        return None;
    };
    let (target_group, tool_group) =
        if first.scope_reference_ordinal <= second.scope_reference_ordinal {
            (first, second)
        } else {
            (second, first)
        };
    let tool_ordinal = usize::try_from(tool_group.scope_reference_ordinal).ok()?;
    let (Some(target_members), Some(tool_members)) = (
        references.get(1..tool_ordinal),
        references.get(tool_ordinal.checked_add(1)?..),
    ) else {
        return None;
    };
    if target_group.scope_reference_ordinal != 0
        || target_members.is_empty()
        || tool_members.is_empty()
        || references.first() != Some(&target_group.record_index)
        || target_group.role() != DesignOperandRole::BODIES_A
        || !target_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq(target_members.iter().copied())
        || references.get(tool_ordinal) != Some(&tool_group.record_index)
        || !tool_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq(tool_members.iter().copied())
    {
        return None;
    }
    let body_recipe_member = |group: &DesignConstructionOperandGroup, ordinal, record_index| {
        body_recipe_operands.iter().any(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
                && operand.owner.group() == Some((group.record_index, ordinal))
                && operand.record_index() == record_index
        })
    };
    let entity_selection_member =
        |group: &DesignConstructionOperandGroup, ordinal, record_index| {
            entity_selection_operands.iter().any(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == scope.record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.record_index() == record_index
            })
        };
    let mut untyped_members = 0usize;
    for (ordinal, member) in target_group.members().iter().enumerate() {
        let ordinal = u32::try_from(ordinal).ok()?;
        if body_recipe_member(target_group, ordinal, member.value) {
            continue;
        }
        if entity_selection_member(target_group, ordinal, member.value) {
            return None;
        }
        untyped_members += 1;
    }
    match tool_group.role() {
        DesignOperandRole::ROLE_0X9 => {
            let [record_index] = tool_members else {
                return None;
            };
            unique_feature_match(face_operands.iter().filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == scope.record_index
                    && operand.scope_reference_ordinal == tool_group.scope_reference_ordinal
                    && operand.group_record_index() == Some(tool_group.record_index)
                    && operand.group_member_ordinal() == Some(0)
                    && operand.record_index() == *record_index
            }))
            .filter(|operand| operand.is_terminal_face_recipe())?;
        }
        DesignOperandRole::ROLE_0X21 => {
            for (ordinal, member) in tool_group.members().iter().enumerate() {
                let ordinal = u32::try_from(ordinal).ok()?;
                if !entity_selection_member(tool_group, ordinal, member.value)
                    && !body_recipe_member(tool_group, ordinal, member.value)
                {
                    untyped_members += 1;
                }
            }
        }
        _ => return None,
    }
    let faces = resolved_body_recipe_selection(scope, target_group, body_recipe_operands)
        .unwrap_or_else(|| FaceSelection::Native(target_group.id.clone()));
    if untyped_members != 0 {
        losses.push(crate::loss::F3dLossCode::FeatureTrimMemberTypeAssumed.note(format!(
            "SurfaceTrim {} projects {untyped_members} group member(s) that have no decoded operand record as target bodies or trimming tools by the role of their group.",
            scope.id
        )));
    }
    Some(FeatureDefinition::Operation(
        FeatureOperation::TrimSurface {
            faces,
            tool: PathRef::Native(tool_group.id.clone()),
            keep: TrimRegion::Unresolved,
        },
    ))
}
