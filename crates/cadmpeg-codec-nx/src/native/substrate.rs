// SPDX-License-Identifier: Apache-2.0
//! Two-view single-parse substrate for the expensive per-stream Parasolid scans.
//!
//! Decode geometry and native extractors read different byte views of each
//! Parasolid stream (delta-extended semantic bytes vs raw `stream.inflated`).
//! [`ParsedStreams`] holds both and shares one parse only when the views are
//! identical.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::decode::Scan;
use crate::deltas::census::Census;
use crate::intersection::{self, CurveScan};
use crate::parasolid::{Stream, StreamKind};
use crate::topology::{BlendSurface, Graph, OffsetSurface, SurfaceCurve, TrimmedCurve};

struct TopologyStream<'a> {
    bytes: Cow<'a, [u8]>,
    delta_census: Option<Census>,
}

/// The topology-merged bytes per stream: each stream's inflated bytes with delta
/// full-record merges applied. Unpaired delta streams that carry records or tombstones
/// are merged against an empty partition; paired delta streams are merged into their
/// partition and then cleared.
pub(crate) fn topology_streams<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a Scan<'_>,
) -> Result<Vec<Cow<'a, [u8]>>, CodecError> {
    let pairs = DeltaPairs::of(ctx, scan)?;
    let semantic = prepare_topology_streams(ctx, scan, &pairs, None)?;
    ctx.collect_vec(
        semantic.into_iter().map(|stream| stream.bytes),
        "nx topology byte views",
    )
}

/// Delta streams paired with partitions, plus the scoped set of every paired delta.
struct DeltaPairs<'ctx> {
    by_partition: BTreeMap<usize, Vec<usize>>,
    paired: BTreeSet<usize>,
    _paired_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx> DeltaPairs<'ctx> {
    fn of(ctx: &'ctx DecodeContext<'_>, scan: &Scan) -> Result<Self, CodecError> {
        let by_partition = paired_delta_streams(ctx, scan)?;
        let mut paired_storage = ctx.reserve_scoped(0, "nx paired topology deltas")?;
        let mut paired = BTreeSet::new();
        for deltas in ctx
            .admit_iter(&by_partition, "nx paired topology deltas")?
            .map(|(_, deltas)| deltas)
        {
            for &delta in ctx.admit_iter(deltas, "nx paired topology deltas")? {
                paired_storage.with_storage(|| {
                    ctx.insert_btree_set(&mut paired, delta, "nx paired topology deltas")
                })?;
            }
        }
        Ok(Self {
            by_partition,
            paired,
            _paired_storage: paired_storage,
        })
    }
}

fn prepare_topology_streams<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a Scan<'_>,
    pairs: &DeltaPairs<'_>,
    mut unmatched_tombstone_counts: Option<&mut BTreeMap<&'static str, usize>>,
) -> Result<Vec<TopologyStream<'a>>, CodecError> {
    let mut semantic = ctx.collect_vec(
        ctx.admit_iter(&scan.streams, "nx prepared topology streams")?
            .map(|stream| TopologyStream {
                bytes: Cow::Borrowed(stream.inflated.as_slice()),
                delta_census: None,
            }),
        "nx prepared topology streams",
    )?;
    let mut merge =
        |partition: &[u8], deltas: &[u8], census: &Census| -> Result<Vec<u8>, CodecError> {
            if let Some(totals) = unmatched_tombstone_counts.as_deref_mut() {
                let result = crate::deltas::merge_full_records_with_tombstone_census(
                    ctx, partition, deltas, census,
                )?;
                for (&family, &count) in ctx.admit_iter(
                    &result.unmatched_tombstones,
                    "NX unmatched tombstone family totals",
                )? {
                    match ctx.get_mut_btree_map(
                        totals,
                        family,
                        "NX unmatched tombstone family totals",
                    )? {
                        Some(total) => {
                            *total = total.checked_add(count).ok_or_else(|| {
                                ctx.refuse_codec_limit(
                                    "NX unmatched tombstone family totals",
                                    u64::MAX,
                                    u64::MAX,
                                )
                            })?;
                        }
                        None => {
                            ctx.insert_btree_map(
                                totals,
                                family,
                                count,
                                "NX unmatched tombstone family totals",
                            )?;
                        }
                    }
                }
                Ok(result.merged)
            } else {
                crate::deltas::merge_full_records_with_census(ctx, partition, deltas, census)
            }
        };
    for (delta, stream) in ctx
        .admit_iter(&scan.streams, "nx unpaired topology deltas")?
        .enumerate()
    {
        if stream.kind() == StreamKind::Deltas
            && !ctx.contains_btree_set(&pairs.paired, &delta, "nx paired topology deltas")?
        {
            let census = crate::deltas::census::walk(ctx, &stream.inflated)?;
            if !census.records.is_empty() || !census.tombstones.is_empty() {
                let merged = merge(&[], &stream.inflated, &census)?;
                semantic[delta].bytes = Cow::Owned(merged);
            }
            semantic[delta].delta_census = Some(census);
        }
    }
    for (&partition, deltas) in ctx.admit_iter(&pairs.by_partition, "nx paired topology merges")? {
        for &delta in ctx.admit_iter(deltas, "nx paired topology merges")? {
            let census = crate::deltas::census::walk(ctx, &semantic[delta].bytes)?;
            let merged = merge(&semantic[partition].bytes, &semantic[delta].bytes, &census)?;
            semantic[partition].bytes = Cow::Owned(merged);
            semantic[delta].bytes = Cow::Borrowed(&[]);
            semantic[delta].delta_census = Some(census);
        }
    }
    Ok(semantic)
}

/// Map each partition stream ordinal to the delta stream ordinals that pair with it,
/// restricting the delta candidates to those the segment stream links mark as `deltas`
/// when any links are present.
pub(super) fn paired_delta_streams(
    ctx: &DecodeContext<'_>,
    scan: &Scan,
) -> Result<BTreeMap<usize, Vec<usize>>, CodecError> {
    let mut has_links = false;
    let mut linked_reservation = ctx.reserve_scoped(0, "nx linked delta candidates")?;
    let mut linked_deltas = BTreeSet::new();
    for wrapper in scan.container.segment_stream_wrappers() {
        ctx.charge_work(1, "nx linked delta wrappers")?;
        let Some(ordinal) = ctx.position_by(
            &scan.streams,
            |stream| Ok(stream.file_offset == wrapper.zlib_offset),
            "nx linked delta stream matching",
        )?
        else {
            continue;
        };
        has_links = true;
        if scan.streams[ordinal].kind() == crate::parasolid::StreamKind::Deltas {
            linked_reservation.with_storage(|| {
                ctx.insert_btree_set(&mut linked_deltas, ordinal, "nx linked delta candidates")
            })?;
        }
    }
    pair_stream_indices(ctx, &scan.streams, has_links.then_some(&linked_deltas))
}

/// Pair each eligible delta stream with the nearest preceding partition stream of the
/// same schema. `eligible_deltas`, when `Some`, restricts pairing to those delta
/// ordinals; when `None`, every delta stream is eligible.
pub(super) fn pair_stream_indices(
    ctx: &DecodeContext<'_>,
    streams: &[Stream],
    eligible_deltas: Option<&BTreeSet<usize>>,
) -> Result<BTreeMap<usize, Vec<usize>>, CodecError> {
    let mut pairs = BTreeMap::<usize, Vec<usize>>::new();
    for (delta, stream) in ctx
        .admit_iter(streams, "nx delta partition pairs")?
        .enumerate()
    {
        if stream.kind() != StreamKind::Deltas {
            continue;
        }
        if let Some(eligible) = eligible_deltas {
            if !ctx.contains_btree_set(eligible, &delta, "nx linked delta candidates")? {
                continue;
            }
        }
        let schema = stream
            .schema_token()
            .map(cadmpeg_parasolid::OwnedSchemaToken::value);
        let mut partition = None;
        for (ordinal, candidate) in streams[..delta].iter().enumerate().rev() {
            ctx.charge_work(1, "nx delta partition scan")?;
            if candidate.kind() == StreamKind::Partition
                && ctx.equal(
                    &candidate
                        .schema_token()
                        .map(cadmpeg_parasolid::OwnedSchemaToken::value),
                    &schema,
                    "nx delta partition schema",
                )?
            {
                partition = Some(ordinal);
                break;
            }
        }
        let Some(partition) = partition else {
            continue;
        };
        let deltas = ctx
            .entry_btree_map(&mut pairs, partition, "nx delta pair partitions")?
            .or_default();
        ctx.push_vec(deltas, delta, "nx delta pair members")?;
    }
    Ok(pairs)
}

/// The cached parses of one Parasolid byte view of one stream.
///
/// `graph` is [`topology::Graph::parse`] of this view's graph bytes; the four scanner
/// vectors and `intersections` are the topology/intersection scans of this view's
/// bytes. For the raw view every field derives from `stream.inflated`. For the
/// semantic view `graph` derives from the topology-merged bytes and the scanners from
/// the delta-extended semantic bytes, matching the decode geometry path exactly.
pub(crate) struct StreamView {
    /// Topology record graph.
    pub(crate) graph: Rc<Graph>,
    /// Type-60 offset surfaces.
    pub(crate) offset_surfaces: Vec<OffsetSurface>,
    /// Type-56 rolling-ball blend surfaces.
    pub(crate) blend_surfaces: Vec<BlendSurface>,
    /// Type-133 trimmed curves.
    pub(crate) trimmed_curves: Vec<TrimmedCurve>,
    /// Type-137 surface curves.
    pub(crate) surface_curves: Vec<SurfaceCurve>,
    /// Intersection-construction scan.
    pub(crate) intersections: CurveScan,
}

impl StreamView {
    /// An all-empty view, used for non-Parasolid streams which neither consumer reads.
    fn empty() -> Self {
        StreamView {
            graph: Rc::new(Graph::default()),
            offset_surfaces: Vec::new(),
            blend_surfaces: Vec::new(),
            trimmed_curves: Vec::new(),
            surface_curves: Vec::new(),
            intersections: CurveScan::default(),
        }
    }

    /// Parse every cached family from a single byte buffer with the plain
    /// intersection scan. This is the raw view (`stream.inflated`); it is also the
    /// semantic view whenever the semantic bytes equal the raw bytes.
    fn parse_uniform(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        point_layout: crate::intersection::ChartPointLayout,
    ) -> Result<Self, CodecError> {
        let graph = Rc::new(Graph::parse(ctx, bytes)?);
        Ok(StreamView {
            offset_surfaces: graph.offset_surfaces(ctx)?,
            blend_surfaces: graph.blend_surfaces(ctx)?,
            trimmed_curves: graph.trimmed_curves(ctx)?,
            surface_curves: graph.surface_curves(ctx)?,
            intersections: intersection::scan_with_graph(ctx, bytes, &graph, point_layout)?,
            graph,
        })
    }

    /// Parse the semantic view: `graph` from the topology-merged bytes, the scanners
    /// from the delta-extended semantic bytes, and `intersections` via the
    /// auxiliary-replacement scan when this stream has paired delta streams.
    fn parse_semantic(
        ctx: &DecodeContext<'_>,
        graph: Rc<Graph>,
        semantic_bytes: &[u8],
        topology_len: usize,
        scan: &Scan,
        paired_deltas: Option<&Vec<usize>>,
        point_layout: crate::intersection::ChartPointLayout,
    ) -> Result<(Self, Rc<Graph>), CodecError> {
        let topology_bytes = semantic_bytes
            .get(..topology_len)
            .ok_or_else(|| CodecError::malformed("NX topology bytes exceed their semantic view"))?;
        // The topology bytes are a prefix of the semantic bytes.
        let semantic_graph = if topology_len == semantic_bytes.len() {
            None
        } else {
            Some(Rc::new(Graph::parse(ctx, semantic_bytes)?))
        };
        let scan_graph = semantic_graph.as_deref().unwrap_or(&graph);
        let nurbs_graph = semantic_graph
            .as_ref()
            .map_or_else(|| Rc::clone(&graph), Rc::clone);
        let intersections = if let Some(delta_indices) = paired_deltas {
            let mut replacement_streams = Vec::new();
            let mut _replacement_reservation =
                ctx.reserve_scoped(0, "nx auxiliary replacement views")?;
            for &delta in ctx.admit_iter(delta_indices, "nx auxiliary replacement views")? {
                _replacement_reservation.with_storage(|| {
                    ctx.push_vec(
                        &mut replacement_streams,
                        scan.streams[delta].inflated.as_slice(),
                        "nx auxiliary replacement views",
                    )
                })?;
            }
            intersection::scan_with_auxiliary_replacements_and_graph(
                ctx,
                semantic_bytes,
                topology_bytes,
                &replacement_streams,
                scan_graph,
            )?
        } else {
            intersection::scan_with_graph(ctx, semantic_bytes, scan_graph, point_layout)?
        };
        Ok((
            StreamView {
                offset_surfaces: scan_graph.offset_surfaces(ctx)?,
                blend_surfaces: scan_graph.blend_surfaces(ctx)?,
                trimmed_curves: scan_graph.trimmed_curves(ctx)?,
                surface_curves: scan_graph.surface_curves(ctx)?,
                intersections,
                graph,
            },
            nurbs_graph,
        ))
    }
}

/// The raw and semantic parses of one stream. The two views share an [`Rc`] when the
/// byte views are proven identical, so shared streams parse exactly once.
pub(crate) struct StreamParses {
    raw: Rc<StreamView>,
    semantic: Rc<StreamView>,
}

impl StreamParses {
    /// The view the native record extractors read: parses of `stream.inflated`.
    pub(super) fn view_for_records(&self) -> &StreamView {
        &self.raw
    }

    /// The view the decode geometry (IR) path reads: parses of the delta-extended
    /// semantic bytes.
    pub(crate) fn view_for_geometry(&self) -> &StreamView {
        &self.semantic
    }
}

/// One stream's shared parses, prepared bytes, and deferred geometry inputs.
struct StreamParse<'a> {
    views: StreamParses,
    semantic_bytes: Cow<'a, [u8]>,
    delta_census: Option<Census>,
    nurbs_graph: Rc<Graph>,
}

/// Every expensive per-stream Parasolid parse, once per distinct byte view,
/// indexed by stream ordinal. NURBS parsing remains deferred until requested.
pub(crate) struct ParsedStreams<'a> {
    streams: Vec<StreamParse<'a>>,
    unmatched_tombstone_counts: BTreeMap<&'static str, usize>,
}

impl<'a> ParsedStreams<'a> {
    /// Prepare the semantic and topology byte views, then parse each family needed by
    /// its consumer once per byte view. Non-Parasolid streams get empty views. A
    /// stream's raw and semantic views share one parse when the topology-merged and
    /// delta-extended byte views both equal `stream.inflated` and the stream has no
    /// auxiliary-replacement deltas. NURBS parsing is deferred until a geometry
    /// consumer requests the selected stream's geometry.
    pub(crate) fn parse(ctx: &DecodeContext<'_>, scan: &'a Scan) -> Result<Self, CodecError> {
        let mut unmatched_tombstone_counts = BTreeMap::new();
        let pairs = DeltaPairs::of(ctx, scan)?;
        let mut topology_streams =
            prepare_topology_streams(ctx, scan, &pairs, Some(&mut unmatched_tombstone_counts))?;

        let mut streams = Vec::new();
        for (si, stream) in ctx
            .admit_iter(&scan.streams, "nx parsed stream records")?
            .enumerate()
        {
            let mut semantic_bytes = std::mem::take(&mut topology_streams[si].bytes);
            let crate::parasolid::StreamBody::Parasolid { subtype, .. } = &stream.body else {
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<StreamView>()),
                    "nx empty parsed stream view",
                )?;
                let empty = Rc::new(StreamView::empty());
                let empty_graph = Rc::clone(&empty.graph);
                ctx.push_vec(
                    &mut streams,
                    StreamParse {
                        views: StreamParses {
                            raw: empty.clone(),
                            semantic: empty,
                        },
                        semantic_bytes,
                        delta_census: topology_streams[si].delta_census.take(),
                        nurbs_graph: empty_graph,
                    },
                    "nx parsed stream records",
                )?;
                continue;
            };
            let point_layout = subtype.chart_point_layout();
            let paired =
                ctx.get_btree_map(&pairs.by_partition, &si, "nx parsed stream paired deltas")?;
            let topology_matches_raw = ctx.equal_bytes(
                semantic_bytes.as_ref(),
                &stream.inflated,
                "nx raw topology comparison",
            )?;
            let mut residual = Vec::new();
            let mut residual_reservation =
                ctx.reserve_scoped(0, "nx semantic residual aggregation")?;
            let mut append_residual = |delta: usize| -> Result<(), CodecError> {
                let Some(census) = topology_streams[delta].delta_census.as_ref() else {
                    return Ok(());
                };
                let part = crate::deltas::semantic_residual_with_census(
                    ctx,
                    &scan.streams[delta].inflated,
                    census,
                )?;
                residual_reservation.with_storage(|| {
                    ctx.extend_retained_bytes(
                        &mut residual,
                        &part,
                        "nx semantic residual aggregation",
                    )
                })
            };
            if stream.kind() == StreamKind::Deltas
                && !ctx.contains_btree_set(&pairs.paired, &si, "nx parsed stream paired deltas")?
            {
                append_residual(si)?;
            }
            if let Some(deltas) = paired {
                for &delta in ctx.admit_iter(deltas, "nx semantic residual aggregation")? {
                    append_residual(delta)?;
                }
            }
            let identical = paired.is_none() && topology_matches_raw && residual.is_empty();
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<StreamView>()),
                "nx raw parsed stream view",
            )?;
            let raw = Rc::new(StreamView::parse_uniform(
                ctx,
                &stream.inflated,
                point_layout,
            )?);
            let (semantic, nurbs_graph) = if identical {
                (Rc::clone(&raw), Rc::clone(&raw.graph))
            } else {
                let graph = Rc::new(Graph::parse(ctx, &semantic_bytes)?);
                // Paired deltas scan auxiliary replacements against the topology bytes,
                // which stay the prefix of the semantic bytes before the residual.
                let topology_len = semantic_bytes.len();
                match &mut semantic_bytes {
                    Cow::Borrowed(bytes) => {
                        let total_len =
                            bytes.len().checked_add(residual.len()).ok_or_else(|| {
                                ctx.refuse_codec_limit("nx extended semantic topology", 0, u64::MAX)
                            })?;
                        let mut owned = Vec::new();
                        ctx.reserve_capacity(
                            &mut owned,
                            total_len,
                            "nx extended semantic topology",
                        )?;
                        ctx.extend_retained_bytes(
                            &mut owned,
                            bytes,
                            "nx extended semantic topology",
                        )?;
                        ctx.extend_retained_bytes(
                            &mut owned,
                            &residual,
                            "nx extended semantic topology",
                        )?;
                        semantic_bytes = Cow::Owned(owned);
                    }
                    Cow::Owned(bytes) => {
                        ctx.extend_retained_bytes(
                            bytes,
                            &residual,
                            "nx extended semantic topology",
                        )?;
                    }
                }
                let topology_len = if paired.is_some() {
                    topology_len
                } else {
                    semantic_bytes.len()
                };
                let (semantic, nurbs_graph) = StreamView::parse_semantic(
                    ctx,
                    graph,
                    &semantic_bytes,
                    topology_len,
                    scan,
                    paired,
                    point_layout,
                )?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<StreamView>()),
                    "nx semantic parsed stream view",
                )?;
                (Rc::new(semantic), nurbs_graph)
            };
            ctx.push_vec(
                &mut streams,
                StreamParse {
                    views: StreamParses { raw, semantic },
                    semantic_bytes,
                    delta_census: topology_streams[si].delta_census.take(),
                    nurbs_graph,
                },
                "nx parsed stream records",
            )?;
        }

        Ok(ParsedStreams {
            streams,
            unmatched_tombstone_counts,
        })
    }

    pub(crate) fn unmatched_tombstone_counts(&self) -> &BTreeMap<&'static str, usize> {
        &self.unmatched_tombstone_counts
    }

    /// Move the delta censuses into the native extractor after all semantic
    /// residuals have been built. Each delta walk is owned by one decode.
    pub(super) fn take_delta_censuses(
        &mut self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<Option<Census>>, CodecError> {
        let mut censuses = Vec::new();
        for index in ctx.admit_iter(&(0..self.streams.len()), "nx delta census slots")? {
            ctx.push_vec(
                &mut censuses,
                self.streams[index].delta_census.take(),
                "nx delta census slots",
            )?;
        }
        Ok(censuses)
    }

    /// The cached parses of the stream at `ordinal`.
    pub(crate) fn stream(&self, ordinal: usize) -> &StreamParses {
        &self.streams[ordinal].views
    }

    /// Iterate `(ordinal, parses)` over every stream.
    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &StreamParses)> {
        self.streams
            .iter()
            .enumerate()
            .map(|(ordinal, stream)| (ordinal, &stream.views))
    }

    /// Parse NURBS geometry for the selected semantic stream when requested.
    pub(crate) fn parse_nurbs(
        &self,
        ctx: &DecodeContext<'_>,
        ordinal: usize,
    ) -> Result<crate::nurbs::Parsed, CodecError> {
        let stream = &self.streams[ordinal];
        crate::nurbs::parse_with_graph(ctx, &stream.semantic_bytes, &stream.nurbs_graph)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::test_deltas::bspline_partition_stream;
    use cadmpeg_core::decode::{DecodeContext, ResourceDimension};

    use super::{topology_streams, ParsedStreams};
    use std::borrow::Cow;

    fn one_delta_pair() -> Vec<crate::parasolid::Stream> {
        let stream = |subtype, file_offset| crate::parasolid::Stream {
            file_offset,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype,
                schema: Some(
                    cadmpeg_parasolid::OwnedSchemaToken::parse(
                        &cadmpeg_test_support::service_decode_context(),
                        "SCH_PAIR".into(),
                    )
                    .expect("service token admission")
                    .expect("the fixture text is a schema token"),
                ),
            },
        };
        vec![
            stream(crate::parasolid::ParasolidSubtype::Partition, 0),
            stream(crate::parasolid::ParasolidSubtype::Deltas, 1),
        ]
    }

    fn scan_with_streams(streams: Vec<crate::parasolid::Stream>) -> crate::decode::Scan<'static> {
        crate::decode::Scan {
            container: crate::container::Container {
                data: Vec::new().into(),
                physical_size: 0,
                layout: crate::container::test_modern_layout(0x06),
                entries: Vec::new(),
                fastload_table: None,
                indexed_section_layouts: std::sync::OnceLock::new(),
                om_section_cache: std::sync::OnceLock::new(),
            },
            streams,
        }
    }

    fn with_collection_limit<T>(
        max_collection_items: u64,
        f: impl FnOnce(&DecodeContext<'_>) -> T,
    ) -> T {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = max_collection_items;
            },
            |ctx| f(ctx),
        )
    }

    #[test]
    fn topology_preparation_refuses_stream_slots_at_collection_limit() {
        let scan = scan_with_streams(vec![crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }]);
        let error = with_collection_limit(0, |ctx| topology_streams(ctx, &scan))
            .expect_err("the prepared stream needs one collection item");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx prepared topology streams"
        ));
    }

    #[test]
    fn topology_preparation_refuses_stream_slots_at_retained_limit() {
        let scan = scan_with_streams(vec![crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }]);

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes =
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                        super::TopologyStream<'_>,
                    >()) - 1;
            },
            |ctx| {
                let error = topology_streams(ctx, &scan)
                    .expect_err("one prepared stream exceeds the retained limit");
                assert!(matches!(
                    error,
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                        if limit.dimension == ResourceDimension::RetainedBytes
                            && limit.operation == "nx prepared topology streams"
                ));
            },
        );
    }

    #[test]
    fn topology_preparation_refuses_paired_delta_scoped_limit() {
        let scan = scan_with_streams(one_delta_pair());

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_materialized_bytes =
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<usize>()) - 1;
            },
            |ctx| {
                let error = topology_streams(ctx, &scan)
                    .expect_err("one paired delta exceeds the scoped limit");
                assert!(matches!(
                    error,
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                        if limit.dimension == ResourceDimension::MaterializedBytes
                            && limit.operation == "nx paired topology deltas"
                ));
            },
        );
    }

    #[test]
    fn topology_views_refuse_output_slots_at_collection_limit() {
        let scan = scan_with_streams(vec![crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }]);
        let error = with_collection_limit(1, |ctx| topology_streams(ctx, &scan))
            .expect_err("the output view needs a second collection item");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx topology byte views"
        ));
    }

    #[test]
    fn parsed_streams_refuse_record_slots_at_collection_limit() {
        let scan = scan_with_streams(vec![crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }]);
        let error = with_collection_limit(1, |ctx| ParsedStreams::parse(ctx, &scan))
            .err()
            .expect("the parsed record needs a second collection item");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx parsed stream records"
        ));
    }

    #[test]
    fn delta_census_transfer_refuses_slots_at_collection_limit() {
        let scan = scan_with_streams(vec![crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }]);
        let error = with_collection_limit(2, |ctx| {
            let mut parsed = ParsedStreams::parse(ctx, &scan)
                .expect("topology and parsed record use the two available slots");
            parsed.take_delta_censuses(ctx)
        })
        .expect_err("census transfer needs a third collection item");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx delta census slots"
        ));
    }

    #[test]
    fn paired_topology_deltas_refuse_index_at_collection_limit() {
        let scan = scan_with_streams(one_delta_pair());
        crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::CollectionItems,
            "nx paired topology deltas",
            |ctx| topology_streams(ctx, &scan),
        );
    }

    #[test]
    fn parsed_streams_refuse_paired_index_at_collection_limit() {
        let scan = scan_with_streams(one_delta_pair());
        crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::CollectionItems,
            "nx paired topology deltas",
            |ctx| ParsedStreams::parse(ctx, &scan).map(|_| ()),
        );
    }

    #[test]
    fn auxiliary_replacement_views_refuse_slots_at_collection_limit() {
        let scan = scan_with_streams(one_delta_pair());
        crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::CollectionItems,
            "nx auxiliary replacement views",
            |ctx| ParsedStreams::parse(ctx, &scan).map(|_| ()),
        );
    }

    #[test]
    fn auxiliary_replacement_views_refuse_scoped_storage() {
        let scan = scan_with_streams(one_delta_pair());
        crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::MaterializedBytes,
            "nx auxiliary replacement views",
            |ctx| ParsedStreams::parse(ctx, &scan).map(|_| ()),
        );
    }

    /// Work admitted so far; the probe refusal fuses the context.
    fn work_used(ctx: &DecodeContext<'_>) -> u64 {
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            ctx.charge_work(u64::MAX, "probe admitted work")
        else {
            panic!("probe refuses");
        };
        limit.used
    }

    fn pair_under_limits(collection_items: u64, work_units: u64) -> cadmpeg_core::CodecError {
        let streams = one_delta_pair();

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = collection_items;
                policy.limits.max_work_units = work_units;
            },
            |ctx| {
                super::pair_stream_indices(ctx, &streams, None)
                    .expect_err("one delta pair exceeds the selected limit")
            },
        )
    }

    #[test]
    fn delta_pairing_refuses_partition_map_at_collection_limit() {
        assert!(matches!(
            pair_under_limits(0, u64::MAX),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx delta pair partitions"
        ));
    }

    #[test]
    fn delta_pairing_refuses_nested_member_at_collection_limit() {
        assert!(matches!(
            pair_under_limits(1, u64::MAX),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx delta pair members"
        ));
    }

    #[test]
    fn delta_pairing_refuses_partition_scan_at_work_limit() {
        let streams = one_delta_pair();
        crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "nx delta partition scan",
            |ctx| super::pair_stream_indices(ctx, &streams, None),
        );
    }

    #[test]
    fn delta_pairing_charges_only_examined_partitions() {
        let preview = |file_offset| crate::parasolid::Stream {
            file_offset,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        };
        let pairing_work = |leading_previews: usize| {
            let mut streams: Vec<_> = (0..leading_previews)
                .map(|offset| preview(10 + offset))
                .collect();
            streams.extend(one_delta_pair());
            crate::test_support::with_decode_context(|ctx| {
                let pairs = super::pair_stream_indices(ctx, &streams, None)
                    .expect("the delta pairs with its preceding partition");
                let partition = leading_previews;
                assert_eq!(
                    pairs,
                    std::collections::BTreeMap::from([(partition, vec![partition + 1])])
                );
                work_used(ctx)
            })
        };
        // Leading streams cost one pairing visit each; the backward partition
        // scan stops at the adjacent partition and never examines them.
        assert_eq!(pairing_work(3), pairing_work(0) + 3);
    }

    #[test]
    fn linked_delta_pairing_refuses_candidate_index_at_collection_limit() {
        let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            crate::test_support::test_om::segment_stream_payload(),
        )]);

        crate::test_support::with_decode_context_over(
            &file,
            |_| {},
            |scan_ctx| {
                let root = cadmpeg_core::decode::View::over_retained(&file);

                let scan = crate::decode::scan(scan_ctx, root).expect("valid linked delta stream");
                let error = with_collection_limit(0, |ctx| super::paired_delta_streams(ctx, &scan))
                    .expect_err("linked delta candidate needs one collection item");
                assert!(matches!(
                    error,
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                        if limit.dimension == ResourceDimension::CollectionItems
                            && limit.operation == "nx linked delta candidates"
                ));
            },
        );
    }

    #[test]
    fn linked_delta_pairing_refuses_stream_match_at_work_limit() {
        let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            crate::test_support::test_om::segment_stream_payload(),
        )]);

        crate::test_support::with_decode_context_over(
            &file,
            |_| {},
            |scan_ctx| {
                let root = cadmpeg_core::decode::View::over_retained(&file);

                let scan = crate::decode::scan(scan_ctx, root).expect("valid linked delta stream");

                crate::test_support::resource_refusal_at(
                    &[],
                    ResourceDimension::WorkUnits,
                    "nx linked delta stream matching",
                    |ctx| super::paired_delta_streams(ctx, &scan),
                );
            },
        );
    }

    #[test]
    fn linked_delta_pairing_charges_only_examined_streams() {
        let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            crate::test_support::test_om::segment_stream_payload(),
        )]);

        crate::test_support::with_decode_context_over(
            &file,
            |_| {},
            |scan_ctx| {
                let root = cadmpeg_core::decode::View::over_retained(&file);

                let mut scan =
                    crate::decode::scan(scan_ctx, root).expect("valid linked delta stream");
                let linking_work = |scan: &crate::decode::Scan<'_>| {
                    crate::test_support::with_decode_context(|ctx| {
                        let pairs = super::paired_delta_streams(ctx, scan)
                            .expect("the wrapper matches the first stream");
                        assert!(pairs.is_empty());
                        work_used(ctx)
                    })
                };
                let without_trailing = linking_work(&scan);
                scan.streams.push(crate::parasolid::Stream {
                    file_offset: 0,
                    consumed: 0,
                    inflated: Vec::new(),
                    body: crate::parasolid::StreamBody::Preview,
                });
                // The trailing stream costs one pairing visit; the wrapper match
                // stops at the first stream and never examines it.
                assert_eq!(linking_work(&scan), without_trailing + 1);
            },
        );
    }

    #[test]
    fn unchanged_stream_views_borrow_the_inflated_bytes() {
        let scan = crate::decode::Scan {
            container: crate::container::Container {
                data: Vec::new().into(),
                physical_size: 0,
                layout: crate::container::test_modern_layout(0x06),
                entries: Vec::new(),
                fastload_table: None,
                indexed_section_layouts: std::sync::OnceLock::new(),
                om_section_cache: std::sync::OnceLock::new(),
            },
            streams: vec![crate::parasolid::Stream {
                file_offset: 0,
                consumed: 3,
                inflated: vec![1, 2, 3],
                body: crate::parasolid::StreamBody::Parasolid {
                    subtype: crate::parasolid::ParasolidSubtype::Partition,
                    schema: Some(
                        cadmpeg_parasolid::OwnedSchemaToken::parse(
                            &cadmpeg_test_support::service_decode_context(),
                            "SCH_TEST_1_9999".into(),
                        )
                        .expect("service token admission")
                        .expect("the fixture text is a schema token"),
                    ),
                },
            }],
        };

        let topology = crate::test_support::with_decode_context(|ctx| {
            topology_streams(ctx, &scan).expect("test topology streams")
        });
        let parsed = crate::test_support::with_decode_context(|ctx| {
            ParsedStreams::parse(ctx, &scan).expect("test parsed streams")
        });

        assert!(matches!(topology[0], Cow::Borrowed(_)));
        assert!(matches!(parsed.streams[0].semantic_bytes, Cow::Borrowed(_)));
        assert!(std::ptr::eq(
            topology[0].as_ptr(),
            scan.streams[0].inflated.as_ptr()
        ));
        assert!(std::ptr::eq(
            parsed.streams[0].semantic_bytes.as_ptr(),
            scan.streams[0].inflated.as_ptr()
        ));
    }

    #[test]
    fn nurbs_geometry_is_parsed_for_the_selected_semantic_stream() {
        let stream = |file_offset| {
            let inflated = bspline_partition_stream();
            crate::parasolid::Stream {
                file_offset,
                consumed: u64::try_from(inflated.len()).expect("test stream length fits u64"),
                inflated,
                body: crate::parasolid::StreamBody::Parasolid {
                    subtype: crate::parasolid::ParasolidSubtype::Partition,
                    schema: Some(
                        cadmpeg_parasolid::OwnedSchemaToken::parse(
                            &cadmpeg_test_support::service_decode_context(),
                            "SCH_TEST_1_9999".into(),
                        )
                        .expect("service token admission")
                        .expect("the fixture text is a schema token"),
                    ),
                },
            }
        };
        let scan = crate::decode::Scan {
            container: crate::container::Container {
                data: Vec::new().into(),
                physical_size: 0,
                layout: crate::container::test_modern_layout(0x06),
                entries: Vec::new(),
                fastload_table: None,
                indexed_section_layouts: std::sync::OnceLock::new(),
                om_section_cache: std::sync::OnceLock::new(),
            },
            streams: vec![stream(0), stream(1)],
        };

        let parsed = crate::test_support::with_decode_context(|ctx| {
            ParsedStreams::parse(ctx, &scan).expect("test parsed streams")
        });

        let expected = crate::test_support::with_decode_context(|ctx| {
            crate::nurbs::parse_with_graph(
                ctx,
                &parsed.streams[1].semantic_bytes,
                &parsed.streams[1].nurbs_graph,
            )
        })
        .unwrap();
        let actual =
            crate::test_support::with_decode_context(|ctx| parsed.parse_nurbs(ctx, 1)).unwrap();
        assert_eq!(actual.surfaces.len(), expected.surfaces.len());
        assert_eq!(actual.curves.len(), expected.curves.len());
        assert_eq!(actual.pcurves.len(), expected.pcurves.len());
        assert!(!actual.surfaces.is_empty());
        assert!(!actual.curves.is_empty());
        assert_eq!(
            actual
                .surfaces
                .iter()
                .map(|surface| surface.pos)
                .collect::<Vec<_>>(),
            expected
                .surfaces
                .iter()
                .map(|surface| surface.pos)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            actual
                .curves
                .iter()
                .map(|curve| curve.pos)
                .collect::<Vec<_>>(),
            expected
                .curves
                .iter()
                .map(|curve| curve.pos)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            actual
                .pcurves
                .iter()
                .map(|pcurve| pcurve.pos)
                .collect::<Vec<_>>(),
            expected
                .pcurves
                .iter()
                .map(|pcurve| pcurve.pos)
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn segment_order_pairs_delta_across_intervening_non_history_stream() {
        use crate::parasolid::Stream;
        use std::collections::BTreeSet;

        let stream = |subtype, schema: Option<&str>, file_offset| Stream {
            file_offset,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype,
                schema: schema.map(|schema| {
                    cadmpeg_parasolid::OwnedSchemaToken::parse(
                        &cadmpeg_test_support::service_decode_context(),
                        schema.into(),
                    )
                    .expect("service token admission")
                    .expect("the fixture text is a schema token")
                }),
            },
        };
        let streams = vec![
            stream(
                crate::parasolid::ParasolidSubtype::Partition,
                Some("SCH_A"),
                10,
            ),
            Stream {
                file_offset: 20,
                consumed: 0,
                inflated: Vec::new(),
                body: crate::parasolid::StreamBody::Preview,
            },
            stream(
                crate::parasolid::ParasolidSubtype::Deltas,
                Some("SCH_A"),
                30,
            ),
            stream(
                crate::parasolid::ParasolidSubtype::Partition,
                Some("SCH_B"),
                40,
            ),
            stream(
                crate::parasolid::ParasolidSubtype::Deltas,
                Some("SCH_A"),
                50,
            ),
            stream(
                crate::parasolid::ParasolidSubtype::Deltas,
                Some("SCH_B"),
                60,
            ),
        ];
        let eligible = BTreeSet::from([2usize, 5]);
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| {
                super::pair_stream_indices(ctx, &streams, Some(&eligible))
                    .expect("test delta pairing")
            }),
            std::collections::BTreeMap::from([(0, vec![2]), (3, vec![5])])
        );
    }
}
