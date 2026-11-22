// SPDX-License-Identifier: Apache-2.0
//! Independent profile, rule thickness and material-side inputs of a base flange.

use super::{
    native_stream, unique_feature_match, DesignConstructionOperandGroup, DesignOperandRole,
    DesignParameterScope, DesignSketchPlacement, DESIGN_LENGTH_TO_MILLIMETRE,
};
use crate::loss::F3dLossCode;
use crate::records::sheet_metal_settings::DesignSheetMetalSettings;
use cadmpeg_ir::features::profiles::PlanarProfileRef;
use cadmpeg_ir::features::sheet_metal_forms::SheetMetalThicknessSide;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveLength;

pub(super) fn project(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    placements: &[DesignSketchPlacement],
    settings: &[DesignSheetMetalSettings],
    losses: &mut Vec<LossNote>,
) -> Option<(FeatureDefinition, bool)> {
    let stream = native_stream(&scope.id)?;
    let operation = scope.base_flange_operation();
    let group = unique_feature_match(groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && group.role() == DesignOperandRole::PROFILE
    }));
    let profile = scope
        .base_flange_profile()
        .and_then(|profile| {
            let group = group?;
            if group.scope_reference_ordinal != 0
                || profile.scope_reference_ordinal != 1
                || !group
                    .members()
                    .iter()
                    .map(|member| member.value)
                    .eq([profile.record_index])
                || operation.is_some_and(|operation| {
                    group.record_index != operation.profile_group_record_index
                        || profile.record_index != operation.profile_record_index
                })
            {
                return None;
            }
            let placement = placements.iter().find(|placement| {
                native_stream(&placement.id) == Some(stream)
                    && placement.entity_id == profile.entity_id
            })?;
            let sketch = crate::ids::neutral_sketch_id(placement);
            if operation.is_none() && profile.region_selection.is_some() {
                let selections = cadmpeg_ir::features::selections::NativeSelections::try_from(vec![scope.id.clone()]).ok()?;
                losses.push(F3dLossCode::BaseFlangeProfileUnresolved.note(format!(
                    "Base flange {} retains its selected profile regions in the source scope (DR-67).",
                    scope.id
                )));
                Some(PlanarProfileRef::SketchSelection { sketch, selections })
            } else {
                Some(PlanarProfileRef::Sketch(sketch))
            }
        })
        .unwrap_or_else(|| {
            losses.push(F3dLossCode::BaseFlangeProfileUnresolved.note(format!(
                "Base flange {} retains its profile in the source scope.",
                scope.id
            )));
            PlanarProfileRef::Unresolved(scope.id.clone())
        });
    let rule_thickness = unique_feature_match(settings.iter().filter(|settings| {
        native_stream(settings.id()) == Some(stream)
            && operation.map_or_else(
                || {
                    settings.owner_record_index == u64::from(scope.record_index)
                        && scope
                            .reference_members()
                            .values()
                            .any(|&index| u64::from(index) == settings.record_index)
                },
                |operation| settings.record_index == u64::from(operation.settings_record_index),
            )
    }))
    .and_then(|settings| settings.rule_thickness);
    let source_thickness =
        rule_thickness.or_else(|| operation.map(|operation| operation.thickness));
    let thickness = source_thickness
        .and_then(|value| PositiveLength::new(value.get() * DESIGN_LENGTH_TO_MILLIMETRE));
    if thickness.is_none() {
        losses.push(F3dLossCode::BaseFlangeThicknessUnresolved.note(format!(
            "Base flange {} retains its thickness in native records.",
            scope.id
        )));
    }
    let side = operation.map(|_| SheetMetalThicknessSide::Forward);
    if side.is_some() {
        losses.push(F3dLossCode::BaseFlangeSideAssumed.note(format!(
            "Base flange {} assumes the Forward material side; its operation frame stores no readable side (DR-67).",
            scope.id
        )));
    } else {
        losses.push(F3dLossCode::BaseFlangeSideUnresolved.note(format!(
            "Base flange {} retains material-side controls in its unread operation frame (DR-67).",
            scope.id
        )));
    }
    let stored_thickness = rule_thickness.is_none() && operation.is_some() && thickness.is_some();
    Some((
        FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
            profile,
            thickness,
            side,
        }),
        stored_thickness,
    ))
}

#[cfg(test)]
mod tests;
