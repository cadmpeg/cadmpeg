// SPDX-License-Identifier: Apache-2.0
//! `SurfaceUntrim` scope projection.

use crate::design::face_resolve::resolved_historical_face_group;
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::DesignSurfaceUntrimLoops;
use crate::records::topology::{
    construction::DesignConstructionOperandGroup, face::DesignFaceOperand,
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
    groups.iter().find(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && group.record_index == record_index
    })
}

pub(super) fn project_surface_untrim(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    losses: &mut Vec<LossNote>,
) -> FeatureDefinition {
    let Some(operation) = scope.surface_untrim_operation() else {
        losses.push(F3dLossCode::SurfaceUntrimFrameAssumed.note(format!(
            "SurfaceUntrim {} assumes its frame untrims the faces of its ordered references; the loops and extension are unresolved.",
            scope.id
        )));
        return FeatureDefinition::Operation(FeatureOperation::UntrimSurface {
            faces: FaceSelection::Native(scope.id.clone()),
            loops: UntrimLoops::Unresolved,
            extension: None,
        });
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
        .unwrap_or_else(|| FaceSelection::Native(group.id.clone())),
        None => {
            losses.push(F3dLossCode::SurfaceUntrimGroupSelectionAssumed.note(format!(
                "SurfaceUntrim {} retains its ordered references as the face selection because the definition of face group record {} is missing.",
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
                losses.push(F3dLossCode::SurfaceUntrimGroupSelectionAssumed.note(format!(
                    "SurfaceUntrim {} leaves its selected loops unresolved because it lacks one defined loop group.",
                    scope.id
                )));
                EdgeSelection::Unresolved
            }),
        },
    };
    if matches!(
        operation.loops,
        DesignSurfaceUntrimLoops::Internal | DesignSurfaceUntrimLoops::External
    ) || (operation.loops != DesignSurfaceUntrimLoops::Manual
        && !operation.loop_group_record_indices.is_empty())
    {
        losses.push(F3dLossCode::SurfaceUntrimLoopTypeAssumed.note(format!(
            "SurfaceUntrim {} reads loop type {:?} with {} stored loop group(s) as the loops removed from every selected face.",
            scope.id,
            operation.loops,
            operation.loop_group_record_indices.len()
        )));
    }
    FeatureDefinition::Operation(FeatureOperation::UntrimSurface {
        faces,
        loops,
        extension: cadmpeg_ir::scalar::NonNegativeLength::new(operation.extension.get() * 10.0),
    })
}
