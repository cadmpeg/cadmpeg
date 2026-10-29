// SPDX-License-Identifier: Apache-2.0
//! Bind solved placement geometry to construction-plane scopes.

use crate::ids::native_stream;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut, DesignWorkPlaneTransform,
};
use crate::records::placement_construction::DesignPlacementConstruction;
use crate::records::sketch_placement::SketchPlacementMatrix;

pub(crate) fn has_operand(
    scope: &DesignParameterScope,
    placements: &[DesignPlacementConstruction],
) -> bool {
    placements
        .iter()
        .any(|placement| references(scope, placement))
}

fn references(scope: &DesignParameterScope, placement: &DesignPlacementConstruction) -> bool {
    native_stream(&scope.id).is_some_and(|stream| native_stream(&placement.id) == Some(stream))
        && scope
            .reference_members()
            .values()
            .any(|index| u64::from(*index) == placement.record_index)
}

pub(crate) fn solved_frame(
    scope: &DesignParameterScope,
    placements: &[DesignPlacementConstruction],
) -> Option<DesignWorkPlaneTransform> {
    if !matches!(
        scope.kind(),
        DesignFeatureKind::WorkPlane | DesignFeatureKind::PlanoDeObra
    ) {
        return None;
    }
    let mut candidates = placements
        .iter()
        .filter(|placement| references(scope, placement));
    let placement = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    if placement.fields.inferred_x_axis.value != 0
        || placement.fields.inferred_z_axis.value != 0
        || placement.fields.z_flip.value != 0
        || placement
            .fields
            .axis_flipped
            .as_ref()
            .is_some_and(|value| value.value != 0)
        || placement
            .fields
            .use_u_direction
            .as_ref()
            .is_some_and(|value| value.value != 0)
    {
        return None;
    }
    let (matrix, offset) = match (&placement.fields.matrix, &placement.fields.identity) {
        (Some(matrix), _) => (matrix.value, matrix.offset),
        (None, Some(identity)) if identity.value => {
            (SketchPlacementMatrix::IDENTITY, identity.offset)
        }
        _ => return None,
    };
    crate::design::feature_project::work_plane_datum_frame(matrix.rows())?;
    Some(DesignWorkPlaneTransform {
        work_plane_transform: matrix,
        work_plane_transform_offset: offset,
        reference: None,
        work_plane_construction: None,
    })
}

pub(crate) fn bind(
    scopes: &mut [DesignParameterScope],
    placements: &[DesignPlacementConstruction],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let mut losses = Vec::new();
    for scope in scopes {
        if !matches!(
            scope.kind(),
            DesignFeatureKind::WorkPlane | DesignFeatureKind::PlanoDeObra
        ) {
            continue;
        }
        if has_operand(scope, placements) {
            let frame = solved_frame(scope, placements);
            if let DesignScopePayloadMut::WorkPlane(slot)
            | DesignScopePayloadMut::PlanoDeObra(slot) = scope.payload_mut()
            {
                *slot = frame;
            }
        }
        if scope.work_plane_frame().is_none() {
            losses.push(crate::loss::F3dLossCode::WorkPlaneFrameUnbound.note(format!(
                "WorkPlane {} retains its native definition because no unique checked solved frame binds its operands.",
                scope.id
            )));
        }
    }
    losses
}

#[cfg(test)]
mod tests;
