// SPDX-License-Identifier: Apache-2.0
//! Resolve Emboss sketch-profile operands to neutral profile loops or regions.

use super::{
    histories_for_scope, region_members, resolved_sketch_profile_regions, ExtrudeProfileResolution,
    SketchCurveSelectionResolution,
};
use crate::ids::native_stream;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PlanarProfileRef};
use cadmpeg_ir::report::loss::LossNote;

/// Bind each Emboss `Wrap` profile to the Sketch loops or atomic regions that
/// its sketch-profile operand selects exactly. Returns one loss for each Emboss
/// whose profile stays native.
pub(crate) fn bind_emboss_profile_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    curve_resolution: &SketchCurveSelectionResolution<'_>,
    resolution: ExtrudeProfileResolution<'_>,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut scopes = curve_resolution
            .scopes
            .iter()
            .filter(|scope| scope.id == native_ref && scope.kind() == DesignFeatureKind::Emboss);
        let (Some(scope), None) = (scopes.next(), scopes.next()) else {
            continue;
        };
        let mut edit_result = Ok(());
        let mut native = false;
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Wrap { profile, .. }) = definition
            else {
                return;
            };
            if !matches!(profile, PlanarProfileRef::Native(_)) {
                return;
            }
            match resolved_emboss_profile(scope, curve_resolution, resolution) {
                Ok(Some(resolved)) => *profile = resolved,
                Ok(None) => native = true,
                Err(error) => edit_result = Err(error),
            }
        });
        resolution.profile_work.finish_selection();
        edit_result?;
        if native {
            losses.push(crate::loss::F3dLossCode::EmbossProfileSelectionNative.note(format!(
                "Emboss scope {} retains its native sketch-profile reference because the selected contour mapping is unresolved.",
                scope.id
            )));
        }
    }
    Ok(losses)
}

fn resolved_emboss_profile(
    scope: &DesignParameterScope,
    curve_resolution: &SketchCurveSelectionResolution<'_>,
    resolution: ExtrudeProfileResolution<'_>,
) -> Result<Option<PlanarProfileRef>, CodecError> {
    let Some(operand) = scope
        .emboss_operand_construction()
        .and_then(|construction| construction.sketch_profile.as_ref())
    else {
        return Ok(None);
    };
    let Some(stream) = native_stream(&scope.id) else {
        return Ok(None);
    };
    let mut placements = curve_resolution.placements.iter().filter(|placement| {
        native_stream(&placement.id) == Some(stream) && placement.entity_id == operand.entity_id
    });
    let (Some(placement), None) = (placements.next(), placements.next()) else {
        return Ok(None);
    };
    let sketch_id = crate::ids::neutral_sketch_id(placement);
    let Some(sketch) = curve_resolution
        .sketches
        .iter()
        .find(|sketch| sketch.id == sketch_id)
    else {
        return Ok(None);
    };
    if let Some(profiles) = resolved_sketch_profile_regions(
        stream,
        operand,
        sketch,
        curve_resolution.curve_identities,
        curve_resolution.sketch_entities,
    ) {
        return Ok(PlanarProfileRef::sketch_profiles(sketch.id.clone(), profiles).ok());
    }
    let histories =
        histories_for_scope(&scope.id, resolution.scope_histories, resolution.histories);
    region_members::resolve(
        stream,
        operand,
        sketch,
        curve_resolution.curve_identities,
        curve_resolution.sketch_entities,
        resolution.scoped(histories),
        true,
    )
}
