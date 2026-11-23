// SPDX-License-Identifier: Apache-2.0
//! Project a `SurfaceExtend` scope as a neutral surface extension.

use crate::design::face_resolve::historical_face_selection_with_native;
use crate::history::body_closures::stable_ref;
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::DesignSurfaceExtendOperation;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

#[cfg(test)]
mod tests;

/// The typed definition of a `SurfaceExtend` scope with a positive distance.
pub(super) fn project_surface_extend(
    scope: &DesignParameterScope,
    edge_operands: &[DesignEdgeOperand],
) -> Option<FeatureDefinition> {
    let operation = scope.surface_extend_operation()?;
    let distance = cadmpeg_ir::scalar::PositiveLength::new(operation.distance.get() * 10.0)?;
    let stream = native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let native = format!(
        "{stream}:design-record#{}",
        u64::from(operation.boundary_record_index)
    );
    let faces = boundary_faces(scope, operation, edge_operands, native.clone())
        .unwrap_or(FaceSelection::Native(native));
    Some(FeatureDefinition::Operation(
        FeatureOperation::ExtendSurface {
            faces,
            distance: Some(distance),
            method: operation.extension(),
        },
    ))
}

/// The input-state faces of the stored boundary edges. Each edge recipe needs
/// one operand, and that operand one candidate face in the common input state
/// with a boundary edge the feature changes.
fn boundary_faces(
    scope: &DesignParameterScope,
    operation: &DesignSurfaceExtendOperation,
    edge_operands: &[DesignEdgeOperand],
    native: String,
) -> Option<FaceSelection> {
    let stream = native_stream(&scope.id);
    let mut state = None;
    let mut slots = Vec::new();
    for &record_index in &operation.edge_record_indices {
        let mut operands = edge_operands.iter().filter(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == scope.record_index
                && operand.record_index() == record_index
        });
        let operand = operands.next()?;
        if operands.next().is_some() {
            return None;
        }
        let operand_state = operand.recipe_state_id?;
        if *state.get_or_insert(operand_state) != operand_state {
            return None;
        }
        let [face] = operand.preceding_candidate_faces.as_slice() else {
            return None;
        };
        if !operand
            .changed_boundary_edge_slots
            .iter()
            .any(|edge| operand.preceding_boundary_edge_slots.contains(edge))
        {
            return None;
        }
        let slot = stable_ref(face.as_str())?;
        if !slots.contains(&slot) {
            slots.push(slot);
        }
    }
    match historical_face_selection_with_native(scope, state?, slots, native)? {
        selection @ FaceSelection::Historical { .. } => Some(selection),
        _ => None,
    }
}
