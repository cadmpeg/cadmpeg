// SPDX-License-Identifier: Apache-2.0
//! Surface trim, stitch, ruled surface and boundary fill projection.

use super::{design_positive_length, merge_edge_selections};
use crate::design::edge_resolve::resolved_edge_group;
use crate::design::face_resolve::{design_angle, resolved_body_recipe_selection};
use crate::ids::native_stream;
use crate::records::{
    feature::scope::DesignParameterScope,
    parameters::DesignParameter,
    topology::{
        body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
        edge_identity::DesignEdgeIdentityOperand, edge_identity::DesignEdgeOperand,
        extrude_selection::DesignOperandRole,
    },
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(in crate::design) fn project_surface_stitch(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

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
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, RuledSurfaceCorner, RuledSurfaceMode,
    };

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

pub(super) fn project_boundary_fill(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::BoundaryFill
        || scope.reference_members().len() < 5
    {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let mut groups = Vec::new();
    for group in construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    }) {
        groups.push(group);
    }
    groups.sort_by(|left, right| {
        left.scope_reference_ordinal
            .cmp(&right.scope_reference_ordinal)
    });
    let (tools, cells) = groups.split_first()?;
    if tools.scope_reference_ordinal != 0
        || tools.record_index != *scope.reference_members().values().next()?
        || tools.role() != DesignOperandRole::BODIES_A
        || cells.is_empty()
    {
        return None;
    }
    for (index, group) in groups.iter().enumerate() {
        let start = usize::try_from(group.scope_reference_ordinal).ok()?;
        let end = groups
            .get(index + 1)
            .and_then(|next| usize::try_from(next.scope_reference_ordinal).ok())
            .unwrap_or(scope.reference_members().len() - 1);
        if start >= end
            || group.record_index != *scope.reference_members().values().nth(start)?
            || !group.members().iter().map(|member| member.value).eq(scope
                .reference_members()
                .values_in(start + 1..end)?
                .copied())
            || (index > 0 && group.role() != DesignOperandRole::ROLE_0X5)
        {
            return None;
        }
    }
    let selected_cells: Vec<_> = cells
        .iter()
        .map(|cell| BodySelection::Native(cell.id.clone()))
        .collect();
    let selected_cells = selected_cells.try_into().ok()?;
    Some(FeatureDefinition::Operation(
        FeatureOperation::BoundaryFill {
            tools: BodySelection::Native(tools.id.clone()),
            cells: selected_cells,
        },
    ))
}

/// Project the source selections of a `SurfaceTrim` scope.
///
/// Fusion stores the surface targets as a role-`0x04` body-recipe group and
/// the trimming tools as a role-`0x21` group of entity-selection or
/// body-recipe operands. The cell table that selects the cells to remove is
/// decoded separately and bound to the projected operation after the source
/// selections have been resolved.
pub(super) fn project_surface_trim(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    body_recipe_operands: &[DesignBodyRecipeOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, PathRef, TrimRegion,
    };

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
        || tool_group.role() != DesignOperandRole::ROLE_0X21
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
    for (ordinal, member) in tool_group.members().iter().enumerate() {
        let ordinal = u32::try_from(ordinal).ok()?;
        if !entity_selection_member(tool_group, ordinal, member.value)
            && !body_recipe_member(tool_group, ordinal, member.value)
        {
            untyped_members += 1;
        }
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
