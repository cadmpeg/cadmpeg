// SPDX-License-Identifier: Apache-2.0

use super::{native_stream, unique_feature_match, ProjectInputs, DESIGN_LENGTH_TO_MILLIMETRE};
use crate::design::decode::operands::face_sources::{
    face_source_record_is_registered, FaceSourceType,
};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_ir::features::profiles::PlanarProfileRef;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveLength;

pub(super) fn project(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    losses: &mut Vec<LossNote>,
) -> Option<FeatureDefinition> {
    let stream = native_stream(&scope.id)?;
    if !face_source_record_is_registered(
        inputs.design_types,
        &scope.id,
        scope.record_index,
        scope.class_tag.as_str(),
        FaceSourceType::Scope,
    ) {
        return None;
    }

    let group = unique_feature_match(inputs.face_source_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && !group.source_members.is_empty()
    }));
    let profile = match group {
        Some(group) => {
            losses.push(F3dLossCode::BaseFaceRegionsNative.note(format!(
                "Face scope {} retains its selected regions through source carrier {}.",
                scope.id, group.id,
            )));
            PlanarProfileRef::Native(group.id.clone())
        }
        None => {
            losses.push(F3dLossCode::BaseFaceSourceCarrierUnresolved.note(format!(
                "Face scope {} has no unique decoded source-region carrier.",
                scope.id,
            )));
            PlanarProfileRef::Unresolved(scope.id.clone())
        }
    };

    let thickness = unique_feature_match(inputs.sheet_metal_settings.iter().filter(|settings| {
        native_stream(settings.id()) == Some(stream)
            && settings.owner_record_index == u64::from(scope.record_index)
            && group
                .is_none_or(|group| settings.record_index == u64::from(group.paired_record_index))
            && scope
                .reference_members()
                .values()
                .any(|&record| u64::from(record) == settings.record_index)
    }))
    .and_then(|settings| settings.rule_thickness)
    .and_then(|thickness| PositiveLength::new(thickness.get() * DESIGN_LENGTH_TO_MILLIMETRE));
    if thickness.is_none() {
        losses.push(F3dLossCode::BaseFaceThicknessUnresolved.note(format!(
            "Face scope {} has no unique positive sheet-metal rule thickness.",
            scope.id,
        )));
    }
    let side = scope
        .flange_side
        .map(|stored| stored.value.thickness_side());
    if side.is_none() {
        losses.push(F3dLossCode::BaseFaceMaterialSideUnresolved.note(format!(
            "Face scope {} stores no admitted sheet thickness side.",
            scope.id,
        )));
    }
    losses.push(F3dLossCode::BaseFaceResultChoiceUnresolved.note(format!(
        "Face scope {} leaves the body or component creation choice unresolved (DR-67).",
        scope.id,
    )));
    Some(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalBaseFlange {
            profile,
            thickness,
            side,
        },
    ))
}

#[cfg(test)]
mod tests;
