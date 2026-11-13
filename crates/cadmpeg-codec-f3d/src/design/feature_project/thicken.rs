// SPDX-License-Identifier: Apache-2.0

use super::{direct_face_selection, native_stream};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_ir::features::{
    BodySelection, BooleanKind, FaceSelection, FeatureDefinition, FeatureOperation,
    SolidFeatureResult, ThickenSide,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveLength;

pub(crate) fn project_thicken(
    scope: &DesignParameterScope,
    operands: &[DesignFaceOperand],
    groups: &[DesignConstructionOperandGroup],
    losses: &mut Vec<LossNote>,
) -> Option<FeatureDefinition> {
    let DesignScopePayload::Thicken(operation) = scope.payload() else {
        return None;
    };
    let selected = |index| {
        operation.as_ref().is_none_or(|operation| {
            operation.group_record_indices.is_empty()
                || operation.group_record_indices.contains(&index)
        })
    };
    let candidates = || {
        groups.iter().filter(|group| {
            native_stream(&group.id) == native_stream(&scope.id)
                && group.scope_record_index == scope.record_index
                && selected(group.record_index)
                && matches!(
                    group.role(),
                    DesignOperandRole::ROLE_0X5 | DesignOperandRole::ROLE_0X12
                )
                && !group.members().is_empty()
        })
    };
    let complete = operation.as_ref().is_none_or(|operation| {
        operation.group_record_indices.iter().all(|index| {
            candidates()
                .filter(|group| group.record_index == *index)
                .count()
                == 1
        })
    });
    let mut faces = direct_face_selection(
        scope,
        operands.iter().filter(|operand| {
            operation.as_ref().is_none_or(|operation| {
                operation.group_record_indices.is_empty()
                    || operand.group_record_index().is_none_or(selected)
            })
        }),
    )
    .unwrap_or_else(|| {
        let mut groups = candidates();
        match (groups.next(), groups.next()) {
            (Some(group), None) if complete => FaceSelection::Native(group.id.clone()),
            _ => FaceSelection::Native(scope.id.clone()),
        }
    });
    list_thicken_whole_bodies(&mut faces, scope, groups);
    if !complete || faces == FaceSelection::Native(scope.id.clone()) {
        losses.push(F3dLossCode::ThickenFaceSelectionAssumed.note(format!(
            "Thicken {} retains its scope selection where the input groups do not resolve uniquely (DR-489).", scope.id
        )));
        if !complete {
            faces = match faces {
                FaceSelection::Historical {
                    state,
                    faces,
                    native,
                } => FaceSelection::historical_partial(
                    state,
                    faces.as_slice().to_vec(),
                    vec![scope.id.clone()],
                    native.to_string(),
                )
                .unwrap_or_else(|_| FaceSelection::Native(scope.id.clone())),
                FaceSelection::HistoricalPartial {
                    state,
                    faces,
                    unresolved,
                    native,
                } => {
                    let mut unresolved = unresolved.as_slice().to_vec();
                    if !unresolved.contains(&scope.id) {
                        unresolved.push(scope.id.clone());
                    }
                    FaceSelection::historical_partial(
                        state,
                        faces.as_slice().to_vec(),
                        unresolved,
                        native.to_string(),
                    )
                    .unwrap_or_else(|_| FaceSelection::Native(scope.id.clone()))
                }
                _ => FaceSelection::Native(scope.id.clone()),
            };
        }
    }
    let Some(operation) = operation else {
        losses.push(F3dLossCode::ThickenFrameAssumed.note(format!(
            "Thicken {} projects its available face selection with unresolved construction settings because its prefix has no closed reading (DR-489).", scope.id
        )));
        return Some(FeatureDefinition::Operation(FeatureOperation::thicken(
            faces, None, None,
        )));
    };
    let signed = operation
        .signed_thickness
        .map(cadmpeg_ir::scalar::FiniteReal::get);
    let thickness = signed.and_then(|value| {
        PositiveLength::new(value.abs() * if operation.symmetric { 20.0 } else { 10.0 })
    });
    if signed.is_some() && thickness.is_none() {
        losses.push(F3dLossCode::ThickenThicknessUnrepresentable.note(format!(
            "Thicken {} retains its signed source thickness while its zero or overflowing millimetre magnitude stays unresolved.", scope.id
        )));
    }
    let side = if operation.symmetric {
        Some(ThickenSide::Both)
    } else {
        signed.and_then(|value| {
            if value > 0.0 {
                Some(ThickenSide::Forward)
            } else if value < 0.0 {
                Some(ThickenSide::Reverse)
            } else {
                None
            }
        })
    };
    let result = match operation.boolean {
        None => SolidFeatureResult::NewBody {},
        Some(boolean) => {
            use crate::records::feature::direct_face::DesignThickenBooleanOperation;
            let mut targets = groups.iter().filter(|group| {
                native_stream(&group.id) == native_stream(&scope.id)
                    && group.scope_record_index == scope.record_index
                    && group.record_index == boolean.target_group_record_index
                    && group.role() == DesignOperandRole::BODIES_A
                    && !group.members().is_empty()
            });
            let targets = match (targets.next(), targets.next()) {
                (Some(group), None) => BodySelection::Native(group.id.clone()),
                _ => {
                    losses.push(F3dLossCode::ThickenTargetSelectionAssumed.note(format!(
                        "Thicken {} retains its Boolean target reference {} in the scope selection because the target group is missing or inconsistent (DR-489).", scope.id, boolean.target_group_record_index
                    )));
                    BodySelection::Native(scope.id.clone())
                }
            };
            SolidFeatureResult::Boolean {
                operation: match boolean.operation {
                    DesignThickenBooleanOperation::Join => BooleanKind::Join,
                    DesignThickenBooleanOperation::Cut => BooleanKind::Cut,
                    DesignThickenBooleanOperation::Intersect => BooleanKind::Intersect,
                },
                targets,
            }
        }
    };
    Some(FeatureDefinition::Operation(FeatureOperation::Thicken {
        faces,
        thickness,
        side,
        result: Some(result),
        tangent_chain: Some(operation.tangent_chain),
    }))
}

pub(crate) fn list_thicken_whole_bodies(
    selection: &mut FaceSelection,
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) {
    let (state, faces, mut unresolved, native) = match &*selection {
        FaceSelection::Historical {
            state,
            faces,
            native,
        } => (state, faces.as_slice(), Vec::new(), native),
        FaceSelection::HistoricalPartial {
            state,
            faces,
            unresolved,
            native,
        } => (
            state,
            faces.as_slice(),
            unresolved.as_slice().to_vec(),
            native,
        ),
        _ => return,
    };
    let selected = |index| match scope.payload() {
        DesignScopePayload::Thicken(Some(operation)) => {
            operation.group_record_indices.is_empty()
                || operation.group_record_indices.contains(&index)
        }
        _ => true,
    };
    let listed = unresolved.len();
    for group in groups.iter().filter(|group| {
        native_stream(&group.id) == native_stream(&scope.id)
            && group.scope_record_index == scope.record_index
            && selected(group.record_index)
            && group.role() == DesignOperandRole::ROLE_0X5
            && !group.members().is_empty()
    }) {
        if !unresolved.contains(&group.id) {
            unresolved.push(group.id.clone());
        }
    }
    if unresolved.len() != listed {
        *selection = FaceSelection::historical_partial(
            state.clone(),
            faces.to_vec(),
            unresolved,
            native.to_string(),
        )
        .unwrap_or_else(|_| FaceSelection::Native(scope.id.clone()));
    }
}
