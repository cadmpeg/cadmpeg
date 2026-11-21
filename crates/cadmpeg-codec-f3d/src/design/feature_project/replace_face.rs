// SPDX-License-Identifier: Apache-2.0
use crate::design::face_resolve::{resolved_body_recipe_selection, resolved_historical_face_group};
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::{
    body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
    extrude_selection::DesignOperandRole, face::DesignFaceOperand,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::replace_face::ReplaceFaceOperands;
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;

fn selection(
    scope: &DesignParameterScope,
    indices: &[u32],
    replacement: bool,
    groups: &[DesignConstructionOperandGroup],
    faces: &[DesignFaceOperand],
    bodies: &[DesignBodyRecipeOperand],
    losses: &mut Vec<LossNote>,
) -> FaceSelection {
    if indices.len() > 1 {
        losses.push(F3dLossCode::ReplaceFaceGroupsAggregatedAssumed.note(format!(
            "ReplaceFace {} reads its ordered {} group references as one scope-native selection (DR-810).", scope.id, if replacement { "replacement" } else { "target" }
        )));
        return FaceSelection::Native(scope.id.clone());
    }
    let stream = native_stream(&scope.id);
    let matching = |group: &&DesignConstructionOperandGroup| {
        stream.is_some()
            && native_stream(&group.id) == stream
            && group.scope_record_index == scope.record_index
            && indices.contains(&group.record_index)
    };
    let mut candidates = groups.iter().filter(matching);
    if let (Some(group), None) = (candidates.next(), candidates.next()) {
        let role_ok = if replacement {
            matches!(
                group.role(),
                DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X21
            )
        } else {
            group.role() == DesignOperandRole::ROLE_0X10
        };
        if indices.len() == 1 && role_ok && !group.members().is_empty() {
            let resolved = resolved_historical_face_group(
                scope,
                scope.previous_history_state_id(),
                group,
                faces,
            )
            .or_else(|| resolved_body_recipe_selection(scope, group, bodies));
            if let Some(selection) = resolved {
                return selection;
            }
            if group.role() == DesignOperandRole::ROLE_0X21 {
                losses.push(F3dLossCode::ReplaceFaceSupportAssumed.note(format!(
                    "ReplaceFace {} retains group {} as replacement support with an unresolved face binding (DR-810).", scope.id, group.id
                )));
            }
            return FaceSelection::Native(group.id.clone());
        }
    }
    losses.push(F3dLossCode::ReplaceFaceSelectionAssumed.note(format!(
        "ReplaceFace {} retains its ordered {} group references as a scope selection because their definitions are missing, ambiguous or inconsistent (DR-810).",
        scope.id, if replacement { "replacement" } else { "target" }
    )));
    FaceSelection::Native(scope.id.clone())
}

pub(super) fn project_replace_face(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    faces: &[DesignFaceOperand],
    bodies: &[DesignBodyRecipeOperand],
    losses: &mut Vec<LossNote>,
) -> Result<FeatureDefinition, CodecError> {
    let (replacement_indices, target_indices, tangent_chain) = if let Some(operation) =
        scope.replace_face_operation()
    {
        if let Some(word) = operation.flagged_prefix_word {
            losses.push(F3dLossCode::ReplaceFaceFlaggedPrefixWordAssumed.note(format!(
                "ReplaceFace {} retains flagged prefix word {word} as auxiliary metadata while applying its selections (DR-810).", scope.id
            )));
        }
        losses.push(F3dLossCode::ReplaceFaceTangentChainAssumed.note(format!(
            "ReplaceFace {} reads its prefix Boolean as tangent-chain expansion of its target faces (DR-810).", scope.id
        )));
        if operation.trailing_identity.as_str() != "00000000-0000-0000-0000-000000000000" {
            losses.push(F3dLossCode::ReplaceFaceIdentityAssumed.note(format!(
                "ReplaceFace {} retains identifier {} as auxiliary metadata while applying its selections (DR-810).", scope.id, operation.trailing_identity.as_str()
            )));
        }
        (
            operation.replacement_group_record_indices.clone(),
            operation.target_group_record_indices.clone(),
            Some(operation.tangent_chain),
        )
    } else {
        losses.push(F3dLossCode::ReplaceFaceFrameAssumed.note(format!(
            "ReplaceFace {} reads its owned role-0x09 or role-0x21 groups as replacement support and role-0x10 groups as target faces while its prefix control is unresolved (DR-810).", scope.id
        )));
        let owned = |role: DesignOperandRole| {
            groups
                .iter()
                .filter(move |group| {
                    native_stream(&group.id) == native_stream(&scope.id)
                        && group.scope_record_index == scope.record_index
                        && group.role() == role
                })
                .map(|group| group.record_index)
        };
        (
            owned(DesignOperandRole::ROLE_0X9)
                .chain(owned(DesignOperandRole::ROLE_0X21))
                .collect(),
            owned(DesignOperandRole::ROLE_0X10).collect(),
            None,
        )
    };
    let targets = selection(scope, &target_indices, false, groups, faces, bodies, losses);
    let replacements = selection(
        scope,
        &replacement_indices,
        true,
        groups,
        faces,
        bodies,
        losses,
    );
    let operands = match ReplaceFaceOperands::new(targets, replacements) {
        Ok(value) => value,
        Err(_) => {
            losses.push(F3dLossCode::ReplaceFaceOverlapUnresolved.note(format!(
                "ReplaceFace {} retains its native selections because the resolved face memberships overlap (DR-810).", scope.id
            )));
            ReplaceFaceOperands::new(FaceSelection::Native(scope.id.clone()), FaceSelection::Native(scope.id.clone())).map_err(CodecError::malformed)?
        }
    }.with_tangent_chain(tangent_chain);
    Ok(FeatureDefinition::Operation(
        FeatureOperation::ReplaceFace { operands },
    ))
}
