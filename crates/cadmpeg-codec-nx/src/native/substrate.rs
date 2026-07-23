// SPDX-License-Identifier: Apache-2.0
//! Two-view single-parse substrate for the expensive per-stream Parasolid scans.
//!
//! Four topology scanners ([`topology::offset_surfaces`], [`topology::blend_surfaces`],
//! [`topology::trimmed_curves`], [`topology::surface_curves`]), [`topology::Graph::parse`],
//! and [`intersection::scan`] were each run twice per Parasolid stream: once by the
//! decode tier's geometry path on the delta-extended *semantic* stream bytes, and once
//! by the native record extractors on the raw `stream.inflated` bytes. These are two
//! distinct byte views — on delta-bearing files they legitimately differ, so a naive
//! "parse once per stream" would silently change output.
//!
//! [`ParsedStreams`] holds both views per stream and shares one parse only when the
//! byte views are identical (no delta residual and no auxiliary-replacement scan). It
//! also absorbs the semantic-stream preparation so the decode geometry path reads its
//! semantic bytes from here rather than recomputing them.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::decode::Scan;
use crate::intersection::{self, CurveScan};
use crate::parasolid::{Stream, StreamKind};
use crate::topology::{self, BlendSurface, Graph, OffsetSurface, SurfaceCurve, TrimmedCurve};

/// The delta-extended semantic bytes per stream: the topology-merged view plus each
/// unpaired delta stream's semantic residual, with paired delta streams folded into
/// their partition and then cleared. This is the byte view the decode geometry path's
/// scanners read.
pub(crate) fn semantic_streams(scan: &Scan) -> Vec<Vec<u8>> {
    let mut semantic = topology_streams(scan);
    let pairs = paired_delta_streams(scan);
    let paired_deltas = pairs.values().flatten().copied().collect::<BTreeSet<_>>();
    for (delta, stream) in scan.streams.iter().enumerate() {
        if stream.kind == StreamKind::Deltas && !paired_deltas.contains(&delta) {
            semantic[delta].extend_from_slice(&crate::deltas::semantic_residual(&stream.inflated));
        }
    }
    for (partition, deltas) in pairs {
        for delta in deltas {
            semantic[partition].extend_from_slice(&crate::deltas::semantic_residual(
                &scan.streams[delta].inflated,
            ));
            semantic[delta].clear();
        }
    }
    semantic
}

/// The topology-merged bytes per stream: each stream's inflated bytes with delta
/// full-record merges applied. Unpaired delta streams that carry records or tombstones
/// are merged against an empty partition; paired delta streams are merged into their
/// partition and then cleared.
pub(crate) fn topology_streams(scan: &Scan) -> Vec<Vec<u8>> {
    let mut semantic = scan
        .streams
        .iter()
        .map(|stream| stream.inflated.clone())
        .collect::<Vec<_>>();
    let pairs = paired_delta_streams(scan);
    let paired_deltas = pairs.values().flatten().copied().collect::<BTreeSet<_>>();
    for (delta, stream) in scan.streams.iter().enumerate() {
        if stream.kind == StreamKind::Deltas && !paired_deltas.contains(&delta) {
            let census = crate::deltas::walk(&stream.inflated);
            if !census.records.is_empty() || !census.tombstones.is_empty() {
                semantic[delta] = crate::deltas::merge_full_records(&[], &stream.inflated);
            }
        }
    }
    for (partition, deltas) in pairs {
        for delta in deltas {
            semantic[partition] =
                crate::deltas::merge_full_records(&semantic[partition], &semantic[delta]);
            semantic[delta].clear();
        }
    }
    semantic
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
    pub(crate) graph: Graph,
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
            graph: Graph::default(),
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
    fn parse_uniform(bytes: &[u8]) -> Self {
        StreamView {
            graph: Graph::parse(bytes),
            offset_surfaces: topology::offset_surfaces(bytes),
            blend_surfaces: topology::blend_surfaces(bytes),
            trimmed_curves: topology::trimmed_curves(bytes),
            surface_curves: topology::surface_curves(bytes),
            intersections: intersection::scan(bytes),
        }
    }

    /// Parse the semantic view: `graph` from the topology-merged bytes, the scanners
    /// from the delta-extended semantic bytes, and `intersections` via the
    /// auxiliary-replacement scan when this stream has paired delta streams.
    fn parse_semantic(
        topology_bytes: &[u8],
        semantic_bytes: &[u8],
        scan: &Scan,
        paired_deltas: Option<&Vec<usize>>,
    ) -> Self {
        let intersections = if let Some(delta_indices) = paired_deltas {
            let replacement_streams = delta_indices
                .iter()
                .map(|delta| scan.streams[*delta].inflated.as_slice())
                .collect::<Vec<_>>();
            intersection::scan_with_auxiliary_replacements(
                semantic_bytes,
                topology_bytes,
                &replacement_streams,
            )
        } else {
            intersection::scan(semantic_bytes)
        };
        StreamView {
            graph: Graph::parse(topology_bytes),
            offset_surfaces: topology::offset_surfaces(semantic_bytes),
            blend_surfaces: topology::blend_surfaces(semantic_bytes),
            trimmed_curves: topology::trimmed_curves(semantic_bytes),
            surface_curves: topology::surface_curves(semantic_bytes),
            intersections,
        }
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
/// path's NURBS and candidate scanners still read directly.
pub(crate) struct ParsedStreams {
    per_stream: Vec<StreamParses>,
    semantic_streams: Vec<Vec<u8>>,
}

impl ParsedStreams {
    /// Prepare the semantic and topology byte views, then parse every family once per
    /// byte view. Non-Parasolid streams get empty views. A stream's raw and semantic
    /// views share one parse when the topology-merged and delta-extended byte views
    /// both equal `stream.inflated` and the stream has no auxiliary-replacement deltas.
    pub(crate) fn parse(scan: &Scan) -> Self {
        let semantic_streams = semantic_streams(scan);
        let topology_streams = topology_streams(scan);
        let delta_pairs = paired_delta_streams(scan);

        let per_stream = scan
            .streams
            .iter()
            .enumerate()
            .map(|(si, stream)| {
                if !stream.kind.is_parasolid() {
                    let empty = Rc::new(StreamView::empty());
                    return StreamParses {
                        raw: empty.clone(),
                        semantic: empty,
                    };
                }
                let raw = Rc::new(StreamView::parse_uniform(&stream.inflated));
                let paired = delta_pairs.get(&si);
                let identical = paired.is_none()
                    && topology_streams[si] == stream.inflated
                    && semantic_streams[si] == stream.inflated;
                let semantic = if identical {
                    Rc::clone(&raw)
                } else {
                    Rc::new(StreamView::parse_semantic(
                        &topology_streams[si],
                        &semantic_streams[si],
                        scan,
                        paired,
                    ))
                };
                StreamParses { raw, semantic }
            })
            .collect();

        ParsedStreams {
            per_stream,
            semantic_streams,
        }
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
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use std::io::{Cursor, Write};

    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    use cadmpeg_ir::codec::{Codec, CodecEntry, Confidence, DecodeOptions};
    use cadmpeg_ir::geometry::{
        BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry,
        ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::report::LossCategory;
    use cadmpeg_ir::Exactness;

    use crate::container;
    use crate::parasolid::{self, StreamKind};
    use crate::test_support::*;
    use crate::NxCodec;

    use super::*;

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
