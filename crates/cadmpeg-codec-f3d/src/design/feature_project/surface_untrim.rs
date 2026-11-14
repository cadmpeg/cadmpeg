// SPDX-License-Identifier: Apache-2.0
use crate::design::face_resolve::{resolved_body_recipe_selection, resolved_historical_face_group};
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::DesignSurfaceUntrimLoops;
use crate::records::topology::{
    body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
    face::DesignFaceOperand,
};
use cadmpeg_ir::features::{
    EdgeSelection, FaceSelection, FeatureDefinition, FeatureOperation, UntrimLoops,
};
use cadmpeg_ir::report::loss::LossNote;

fn scope_group<'a>(
    scope: &DesignParameterScope,
    groups: &'a [DesignConstructionOperandGroup],
    record_index: u32,
) -> Option<&'a DesignConstructionOperandGroup> {
    let stream = native_stream(&scope.id)?;
    let mut matching = groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && group.record_index == record_index
    });
    let (Some(group), None) = (matching.next(), matching.next()) else {
        return None;
    };
    (group.role() == crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X9
        && !group.members().is_empty())
    .then_some(group)
}

pub(super) fn project_surface_untrim(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    body_recipe_operands: &[DesignBodyRecipeOperand],
    losses: &mut Vec<LossNote>,
) -> FeatureDefinition {
    let Some(operation) = scope.surface_untrim_operation() else {
        losses.push(F3dLossCode::SurfaceUntrimFrameAssumed.note(format!(
            "SurfaceUntrim {} assumes its frame untrims the faces of its ordered references; the loops and extension are unresolved.",
            scope.id
        )));
        return FeatureDefinition::Operation(FeatureOperation::untrim_surface(
            FaceSelection::Native(scope.id.clone()),
        ));
    };
    let faces = match scope_group(
        scope,
        construction_groups,
        operation.face_group_record_index,
    ) {
        Some(group) => resolved_historical_face_group(
            scope,
            scope.previous_history_state_id(),
            group,
            face_operands,
        )
        .or_else(|| resolved_body_recipe_selection(scope, group, body_recipe_operands))
        .unwrap_or_else(|| FaceSelection::Native(group.id.clone())),
        None => {
            losses.push(F3dLossCode::SurfaceUntrimGroupSelectionAssumed.note(format!(
                "SurfaceUntrim {} retains its ordered references as the face selection because face group record {} is missing, ambiguous or inconsistent (DR-504).",
                scope.id, operation.face_group_record_index
            )));
            FaceSelection::Native(scope.id.clone())
        }
    };
    let loops = match operation.loops {
        DesignSurfaceUntrimLoops::All => UntrimLoops::All,
        DesignSurfaceUntrimLoops::Internal => UntrimLoops::Internal,
        DesignSurfaceUntrimLoops::External => UntrimLoops::External,
        DesignSurfaceUntrimLoops::Manual => UntrimLoops::Selected {
            edges: match operation.loop_group_record_indices.as_slice() {
                [loop_group] => scope_group(scope, construction_groups, *loop_group)
                    .map(|group| EdgeSelection::Native(group.id.clone())),
                _ => None,
            }
            .unwrap_or_else(|| {
                if operation.loop_group_record_indices.len() <= 1
                    || operation.loop_group_record_indices.iter().any(|index| {
                        scope_group(scope, construction_groups, *index).is_none()
                    })
                {
                    losses.push(F3dLossCode::SurfaceUntrimGroupSelectionAssumed.note(format!(
                        "SurfaceUntrim {} retains its ordered loop-group references as a scope selection because a group definition is missing, ambiguous or inconsistent (DR-504).", scope.id
                    )));
                }
                EdgeSelection::Native(scope.id.clone())
            }),
        },
    };
    if operation.loops == DesignSurfaceUntrimLoops::Manual
        && operation.loop_group_record_indices.len() > 1
    {
        losses.push(F3dLossCode::SurfaceUntrimLoopGroupsAggregatedAssumed.note(format!(
            "SurfaceUntrim {} treats its {} ordered loop groups as one scope-native manual-loop selection (DR-504).",
            scope.id, operation.loop_group_record_indices.len()
        )));
    }
    if operation.loops != DesignSurfaceUntrimLoops::Manual
        && !operation.loop_group_record_indices.is_empty()
    {
        losses.push(F3dLossCode::SurfaceUntrimStoredLoopGroupsAssumed.note(format!(
            "SurfaceUntrim {} applies loop choice {:?} while retaining {} stored manual-loop group(s) as inactive selections (DR-504).",
            scope.id,
            operation.loops,
            operation.loop_group_record_indices.len()
        )));
    }
    let extension = operation.extension.and_then(|value| {
        let extension = cadmpeg_ir::scalar::Length::new(value.get() * 10.0);
        if extension.is_none() {
            losses.push(F3dLossCode::SurfaceUntrimExtensionUnrepresentable.note(format!(
                "SurfaceUntrim {} retains its source extension because its millimetre conversion overflows.", scope.id
            )));
        } else if value.get() < 0.0 {
            losses.push(F3dLossCode::SurfaceUntrimNegativeExtensionAssumed.note(format!(
                "SurfaceUntrim {} reads its negative evaluated distance as signed exterior-boundary extension (DR-505).", scope.id
            )));
        }
        extension
    });
    FeatureDefinition::Operation(FeatureOperation::UntrimSurface {
        faces,
        loops,
        extension,
    })
}
