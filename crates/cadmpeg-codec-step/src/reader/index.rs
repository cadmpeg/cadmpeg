// SPDX-License-Identifier: Apache-2.0
//! Numeric indexes for STEP geometry carriers.

use std::collections::HashMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point3;

/// Decode-local lookup tables for STEP carrier instance ids.
pub(super) struct CarrierIndex {
    pub(super) curves: HashMap<u64, usize>,
    pub(super) points: HashMap<u64, usize>,
    pub(super) surfaces: HashMap<u64, usize>,
    point_positions: Vec<Point3>,
}

impl CarrierIndex {
    pub(super) fn from_ir(ir: &CadIr) -> Self {
        Self {
            curves: ir
                .model
                .curves
                .iter()
                .enumerate()
                .filter_map(|(index, curve)| {
                    step_instance_id(&curve.id.as_str()).map(|id| (id, index))
                })
                .collect(),
            points: ir
                .model
                .points
                .iter()
                .enumerate()
                .filter_map(|(index, point)| {
                    step_instance_id(&point.id.as_str()).map(|id| (id, index))
                })
                .collect(),
            surfaces: ir
                .model
                .surfaces
                .iter()
                .enumerate()
                .filter_map(|(index, surface)| {
                    step_instance_id(&surface.id.as_str()).map(|id| (id, index))
                })
                .collect(),
            point_positions: ir.model.points.iter().map(|point| point.position).collect(),
        }
    }

    pub(super) fn get(&self, id: u64) -> Option<&Point3> {
        self.points
            .get(&id)
            .and_then(|index| self.point_positions.get(*index))
    }

    pub(super) fn contains_key(&self, id: u64) -> bool {
        self.points.contains_key(&id)
    }
}

/// Extract the numeric STEP instance id from a canonical IR identity.
pub(super) fn step_instance_id(identity: &str) -> Option<u64> {
    identity.rsplit_once('#')?.1.parse().ok()
}
