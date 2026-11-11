// SPDX-License-Identifier: Apache-2.0
//! The body or body-face target of a one-sided to-object `Revolve`.

use super::unique_feature_match;
use crate::design::face_resolve::{resolved_body_recipe_selection, resolved_historical_face_group};
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::body_recipe::DesignBodyRecipeOperand;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::entity_selection::DesignEntitySelectionOperand;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_ir::features::{
    AngularTermination, FaceSelection, FeatureDefinition, FeatureOperation,
    FeatureSupportPlaneFrame, PartialRevolveConstruction, PlaneTargetSource, RevolveConstruction,
    RevolveExtent,
};

/// Scope records a one-sided `Revolve` target can name.
pub(super) struct RevolveTargetInputs<'a> {
    pub(super) scope: &'a DesignParameterScope,
    pub(super) scopes: &'a [DesignParameterScope],
    pub(super) construction_groups: &'a [DesignConstructionOperandGroup],
    pub(super) entity_selection_operands: &'a [DesignEntitySelectionOperand],
    pub(super) face_operands: &'a [DesignFaceOperand],
    pub(super) body_recipe_operands: &'a [DesignBodyRecipeOperand],
    pub(super) histories: &'a [crate::history_records::AsmHistory],
    pub(super) design_types: &'a [crate::records::entity_header::SegmentType],
    pub(super) component_naming_spaces: &'a [crate::records::recipes::DesignComponentNamingSpace],
}

/// Replace the native target group of a projected one-sided `Revolve` with
/// what it selects: every face of a body-recipe target in the feature's
/// input state, the one face of a face-recipe target, or the plane of a
/// `WorkPlane` or origin-plane entity selection. A face recipe that resolves
/// several faces keeps the native group.
pub(super) fn bind_revolve_shape_target(
    definition: &mut FeatureDefinition,
    inputs: &RevolveTargetInputs<'_>,
) {
    let RevolveTargetInputs {
        scope,
        construction_groups,
        face_operands,
        body_recipe_operands,
        histories,
        ..
    } = *inputs;
    let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) = definition
    else {
        return;
    };
    let (RevolveConstruction::Resolved {
        extent: RevolveExtent::OneSided { termination },
        ..
    }
    | RevolveConstruction::Unresolved(PartialRevolveConstruction::Axis {
        extent: Some(RevolveExtent::OneSided { termination }),
        ..
    })) = construction
    else {
        return;
    };
    let AngularTermination::ToShape {
        target: FaceSelection::Native(native),
    } = termination
    else {
        return;
    };
    let stream = native_stream(&scope.id);
    let Some(group) = unique_feature_match(construction_groups.iter().filter(|group| {
        group.id == *native
            && group.scope_record_index == scope.record_index
            && native_stream(&group.id) == stream
    })) else {
        return;
    };
    if let Some(target) = resolved_body_recipe_selection(scope, group, body_recipe_operands) {
        *termination = AngularTermination::ToShape { target };
    } else if let Some((plane, source)) = selected_target_plane(inputs, group) {
        let Some(native) = cadmpeg_core::text::NonBlankString::new(native.clone()) else {
            return;
        };
        *termination = AngularTermination::ToPlane {
            plane,
            native,
            source,
        };
    } else if let Some(face) = resolved_historical_face_group(
        scope,
        crate::history::effective_scope_previous_history_state_id(scope, histories),
        group,
        face_operands,
    )
    .filter(|face| matches!(face, FaceSelection::Historical { faces, .. } if faces.len() == 1))
    {
        *termination = AngularTermination::ToFace { face, offset: None };
    }
}

/// Return the frame of the plane a target group's sole entity selection
/// names: the `WorkPlane` scope at the next record, or an origin plane of a
/// scope whose Design stream binds one component.
fn selected_target_plane(
    inputs: &RevolveTargetInputs<'_>,
    group: &DesignConstructionOperandGroup,
) -> Option<(FeatureSupportPlaneFrame, Option<PlaneTargetSource>)> {
    let RevolveTargetInputs {
        scope,
        scopes,
        entity_selection_operands,
        design_types,
        component_naming_spaces,
        ..
    } = *inputs;
    if let Some(work_plane) =
        super::selected_work_plane(scope, group, entity_selection_operands, scopes)
    {
        let frame = super::work_plane_support_frame(work_plane.work_plane_transform()?.into())?;
        return Some((frame, Some(PlaneTargetSource::WorkPlane)));
    }
    let (ordinal, _) =
        super::selected_origin_plane(scope, group, entity_selection_operands, design_types)?;
    if !super::scope_stream_has_one_component(scope, component_naming_spaces) {
        return None;
    }
    let frame = crate::history::selection::design_geometry_origin_plane_frame(ordinal)?;
    Some((frame, None))
}

#[cfg(test)]
mod tests;
