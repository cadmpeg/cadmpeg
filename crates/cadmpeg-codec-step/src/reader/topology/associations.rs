// SPDX-License-Identifier: Apache-2.0
//! Stage-wide surface-curve associations and decoded pcurve declarations.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::PcurveId;

use crate::ids::{self, kind};
use crate::parse::{Exchange, Value};
use crate::reader::{RecordExt, ValueExt};

use super::{entity_parameter, surface_curve_pcurves};

struct CurveAssociations {
    candidates: BTreeMap<u64, Vec<PcurveId>>,
    members: BTreeSet<u64>,
}

pub(super) struct PcurveAssociations<'ctx, 'records, 'arena> {
    ctx: &'ctx DecodeContext<'arena>,
    exchange: &'records Exchange,
    decoded_pcurves: &'records BTreeSet<u64>,
    curves: BTreeMap<u64, CurveAssociations>,
    pcurve_surfaces: BTreeMap<u64, Option<u64>>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx, 'records, 'arena> PcurveAssociations<'ctx, 'records, 'arena> {
    pub(super) fn new(exchange: &'records Exchange, decoded_pcurves: &'records BTreeSet<u64>, ctx: &'ctx DecodeContext<'arena>) -> Result<Self, CodecError> {
        Ok(Self {
            ctx,
            exchange,
            decoded_pcurves,
            curves: BTreeMap::new(),
            pcurve_surfaces: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "STEP pcurve association facts")?,
        })
    }

    pub(super) fn contains(&mut self, curve_step: u64, surface_step: u64, pcurve_step: u64) -> Result<bool, CodecError> {
        self.candidates(curve_step, surface_step)?;
        let Some(groups) = self.ctx.get_btree_map(&self.curves, &curve_step, "STEP curve association lookup")? else { return Ok(false); };
        if !self.ctx.contains_btree_set(&groups.members, &pcurve_step, "STEP explicit seam pcurve lookup")? { return Ok(false); }
        Ok(self.ctx.get_btree_map(&self.pcurve_surfaces, &pcurve_step, "STEP pcurve declaration lookup")?.copied().flatten() == Some(surface_step))
    }

    /// Retain the source list's order and multiplicity within each surface.
    pub(super) fn candidates(
        &mut self,
        curve_step: u64,
        surface_step: u64,
    ) -> Result<&[PcurveId], CodecError> {
        let Self { ctx, exchange, decoded_pcurves, curves, pcurve_surfaces, storage } = self;
        let ctx = *ctx;
        let exchange = *exchange;
        let decoded_pcurves = *decoded_pcurves;
        if !ctx.contains_key_btree_map(curves, &curve_step, "STEP curve association membership")? {
            let groups = storage.with_storage(|| -> Result<_, CodecError> {
                let mut groups = CurveAssociations { candidates: BTreeMap::new(), members: BTreeSet::new() };
                let Some(curve) = ctx.get_btree_map(exchange.records(), &curve_step, "STEP topology record lookup")? else {
                    return Ok(groups);
                };
                let candidate_eligible = ctx.any_by(&curve.partials[..], |partial| Ok(matches!(partial.name.as_str(),
                    "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE")), "STEP associated pcurves traversal")?;
                ctx.fold(surface_curve_pcurves(ctx, curve)?, (), |(), source_value| {
                    let Some(pcurve_step) = source_value.reference() else { return Ok(()); };
                    ctx.insert_btree_set(&mut groups.members, pcurve_step, "step_associated_pcurve_members")?;
                    let surface = if let Some(&surface) = ctx.get_btree_map(pcurve_surfaces, &pcurve_step, "STEP pcurve declaration lookup")? {
                        surface
                    } else {
                        let surface = if let Some(pcurve) = ctx.get_btree_map(exchange.records(), &pcurve_step, "STEP topology record lookup")? {
                            if pcurve.partial(ctx, "PCURVE")?.is_some()
                                && ctx.contains_btree_set(decoded_pcurves, &pcurve_step, "STEP topology decoded_pcurves membership")? {
                                entity_parameter(ctx, pcurve, "PCURVE", 1)?.and_then(Value::reference)
                            } else { None }
                        } else { None };
                        ctx.insert_btree_map(pcurve_surfaces, pcurve_step, surface, "step_associated_pcurve_nodes")?;
                        surface
                    };
                    if let Some(surface) = surface.filter(|_| candidate_eligible) {
                        ctx.push_btree_group(&mut groups.candidates, surface,
                            PcurveId::from(ids::data(kind!("pcurve"), pcurve_step)),
                            "step_associated_pcurve_surfaces", "step_associated_pcurves")?;
                    }
                    Ok(())
                }, "STEP surface curve pcurve reference traversal")?;
                Ok(groups)
            })?;
            storage.with_storage(|| ctx.insert_btree_map(curves, curve_step, groups, "step_associated_curve_facts"))?;
        }
        if let Some(groups) = ctx.get_btree_map(curves, &curve_step, "STEP curve association lookup")? {
            if let Some(candidates) = ctx.get_btree_map(&groups.candidates, &surface_step, "STEP pcurve association surface lookup")? {
                return Ok(candidates);
            }
        }
        Ok(&[])
    }
}

#[cfg(test)]
mod tests;
