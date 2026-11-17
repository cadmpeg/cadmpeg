// SPDX-License-Identifier: Apache-2.0
//! Plane-angle values of CSG items, read against the turn of their unit.

use std::collections::BTreeSet;
use std::f64::consts::TAU;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::super::geometry::{unit_scale_radians, GeometryData};
use super::super::RecordExt;
use crate::parse::{Exchange, Value};

/// A stated angle within this fraction of a full turn of its unit is a full
/// turn.
const FULL_TURN_RELATIVE_TOLERANCE: f64 = 1.0e-9;

/// The number of degrees in one turn.
const DEGREES_PER_TURN: f64 = 360.0;

/// The plane-angle units of an exchange.
pub(super) struct AngleUnits<'a> {
    geometry: &'a GeometryData,
    /// The radian scale of each conversion-based plane-angle unit named
    /// `degree`.
    degree_scales: Vec<f64>,
}

impl<'a> AngleUnits<'a> {
    /// Index the degree units of `exchange`.
    pub(super) fn new(
        exchange: &Exchange,
        geometry: &'a GeometryData,
        ctx: &DecodeContext<'_>,
    ) -> Result<Self, CodecError> {
        let mut degree_scales = Vec::new();
        for (id, record) in exchange.entities("CONVERSION_BASED_UNIT") {
            if record.partial("PLANE_ANGLE_UNIT").is_none() {
                continue;
            }
            let Some(Value::String(bytes)) = record
                .partial("CONVERSION_BASED_UNIT")
                .and_then(|partial| partial.parameters.first())
            else {
                continue;
            };
            let named_degree = exchange
                .decode_string(bytes)
                .is_ok_and(|name| name.trim().eq_ignore_ascii_case("degree"));
            if !named_degree {
                continue;
            }
            if let Some(scale) = unit_scale_radians(id, exchange, &mut BTreeSet::new(), ctx)? {
                degree_scales.push(scale.get());
            }
        }
        Ok(Self {
            geometry,
            degree_scales,
        })
    }

    /// The number of units of `record`'s plane-angle unit in one turn. A
    /// unit named `degree` measures one 360th of a turn, whatever radian
    /// factor it states.
    fn units_per_turn(&self, record: u64) -> f64 {
        let scale = self.geometry.units.angle([record]).get();
        if self.degree_scales.contains(&scale) {
            DEGREES_PER_TURN
        } else {
            TAU / scale
        }
    }

    /// The angle `value` of `record`, in radians.
    pub(super) fn radians(&self, record: u64, value: f64) -> f64 {
        value * TAU / self.units_per_turn(record)
    }

    /// The angle `value` of `record` in radians, exactly `2π` where the value
    /// is one turn of its unit, and `None` above one turn.
    pub(super) fn sweep(&self, record: u64, value: f64) -> Option<f64> {
        let turn = self.units_per_turn(record);
        if (value - turn).abs() <= FULL_TURN_RELATIVE_TOLERANCE * turn {
            return Some(TAU);
        }
        (value < turn).then(|| value * TAU / turn)
    }
}
