// SPDX-License-Identifier: Apache-2.0
//! Removal of the legacy design arenas after native validation can use v11 states.

use cadmpeg_ir::CadIr;

const VALIDATES_CONTRACTED_DESIGN: bool = false;

pub(crate) fn finish(ir: &mut CadIr) {
    if VALIDATES_CONTRACTED_DESIGN {
        contract(ir);
    }
}

/// Removes the legacy design arenas of a document without history bodies,
/// and returns whether it removed them.
fn contract(ir: &mut CadIr) -> bool {
    if !ir.model.history_bodies.is_empty() {
        return false;
    }
    ir.model.features.clear();
    ir.model.feature_input_topologies.clear();
    ir.model.feature_result_topologies.clear();
    ir.model.parameters.clear();
    ir.model.sketches.clear();
    ir.model.sketch_entities.clear();
    ir.model.sketch_constraints.clear();
    ir.model.assembly_joints.clear();
    ir.model.configurations.clear();
    ir.model.spatial_sketches.clear();
    ir.model.spatial_sketch_entities.clear();
    ir.model.spatial_sketch_constraints.clear();
    ir.model.spreadsheets.clear();
    true
}

#[cfg(test)]
mod tests;
