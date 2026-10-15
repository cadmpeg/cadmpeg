// SPDX-License-Identifier: Apache-2.0
//! Shared measure carriers for characteristic representations.

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::pmi::PmiValue;
use cadmpeg_ir::report::loss::LossNote;

use super::carriers::{CarrierGraph, CarrierSets, RecordCarriers, Summary};
use super::{
    is_measure_record, measure, measure_item_name, MeasureContext, MeasureParameters,
    NominalSelection,
};
use crate::parse::{Exchange, Value};

struct CachedMeasure {
    value: Option<PmiValue>,
    losses: Vec<LossNote>,
}

pub(super) struct CharacteristicAnalysis {
    pub(super) selection: NominalSelection,
    pub(super) losses: Vec<LossNote>,
}

pub(super) struct CharacteristicIndex<'ctx, 'arena> {
    graph: CarrierGraph<'ctx, 'arena>,
    items: HashMap<u64, CachedMeasure>,
    analyses: HashMap<(u64, bool), CharacteristicAnalysis>,
    ctx: &'ctx DecodeContext<'arena>,
    storage: ScopedReservation<'ctx>,
}

/// Value aggregates contribute to reference depth as well as record edges.
fn edges(
    value: &Value,
    depth: usize,
    children: &mut Vec<(u64, usize)>,
    storage: &mut ScopedReservation<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("STEP characteristic carrier value walk")?;
    match value {
        Value::Reference(id) => ctx.push_scoped_vec(
            storage,
            children,
            (*id, depth),
            "STEP characteristic carrier edges",
        )?,
        Value::List(values) => {
            let mut source = values.iter();
            for _ in 0..source.len() {
                let value = ctx
                    .next_charged(&mut source, "STEP characteristic carrier list traversal")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP characteristic list source ended early")
                    })?;
                edges(value, depth + 1, children, storage, ctx)?;
            }
        }
        Value::Typed(_, inner) => {
            ctx.charge_work(1, "STEP characteristic carrier typed descent")?;
            edges(inner, depth + 1, children, storage, ctx)?;
        }
        _ => {}
    }
    Ok(())
}

impl<'ctx, 'arena> CharacteristicIndex<'ctx, 'arena> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'arena>) -> Result<Self, CodecError> {
        Ok(Self {
            graph: CarrierGraph::new(ctx)?,
            items: HashMap::new(),
            analyses: HashMap::new(),
            ctx,
            storage: ctx.reserve_scoped(0, "STEP characteristic carrier scratch")?,
        })
    }

    fn ensure(
        &mut self,
        id: u64,
        exchange: &Exchange,
        measurements: &MeasureContext<'_, 'ctx>,
    ) -> Result<(), CodecError> {
        let ctx = self.ctx;
        let items = &mut self.items;
        let storage = &mut self.storage;
        self.graph.ensure(id, |id, _sets, node_storage| {
            let mut local = Summary::default();
            let mut children = Vec::new();
            let Some(record) = ctx.get_btree_map(
                exchange.records(),
                &id,
                "STEP characteristic carrier record",
            )?
            else {
                return Ok(RecordCarriers { local, children });
            };
            if is_measure_record(ctx, record)? {
                let reports =
                    std::cell::RefCell::new(ctx.reserve_scoped(0, "STEP measure carrier reports")?);
                let mut losses = Vec::new();
                let mut cached = MeasureContext {
                    length_scale: measurements.length_scale,
                    angle_scale: measurements.angle_scale,
                    graph_limit: measurements.graph_limit,
                    cache: measurements.cache,
                    losses: (&mut losses, &reports),
                };
                let value = storage
                    .with_storage(|| measure(&Value::Reference(id), exchange, &mut cached, ctx))?;
                let mut name_storage =
                    ctx.reserve_scoped(0, "STEP measure carrier name scratch")?;
                let name = if value.is_some() {
                    storage.with_storage(|| {
                        measure_item_name(
                            id,
                            record,
                            exchange,
                            (&mut losses, &reports),
                            (ctx, &mut name_storage),
                        )
                    })?
                } else {
                    None
                };
                if value.is_some() {
                    local[0] = CarrierSets::leaf(id);
                    if name
                        .as_deref()
                        .is_some_and(|name| name.eq_ignore_ascii_case("nominal value"))
                    {
                        local[1] = CarrierSets::leaf(id);
                    }
                }
                drop(name);
                drop(name_storage);
                if !losses.is_empty() {
                    local[2] = CarrierSets::leaf(id);
                }
                storage.absorb(&mut reports.into_inner())?;
                storage.with_storage(|| {
                    ctx.insert_hash_map(
                        items,
                        id,
                        CachedMeasure { value, losses },
                        "step_pmi_measure_values",
                    )
                })?;
            } else {
                let mut partials = record.partials.iter();
                for _ in 0..partials.len() {
                    let partial = ctx
                        .next_charged(
                            &mut partials,
                            "STEP characteristic carrier partial traversal",
                        )?
                        .ok_or_else(|| {
                            CodecError::malformed("STEP characteristic partial source ended early")
                        })?;
                    let mut parameters = partial.parameters.iter();
                    for _ in 0..parameters.len() {
                        let value = ctx
                            .next_charged(
                                &mut parameters,
                                "STEP characteristic carrier parameter traversal",
                            )?
                            .ok_or_else(|| {
                                CodecError::malformed(
                                    "STEP characteristic parameter source ended early",
                                )
                            })?;
                        edges(value, 1, &mut children, node_storage, ctx)?;
                    }
                }
            }
            Ok(RecordCarriers { local, children })
        })
    }

    pub(super) fn value_summary(
        &mut self,
        value: &Value,
        depth: usize,
        exchange: &Exchange,
        measurements: &MeasureContext<'_, 'ctx>,
    ) -> Result<Summary, CodecError> {
        if depth >= measurements.graph_limit {
            return Ok(Summary::default());
        }
        let ctx = self.ctx;
        let _nested = ctx.enter_nested("step_pmi_measure_id_walk")?;
        let mut summary = Summary::default();
        match value {
            Value::Reference(id) => {
                self.ensure(*id, exchange, measurements)?;
                summary = self.graph.summary(*id, measurements.graph_limit - depth)?;
            }
            Value::List(values) => {
                let mut source = values.iter();
                for _ in 0..source.len() {
                    let value = ctx
                        .next_charged(
                            &mut source,
                            "STEP characteristic carrier root list traversal",
                        )?
                        .ok_or_else(|| {
                            CodecError::malformed("STEP characteristic list source ended early")
                        })?;
                    let child = self.value_summary(value, depth + 1, exchange, measurements)?;
                    for (target, child) in summary.iter_mut().zip(child) {
                        *target = self.graph.sets.union(*target, child)?;
                    }
                }
            }
            Value::Typed(_, inner) => {
                ctx.charge_work(1, "STEP typed measure ID descent")?;
                summary = self.value_summary(inner, depth + 1, exchange, measurements)?;
            }
            _ => {}
        }
        Ok(summary)
    }

    pub(super) fn analysis(
        &mut self,
        key: (u64, bool),
        parameters: &MeasureParameters<'_>,
        exchange: &Exchange,
        measurements: &MeasureContext<'_, 'ctx>,
    ) -> Result<&CharacteristicAnalysis, CodecError> {
        let ctx = self.ctx;
        if !ctx.contains_key_hash_map(
            &self.analyses,
            &key,
            "STEP characteristic analysis lookup",
        )? {
            let reports = std::cell::RefCell::new(
                ctx.reserve_scoped(0, "STEP characteristic cached reports")?,
            );
            let mut losses = Vec::new();
            let mut cached = MeasureContext {
                length_scale: measurements.length_scale,
                angle_scale: measurements.angle_scale,
                graph_limit: measurements.graph_limit,
                cache: measurements.cache,
                losses: (&mut losses, &reports),
            };
            let (selection, mut storage) = ctx
                .with_scoped_storage("STEP characteristic analysis values", || {
                    self.analyze(parameters, exchange, &mut cached)
                })?;
            storage.absorb(&mut reports.into_inner())?;
            self.storage.absorb(&mut storage)?;
            self.storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut self.analyses,
                    key,
                    CharacteristicAnalysis { selection, losses },
                    "STEP characteristic analyses",
                )
            })?;
        }
        Ok(ctx
            .get_hash_map(&self.analyses, &key, "STEP characteristic analysis lookup")?
            .expect("cached analysis"))
    }

    pub(super) fn analyze(
        &mut self,
        parameters: &MeasureParameters<'_>,
        exchange: &Exchange,
        measurements: &mut MeasureContext<'_, 'ctx>,
    ) -> Result<NominalSelection, CodecError> {
        let ctx = self.ctx;
        let mut summary = Summary::default();
        parameters.visit(ctx, |parameter| {
            let child = self.value_summary(parameter, 0, exchange, measurements)?;
            for (target, child) in summary.iter_mut().zip(child) {
                *target = self.graph.sets.union(*target, child)?;
            }
            Ok(())
        })?;
        self.graph.sets.visit(summary[2], &mut |id| {
            let cached = ctx
                .get_hash_map(&self.items, &id, "STEP characteristic measure cache lookup")?
                .expect("cached measure");
            let mut losses = cached.losses.iter();
            for _ in 0..losses.len() {
                let loss = ctx
                    .next_charged(&mut losses, "STEP characteristic measure diagnostic replay")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP characteristic loss source ended early")
                    })?;
                let loss =
                    loss.try_clone_for_decode(ctx, "STEP characteristic measure loss copy")?;
                ctx.push_scoped_vec(
                    &mut measurements.losses.1.borrow_mut(),
                    measurements.losses.0,
                    loss,
                    "step_pmi_losses",
                )?;
            }
            Ok(())
        })?;
        let named = self.graph.sets.count(summary[1]);
        let all = self.graph.sets.count(summary[0]);
        let selected = if named == 1 {
            Some(summary[1])
        } else if named == 0 && all == 1 {
            Some(summary[0])
        } else {
            None
        };
        if let Some(set) = selected {
            let id = self.graph.sets.first(set).expect("one selected measure");
            let value = ctx
                .get_hash_map(&self.items, &id, "STEP characteristic measure cache lookup")?
                .and_then(|measure| measure.value)
                .expect("selected value");
            return Ok(NominalSelection::Unique(value));
        }
        if named > 1 {
            return Ok(NominalSelection::NamedAmbiguous(named));
        }
        if all > 1 {
            return Ok(NominalSelection::UnnamedAmbiguous(all));
        }
        // If discovered measures yield no values, the original route tries each
        // representation parameter as a raw measure in parameter order.
        let mut scratch = ctx.reserve_scoped(0, "STEP characteristic fallback values")?;
        let mut values = Vec::new();
        parameters.visit(ctx, |parameter| {
            if let Some(value) = measure(parameter, exchange, measurements, ctx)? {
                ctx.push_scoped_vec(&mut scratch, &mut values, value, "step_pmi_measure_values")?;
            }
            Ok(())
        })?;
        Ok(match values.as_slice() {
            [] => NominalSelection::Absent,
            [value] => NominalSelection::Unique(*value),
            values => NominalSelection::UnnamedAmbiguous(values.len()),
        })
    }
}
