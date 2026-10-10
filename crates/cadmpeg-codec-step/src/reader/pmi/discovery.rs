// SPDX-License-Identifier: Apache-2.0
//! Shared annotation selection summaries and diagnostic replay.

use super::super::geometry::GeometryData;
use super::super::reference::references;
use super::super::RecordExt;
use super::{collect_placement_candidates, collect_typed_placement_candidates, find_annotation_text};
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Default)]
enum TextSelection {
    #[default]
    Absent,
    Unique(u64),
    Ambiguous(usize),
}

#[derive(Default)]
struct TextSummary {
    selection: TextSelection,
    // Only carriers that emit diagnostics need source-order replay.
    warnings: Vec<u64>,
}

struct CachedText {
    text: Option<String>,
    losses: Vec<LossNote>,
}

#[derive(Clone, Copy)]
pub(super) enum PlacementSelection {
    Absent,
    Unique(Transform),
    Ambiguous(usize),
}

enum RecordReferences {
    None,
    One(u64),
    Many,
}

pub(super) struct AnnotationDiscoveryIndex<'ctx, 'arena> {
    aliases: BTreeMap<(u64, usize, bool), (u64, usize)>,
    texts: BTreeMap<u64, CachedText>,
    summaries: BTreeMap<(u64, usize), Option<TextSummary>>,
    placements: BTreeMap<(u64, usize), PlacementSelection>,
    ctx: &'ctx DecodeContext<'arena>,
    storage: ScopedReservation<'ctx>,
}

fn record_references(record: &RawRecord, ctx: &DecodeContext<'_>) -> Result<RecordReferences, CodecError> {
    ctx.charge_work(0, "STEP annotation reference traversal")?;
    let mut selected = None;
    let mut partials = record.partials.iter();
    for _ in 0..partials.len() {
        let partial = ctx.next_charged(&mut partials, "STEP annotation reference partial traversal")?
            .ok_or_else(|| CodecError::malformed("STEP annotation partial source ended early"))?;
        let mut parameters = partial.parameters.iter();
        for _ in 0..parameters.len() {
            let value = ctx.next_charged(&mut parameters, "STEP annotation reference parameter traversal")?
                .ok_or_else(|| CodecError::malformed("STEP annotation parameter source ended early"))?;
            for reference in references(value, ctx) {
                let reference = reference?;
                match selected {
                    Some(prior) if prior != reference => return Ok(RecordReferences::Many),
                    None => selected = Some(reference),
                    _ => {}
                }
            }
        }
    }
    Ok(selected.map_or(RecordReferences::None, RecordReferences::One))
}

impl<'ctx, 'arena> AnnotationDiscoveryIndex<'ctx, 'arena> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'arena>) -> Result<Self, CodecError> {
        Ok(Self {
            aliases: BTreeMap::new(),
            texts: BTreeMap::new(),
            summaries: BTreeMap::new(),
            placements: BTreeMap::new(),
            ctx,
            storage: ctx.reserve_scoped(0, "STEP annotation discovery index scratch")?,
        })
    }

    // A carrier-free record with one distinct child shares that child's query.
    // The value depth remains part of the key so cutoffs do not change selection.
    fn canonical(
        &mut self,
        mut key: (u64, usize),
        placement: bool,
        exchange: &Exchange,
        geometry: &GeometryData,
    ) -> Result<(u64, usize), CodecError> {
        let ctx = self.ctx;
        let mut scratch = ctx.reserve_scoped(0, "STEP annotation alias scratch")?;
        let mut seen = BTreeSet::new();
        let mut path = Vec::new();
        loop {
            if let Some(&query) = ctx.get_btree_map(&self.aliases, &(key.0, key.1, placement), "STEP annotation alias lookup")? {
                key = query;
                break;
            }
            if key.1 >= 256 || !scratch.with_storage(|| ctx.insert_btree_set(&mut seen, key.0, "STEP annotation alias visited"))? {
                break;
            }
            let Some(record) = ctx.get_btree_map(exchange.records(), &key.0, "STEP annotation alias record")? else { break };
            if placement {
                let (local, _local_storage) = ctx.with_scoped_storage("STEP annotation local placement scratch", || {
                    let mut local = BTreeMap::new();
                    collect_typed_placement_candidates(record, geometry, &mut local, ctx)?;
                    Ok::<_, CodecError>(local)
                })?;
                if !local.is_empty() { break; }
            } else if record.partial(ctx, "TEXT_LITERAL")?.and_then(|partial| partial.parameters.first()).is_some()
                || record.partial(ctx, "TEXT_LITERAL_WITH_ASSOCIATED_CURVES")?.and_then(|partial| partial.parameters.first()).is_some()
            {
                break;
            }
            let RecordReferences::One(child) = record_references(record, ctx)? else { break };
            ctx.push_scoped_vec(&mut scratch, &mut path, key, "STEP annotation alias path")?;
            key = (child, key.1 + 1);
        }
        for source in ctx.admit_iter(path, "STEP annotation alias installation")? {
            self.storage.with_storage(|| ctx.insert_btree_map(&mut self.aliases, (source.0, source.1, placement), key, "STEP annotation aliases"))?;
        }
        Ok(key)
    }

    fn cache_text(&mut self, id: u64, exchange: &Exchange) -> Result<(), CodecError> {
        let ctx = self.ctx;
        if ctx.contains_key_btree_map(&self.texts, &id, "STEP annotation text cache lookup")? { return Ok(()); }
        let record = ctx.get_btree_map(exchange.records(), &id, "STEP annotation text record")?
            .ok_or_else(|| CodecError::malformed("STEP annotation text carrier is missing"))?;
        let value = record.partial(ctx, "TEXT_LITERAL")?.and_then(|partial| partial.parameters.first())
            .map_or_else(|| -> Result<_, CodecError> {
                Ok(record.partial(ctx, "TEXT_LITERAL_WITH_ASSOCIATED_CURVES")?.and_then(|partial| partial.parameters.first()))
            }, |value| Ok(Some(value)))?
            .ok_or_else(|| CodecError::malformed("STEP annotation text value is missing"))?;
        self.storage.with_storage(|| {
            let mut losses = Vec::new();
            let text = super::super::decode_text_charged(exchange, value, &mut losses, id, "PMI annotation text", StepLossCode::MetadataStringInvalid, ctx)?;
            ctx.insert_btree_map(&mut self.texts, id, CachedText { text, losses }, "STEP annotation text cache")
        })?;
        Ok(())
    }

    fn collect_text_summary(
        &mut self,
        (id, depth): (u64, usize),
        exchange: &Exchange,
        (visited, visited_storage): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
        (summary, summary_storage): (&mut TextSummary, &mut ScopedReservation<'_>),
    ) -> Result<bool, CodecError> {
        let ctx = self.ctx;
        if depth >= 256 { return Ok(false); }
        if !visited_storage.with_storage(|| ctx.insert_btree_set(visited, id, "STEP annotation summary visited"))? { return Ok(true); }
        let _nested = ctx.enter_nested("STEP annotation summary walk")?;
        let Some(record) = ctx.get_btree_map(exchange.records(), &id, "STEP annotation summary record")? else { return Ok(true) };
        if record.partial(ctx, "TEXT_LITERAL")?.and_then(|partial| partial.parameters.first()).is_some()
            || record.partial(ctx, "TEXT_LITERAL_WITH_ASSOCIATED_CURVES")?.and_then(|partial| partial.parameters.first()).is_some()
        {
            self.cache_text(id, exchange)?;
            let cached = ctx.get_btree_map(&self.texts, &id, "STEP annotation text cache lookup")?
                .ok_or_else(|| CodecError::malformed("STEP annotation text was not cached"))?;
            if cached.text.is_some() {
                summary.selection = match summary.selection {
                    TextSelection::Absent => TextSelection::Unique(id),
                    TextSelection::Unique(_) => TextSelection::Ambiguous(2),
                    TextSelection::Ambiguous(count) => TextSelection::Ambiguous(count + 1),
                };
            }
            if !cached.losses.is_empty() {
                ctx.push_scoped_vec(summary_storage, &mut summary.warnings, id, "STEP annotation diagnostic carriers")?;
            }
        }
        let mut complete = true;
        let mut partials = record.partials.iter();
        for _ in 0..partials.len() {
            let partial = ctx.next_charged(&mut partials, "STEP annotation summary partial traversal")?
                .ok_or_else(|| CodecError::malformed("STEP annotation partial source ended early"))?;
            let mut parameters = partial.parameters.iter();
            for _ in 0..parameters.len() {
                let value = ctx.next_charged(&mut parameters, "STEP annotation summary parameter traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP annotation parameter source ended early"))?;
                for reference in references(value, ctx) {
                    complete &= self.collect_text_summary((reference?, depth + 1), exchange, (visited, visited_storage), (summary, summary_storage))?;
                }
            }
        }
        Ok(complete)
    }

    pub(super) fn text(
        &mut self,
        id: u64,
        exchange: &Exchange,
        geometry: &GeometryData,
        (used, claims): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
        (losses, reports): (&mut Vec<LossNote>, &std::cell::RefCell<ScopedReservation<'_>>),
    ) -> Result<Option<String>, CodecError> {
        let ctx = self.ctx;
        let key = self.canonical((id, 0), false, exchange, geometry)?;
        if !ctx.contains_key_btree_map(&self.summaries, &key, "STEP annotation summary lookup")? {
            let mut visited_storage = ctx.reserve_scoped(0, "STEP annotation summary visited scratch")?;
            let mut summary_storage = ctx.reserve_scoped(0, "STEP annotation summary scratch")?;
            let mut summary = TextSummary::default();
            let mut visited = BTreeSet::new();
            let complete = self.collect_text_summary(key, exchange, (&mut visited, &mut visited_storage), (&mut summary, &mut summary_storage))?;
            drop(visited);
            drop(visited_storage);
            let summary = if complete {
                self.storage.absorb(&mut summary_storage)?;
                Some(summary)
            } else {
                drop(summary);
                drop(summary_storage);
                None
            };
            self.storage.with_storage(|| ctx.insert_btree_map(&mut self.summaries, key, summary, "STEP annotation summaries"))?;
        }
        let summary = ctx.get_btree_map(&self.summaries, &key, "STEP annotation summary lookup")?
            .ok_or_else(|| CodecError::malformed("STEP annotation summary is missing"))?;
        let Some(summary) = summary else {
            return find_annotation_text(id, exchange, &mut BTreeSet::new(), (used, claims), (losses, reports), 0, ctx);
        };
        for carrier in ctx.admit_iter(&summary.warnings, "STEP annotation diagnostic replay")? {
            let cached = ctx.get_btree_map(&self.texts, carrier, "STEP annotation text cache lookup")?
                .ok_or_else(|| CodecError::malformed("STEP annotation diagnostic carrier is missing"))?;
            for loss in ctx.admit_iter(&cached.losses, "STEP annotation loss replay")? {
                let loss = loss.try_clone_for_decode(ctx, "STEP annotation cached loss copy")?;
                ctx.push_scoped_vec(&mut reports.borrow_mut(), losses, loss, "step_pmi_losses")?;
            }
        }
        match summary.selection {
            TextSelection::Absent => Ok(None),
            TextSelection::Unique(carrier) => {
                claims.with_storage(|| ctx.insert_btree_set(used, carrier, "step_pmi_annotation_text_used"))?;
                let text = ctx.get_btree_map(&self.texts, &carrier, "STEP annotation text cache lookup")?
                    .and_then(|cached| cached.text.as_deref()).ok_or_else(|| CodecError::malformed("STEP selected annotation text is missing"))?;
                Ok(Some(ctx.copy_retained_text(text, "step_string_text")?))
            }
            TextSelection::Ambiguous(count) => {
                let message = ctx.format_retained(format_args!("presentation annotation #{id} has {count} reachable text carriers with no ordered composition"), "step_annotation_unordered_text")?;
                ctx.push_scoped_vec(&mut reports.borrow_mut(), losses, StepLossCode::PresentationAnnotationTextUnordered.note(message), "step_pmi_losses")?;
                Ok(None)
            }
        }
    }

    pub(super) fn placement(
        &mut self,
        record: &RawRecord,
        exchange: &Exchange,
        geometry: &GeometryData,
    ) -> Result<PlacementSelection, CodecError> {
        let ctx = self.ctx;
        match record_references(record, ctx)? {
            RecordReferences::None => Ok(PlacementSelection::Absent),
            RecordReferences::One(id) => {
                let key = self.canonical((id, 0), true, exchange, geometry)?;
                if !ctx.contains_key_btree_map(&self.placements, &key, "STEP annotation placement summary lookup")? {
                    let (summary, scratch) = ctx.with_scoped_storage("STEP annotation placement selection scratch", || {
                        let mut visited = BTreeMap::new();
                        let mut candidates = BTreeMap::new();
                        collect_placement_candidates(key.0, exchange, geometry, &mut visited, &mut candidates, key.1, ctx)?;
                        Ok::<_, CodecError>(match candidates.len() {
                            0 => PlacementSelection::Absent,
                            1 => PlacementSelection::Unique(*candidates.first_key_value().expect("one placement candidate").1),
                            count => PlacementSelection::Ambiguous(count),
                        })
                    })?;
                    drop(scratch);
                    self.storage.with_storage(|| ctx.insert_btree_map(&mut self.placements, key, summary, "STEP annotation placement summaries"))?;
                }
                let summary = ctx.get_btree_map(&self.placements, &key, "STEP annotation placement summary lookup")?
                    .ok_or_else(|| CodecError::malformed("STEP annotation placement summary is missing"))?;
                Ok(*summary)
            }
            RecordReferences::Many => {
                let (summary, _scratch) = ctx.with_scoped_storage("STEP annotation placement selection scratch", || {
                    let mut visited = BTreeMap::new();
                    let mut candidates = BTreeMap::new();
                    let mut partials = record.partials.iter();
                    for _ in 0..partials.len() {
                        let partial = ctx.next_charged(&mut partials, "STEP annotation placement partial traversal")?
                            .ok_or_else(|| CodecError::malformed("STEP annotation partial source ended early"))?;
                        let mut parameters = partial.parameters.iter();
                        for _ in 0..parameters.len() {
                            let value = ctx.next_charged(&mut parameters, "STEP annotation placement parameter traversal")?
                                .ok_or_else(|| CodecError::malformed("STEP annotation parameter source ended early"))?;
                            for reference in references(value, ctx) {
                                collect_placement_candidates(reference?, exchange, geometry, &mut visited, &mut candidates, 0, ctx)?;
                            }
                        }
                    }
                    Ok::<_, CodecError>(match candidates.len() {
                        0 => PlacementSelection::Absent,
                        1 => PlacementSelection::Unique(*candidates.first_key_value().expect("one placement candidate").1),
                        count => PlacementSelection::Ambiguous(count),
                    })
                })?;
                Ok(summary)
            }
        }
    }
}

#[cfg(test)]
mod tests;
