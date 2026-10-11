// SPDX-License-Identifier: Apache-2.0
//! Shared conversion-unit rules and scalar values for one geometry stage.

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::PositiveReal;

use crate::parse::{Exchange, Value};

use super::super::{find_record_value, RecordExt, ValueExt};
use super::{si_prefix, typed_number, UnitScaleKind};

const MAX_UNIT_DEPTH: usize = 256;

#[derive(Clone, Copy)]
struct UnitValue {
    length: Option<PositiveReal>,
    angle: Option<PositiveReal>,
    steps: usize,
}

impl UnitValue {
    const EMPTY: Self = Self { length: None, angle: None, steps: 0 };

    fn select(self, kind: UnitScaleKind) -> Option<PositiveReal> {
        match kind {
            UnitScaleKind::Length => self.length,
            UnitScaleKind::Angle => self.angle,
        }
    }

    fn scaled(self, factor: f64) -> Self {
        let steps = if self.steps < MAX_UNIT_DEPTH { self.steps + 1 } else { MAX_UNIT_DEPTH };
        Self {
            length: (steps < MAX_UNIT_DEPTH).then(|| self.length.and_then(|value| PositiveReal::new(factor * value.get()))).flatten(),
            angle: (steps < MAX_UNIT_DEPTH).then(|| self.angle.and_then(|value| PositiveReal::new(factor * value.get()))).flatten(),
            steps,
        }
    }
}

#[derive(Clone, Copy)]
enum UnitRule {
    Value(UnitValue),
    Factor { value: f64, base: u64 },
}

#[derive(Clone, Copy)]
enum UnitState {
    Parsed(UnitRule),
    Active(UnitRule),
    Ready(UnitValue),
}

struct Frame {
    unit: u64,
    factor: f64,
}

pub(super) struct UnitScalarIndex<'records, 'ctx, 'bytes> {
    exchange: &'records Exchange,
    ctx: &'ctx DecodeContext<'bytes>,
    nodes: HashMap<u64, UnitState>,
    factors: HashMap<u64, Option<(f64, u64)>>,
    storage: ScopedReservation<'ctx>,
}

impl<'records, 'ctx, 'bytes> UnitScalarIndex<'records, 'ctx, 'bytes> {
    pub(super) fn new(exchange: &'records Exchange, ctx: &'ctx DecodeContext<'bytes>) -> Result<Self, CodecError> {
        Ok(Self {
            exchange,
            ctx,
            nodes: HashMap::new(),
            factors: HashMap::new(),
            storage: ctx.reserve_scoped(0, "step unit scalar memo storage")?,
        })
    }

    pub(super) fn resolve(&mut self, kind: UnitScaleKind, root: u64) -> Result<Option<PositiveReal>, CodecError> {
        let ctx = self.ctx;
        let operation = match kind {
            UnitScaleKind::Length => "step_length_unit_scale_walk",
            UnitScaleKind::Angle => "step_angle_unit_scale_walk",
        };
        let _depth = ctx.enter_nested(operation)?;
        let mut path_storage = ctx.reserve_scoped(0, "step unit scalar path storage")?;
        let mut path = Vec::<Frame>::new();
        let mut current = root;
        let mut value;
        loop {
            ctx.charge_work(1, operation)?;
            // A cut proves the root cannot resolve, but does not prove that
            // the suffix nodes cannot resolve when requested as roots.
            if path.len() >= MAX_UNIT_DEPTH {
                for frame in &path {
                    ctx.charge_work(1, "step unit scalar path reset")?;
                    let slot = ctx.get_mut_hash_map(&mut self.nodes, &frame.unit, "step unit scalar memo lookup")?.ok_or_else(invalid_state)?;
                    if let UnitState::Active(rule) = *slot {
                        *slot = UnitState::Parsed(rule);
                    }
                }
                value = UnitValue { steps: MAX_UNIT_DEPTH, ..UnitValue::EMPTY };
                let slot = ctx.get_mut_hash_map(&mut self.nodes, &root, "step unit scalar memo lookup")?.ok_or_else(invalid_state)?;
                *slot = UnitState::Ready(value);
                return Ok(value.select(kind));
            }
            let state = ctx.get_hash_map(&self.nodes, &current, "step unit scalar memo lookup")?.copied();
            let rule = match state {
                Some(UnitState::Ready(ready)) => { value = ready; break; }
                Some(UnitState::Active(_)) => { value = UnitValue::EMPTY; break; }
                Some(UnitState::Parsed(rule)) => rule,
                None => {
                    let rule = unit_rule(current, self.exchange, ctx, &mut |factor| self.factor(factor))?;
                    self.storage.with_storage(|| ctx.insert_hash_map(&mut self.nodes, current, UnitState::Parsed(rule), "step unit scalar memo nodes"))?;
                    rule
                }
            };
            match rule {
                UnitRule::Value(leaf) => {
                    let slot = ctx.get_mut_hash_map(&mut self.nodes, &current, "step unit scalar memo lookup")?.ok_or_else(invalid_state)?;
                    *slot = UnitState::Ready(leaf);
                    value = leaf;
                    break;
                }
                UnitRule::Factor { value: factor, base } => {
                    let slot = ctx.get_mut_hash_map(&mut self.nodes, &current, "step unit scalar memo lookup")?.ok_or_else(invalid_state)?;
                    *slot = UnitState::Active(rule);
                    path_storage.with_storage(|| ctx.push_vec(&mut path, Frame { unit: current, factor }, "step unit scalar path"))?;
                    current = base;
                }
            }
        }
        while let Some(frame) = path.pop() {
            ctx.charge_work(1, "step unit scalar path resolution")?;
            value = value.scaled(frame.factor);
            let slot = ctx.get_mut_hash_map(&mut self.nodes, &frame.unit, "step unit scalar memo lookup")?.ok_or_else(invalid_state)?;
            *slot = UnitState::Ready(value);
        }
        Ok(value.select(kind))
    }

    fn factor(&mut self, id: u64) -> Result<Option<(f64, u64)>, CodecError> {
        let ctx = self.ctx;
        if let Some(fields) = ctx.get_hash_map(&self.factors, &id, "step unit factor memo lookup")? {
            return Ok(*fields);
        }
        let fields = factor_fields(id, self.exchange, ctx)?;
        self.storage.with_storage(|| ctx.insert_hash_map(&mut self.factors, id, fields, "step unit factor memo fields"))?;
        Ok(fields)
    }
}

fn factor_fields(id: u64, exchange: &Exchange, ctx: &DecodeContext<'_>) -> Result<Option<(f64, u64)>, CodecError> {
    let Some(factor) = ctx.get_btree_map(exchange.records(), &id, "step_geometry_lookup")? else { return Ok(None); };
    let Some(value) = find_record_value(factor, ctx, |value| typed_number(value, ctx))? else { return Ok(None); };
    let Some(base) = find_record_value(factor, ctx, |value| Ok(value.reference()))? else { return Ok(None); };
    Ok(Some((value, base)))
}

fn unit_rule(
    id: u64,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
    factor: &mut impl FnMut(u64) -> Result<Option<(f64, u64)>, CodecError>,
) -> Result<UnitRule, CodecError> {
    let Some(record) = ctx.get_btree_map(exchange.records(), &id, "step_geometry_lookup")? else {
        return Ok(UnitRule::Value(UnitValue::EMPTY));
    };
    if let Some(unit) = record.partial(ctx, "SI_UNIT")? {
        let prefix = match unit.parameters.first() {
            Some(Value::Omitted) => Some(1.0),
            Some(Value::Enumeration(prefix)) => si_prefix(prefix),
            _ => None,
        };
        let name = unit.parameters.get(1).and_then(Value::enumeration);
        return Ok(UnitRule::Value(UnitValue {
            length: (name == Some("METRE")).then(|| prefix.and_then(|prefix| PositiveReal::new(prefix * 1000.0))).flatten(),
            angle: (name == Some("RADIAN")).then(|| prefix.and_then(PositiveReal::new)).flatten(),
            steps: 0,
        }));
    }
    if let Some(unit) = record.partial(ctx, "CONVERSION_BASED_UNIT")? {
        if let Some(id) = unit.parameters.get(1).and_then(Value::reference) {
            if let Some((value, base)) = factor(id)? {
                return Ok(UnitRule::Factor { value, base });
            }
        }
    }
    Ok(UnitRule::Value(UnitValue::EMPTY))
}

fn invalid_state() -> CodecError {
    CodecError::malformed("STEP unit scalar memo lost its active node")
}

#[cfg(test)]
mod tests;
