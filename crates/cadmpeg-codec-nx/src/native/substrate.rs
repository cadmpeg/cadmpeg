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

use crate::decode::Scan;
use crate::deltas::Census;
use crate::intersection::{self, CurveScan};
use crate::parasolid::{Stream, StreamKind};
use crate::topology::{BlendSurface, Graph, OffsetSurface, SurfaceCurve, TrimmedCurve};

struct TopologyPreparation<'a> {
    streams: Vec<Cow<'a, [u8]>>,
    unmatched_tombstone_counts: BTreeMap<&'static str, usize>,
    delta_censuses: Vec<Option<Census>>,
}

/// The topology-merged bytes per stream: each stream's inflated bytes with delta
/// full-record merges applied. Unpaired delta streams that carry records or tombstones
/// are merged against an empty partition; paired delta streams are merged into their
/// partition and then cleared.
pub(crate) fn topology_streams<'a>(scan: &'a Scan<'_>) -> Vec<Cow<'a, [u8]>> {
    prepare_topology_streams(scan, false).streams
}

fn topology_streams_with_unmatched_tombstones<'a>(scan: &'a Scan<'_>) -> TopologyPreparation<'a> {
    prepare_topology_streams(scan, true)
}

fn prepare_topology_streams<'a>(
    scan: &'a Scan<'_>,
    collect_unmatched_tombstones: bool,
) -> TopologyPreparation<'a> {
    let mut semantic = scan
        .streams
        .iter()
        .map(|stream| Cow::Borrowed(stream.inflated.as_slice()))
        .collect::<Vec<_>>();
    let pairs = paired_delta_streams(scan);
    let paired_deltas = pairs.values().flatten().copied().collect::<BTreeSet<_>>();
    let mut delta_censuses = vec![None; scan.streams.len()];
    let mut unmatched_tombstone_counts = BTreeMap::new();
    let mut add_counts = |counts: BTreeMap<&'static str, usize>| {
        if !collect_unmatched_tombstones {
            return;
        }
        for (family, count) in counts {
            *unmatched_tombstone_counts.entry(family).or_default() += count;
        }
    };
    for (delta, stream) in scan.streams.iter().enumerate() {
        if stream.kind == StreamKind::Deltas && !paired_deltas.contains(&delta) {
            let census = crate::deltas::walk(&stream.inflated);
            if !census.records.is_empty() || !census.tombstones.is_empty() {
                let merged = crate::deltas::merge_full_records_with_census(
                    &[],
                    &stream.inflated,
                    &census,
                    collect_unmatched_tombstones,
                );
                add_counts(merged.unmatched_tombstones);
                semantic[delta] = Cow::Owned(merged.merged);
            }
            delta_censuses[delta] = Some(census);
        }
    }
    for (partition, deltas) in pairs {
        for delta in deltas {
            let census = crate::deltas::walk(&semantic[delta]);
            let merged = crate::deltas::merge_full_records_with_census(
                &semantic[partition],
                &semantic[delta],
                &census,
                collect_unmatched_tombstones,
            );
            add_counts(merged.unmatched_tombstones);
            semantic[partition] = Cow::Owned(merged.merged);
            semantic[delta] = Cow::Borrowed(&[]);
            delta_censuses[delta] = Some(census);
        }
    }
    TopologyPreparation {
        streams: semantic,
        unmatched_tombstone_counts,
        delta_censuses,
    }
}

/// Map each partition stream ordinal to the delta stream ordinals that pair with it,
/// restricting the delta candidates to those the segment stream links mark as `deltas`
/// when any links are present.
pub(crate) fn paired_delta_streams(scan: &Scan) -> BTreeMap<usize, Vec<usize>> {
    let links = super::segments::segment_stream_links(&scan.container, &scan.streams);
    let linked_deltas = links
        .iter()
        .filter(|link| link.stream_kind == "deltas")
        .map(|link| link.stream_ordinal as usize)
        .collect::<BTreeSet<_>>();
    pair_stream_indices(&scan.streams, (!links.is_empty()).then_some(&linked_deltas))
}

/// Pair each eligible delta stream with the nearest preceding partition stream of the
/// same schema. `eligible_deltas`, when `Some`, restricts pairing to those delta
/// ordinals; when `None`, every delta stream is eligible.
pub(crate) fn pair_stream_indices(
    streams: &[Stream],
    eligible_deltas: Option<&BTreeSet<usize>>,
) -> BTreeMap<usize, Vec<usize>> {
    let mut pairs = BTreeMap::<usize, Vec<usize>>::new();
    for (delta, stream) in streams.iter().enumerate() {
        if stream.kind != StreamKind::Deltas
            || eligible_deltas.is_some_and(|eligible| !eligible.contains(&delta))
        {
            continue;
        }
        let partition = streams[..delta]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, candidate)| {
                candidate.kind == StreamKind::Partition && candidate.schema == stream.schema
            })
            .map(|(partition, _)| partition);
        if let Some(partition) = partition {
            pairs.entry(partition).or_default().push(delta);
        }
    }
    pairs
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
    fn parse_uniform(bytes: &[u8], point_layout: crate::intersection::ChartPointLayout) -> Self {
        let graph = Rc::new(Graph::parse(bytes));
        StreamView {
            offset_surfaces: graph.offset_surfaces(),
            blend_surfaces: graph.blend_surfaces(),
            trimmed_curves: graph.trimmed_curves(),
            surface_curves: graph.surface_curves(),
            intersections: intersection::scan_with_graph(bytes, &graph, point_layout),
            graph,
        }
    }

    /// Parse the semantic view: `graph` from the topology-merged bytes, the scanners
    /// from the delta-extended semantic bytes, and `intersections` via the
    /// auxiliary-replacement scan when this stream has paired delta streams.
    fn parse_semantic(
        graph: Rc<Graph>,
        topology_bytes: &[u8],
        semantic_bytes: &[u8],
        scan: &Scan,
        paired_deltas: Option<&Vec<usize>>,
        point_layout: crate::intersection::ChartPointLayout,
    ) -> (Self, Rc<Graph>) {
        let semantic_graph =
            (semantic_bytes != topology_bytes).then(|| Rc::new(Graph::parse(semantic_bytes)));
        let scan_graph = semantic_graph.as_deref().unwrap_or(&graph);
        let nurbs_graph = semantic_graph
            .as_ref()
            .map_or_else(|| Rc::clone(&graph), Rc::clone);
        let intersections = if let Some(delta_indices) = paired_deltas {
            let replacement_streams = delta_indices
                .iter()
                .map(|delta| scan.streams[*delta].inflated.as_slice())
                .collect::<Vec<_>>();
            intersection::scan_with_auxiliary_replacements_and_graph(
                semantic_bytes,
                topology_bytes,
                &replacement_streams,
                scan_graph,
            )
        } else {
            intersection::scan_with_graph(semantic_bytes, scan_graph, point_layout)
        };
        (
            StreamView {
                offset_surfaces: scan_graph.offset_surfaces(),
                blend_surfaces: scan_graph.blend_surfaces(),
                trimmed_curves: scan_graph.trimmed_curves(),
                surface_curves: scan_graph.surface_curves(),
                intersections,
                graph,
            },
            nurbs_graph,
        )
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
    pub(crate) fn view_for_records(&self) -> &StreamView {
        &self.raw
    }

    /// The view the decode geometry (IR) path reads: parses of the delta-extended
    /// semantic bytes.
    pub(crate) fn view_for_geometry(&self) -> &StreamView {
        &self.semantic
    }
}

/// Every expensive per-stream Parasolid parse, once per distinct byte view, indexed by
/// stream ordinal. Also owns the prepared semantic stream bytes the decode geometry
/// path's candidate scanners still read directly. Geometry callers may request a
/// separately owned NURBS cache and take each stream's entry without cloning it.
pub(crate) struct ParsedStreams<'a> {
    per_stream: Vec<StreamParses>,
    semantic_streams: Vec<Cow<'a, [u8]>>,
    unmatched_tombstone_counts: BTreeMap<&'static str, usize>,
    delta_censuses: Vec<Option<Census>>,
    nurbs: Vec<Option<crate::nurbs::Parsed>>,
    nurbs_graphs: Vec<Rc<Graph>>,
}

impl<'a> ParsedStreams<'a> {
    /// Prepare the semantic and topology byte views, then parse each family needed by
    /// its consumer once per byte view. Non-Parasolid streams get empty views. A
    /// stream's raw and semantic views share one parse when the topology-merged and
    /// delta-extended byte views both equal `stream.inflated` and the stream has no
    /// auxiliary-replacement deltas; only that shared view needs NURBS geometry. NURBS parsing
    /// is deferred until a geometry consumer takes the selected stream's cache.
    pub(crate) fn parse(scan: &'a Scan) -> Self {
        let topology = topology_streams_with_unmatched_tombstones(scan);
        let topology_streams = topology.streams;
        let unmatched_tombstone_counts = topology.unmatched_tombstone_counts;
        let delta_censuses = topology.delta_censuses;
        let delta_pairs = paired_delta_streams(scan);
        let paired_deltas = delta_pairs
            .values()
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>();

        let mut per_stream = Vec::with_capacity(scan.streams.len());
        let mut semantic_streams = Vec::with_capacity(scan.streams.len());
        let mut nurbs = Vec::with_capacity(scan.streams.len());
        let mut nurbs_graphs = Vec::with_capacity(scan.streams.len());
        for (si, (stream, mut semantic_bytes)) in
            scan.streams.iter().zip(topology_streams).enumerate()
        {
            if !stream.kind.is_parasolid() {
                let empty = Rc::new(StreamView::empty());
                let empty_graph = Rc::clone(&empty.graph);
                per_stream.push(StreamParses {
                    raw: empty.clone(),
                    semantic: empty,
                });
                semantic_streams.push(semantic_bytes);
                nurbs.push(None);
                nurbs_graphs.push(empty_graph);
                continue;
            }

            let point_layout = stream
                .kind
                .chart_point_layout()
                .expect("Parasolid stream has a chart point layout");
            let paired = delta_pairs.get(&si);
            let topology_matches_raw = semantic_bytes.as_ref() == stream.inflated;
            let mut residual = Vec::new();
            if stream.kind == StreamKind::Deltas && !paired_deltas.contains(&si) {
                if let Some(census) = delta_censuses[si].as_ref() {
                    residual.extend_from_slice(&crate::deltas::semantic_residual_with_census(
                        &stream.inflated,
                        census,
                    ));
                }
            }
            if let Some(deltas) = paired {
                for delta in deltas {
                    if let Some(census) = delta_censuses[*delta].as_ref() {
                        residual.extend_from_slice(&crate::deltas::semantic_residual_with_census(
                            &scan.streams[*delta].inflated,
                            census,
                        ));
                    }
                }
            }
            let identical = paired.is_none() && topology_matches_raw && residual.is_empty();
            let raw = Rc::new(StreamView::parse_uniform(&stream.inflated, point_layout));
            let (semantic, nurbs_graph) = if identical {
                (Rc::clone(&raw), Rc::clone(&raw.graph))
            } else {
                let graph = Rc::new(Graph::parse(&semantic_bytes));
                let topology_for_auxiliary = paired.map(|_| semantic_bytes.clone());
                semantic_bytes.to_mut().extend_from_slice(&residual);
                let (semantic, nurbs_graph) = StreamView::parse_semantic(
                    graph,
                    topology_for_auxiliary
                        .as_deref()
                        .unwrap_or(semantic_bytes.as_ref()),
                    &semantic_bytes,
                    scan,
                    paired,
                    point_layout,
                );
                (Rc::new(semantic), nurbs_graph)
            };
            per_stream.push(StreamParses { raw, semantic });
            semantic_streams.push(semantic_bytes);
            nurbs.push(None);
            nurbs_graphs.push(nurbs_graph);
        }

        ParsedStreams {
            per_stream,
            semantic_streams,
            unmatched_tombstone_counts,
            delta_censuses,
            nurbs,
            nurbs_graphs,
        }
    }

    pub(crate) fn unmatched_tombstone_counts(&self) -> &BTreeMap<&'static str, usize> {
        &self.unmatched_tombstone_counts
    }

    /// Move the delta censuses into the native extractor after all semantic
    /// residuals have been built. Each delta walk is owned by one decode.
    pub(crate) fn take_delta_censuses(&mut self) -> Vec<Option<Census>> {
        std::mem::take(&mut self.delta_censuses)
    }

    /// The cached parses of the stream at `ordinal`.
    pub(crate) fn stream(&self, ordinal: usize) -> &StreamParses {
        &self.per_stream[ordinal]
    }

    /// Iterate `(ordinal, parses)` over every stream.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (usize, &StreamParses)> {
        self.per_stream.iter().enumerate()
    }

    /// The prepared delta-extended semantic bytes of the stream at `ordinal`.
    pub(crate) fn semantic_bytes(&self, ordinal: usize) -> &[u8] {
        &self.semantic_streams[ordinal]
    }

    /// Parse and move the semantic NURBS cache for one selected stream into the geometry builder.
    pub(crate) fn take_nurbs(&mut self, ordinal: usize) -> crate::nurbs::Parsed {
        if self.nurbs[ordinal].is_none() {
            let parsed = crate::nurbs::parse_with_graph(
                &self.semantic_streams[ordinal],
                &self.nurbs_graphs[ordinal],
            );
            self.nurbs[ordinal] = Some(parsed);
        }
        std::mem::take(&mut self.nurbs[ordinal])
            .expect("selected Parasolid stream has a prepared NURBS cache")
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::bspline_partition_stream;

    use crate::parasolid::StreamKind;

    use super::*;

    #[test]
    fn unchanged_stream_views_borrow_the_inflated_bytes() {
        let scan = crate::decode::Scan {
            container: crate::container::Container {
                data: Vec::new().into(),
                version: 0x06,
                header_entry_count: 0,
                physical_size: 0,
                layout: crate::container::TEST_MODERN_LAYOUT,
                entries: Vec::new(),
                indexed_section_layouts: std::sync::OnceLock::new(),
                om_operation_label_layouts: std::sync::OnceLock::new(),
                om_section_cache: std::sync::OnceLock::new(),
            },
            streams: vec![crate::parasolid::Stream {
                file_offset: 0,
                consumed: 3,
                inflated: vec![1, 2, 3],
                kind: StreamKind::Partition,
                schema: Some("schema".into()),
            }],
        };

        let topology = topology_streams(&scan);
        let parsed = ParsedStreams::parse(&scan);

        assert!(matches!(topology[0], Cow::Borrowed(_)));
        assert!(matches!(parsed.semantic_streams[0], Cow::Borrowed(_)));
        assert!(std::ptr::eq(
            topology[0].as_ptr(),
            scan.streams[0].inflated.as_ptr()
        ));
        assert!(std::ptr::eq(
            parsed.semantic_streams[0].as_ptr(),
            scan.streams[0].inflated.as_ptr()
        ));
    }

    #[test]
    fn nurbs_cache_is_lazy_and_moved_after_preparation() {
        let stream = |file_offset| {
            let inflated = bspline_partition_stream();
            crate::parasolid::Stream {
                file_offset,
                consumed: u64::try_from(inflated.len()).expect("test stream length fits u64"),
                inflated,
                kind: StreamKind::Partition,
                schema: Some("schema".into()),
            }
        };
        let scan = crate::decode::Scan {
            container: crate::container::Container {
                data: Vec::new().into(),
                version: 0x06,
                header_entry_count: 0,
                physical_size: 0,
                layout: crate::container::TEST_MODERN_LAYOUT,
                entries: Vec::new(),
                indexed_section_layouts: std::sync::OnceLock::new(),
                om_operation_label_layouts: std::sync::OnceLock::new(),
                om_section_cache: std::sync::OnceLock::new(),
            },
            streams: vec![stream(0), stream(1)],
        };

        let mut parsed = ParsedStreams::parse(&scan);
        assert!(parsed.nurbs.iter().all(Option::is_none));

        let expected =
            crate::nurbs::parse_with_graph(parsed.semantic_bytes(1), &parsed.nurbs_graphs[1]);
        let actual = parsed.take_nurbs(1);
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
        assert!(parsed.nurbs[0].is_none());
        assert!(parsed.nurbs[1].is_none());
    }

    #[test]
    fn segment_order_pairs_delta_across_intervening_non_history_stream() {
        use crate::parasolid::{Stream, StreamKind};
        use std::collections::BTreeSet;

        let stream = |kind, schema: Option<&str>, file_offset| Stream {
            file_offset,
            consumed: 0,
            inflated: Vec::new(),
            kind,
            schema: schema.map(str::to_string),
        };
        let streams = vec![
            stream(StreamKind::Partition, Some("SCH_A"), 10),
            stream(StreamKind::Preview, None, 20),
            stream(StreamKind::Deltas, Some("SCH_A"), 30),
            stream(StreamKind::Partition, Some("SCH_B"), 40),
            stream(StreamKind::Deltas, Some("SCH_A"), 50),
            stream(StreamKind::Deltas, Some("SCH_B"), 60),
        ];
        let eligible = BTreeSet::from([2usize, 5]);
        assert_eq!(
            super::pair_stream_indices(&streams, Some(&eligible)),
            std::collections::BTreeMap::from([(0, vec![2]), (3, vec![5])])
        );
    }
}
