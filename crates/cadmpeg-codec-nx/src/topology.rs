// SPDX-License-Identifier: Apache-2.0
//! Parse supported fixed-record Parasolid topology.
//!
//! [`Graph`] indexes records by type and stream-scoped XMT identifier. Record
//! offsets connect nodes to carriers returned by [`crate::geometry`] and
//! [`crate::nurbs`]. The parser covers the fixed-record families used by the
//! crate's B-rep reconstruction; unsupported framing and record types are absent
//! from the graph.
#![deny(clippy::disallowed_methods)]

use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Point3;
use std::collections::{BTreeMap, BTreeSet};

use crate::framing::{
    fixed_len, fixed_record_boundary, fixed_record_candidates as framed_record_candidates,
    read_and_advance, read_sequence_at, read_xmt,
};
use crate::vec3_at::vec3_be_at;

/// Exact inline schema header for the `intersection_data` one-byte record
/// family. The terminal `5a` is the record tag; callers use the prefix before
/// that byte as the stream-level schema anchor.
pub(crate) const TYPE_38_SCHEMA_HEADER: &[u8] = &[
    0x00, 0x26, 0x0c, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x41, 0x11,
    0x69, 0x6e, 0x74, 0x65, 0x72, 0x73, 0x65, 0x63, 0x74, 0x69, 0x6f, 0x6e, 0x5f, 0x64, 0x61, 0x74,
    0x61, 0x00, 0xcc, 0x00, 0x01, 0x5a,
];

const TYPE_38_SCHEMA_PREFIX_LEN: usize = TYPE_38_SCHEMA_HEADER.len() - 1;

/// A supported fixed-record node with its XMT identifier and source offset.
#[derive(Debug, Clone)]
pub struct Node {
    /// Parasolid node type.
    pub kind: u8,
    /// Stream-scoped XMT identifier.
    pub xmt: u32,
    /// Record type-tag offset in the inflated stream.
    pub pos: usize,
    shift: usize,
    bytes: Vec<u8>,
}

/// Decoded fields needed from a sequentially framed FACE record.
#[derive(Debug, Clone, Copy)]
pub struct FaceFields {
    /// Attribute-list reference.
    pub attributes: u32,
    /// Face tolerance in Parasolid metres.
    pub tolerance: f64,
    /// Next face in the owning shell, or the null reference.
    pub next_face: u32,
    /// First loop reference.
    pub loop_xmt: u32,
    /// Owning shell reference.
    pub shell: u32,
    /// Surface-carrier reference.
    pub surface: u32,
    /// Stored orientation byte.
    pub sense: u8,
}

/// Decoded fields needed from a sequentially framed EDGE record.
#[derive(Debug, Clone, Copy)]
pub struct EdgeFields {
    /// Attribute-list reference.
    pub attributes: u32,
    /// Edge tolerance in Parasolid metres.
    pub tolerance: f64,
    /// First fin reference.
    pub fin: u32,
    /// Curve-carrier reference.
    pub curve: u32,
}

/// Exact topology witnesses carried by the unique edge using one curve.
#[derive(Debug, Clone, Copy)]
pub struct CurveEdgeWitness {
    /// Ordered model-space edge endpoints in millimetres.
    pub endpoints: [Point3; 2],
    /// Serialized edge tolerance in Parasolid metres.
    pub tolerance: f64,
}

/// Sequentially decoded SHELL references.
#[derive(Debug, Clone, Copy)]
pub struct ShellFields {
    /// Attribute-list reference.
    pub attributes: u32,
    /// Owning body.
    pub body: u32,
    /// Next shell in the owning body.
    pub next_shell: u32,
    /// First face in the shell.
    pub first_face: u32,
    /// First fixed shell sentinel.
    pub sentinel_0: u32,
    /// Second fixed shell sentinel.
    pub sentinel_1: u32,
    /// Owning region.
    pub region: u32,
    /// Face ownership anchor, or null when ownership uses the FACE chain.
    pub last_face: u32,
}

/// Sequentially decoded LOOP references.
#[derive(Debug, Clone, Copy)]
pub struct LoopFields {
    /// Attribute-list reference.
    pub attributes: u32,
    /// First fin in the loop.
    pub fin: u32,
    /// Owning face.
    pub face: u32,
    /// Next loop owned by the same face, or the null reference.
    pub next_loop: u32,
}

/// Sequentially decoded FIN references and sense.
#[derive(Debug, Clone, Copy)]
pub struct FinFields {
    /// Attribute-list reference.
    pub attributes: u32,
    /// Owning loop.
    pub loop_xmt: u32,
    /// Forward fin in the ring.
    pub forward: u32,
    /// Backward fin in the ring.
    pub backward: u32,
    /// Vertex at this fin.
    pub vertex: u32,
    /// Edge carried by this fin.
    pub edge: u32,
    /// Partner fin on the opposite side of the edge.
    pub other: u32,
    /// Curve carried by this fin.
    pub curve_xmt: u32,
    /// Stored orientation byte.
    pub sense: u8,
}

/// Sequentially decoded VERTEX fields.
#[derive(Debug, Clone, Copy)]
pub struct VertexFields {
    /// Attribute-list reference.
    pub attributes: u32,
    /// Referenced point record.
    pub point: u32,
    /// Vertex tolerance in Parasolid metres.
    pub tolerance: f64,
}

impl Node {
    /// Inflated-stream offset of this topology record's attribute-list field.
    pub fn attribute_field_offset(&self) -> Option<usize> {
        match self.kind {
            13..=16 | 18 => Some(self.pos + 8 + self.shift),
            17 => Some(self.pos + 4 + self.shift),
            _ => None,
        }
    }

    /// First byte after this complete record in its source stream.
    pub fn end(&self) -> usize {
        self.pos + self.bytes.len()
    }

    /// Locate the payload following the five-reference compact geometry header.
    pub fn compact_tail_offset(&self) -> Option<usize> {
        let mut at = 8 + self.shift;
        read_sequence_at(&self.bytes, &mut at, 5)?;
        matches!(self.bytes.get(at), Some(b'+' | b'-')).then_some(at + 1)
    }

    /// Decode adjacent references at the start of a compact geometry payload.
    pub fn compact_tail_references(&self, count: usize) -> Option<Vec<u32>> {
        let mut at = self.compact_tail_offset()?;
        read_sequence_at(&self.bytes, &mut at, count)
    }

    /// Read a byte at its logical record offset.
    pub fn byte_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(offset + self.shift).copied()
    }

    /// Read a big-endian floating-point field at its logical record offset.
    pub fn f64_at(&self, offset: usize) -> Option<f64> {
        View::f64_be_at(&self.bytes, offset + self.shift)
    }

    /// Read a big-endian unsigned 32-bit field at a logical record offset.
    pub fn u32_at(&self, offset: usize) -> Option<u32> {
        View::u32_be_at(&self.bytes, offset + self.shift)
    }

    /// Decode FACE fields while accumulating every preceding large-index shift.
    pub fn face_fields(&self) -> Option<FaceFields> {
        (self.kind == 14).then_some(())?;
        let mut at = 8 + self.shift;
        let attributes = read_and_advance(&self.bytes, &mut at)?;
        let tolerance = View::f64_be_at(&self.bytes, at)?;
        at += 8;
        let refs = read_sequence_at(&self.bytes, &mut at, 5)?;
        let sense = *self.bytes.get(at)?;
        matches!(sense, b'+' | b'-').then_some(())?;
        Some(FaceFields {
            attributes,
            tolerance,
            next_face: refs[0],
            loop_xmt: refs[2],
            shell: refs[3],
            surface: refs[4],
            sense,
        })
    }

    /// Decode EDGE fields while accumulating every preceding large-index shift.
    pub fn edge_fields(&self) -> Option<EdgeFields> {
        (self.kind == 16).then_some(())?;
        let mut at = 8 + self.shift;
        let attributes = read_and_advance(&self.bytes, &mut at)?;
        let tolerance = View::f64_be_at(&self.bytes, at)?;
        at += 8;
        let refs = read_sequence_at(&self.bytes, &mut at, 7)?;
        Some(EdgeFields {
            attributes,
            tolerance,
            fin: refs[0],
            curve: refs[3],
        })
    }

    /// Decode SHELL references with cumulative large-index shifts.
    pub fn shell_fields(&self) -> Option<ShellFields> {
        (self.kind == 13).then_some(())?;
        let mut at = 8 + self.shift;
        let refs = read_sequence_at(&self.bytes, &mut at, 8)?;
        Some(ShellFields {
            attributes: refs[0],
            body: refs[1],
            next_shell: refs[2],
            first_face: refs[3],
            sentinel_0: refs[4],
            sentinel_1: refs[5],
            region: refs[6],
            last_face: refs[7],
        })
    }

    /// Decode LOOP references with cumulative large-index shifts.
    pub fn loop_fields(&self) -> Option<LoopFields> {
        (self.kind == 15).then_some(())?;
        let mut at = 8 + self.shift;
        let refs = read_sequence_at(&self.bytes, &mut at, 4)?;
        Some(LoopFields {
            attributes: refs[0],
            fin: refs[1],
            face: refs[2],
            next_loop: refs[3],
        })
    }

    /// Decode FIN references with cumulative large-index shifts.
    pub fn fin_fields(&self) -> Option<FinFields> {
        (self.kind == 17).then_some(())?;
        let mut at = 4 + self.shift;
        let refs = read_sequence_at(&self.bytes, &mut at, 9)?;
        let sense = *self.bytes.get(at)?;
        matches!(sense, b'+' | b'-').then_some(())?;
        Some(FinFields {
            attributes: refs[0],
            loop_xmt: refs[1],
            forward: refs[2],
            backward: refs[3],
            vertex: refs[4],
            other: refs[5],
            edge: refs[6],
            curve_xmt: refs[7],
            sense,
        })
    }

    /// Decode VERTEX fields with cumulative large-index shifts.
    pub fn vertex_fields(&self) -> Option<VertexFields> {
        (self.kind == 18).then_some(())?;
        let mut at = 8 + self.shift;
        let refs = read_sequence_at(&self.bytes, &mut at, 5)?;
        let tolerance = View::f64_be_at(&self.bytes, at)?;
        Some(VertexFields {
            attributes: refs[0],
            point: refs[4],
            tolerance,
        })
    }

    /// Decode a fully framed POINT position into model millimeters.
    pub fn point_position(&self) -> Option<Point3> {
        (self.kind == 29).then_some(())?;
        let mut at = 8 + self.shift;
        read_sequence_at(&self.bytes, &mut at, 4)?;
        let xyz = vec3_be_at(&self.bytes, at)?;
        xyz.iter()
            .all(|value| value.is_finite() && (*value * 1000.0).is_finite())
            .then(|| Point3::new(xyz[0] * 1000.0, xyz[1] * 1000.0, xyz[2] * 1000.0))
    }

    /// Decode this graph-owned fixed analytic surface carrier.
    pub fn surface_geometry(&self) -> Option<cadmpeg_ir::geometry::SurfaceGeometry> {
        matches!(self.kind, 50..=54).then_some(())?;
        let payload_shift = self.compact_tail_offset()?.checked_sub(19)?;
        crate::geometry::decode_surface_record(&self.bytes, self.kind, payload_shift)
    }

    /// Decode this graph-owned fixed analytic curve carrier.
    pub fn curve_geometry(&self) -> Option<cadmpeg_ir::geometry::CurveGeometry> {
        matches!(self.kind, 30..=32).then_some(())?;
        let payload_shift = self.compact_tail_offset()?.checked_sub(19)?;
        crate::geometry::decode_curve_record(&self.bytes, self.kind, payload_shift)
    }
}

/// An index of supported records keyed by `(node type, XMT identifier)`.
#[derive(Debug, Default)]
pub struct Graph {
    nodes: BTreeMap<(u8, u32), Node>,
    by_pos: BTreeMap<usize, (u8, u32)>,
}

/// A type-133 parameter restriction over a basis curve.
#[derive(Debug, Clone, Copy)]
pub struct TrimmedCurve {
    /// Cross-reference index (XMT) of the tag-133 record.
    pub xmt: u32,
    /// Cross-reference index of the untrimmed basis curve record.
    pub basis: u32,
    /// Stored start and end points in millimetres.
    pub points: [[f64; 3]; 2],
    /// `[start, end]` parameter range of the trim, in the basis curve's own parameterization.
    pub parameters: [f64; 2],
    /// Record type-tag offset in the inflated stream.
    pub pos: usize,
}

/// A type-137 curve-on-surface wrapper.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceCurve {
    /// Cross-reference index of the `SP_CURVE` record.
    pub xmt: u32,
    /// Supporting surface reference.
    pub surface: u32,
    /// Dimension-2 `B_CURVE` reference.
    pub pcurve: u32,
    /// Original model-space curve reference.
    pub original: u32,
    /// Fit tolerance to the original curve, in Parasolid metres.
    pub tolerance: f64,
    /// Record type-tag offset in the inflated stream.
    pub pos: usize,
}

/// A type-60 offset surface referencing its support carrier.
#[derive(Debug, Clone, Copy)]
pub struct OffsetSurface {
    /// Cross-reference index of the offset surface record.
    pub xmt: u32,
    /// Serialized `V`, `I`, or `U` discriminator.
    pub discriminator: char,
    /// Serialized true-offset flag.
    pub true_offset: bool,
    /// Cross-reference index of the support surface.
    pub support: u32,
    /// Signed offset distance in millimetres.
    pub distance: f64,
    /// Record type-tag offset in the inflated stream.
    pub pos: usize,
}

/// A type-56 rolling-ball blend surface.
#[derive(Debug, Clone, Copy)]
pub struct BlendSurface {
    /// Cross-reference index of the blend surface record.
    pub xmt: u32,
    /// Ordered support-surface references.
    pub supports: [u32; 2],
    /// Ball-centre spine curve reference.
    pub spine: u32,
    /// Signed support offsets in millimetres.
    pub offsets: [f64; 2],
    /// Dimensionless thumb weights in support order.
    pub thumb_weights: [f64; 2],
    /// Record type-tag offset in the inflated stream.
    pub pos: usize,
}

/// A type-38 surface-intersection construction record.
#[derive(Debug, Clone, Copy)]
pub struct CompositeCurve {
    /// Cross-reference index of the curve record.
    pub xmt: u32,
    /// Five ordered common-header references.
    pub header_references: [u32; 5],
    /// Serialized orientation sense.
    pub sense: bool,
    /// Six ordered construction references.
    pub references: [u32; 6],
    /// Whether the record uses the single-byte delta-twin tag.
    pub delta_twin: bool,
    /// Record type-tag offset in the inflated stream.
    pub pos: usize,
}

/// Decode validated type-38 surface-intersection construction records.
pub fn composite_curves(stream: &[u8]) -> Vec<CompositeCurve> {
    Graph::parse(stream).composite_curves()
}

impl Graph {
    pub(crate) fn composite_curves(&self) -> Vec<CompositeCurve> {
        self.of_kind(38)
            .filter_map(|node| {
                let mut at = 8 + node.shift;
                let header = read_sequence_at(&node.bytes, &mut at, 5)?;
                let sense = match node.bytes.get(at) {
                    Some(b'+') => true,
                    Some(b'-') => false,
                    _ => return None,
                };
                at += 1;
                let references: [u32; 6] =
                    read_sequence_at(&node.bytes, &mut at, 6)?.try_into().ok()?;
                let chart_with_optional_terms =
                    references[2] > 1 && references[3..=4].iter().all(|reference| *reference >= 1);
                let null_witness = references[2..=4].iter().all(|reference| *reference == 1);
                (references.iter().all(|reference| *reference != 0)
                    && (chart_with_optional_terms || null_witness)
                    && (references[0] > 1 || references[1] > 1))
                    .then_some(CompositeCurve {
                        xmt: node.xmt,
                        header_references: header.try_into().ok()?,
                        sense,
                        references,
                        delta_twin: false,
                        pos: node.pos,
                    })
            })
            .collect()
    }
}

/// Decode single-byte `0x5a` intersection-data construction records.
pub fn intersection_data_curves(stream: &[u8]) -> Vec<CompositeCurve> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut schema_anchor_seen = false;
    for (pos, byte) in stream.iter().enumerate() {
        schema_anchor_seen |= intersection_data_schema_prefix_at(stream, pos);
        if *byte != 0x5a || !schema_anchor_seen {
            continue;
        }
        let Some((curve, _)) = intersection_data_curve_at(stream, pos, schema_anchor_seen) else {
            continue;
        };
        if !seen.insert(curve.xmt) {
            continue;
        }
        out.push(curve);
    }
    out
}

/// Return whether the exact type-38 schema prefix starts at `offset`.
pub(crate) fn intersection_data_schema_prefix_at(stream: &[u8], offset: usize) -> bool {
    stream.get(offset..offset.saturating_add(TYPE_38_SCHEMA_PREFIX_LEN))
        == Some(&TYPE_38_SCHEMA_HEADER[..TYPE_38_SCHEMA_PREFIX_LEN])
}

pub(crate) fn intersection_data_curve_at(
    stream: &[u8],
    pos: usize,
    schema_anchor_seen: bool,
) -> Option<(CompositeCurve, usize)> {
    (stream.get(pos) == Some(&0x5a)).then_some(())?;
    schema_anchor_seen.then_some(())?;
    let (xmt, xmt_extra) = read_xmt(stream, pos.checked_add(1)?)?;
    (xmt > 1).then_some(())?;
    let mut at = pos.checked_add(7 + xmt_extra)?;
    let mut header_references = [0u32; 5];
    for reference in &mut header_references {
        let (value, extra) = read_xmt(stream, at)?;
        *reference = value;
        at += 2 + extra;
    }
    (header_references[0] == 1).then_some(())?;
    let sense = match stream.get(at) {
        Some(b'+') => true,
        Some(b'-') => false,
        _ => return None,
    };
    at += 1;
    let mut references = [0u32; 6];
    for reference in &mut references {
        let (value, extra) = read_xmt(stream, at)?;
        *reference = value;
        at += 2 + extra;
    }
    let complete_witness = references[2..=4].iter().all(|reference| *reference > 1);
    let null_witness = references[2..=4].iter().all(|reference| *reference == 1);
    (references.iter().all(|reference| *reference != 0)
        && (complete_witness || null_witness)
        && (references[0] > 1 || references[1] > 1))
        .then_some(())?;
    Some((
        CompositeCurve {
            xmt,
            header_references,
            sense,
            references,
            delta_twin: true,
            pos,
        },
        at,
    ))
}

/// Decode validated type-56 rolling-ball blend surfaces.
pub fn blend_surfaces(stream: &[u8]) -> Vec<BlendSurface> {
    Graph::parse(stream).blend_surfaces()
}

impl Graph {
    pub(crate) fn blend_surfaces(&self) -> Vec<BlendSurface> {
        self.of_kind(56)
            .filter_map(|node| {
                let mut at = node.compact_tail_offset()?;
                (*node.bytes.get(at)? == b'R').then_some(())?;
                at += 1;
                let refs = read_sequence_at(&node.bytes, &mut at, 3)?;
                let values = [
                    View::f64_be_at(&node.bytes, at)?,
                    View::f64_be_at(&node.bytes, at + 8)?,
                    View::f64_be_at(&node.bytes, at + 16)?,
                    View::f64_be_at(&node.bytes, at + 24)?,
                ];
                if !values.iter().all(|value| value.is_finite())
                    || node.bytes.get(at + 32..at + 40)? != [0, 1, 0, 1, 0, 1, 0, 1]
                    || refs[0] <= 1
                    || refs[1] <= 1
                    || values[0] == 0.0
                    || values[1] == 0.0
                    || !(values[0] * 1000.0).is_finite()
                    || !(values[1] * 1000.0).is_finite()
                    || (values[0].abs() - values[1].abs()).abs() > 1.0e-9
                {
                    return None;
                }
                Some(BlendSurface {
                    xmt: node.xmt,
                    supports: [refs[0], refs[1]],
                    spine: refs[2],
                    offsets: [values[0] * 1000.0, values[1] * 1000.0],
                    thumb_weights: [values[2], values[3]],
                    pos: node.pos,
                })
            })
            .collect()
    }
}

/// Decode validated type-60 offset-surface records.
pub fn offset_surfaces(stream: &[u8]) -> Vec<OffsetSurface> {
    Graph::parse(stream).offset_surfaces()
}

impl Graph {
    pub(crate) fn offset_surfaces(&self) -> Vec<OffsetSurface> {
        self.of_kind(60)
            .filter_map(|node| {
                let mut at = node.compact_tail_offset()?;
                let discriminator = match node.bytes.get(at)? {
                    b'V' => 'V',
                    b'I' => 'I',
                    b'U' => 'U',
                    _ => return None,
                };
                at += 1;
                let true_offset = match node.bytes.get(at)? {
                    0 => false,
                    1 => true,
                    _ => return None,
                };
                at += 1;
                let support = read_and_advance(&node.bytes, &mut at)?;
                let distance = View::f64_be_at(&node.bytes, at)?;
                let distance = distance * 1000.0;
                (support > 1 && distance.is_finite()).then_some(OffsetSurface {
                    xmt: node.xmt,
                    discriminator,
                    true_offset,
                    support,
                    distance,
                    pos: node.pos,
                })
            })
            .collect()
    }
}

/// Decode type-137 surface-curve records as aliases of their 3D basis curves.
pub fn surface_curves(stream: &[u8]) -> Vec<SurfaceCurve> {
    Graph::parse(stream).surface_curves()
}

impl Graph {
    pub(crate) fn surface_curves(&self) -> Vec<SurfaceCurve> {
        self.of_kind(137)
            .filter_map(|node| {
                let mut at = node.compact_tail_offset()?;
                let refs = read_sequence_at(&node.bytes, &mut at, 3)?;
                let tolerance = View::f64_be_at(&node.bytes, at)?;
                (refs[0] > 1 && refs[1] > 1 && tolerance.is_finite()).then_some(SurfaceCurve {
                    xmt: node.xmt,
                    surface: refs[0],
                    pcurve: refs[1],
                    original: refs[2],
                    tolerance,
                    pos: node.pos,
                })
            })
            .collect()
    }
}

/// Decode supported type-133 trimmed-curve records.
///
/// The result retains the basis-curve reference and parameter range. Topological
/// endpoints come from the corresponding edge and vertex records.
pub fn trimmed_curves(stream: &[u8]) -> Vec<TrimmedCurve> {
    Graph::parse(stream).trimmed_curves()
}

impl Graph {
    pub(crate) fn trimmed_curves(&self) -> Vec<TrimmedCurve> {
        self.of_kind(133)
            .filter_map(|node| {
                let mut at = node.compact_tail_offset()?;
                let basis = read_and_advance(&node.bytes, &mut at)?;
                let mut point_0 = vec3_be_at(&node.bytes, at)?;
                let mut point_1 = vec3_be_at(&node.bytes, at + 24)?;
                if point_0.iter().chain(point_1.iter()).any(|coordinate| {
                    !coordinate.is_finite() || !(*coordinate * 1000.0).is_finite()
                }) {
                    return None;
                }
                for coordinate in point_0.iter_mut().chain(point_1.iter_mut()) {
                    *coordinate *= 1000.0;
                }
                let p0 = View::f64_be_at(&node.bytes, at + 48)?;
                let p1 = View::f64_be_at(&node.bytes, at + 56)?;
                (basis > 1 && p0.is_finite() && p1.is_finite()).then_some(TrimmedCurve {
                    xmt: node.xmt,
                    basis,
                    points: [point_0, point_1],
                    parameters: [p0, p1],
                    pos: node.pos,
                })
            })
            .collect()
    }
}

impl Graph {
    /// Parse supported fixed-record nodes from a neutral-binary stream.
    pub fn parse(stream: &[u8]) -> Self {
        let mut candidates = Vec::new();
        for pos in 0..stream.len().saturating_sub(3) {
            if stream[pos] != 0 {
                continue;
            }
            let kind = stream[pos + 1];
            let Some(len) = fixed_len(kind) else {
                continue;
            };
            candidates.extend(Self::fixed_record_candidates(stream, pos, kind, len));
        }

        let selected = Self::select_unique_candidates(candidates);
        let mut graph = Self::default();
        for node in Self::select_non_overlapping_candidates(stream, selected) {
            let key = (node.kind, node.xmt);
            graph.by_pos.insert(node.pos, key);
            graph.nodes.insert(key, node);
        }
        graph
    }

    fn fixed_record_candidates(stream: &[u8], pos: usize, kind: u8, len: usize) -> Vec<Node> {
        let candidates = framed_record_candidates(stream, pos, kind, len)
            .into_iter()
            .filter_map(|frame| {
                let bytes = stream.get(pos..frame.end)?;
                let node = Node {
                    kind,
                    xmt: frame.xmt,
                    pos,
                    shift: frame.shift,
                    bytes: bytes.to_vec(),
                };
                node.has_valid_family_framing().then_some(node)
            })
            .collect::<Vec<_>>();
        if candidates.len() < 2 {
            return candidates;
        }

        let boundary_candidates = candidates
            .iter()
            .filter(|candidate| fixed_record_boundary(stream, candidate.end()))
            .collect::<Vec<_>>();
        match boundary_candidates.as_slice() {
            [candidate] => vec![(*candidate).clone()],
            _ => Vec::new(),
        }
    }

    /// Keep one complete physical record for each serialized identity.
    ///
    /// A second record with the same `(kind, xmt)` is not a recoverable choice:
    /// the fixed-record grammar provides no discriminator that can make one
    /// authoritative. Invalidate the identity instead of ranking candidates
    /// by topology shape, reference counts, or scan position.
    fn select_unique_candidates(candidates: Vec<Node>) -> Vec<Node> {
        let mut by_key = BTreeMap::<(u8, u32), Vec<Node>>::new();
        for node in candidates {
            by_key.entry((node.kind, node.xmt)).or_default().push(node);
        }
        by_key
            .into_values()
            .filter_map(|nodes| match nodes.as_slice() {
                [node] => Some(node.clone()),
                _ => None,
            })
            .collect()
    }

    /// Discard overlapping candidates when no serialized ownership boundary
    /// identifies which record owns the bytes.
    fn select_non_overlapping_candidates(stream: &[u8], mut nodes: Vec<Node>) -> Vec<Node> {
        nodes.sort_by(|left, right| {
            left.pos
                .cmp(&right.pos)
                .then_with(|| left.end().cmp(&right.end()))
        });
        let mut selected = Vec::new();
        let mut start = 0;
        while let Some(first) = nodes.get(start) {
            if fixed_record_boundary(stream, first.end()) {
                let end = first.end();
                selected.push(first.clone());
                start += 1;
                while nodes
                    .get(start)
                    .is_some_and(|candidate| candidate.pos < end)
                {
                    start += 1;
                }
                continue;
            }
            let mut end = start + 1;
            let mut cluster_end = first.end();
            while nodes
                .get(end)
                .is_some_and(|candidate| candidate.pos < cluster_end)
            {
                cluster_end = cluster_end.max(nodes[end].end());
                end += 1;
            }
            let cluster = &nodes[start..end];
            if let [node] = cluster {
                selected.push(node.clone());
            } else {
                let boundary_candidates = cluster
                    .iter()
                    .filter(|candidate| fixed_record_boundary(stream, candidate.end()))
                    .collect::<Vec<_>>();
                if let [node] = boundary_candidates.as_slice() {
                    selected.push((*node).clone());
                }
            }
            start = end;
        }
        selected
    }

    fn has_node_id(node: &Node) -> bool {
        matches!(
            node.kind,
            13..=16
                | 18..=19
                | 29..=32
                | 38
                | 50..=54
                | 56
                | 60
                | 124
                | 133..=134
                | 137
        ) && node.u32_at(4).is_some_and(|node_id| node_id <= 1_000_000)
    }

    /// Look up a node by record type and XMT identifier.
    pub fn get(&self, kind: u8, xmt: u32) -> Option<&Node> {
        self.nodes.get(&(kind, xmt))
    }

    /// Look up the node whose type tag starts at `pos`.
    pub fn at_pos(&self, pos: usize) -> Option<&Node> {
        let &(kind, xmt) = self.by_pos.get(&pos)?;
        self.get(kind, xmt)
    }

    /// Iterate nodes of one record type in physical record order.
    pub fn of_kind(&self, kind: u8) -> impl Iterator<Item = &Node> {
        self.by_pos.values().filter_map(move |key| {
            let node = self.nodes.get(key)?;
            (node.kind == kind).then_some(node)
        })
    }

    /// Curve identities occupying typed curve-reference slots in the fixed
    /// topology and procedural graph.
    pub fn referenced_curve_xmts(&self) -> BTreeSet<u32> {
        let mut references = BTreeSet::new();
        references.extend(
            self.of_kind(16)
                .filter_map(Node::edge_fields)
                .map(|fields| fields.curve)
                .filter(|reference| *reference > 1),
        );
        references.extend(
            self.of_kind(17)
                .filter_map(Node::fin_fields)
                .map(|fields| fields.curve_xmt)
                .filter(|reference| *reference > 1),
        );
        for node in self.of_kind(56) {
            let Some(mut at) = node.compact_tail_offset() else {
                continue;
            };
            if node.bytes.get(at) != Some(&b'R') {
                continue;
            }
            at += 1;
            if let Some(spine) = read_sequence_at(&node.bytes, &mut at, 3)
                .and_then(|items| items.get(2).copied())
                .filter(|reference| *reference > 1)
            {
                references.insert(spine);
            }
        }
        for node in self.of_kind(133) {
            if let Some(reference) = node
                .compact_tail_references(1)
                .and_then(|items| items.first().copied())
                .filter(|reference| *reference > 1)
            {
                references.insert(reference);
            }
        }
        for node in self.of_kind(137) {
            if let Some(reference) = node
                .compact_tail_references(3)
                .and_then(|items| items.get(2).copied())
                .filter(|reference| *reference > 1)
            {
                references.insert(reference);
            }
        }
        references
    }

    /// Resolve the exact witnesses of the unique edge carrying a curve.
    pub fn unique_curve_edge_witness(&self, curve_xmt: u32) -> Option<CurveEdgeWitness> {
        let edges = self
            .of_kind(16)
            .filter_map(Node::edge_fields)
            .filter(|edge| edge.curve == curve_xmt)
            .collect::<Vec<_>>();
        let [edge] = edges.as_slice() else {
            return None;
        };
        let first_fin = self.get(17, edge.fin)?.fin_fields()?;
        let second_fin = self.get(17, first_fin.forward)?.fin_fields()?;
        let position = |vertex_xmt| {
            let point_xmt = self.get(18, vertex_xmt)?.vertex_fields()?.point;
            self.get(29, point_xmt)?.point_position()
        };
        Some(CurveEdgeWitness {
            endpoints: [position(first_fin.vertex)?, position(second_fin.vertex)?],
            tolerance: edge.tolerance,
        })
    }

    /// Carrier identities required by the surviving fixed topology image.
    pub fn referenced_carrier_xmts(&self) -> BTreeSet<u32> {
        let mut references = self.referenced_curve_xmts();
        references.extend(
            self.of_kind(14)
                .filter_map(Node::face_fields)
                .map(|fields| fields.surface)
                .filter(|reference| *reference > 1),
        );
        references.extend(
            self.of_kind(18)
                .filter_map(Node::vertex_fields)
                .map(|fields| fields.point)
                .filter(|reference| *reference > 1),
        );
        references
    }

    /// Return SHELL nodes whose ownership fields define a body shape.
    pub fn body_shape_shells(&self) -> Vec<&Node> {
        self.of_kind(13)
            .filter(|shell| self.is_body_shape_shell(shell))
            .collect()
    }

    /// Return whether every body-shape face has a non-empty valid loop chain
    /// and every non-null radial FIN partner belongs to the same reachable
    /// body topology.
    pub fn has_complete_body_topology(&self) -> bool {
        let shells = self.body_shape_shells();
        if shells.is_empty() {
            return false;
        }
        let mut reachable_fins = BTreeSet::new();
        for shell in shells {
            let Some(face_xmts) = self.shell_face_xmts(shell) else {
                return false;
            };
            for face_xmt in face_xmts {
                let Some(rings) = self.face_loop_rings(face_xmt) else {
                    return false;
                };
                if rings.is_empty() {
                    return false;
                }
                reachable_fins.extend(rings.into_iter().flat_map(|(_, ring)| ring));
            }
        }
        reachable_fins.iter().all(|xmt| {
            self.get(17, *xmt)
                .and_then(Node::fin_fields)
                .is_some_and(|fields| fields.other == 1 || reachable_fins.contains(&fields.other))
        })
    }

    /// Count faces owned by validated body-shape shells.
    pub fn body_shape_face_count(&self) -> usize {
        self.body_shape_shells()
            .into_iter()
            .filter_map(|shell| self.shell_face_xmts(shell).map(|faces| faces.len()))
            .sum()
    }

    /// Return the validated loop-to-FIN rings owned by a face.
    ///
    /// The face's loop chain must terminate at the null reference. Each loop
    /// points back to the face. Each FIN cycle closes at its first FIN, stays in
    /// the loop, and has reciprocal forward/backward links. Every FIN resolves
    /// its edge and vertex.
    pub fn face_loop_rings(&self, face_xmt: u32) -> Option<Vec<(u32, Vec<u32>)>> {
        let face = self.get(14, face_xmt)?.face_fields()?;
        let mut loop_xmt = face.loop_xmt;
        let mut seen_loops = BTreeSet::new();
        let mut rings = Vec::new();
        while loop_xmt != 1 {
            if !seen_loops.insert(loop_xmt) {
                return None;
            }
            let fields = self.get(15, loop_xmt)?.loop_fields()?;
            if fields.face != face_xmt {
                return None;
            }
            rings.push((loop_xmt, self.fin_ring(loop_xmt, fields.fin)?));
            loop_xmt = fields.next_loop;
        }
        Some(rings)
    }

    fn fin_ring(&self, loop_xmt: u32, first: u32) -> Option<Vec<u32>> {
        (first != 1).then_some(())?;
        let mut current = first;
        let mut previous = None;
        let mut seen = BTreeSet::new();
        let mut ring = Vec::new();
        loop {
            if !seen.insert(current) {
                return (current == first).then_some(ring);
            }
            ring.push(current);
            let fields = self.get(17, current)?.fin_fields()?;
            let vertex_resolves = self.get(18, fields.vertex).is_some()
                || (fields.vertex == 1 && fields.forward == current && fields.backward == current);
            if fields.loop_xmt != loop_xmt
                || self.get(16, fields.edge).is_none()
                || !vertex_resolves
            {
                return None;
            }
            if fields.other != 1 {
                let other = self.get(17, fields.other)?.fin_fields()?;
                if other.other != current || other.edge != fields.edge {
                    return None;
                }
            }
            if let Some(previous) = previous {
                if fields.backward != previous {
                    return None;
                }
            }
            let next = self.get(17, fields.forward)?.fin_fields()?;
            if next.backward != current {
                return None;
            }
            previous = Some(current);
            current = fields.forward;
        }
    }

    fn is_body_shape_shell(&self, shell: &Node) -> bool {
        let Some(fields) = shell.shell_fields() else {
            return false;
        };
        if fields.attributes != 1
            || fields.next_shell != 1
            || fields.sentinel_0 != 1
            || fields.sentinel_1 != 1
            || fields.body <= 1
            || fields.region <= 1
        {
            return false;
        }

        self.shell_face_xmts(shell).is_some()
    }

    pub(crate) fn shell_face_xmts(&self, shell: &Node) -> Option<Vec<u32>> {
        let fields = shell.shell_fields()?;
        if fields.last_face != 1 {
            (fields.last_face == fields.first_face).then_some(())?;
            self.get(14, fields.first_face)
                .and_then(Node::face_fields)
                .filter(|face| face.shell == shell.xmt)?;
            let faces: Vec<_> = self
                .of_kind(14)
                .filter(|face| {
                    face.face_fields()
                        .is_some_and(|fields| fields.shell == shell.xmt)
                })
                .map(|face| face.xmt)
                .collect();
            return (!faces.is_empty()).then_some(faces);
        }

        let mut face_xmt = fields.first_face;
        let mut visited = BTreeSet::new();
        while face_xmt != 1 {
            if !visited.insert(face_xmt) {
                return None;
            }
            let face = self.get(14, face_xmt).and_then(Node::face_fields)?;
            if face.shell != shell.xmt {
                return None;
            }
            face_xmt = face.next_face;
        }
        (!visited.is_empty()).then(|| visited.into_iter().collect())
    }
}

impl Node {
    fn has_valid_family_framing(&self) -> bool {
        if matches!(
            self.kind,
            13..=16
                | 18..=19
                | 29..=32
                | 38
                | 50..=54
                | 56
                | 60
                | 124
                | 133..=134
                | 137
        ) && !Graph::has_node_id(self)
        {
            return false;
        }
        match self.kind {
            13 => self.shell_fields().is_some(),
            14 => self
                .face_fields()
                .is_some_and(|fields| fields.tolerance.is_finite()),
            15 => self.loop_fields().is_some(),
            16 => self
                .edge_fields()
                .is_some_and(|fields| fields.tolerance.is_finite()),
            17 => self.fin_fields().is_some(),
            18 => self
                .vertex_fields()
                .is_some_and(|fields| fields.tolerance.is_finite()),
            29 => self.point_position().is_some(),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests;
