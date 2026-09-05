// SPDX-License-Identifier: Apache-2.0
//! Numeric indexes for STEP geometry carriers.

use std::collections::HashMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct CurveIndex(pub(super) usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct SurfaceIndex(pub(super) usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct PointIndex(pub(super) usize);

pub(super) struct PointCarrier {
    pub(super) index: PointIndex,
    position: Point3,
}

/// Decode-local lookup tables for STEP carrier instance ids.
pub(super) struct CarrierIndex {
    pub(super) curves: HashMap<u64, CurveIndex>,
    pub(super) points: HashMap<u64, PointCarrier>,
    pub(super) surfaces: HashMap<u64, SurfaceIndex>,
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
                    step_instance_id(&curve.id.as_str()).map(|id| (id, CurveIndex(index)))
                })
                .collect(),
            points: ir
                .model
                .points
                .iter()
                .enumerate()
                .filter_map(|(index, point)| {
                    step_instance_id(&point.id.as_str()).map(|id| {
                        (
                            id,
                            PointCarrier {
                                index: PointIndex(index),
                                position: point.position,
                            },
                        )
                    })
                })
                .collect(),
            surfaces: ir
                .model
                .surfaces
                .iter()
                .enumerate()
                .filter_map(|(index, surface)| {
                    step_instance_id(&surface.id.as_str()).map(|id| (id, SurfaceIndex(index)))
                })
                .collect(),
        }
    }

    pub(super) fn get(&self, id: u64) -> Option<&Point3> {
        self.points.get(&id).map(|point| &point.position)
    }

    pub(super) fn contains_key(&self, id: u64) -> bool {
        self.points.contains_key(&id)
    }
}

/// Extract the numeric STEP instance id from a canonical IR identity.
pub(super) fn step_instance_id(identity: &str) -> Option<u64> {
    identity.rsplit_once('#')?.1.parse().ok()
}
