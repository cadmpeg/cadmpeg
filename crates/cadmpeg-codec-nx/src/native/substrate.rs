// SPDX-License-Identifier: Apache-2.0
//! Shared primary parses and sequential Deltas source ownership.
//! Deltas streams have an empty semantic view; their physical events are retained separately.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use cadmpeg_core::CodecError;

use crate::decode::Scan;
use crate::intersection::{self, CurveScan};
use crate::parasolid::{Stream, StreamKind};
use crate::topology::{BlendSurface, Graph, OffsetSurface, SurfaceCurve, TrimmedCurve};

struct TopologyStream<'a> {
    source: &'a [u8],
    deltas: Option<crate::records::DeltasRecords<'a>>,
}

#[cfg(test)]
pub(crate) fn topology_streams<'a>(scan: &'a Scan<'_>) -> Result<Vec<&'a [u8]>, CodecError> {
    Ok(prepare_topology_streams(scan)?
        .into_iter()
        .map(|stream| stream.source)
        .collect())
}

fn prepare_topology_streams<'a>(scan: &'a Scan<'_>) -> Result<Vec<TopologyStream<'a>>, CodecError> {
    let mut semantic = Vec::with_capacity(scan.streams.len());
    for stream in &scan.streams {
        let (source, deltas): (&[u8], _) = if stream.kind() == StreamKind::Deltas {
            (
                &[],
                Some(crate::records::DeltasRecords::read(&stream.inflated)?),
            )
        } else {
            (&stream.inflated, None)
        };
        semantic.push(TopologyStream { source, deltas });
    }
    Ok(semantic)
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

/// Cached primary topology and geometry. Deltas streams retain an empty view.
pub(crate) struct StreamView<'a> {
    /// Original stream bytes; empty for Deltas streams.
    pub(crate) source: &'a [u8],
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

impl<'a> StreamView<'a> {
    fn without_geometry(source: &'a [u8]) -> Self {
        StreamView {
            source,
            graph: Rc::new(Graph::default()),
            offset_surfaces: Vec::new(),
            blend_surfaces: Vec::new(),
            trimmed_curves: Vec::new(),
            surface_curves: Vec::new(),
            intersections: CurveScan::default(),
        }
    }

    /// Parse cached primary families from their original stream bytes.
    fn parse_uniform(
        bytes: &'a [u8],
        point_layout: Option<crate::intersection::ChartPointLayout>,
    ) -> Result<Self, CodecError> {
        let graph = Rc::new(Graph::parse(bytes));
        Ok(StreamView {
            source: bytes,
            offset_surfaces: graph.offset_surfaces(),
            blend_surfaces: graph.blend_surfaces(),
            trimmed_curves: graph.trimmed_curves(),
            surface_curves: graph.surface_curves(),
            intersections: match point_layout {
                Some(layout) => intersection::scan_with_graph(bytes, &graph, layout)?,
                None => CurveScan::default(),
            },
            graph,
        })
    }
}

/// One stream's shared parses, raw source, and deferred geometry inputs.
struct StreamParse<'a> {
    view: StreamView<'a>,
    deltas: Option<crate::records::DeltasRecords<'a>>,
}

/// Every expensive per-stream Parasolid parse, once per stream, indexed by
/// stream ordinal. NURBS parsing remains deferred until requested.
pub(crate) struct ParsedStreams<'a> {
    streams: Vec<StreamParse<'a>>,
}

impl<'a> ParsedStreams<'a> {
    /// Parse primary geometry once and retain the Deltas owner for native consumers.
    pub(crate) fn parse(scan: &'a Scan) -> Result<Self, CodecError> {
        let mut topology = prepare_topology_streams(scan)?;
        let mut streams = Vec::with_capacity(scan.streams.len());
        for (ordinal, stream) in scan.streams.iter().enumerate() {
            let source = topology[ordinal].source;
            let view = match &stream.body {
                crate::parasolid::StreamBody::Parasolid { subtype, .. }
                    if stream.kind() != StreamKind::Deltas =>
                {
                    StreamView::parse_uniform(&stream.inflated, subtype.chart_point_layout())?
                }
                _ => StreamView::without_geometry(source),
            };
            streams.push(StreamParse {
                view,
                deltas: topology[ordinal].deltas.take(),
            });
        }
        Ok(Self { streams })
    }

    pub(crate) fn deltas(&self, ordinal: usize) -> Option<&crate::records::DeltasRecords<'a>> {
        self.streams.get(ordinal)?.deltas.as_ref()
    }

    pub(super) fn take_deltas(&mut self) -> Vec<Option<crate::records::DeltasRecords<'a>>> {
        self.streams
            .iter_mut()
            .map(|stream| stream.deltas.take())
            .collect()
    }

    /// The cached parses of the stream at `ordinal`.
    pub(crate) fn stream(&self, ordinal: usize) -> &StreamView<'a> {
        &self.streams[ordinal].view
    }

    /// Iterate `(ordinal, parses)` over every stream.
    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &StreamView<'a>)> {
        self.streams
            .iter()
            .enumerate()
            .map(|(ordinal, stream)| (ordinal, &stream.view))
    }

    /// Parse NURBS geometry for the selected primary stream when requested.
    pub(crate) fn parse_nurbs(&self, ordinal: usize) -> crate::nurbs::Parsed {
        let stream = &self.streams[ordinal];
        crate::nurbs::parse_with_graph(stream.view.source, &stream.view.graph)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::test_deltas::bspline_partition_stream;

    use super::{topology_streams, ParsedStreams};

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

        assert!(std::ptr::eq(
            topology[0],
            scan.streams[0].inflated.as_slice()
        ));
        assert!(std::ptr::eq(
            parsed.streams[0].view.source,
            scan.streams[0].inflated.as_slice()
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

        let expected = crate::nurbs::parse_with_graph(
            parsed.streams[1].view.source,
            &parsed.streams[1].view.graph,
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
}
