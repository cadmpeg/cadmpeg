// SPDX-License-Identifier: Apache-2.0
//! Body-relative exact-read spans for the opt-in read ledger.
//!
//! [`node_spans`] names the nodes the node readers admit in each stream of a
//! site that the decoder reads from its node sequence, one span over each
//! node's complete range. [`collect`] re-runs the scan admission predicates
//! over a stream body the decoder scans and records the byte range each
//! admitted record interprets. Topology spans name admitted records. Analytic spans name
//! every carrier the carrier scan admits. Spline spans name the wrapper,
//! descriptor, and semantic array values of every admitted NURBS carrier; the
//! wrapper, descriptor, and array spans of one carrier are separate entries.
//! Array value spans cover the semantic prefix only: physical float slots past
//! the descriptor knot counts are not recorded. The decode path never calls
//! this module, so collection cannot change decode results, admission, or
//! selection.

/// One interpreted byte range: body-relative offset and length, record tag,
/// and source record id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RawSpan {
    pub(crate) offset: usize,
    pub(crate) length: usize,
    pub(crate) tag: u8,
    pub(crate) record: u32,
}

impl RawSpan {
    pub(crate) fn at(offset: usize, length: usize, tag: u8, record: u32) -> Option<Self> {
        if length == 0 {
            return None;
        }
        Some(Self {
            offset,
            length,
            tag,
            record,
        })
    }
}

/// Every exact-read span of one Parasolid stream body, ordered by offset.
/// `plain` reads a plain stream (spec §3.3). `Some(deltas)` selects the
/// analytic record framing of one stream role.
pub(crate) fn collect(stream_body: &[u8], plain: bool, role: Option<bool>) -> Vec<RawSpan> {
    let mut spans = Vec::new();
    spans.extend(super::analytic_spans(stream_body, role));
    spans.extend(super::topology::topology_spans(stream_body, plain));
    spans.extend(super::spline::spline_spans(stream_body));
    spans.sort_by(|left, right| {
        (left.offset, left.length, left.tag, left.record).cmp(&(
            right.offset,
            right.length,
            right.tag,
            right.record,
        ))
    });
    spans
}

/// The exact-read spans of a stream body that the decoder scans, with the
/// analytic record framing of the stream's role.
pub(crate) fn scanned(
    stream_body: &[u8],
    header: &crate::parasolid::StreamHeader,
    plain: bool,
) -> Vec<RawSpan> {
    collect(
        stream_body,
        plain,
        Some(super::graph::is_deltas_stream(header)),
    )
}

/// The exact-read spans of each stream of one site, in the order of
/// `bodies`, for the streams the decoder reads from their node sequence:
/// one span over the complete range of each node that a node reader admits.
/// A family whose node reader returns no result names its scanned spans, and
/// the decoder scans that family. `None` names a stream the decoder scans.
pub(crate) fn node_spans(
    bodies: &[(&[u8], &crate::parasolid::StreamHeader)],
) -> Vec<Option<Vec<RawSpan>>> {
    use super::topology::nodes;
    let deltas: Vec<_> = bodies
        .iter()
        .map(|(payload, header)| {
            super::graph::is_deltas_stream(header)
                .then(|| cadmpeg_parasolid::records::DeltasRecords::read(payload).ok())
                .flatten()
                .filter(|records| {
                    *records.end() == cadmpeg_parasolid::records::deltas::DeltasEnd::Complete
                })
        })
        .collect();
    let agreement = nodes::DeltasAgreement::new(
        deltas
            .iter()
            .zip(bodies)
            .filter_map(|(records, (payload, _))| Some((records.as_ref()?, *payload))),
    );
    let mut out: Vec<Option<Vec<RawSpan>>> = Vec::new();
    for ((payload, header), records) in bodies.iter().zip(&deltas) {
        let offset = header.body_offset;
        let body = payload.get(offset..).unwrap_or_default();
        let spans = if super::graph::is_deltas_stream(header) {
            records.as_ref().and_then(|records| {
                let nodes = nodes::consistent_deltas_nodes(records, &agreement);
                super::typed::nodes::facts_of(nodes.iter(), offset)?;
                let tables = nodes::deltas_tables(records, payload, offset, &agreement)?;
                Some(admitted(&nodes, body, offset, &tables, Some(true)))
            })
        } else {
            nodes::sequence(payload).and_then(|records| {
                let tables = nodes::tables(&records, payload, offset)?;
                super::typed::nodes::facts(&records, offset)?;
                let nodes: Vec<_> = records.nodes().collect();
                Some(admitted(&nodes, body, offset, &tables, None))
            })
        };
        out.push(spans);
    }
    let deltas_from_nodes = bodies
        .iter()
        .zip(&out)
        .all(|((_, header), spans)| !super::graph::is_deltas_stream(header) || spans.is_some());
    if !deltas_from_nodes {
        for ((_, header), spans) in bodies.iter().zip(&mut out) {
            if super::graph::is_deltas_stream(header) {
                *spans = None;
            }
        }
    }
    out
}

/// The spans of the admitted topology, analytic, and spatial NURBS nodes of
/// one stream, in offset order. `role` is the analytic record framing the
/// decoder scans when the analytic node reader returns no result.
fn admitted<N: cadmpeg_parasolid::records::NodeFields>(
    nodes: &[N],
    body: &[u8],
    body_offset: usize,
    tables: &super::topology::Tables,
    role: Option<bool>,
) -> Vec<RawSpan> {
    use cadmpeg_parasolid::records::node_type::{
        BSPLINE_VERTICES, B_CURVE, B_SURFACE, EDGE, FACE, FIN, KNOT_MULT, KNOT_SET, LOOP,
        NURBS_CURVE, NURBS_SURF, POINT, VERTEX,
    };
    let mut keep = std::collections::HashSet::<(u16, u32)>::new();
    for (kind, indices) in [
        (FACE, tables.bridges().keys().copied().collect::<Vec<_>>()),
        (LOOP, tables.loops().keys().copied().collect()),
        (EDGE, tables.edge_uses().keys().copied().collect()),
        (FIN, tables.coedges().keys().copied().collect()),
        (VERTEX, tables.vertex_uses().keys().copied().collect()),
        (POINT, tables.points().keys().copied().collect()),
    ] {
        keep.extend(indices.into_iter().map(|index| (kind, index)));
    }
    let mut spans = Vec::new();
    let analytic = super::carrier_nodes::analytic_of(nodes, body_offset);
    let analytic_attrs: std::collections::HashSet<u32> = match &analytic {
        Some(carriers) => carriers
            .iter()
            .map(|carrier| match carrier {
                super::Carrier::Curve(curve) => curve.attr,
                super::Carrier::Surface(surface) => surface.attr,
            })
            .collect(),
        None => {
            spans.extend(super::analytic_spans(body, role));
            std::collections::HashSet::new()
        }
    };
    let by_index: std::collections::HashMap<(u16, u32), &N> = nodes
        .iter()
        .map(|node| ((node.kind(), node.index()), node))
        .collect();
    let mut refusals = Vec::new();
    match super::spline::nodes::carriers_of(nodes, body_offset, &mut refusals) {
        Some(splines) => {
            let spline_kinds: std::collections::HashMap<u32, u16> = nodes
                .iter()
                .filter(|node| {
                    matches!(
                        node.kind(),
                        BSPLINE_VERTICES | KNOT_MULT | KNOT_SET | NURBS_CURVE | NURBS_SURF
                    )
                })
                .map(|node| (node.index(), node.kind()))
                .collect();
            let wrappers = splines
                .curves
                .keys()
                .map(|index| (B_CURVE, *index))
                .chain(splines.surfaces.keys().map(|index| (B_SURFACE, *index)));
            for (kind, index) in wrappers {
                keep.insert((kind, index));
                let Some(wrapper) = by_index.get(&(kind, index)) else {
                    continue;
                };
                let (descriptor_kind, arrays): (u16, &[&str]) = if kind == B_CURVE {
                    (NURBS_CURVE, &["bspline_vertices", "knot_mult", "knots"])
                } else {
                    (
                        NURBS_SURF,
                        &[
                            "bspline_vertices",
                            "u_knot_mult",
                            "v_knot_mult",
                            "u_knots",
                            "v_knots",
                        ],
                    )
                };
                let Some(descriptor) = wrapper.pointer("nurbs") else {
                    continue;
                };
                keep.insert((descriptor_kind, descriptor));
                let Some(descriptor) = by_index.get(&(descriptor_kind, descriptor)) else {
                    continue;
                };
                for field in arrays {
                    let array = descriptor
                        .pointer(field)
                        .and_then(|index| Some((*spline_kinds.get(&index)?, index)));
                    keep.extend(array);
                }
            }
        }
        None => spans.extend(super::spline::spline_spans(body)),
    }
    for node in nodes {
        let admitted = keep.contains(&(node.kind(), node.index()))
            || (analytic_attrs.contains(&node.index())
                && super::carrier_nodes::is_analytic_kind(node.kind()));
        if !admitted {
            continue;
        }
        let range = node.range();
        let (Some(offset), Ok(tag)) = (
            range.start.checked_sub(body_offset),
            u8::try_from(node.kind()),
        ) else {
            continue;
        };
        spans.extend(RawSpan::at(offset, range.len(), tag, node.index()));
    }
    spans.sort_by_key(|span| (span.offset, span.length, span.tag, span.record));
    spans
}

#[cfg(test)]
mod tests;
