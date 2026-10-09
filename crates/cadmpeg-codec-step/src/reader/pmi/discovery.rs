// SPDX-License-Identifier: Apache-2.0
//! Scoped annotation text and placement discovery.

use super::super::geometry::GeometryData;
use super::super::reference::references;
use super::super::RecordExt;
use super::{
    collect_placement_candidates, collect_typed_placement_candidates, find_annotation_text,
};
use crate::loss::StepLossCode;
use crate::parse::Exchange;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;
use std::collections::{BTreeMap, BTreeSet};

struct AnnotationGraph {
    // First-visit DFS order preserves invalid-string warning order.
    text_carriers: Vec<u64>,
    placements: BTreeMap<u64, Transform>,
    // An independent cyclic query can be reused only when it reaches none
    // of the caller's active ancestors.
    cyclic_queries: BTreeSet<(u64, usize)>,
}

type IndexedAnnotationGraph<'ctx> = (Option<AnnotationGraph>, ScopedReservation<'ctx>);

struct CachedAnnotationText<'ctx> {
    text: Option<String>,
    losses: Vec<LossNote>,
    _storage: ScopedReservation<'ctx>,
}

pub(super) struct AnnotationDiscoveryIndex<'ctx> {
    independent_reach: BTreeMap<(u64, usize), BTreeSet<u64>>,
    graphs: BTreeMap<(u64, usize), IndexedAnnotationGraph<'ctx>>,
    texts: BTreeMap<u64, CachedAnnotationText<'ctx>>,
    storage: ScopedReservation<'ctx>,
}

fn index_annotation_graph<'ctx>(
    id: u64,
    depth: usize,
    exchange: &Exchange,
    geometry: &GeometryData,
    active: &mut BTreeSet<u64>,
    index: &mut AnnotationDiscoveryIndex<'ctx>,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if depth >= 256 || ctx.contains_btree_set(active, &id, "STEP annotation graph active lookup")? {
        return Ok(false);
    }
    if let Some((graph, _)) =
        ctx.get_btree_map(&index.graphs, &(id, depth), "STEP annotation graph lookup")?
    {
        if graph.is_some() || active.len() > 1 {
            return annotation_graph_reusable(
                graph.as_ref(),
                active,
                &index.independent_reach,
                ctx,
            );
        }
    }
    let _nested = ctx.enter_nested("step_annotation_graph_walk")?;
    let (_, _active_storage) = ctx
        .with_scoped_storage("STEP annotation graph active scratch", || {
            ctx.insert_btree_set(active, id, "step_annotation_graph_active")
        })?;
    let (graph_buffer, storage) =
        ctx.with_scoped_storage("STEP annotation graph result scratch", || {
            let mut graph = AnnotationGraph {
                text_carriers: Vec::new(),
                placements: BTreeMap::new(),
                cyclic_queries: BTreeSet::new(),
            };
            let mut reusable = true;
            let mut seen_storage =
                ctx.reserve_scoped(0, "STEP annotation carrier dedup scratch")?;
            let mut text_seen = BTreeSet::new();
            if let Some(record) =
                ctx.get_btree_map(exchange.records(), &id, "STEP pmi record get")?
            {
                let text = record
                    .partial(ctx, "TEXT_LITERAL")?
                    .and_then(|partial| partial.parameters.first())
                    .map_or_else(
                        || -> Result<_, CodecError> {
                            Ok(record
                                .partial(ctx, "TEXT_LITERAL_WITH_ASSOCIATED_CURVES")?
                                .and_then(|partial| partial.parameters.first()))
                        },
                        |value| Ok(Some(value)),
                    )?;
                if text.is_some() {
                    ctx.push_vec(
                        &mut graph.text_carriers,
                        id,
                        "step_annotation_graph_text_carriers",
                    )?;
                    seen_storage.with_storage(|| {
                        ctx.insert_btree_set(&mut text_seen, id, "step_annotation_graph_text_seen")
                    })?;
                }
                collect_typed_placement_candidates(record, geometry, &mut graph.placements, ctx)?;
                for partial in ctx.admit_iter(
                    &record.partials[..],
                    "STEP annotation graph partial traversal",
                )? {
                    for parameter in ctx.admit_iter(
                        partial.parameters.as_slice(),
                        "STEP annotation graph parameter traversal",
                    )? {
                        for reference in references(parameter, ctx) {
                            let reference = reference?;
                            if !index_annotation_graph(
                                reference,
                                depth + 1,
                                exchange,
                                geometry,
                                active,
                                index,
                                ctx,
                            )? {
                                reusable = false;
                                continue;
                            }
                            let (child, _) = ctx
                                .get_btree_map(
                                    &index.graphs,
                                    &(reference, depth + 1),
                                    "STEP annotation graph lookup",
                                )?
                                .ok_or_else(|| {
                                    CodecError::malformed("STEP annotation child was not indexed")
                                })?;
                            let child = child.as_ref().ok_or_else(|| {
                                CodecError::malformed("STEP annotation child is incomplete")
                            })?;
                            for &carrier in ctx.admit_iter(
                                &child.text_carriers,
                                "STEP annotation text carrier merge",
                            )? {
                                if seen_storage.with_storage(|| {
                                    ctx.insert_btree_set(
                                        &mut text_seen,
                                        carrier,
                                        "step_annotation_graph_text_seen",
                                    )
                                })? {
                                    ctx.push_vec(
                                        &mut graph.text_carriers,
                                        carrier,
                                        "step_annotation_graph_text_carriers",
                                    )?;
                                }
                            }
                            for (&carrier, &transform) in ctx
                                .admit_iter(&child.placements, "STEP annotation placement merge")?
                            {
                                ctx.insert_btree_map(
                                    &mut graph.placements,
                                    carrier,
                                    transform,
                                    "step_pmi_placement_candidates",
                                )?;
                            }
                            for &query in ctx.admit_iter(
                                &child.cyclic_queries,
                                "STEP annotation cyclic query merge",
                            )? {
                                ctx.insert_btree_set(
                                    &mut graph.cyclic_queries,
                                    query,
                                    "step_annotation_cyclic_queries",
                                )?;
                            }
                        }
                    }
                }
            }
            Ok::<_, CodecError>(reusable.then_some(graph))
        })?;
    let graph = graph_buffer;
    ctx.remove_btree_set(active, &id, "STEP annotation graph active remove")?;
    let (selected_graph, selected_storage) = if graph.is_some() {
        (graph, storage)
    } else if active.len() <= 1 {
        drop(storage);
        independent_annotation_graph(id, depth, exchange, geometry, index, ctx)?
    } else {
        drop(storage);
        (
            None,
            ctx.reserve_scoped(0, "STEP incomplete annotation graph")?,
        )
    };
    let storage = selected_storage;
    let graph = selected_graph;
    let reusable =
        annotation_graph_reusable(graph.as_ref(), active, &index.independent_reach, ctx)?;
    index.storage.with_storage(|| {
        ctx.insert_btree_map(
            &mut index.graphs,
            (id, depth),
            (graph, storage),
            "step_annotation_graph_entries",
        )
    })?;
    Ok(reusable)
}

fn annotation_graph_reusable(
    graph: Option<&AnnotationGraph>,
    active: &BTreeSet<u64>,
    reach: &BTreeMap<(u64, usize), BTreeSet<u64>>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(graph) = graph else {
        return Ok(false);
    };
    if graph.cyclic_queries.is_empty() {
        return Ok(true);
    }
    ctx.all_by(
        &graph.cyclic_queries,
        |query| {
            let nodes = ctx
                .get_btree_map(reach, query, "STEP independent annotation reach lookup")?
                .ok_or_else(|| {
                    CodecError::malformed("STEP independent annotation reach is missing")
                })?;
            ctx.all_by(
                active,
                |ancestor| {
                    Ok(!ctx.contains_btree_set(
                        nodes,
                        ancestor,
                        "STEP annotation cyclic ancestor lookup",
                    )?)
                },
                "STEP annotation cyclic ancestor traversal",
            )
        },
        "STEP annotation cyclic query traversal",
    )
}

fn independent_annotation_graph<'ctx>(
    id: u64,
    depth: usize,
    exchange: &Exchange,
    geometry: &GeometryData,
    index: &mut AnnotationDiscoveryIndex<'ctx>,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<IndexedAnnotationGraph<'ctx>, CodecError> {
    let (graph_reach_buffer, storage) =
        ctx.with_scoped_storage("STEP independent annotation graph scratch", || {
            let mut reach = BTreeSet::new();
            let mut carriers = Vec::new();
            let mut complete = true;
            annotation_graph_text_carriers(
                id,
                depth,
                exchange,
                &mut reach,
                &mut carriers,
                &mut complete,
                ctx,
            )?;
            if !complete {
                return Ok((None, BTreeSet::new()));
            }
            let mut placements = BTreeMap::new();
            // A complete text walk visits every reachable record. Placement
            // discovery has the same reachable set and no source-order choice.
            for &node in ctx.admit_iter(&reach, "STEP independent placement record traversal")? {
                if let Some(record) =
                    ctx.get_btree_map(exchange.records(), &node, "STEP pmi record get")?
                {
                    collect_typed_placement_candidates(record, geometry, &mut placements, ctx)?;
                }
            }
            let mut cyclic_queries = BTreeSet::new();
            ctx.insert_btree_set(
                &mut cyclic_queries,
                (id, depth),
                "step_annotation_cyclic_queries",
            )?;
            Ok::<_, CodecError>((
                Some(AnnotationGraph {
                    text_carriers: carriers,
                    placements,
                    cyclic_queries,
                }),
                reach,
            ))
        })?;
    let (graph, reach) = graph_reach_buffer;
    if graph.is_some() {
        // This reach set and its owning query reservation both live in the
        // stage index. Parent summaries retain only its fixed query key.
        index.storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut index.independent_reach,
                (id, depth),
                reach,
                "step_independent_annotation_reach",
            )
        })?;
        Ok((graph, storage))
    } else {
        drop(storage);
        Ok((
            None,
            ctx.reserve_scoped(0, "STEP incomplete annotation graph")?,
        ))
    }
}

fn annotation_graph_text_carriers(
    id: u64,
    depth: usize,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    carriers: &mut Vec<u64>,
    complete: &mut bool,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let (pending_buffer, mut pending_storage) =
        ctx.temporary_vec(0, "STEP independent annotation worklist")?;
    let mut pending = pending_buffer;
    ctx.push_scoped_vec(
        &mut pending_storage,
        &mut pending,
        (id, depth),
        "STEP independent annotation worklist",
    )?;
    while let Some((id, depth)) = pending.pop() {
        ctx.charge_work(1, "STEP independent annotation worklist step")?;
        if depth >= 256 {
            *complete = false;
            continue;
        }
        if !ctx.insert_btree_set(visited, id, "step_pmi_annotation_text_visited")? {
            continue;
        }
        let Some(record) = ctx.get_btree_map(exchange.records(), &id, "STEP pmi record get")?
        else {
            continue;
        };
        let text = record
            .partial(ctx, "TEXT_LITERAL")?
            .and_then(|partial| partial.parameters.first())
            .map_or_else(
                || -> Result<_, CodecError> {
                    Ok(record
                        .partial(ctx, "TEXT_LITERAL_WITH_ASSOCIATED_CURVES")?
                        .and_then(|partial| partial.parameters.first()))
                },
                |value| Ok(Some(value)),
            )?;
        if text.is_some() {
            ctx.push_vec(carriers, id, "step_annotation_graph_text_carriers")?;
        }
        let (children_buffer, mut child_storage) =
            ctx.temporary_vec(0, "STEP independent annotation children")?;
        let mut children = children_buffer;
        for partial in ctx.admit_iter(
            &record.partials[..],
            "STEP independent annotation partial traversal",
        )? {
            for parameter in ctx.admit_iter(
                partial.parameters.as_slice(),
                "STEP independent annotation parameter traversal",
            )? {
                for reference in references(parameter, ctx) {
                    ctx.push_scoped_vec(
                        &mut child_storage,
                        &mut children,
                        reference?,
                        "STEP independent annotation children",
                    )?;
                }
            }
        }
        for &child in ctx
            .admit_iter(&children, "STEP independent annotation children traversal")?
            .rev()
        {
            ctx.push_scoped_vec(
                &mut pending_storage,
                &mut pending,
                (child, depth + 1),
                "STEP independent annotation worklist",
            )?;
        }
    }
    Ok(())
}

fn cache_annotation_text<'ctx>(
    id: u64,
    exchange: &Exchange,
    texts: &mut BTreeMap<u64, CachedAnnotationText<'ctx>>,
    storage: &mut ScopedReservation<'ctx>,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<(), CodecError> {
    if ctx.contains_key_btree_map(texts, &id, "STEP annotation text cache lookup")? {
        return Ok(());
    }
    let record = ctx
        .get_btree_map(exchange.records(), &id, "STEP pmi record get")?
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
    let (text_losses_buffer, text_storage) =
        ctx.with_scoped_storage("STEP cached annotation text scratch", || {
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
            Ok::<_, CodecError>((text, losses))
        })?;
    let (text, losses) = text_losses_buffer;
    storage.with_storage(|| {
        ctx.insert_btree_map(
            texts,
            id,
            CachedAnnotationText {
                text,
                losses,
                _storage: text_storage,
            },
            "step_annotation_text_cache",
        )
    })?;
    Ok(())
}

fn indexed_annotation_text<'ctx>(
    id: u64,
    exchange: &Exchange,
    index: &mut AnnotationDiscoveryIndex<'ctx>,
    (used, claim_storage): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<ScopedReservation<'_>>,
    ),
    ctx: &'ctx DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let (graph, _) = ctx
        .get_btree_map(&index.graphs, &(id, 0), "STEP annotation graph lookup")?
        .ok_or_else(|| CodecError::malformed("STEP annotation graph was not indexed"))?;
    let graph = graph
        .as_ref()
        .ok_or_else(|| CodecError::malformed("STEP annotation graph is incomplete"))?;
    let mut selected = None;
    let mut count = 0;
    for &carrier in ctx.admit_iter(
        &graph.text_carriers,
        "STEP indexed annotation text traversal",
    )? {
        cache_annotation_text(carrier, exchange, &mut index.texts, &mut index.storage, ctx)?;
        let cached = ctx
            .get_btree_map(&index.texts, &carrier, "STEP annotation text cache lookup")?
            .ok_or_else(|| CodecError::malformed("STEP annotation text was not indexed"))?;
        for loss in ctx.admit_iter(&cached.losses, "STEP cached annotation loss traversal")? {
            let loss = loss.try_clone_for_decode(ctx, "step_annotation_cached_loss_copy")?;
            ctx.push_scoped_vec(
                &mut slot_storage.borrow_mut(),
                losses,
                loss,
                "step_pmi_losses",
            )?;
        }
        if cached.text.is_some() {
            count += 1;
            selected = Some(carrier);
        }
    }
    match (count, selected) {
        (0, _) => Ok(None),
        (1, Some(carrier)) => {
            claim_storage.with_storage(|| {
                ctx.insert_btree_set(used, carrier, "step_pmi_annotation_text_used")
            })?;
            let text = ctx
                .get_btree_map(&index.texts, &carrier, "STEP annotation text cache lookup")?
                .and_then(|cached| cached.text.as_deref())
                .ok_or_else(|| CodecError::malformed("STEP selected annotation text is missing"))?;
            Ok(Some(ctx.copy_retained_text(text, "step_string_text")?))
        }
        _ => {
            let message = ctx.format_retained(format_args!("presentation annotation #{id} has {count} reachable text carriers with no ordered composition"), "step_annotation_unordered_text")?;
            ctx.push_scoped_vec(
                &mut slot_storage.borrow_mut(),
                losses,
                StepLossCode::PresentationAnnotationTextUnordered.note(message),
                "step_pmi_losses",
            )?;
            Ok(None)
        }
    }
}

impl<'ctx> AnnotationDiscoveryIndex<'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            graphs: BTreeMap::new(),
            texts: BTreeMap::new(),
            independent_reach: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "STEP annotation discovery index scratch")?,
        })
    }

    pub(super) fn text(
        &mut self,
        id: u64,
        exchange: &Exchange,
        geometry: &GeometryData,
        claims: (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
        losses: (
            &mut Vec<LossNote>,
            &std::cell::RefCell<ScopedReservation<'_>>,
        ),
        ctx: &'ctx DecodeContext<'_>,
    ) -> Result<Option<String>, CodecError> {
        if index_annotation_graph(id, 0, exchange, geometry, &mut BTreeSet::new(), self, ctx)? {
            indexed_annotation_text(id, exchange, self, claims, losses, ctx)
        } else {
            find_annotation_text(id, exchange, &mut BTreeSet::new(), claims, losses, 0, ctx)
        }
    }

    pub(super) fn placements(
        &mut self,
        id: u64,
        exchange: &Exchange,
        geometry: &GeometryData,
        visited: &mut BTreeMap<u64, usize>,
        (candidates, storage): (&mut BTreeMap<u64, Transform>, &mut ScopedReservation<'_>),
        ctx: &'ctx DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        if index_annotation_graph(id, 0, exchange, geometry, &mut BTreeSet::new(), self, ctx)? {
            let (graph, _) = ctx
                .get_btree_map(&self.graphs, &(id, 0), "STEP annotation graph lookup")?
                .ok_or_else(|| CodecError::malformed("STEP annotation graph was not indexed"))?;
            let graph = graph
                .as_ref()
                .ok_or_else(|| CodecError::malformed("STEP annotation graph is incomplete"))?;
            for (&carrier, &transform) in
                ctx.admit_iter(&graph.placements, "STEP indexed placement traversal")?
            {
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        candidates,
                        carrier,
                        transform,
                        "step_pmi_placement_candidates",
                    )
                })?;
            }
            Ok(())
        } else {
            storage.with_storage(|| {
                collect_placement_candidates(id, exchange, geometry, visited, candidates, 0, ctx)
            })
        }
    }
}

#[cfg(test)]
mod tests;
