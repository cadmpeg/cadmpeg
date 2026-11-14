use super::{native_stream, project_face_selection, ProjectInputs};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    features::{
        sheet_metal::{SheetMetalRefoldConstruction, SheetMetalUnfoldConstruction},
        FaceSelection, FeatureDefinition, FeatureId, FeatureOperation,
    },
    report::loss::LossNote,
};
use std::collections::HashMap;

pub(super) fn unfold(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<FeatureDefinition, CodecError> {
    let mut construction = SheetMetalUnfoldConstruction::default();
    if let Some(operation) = scope.unfold_operation() {
        construction.all_bends = match operation.control.value {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        };
        let code = if construction.all_bends.is_some() {
            F3dLossCode::UnfoldAllBendsFlagAssumed
        } else {
            F3dLossCode::UnfoldControlUnresolved
        };
        losses.push(code.note(format!(
            "Unfold scope {} reads control {} as the all-bends flag when Boolean.",
            scope.id, operation.control.value
        )));
        losses.push(F3dLossCode::UnfoldSelectionSlotsAssumed.note(format!("Unfold scope {} reads the first and fourth prefix references as bend and stationary face groups.", scope.id)));
        let mut selection = |index| -> Result<FaceSelection, CodecError> {
            let mut groups = inputs.construction_groups.iter().filter(|group| {
                group.scope_record_index == scope.record_index
                    && group.record_index == index
                    && native_stream(&group.id) == native_stream(&scope.id)
            });
            let first = groups.next();
            let value = if groups.next().is_none() {
                match first {
                    Some(group) => project_face_selection(
                        scope,
                        group,
                        inputs.face_operands,
                        inputs.histories,
                    )?,
                    None => FaceSelection::Native(scope.id.clone()),
                }
            } else {
                FaceSelection::Native(scope.id.clone())
            };
            if matches!(value, FaceSelection::Native(_)) {
                losses.push(F3dLossCode::UnfoldSelectionNative.note(format!(
                    "Unfold scope {} retains face-group {} through its native selection.",
                    scope.id, index
                )));
            }
            Ok(value)
        };
        construction.stationary = Some(selection(operation.stationary_group.value)?);
        construction.bends = Some(selection(operation.bend_group.value)?);
        construction.neutral_factor = operation
            .rule_material_scalar
            .as_ref()
            .map(|value| value.value);
        if construction.neutral_factor.is_some() {
            losses.push(F3dLossCode::UnfoldMaterialScalarFactorAssumed.note(format!("Unfold scope {} reads the linked rule's material scalar as its neutral-axis factor.", scope.id)));
        }
    } else {
        losses.push(F3dLossCode::UnfoldEnvelopeUnresolved.note(format!(
            "Unfold scope {} retains an unresolved construction prefix.",
            scope.id
        )));
    }
    if construction.neutral_factor.is_none() {
        losses.push(F3dLossCode::UnfoldRuleFactorUnresolved.note(format!(
            "Unfold scope {} retains an unresolved rule neutral-axis factor.",
            scope.id
        )));
    }
    Ok(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalUnfold { construction },
    ))
}

pub(super) fn refold(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    ids: &HashMap<(&str, u32), FeatureId>,
    ordinals: &HashMap<(&str, u32), u64>,
    losses: &mut Vec<LossNote>,
) -> FeatureDefinition {
    let source = (|| {
        let operation = scope.refold_operation()?;
        let stream = native_stream(&scope.id)?;
        let [source] = operation.preceding_unfolds.as_slice() else {
            return None;
        };
        if !inputs.scopes.iter().any(|candidate| {
            candidate.record_index == *source
                && native_stream(&candidate.id) == Some(stream)
                && candidate.kind() == DesignFeatureKind::Unfold
        }) {
            return None;
        }
        let source_ordinal = ordinals.get(&(stream, *source))?;
        let target_ordinal = ordinals.get(&(stream, scope.record_index))?;
        if source_ordinal >= target_ordinal {
            return None;
        }
        ids.get(&(stream, *source)).cloned()
    })();
    if source.is_some() {
        losses.push(F3dLossCode::RefoldGroupSourceAssumed.note(format!("Refold scope {} binds the unique preceding typed Unfold in its ordered UnfoldRefoldGroupMode membership.", scope.id)));
    } else {
        losses.push(F3dLossCode::RefoldSourceUnresolved.note(format!(
            "Refold scope {} retains an absent, ambiguous or un-emitted preceding Unfold source.",
            scope.id
        )));
    }
    FeatureDefinition::Operation(FeatureOperation::SheetMetalRefold {
        construction: SheetMetalRefoldConstruction { unfold: source },
    })
}

#[cfg(test)]
mod tests;
