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

use std::rc::Rc;

use crate::decode::{self, Scan};
use crate::intersection::{self, CurveScan};
use crate::topology::{self, BlendSurface, Graph, OffsetSurface, SurfaceCurve, TrimmedCurve};

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
        let semantic_streams = decode::semantic_streams(scan);
        let topology_streams = decode::topology_streams(scan);
        let delta_pairs = decode::paired_delta_streams(scan);

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
