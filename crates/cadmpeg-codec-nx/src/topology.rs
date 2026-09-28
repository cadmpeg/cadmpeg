// SPDX-License-Identifier: Apache-2.0
//! Parse supported fixed-record Parasolid topology.
//!
//! [`Graph`] indexes records by type and stream-scoped XMT identifier. Record
//! offsets connect nodes to carriers returned by [`crate::geometry`] and
//! [`crate::nurbs`]. The parser covers the fixed-record families used by the
//! crate's B-rep reconstruction; unsupported framing and record types are absent
//! from the graph.
#![deny(clippy::disallowed_methods)]

use crate::framing::node_kind::NodeKind;
use crate::framing::xmt_reference::XmtTarget;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Sense;
use std::collections::{BTreeMap, BTreeSet};

use crate::framing::{
    fixed_len, fixed_record_boundary, fixed_record_candidates as framed_record_candidates,
    read_and_advance, read_sequence_at, read_xmt, skip_sequence_at,
};
use crate::vec3_at::vec3_be_at;
pub(crate) mod trimmed_curve_state;
use trimmed_curve_state::TrimmedCurveState;
pub(crate) mod blend_surface_state;
use blend_surface_state::BlendSurfaceState;
pub(crate) mod offset_surface_state;
use offset_surface_state::OffsetSurfaceState;
pub(crate) mod surface_curve_state;
use surface_curve_state::SurfaceCurveState;

/// Exact inline schema header for the `intersection_data` one-byte record
/// family. Its terminal `5a` is also the standalone record tag when the
/// following fields form a complete shared record.
pub(crate) const TYPE_38_SCHEMA_HEADER: &[u8] = &[
    0x00, 0x26, 0x0c, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x41, 0x11,
    0x69, 0x6e, 0x74, 0x65, 0x72, 0x73, 0x65, 0x63, 0x74, 0x69, 0x6f, 0x6e, 0x5f, 0x64, 0x61, 0x74,
    0x61, 0x00, 0xcc, 0x00, 0x01, 0x5a,
];

/// A supported fixed-record node with its XMT identifier and source offset.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    /// Parasolid node type.
    kind: NodeKind,
    /// Stream-scoped XMT identifier.
    pub(crate) xmt: u32,
    /// Record type-tag offset in the inflated stream.
    pub(crate) pos: usize,
    shift: usize,
    bytes: Vec<u8>,
}

/// Decoded fields needed from a sequentially framed FACE record.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FaceFields {
    /// Attribute-list reference.
    pub(crate) attributes: Option<XmtTarget>,
    /// Face tolerance in Parasolid metres.
    pub(crate) tolerance: f64,
    /// Next face in the owning shell, or the null reference.
    next_face: Option<XmtTarget>,
    /// First loop reference.
    loop_xmt: Option<XmtTarget>,
    /// Owning shell reference.
    pub(crate) shell: Option<XmtTarget>,
    /// Surface-carrier reference.
    pub(crate) surface: Option<XmtTarget>,
    /// Decoded orientation.
    pub(crate) sense: Sense,
}

/// Decoded fields needed from a sequentially framed EDGE record.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EdgeFields {
    /// Attribute-list reference.
    pub(crate) attributes: Option<XmtTarget>,
    /// Edge tolerance in Parasolid metres.
    pub(crate) tolerance: f64,
    /// First fin reference.
    pub(crate) fin: Option<XmtTarget>,
    /// Curve-carrier reference.
    pub(crate) curve: Option<XmtTarget>,
}

/// Exact topology witnesses carried by the unique edge using one curve.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CurveEdgeWitness {
    /// Ordered model-space edge endpoints in millimetres.
    pub(crate) endpoints: [FinitePoint3; 2],
    /// Serialized edge tolerance in Parasolid metres.
    pub(crate) tolerance: f64,
}

/// Sequentially decoded SHELL references.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ShellFields {
    /// Attribute-list reference.
    pub(crate) attributes: Option<XmtTarget>,
    /// Owning body.
    pub(crate) body: Option<XmtTarget>,
    /// Next shell in the owning body.
    next_shell: Option<XmtTarget>,
    /// First face in the shell.
    first_face: Option<XmtTarget>,
    /// First fixed shell sentinel.
    sentinel_0: Option<XmtTarget>,
    /// Second fixed shell sentinel.
    sentinel_1: Option<XmtTarget>,
    /// Owning region.
    pub(crate) region: Option<XmtTarget>,
    /// Face ownership anchor, or null when ownership uses the FACE chain.
    last_face: Option<XmtTarget>,
}

/// Sequentially decoded LOOP references.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopFields {
    /// Attribute-list reference.
    pub(crate) attributes: Option<XmtTarget>,
    /// First fin in the loop.
    fin: Option<XmtTarget>,
    /// Owning face.
    pub(crate) face: Option<XmtTarget>,
    /// Next loop owned by the same face, or the null reference.
    next_loop: Option<XmtTarget>,
}

/// Sequentially decoded FIN references and sense.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FinFields {
    /// Attribute-list reference.
    pub(crate) attributes: Option<XmtTarget>,
    /// Owning loop.
    pub(crate) loop_xmt: Option<XmtTarget>,
    /// Forward fin in the ring.
    pub(crate) forward: Option<XmtTarget>,
    /// Backward fin in the ring.
    pub(crate) backward: Option<XmtTarget>,
    /// Vertex at this fin.
    pub(crate) vertex: Option<XmtTarget>,
    /// Edge carried by this fin.
    pub(crate) edge: Option<XmtTarget>,
    /// Partner fin on the opposite side of the edge.
    pub(crate) other: Option<XmtTarget>,
    /// Curve carried by this fin.
    pub(crate) curve_xmt: Option<XmtTarget>,
    /// Decoded orientation.
    pub(crate) sense: Sense,
}

/// Why a face's linked loop boundary cannot be admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FaceLoopFailure {
    InvalidFace {
        face_xmt: u32,
    },
    InvalidLoopChain {
        loop_xmt: u32,
    },
    InvalidFinRing {
        loop_xmt: u32,
        fin_xmt: u32,
    },
    UnresolvedFinEdge {
        loop_xmt: u32,
        fin_xmt: u32,
        edge_xmt: Option<u32>,
    },
}

#[derive(Debug)]
pub(crate) enum FaceLoopError {
    Invalid(FaceLoopFailure),
    Codec(CodecError),
}

impl From<FaceLoopFailure> for FaceLoopError {
    fn from(failure: FaceLoopFailure) -> Self {
        Self::Invalid(failure)
    }
}

impl From<CodecError> for FaceLoopError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

impl std::fmt::Display for FaceLoopFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFace { face_xmt } => write!(f, "FACE {face_xmt} has no decoded record"),
            Self::InvalidLoopChain { loop_xmt } => {
                write!(f, "LOOP {loop_xmt} breaks the face loop chain")
            }
            Self::InvalidFinRing { loop_xmt, fin_xmt } => {
                write!(f, "FIN {fin_xmt} breaks LOOP {loop_xmt}'s ring")
            }
            Self::UnresolvedFinEdge {
                loop_xmt,
                fin_xmt,
                edge_xmt,
            } => match edge_xmt {
                Some(edge_xmt) => write!(
                    f,
                    "FIN {fin_xmt} in LOOP {loop_xmt} refers to unresolved EDGE {edge_xmt}"
                ),
                None => write!(f, "FIN {fin_xmt} in LOOP {loop_xmt} has no EDGE reference"),
            },
        }
    }
}

/// Sequentially decoded VERTEX fields.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VertexFields {
    /// Attribute-list reference.
    pub(crate) attributes: Option<XmtTarget>,
    /// Referenced point record.
    pub(crate) point: Option<XmtTarget>,
    /// Vertex tolerance in Parasolid metres.
    pub(crate) tolerance: f64,
}

impl Node {
    /// Kernel node identity serialized by fixed topology families.
    pub(crate) fn node_id(&self) -> Option<u32> {
        matches!(
            self.kind,
            NodeKind::Body
                | NodeKind::Shell
                | NodeKind::Face
                | NodeKind::Loop
                | NodeKind::Edge
                | NodeKind::Vertex
                | NodeKind::Region
        )
        .then(|| self.u32_at(4))
        .flatten()
    }

    /// Inflated-stream offset of this topology record's attribute-list field.
    pub(crate) fn attribute_field_offset(&self) -> Option<usize> {
        match self.kind {
            NodeKind::Shell
            | NodeKind::Face
            | NodeKind::Loop
            | NodeKind::Edge
            | NodeKind::Vertex => Some(self.pos + 8 + self.shift),
            NodeKind::Fin => Some(self.pos + 4 + self.shift),
            _ => None,
        }
    }

    /// First byte after this complete record in its source stream.
    pub(crate) fn end(&self) -> usize {
        self.pos + self.bytes.len()
    }

    /// Locate the payload following the five-reference compact geometry header.
    fn compact_tail_offset(&self) -> Option<usize> {
        let mut at = 8 + self.shift;
        skip_sequence_at(&self.bytes, &mut at, 5)?;
        matches!(self.bytes.get(at), Some(b'+' | b'-')).then_some(at + 1)
    }

    /// Decode adjacent references at the start of a compact geometry payload.
    pub(crate) fn compact_tail_references(&self, count: usize) -> Option<Vec<u32>> {
        let mut at = self.compact_tail_offset()?;
        read_sequence_at(&self.bytes, &mut at, count)
    }

    /// Read a byte at its logical record offset.
    pub(crate) fn byte_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(offset + self.shift).copied()
    }

    /// Read a big-endian floating-point field at its logical record offset.
    pub(crate) fn f64_at(&self, offset: usize) -> Option<f64> {
        View::f64_be_at(&self.bytes, offset + self.shift)
    }

    /// Read a big-endian unsigned 32-bit field at a logical record offset.
    pub(crate) fn u32_at(&self, offset: usize) -> Option<u32> {
        View::u32_be_at(&self.bytes, offset + self.shift)
    }

    /// Decode FACE fields while accumulating every preceding large-index shift.
    pub(crate) fn face_fields(&self) -> Option<FaceFields> {
        (self.kind == NodeKind::Face).then_some(())?;
        let mut at = 8 + self.shift;
        let attributes = read_and_advance(&self.bytes, &mut at)?;
        let tolerance = View::f64_be_at(&self.bytes, at)?;
        at += 8;
        let refs = read_sequence_at(&self.bytes, &mut at, 5)?;
        let sense = match self.bytes.get(at) {
            Some(b'+') => Sense::Forward,
            Some(b'-') => Sense::Reversed,
            _ => return None,
        };
        Some(FaceFields {
            attributes: XmtTarget::from_wire(attributes),
            tolerance,
            next_face: XmtTarget::from_wire(refs[0]),
            loop_xmt: XmtTarget::from_wire(refs[2]),
            shell: XmtTarget::from_wire(refs[3]),
            surface: XmtTarget::from_wire(refs[4]),
            sense,
        })
    }

    /// Decode EDGE fields while accumulating every preceding large-index shift.
    pub(crate) fn edge_fields(&self) -> Option<EdgeFields> {
        (self.kind == NodeKind::Edge).then_some(())?;
        let mut at = 8 + self.shift;
        let attributes = read_and_advance(&self.bytes, &mut at)?;
        let tolerance = View::f64_be_at(&self.bytes, at)?;
        at += 8;
        let refs = read_sequence_at(&self.bytes, &mut at, 7)?;
        Some(EdgeFields {
            attributes: XmtTarget::from_wire(attributes),
            tolerance,
            fin: XmtTarget::from_wire(refs[0]),
            curve: XmtTarget::from_wire(refs[3]),
        })
    }

    /// Decode SHELL references with cumulative large-index shifts.
    pub(crate) fn shell_fields(&self) -> Option<ShellFields> {
        (self.kind == NodeKind::Shell).then_some(())?;
        let mut at = 8 + self.shift;
        let refs = read_sequence_at(&self.bytes, &mut at, 8)?;
        Some(ShellFields {
            attributes: XmtTarget::from_wire(refs[0]),
            body: XmtTarget::from_wire(refs[1]),
            next_shell: XmtTarget::from_wire(refs[2]),
            first_face: XmtTarget::from_wire(refs[3]),
            sentinel_0: XmtTarget::from_wire(refs[4]),
            sentinel_1: XmtTarget::from_wire(refs[5]),
            region: XmtTarget::from_wire(refs[6]),
            last_face: XmtTarget::from_wire(refs[7]),
        })
    }

    /// Decode LOOP references with cumulative large-index shifts.
    pub(crate) fn loop_fields(&self) -> Option<LoopFields> {
        (self.kind == NodeKind::Loop).then_some(())?;
        let mut at = 8 + self.shift;
        let refs = read_sequence_at(&self.bytes, &mut at, 4)?;
        Some(LoopFields {
            attributes: XmtTarget::from_wire(refs[0]),
            fin: XmtTarget::from_wire(refs[1]),
            face: XmtTarget::from_wire(refs[2]),
            next_loop: XmtTarget::from_wire(refs[3]),
        })
    }

    /// Decode FIN references with cumulative large-index shifts.
    pub(crate) fn fin_fields(&self) -> Option<FinFields> {
        (self.kind == NodeKind::Fin).then_some(())?;
        let mut at = 4 + self.shift;
        let refs = read_sequence_at(&self.bytes, &mut at, 9)?;
        let sense = match self.bytes.get(at) {
            Some(b'+') => Sense::Forward,
            Some(b'-') => Sense::Reversed,
            _ => return None,
        };
        Some(FinFields {
            attributes: XmtTarget::from_wire(refs[0]),
            loop_xmt: XmtTarget::from_wire(refs[1]),
            forward: XmtTarget::from_wire(refs[2]),
            backward: XmtTarget::from_wire(refs[3]),
            vertex: XmtTarget::from_wire(refs[4]),
            other: XmtTarget::from_wire(refs[5]),
            edge: XmtTarget::from_wire(refs[6]),
            curve_xmt: XmtTarget::from_wire(refs[7]),
            sense,
        })
    }

    /// Decode VERTEX fields with cumulative large-index shifts.
    pub(crate) fn vertex_fields(&self) -> Option<VertexFields> {
        (self.kind == NodeKind::Vertex).then_some(())?;
        let mut at = 8 + self.shift;
        let refs = read_sequence_at(&self.bytes, &mut at, 5)?;
        let tolerance = View::f64_be_at(&self.bytes, at)?;
        Some(VertexFields {
            attributes: XmtTarget::from_wire(refs[0]),
            point: XmtTarget::from_wire(refs[4]),
            tolerance,
        })
    }

    /// Decode a fully framed POINT position into model millimeters.
    pub(crate) fn point_position(&self) -> Option<FinitePoint3> {
        (self.kind == NodeKind::Point).then_some(())?;
        let mut at = 8 + self.shift;
        skip_sequence_at(&self.bytes, &mut at, 4)?;
        let xyz = vec3_be_at(&self.bytes, at)?;
        FinitePoint3::new(Point3::new(
            xyz[0] * 1000.0,
            xyz[1] * 1000.0,
            xyz[2] * 1000.0,
        ))
    }

    /// Decode this graph-owned fixed analytic surface carrier.
    pub(crate) fn surface_geometry(&self) -> Option<cadmpeg_ir::geometry::SurfaceGeometry> {
        matches!(
            self.kind,
            NodeKind::Plane
                | NodeKind::Cylinder
                | NodeKind::Cone
                | NodeKind::Sphere
                | NodeKind::Torus
        )
        .then_some(())?;
        let payload_shift = self.compact_tail_offset()?.checked_sub(19)?;
        crate::geometry::decode_surface_record(&self.bytes, self.kind, payload_shift)
    }

    /// Decode this graph-owned fixed analytic curve carrier.
    pub(crate) fn curve_geometry(&self) -> Option<cadmpeg_ir::geometry::CurveGeometry> {
        matches!(
            self.kind,
            NodeKind::Line | NodeKind::Circle | NodeKind::Ellipse
        )
        .then_some(())?;
        let payload_shift = self.compact_tail_offset()?.checked_sub(19)?;
        crate::geometry::decode_curve_record(&self.bytes, self.kind, payload_shift)
    }

    fn reference_targets(&self) -> Vec<(ReferenceRole, u32)> {
        let references = match self.kind {
            NodeKind::Shell => self.shell_fields().map_or_else(Vec::new, |fields| {
                vec![
                    (ReferenceRole::Body, fields.body),
                    (ReferenceRole::Region, fields.region),
                ]
            }),
            NodeKind::Face => self.face_fields().map_or_else(Vec::new, |fields| {
                vec![(ReferenceRole::Surface, fields.surface)]
            }),
            NodeKind::Edge => self.edge_fields().map_or_else(Vec::new, |fields| {
                vec![(ReferenceRole::Curve, fields.curve)]
            }),
            NodeKind::Fin => self.fin_fields().map_or_else(Vec::new, |fields| {
                vec![(ReferenceRole::Curve, fields.curve_xmt)]
            }),
            NodeKind::Vertex => self.vertex_fields().map_or_else(Vec::new, |fields| {
                vec![(ReferenceRole::Point, fields.point)]
            }),
            NodeKind::BlendSurface => {
                let Some(mut at) = self.compact_tail_offset() else {
                    return Vec::new();
                };
                if self.bytes.get(at) != Some(&b'R') {
                    return Vec::new();
                }
                at += 1;
                read_sequence_at(&self.bytes, &mut at, 3).map_or_else(Vec::new, |references| {
                    vec![
                        (ReferenceRole::Surface, XmtTarget::from_wire(references[0])),
                        (ReferenceRole::Surface, XmtTarget::from_wire(references[1])),
                        (ReferenceRole::Curve, XmtTarget::from_wire(references[2])),
                    ]
                })
            }
            NodeKind::OffsetSurface => {
                let Some(mut at) = self.compact_tail_offset() else {
                    return Vec::new();
                };
                if !matches!(self.bytes.get(at), Some(b'V' | b'I' | b'U'))
                    || !matches!(self.bytes.get(at + 1), Some(0 | 1))
                {
                    return Vec::new();
                }
                at += 2;
                read_and_advance(&self.bytes, &mut at).map_or_else(Vec::new, |reference| {
                    vec![(ReferenceRole::Surface, XmtTarget::from_wire(reference))]
                })
            }
            NodeKind::TrimmedCurve => {
                let Some(mut at) = self.compact_tail_offset() else {
                    return Vec::new();
                };
                read_and_advance(&self.bytes, &mut at).map_or_else(Vec::new, |reference| {
                    vec![(ReferenceRole::Curve, XmtTarget::from_wire(reference))]
                })
            }
            NodeKind::SpCurve => {
                let Some(mut at) = self.compact_tail_offset() else {
                    return Vec::new();
                };
                read_sequence_at(&self.bytes, &mut at, 3).map_or_else(Vec::new, |references| {
                    vec![
                        (ReferenceRole::Surface, XmtTarget::from_wire(references[0])),
                        (ReferenceRole::Curve, XmtTarget::from_wire(references[1])),
                        (ReferenceRole::Curve, XmtTarget::from_wire(references[2])),
                    ]
                })
            }
            _ => Vec::new(),
        };
        references
            .into_iter()
            .filter_map(|(role, reference)| Some((role, u32::from(reference?))))
            .collect()
    }
}

/// An index of supported records keyed by `(node type, XMT identifier)`.
#[derive(Debug, Default)]
pub(crate) struct Graph {
    nodes: BTreeMap<(NodeKind, u32), Node>,
    by_pos: BTreeMap<usize, (NodeKind, u32)>,
    /// Record keys grouped by kind in their physical stream order.
    by_kind: BTreeMap<NodeKind, Vec<(NodeKind, u32)>>,
}

fn push_scoped<T>(
    ctx: &DecodeContext<'_>,
    reservation: &mut ScopedReservation<'_>,
    values: &mut Vec<T>,
    value: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    reservation.grow(u64_from_index(std::mem::size_of::<T>()))?;
    values
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    values.push(value);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ReferenceRole {
    Body,
    Point,
    Curve,
    Region,
    Surface,
}

/// A type-133 parameter restriction over a basis curve.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TrimmedCurve {
    /// Cross-reference index (XMT) of the tag-133 record.
    pub(crate) xmt: u32,
    pub(crate) state: TrimmedCurveState,
    /// Record type-tag offset in the inflated stream.
    pub(crate) pos: usize,
}

/// A type-137 curve-on-surface wrapper.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SurfaceCurve {
    /// Cross-reference index of the `SP_CURVE` record.
    pub(crate) xmt: u32,
    pub(crate) state: SurfaceCurveState,
    /// Record type-tag offset in the inflated stream.
    pub(crate) pos: usize,
}

/// Admitted serialized offset-surface status discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "char", into = "char")]
pub(crate) enum OffsetSurfaceDiscriminator {
    V,
    I,
    U,
}

impl From<OffsetSurfaceDiscriminator> for char {
    fn from(value: OffsetSurfaceDiscriminator) -> Self {
        match value {
            OffsetSurfaceDiscriminator::V => 'V',
            OffsetSurfaceDiscriminator::I => 'I',
            OffsetSurfaceDiscriminator::U => 'U',
        }
    }
}

impl TryFrom<char> for OffsetSurfaceDiscriminator {
    type Error = &'static str;
    fn try_from(value: char) -> Result<Self, Self::Error> {
        match value {
            'V' => Ok(Self::V),
            'I' => Ok(Self::I),
            'U' => Ok(Self::U),
            _ => Err("invalid offset-surface discriminator"),
        }
    }
}

/// A type-60 offset surface referencing its support carrier.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OffsetSurface {
    /// Cross-reference index of the offset surface record.
    pub(crate) xmt: u32,
    /// Serialized `V`, `I`, or `U` discriminator.
    pub(crate) discriminator: OffsetSurfaceDiscriminator,
    /// Serialized true-offset flag.
    pub(crate) true_offset: bool,
    /// Checked support reference and signed model distance.
    pub(crate) state: OffsetSurfaceState,
    /// Record type-tag offset in the inflated stream.
    pub(crate) pos: usize,
}

/// A type-56 rolling-ball blend surface.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BlendSurface {
    /// Cross-reference index of the blend surface record.
    pub(crate) xmt: u32,
    /// Checked supports, source-admitted offsets, and thumb weights.
    pub(crate) state: BlendSurfaceState,
    /// Record type-tag offset in the inflated stream.
    pub(crate) pos: usize,
}

/// A type-38 surface-intersection construction record.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CompositeCurve {
    /// Cross-reference index of the curve record.
    pub(crate) xmt: u32,
    /// Five ordered common-header references.
    pub(crate) header_references: [Option<XmtTarget>; 5],
    /// Serialized orientation sense.
    pub(crate) sense: bool,
    /// Six ordered construction references.
    pub(crate) references: [Option<XmtTarget>; 6],
    /// Whether the record uses the single-byte delta-twin tag.
    pub(crate) delta_twin: bool,
    /// Record type-tag offset in the inflated stream.
    pub(crate) pos: usize,
}

/// Decode validated type-38 surface-intersection construction records.
pub(crate) fn composite_curves(ctx: &DecodeContext<'_>, stream: &[u8]) -> Result<Vec<CompositeCurve>, CodecError> {
    Graph::parse(ctx, stream)?.composite_curves(ctx)
}

fn collect_graph_records<T>(
    ctx: &DecodeContext<'_>,
    operation: &'static str,
    records: impl Iterator<Item = T>,
) -> Result<Vec<T>, CodecError> {
    let mut out = Vec::new();
    for record in records {
        ctx.charge_collection_items(1, operation)?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<T>()), operation)?;
        out.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
        out.push(record);
    }
    Ok(out)
}

fn insert_reference(
    ctx: &DecodeContext<'_>,
    references: &mut BTreeSet<u32>,
    reference: u32,
) -> Result<(), CodecError> {
    if !references.contains(&reference) {
        ctx.charge_collection_items(1, "NX topology carrier references")?;
    }
    references.insert(reference);
    Ok(())
}

impl Graph {
    pub(crate) fn composite_curves(&self, ctx: &DecodeContext<'_>) -> Result<Vec<CompositeCurve>, CodecError> {
        collect_graph_records(ctx, "NX composite curves", self.of_kind(NodeKind::Intersection)
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
                        header_references: <[u32; 5]>::try_from(header)
                            .ok()?
                            .map(XmtTarget::from_wire),
                        sense,
                        references: references.map(XmtTarget::from_wire),
                        delta_twin: false,
                        pos: node.pos,
                    })
            })
        )
    }
}

/// Decode single-byte `0x5a` intersection-data construction records.
pub(crate) fn intersection_data_curves(ctx: &DecodeContext<'_>, stream: &[u8]) -> Result<Vec<CompositeCurve>, CodecError> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut schema_anchor_seen = false;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(stream.len()), "scan NX intersection data")?;
    for (pos, byte) in stream.iter().enumerate() {
        schema_anchor_seen |= intersection_data_schema_header_at(stream, pos);
        if *byte != 0x5a || !schema_anchor_seen {
            continue;
        }
        let Some((curve, _)) = intersection_data_curve_at(stream, pos, schema_anchor_seen) else {
            continue;
        };
        if seen.contains(&curve.xmt) {
            continue;
        }
        ctx.charge_collection_items(1, "NX intersection identities")?;
        seen.insert(curve.xmt);
        ctx.charge_collection_items(1, "NX intersection data curves")?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<CompositeCurve>()), "NX intersection data curves")?;
        out.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("NX intersection data curves", 0, 1))?;
        out.push(curve);
    }
    Ok(out)
}

/// Return whether the complete type-38 schema header starts at `offset`.
pub(crate) fn intersection_data_schema_header_at(stream: &[u8], offset: usize) -> bool {
    stream.get(offset..offset.saturating_add(TYPE_38_SCHEMA_HEADER.len()))
        == Some(TYPE_38_SCHEMA_HEADER)
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
            header_references: header_references.map(XmtTarget::from_wire),
            sense,
            references: references.map(XmtTarget::from_wire),
            delta_twin: true,
            pos,
        },
        at,
    ))
}

/// Decode validated type-56 rolling-ball blend surfaces.
pub(crate) fn blend_surfaces(ctx: &DecodeContext<'_>, stream: &[u8]) -> Result<Vec<BlendSurface>, CodecError> {
    Graph::parse(ctx, stream)?.blend_surfaces(ctx)
}

impl Graph {
    pub(crate) fn blend_surfaces(&self, ctx: &DecodeContext<'_>) -> Result<Vec<BlendSurface>, CodecError> {
        collect_graph_records(ctx, "NX blend surfaces", self.of_kind(NodeKind::BlendSurface)
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
                (node.bytes.get(at + 32..at + 40)? == [0, 1, 0, 1, 0, 1, 0, 1]).then_some(())?;
                Some(BlendSurface {
                    xmt: node.xmt,
                    state: BlendSurfaceState::from_metres(
                        [refs[0], refs[1]],
                        refs[2],
                        [values[0], values[1]],
                        [values[2], values[3]],
                    )
                    .ok()?,
                    pos: node.pos,
                })
            })
        )
    }
}

/// Decode validated type-60 offset-surface records.
pub(crate) fn offset_surfaces(ctx: &DecodeContext<'_>, stream: &[u8]) -> Result<Vec<OffsetSurface>, CodecError> {
    Graph::parse(ctx, stream)?.offset_surfaces(ctx)
}

impl Graph {
    pub(crate) fn offset_surfaces(&self, ctx: &DecodeContext<'_>) -> Result<Vec<OffsetSurface>, CodecError> {
        collect_graph_records(ctx, "NX offset surfaces", self.of_kind(NodeKind::OffsetSurface)
            .filter_map(|node| {
                let mut at = node.compact_tail_offset()?;
                let discriminator =
                    OffsetSurfaceDiscriminator::try_from(char::from(*node.bytes.get(at)?)).ok()?;
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
                Some(OffsetSurface {
                    xmt: node.xmt,
                    discriminator,
                    true_offset,
                    state: OffsetSurfaceState::new(support, distance).ok()?,
                    pos: node.pos,
                })
            })
        )
    }
}

/// Decode type-137 surface-curve records as aliases of their 3D basis curves.
pub(crate) fn surface_curves(ctx: &DecodeContext<'_>, stream: &[u8]) -> Result<Vec<SurfaceCurve>, CodecError> {
    Graph::parse(ctx, stream)?.surface_curves(ctx)
}

impl Graph {
    pub(crate) fn surface_curves(&self, ctx: &DecodeContext<'_>) -> Result<Vec<SurfaceCurve>, CodecError> {
        collect_graph_records(ctx, "NX surface curves", self.of_kind(NodeKind::SpCurve)
            .filter_map(|node| {
                let mut at = node.compact_tail_offset()?;
                let refs = read_sequence_at(&node.bytes, &mut at, 3)?;
                let tolerance = View::f64_be_at(&node.bytes, at)?;
                Some(SurfaceCurve {
                    xmt: node.xmt,
                    state: SurfaceCurveState::new(refs[0], refs[1], refs[2], tolerance).ok()?,
                    pos: node.pos,
                })
            })
        )
    }
}

/// Decode supported type-133 trimmed-curve records.
///
/// The result retains the basis-curve reference and parameter range. Topological
/// endpoints come from the corresponding edge and vertex records.
pub(crate) fn trimmed_curves(ctx: &DecodeContext<'_>, stream: &[u8]) -> Result<Vec<TrimmedCurve>, CodecError> {
    Graph::parse(ctx, stream)?.trimmed_curves(ctx)
}

impl Graph {
    pub(crate) fn trimmed_curves(&self, ctx: &DecodeContext<'_>) -> Result<Vec<TrimmedCurve>, CodecError> {
        collect_graph_records(ctx, "NX trimmed curves", self.of_kind(NodeKind::TrimmedCurve)
            .filter_map(|node| {
                let mut at = node.compact_tail_offset()?;
                let basis = read_and_advance(&node.bytes, &mut at)?;
                let point_0 = vec3_be_at(&node.bytes, at)?;
                let point_1 = vec3_be_at(&node.bytes, at + 24)?;
                let p0 = View::f64_be_at(&node.bytes, at + 48)?;
                let p1 = View::f64_be_at(&node.bytes, at + 56)?;
                Some(TrimmedCurve {
                    xmt: node.xmt,
                    state: TrimmedCurveState::from_metres(basis, [point_0, point_1], [p0, p1])
                        .ok()?,
                    pos: node.pos,
                })
            })
        )
    }
}

impl Graph {
    /// Parse supported fixed-record nodes from a neutral-binary stream.
    pub(crate) fn parse(ctx: &DecodeContext<'_>, stream: &[u8]) -> Result<Self, CodecError> {
        let (mut baseline, mut baseline_bytes) = Self::parse_fixed_records(ctx, stream, false)?;
        let (full_domain, full_domain_bytes) = Self::parse_fixed_records(ctx, stream, true)?;
        let preserves_baseline = baseline.nodes.iter().all(|(key, node)| {
            full_domain
                .nodes
                .get(key)
                .is_some_and(|candidate| candidate.pos == node.pos && candidate.bytes == node.bytes)
        });
        if !preserves_baseline {
            baseline_bytes.commit()?;
            return Ok(baseline);
        }
        if !baseline.has_complete_body_topology(ctx)?
            && full_domain.has_complete_body_topology(ctx)?
            && full_domain.body_shape_face_count() != 0
        {
            full_domain_bytes.commit()?;
            Ok(full_domain)
        } else {
            baseline.admit_referenced_full_domain_nodes(ctx, &mut baseline_bytes, &full_domain)?;
            baseline_bytes.commit()?;
            Ok(baseline)
        }
    }

    /// Admit full-domain nodes through unique typed XMT references.
    fn admit_referenced_full_domain_nodes(
        &mut self,
        ctx: &DecodeContext<'_>,
        reservation: &mut ScopedReservation<'_>,
        full_domain: &Self,
    ) -> Result<(), CodecError> {
        let mut candidates = BTreeMap::<(ReferenceRole, u32), Option<&Node>>::new();
        for node in full_domain.nodes.values() {
            let Some(role) = ReferenceRole::for_kind(node.kind) else {
                continue;
            };
            if !candidates.contains_key(&(role, node.xmt)) {
                ctx.charge_collection_items(1, "NX topology candidate identities")?;
            }
            candidates
                .entry((role, node.xmt))
                .and_modify(|candidate| *candidate = None)
                .or_insert(Some(node));
        }
        let mut required = BTreeSet::new();
        for target in self.nodes.values().flat_map(Node::reference_targets).filter(|(_, xmt)| *xmt > 1) {
            if !required.contains(&target) {
                ctx.charge_collection_items(1, "NX topology required targets")?;
            }
            required.insert(target);
        }
        let mut changed = false;
        while let Some(target) = required.pop_first() {
            let Some(Some(candidate)) = candidates.get(&target) else {
                continue;
            };
            let key = (candidate.kind, candidate.xmt);
            if self.nodes.contains_key(&key) {
                continue;
            }
            for target in candidate.reference_targets().into_iter().filter(|(_, xmt)| *xmt > 1) {
                if !required.contains(&target) {
                    ctx.charge_collection_items(1, "NX topology required targets")?;
                }
                required.insert(target);
            }
            ctx.charge_collection_items(2, "NX topology admitted node indices")?;
            reservation.grow(u64_from_index(candidate.bytes.len()))?;
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(candidate.bytes.len()).map_err(|_| {
                ctx.refuse_codec_limit("NX topology admitted node bytes", 0, u64_from_index(candidate.bytes.len()))
            })?;
            bytes.extend_from_slice(&candidate.bytes);
            self.by_pos.insert(candidate.pos, key);
            self.nodes.insert(key, Node {
                kind: candidate.kind,
                xmt: candidate.xmt,
                pos: candidate.pos,
                shift: candidate.shift,
                bytes,
            });
            changed = true;
        }
        if changed {
            self.by_kind.clear();
            for &key in self.by_pos.values() {
                if !self.by_kind.contains_key(&key.0) {
                    ctx.charge_collection_items(1, "NX topology kind indices")?;
                }
                let keys = self.by_kind.entry(key.0).or_default();
                ctx.charge_collection_items(1, "NX topology kind entries")?;
                keys.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("NX topology kind entries", 0, 1))?;
                keys.push(key);
            }
        }
        Ok(())
    }

    fn parse_fixed_records<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        stream: &[u8],
        full_node_id_domain: bool,
    ) -> Result<(Self, ScopedReservation<'ctx>), CodecError> {
        let mut candidates = Vec::new();
        let mut ownership_candidates = Vec::new();
        let mut candidate_reservation = ctx.reserve_scoped(0, "NX topology candidates")?;
        let mut ownership_reservation = ctx.reserve_scoped(0, "NX topology ownership candidates")?;
        for pos in 0..stream.len().saturating_sub(3) {
            ctx.charge_work(1, "scan NX topology candidates")?;
            if stream[pos] != 0 {
                continue;
            }
            let Ok(kind) = NodeKind::try_from(stream[pos + 1]) else {
                continue;
            };
            let len = fixed_len(kind);
            for candidate in Self::fixed_record_candidates(stream, pos, kind, len, full_node_id_domain)
                .into_iter()
                .flatten()
            {
                if matches!(kind, NodeKind::Body | NodeKind::Region) {
                    push_scoped(ctx, &mut ownership_reservation, &mut ownership_candidates, candidate, "NX topology ownership candidates")?;
                } else {
                    push_scoped(ctx, &mut candidate_reservation, &mut candidates, candidate, "NX topology candidates")?;
                }
            }
        }

        // Resolve physical overlap before identity uniqueness. A candidate
        // that is wholly contained in a selected record is payload data, not
        // a second serialized node. Counting it first can invalidate the real
        // node and make otherwise stable identities depend on unrelated bytes.
        let (non_overlapping, _non_overlapping_reservation) =
            Self::select_non_overlapping_candidates(ctx, stream, candidates)?;
        let (selected, _selected_reservation) = Self::select_unique_candidates(ctx, non_overlapping)?;
        // BODY and REGION carry ownership identity only. Their opaque fixed
        // payloads can contain complete-looking typed tags, so they are
        // admitted after typed topology/carrier selection and never veto a
        // typed candidate. An ownership node that shares bytes with a typed
        // node is ambiguous and is omitted; shells retain the identity even
        // when the optional BODY or REGION record is absent.
        let (non_overlapping_ownership, _ownership_nonoverlap_reservation) =
            Self::select_non_overlapping_candidates(ctx, stream, ownership_candidates)?;
        let (ownership, _ownership_unique_reservation) =
            Self::select_unique_candidates(ctx, non_overlapping_ownership)?;
        let mut admitted_ownership = Vec::new();
        let mut admitted_reservation = ctx.reserve_scoped(0, "NX admitted ownership candidates")?;
        for candidate in ownership {
            if selected.iter().all(|selected| !selected.overlaps(candidate)) {
                push_scoped(ctx, &mut admitted_reservation, &mut admitted_ownership, candidate, "NX admitted ownership candidates")?;
            }
        }
        let mut graph = Self::default();
        let mut node_reservation = ctx.reserve_scoped(0, "NX topology node bytes")?;
        for candidate in selected.into_iter().chain(admitted_ownership) {
            let Some(node) = candidate.materialize(ctx, &mut node_reservation, stream)? else {
                continue;
            };
            let key = (node.kind, node.xmt);
            ctx.charge_collection_items(2, "NX topology node indices")?;
            graph.by_pos.insert(node.pos, key);
            graph.nodes.insert(key, node);
        }
        for &key in graph.by_pos.values() {
            if !graph.by_kind.contains_key(&key.0) {
                ctx.charge_collection_items(1, "NX topology kind indices")?;
            }
            let keys = graph.by_kind.entry(key.0).or_default();
            ctx.charge_collection_items(1, "NX topology kind entries")?;
            keys.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("NX topology kind entries", 0, 1))?;
            keys.push(key);
        }
        Ok((graph, node_reservation))
    }

    fn fixed_record_candidates(
        stream: &[u8],
        pos: usize,
        kind: NodeKind,
        len: usize,
        full_node_id_domain: bool,
    ) -> [Option<NodeCandidate>; 2] {
        let mut candidates = [None; 2];
        let mut count = 0;
        for frame in framed_record_candidates(stream, pos, kind, len)
            .into_iter()
            .flatten()
        {
            let Some(()) = candidate_has_valid_family_framing(
                stream,
                pos,
                kind,
                frame.shift,
                frame.end,
                full_node_id_domain,
            ) else {
                continue;
            };
            candidates[count] = Some(NodeCandidate {
                kind,
                xmt: frame.xmt,
                pos,
                shift: frame.shift,
                end: frame.end,
            });
            count += 1;
        }
        if count < 2 {
            return candidates;
        }

        let mut boundary_candidates = candidates
            .iter()
            .flatten()
            .copied()
            .filter(|candidate| fixed_record_boundary(stream, candidate.end()));
        let Some(candidate) = boundary_candidates.next() else {
            return [None; 2];
        };
        if boundary_candidates.next().is_none() {
            [Some(candidate), None]
        } else {
            [None; 2]
        }
    }

    /// Keep one complete physical record for each serialized identity.
    ///
    /// A second record with the same `(kind, xmt)` is not a recoverable choice:
    /// the fixed-record grammar provides no discriminator that can make one
    /// authoritative. Invalidate the identity instead of ranking candidates
    /// by topology shape, reference counts, or scan position.
    fn select_unique_candidates<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        candidates: Vec<NodeCandidate>,
    ) -> Result<(Vec<NodeCandidate>, ScopedReservation<'ctx>), CodecError> {
        let mut by_key = BTreeMap::<(NodeKind, u32), Option<NodeCandidate>>::new();
        for node in candidates {
            match by_key.entry((node.kind, node.xmt)) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    ctx.charge_collection_items(1, "NX topology unique candidate keys")?;
                    entry.insert(Some(node));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    // A duplicate identity is invalid. Retain only the fact
                    // that it is ambiguous; do not retain every overlapping
                    // physical interpretation of the same identity.
                    entry.insert(None);
                }
            }
        }
        let mut selected = Vec::new();
        let mut reservation = ctx.reserve_scoped(0, "NX topology unique candidates")?;
        for candidate in by_key.into_values().flatten() {
            push_scoped(ctx, &mut reservation, &mut selected, candidate, "NX topology unique candidates")?;
        }
        Ok((selected, reservation))
    }

    /// Discard overlapping candidates when no serialized ownership boundary
    /// identifies which record owns the bytes.
    fn select_non_overlapping_candidates<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        stream: &[u8],
        mut nodes: Vec<NodeCandidate>,
    ) -> Result<(Vec<NodeCandidate>, ScopedReservation<'ctx>), CodecError> {
        let count = u64_from_index(nodes.len());
        let sort_work = count.checked_mul(u64::from(usize::BITS - nodes.len().leading_zeros()))
            .ok_or_else(|| ctx.refuse_codec_limit("sort NX topology candidates", 0, count))?;
        ctx.charge_work(sort_work, "sort NX topology candidates")?;
        nodes.sort_by(|left, right| {
            left.pos
                .cmp(&right.pos)
                .then_with(|| left.end().cmp(&right.end()))
        });
        let mut selected = Vec::new();
        let mut reservation = ctx.reserve_scoped(0, "NX topology nonoverlapping candidates")?;
        let mut start = 0;
        while let Some(first) = nodes.get(start).copied() {
            ctx.charge_work(1, "select NX topology candidates")?;
            if fixed_record_boundary(stream, first.end()) {
                let end = first.end();
                push_scoped(ctx, &mut reservation, &mut selected, first, "NX topology nonoverlapping candidates")?;
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
                push_scoped(ctx, &mut reservation, &mut selected, *node, "NX topology nonoverlapping candidates")?;
            } else {
                let mut boundary_candidates = cluster
                    .iter()
                    .copied()
                    .filter(|candidate| fixed_record_boundary(stream, candidate.end()));
                let Some(node) = boundary_candidates.next() else {
                    start = end;
                    continue;
                };
                if boundary_candidates.next().is_none() {
                    push_scoped(ctx, &mut reservation, &mut selected, node, "NX topology nonoverlapping candidates")?;
                }
            }
            start = end;
        }
        Ok((selected, reservation))
    }

    /// Look up a node by record type and XMT identifier.
    pub(crate) fn get(&self, kind: NodeKind, xmt: u32) -> Option<&Node> {
        self.nodes.get(&(kind, xmt))
    }

    pub(crate) fn get_target(&self, kind: NodeKind, target: Option<XmtTarget>) -> Option<&Node> {
        self.get(kind, u32::from(target?))
    }

    /// Look up the node whose type tag starts at `pos`.
    pub(crate) fn at_pos(&self, pos: usize) -> Option<&Node> {
        let &(kind, xmt) = self.by_pos.get(&pos)?;
        self.get(kind, xmt)
    }

    /// Iterate nodes of one record type in physical record order.
    pub(crate) fn of_kind(&self, kind: NodeKind) -> impl Iterator<Item = &Node> {
        self.by_kind
            .get(&kind)
            .into_iter()
            .flat_map(|keys| keys.iter())
            .filter_map(|key| self.nodes.get(key))
    }

    /// Resolve one current XMT identity from a unique kernel node identity.
    pub(crate) fn unique_xmt_by_node_id(&self, kind: NodeKind, node_id: u32) -> Option<u32> {
        let mut matches = self
            .of_kind(kind)
            .filter(|node| node.node_id() == Some(node_id))
            .map(|node| node.xmt);
        let xmt = matches.next()?;
        matches.next().is_none().then_some(xmt)
    }

    /// Curve identities occupying typed curve-reference slots in the fixed
    /// topology and procedural graph.
    pub(crate) fn referenced_curve_xmts(&self, ctx: &DecodeContext<'_>) -> Result<BTreeSet<u32>, CodecError> {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(self.nodes.len()), "scan NX topology curve references")?;
        let mut references = BTreeSet::new();
        for reference in self.of_kind(NodeKind::Edge)
            .filter_map(Node::edge_fields)
            .filter_map(|fields| fields.curve.map(u32::from))
            .filter(|reference| *reference > 1)
        {
            insert_reference(ctx, &mut references, reference)?;
        }
        for reference in self.of_kind(NodeKind::Fin)
            .filter_map(Node::fin_fields)
            .filter_map(|fields| fields.curve_xmt.map(u32::from))
            .filter(|reference| *reference > 1)
        {
            insert_reference(ctx, &mut references, reference)?;
        }
        for node in self.of_kind(NodeKind::BlendSurface) {
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
                insert_reference(ctx, &mut references, spine)?;
            }
        }
        for node in self.of_kind(NodeKind::TrimmedCurve) {
            if let Some(reference) = node
                .compact_tail_references(1)
                .and_then(|items| items.first().copied())
                .filter(|reference| *reference > 1)
            {
                insert_reference(ctx, &mut references, reference)?;
            }
        }
        for node in self.of_kind(NodeKind::SpCurve) {
            if let Some(reference) = node
                .compact_tail_references(3)
                .and_then(|items| items.get(2).copied())
                .filter(|reference| *reference > 1)
            {
                insert_reference(ctx, &mut references, reference)?;
            }
        }
        Ok(references)
    }

    /// Resolve the exact witnesses of the unique edge carrying a curve.
    pub(crate) fn unique_curve_edge_witness(&self, curve_xmt: u32) -> Option<CurveEdgeWitness> {
        let mut edges = self
            .of_kind(NodeKind::Edge)
            .filter_map(Node::edge_fields)
            .filter(|edge| edge.curve.map(u32::from) == Some(curve_xmt));
        let edge = edges.next()?;
        edges.next().is_none().then_some(())?;
        let first_fin = self.get_target(NodeKind::Fin, edge.fin)?.fin_fields()?;
        let second_fin = self
            .get_target(NodeKind::Fin, first_fin.forward)?
            .fin_fields()?;
        let position = |vertex_xmt| {
            let point_xmt = self
                .get_target(NodeKind::Vertex, vertex_xmt)?
                .vertex_fields()?
                .point;
            self.get_target(NodeKind::Point, point_xmt)?
                .point_position()
        };
        Some(CurveEdgeWitness {
            endpoints: [position(first_fin.vertex)?, position(second_fin.vertex)?],
            tolerance: edge.tolerance,
        })
    }

    /// Carrier identities required by the surviving fixed topology image.
    pub(crate) fn referenced_carrier_xmts(&self, ctx: &DecodeContext<'_>) -> Result<BTreeSet<u32>, CodecError> {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(self.nodes.len()), "scan NX topology carrier references")?;
        let mut references = self.referenced_curve_xmts(ctx)?;
        for reference in self.of_kind(NodeKind::Face)
            .filter_map(Node::face_fields)
            .filter_map(|fields| fields.surface.map(u32::from))
            .filter(|reference| *reference > 1)
        {
            insert_reference(ctx, &mut references, reference)?;
        }
        for reference in self.of_kind(NodeKind::Vertex)
            .filter_map(Node::vertex_fields)
            .filter_map(|fields| fields.point.map(u32::from))
            .filter(|reference| *reference > 1)
        {
            insert_reference(ctx, &mut references, reference)?;
        }
        Ok(references)
    }

    /// Return SHELL nodes whose ownership fields define a body shape.
    pub(crate) fn body_shape_shells(&self) -> impl Iterator<Item = &Node> + '_ {
        self.of_kind(NodeKind::Shell)
            .filter(|shell| self.is_body_shape_shell(shell))
    }

    /// Return whether every body-shape face has a non-empty valid loop chain
    /// and every non-null radial FIN partner belongs to the same reachable
    /// body topology.
    pub(crate) fn has_complete_body_topology(&self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        let mut shells = self.body_shape_shells().peekable();
        if shells.peek().is_none() {
            return Ok(false);
        }
        let mut reachable_fins = BTreeSet::new();
        for shell in shells {
            let Some(face_xmts) = self.shell_face_xmts(shell) else {
                return Ok(false);
            };
            for face_xmt in face_xmts {
                let rings = match self.face_loop_rings(ctx, face_xmt) {
                    Ok(rings) => rings,
                    Err(FaceLoopError::Invalid(_)) => return Ok(false),
                    Err(FaceLoopError::Codec(error)) => return Err(error),
                };
                if rings.is_empty() {
                    return Ok(false);
                }
                for (_, ring) in rings {
                    for xmt in ring {
                        if !reachable_fins.contains(&xmt) {
                            ctx.charge_collection_items(1, "NX reachable FIN identities")?;
                        }
                        reachable_fins.insert(xmt);
                    }
                }
            }
        }
        Ok(reachable_fins.iter().all(|xmt| {
            self.get(NodeKind::Fin, *xmt)
                .and_then(Node::fin_fields)
                .is_some_and(|fields| {
                    fields
                        .other
                        .is_none_or(|other| reachable_fins.contains(&u32::from(other)))
                })
        }))
    }

    /// Count faces owned by validated body-shape shells.
    pub(crate) fn body_shape_face_count(&self) -> usize {
        self.body_shape_shells()
            .filter_map(|shell| self.shell_face_xmts(shell).map(|faces| faces.len()))
            .sum()
    }

    /// Return the validated loop-to-FIN rings owned by a face.
    ///
    /// The face's loop chain must terminate at the null reference. Each loop
    /// points back to the face. Each FIN cycle closes at its first FIN, stays in
    /// the loop, and has reciprocal forward/backward links. Every FIN resolves
    /// its edge and vertex.
    pub(crate) fn face_loop_rings(
        &self,
        ctx: &DecodeContext<'_>,
        face_xmt: u32,
    ) -> Result<Vec<(u32, Vec<u32>)>, FaceLoopError> {
        let face = self
            .get(NodeKind::Face, face_xmt)
            .and_then(Node::face_fields)
            .ok_or(FaceLoopFailure::InvalidFace { face_xmt })?;
        let mut loop_xmt = face.loop_xmt;
        let mut seen_loops = BTreeSet::new();
        let mut seen_reservation = ctx.reserve_scoped(0, "NX face loop identities")?;
        let mut rings = Vec::new();
        while let Some(target) = loop_xmt {
            let current = u32::from(target);
            if seen_loops.contains(&current) {
                return Err(FaceLoopFailure::InvalidLoopChain { loop_xmt: current }.into());
            }
            ctx.charge_work(1, "walk NX face loops")?;
            ctx.charge_collection_items(1, "NX face loop identities")?;
            seen_reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<u32>()))?;
            seen_loops.insert(current);
            let fields = self
                .get(NodeKind::Loop, current)
                .and_then(Node::loop_fields)
                .ok_or(FaceLoopFailure::InvalidLoopChain { loop_xmt: current })?;
            if fields.face.map(u32::from) != Some(face_xmt) {
                return Err(FaceLoopFailure::InvalidLoopChain { loop_xmt: current }.into());
            }
            let first_fin = fields
                .fin
                .ok_or(FaceLoopFailure::InvalidLoopChain { loop_xmt: current })?;
            let ring = self.fin_ring(ctx, current, first_fin)?;
            ctx.charge_collection_items(1, "NX face loop rings")?;
            ctx.charge_retained(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(u32, Vec<u32>)>()), "NX face loop rings")?;
            rings.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("NX face loop rings", 0, 1))?;
            rings.push((current, ring));
            loop_xmt = fields.next_loop;
        }
        Ok(rings)
    }

    fn fin_ring(&self, ctx: &DecodeContext<'_>, loop_xmt: u32, first: XmtTarget) -> Result<Vec<u32>, FaceLoopError> {
        let first = u32::from(first);
        let mut current = first;
        let mut previous = None;
        let mut seen = BTreeSet::new();
        let mut seen_reservation = ctx.reserve_scoped(0, "NX FIN ring identities")?;
        let mut ring = Vec::new();
        loop {
            if seen.contains(&current) {
                return if current == first {
                    Ok(ring)
                } else {
                    Err(FaceLoopFailure::InvalidFinRing {
                        loop_xmt,
                        fin_xmt: current,
                    }.into())
                };
            }
            ctx.charge_work(1, "walk NX FIN ring")?;
            ctx.charge_collection_items(1, "NX FIN ring identities")?;
            seen_reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<u32>()))?;
            seen.insert(current);
            ctx.charge_collection_items(1, "NX FIN ring entries")?;
            ctx.charge_retained(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<u32>()), "NX FIN ring entries")?;
            ring.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("NX FIN ring entries", 0, 1))?;
            ring.push(current);
            let invalid_fin = FaceLoopFailure::InvalidFinRing {
                loop_xmt,
                fin_xmt: current,
            };
            let fields = self
                .get(NodeKind::Fin, current)
                .and_then(Node::fin_fields)
                .ok_or(invalid_fin)?;
            let vertex_resolves = self.get_target(NodeKind::Vertex, fields.vertex).is_some()
                || (fields.vertex.is_none()
                    && fields.forward.map(u32::from) == Some(current)
                    && fields.backward.map(u32::from) == Some(current));
            if self.get_target(NodeKind::Edge, fields.edge).is_none() {
                return Err(FaceLoopFailure::UnresolvedFinEdge {
                    loop_xmt,
                    fin_xmt: current,
                    edge_xmt: fields.edge.map(u32::from),
                }.into());
            }
            if fields.loop_xmt.map(u32::from) != Some(loop_xmt) || !vertex_resolves {
                return Err(invalid_fin.into());
            }
            if let Some(other_xmt) = fields.other {
                let other = self
                    .get(NodeKind::Fin, u32::from(other_xmt))
                    .and_then(Node::fin_fields)
                    .ok_or(invalid_fin)?;
                if other.other.map(u32::from) != Some(current) || other.edge != fields.edge {
                    return Err(invalid_fin.into());
                }
            }
            if let Some(previous) = previous {
                if fields.backward.map(u32::from) != Some(previous) {
                    return Err(invalid_fin.into());
                }
            }
            let next = self
                .get_target(NodeKind::Fin, fields.forward)
                .and_then(Node::fin_fields)
                .ok_or(invalid_fin)?;
            if next.backward.map(u32::from) != Some(current) {
                return Err(invalid_fin.into());
            }
            previous = Some(current);
            current = u32::from(fields.forward.ok_or(invalid_fin)?);
        }
    }

    fn is_body_shape_shell(&self, shell: &Node) -> bool {
        let Some(fields) = shell.shell_fields() else {
            return false;
        };
        if fields.attributes.is_some()
            || fields.next_shell.is_some()
            || fields.sentinel_0.is_some()
            || fields.sentinel_1.is_some()
            || fields.body.is_none_or(|target| u32::from(target) == 0)
            || fields.region.is_none_or(|target| u32::from(target) == 0)
        {
            return false;
        }

        self.shell_face_xmts(shell).is_some()
    }

    pub(crate) fn shell_face_xmts(&self, shell: &Node) -> Option<Vec<u32>> {
        let fields = shell.shell_fields()?;
        if fields.last_face.is_some() {
            (fields.last_face == fields.first_face).then_some(())?;
            self.get_target(NodeKind::Face, fields.first_face)
                .and_then(Node::face_fields)
                .filter(|face| face.shell.map(u32::from) == Some(shell.xmt))?;
            let faces: Vec<_> = self
                .of_kind(NodeKind::Face)
                .filter(|face| {
                    face.face_fields()
                        .is_some_and(|fields| fields.shell.map(u32::from) == Some(shell.xmt))
                })
                .map(|face| face.xmt)
                .collect();
            return (!faces.is_empty()).then_some(faces);
        }

        let mut face_xmt = fields.first_face;
        let mut visited = BTreeSet::new();
        while let Some(target) = face_xmt {
            let current = u32::from(target);
            if !visited.insert(current) {
                return None;
            }
            let face = self
                .get(NodeKind::Face, current)
                .and_then(Node::face_fields)?;
            if face.shell.map(u32::from) != Some(shell.xmt) {
                return None;
            }
            face_xmt = face.next_face;
        }
        (!visited.is_empty()).then(|| visited.into_iter().collect())
    }
}

impl ReferenceRole {
    fn for_kind(kind: NodeKind) -> Option<Self> {
        match kind {
            NodeKind::Body => Some(Self::Body),
            NodeKind::Region => Some(Self::Region),
            NodeKind::Point => Some(Self::Point),
            NodeKind::Line
            | NodeKind::Circle
            | NodeKind::Ellipse
            | NodeKind::Intersection
            | NodeKind::TrimmedCurve
            | NodeKind::BCurve
            | NodeKind::SpCurve => Some(Self::Curve),
            NodeKind::Plane
            | NodeKind::Cylinder
            | NodeKind::Cone
            | NodeKind::Sphere
            | NodeKind::Torus
            | NodeKind::BlendSurface
            | NodeKind::OffsetSurface
            | NodeKind::BSurface => Some(Self::Surface),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct NodeCandidate {
    kind: NodeKind,
    xmt: u32,
    pos: usize,
    shift: usize,
    end: usize,
}

impl NodeCandidate {
    fn end(self) -> usize {
        self.end
    }

    fn overlaps(self, other: Self) -> bool {
        self.pos < other.end() && other.pos < self.end()
    }

    fn materialize(
        self,
        ctx: &DecodeContext<'_>,
        reservation: &mut ScopedReservation<'_>,
        stream: &[u8],
    ) -> Result<Option<Node>, CodecError> {
        let Some(bytes) = stream.get(self.pos..self.end) else {
            return Ok(None);
        };
        reservation.grow(u64_from_index(bytes.len()))?;
        let mut owned = Vec::new();
        owned.try_reserve_exact(bytes.len()).map_err(|_| {
            ctx.refuse_codec_limit("NX topology node bytes", 0, u64_from_index(bytes.len()))
        })?;
        owned.extend_from_slice(bytes);
        Ok(Some(Node {
            kind: self.kind,
            xmt: self.xmt,
            pos: self.pos,
            shift: self.shift,
            bytes: owned,
        }))
    }
}

fn candidate_has_valid_family_framing(
    stream: &[u8],
    pos: usize,
    kind: NodeKind,
    shift: usize,
    end: usize,
    full_node_id_domain: bool,
) -> Option<()> {
    let bytes = stream.get(pos..end)?;
    // A complete topology graph is the positive witness for the full u32
    // identity domain. The baseline excludes high-entropy payload matches so
    // they cannot displace framed records before that graph proof exists.
    if matches!(
        kind,
        NodeKind::Shell
            | NodeKind::Face
            | NodeKind::Loop
            | NodeKind::Edge
            | NodeKind::Vertex
            | NodeKind::Region
            | NodeKind::Point
            | NodeKind::Line
            | NodeKind::Circle
            | NodeKind::Ellipse
            | NodeKind::Intersection
            | NodeKind::Plane
            | NodeKind::Cylinder
            | NodeKind::Cone
            | NodeKind::Sphere
            | NodeKind::Torus
            | NodeKind::BlendSurface
            | NodeKind::OffsetSurface
            | NodeKind::BSurface
            | NodeKind::TrimmedCurve
            | NodeKind::BCurve
            | NodeKind::SpCurve
    ) && !full_node_id_domain
        && View::u32_be_at(bytes, 4 + shift).is_none_or(|node_id| node_id > 1_000_000)
    {
        return None;
    }
    match kind {
        NodeKind::Shell => {
            let mut at = 8 + shift;
            skip_sequence_at(bytes, &mut at, 8)?;
        }
        NodeKind::Face => {
            let mut at = 8 + shift;
            read_and_advance(bytes, &mut at)?;
            View::f64_be_at(bytes, at)?.is_finite().then_some(())?;
            at += 8;
            skip_sequence_at(bytes, &mut at, 5)?;
            matches!(bytes.get(at), Some(b'+' | b'-')).then_some(())?;
        }
        NodeKind::Loop => {
            let mut at = 8 + shift;
            skip_sequence_at(bytes, &mut at, 4)?;
        }
        NodeKind::Edge => {
            let mut at = 8 + shift;
            read_and_advance(bytes, &mut at)?;
            View::f64_be_at(bytes, at)?.is_finite().then_some(())?;
            at += 8;
            skip_sequence_at(bytes, &mut at, 7)?;
        }
        NodeKind::Fin => {
            let mut at = 4 + shift;
            skip_sequence_at(bytes, &mut at, 9)?;
            matches!(bytes.get(at), Some(b'+' | b'-')).then_some(())?;
        }
        NodeKind::Vertex => {
            let mut at = 8 + shift;
            skip_sequence_at(bytes, &mut at, 5)?;
            View::f64_be_at(bytes, at)?.is_finite().then_some(())?;
        }
        NodeKind::Point => {
            let mut at = 8 + shift;
            skip_sequence_at(bytes, &mut at, 4)?;
            let point = vec3_be_at(bytes, at)?;
            point
                .iter()
                .all(|value| (*value * 1000.0).is_finite())
                .then_some(())?;
        }
        _ => {}
    }
    Some(())
}

#[cfg(test)]
mod tests;
