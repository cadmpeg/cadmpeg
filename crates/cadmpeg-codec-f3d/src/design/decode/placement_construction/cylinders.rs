// SPDX-License-Identifier: Apache-2.0
//! Bind named cylinder primitives to their solved placement operands.

use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::primitives::DesignCylinderConstruction;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayloadMut};
use crate::records::identity::Located;
use crate::records::placement_construction::DesignPlacementConstruction;
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;

fn solved_frame(
    scope: &DesignParameterScope,
    placements: &[DesignPlacementConstruction],
    construction: Option<DesignCylinderConstruction>,
) -> Option<Located<SketchPlacementMatrix>> {
    let stream = native_stream(&scope.id)?;
    let mut matches = placements.iter().filter(|placement| {
        native_stream(&placement.id) == Some(stream)
            && scope.reference_members().values().any(|record_index| {
                u64::from(*record_index) == placement.record_index
                    && construction
                        .is_none_or(|value| value.placement_record_index == *record_index)
            })
    });
    let placement = matches.next()?;
    let supported_version = match construction {
        Some(_) => matches!(placement.type_version, 4 | 6 | 8 | 9 | 16),
        None => matches!(placement.type_version, 8 | 9 | 16),
    };
    if matches.next().is_some()
        || !supported_version
        || placement.fields.inferred_x_axis.value != 0
        || placement.fields.inferred_z_axis.value != 0
    {
        return None;
    }
    let frame = super::checked_solved_frame(placement)?;
    if construction.is_some() || matches!(placement.type_version, 8 | 16) {
        let [x, y, z, _] = frame.value.rows();
        cadmpeg_ir::features::geometry::FeatureRigidPlacement::new(
            cadmpeg_ir::transform::Transform::affine([x, y, z])?,
        )?;
    }
    let Some(local) = construction.and_then(|value| value.local_frame) else {
        return Some(frame);
    };
    let [support_x, support_y, support_z, _] = frame.value.rows();
    let [local_x, local_y, local_z, _] = local.value.rows();
    let composed = Transform::affine([support_x, support_y, support_z])?
        .compose(Transform::affine([local_x, local_y, local_z])?)
        .ok()?;
    if !composed.is_proper_rigid() {
        return None;
    }
    Some(Located {
        value: composed.rows().try_into().ok()?,
        offset: frame.offset,
    })
}

pub(crate) fn bind(
    scopes: &mut [DesignParameterScope],
    placements: &[DesignPlacementConstruction],
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    for scope in scopes {
        let DesignScopePayloadMut::CylinderPrimitive(Some(cylinder)) = scope.payload_mut() else {
            continue;
        };
        if cylinder.transform.is_some() {
            continue;
        }
        let construction = cylinder.construction;
        let transform = solved_frame(scope, placements, construction);
        if let DesignScopePayloadMut::CylinderPrimitive(Some(cylinder)) = scope.payload_mut() {
            cylinder.transform = transform;
        }
        if let Some(construction) = construction {
            if transform.is_some() {
                let frame_reading = if construction.local_frame.is_some() {
                    "composes its scope-local matrix after the referenced support"
                } else {
                    "uses the referenced support frame as its base frame"
                };
                losses.push(F3dLossCode::CylinderLocalFrameAssumed.note(format!(
                    "CylinderPrimitive {} {frame_reading} (DR-813).",
                    scope.id
                )));
            } else {
                losses.push(F3dLossCode::CylinderPlacementUnbound.note(format!(
                    "CylinderPrimitive {} retains its native placement because support record {} has no unique checked solved frame.",
                    scope.id, construction.placement_record_index
                )));
            }
        } else if transform.is_none() {
            losses.push(F3dLossCode::CylinderPlacementUnbound.note(format!(
                "CylinderPrimitive {} retains its native placement because its references require one checked version-8, version-9, or version-16 placement operand.",
                scope.id
            )));
        }
    }
    losses
}

#[cfg(test)]
mod tests;
