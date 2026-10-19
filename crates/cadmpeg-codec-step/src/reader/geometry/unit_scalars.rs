// SPDX-License-Identifier: Apache-2.0
//! Conversion-unit memo shared by the readers of one immutable exchange.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::PositiveReal;

use super::super::{record_values, RecordExt, ValueExt};
use super::{si_prefix, UnitScaleKind};
use crate::parse::{Exchange, Value};

pub(super) struct UnitScalars<'a> {
    exchange: &'a Exchange,
    values: BTreeMap<(UnitScaleKind, u64), Option<PositiveReal>>,
}

impl<'a> UnitScalars<'a> {
    pub(super) fn new(exchange: &'a Exchange) -> Self {
        Self {
            exchange,
            values: BTreeMap::new(),
        }
    }

    pub(super) fn scale(
        &mut self,
        kind: UnitScaleKind,
        id: u64,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<PositiveReal>, CodecError> {
        self.resolve(kind, id, &mut BTreeSet::new(), ctx)
    }

    pub(super) fn resolve(
        &mut self,
        kind: UnitScaleKind,
        id: u64,
        active: &mut BTreeSet<u64>,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<PositiveReal>, CodecError> {
        let operation = match kind {
            UnitScaleKind::Length => "step_length_unit_scale_walk",
            UnitScaleKind::Angle => "step_angle_unit_scale_walk",
        };
        // The session limits actual recursion. A memo hit does not recreate
        // its resolved ancestry and needs only this frame.
        let _depth = ctx.enter_nested(operation)?;
        if active.contains(&id) {
            return Ok(None);
        }
        if let Some(value) = self.values.get(&(kind, id)) {
            return Ok(*value);
        }
        active.insert(id);
        let result = self.value(kind, id, active, ctx);
        active.remove(&id);
        let value = result?;
        self.values.insert((kind, id), value);
        Ok(value)
    }

    fn value(
        &mut self,
        kind: UnitScaleKind,
        id: u64,
        active: &mut BTreeSet<u64>,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<PositiveReal>, CodecError> {
        let Some(record) = self.exchange.records().get(&id) else {
            return Ok(None);
        };
        if let Some(unit) = record.partial("SI_UNIT") {
            let name = match kind {
                UnitScaleKind::Length => "METRE",
                UnitScaleKind::Angle => "RADIAN",
            };
            if unit.parameters.get(1).and_then(Value::enumeration) != Some(name) {
                return Ok(None);
            }
            let prefix = match unit.parameters.first() {
                Some(Value::Omitted) => Some(1.0),
                Some(Value::Enumeration(prefix)) => si_prefix(prefix),
                _ => None,
            };
            let multiplier = match kind {
                UnitScaleKind::Length => 1000.0,
                UnitScaleKind::Angle => 1.0,
            };
            return Ok(prefix.and_then(|prefix| PositiveReal::new(prefix * multiplier)));
        }
        let Some(unit) = record.partial("CONVERSION_BASED_UNIT") else {
            return Ok(None);
        };
        let Some(factor) = unit
            .parameters
            .get(1)
            .and_then(Value::reference)
            .and_then(|id| self.exchange.records().get(&id))
        else {
            return Ok(None);
        };
        let Some(value) = record_values(factor).find_map(ValueExt::typed_number) else {
            return Ok(None);
        };
        let Some(base) = record_values(factor).find_map(Value::reference) else {
            return Ok(None);
        };
        Ok(self
            .resolve(kind, base, active, ctx)?
            .and_then(|base| PositiveReal::new(value * base.get())))
    }
}

#[cfg(test)]
mod tests;
