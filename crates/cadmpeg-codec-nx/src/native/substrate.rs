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
pub(crate) fn topology_streams<'a>(scan: &'a Scan<'_>) -> Result<Vec<Cow<'a, [u8]>>, CodecError> {
    let semantic = prepare_topology_streams(scan, None)?;
    let mut bytes = Vec::new();
    for stream in semantic {
        bytes.push(stream.bytes);
    }
    Ok(bytes)
}

fn prepare_topology_streams<'a>(
    scan: &'a Scan<'_>,
    mut unmatched_tombstone_counts: Option<&mut BTreeMap<&'static str, usize>>,
) -> Result<Vec<TopologyStream<'a>>, CodecError> {
    let mut semantic = Vec::new();
    for stream in &scan.streams {
        semantic.push(TopologyStream {
            bytes: Cow::Borrowed(stream.inflated.as_slice()),
            delta_census: None,
        });
    }
    let pairs = paired_delta_streams(scan);
    let paired_deltas = pairs.values().flatten().copied().collect::<BTreeSet<_>>();
    let mut merge =
        |partition: &[u8], deltas: &[u8], census: &Census| -> Result<Vec<u8>, CodecError> {
            if let Some(totals) = unmatched_tombstone_counts.as_deref_mut() {
                let result = crate::deltas::merge_full_records_with_tombstone_census(
                    partition, deltas, census,
                )?;
                for (family, count) in result.unmatched_tombstones {
                    *totals.entry(family).or_default() += count;
                }
                Ok(result.merged)
            } else {
                crate::deltas::merge_full_records_with_census(partition, deltas, census)
            }
        };
    for (delta, stream) in scan.streams.iter().enumerate() {
        if stream.kind() == StreamKind::Deltas && !paired_deltas.contains(&delta) {
            let census = crate::deltas::census::walk(&stream.inflated)?;
            if !census.records.is_empty() || !census.tombstones.is_empty() {
                let merged = merge(&[], &stream.inflated, &census)?;
                semantic[delta].bytes = Cow::Owned(merged);
            }
            semantic[delta].delta_census = Some(census);
        }
    }
    for (partition, deltas) in pairs {
        for delta in deltas {
            let census = crate::deltas::census::walk(&semantic[delta].bytes)?;
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
pub(super) fn paired_delta_streams(scan: &Scan) -> BTreeMap<usize, Vec<usize>> {
    // The first stream at each file offset answers a wrapper lookup.
    let mut streams_by_offset = BTreeMap::new();
    for (ordinal, stream) in scan.streams.iter().enumerate() {
        streams_by_offset
            .entry(stream.file_offset)
            .or_insert((ordinal, stream));
    }
    let mut has_links = false;
    let mut linked_deltas = BTreeSet::new();
    for wrapper in scan.container.segment_stream_wrappers() {
        if let Some(&(ordinal, stream)) = streams_by_offset.get(&wrapper.zlib_offset) {
            has_links = true;
            if stream.kind() == crate::parasolid::StreamKind::Deltas {
                linked_deltas.insert(ordinal);
            }
        }
    }
    pair_stream_indices(&scan.streams, has_links.then_some(&linked_deltas))
}

/// Pair each eligible delta stream with the nearest preceding partition stream of the
/// same schema. `eligible_deltas`, when `Some`, restricts pairing to those delta
/// ordinals; when `None`, every delta stream is eligible.
pub(super) fn pair_stream_indices(
    streams: &[Stream],
    eligible_deltas: Option<&BTreeSet<usize>>,
) -> BTreeMap<usize, Vec<usize>> {
    let mut pairs = BTreeMap::<usize, Vec<usize>>::new();
    // The latest partition seen so far for each schema is the nearest preceding one.
    let mut latest_partition = BTreeMap::new();
    for (ordinal, stream) in streams.iter().enumerate() {
        if stream.kind() == StreamKind::Partition {
            latest_partition.insert(stream.schema_token(), ordinal);
            continue;
        }
        if stream.kind() != StreamKind::Deltas
            || eligible_deltas.is_some_and(|eligible| !eligible.contains(&ordinal))
        {
            continue;
        }
        if let Some(&partition) = latest_partition.get(&stream.schema_token()) {
            pairs.entry(partition).or_default().push(ordinal);
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
    fn parse_uniform(
        bytes: &[u8],
        point_layout: crate::intersection::ChartPointLayout,
    ) -> Result<Self, CodecError> {
        let graph = Rc::new(Graph::parse(bytes));
        Ok(StreamView {
            offset_surfaces: graph.offset_surfaces(),
            blend_surfaces: graph.blend_surfaces(),
            trimmed_curves: graph.trimmed_curves(),
            surface_curves: graph.surface_curves(),
            intersections: intersection::scan_with_graph(bytes, &graph, point_layout)?,
            graph,
        })
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
    ) -> Result<(Self, Rc<Graph>), CodecError> {
        let semantic_graph = if semantic_bytes == topology_bytes {
            None
        } else {
            Some(Rc::new(Graph::parse(semantic_bytes)))
        };
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
            )?
        } else {
            intersection::scan_with_graph(semantic_bytes, scan_graph, point_layout)?
        };
        Ok((
            StreamView {
                offset_surfaces: scan_graph.offset_surfaces(),
                blend_surfaces: scan_graph.blend_surfaces(),
                trimmed_curves: scan_graph.trimmed_curves(),
                surface_curves: scan_graph.surface_curves(),
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
    pub(crate) fn parse(scan: &'a Scan) -> Result<Self, CodecError> {
        let mut unmatched_tombstone_counts = BTreeMap::new();
        let mut topology_streams =
            prepare_topology_streams(scan, Some(&mut unmatched_tombstone_counts))?;
        let delta_pairs = paired_delta_streams(scan);
        let paired_deltas = delta_pairs
            .values()
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>();

        let mut streams = Vec::new();
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
                    let part =
                        crate::deltas::semantic_residual_with_census(&stream.inflated, census);
                    residual.extend_from_slice(&part);
                }
            }
            if let Some(deltas) = paired {
                for delta in deltas {
                    if let Some(census) = topology_streams[*delta].delta_census.as_ref() {
                        let part = crate::deltas::semantic_residual_with_census(
                            &scan.streams[*delta].inflated,
                            census,
                        );
                        residual.extend_from_slice(&part);
                    }
                }
            }
            let identical = paired.is_none() && topology_matches_raw && residual.is_empty();

            let raw = Rc::new(StreamView::parse_uniform(&stream.inflated, point_layout)?);
            let (semantic, nurbs_graph) = if identical {
                (Rc::clone(&raw), Rc::clone(&raw.graph))
            } else {
                let graph = Rc::new(Graph::parse(&semantic_bytes));
                let topology_for_auxiliary = if paired.is_some() {
                    Some(match &semantic_bytes {
                        Cow::Borrowed(bytes) => Cow::Borrowed(*bytes),
                        Cow::Owned(bytes) => Cow::Owned(bytes.clone()),
                    })
                } else {
                    None
                };
                match &mut semantic_bytes {
                    Cow::Borrowed(bytes) => {
                        let mut owned = bytes.to_vec();
                        owned.extend_from_slice(&residual);
                        semantic_bytes = Cow::Owned(owned);
                    }
                    Cow::Owned(bytes) => {
                        bytes.extend_from_slice(&residual);
                    }
                }
                let (semantic, nurbs_graph) = StreamView::parse_semantic(
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

    /// Read the raw delta census before native extraction consumes it.
    pub(crate) fn delta_census(&self, ordinal: usize) -> Option<&Census> {
        self.streams.get(ordinal)?.delta_census.as_ref()
    }

    /// Move the delta censuses into the native extractor after semantic
    /// residuals and source metadata have been built.
    pub(super) fn take_delta_censuses(&mut self) -> Vec<Option<Census>> {
        self.streams
            .iter_mut()
            .map(|stream| stream.delta_census.take())
            .collect()
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

    #[test]
    fn delta_pairing_matches_the_nearest_preceding_partition() {
        let mut streams = vec![crate::parasolid::Stream {
            file_offset: 10,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }];
        streams.extend(one_delta_pair());

        assert_eq!(
            super::pair_stream_indices(&streams, None),
            std::collections::BTreeMap::from([(1, vec![2])])
        );
    }

    #[test]
    fn linked_delta_pairing_ignores_an_unlinked_trailing_stream() {
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
                scan.streams.push(crate::parasolid::Stream {
                    file_offset: 0,
                    consumed: 0,
                    inflated: Vec::new(),
                    body: crate::parasolid::StreamBody::Preview,
                });

                let pairs = super::paired_delta_streams(&scan);
                assert!(pairs.is_empty());
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
                        cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_TEST_1_9999")
                            .expect("the fixture text is a schema token"),
                    ),
                },
            }],
        };

        let topology = topology_streams(&scan).expect("test topology streams");
        let parsed = ParsedStreams::parse(&scan).expect("test parsed streams");

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

        let parsed = ParsedStreams::parse(&scan).expect("test parsed streams");

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
            super::pair_stream_indices(&streams, Some(&eligible)),
            std::collections::BTreeMap::from([(0, vec![2]), (3, vec![5])])
        );
    }
}
