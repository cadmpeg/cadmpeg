// SPDX-License-Identifier: Apache-2.0
//! Plane-angle values of CSG items, read against the turn of their unit.

use std::f64::consts::TAU;

use super::super::geometry::GeometryData;

/// A stated angle within this fraction of a full turn of its unit is a full
/// turn.
const FULL_TURN_RELATIVE_TOLERANCE: f64 = 1.0e-9;

/// The plane-angle units of an exchange.
pub(super) struct AngleUnits<'a> {
    geometry: &'a GeometryData,
}

impl<'a> AngleUnits<'a> {
    pub(super) fn new(geometry: &'a GeometryData) -> Self {
        Self { geometry }
    }

    /// The number of units of `record`'s plane-angle unit in one turn.
    fn units_per_turn(&self, record: u64) -> f64 {
        TAU / self.geometry.units.angle([record]).get()
    }

    /// The angle `value` of `record`, in radians.
    pub(super) fn radians(&self, record: u64, value: f64) -> f64 {
        value * self.geometry.units.angle([record]).get()
    }

    /// The angle `value` of `record` in radians, exactly `2π` where the value
    /// is one turn of its unit, and `None` above one turn.
    pub(super) fn sweep(&self, record: u64, value: f64) -> Option<f64> {
        let turn = self.units_per_turn(record);
        if (value - turn).abs() <= FULL_TURN_RELATIVE_TOLERANCE * turn {
            return Some(TAU);
        }
        (value < turn).then(|| self.radians(record, value))
    }
}
