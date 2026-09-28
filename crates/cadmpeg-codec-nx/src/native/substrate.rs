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
    let semantic = prepare_topology_streams(ctx, scan, None)?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(semantic.len()),
        "nx topology byte views",
    )?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(semantic.len())
        .map_err(|_| ctx.refuse_codec_limit("nx topology byte views", 0, 1))?;
    for stream in semantic {
        bytes.push(stream.bytes);
    }
    Ok(bytes)
}

fn prepare_topology_streams<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a Scan<'_>,
    mut unmatched_tombstone_counts: Option<&mut BTreeMap<&'static str, usize>>,
) -> Result<Vec<TopologyStream<'a>>, CodecError> {
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(scan.streams.len()),
        "nx prepared topology streams",
    )?;
    let mut semantic = Vec::new();
    semantic
        .try_reserve_exact(scan.streams.len())
        .map_err(|_| ctx.refuse_codec_limit("nx prepared topology streams", 0, 1))?;
    for stream in &scan.streams {
        semantic.push(TopologyStream {
            bytes: Cow::Borrowed(stream.inflated.as_slice()),
            delta_census: None,
        });
    }
    let pairs = paired_delta_streams(ctx, scan)?;
    let mut paired_deltas = BTreeSet::new();
    for delta in pairs.values().flatten().copied() {
        if !paired_deltas.contains(&delta) {
            ctx.charge_collection_items(1, "nx paired topology deltas")?;
            paired_deltas.insert(delta);
        }
    }
    let mut merge = |partition: &[u8], deltas: &[u8], census: &Census| -> Result<Vec<u8>, CodecError> {
        if let Some(totals) = unmatched_tombstone_counts.as_deref_mut() {
            let result =
                crate::deltas::merge_full_records_with_tombstone_census(ctx, partition, deltas, census)?;
            for (family, count) in result.unmatched_tombstones {
                if !totals.contains_key(family) {
                    ctx.charge_collection_items(1, "NX unmatched tombstone family totals")?;
                }
                *totals.entry(family).or_default() += count;
            }
            Ok(result.merged)
        } else {
            crate::deltas::merge_full_records_with_census(ctx, partition, deltas, census)
        }
    };
    for (delta, stream) in scan.streams.iter().enumerate() {
        if stream.kind() == StreamKind::Deltas && !paired_deltas.contains(&delta) {
            let census = crate::deltas::census::walk(ctx, &stream.inflated)?;
            if !census.records.is_empty() || !census.tombstones.is_empty() {
                let merged = merge(&[], &stream.inflated, &census)?;
                semantic[delta].bytes = Cow::Owned(merged);
            }
            semantic[delta].delta_census = Some(census);
        }
    }
    for (partition, deltas) in pairs {
        for delta in deltas {
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
    let mut linked_deltas = BTreeSet::new();
    for wrapper in scan.container.segment_stream_wrappers() {
        let mut matched = None;
        for (ordinal, stream) in scan.streams.iter().enumerate() {
            ctx.charge_work(1, "nx linked delta stream matching")?;
            if stream.file_offset == wrapper.zlib_offset {
                matched = Some((ordinal, stream));
                break;
            }
        }
        if let Some((ordinal, stream)) = matched {
            has_links = true;
            if stream.kind() == crate::parasolid::StreamKind::Deltas
                && !linked_deltas.contains(&ordinal)
            {
                ctx.charge_collection_items(1, "nx linked delta candidates")?;
                linked_deltas.insert(ordinal);
            }
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
    for (delta, stream) in streams.iter().enumerate() {
        if stream.kind() != StreamKind::Deltas
            || eligible_deltas.is_some_and(|eligible| !eligible.contains(&delta))
        {
            continue;
        }
        let mut partition = None;
        for (ordinal, candidate) in streams[..delta].iter().enumerate().rev() {
            ctx.charge_work(1, "nx delta partition scan")?;
            if candidate.kind() == StreamKind::Partition
                && candidate.schema_token() == stream.schema_token()
            {
                partition = Some(ordinal);
                break;
            }
        }
        if let Some(partition) = partition {
            if !pairs.contains_key(&partition) {
                ctx.charge_collection_items(1, "nx delta pair partitions")?;
            }
            let deltas = pairs.entry(partition).or_default();
            ctx.charge_collection_items(1, "nx delta pair members")?;
            deltas
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("nx delta pair members", 0, 1))?;
            deltas.push(delta);
        }
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
        topology_bytes: &[u8],
        semantic_bytes: &[u8],
        scan: &Scan,
        paired_deltas: Option<&Vec<usize>>,
        point_layout: crate::intersection::ChartPointLayout,
    ) -> Result<(Self, Rc<Graph>), CodecError> {
        let semantic_graph = if semantic_bytes != topology_bytes {
            Some(Rc::new(Graph::parse(ctx, semantic_bytes)?))
        } else {
            None
        };
        let scan_graph = semantic_graph.as_deref().unwrap_or(&graph);
        let nurbs_graph = semantic_graph
            .as_ref()
            .map_or_else(|| Rc::clone(&graph), Rc::clone);
        let intersections = if let Some(delta_indices) = paired_deltas {
            ctx.charge_collection_items(
                cadmpeg_core::decode::u64_from_index(delta_indices.len()),
                "nx auxiliary replacement views",
            )?;
            let mut replacement_streams = Vec::new();
            replacement_streams
                .try_reserve_exact(delta_indices.len())
                .map_err(|_| ctx.refuse_codec_limit("nx auxiliary replacement views", 0, 1))?;
            for delta in delta_indices {
                replacement_streams.push(scan.streams[*delta].inflated.as_slice());
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
        let mut topology_streams =
            prepare_topology_streams(ctx, scan, Some(&mut unmatched_tombstone_counts))?;
        let delta_pairs = paired_delta_streams(ctx, scan)?;
        let mut paired_deltas = BTreeSet::new();
        for delta in delta_pairs.values().flatten().copied() {
            if !paired_deltas.contains(&delta) {
                ctx.charge_collection_items(1, "nx parsed stream paired deltas")?;
                paired_deltas.insert(delta);
            }
        }

        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(scan.streams.len()),
            "nx parsed stream records",
        )?;
        let mut streams = Vec::new();
        streams
            .try_reserve_exact(scan.streams.len())
            .map_err(|_| ctx.refuse_codec_limit("nx parsed stream records", 0, 1))?;
        for (si, stream) in scan.streams.iter().enumerate() {
            let mut semantic_bytes = std::mem::take(&mut topology_streams[si].bytes);
            let crate::parasolid::StreamBody::Parasolid { subtype, .. } = &stream.body else {
                let empty = Rc::new(StreamView::empty());
                let empty_graph = Rc::clone(&empty.graph);
                streams.push(StreamParse {
                    views: StreamParses {
                        raw: empty.clone(),
                        semantic: empty,
                    },
                    semantic_bytes,
                    delta_census: topology_streams[si].delta_census.take(),
                    nurbs_graph: empty_graph,
                });
                continue;
            };
            let point_layout = subtype.chart_point_layout();
            let paired = delta_pairs.get(&si);
            let topology_matches_raw = semantic_bytes.as_ref() == stream.inflated;
            let mut residual = Vec::new();
            if stream.kind() == StreamKind::Deltas && !paired_deltas.contains(&si) {
                if let Some(census) = topology_streams[si].delta_census.as_ref() {
                    residual.extend_from_slice(&crate::deltas::semantic_residual_with_census(
                        ctx,
                        &stream.inflated,
                        census,
                    )?);
                }
            }
            if let Some(deltas) = paired {
                for delta in deltas {
                    if let Some(census) = topology_streams[*delta].delta_census.as_ref() {
                        residual.extend_from_slice(&crate::deltas::semantic_residual_with_census(
                            ctx,
                            &scan.streams[*delta].inflated,
                            census,
                        )?);
                    }
                }
            }
            let identical = paired.is_none() && topology_matches_raw && residual.is_empty();
            let raw = Rc::new(StreamView::parse_uniform(ctx, &stream.inflated, point_layout)?);
            let (semantic, nurbs_graph) = if identical {
                (Rc::clone(&raw), Rc::clone(&raw.graph))
            } else {
                let graph = Rc::new(Graph::parse(ctx, &semantic_bytes)?);
                let topology_for_auxiliary = paired.map(|_| semantic_bytes.clone());
                semantic_bytes.to_mut().extend_from_slice(&residual);
                let (semantic, nurbs_graph) = StreamView::parse_semantic(
                    ctx,
                    graph,
                    topology_for_auxiliary
                        .as_deref()
                        .unwrap_or(semantic_bytes.as_ref()),
                    &semantic_bytes,
                    scan,
                    paired,
                    point_layout,
                )?;
                (Rc::new(semantic), nurbs_graph)
            };
            streams.push(StreamParse {
                views: StreamParses { raw, semantic },
                semantic_bytes,
                delta_census: topology_streams[si].delta_census.take(),
                nurbs_graph,
            });
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
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(self.streams.len()),
            "nx delta census slots",
        )?;
        let mut censuses = Vec::new();
        censuses
            .try_reserve_exact(self.streams.len())
            .map_err(|_| ctx.refuse_codec_limit("nx delta census slots", 0, 1))?;
        for stream in &mut self.streams {
            censuses.push(stream.delta_census.take());
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

    /// The prepared delta-extended semantic bytes of the stream at `ordinal`.
    pub(crate) fn semantic_bytes(&self, ordinal: usize) -> &[u8] {
        &self.streams[ordinal].semantic_bytes
    }

    /// Parse NURBS geometry for the selected semantic stream when requested.
    pub(crate) fn parse_nurbs(&self, ordinal: usize) -> crate::nurbs::Parsed {
        let stream = &self.streams[ordinal];
        crate::nurbs::parse_with_graph(&stream.semantic_bytes, &stream.nurbs_graph)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::test_deltas::bspline_partition_stream;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

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
                    cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_PAIR")
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
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_collection_items;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits service policy");
        f(&ctx)
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
        let error = with_collection_limit(4, |ctx| topology_streams(ctx, &scan))
            .expect_err("paired topology index needs a fifth collection item");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx paired topology deltas"
        ));
    }

    #[test]
    fn parsed_streams_refuse_paired_index_at_collection_limit() {
        let scan = scan_with_streams(one_delta_pair());
        let error = with_collection_limit(7, |ctx| ParsedStreams::parse(ctx, &scan))
            .err()
            .expect("parsed paired index needs an eighth collection item");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx parsed stream paired deltas"
        ));
    }

    #[test]
    fn auxiliary_replacement_views_refuse_slots_at_collection_limit() {
        let scan = scan_with_streams(one_delta_pair());
        let error = with_collection_limit(10, |ctx| ParsedStreams::parse(ctx, &scan))
            .err()
            .expect("replacement view needs an eleventh collection item");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx auxiliary replacement views"
        ));
    }

    fn pair_under_limits(collection_items: u64, work_units: u64) -> cadmpeg_core::CodecError {
        let streams = one_delta_pair();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = collection_items;
        policy.limits.max_work_units = work_units;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits service policy");
        super::pair_stream_indices(&ctx, &streams, None)
            .expect_err("one delta pair exceeds the selected limit")
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
        assert!(matches!(
            pair_under_limits(u64::MAX, 0),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "nx delta partition scan"
        ));
    }

    #[test]
    fn delta_pairing_charges_only_examined_partitions() {
        let mut streams = vec![crate::parasolid::Stream {
            file_offset: 10,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }];
        streams.extend(one_delta_pair());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits service policy");
        let pairs = super::pair_stream_indices(&ctx, &streams, None)
            .expect("the preceding partition matches within one work unit");
        assert_eq!(pairs, std::collections::BTreeMap::from([(1, vec![2])]));
    }

    #[test]
    fn linked_delta_pairing_refuses_candidate_index_at_collection_limit() {
        let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            crate::test_support::test_om::segment_stream_payload(),
        )]);
        let scan_arena = DecodeArena::new();
        let scan_policy = DecodePolicy::default();
        let (scan_ctx, root) = DecodeContext::from_root_bytes(&file, &scan_arena, &scan_policy)
            .expect("bounded segment stream fixture");
        let scan = crate::decode::scan(&scan_ctx, root).expect("valid linked delta stream");
        let error = with_collection_limit(0, |ctx| super::paired_delta_streams(ctx, &scan))
            .expect_err("linked delta candidate needs one collection item");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx linked delta candidates"
        ));
    }

    #[test]
    fn linked_delta_pairing_refuses_stream_match_at_work_limit() {
        let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            crate::test_support::test_om::segment_stream_payload(),
        )]);
        let scan_arena = DecodeArena::new();
        let scan_policy = DecodePolicy::default();
        let (scan_ctx, root) = DecodeContext::from_root_bytes(&file, &scan_arena, &scan_policy)
            .expect("bounded segment stream fixture");
        let scan = crate::decode::scan(&scan_ctx, root).expect("valid linked delta stream");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits service policy");
        let error = super::paired_delta_streams(&ctx, &scan)
            .expect_err("one wrapper match exceeds zero work units");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "nx linked delta stream matching"
        ));
    }

    #[test]
    fn linked_delta_pairing_charges_only_examined_streams() {
        let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            crate::test_support::test_om::segment_stream_payload(),
        )]);
        let scan_arena = DecodeArena::new();
        let scan_policy = DecodePolicy::default();
        let (scan_ctx, root) = DecodeContext::from_root_bytes(&file, &scan_arena, &scan_policy)
            .expect("bounded segment stream fixture");
        let mut scan = crate::decode::scan(&scan_ctx, root).expect("valid linked delta stream");
        scan.streams.push(crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits service policy");
        let pairs = super::paired_delta_streams(&ctx, &scan)
            .expect("the first stream matches within one work unit");
        assert!(pairs.is_empty());
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
                        cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_TEST_1_9999")
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
                        cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_TEST_1_9999")
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

        let expected = crate::nurbs::parse_with_graph(
            parsed.semantic_bytes(1),
            &parsed.streams[1].nurbs_graph,
        );
        let actual = parsed.parse_nurbs(1);
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
                    cadmpeg_parasolid::OwnedSchemaToken::try_from(schema)
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
