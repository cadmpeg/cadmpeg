// SPDX-License-Identifier: Apache-2.0
//! Combine, align, move, modify and remove-body projection.

use crate::design::feature_project::move_owners;

use crate::design::feature_project::matrix_axis_angle;
use crate::design::feature_project::single_operand_group;
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterOwner;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::math::Vector3;

pub(in crate::design) fn project_combine(
    scope: &DesignParameterScope,
    native_scope: &str,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use crate::records::feature::combine::DesignCombineDestination;
    use cadmpeg_ir::features::{
        BodySelection, CombineDestination, FeatureDefinition, FeatureOperation,
    };

    let operation = scope.combine_operation()?;
    let selection = |record_index| {
        format!(
            "{}{}{}",
            native_scope,
            ":design-record#",
            u64::from(record_index)
        )
    };
    let target = BodySelection::Native(selection(operation.target_record_index));
    let tools = if operation.tools.additional.is_empty() {
        BodySelection::Native(selection(operation.tools.first.record_index))
    } else {
        let mut selected = Vec::new();
        for tool in operation.tools.iter() {
            selected.push(selection(tool.record_index));
        }
        BodySelection::NativeSet(cadmpeg_ir::features::NativeSelections::try_from(selected).ok()?)
    };
    let operands = cadmpeg_ir::features::CombineOperands::new(target, tools).ok()?;
    Some(FeatureDefinition::Operation(FeatureOperation::Combine {
        operands,
        op: operation.operation,
        keep_tools: operation.keep_tools,

        destination: match operation.destination {
            DesignCombineDestination::TargetComponent => CombineDestination::TargetComponent,
            DesignCombineDestination::NewComponent { .. } => CombineDestination::NewComponent,
        },
    }))
}

pub(super) fn project_align(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    let envelope = scope.align_envelope()?;
    let operation = envelope.motion.as_ref()?;
    let stream = native_stream(&scope.id);
    let mut matching = groups.iter().filter(|group| {
        stream.is_some()
            && native_stream(&group.id) == stream
            && group.scope_record_index == scope.record_index
            && group.scope_reference_ordinal == 0
            && group.record_index == envelope.second_reference.value
            && group.role() == DesignOperandRole::BODIES_A
            && !group.members().is_empty()
    });
    let group = match (matching.next(), matching.next()) {
        (Some(group), None) => Some(group),
        _ => None,
    };
    let Some(definition) = move_body(
        operation,
        group.map_or(cadmpeg_ir::features::BodySelection::Unresolved, |group| {
            cadmpeg_ir::features::BodySelection::Native(group.id.clone())
        }),
    ) else {
        losses.push(crate::loss::F3dLossCode::FeatureAlignMotionUnprojected.note(format!(
            "Align scope {} has no finite orientation-preserving body motion in canonical millimetres; its constructor is retained (DR-120).", scope.id
        )));
        return None;
    };
    if group.is_none() {
        losses.push(crate::loss::F3dLossCode::AlignOperandsUnresolved.note(format!(
            "Align scope {} projects its rigid matrix with an unresolved body selection because it has no unique moved-body group (DR-120).", scope.id
        )));
    }
    losses.push(crate::loss::F3dLossCode::FeatureAlignMotionAssumed.note(format!(
        "Align scope {} applies the stored matrix of constructor {} to {}; its source code {}, constructor prefix fields and scope references {:?} are assumed to leave that motion unchanged (DR-120).",
        scope.id,
        operation.transform_record_index,
        group.map_or_else(
            || "an unresolved body selection".to_owned(),
            |group| format!("body group {}", group.record_index)
        ),
        envelope.source_code.value,
        scope.reference_members().values().collect::<Vec<_>>()
    )));
    if operation
        .prefix
        .as_ref()
        .is_some_and(|prefix| prefix.matrix_omitted)
    {
        losses.push(crate::loss::F3dLossCode::FeatureAlignIdentityAssumed.note(format!(
            "Align scope {} takes identity motion from the omitted-matrix marker of constructor {} (DR-120).", scope.id, operation.transform_record_index
        )));
    }
    Some(definition)
}

pub(super) fn project_move(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    owners: &[DesignParameterOwner],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::BodySelection;

    let operation = scope.move_operation()?;
    let group = single_operand_group(groups, scope, DesignOperandRole::BODIES_A)?;
    let definition = move_body(operation, BodySelection::Native(group.id.clone()))?;
    let parameters = move_owners::move_parameter_owners(scope, operation, owners);
    let classified = parameters
        .as_ref()
        .filter(|(_, agrees)| *agrees)
        .map_or(&[][..], |(indices, _)| indices.as_slice());
    let auxiliary = scope
        .reference_members()
        .values()
        .copied()
        .filter(|index| {
            *index != group.record_index
                && *index != operation.transform_record_index
                && !group.members().iter().any(|member| member.value == *index)
                && !classified.contains(index)
        })
        .collect::<Vec<_>>();
    if !auxiliary.is_empty() {
        let disagreement = match &parameters {
            Some((indices, false)) => {
                format!("; its parameter owners {indices:?} disagree with the stored matrix")
            }
            _ => String::new(),
        };
        losses.push(crate::loss::F3dLossCode::FeatureMoveScopeReferencesAssumed.note(format!(
            "Move scope {} applies its stored matrix to body group {}; auxiliary scope references {auxiliary:?} are assumed to leave that motion unchanged{disagreement} (DR-126).",
            scope.id, group.record_index
        )));
    }
    Some(definition)
}

pub(super) fn move_body(
    operation: &crate::records::feature::direct_face::DesignMoveOperation,
    bodies: cadmpeg_ir::features::BodySelection,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    if !operation.preserves_orientation() {
        return None;
    }
    Some(FeatureDefinition::Operation(FeatureOperation::MoveBody {
        bodies,
        translation: cadmpeg_ir::features::FiniteVector3::new(Vector3::new(
            operation.transform[0][3] * 10.0,
            operation.transform[1][3] * 10.0,
            operation.transform[2][3] * 10.0,
        ))?,
        rotation: matrix_axis_angle(operation.transform.as_ref()),
        copies: 0,
    }))
}

pub(in crate::design) fn project_modify(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::BodySelection;

    let operation = scope.modify_operation()?;
    let stream = native_stream(&scope.id)?;
    let body_group = |record_index: u32| {
        groups.iter().find(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == record_index
                && group.role() == DesignOperandRole::BODIES_A
                && group.members().len() == 1
        })
    };
    let selected = operation
        .body_group_record_indices
        .iter()
        .map(|record_index| body_group(*record_index).map(|group| group.id.clone()))
        .collect::<Option<Vec<_>>>()?;
    let bodies = match selected.as_slice() {
        [group] => BodySelection::Native(group.clone()),
        _ => BodySelection::NativeSet(
            cadmpeg_ir::features::NativeSelections::try_from(selected.clone()).ok()?,
        ),
    };
    let definition = move_body(&operation.motion, bodies)?;
    let references = scope
        .reference_members()
        .values()
        .copied()
        .collect::<Vec<_>>();
    let count = operation.body_group_record_indices.len();
    let unlisted = references
        .iter()
        .skip(1)
        .step_by(2)
        .take(count)
        .copied()
        .collect::<Vec<_>>();
    losses.push(
        crate::loss::F3dLossCode::FeatureModifyUnlistedGroupsAssumed.note(format!(
            "Modify scope {} moves the bodies of its listed body groups {:?}; its unlisted body groups {unlisted:?} are assumed to select the same bodies (DR-374).",
            scope.id, operation.body_group_record_indices
        )),
    );
    let table_breaks = (0..2 * count)
        .filter_map(|ordinal| {
            let group = *references.get(1 + 2 * ordinal)?;
            let member = references.get(2 + 2 * ordinal).copied();
            let holds = body_group(group).is_some_and(|group| {
                group.members().first().map(|located| located.value) == member
            });
            (!holds).then_some(group)
        })
        .collect::<Vec<_>>();
    if !table_breaks.is_empty() {
        losses.push(crate::loss::F3dLossCode::FeatureModifyTableAssumed.note(format!(
            "Modify scope {} stores ordered references {table_breaks:?} where its table holds single-member body groups, each followed by its member; the MoveBody assumes that they leave the moved bodies unchanged (DR-374).",
            scope.id
        )));
    }
    if let Some(state) = scope.history_state_id() {
        losses.push(crate::loss::F3dLossCode::FeatureModifyStateAssumed.note(format!(
            "Modify scope {} commits history state {state}; the MoveBody assumes that the state leaves the motion of the transform record unchanged (DR-374).",
            scope.id
        )));
    }
    Some(definition)
}

pub(in crate::design) fn project_remove_body(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        BodyRetentionMode, BodySelection, FeatureDefinition, FeatureOperation,
    };

    let group = single_operand_group(groups, scope, DesignOperandRole::BODIES_A)?;
    Some(FeatureDefinition::Operation(FeatureOperation::DeleteBody {
        bodies: BodySelection::Native(group.id.clone()),
        mode: BodyRetentionMode::DeleteSelected,
    }))
}
