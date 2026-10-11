// SPDX-License-Identifier: Apache-2.0
//! Shared annotation selection summaries and diagnostic replay.

use super::super::geometry::GeometryData;
use super::super::reference::references;
use super::super::RecordExt;
use super::visit_typed_placement_candidates;
use super::carriers::{CarrierGraph, CarrierSets, RecordCarriers, SetId};
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;
use std::collections::{BTreeSet, HashMap, HashSet};

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

#[derive(Clone, Copy)]
enum WarningRange {
    Record { id: u64, warnings: SetId },
    Branch { left: usize, right: usize, warnings: SetId },
}

impl WarningRange {
    fn warnings(self) -> SetId { match self { Self::Record { warnings,.. } | Self::Branch { warnings,.. } => warnings } }
}

#[derive(Clone, Copy)]
enum WarningJob { Record(u64), Range(u64,usize) }

fn warning_ranges(graph: &mut CarrierGraph<'_, '_>, id: u64, start: usize, end: usize, ranges: &mut Vec<WarningRange>, storage: &mut ScopedReservation<'_>, ctx: &DecodeContext<'_>) -> Result<usize,CodecError> {
    // Balanced range construction has at most usize::BITS active calls.
    ctx.charge_work(1,"STEP annotation warning range construction")?;
    let range = if end-start == 1 {
        let child = graph.children(id)?[start].0;
        WarningRange::Record { id: child,warnings: graph.summary(child,256)?[1] }
    } else {
        let middle = start+(end-start)/2;
        let left = warning_ranges(graph,id,start,middle,ranges,storage,ctx)?;
        let right = warning_ranges(graph,id,middle,end,ranges,storage,ctx)?;
        let warnings = graph.sets.union(ranges[left].warnings(),ranges[right].warnings())?;
        WarningRange::Branch { left,right,warnings }
    };
    let index = ranges.len();
    ctx.push_scoped_vec(storage,ranges,range,"STEP annotation warning range nodes")?;
    Ok(index)
}

pub(super) struct AnnotationDiscoveryIndex<'ctx, 'arena> {
    aliases: HashMap<(u64, usize, bool), Option<(u64, usize)>>,
    texts: HashMap<u64, CachedText>,
    summaries: HashMap<(u64, usize), TextSummary>,
    placements: HashMap<(u64, usize), PlacementSelection>,
    graph: CarrierGraph<'ctx, 'arena>,
    warning_ranges: HashMap<u64,Vec<WarningRange>>,
    ctx: &'ctx DecodeContext<'arena>,
    storage: ScopedReservation<'ctx>,
}

fn record_references(
    record: &RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<RecordReferences, CodecError> {
    let mut selected = None;
    let mut partials = record.partials.iter();
    for _ in 0..partials.len() {
        let partial = ctx
            .next_charged(&mut partials, "STEP annotation reference partial traversal")?
            .ok_or_else(|| CodecError::malformed("STEP annotation partial source ended early"))?;
        let mut parameters = partial.parameters.iter();
        for _ in 0..parameters.len() {
            let value = ctx
                .next_charged(
                    &mut parameters,
                    "STEP annotation reference parameter traversal",
                )?
                .ok_or_else(|| {
                    CodecError::malformed("STEP annotation parameter source ended early")
                })?;
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
            aliases: HashMap::new(),
            texts: HashMap::new(),
            summaries: HashMap::new(),
            placements: HashMap::new(),
            graph: CarrierGraph::new(ctx)?,
            warning_ranges: HashMap::new(),
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
    ) -> Result<Option<(u64, usize)>, CodecError> {
        let ctx = self.ctx;
        let mut scratch = ctx.reserve_scoped(0, "STEP annotation alias scratch")?;
        let mut seen = HashSet::new();
        let mut path = Vec::new();
        let mut empty_at_any_depth = false;
        let query = loop {
            if let Some(&query) = ctx.get_hash_map(&self.aliases, &(key.0, key.1, placement), "STEP annotation alias lookup")? {
                empty_at_any_depth = query.is_none() && key.1 == 0;
                break query;
            }
            if key.1 != 0 && ctx.get_hash_map(&self.aliases, &(key.0, 0, placement), "STEP annotation empty alias lookup")?.is_some_and(Option::is_none) {
                empty_at_any_depth = true;
                break None;
            }
            if key.1 >= 256 { break None; }
            if !scratch.with_storage(|| ctx.insert_hash_set(&mut seen, key.0, "STEP annotation alias visited"))? {
                // A carrier-free single-child cycle has no exit to a carrier.
                empty_at_any_depth = true;
                break None;
            }
            let Some(record) = ctx.get_btree_map(exchange.records(), &key.0, "STEP annotation alias record")? else {
                empty_at_any_depth = true;
                break None;
            };
            if placement {
                let mut has_local = false;
                visit_typed_placement_candidates(record,geometry,ctx,|_,_| { has_local = true; Ok(()) })?;
                if has_local { break Some(key); }
            } else if record.partial(ctx, "TEXT_LITERAL")?.and_then(|partial| partial.parameters.first()).is_some()
                || record.partial(ctx, "TEXT_LITERAL_WITH_ASSOCIATED_CURVES")?.and_then(|partial| partial.parameters.first()).is_some() {
                break Some(key);
            }
            match record_references(record, ctx)? {
                RecordReferences::None => {
                    empty_at_any_depth = true;
                    break None;
                },
                RecordReferences::Many => break Some(key),
                RecordReferences::One(child) => {
                    ctx.push_scoped_vec(&mut scratch, &mut path, key, "STEP annotation alias path")?;
                    key = (child, key.1 + 1);
                },
            }
        };
        if empty_at_any_depth {
            self.storage.with_storage(|| ctx.insert_hash_map(&mut self.aliases, (key.0, 0, placement), None, "STEP annotation aliases"))?;
        }
        let mut source_rows = path.into_iter();
        for _ in 0..source_rows.len() {
            let source = ctx.next_charged(&mut source_rows, "STEP annotation alias installation")?.ok_or_else(|| CodecError::malformed("STEP annotation source ended early"))?;
            let depth = if empty_at_any_depth { 0 } else { source.1 };
            self.storage.with_storage(|| ctx.insert_hash_map(&mut self.aliases, (source.0, depth, placement), query, "STEP annotation aliases"))?;
        }
        Ok(query)
    }

    fn ensure(&mut self, id: u64, exchange: &Exchange, geometry: &GeometryData) -> Result<(),CodecError> {
        let ctx = self.ctx;
        let texts = &mut self.texts;
        let storage = &mut self.storage;
        self.graph.ensure(id, |id, sets, node_storage| {
            let mut local = [SetId::default();3];
            let mut children = Vec::new();
            let Some(record) = ctx.get_btree_map(exchange.records(),&id,"STEP annotation graph record")? else { return Ok(RecordCarriers { local,children }); };
            if record.partial(ctx,"TEXT_LITERAL")?.and_then(|partial|partial.parameters.first()).is_some()
                || record.partial(ctx,"TEXT_LITERAL_WITH_ASSOCIATED_CURVES")?.and_then(|partial|partial.parameters.first()).is_some() {
                Self::cache_text(id,exchange,texts,storage,ctx)?;
                let cached = ctx.get_hash_map(texts,&id,"STEP annotation text cache lookup")?.expect("cached carrier");
                if cached.text.is_some() { local[0] = CarrierSets::leaf(id); }
                if !cached.losses.is_empty() { local[1] = CarrierSets::leaf(id); }
            }
            visit_typed_placement_candidates(record,geometry,ctx,|placement,_| {
                local[2] = sets.union(local[2],CarrierSets::leaf(placement))?;
                Ok(())
            })?;
            let mut partials = record.partials.iter();
            for _ in 0..partials.len() {
                let partial = ctx.next_charged(&mut partials,"STEP annotation graph partial traversal")?.ok_or_else(||CodecError::malformed("STEP annotation partial source ended early"))?;
                let mut parameters = partial.parameters.iter();
                for _ in 0..parameters.len() {
                    let value = ctx.next_charged(&mut parameters,"STEP annotation graph parameter traversal")?.ok_or_else(||CodecError::malformed("STEP annotation parameter source ended early"))?;
                    for child in references(value,ctx) { ctx.push_scoped_vec(node_storage,&mut children,(child?,1),"STEP annotation graph edges")?; }
                }
            }
            Ok(RecordCarriers { local,children })
        })
    }

    fn cache_text(id: u64, exchange: &Exchange, texts: &mut HashMap<u64,CachedText>, storage: &mut ScopedReservation<'_>, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        if ctx.contains_key_hash_map(texts, &id, "STEP annotation text cache lookup")? {
            return Ok(());
        }
        let record = ctx
            .get_btree_map(exchange.records(), &id, "STEP annotation text record")?
            .ok_or_else(|| CodecError::malformed("STEP annotation text carrier is missing"))?;
        let value = record
            .partial(ctx, "TEXT_LITERAL")?
            .and_then(|partial| partial.parameters.first())
            .map_or_else(
                || -> Result<_, CodecError> {
                    Ok(record
                        .partial(ctx, "TEXT_LITERAL_WITH_ASSOCIATED_CURVES")?
                        .and_then(|partial| partial.parameters.first()))
                },
                |value| Ok(Some(value)),
            )?
            .ok_or_else(|| CodecError::malformed("STEP annotation text value is missing"))?;
        storage.with_storage(|| {
            let mut losses = Vec::new();
            let text = super::super::decode_text_charged(
                exchange,
                value,
                &mut losses,
                id,
                "PMI annotation text",
                StepLossCode::MetadataStringInvalid,
                ctx,
            )?;
            ctx.insert_hash_map(
                texts,
                id,
                CachedText { text, losses },
                "STEP annotation text cache",
            )
        })?;
        Ok(())
    }

    fn collect_warnings(&mut self, (id,_depth): (u64,usize), (visited,visited_storage): (&mut HashSet<u64>,&mut ScopedReservation<'_>), (summary,summary_storage): (&mut TextSummary,&mut ScopedReservation<'_>)) -> Result<(),CodecError> {
        let ctx = self.ctx;
        let mut scratch = ctx.reserve_scoped(0,"STEP annotation diagnostic cursor scratch")?;
        let mut jobs = Vec::new();
        ctx.push_scoped_vec(&mut scratch,&mut jobs,WarningJob::Record(id),"STEP annotation diagnostic cursor")?;
        let mut emitted = SetId::default();
        while !jobs.is_empty() {
            ctx.charge_work(1,"STEP annotation diagnostic cursor traversal")?;
            match jobs.pop().expect("diagnostic job") {
                WarningJob::Record(id) => {
                    let reachable = self.graph.summary(id,256)?[1];
                    let combined = self.graph.sets.union(reachable,emitted)?;
                    if self.graph.sets.count(combined) == self.graph.sets.count(emitted) { continue; }
                    if !visited_storage.with_storage(||ctx.insert_hash_set(visited,id,"STEP annotation diagnostic visited"))? { continue; }
                    let local = self.graph.local(id)?[1];
                    if let Some(carrier) = self.graph.sets.first(local) {
                        ctx.push_scoped_vec(summary_storage,&mut summary.warnings,carrier,"STEP annotation diagnostic carriers")?;
                        emitted = self.graph.sets.union(emitted,local)?;
                    }
                    let count = self.graph.children(id)?.len();
                    if count != 0 {
                        if !ctx.contains_key_hash_map(&self.warning_ranges,&id,"STEP annotation warning ranges lookup")? {
                            let mut ranges = Vec::new();
                            warning_ranges(&mut self.graph,id,0,count,&mut ranges,&mut self.storage,ctx)?;
                            self.storage.with_storage(||ctx.insert_hash_map(&mut self.warning_ranges,id,ranges,"STEP annotation warning ranges"))?;
                        }
                        let index = ctx.get_hash_map(&self.warning_ranges,&id,"STEP annotation warning ranges lookup")?.expect("warning ranges").len()-1;
                        ctx.push_scoped_vec(&mut scratch,&mut jobs,WarningJob::Range(id,index),"STEP annotation diagnostic cursor")?;
                    }
                },
                WarningJob::Range(id,index) => {
                    let range = ctx.get_hash_map(&self.warning_ranges,&id,"STEP annotation warning ranges lookup")?.expect("warning ranges")[index];
                    let combined = self.graph.sets.union(range.warnings(),emitted)?;
                    if self.graph.sets.count(combined) == self.graph.sets.count(emitted) { continue; }
                    match range {
                        WarningRange::Record { id,.. } => ctx.push_scoped_vec(&mut scratch,&mut jobs,WarningJob::Record(id),"STEP annotation diagnostic cursor")?,
                        WarningRange::Branch { left,right,.. } => {
                            ctx.push_scoped_vec(&mut scratch,&mut jobs,WarningJob::Range(id,right),"STEP annotation diagnostic cursor")?;
                            ctx.push_scoped_vec(&mut scratch,&mut jobs,WarningJob::Range(id,left),"STEP annotation diagnostic cursor")?;
                        },
                    }
                },
            }
        }
        Ok(())
    }

    fn collect_cutoff_summary(&mut self, (id, depth): (u64, usize), (visited, visited_storage): (&mut HashSet<u64>, &mut ScopedReservation<'_>), (selected, summary, summary_storage): (&mut SetId, &mut TextSummary, &mut ScopedReservation<'_>)) -> Result<(), CodecError> {
        if depth >= 256 { return Ok(()); }
        let ctx = self.ctx;
        let _nested = ctx.enter_nested("step_pmi_annotation_text_walk")?;
        let mut scratch = ctx.reserve_scoped(0,"STEP annotation cutoff frames scratch")?;
        let mut frames = Vec::new();
        ctx.push_scoped_vec(&mut scratch,&mut frames,(id,depth,None::<usize>),"STEP annotation cutoff frames")?;
        while !frames.is_empty() {
            ctx.charge_work(1,"STEP annotation cutoff frame traversal")?;
            let (id,depth,next) = frames.pop().expect("cutoff frame");
            if let Some(index) = next {
                if let Some(&(child,_)) = self.graph.children(id)?.get(index) {
                    ctx.charge_work(1,"STEP annotation cutoff edge traversal")?;
                    ctx.push_scoped_vec(&mut scratch,&mut frames,(id,depth,Some(index+1)),"STEP annotation cutoff frames")?;
                    ctx.push_scoped_vec(&mut scratch,&mut frames,(child,depth+1,None),"STEP annotation cutoff frames")?;
                }
                continue;
            }
            if depth >= 256 || ctx.contains_hash_set(visited,&id,"STEP annotation cutoff visited lookup")? { continue; }
            visited_storage.with_storage(||ctx.insert_hash_set(visited,id,"STEP annotation cutoff visited"))?;
            let local = self.graph.local(id)?;
            *selected = self.graph.sets.union(*selected,local[0])?;
            if let Some(carrier) = self.graph.sets.first(local[1]) {
                ctx.push_scoped_vec(summary_storage,&mut summary.warnings,carrier,"STEP annotation diagnostic carriers")?;
            }
            ctx.push_scoped_vec(&mut scratch,&mut frames,(id,depth,Some(0)),"STEP annotation cutoff frames")?;
        }
        Ok(())
    }

    fn collect_text_summary(&mut self, key: (u64,usize), exchange: &Exchange, geometry: &GeometryData, (visited,visited_storage): (&mut HashSet<u64>,&mut ScopedReservation<'_>), (summary,summary_storage): (&mut TextSummary,&mut ScopedReservation<'_>)) -> Result<(),CodecError> {
        self.ensure(key.0,exchange,geometry)?;
        // First-visit depth is observable when a path reaches the cutoff.
        // Keep that traversal over the shared decoded graph, and cache its
        // selection and diagnostic order for this query.
        let mut selected = SetId::default();
        if self.graph.height(key.0)?.saturating_add(key.1) >= 256 {
            self.collect_cutoff_summary(key, (visited, visited_storage), (&mut selected, summary, summary_storage))?;
        } else {
            let carriers = self.graph.summary(key.0,256-key.1)?;
            selected = carriers[0];
            if carriers[1] != SetId::Empty { self.collect_warnings(key,(visited,visited_storage),(summary,summary_storage))?; }
        }
        summary.selection = match self.graph.sets.count(selected) {
            0 => TextSelection::Absent,
            1 => TextSelection::Unique(self.graph.sets.first(selected).expect("one text carrier")),
            count => TextSelection::Ambiguous(count),
        };
        Ok(())
    }

    pub(super) fn text(
        &mut self,
        id: u64,
        exchange: &Exchange,
        geometry: &GeometryData,
        (used, claims): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
        (losses, reports): (
            &mut Vec<LossNote>,
            &std::cell::RefCell<ScopedReservation<'_>>,
        ),
    ) -> Result<Option<String>, CodecError> {
        let ctx = self.ctx;
        let Some(key) = self.canonical((id, 0), false, exchange, geometry)? else { return Ok(None); };
        if !ctx.contains_key_hash_map(&self.summaries, &key, "STEP annotation summary lookup")? {
            let mut visited_storage =
                ctx.reserve_scoped(0, "STEP annotation summary visited scratch")?;
            let mut summary_storage = ctx.reserve_scoped(0, "STEP annotation summary scratch")?;
            let mut summary = TextSummary::default();
            let mut visited = HashSet::new();
            self.collect_text_summary(
                key,
                exchange,
                geometry,
                (&mut visited, &mut visited_storage),
                (&mut summary, &mut summary_storage),
            )?;
            drop(visited);
            drop(visited_storage);
            self.storage.absorb(&mut summary_storage)?;
            self.storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut self.summaries,
                    key,
                    summary,
                    "STEP annotation summaries",
                )
            })?;
        }
        let summary = ctx
            .get_hash_map(&self.summaries, &key, "STEP annotation summary lookup")?
            .ok_or_else(|| CodecError::malformed("STEP annotation summary is missing"))?;
        let mut source_rows = (&summary.warnings).iter();
        for _ in 0..source_rows.len() {
            let carrier = ctx.next_charged(&mut source_rows, "STEP annotation diagnostic replay")?.ok_or_else(|| CodecError::malformed("STEP annotation source ended early"))?;
            let cached = ctx
                .get_hash_map(&self.texts, carrier, "STEP annotation text cache lookup")?
                .ok_or_else(|| {
                    CodecError::malformed("STEP annotation diagnostic carrier is missing")
                })?;
            let mut source_rows = (&cached.losses).iter();
        for _ in 0..source_rows.len() {
            let loss = ctx.next_charged(&mut source_rows, "STEP annotation loss replay")?.ok_or_else(|| CodecError::malformed("STEP annotation source ended early"))?;
                let loss = loss.try_clone_for_decode(ctx, "STEP annotation cached loss copy")?;
                ctx.push_scoped_vec(&mut reports.borrow_mut(), losses, loss, "step_pmi_losses")?;
            }
        }
        match summary.selection {
            TextSelection::Absent => Ok(None),
            TextSelection::Unique(carrier) => {
                claims.with_storage(|| {
                    ctx.insert_btree_set(used, carrier, "step_pmi_annotation_text_used")
                })?;
                let text = ctx
                    .get_hash_map(&self.texts, &carrier, "STEP annotation text cache lookup")?
                    .and_then(|cached| cached.text.as_deref())
                    .ok_or_else(|| {
                        CodecError::malformed("STEP selected annotation text is missing")
                    })?;
                Ok(Some(ctx.copy_retained_text(text, "step_string_text")?))
            }
            TextSelection::Ambiguous(count) => {
                let message = ctx.format_retained(format_args!("presentation annotation #{id} has {count} reachable text carriers with no ordered composition"), "step_annotation_unordered_text")?;
                ctx.push_scoped_vec(
                    &mut reports.borrow_mut(),
                    losses,
                    StepLossCode::PresentationAnnotationTextUnordered.note(message),
                    "step_pmi_losses",
                )?;
                Ok(None)
            }
        }
    }

    fn select_placement(&self, selected: SetId, geometry: &GeometryData) -> Result<PlacementSelection,CodecError> {
        Ok(match self.graph.sets.count(selected) {
            0 => PlacementSelection::Absent,
            1 => {
                let carrier = self.graph.sets.first(selected).expect("one placement carrier");
                let placement = self.ctx.get_btree_map(&geometry.placements,&carrier,"STEP pmi geometry.placements get")?.expect("admitted placement");
                PlacementSelection::Unique(super::super::geometry::placement_transform(*placement).expect("admitted placement transform"))
            },
            count => PlacementSelection::Ambiguous(count),
        })
    }

    pub(super) fn placement(&mut self, record: &RawRecord, exchange: &Exchange, geometry: &GeometryData) -> Result<PlacementSelection,CodecError> {
        let ctx = self.ctx;
        match record_references(record,ctx)? {
            RecordReferences::None => Ok(PlacementSelection::Absent),
            RecordReferences::One(id) => {
                let Some(key) = self.canonical((id,0),true,exchange,geometry)? else { return Ok(PlacementSelection::Absent); };
                if !ctx.contains_key_hash_map(&self.placements,&key,"STEP annotation placement summary lookup")? {
                    self.ensure(key.0,exchange,geometry)?;
                    let selected = self.graph.summary(key.0,256-key.1)?[2];
                    let summary = self.select_placement(selected,geometry)?;
                    self.storage.with_storage(||ctx.insert_hash_map(&mut self.placements,key,summary,"STEP annotation placement summaries"))?;
                }
                Ok(*ctx.get_hash_map(&self.placements,&key,"STEP annotation placement summary lookup")?.expect("placement summary"))
            },
            RecordReferences::Many => {
                let mut selected = SetId::default();
                let mut partials = record.partials.iter();
                for _ in 0..partials.len() {
                    let partial = ctx.next_charged(&mut partials,"STEP annotation placement partial traversal")?.ok_or_else(||CodecError::malformed("STEP annotation partial source ended early"))?;
                    let mut parameters = partial.parameters.iter();
                    for _ in 0..parameters.len() {
                        let value = ctx.next_charged(&mut parameters,"STEP annotation placement parameter traversal")?.ok_or_else(||CodecError::malformed("STEP annotation parameter source ended early"))?;
                        for reference in references(value,ctx) {
                            let child = reference?;
                            self.ensure(child,exchange,geometry)?;
                            let child = self.graph.summary(child,256)?[2];
                            selected = self.graph.sets.union(selected,child)?;
                        }
                    }
                }
                self.select_placement(selected,geometry)
            },
        }
    }
}

#[cfg(test)]
mod tests;
