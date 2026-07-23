// SPDX-License-Identifier: Apache-2.0
//! STEP representation context and unit emission.

use crate::writer::{real, string, Ref};

use super::Builder;
use crate::vocab::{
    GEOMETRIC_REPRESENTATION_CONTEXT, LENGTH_UNIT, PLANE_ANGLE_UNIT, RATIO_UNIT, SOLID_ANGLE_UNIT,
    UNCERTAINTY_MEASURE_WITH_UNIT,
};

impl Builder<'_> {
    pub(super) fn emit_context(&mut self) -> Ref {
        let len = self.emit_length_unit();
        let angle = self.emit_angle_unit();
        let solid = self.emitter.emit_raw(
            SOLID_ANGLE_UNIT,
            "( NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT() )",
        );
        let unc = self.emitter.emit(
            UNCERTAINTY_MEASURE_WITH_UNIT,
            &format!(
                "LENGTH_MEASURE({}),{len},{},{}",
                real(self.ir.tolerances.linear),
                string("distance_accuracy_value"),
                string("maximum model space distance")
            ),
        );
        self.emitter.emit_raw(
            GEOMETRIC_REPRESENTATION_CONTEXT,
            &format!(
                "( GEOMETRIC_REPRESENTATION_CONTEXT(3) \
                 GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT(({unc})) \
                 GLOBAL_UNIT_ASSIGNED_CONTEXT(({len},{angle},{solid})) \
                 REPRESENTATION_CONTEXT('Context','3D') )"
            ),
        )
    }

    pub(super) fn emit_length_unit(&mut self) -> Ref {
        if let Some(unit) = self.units.length {
            return unit;
        }
        let unit = self.emitter.emit_raw(
            LENGTH_UNIT,
            "( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) )",
        );
        self.units.length = Some(unit);
        unit
    }

    pub(super) fn emit_angle_unit(&mut self) -> Ref {
        if let Some(unit) = self.units.angle {
            return unit;
        }
        let unit = self.emitter.emit_raw(
            PLANE_ANGLE_UNIT,
            "( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) )",
        );
        self.units.angle = Some(unit);
        unit
    }

    pub(super) fn emit_ratio_unit(&mut self) -> Ref {
        if let Some(unit) = self.units.ratio {
            return unit;
        }
        let unit = self
            .emitter
            .emit_raw(RATIO_UNIT, "( NAMED_UNIT(*) RATIO_UNIT() )");
        self.units.ratio = Some(unit);
        unit
    }
}
