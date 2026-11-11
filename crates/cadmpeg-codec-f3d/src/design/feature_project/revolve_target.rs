// SPDX-License-Identifier: Apache-2.0
//! The body or body-face target of a one-sided to-object `Revolve`.

use super::unique_feature_match;
use crate::design::face_resolve::{resolved_body_recipe_selection, resolved_historical_face_group};
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::body_recipe::DesignBodyRecipeOperand;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_ir::features::{
    AngularTermination, FaceSelection, FeatureDefinition, FeatureOperation,
    PartialRevolveConstruction, RevolveConstruction, RevolveExtent,
};

/// Replace the native target group of a projected one-sided `Revolve` with
/// the faces it selects in the feature's input state: every face of a
/// body-recipe target, or the one face of a face-recipe target. A face
/// recipe that resolves several faces keeps the native group.
pub(super) fn bind_revolve_shape_target(
    definition: &mut FeatureDefinition,
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    body_recipe_operands: &[DesignBodyRecipeOperand],
    histories: &[crate::history_records::AsmHistory],
) {
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

#[cfg(test)]
mod tests;
