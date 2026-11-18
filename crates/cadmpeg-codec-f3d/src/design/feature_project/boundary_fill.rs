// SPDX-License-Identifier: Apache-2.0

use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::{
    construction::DesignConstructionOperandGroup, extrude_selection::DesignOperandRole,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::boundary_fill::BoundaryFillCell;
use cadmpeg_ir::features::{
    BodySelection, BooleanOp, FeatureDefinition, FeatureOperation, NonEmptyMembers,
    SelectionReference,
};
use cadmpeg_ir::report::loss::LossNote;

pub(super) fn project(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(stream) = native_stream(&scope.id) else {
        return Ok(None);
    };
    let mut groups: Vec<_> = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect();
    groups.sort_by_key(|group| group.scope_reference_ordinal);

    let prefix = scope.boundary_fill().and_then(|fill| fill.prefix.as_ref());
    let operation = prefix.map(|fill| match fill.operation_word {
        1 => BooleanOp::Join,
        2 => BooleanOp::Cut,
        3 => BooleanOp::Intersect,
        4 => BooleanOp::NewBody,
        _ => BooleanOp::Unresolved,
    });
    if operation == Some(BooleanOp::Unresolved) {
        losses.push(
            F3dLossCode::FeatureBoundaryFillOperationUnresolved.note(format!(
                "{} retains an unassigned Boundary Fill operation word.",
                scope.id
            )),
        );
    }
    let operands = prefix.and_then(|fill| fill.operands.as_ref());
    let (tools, cells) = if let Some((tools, cells)) = legacy_selections(scope, &groups) {
        (tools, cells)
    } else if let Some(operands) = operands.filter(|operands| !operands.cells.is_empty()) {
        let tools = super::unique_feature_match(groups.iter().copied().filter(|group| {
            group.scope_reference_ordinal == 0
                && group.role() == DesignOperandRole::BODIES_A
                && !group.members().is_empty()
                && group_matches_scope(scope, group, 1 + group.members().len())
        }))
        .map_or(BodySelection::Unresolved, |group| {
            BodySelection::Native(group.id.clone())
        });
        let mut cells = ctx.alloc_filled(
            operands.cells.len(),
            BoundaryFillCell::Recorded { record: None },
            "Boundary Fill cells",
        )?;
        for (cell, index) in cells.iter_mut().zip(&operands.cells) {
            *cell = BoundaryFillCell::Recorded {
                record: record_reference(stream, *index)?,
            };
        }
        losses.push(F3dLossCode::FeatureBoundaryFillCellsNative.note(format!(
            "{} retains its selected-cell records; their neutral cell boundaries are unresolved.",
            scope.id
        )));
        if matches!(tools, BodySelection::Unresolved) {
            losses.push(
                F3dLossCode::FeatureBoundaryFillToolsUnresolved
                    .note(format!("{} has no unique boundary-body group.", scope.id)),
            );
        }
        (tools, NonEmptyMembers::try_from(cells)?)
    } else {
        losses.push(
            F3dLossCode::FeatureBoundaryFillCellsUnresolved.note(format!(
                "{} has no admitted body-region partition or selected-cell list.",
                scope.id
            )),
        );
        return Ok(None);
    };

    let mut plane_tools = Vec::new();
    let mut remove_tools = None;
    if let Some(operands) = operands {
        plane_tools = ctx.alloc_filled(operands.plane_tools.len(), None, "Boundary Fill planes")?;
        for (tool, index) in plane_tools.iter_mut().zip(&operands.plane_tools) {
            *tool = record_reference(stream, *index)?;
        }
        remove_tools = operands
            .removal
            .as_ref()
            .map(|removal| removal.remove_tools);
        if !plane_tools.is_empty() {
            losses.push(
                F3dLossCode::FeatureBoundaryFillToolsUnresolved.note(format!(
                    "{} retains the native boundary-plane selectors.",
                    scope.id
                )),
            );
        }
    }
    if matches!(
        operation,
        Some(BooleanOp::Join | BooleanOp::Cut | BooleanOp::Intersect)
    ) {
        losses.push(
            F3dLossCode::FeatureBoundaryFillTargetsUnresolved.note(format!(
                "{} retains its Boolean operation with unresolved target bodies.",
                scope.id
            )),
        );
    }
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::BoundaryFill {
            tools,
            cells,
            plane_tools,
            operation,
            remove_tools,
        },
    )))
}

fn legacy_selections(
    scope: &DesignParameterScope,
    groups: &[&DesignConstructionOperandGroup],
) -> Option<(BodySelection, NonEmptyMembers<BoundaryFillCell>)> {
    if scope.reference_members().len() < 5 {
        return None;
    }
    let (tools, cells) = groups.split_first()?;
    if tools.scope_reference_ordinal != 0
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
            || !group_matches_scope(scope, group, end)
            || (index > 0 && group.role() != DesignOperandRole::ROLE_0X5)
        {
            return None;
        }
    }
    let cells = cells
        .iter()
        .map(|cell| BoundaryFillCell::BodyRegion(BodySelection::Native(cell.id.clone())))
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    Some((BodySelection::Native(tools.id.clone()), cells))
}

fn group_matches_scope(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    end: usize,
) -> bool {
    let Some(start) = usize::try_from(group.scope_reference_ordinal).ok() else {
        return false;
    };
    let Some(group_index) = scope.reference_members().values().nth(start) else {
        return false;
    };
    group.record_index == *group_index
        && scope
            .reference_members()
            .values_in(start + 1..end)
            .is_some_and(|refs| {
                group
                    .members()
                    .iter()
                    .map(|member| member.value)
                    .eq(refs.copied())
            })
}

fn record_reference(
    stream: &str,
    index: Option<std::num::NonZeroU32>,
) -> Result<Option<SelectionReference>, CodecError> {
    index
        .map(|index| SelectionReference::new(format!("{stream}:design-record#{index}")))
        .transpose()
        .map_err(|error| {
            CodecError::malformed(format_args!("invalid Boundary Fill record: {error}"))
        })
}
