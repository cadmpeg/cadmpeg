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
use crate::framing::xmt_reference::{NonNullXmt, XmtTarget};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::Sense;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;
use std::sync::OnceLock;

use crate::framing::{
    fixed_record_boundary, fixed_record_candidates as framed_record_candidates, read_and_advance,
    read_sequence_at, read_xmt, skip_sequence_at, FixedRecordFrame,
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
    xmt: NonNullXmt,
    /// Checked source span in the inflated stream.
    pos: usize,
    end: usize,
    shift: usize,
    bytes: Vec<u8>,
}

/// Decoded fields needed from a sequentially framed FACE record.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FaceFields {
    /// Attribute-list reference.
    pub(crate) attributes: Option<XmtTarget>,
    /// Face tolerance in Parasolid metres.
    tolerance: FiniteReal,
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
    tolerance: FiniteReal,
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
    tolerance: FiniteReal,
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
    tolerance: FiniteReal,
}

impl FaceFields {
    pub(crate) fn tolerance(self) -> f64 {
        self.tolerance.get()
    }
}

impl EdgeFields {
    pub(crate) fn tolerance(self) -> f64 {
        self.tolerance.get()
    }
}

impl CurveEdgeWitness {
    pub(crate) fn tolerance(self) -> f64 {
        self.tolerance.get()
    }
}

impl VertexFields {
    pub(crate) fn tolerance(self) -> f64 {
        self.tolerance.get()
    }
}

impl Node {
    pub(crate) fn xmt(&self) -> u32 {
        u32::from(self.xmt)
    }
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }
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
            | NodeKind::Vertex => self
                .pos
                .checked_add(8)?
                .checked_add(self.shift)
                .filter(|at| *at < self.end),
            NodeKind::Fin => self
                .pos
                .checked_add(4)?
                .checked_add(self.shift)
                .filter(|at| *at < self.end),
            _ => None,
        }
    }

    /// First byte after this complete record in its source stream.
    pub(crate) fn end(&self) -> usize {
        self.end
    }

    /// Decode the common-header sense and the following payload offset.
    pub(crate) fn common_header(&self) -> Option<(Sense, usize)> {
        let mut at = 8 + self.shift;
        skip_sequence_at(&self.bytes, &mut at, 5)?;
        let sense = match self.bytes.get(at) {
            Some(b'+') => Sense::Forward,
            Some(b'-') => Sense::Reversed,
            _ => return None,
        };
        Some((sense, at + 1))
    }

    /// Decode adjacent references at the start of a compact geometry payload.
    pub(crate) fn compact_tail_references<const N: usize>(&self) -> Option<[u32; N]> {
        let mut at = self.common_header()?.1;
        read_sequence_at::<N>(&self.bytes, &mut at)
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
        let tolerance = FiniteReal::new(View::f64_be_at(&self.bytes, at)?)?;
        at += 8;
        let refs = read_sequence_at::<5>(&self.bytes, &mut at)?;
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
        let tolerance = FiniteReal::new(View::f64_be_at(&self.bytes, at)?)?;
        at += 8;
        let refs = read_sequence_at::<7>(&self.bytes, &mut at)?;
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
        let refs = read_sequence_at::<8>(&self.bytes, &mut at)?;
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
        let refs = read_sequence_at::<4>(&self.bytes, &mut at)?;
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
        let refs = read_sequence_at::<9>(&self.bytes, &mut at)?;
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
        let refs = read_sequence_at::<5>(&self.bytes, &mut at)?;
        let tolerance = FiniteReal::new(View::f64_be_at(&self.bytes, at)?)?;
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
        let payload_shift = self.common_header()?.1.checked_sub(19)?;
        crate::geometry::decode_surface_record(&self.bytes, self.kind, payload_shift)
    }

    /// Decode this graph-owned fixed analytic curve carrier.
    pub(crate) fn curve_geometry(&self) -> Option<cadmpeg_ir::geometry::CurveGeometry> {
        matches!(
            self.kind,
            NodeKind::Line | NodeKind::Circle | NodeKind::Ellipse
        )
        .then_some(())?;
        let payload_shift = self.common_header()?.1.checked_sub(19)?;
        crate::geometry::decode_curve_record(&self.bytes, self.kind, payload_shift)
    }

    /// Spine curve reference of a rolling-ball `BLEND_SURFACE`.
    fn blend_spine(&self) -> Option<u32> {
        let (_, mut at) = self.common_header()?;
        (self.bytes.get(at) == Some(&b'R')).then_some(())?;
        at += 1;
        Some(read_sequence_at::<3>(&self.bytes, &mut at)?[2])
    }

    /// The typed non-null references this record makes to other records.
    fn reference_targets(&self) -> [Option<(ReferenceRole, u32)>; 3] {
        let target = |role, reference: Option<XmtTarget>| {
            reference
                .map(u32::from)
                .filter(|xmt| *xmt > 1)
                .map(|xmt| (role, xmt))
        };
        let wire = |role, reference: u32| target(role, XmtTarget::from_wire(reference));
        match self.kind {
            NodeKind::Shell => self.shell_fields().map_or([None; 3], |fields| {
                [
                    target(ReferenceRole::Body, fields.body),
                    target(ReferenceRole::Region, fields.region),
                    None,
                ]
            }),
            NodeKind::Face => self.face_fields().map_or([None; 3], |fields| {
                [target(ReferenceRole::Surface, fields.surface), None, None]
            }),
            NodeKind::Edge => self.edge_fields().map_or([None; 3], |fields| {
                [target(ReferenceRole::Curve, fields.curve), None, None]
            }),
            NodeKind::Fin => self.fin_fields().map_or([None; 3], |fields| {
                [target(ReferenceRole::Curve, fields.curve_xmt), None, None]
            }),
            NodeKind::Vertex => self.vertex_fields().map_or([None; 3], |fields| {
                [target(ReferenceRole::Point, fields.point), None, None]
            }),
            NodeKind::BlendSurface => (|| {
                let (_, mut at) = self.common_header()?;
                (self.bytes.get(at) == Some(&b'R')).then_some(())?;
                at += 1;
                let references = read_sequence_at::<3>(&self.bytes, &mut at)?;
                Some([
                    wire(ReferenceRole::Surface, references[0]),
                    wire(ReferenceRole::Surface, references[1]),
                    wire(ReferenceRole::Curve, references[2]),
                ])
            })()
            .unwrap_or([None; 3]),
            NodeKind::OffsetSurface => (|| {
                let (_, mut at) = self.common_header()?;
                (matches!(self.bytes.get(at), Some(b'V' | b'I' | b'U'))
                    && matches!(self.bytes.get(at + 1), Some(0 | 1)))
                .then_some(())?;
                at += 2;
                let reference = read_and_advance(&self.bytes, &mut at)?;
                Some([wire(ReferenceRole::Surface, reference), None, None])
            })()
            .unwrap_or([None; 3]),
            NodeKind::TrimmedCurve => (|| {
                let (_, mut at) = self.common_header()?;
                let reference = read_and_advance(&self.bytes, &mut at)?;
                Some([wire(ReferenceRole::Curve, reference), None, None])
            })()
            .unwrap_or([None; 3]),
            NodeKind::SpCurve => (|| {
                let (_, mut at) = self.common_header()?;
                let references = read_sequence_at::<3>(&self.bytes, &mut at)?;
                Some([
                    wire(ReferenceRole::Surface, references[0]),
                    wire(ReferenceRole::Curve, references[1]),
                    wire(ReferenceRole::Curve, references[2]),
                ])
            })()
            .unwrap_or([None; 3]),
            _ => [None; 3],
        }
    }
}

/// Supported records grouped by type, each group in physical stream order,
/// with identity and offset indexes into the groups.
#[derive(Debug, Default)]
pub(crate) struct Graph {
    /// Nodes of each kind in physical stream order, indexed by `NodeKind::ordinal`.
    kinds: [Vec<Node>; NodeKind::COUNT],
    /// Group position of each `(kind, XMT identifier)` node.
    keys: BTreeMap<(NodeKind, u32), usize>,
    /// Kind and group position of the node whose type tag starts at each offset.
    by_pos: BTreeMap<usize, (NodeKind, usize)>,
    /// XMT identifier of each kernel node identity within its kind, `None` when
    /// the identity repeats. Built on first use.
    node_ids: OnceLock<BTreeMap<(NodeKind, u32), Option<u32>>>,
    /// Group position of the unique EDGE carrying each curve, `None` when
    /// several edges carry it. Built on first use.
    curve_edges: OnceLock<BTreeMap<u32, Option<usize>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ReferenceRole {
    Body,
    Point,
    Curve,
    Region,
    Surface,
}

impl cadmpeg_core::decode::cost::DecodeCost for ReferenceRole {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
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
pub(crate) fn composite_curves(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<Vec<CompositeCurve>, CodecError> {
    Graph::parse(ctx, stream)?.composite_curves(ctx)
}

impl Graph {
    pub(crate) fn composite_curves(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<CompositeCurve>, CodecError> {
        ctx.collect_retained_vec(
            ctx.admit_iter(
                self.of_kind(NodeKind::Intersection),
                "NX topology carrier records",
            )?
            .filter_map(|node| {
                let mut at = 8 + node.shift;
                let header = read_sequence_at::<5>(&node.bytes, &mut at)?;
                let sense = match node.bytes.get(at) {
                    Some(b'+') => true,
                    Some(b'-') => false,
                    _ => return None,
                };
                at += 1;
                let references: [u32; 6] = read_sequence_at::<6>(&node.bytes, &mut at)?;
                let chart_with_optional_terms =
                    references[2] > 1 && references[3..=4].iter().all(|reference| *reference >= 1);
                let null_witness = references[2..=4].iter().all(|reference| *reference == 1);
                (references.iter().all(|reference| *reference != 0)
                    && (chart_with_optional_terms || null_witness)
                    && (references[0] > 1 || references[1] > 1))
                    .then_some(CompositeCurve {
                        xmt: node.xmt(),
                        header_references: header.map(XmtTarget::from_wire),
                        sense,
                        references: references.map(XmtTarget::from_wire),
                        delta_twin: false,
                        pos: node.pos(),
                    })
            }),
            "NX composite curves",
        )
    }
}

/// Decode single-byte `0x5a` intersection-data construction records.
pub(crate) fn intersection_data_curves(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<Vec<CompositeCurve>, CodecError> {
    let mut out = Vec::new();
    let mut seen_storage = ctx.reserve_scoped(0, "NX intersection identities")?;
    let mut seen = BTreeSet::new();
    let mut schema_anchor_seen = false;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(stream.len()),
        "scan NX intersection data",
    )?;
    for (pos, byte) in stream.iter().enumerate() {
        schema_anchor_seen |= intersection_data_schema_header_at(stream, pos);
        if *byte != 0x5a || !schema_anchor_seen {
            continue;
        }
        let Some((curve, _)) = intersection_data_curve_at(stream, pos, schema_anchor_seen) else {
            continue;
        };
        if !seen_storage.with_storage(|| {
            ctx.insert_btree_set(&mut seen, curve.xmt, "NX intersection identities")
        })? {
            continue;
        }
        ctx.reserve_vec(&mut out, 1, "NX intersection data curves")?;
        out.push(curve);
    }
    Ok(out)
}

/// Return whether the complete type-38 schema header starts at `offset`.
pub(crate) fn intersection_data_schema_header_at(stream: &[u8], offset: usize) -> bool {
    offset
        .checked_add(TYPE_38_SCHEMA_HEADER.len())
        .and_then(|end| stream.get(offset..end))
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
pub(crate) fn blend_surfaces(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<Vec<BlendSurface>, CodecError> {
    Graph::parse(ctx, stream)?.blend_surfaces(ctx)
}

impl Graph {
    pub(crate) fn blend_surfaces(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<BlendSurface>, CodecError> {
        ctx.collect_retained_vec(
            ctx.admit_iter(
                self.of_kind(NodeKind::BlendSurface),
                "NX topology carrier records",
            )?
            .filter_map(|node| {
                let mut at = node.common_header()?.1;
                (*node.bytes.get(at)? == b'R').then_some(())?;
                at += 1;
                let refs = read_sequence_at::<3>(&node.bytes, &mut at)?;
                let values = [
                    View::f64_be_at(&node.bytes, at)?,
                    View::f64_be_at(&node.bytes, at + 8)?,
                    View::f64_be_at(&node.bytes, at + 16)?,
                    View::f64_be_at(&node.bytes, at + 24)?,
                ];
                (node.bytes.get(at + 32..at + 40)? == [0, 1, 0, 1, 0, 1, 0, 1]).then_some(())?;
                Some(BlendSurface {
                    xmt: node.xmt(),
                    state: BlendSurfaceState::from_metres(
                        [refs[0], refs[1]],
                        refs[2],
                        [values[0], values[1]],
                        [values[2], values[3]],
                    )
                    .ok()?,
                    pos: node.pos(),
                })
            }),
            "NX blend surfaces",
        )
    }
}

/// Decode validated type-60 offset-surface records.
pub(crate) fn offset_surfaces(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<Vec<OffsetSurface>, CodecError> {
    Graph::parse(ctx, stream)?.offset_surfaces(ctx)
}

impl Graph {
    pub(crate) fn offset_surfaces(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<OffsetSurface>, CodecError> {
        ctx.collect_retained_vec(
            ctx.admit_iter(
                self.of_kind(NodeKind::OffsetSurface),
                "NX topology carrier records",
            )?
            .filter_map(|node| {
                let mut at = node.common_header()?.1;
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
                    xmt: node.xmt(),
                    discriminator,
                    true_offset,
                    state: OffsetSurfaceState::new(support, distance).ok()?,
                    pos: node.pos(),
                })
            }),
            "NX offset surfaces",
        )
    }
}

/// Decode type-137 surface-curve records as aliases of their 3D basis curves.
pub(crate) fn surface_curves(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<Vec<SurfaceCurve>, CodecError> {
    Graph::parse(ctx, stream)?.surface_curves(ctx)
}

impl Graph {
    pub(crate) fn surface_curves(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<SurfaceCurve>, CodecError> {
        ctx.collect_retained_vec(
            ctx.admit_iter(
                self.of_kind(NodeKind::SpCurve),
                "NX topology carrier records",
            )?
            .filter_map(|node| {
                let mut at = node.common_header()?.1;
                let refs = read_sequence_at::<3>(&node.bytes, &mut at)?;
                let tolerance = View::f64_be_at(&node.bytes, at)?;
                Some(SurfaceCurve {
                    xmt: node.xmt(),
                    state: SurfaceCurveState::new(refs[0], refs[1], refs[2], tolerance).ok()?,
                    pos: node.pos(),
                })
            }),
            "NX surface curves",
        )
    }
}

/// Decode supported type-133 trimmed-curve records.
///
/// The result retains the basis-curve reference and parameter range. Topological
/// endpoints come from the corresponding edge and vertex records.
pub(crate) fn trimmed_curves(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<Vec<TrimmedCurve>, CodecError> {
    Graph::parse(ctx, stream)?.trimmed_curves(ctx)
}

impl Graph {
    pub(crate) fn trimmed_curves(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<TrimmedCurve>, CodecError> {
        ctx.collect_retained_vec(
            ctx.admit_iter(
                self.of_kind(NodeKind::TrimmedCurve),
                "NX topology carrier records",
            )?
            .filter_map(|node| {
                let mut at = node.common_header()?.1;
                let basis = read_and_advance(&node.bytes, &mut at)?;
                let point_0 = vec3_be_at(&node.bytes, at)?;
                let point_1 = vec3_be_at(&node.bytes, at + 24)?;
                let p0 = View::f64_be_at(&node.bytes, at + 48)?;
                let p1 = View::f64_be_at(&node.bytes, at + 56)?;
                Some(TrimmedCurve {
                    xmt: node.xmt(),
                    state: TrimmedCurveState::from_metres(basis, [point_0, point_1], [p0, p1])
                        .ok()?,
                    pos: node.pos(),
                })
            }),
            "NX trimmed curves",
        )
    }
}

/// Temporary storage held by one parsed graph until the caller keeps it.
struct GraphStorage<'ctx> {
    /// Copied node record bytes.
    nodes: ScopedReservation<'ctx>,
    /// Kind groups and the identity and position indexes.
    index: ScopedReservation<'ctx>,
}

impl GraphStorage<'_> {
    fn commit_value(self, graph: Graph) -> Result<Graph, CodecError> {
        let graph = self.nodes.commit_value(graph)?;
        self.index.commit_value(graph)
    }
}

/// Fixed-record candidates of one identity domain, in stream order.
#[derive(Default)]
struct DomainCandidates {
    /// Typed topology and carrier candidates.
    typed: Vec<NodeCandidate>,
    /// BODY and REGION candidates, which carry ownership identity only.
    ownership: Vec<NodeCandidate>,
}

type BodyShapeFaceVisit<'graph, 'visit> =
    dyn FnMut(&'graph Node, u32, &[u32]) -> Result<ControlFlow<()>, CodecError> + 'visit;

pub(crate) enum BodyShapeShellVisitor<'graph, 'visit> {
    Faces(&'visit mut BodyShapeFaceVisit<'graph, 'visit>),
    Summary(&'visit mut dyn FnMut(u32, usize) -> Result<ControlFlow<()>, CodecError>),
}

enum BodyShapeShellTraversal<'graph, 'visit> {
    Faces {
        owners: BTreeMap<u32, Vec<u32>>,
        visit: &'visit mut BodyShapeFaceVisit<'graph, 'visit>,
    },
    Summary {
        owner_counts: BTreeMap<u32, usize>,
        visit: &'visit mut dyn FnMut(u32, usize) -> Result<ControlFlow<()>, CodecError>,
    },
}

const INDEX_OPERATION: &str = "NX topology node index";

impl Graph {
    /// Parse supported fixed-record nodes from a neutral-binary stream.
    ///
    /// One scan frames every candidate under both identity domains. The
    /// baseline domain excludes high node identities; the full domain is kept
    /// only when it preserves every baseline node and completes a body
    /// topology that the baseline does not.
    pub(crate) fn parse(ctx: &DecodeContext<'_>, stream: &[u8]) -> Result<Self, CodecError> {
        let (domains, candidate_storage) = Self::scan_candidates(ctx, stream)?;
        let [baseline_candidates, full_candidates] = domains;
        let (baseline_candidate, mut baseline_storage) =
            Self::select_graph(ctx, stream, &baseline_candidates)?;
        let mut baseline = baseline_candidate;
        let (full_candidate, full_domain_storage) =
            Self::select_graph(ctx, stream, &full_candidates)?;
        let full_domain = full_candidate;
        drop((baseline_candidates, full_candidates, candidate_storage));
        if !baseline.is_preserved_by(ctx, &full_domain)? {
            drop(full_domain);
            drop(full_domain_storage);
            return baseline_storage.commit_value(baseline);
        }
        if !baseline.has_complete_body_topology(ctx)? {
            let (complete, faces) = full_domain.body_topology_census(ctx)?;
            if complete && faces != 0 {
                drop(baseline);
                drop(baseline_storage);
                return full_domain_storage.commit_value(full_domain);
            }
        }
        baseline.admit_referenced_full_domain_nodes(ctx, &mut baseline_storage, &full_domain)?;
        drop(full_domain);
        drop(full_domain_storage);
        baseline_storage.commit_value(baseline)
    }

    /// Return whether `other` holds every node of this graph at the same span.
    ///
    /// Both graphs frame the same stream, so equal spans hold equal bytes.
    fn is_preserved_by(&self, ctx: &DecodeContext<'_>, other: &Self) -> Result<bool, CodecError> {
        for group in &self.kinds {
            let mut nodes = group.iter();
            while !nodes.as_slice().is_empty() {
                let Some(node) =
                    ctx.next_charged(&mut nodes, "NX baseline topology preservation")?
                else {
                    break;
                };
                let Some(candidate) = other.get(ctx, node.kind, node.xmt())? else {
                    return Ok(false);
                };
                if (candidate.pos, candidate.end) != (node.pos, node.end) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// Admit full-domain nodes through unique typed XMT references.
    fn admit_referenced_full_domain_nodes<'ctx>(
        &mut self,
        ctx: &'ctx DecodeContext<'_>,
        storage: &mut GraphStorage<'ctx>,
        full_domain: &Self,
    ) -> Result<(), CodecError> {
        const OPERATION: &str = "NX topology full-domain references";
        let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
        let mut candidates = BTreeMap::<(ReferenceRole, u32), Option<&Node>>::new();
        for group in &full_domain.kinds {
            for node in ctx.admit_iter(group, "NX full-domain topology nodes")? {
                let Some(role) = ReferenceRole::for_kind(node.kind) else {
                    continue;
                };
                let key = (role, node.xmt());
                match ctx.get_mut_btree_map(&mut candidates, &key, OPERATION)? {
                    Some(candidate) => *candidate = None,
                    None => {
                        // discarded-value: the key was absent before the admitted insertion.
                        let _ = scratch.with_storage(|| {
                            ctx.insert_btree_map(&mut candidates, key, Some(node), OPERATION)
                        })?;
                    }
                }
            }
        }
        let mut pending = Vec::new();
        for group in &self.kinds {
            for node in ctx.admit_iter(group, "NX topology required source nodes")? {
                for target in node.reference_targets().into_iter().flatten() {
                    scratch.with_storage(|| ctx.push_vec(&mut pending, target, OPERATION))?;
                }
            }
        }
        let mut visited = BTreeSet::new();
        let mut added: [Vec<Node>; NodeKind::COUNT] = Default::default();
        let mut any_added = false;
        while let Some(target) = pending.pop() {
            if !scratch.with_storage(|| ctx.insert_btree_set(&mut visited, target, OPERATION))? {
                continue;
            }
            let Some(Some(candidate)) = ctx.get_btree_map(&candidates, &target, OPERATION)? else {
                continue;
            };
            if ctx.contains_key_btree_map(
                &self.keys,
                &(candidate.kind, candidate.xmt()),
                OPERATION,
            )? {
                continue;
            }
            for target in candidate.reference_targets().into_iter().flatten() {
                scratch.with_storage(|| ctx.push_vec(&mut pending, target, OPERATION))?;
            }
            let bytes = storage.nodes.with_storage(|| {
                ctx.copy_slice(&candidate.bytes, "NX topology admitted node bytes")
            })?;
            let node = Node {
                kind: candidate.kind,
                xmt: candidate.xmt,
                pos: candidate.pos,
                end: candidate.end,
                shift: candidate.shift,
                bytes,
            };
            let group = &mut added[candidate.kind.ordinal()];
            scratch.with_storage(|| ctx.push_vec(group, node, OPERATION))?;
            any_added = true;
        }
        if !any_added {
            return Ok(());
        }
        // Rebuild every kind group in physical order with fresh indexes.
        let mut index = ctx.reserve_scoped(0, INDEX_OPERATION)?;
        let mut graph = Self::default();
        for (existing, mut admitted) in std::mem::take(&mut self.kinds).into_iter().zip(added) {
            if admitted.len() > 1 {
                ctx.stable_sort_by_key(
                    &mut admitted,
                    |node| node.pos,
                    Ord::cmp,
                    "sort NX admitted topology nodes",
                )?;
            }
            let mut existing = existing.into_iter().peekable();
            let mut admitted = admitted.into_iter().peekable();
            while existing.len() != 0 || admitted.len() != 0 {
                ctx.charge_work(1, "merge NX admitted topology nodes")?;
                let take_admitted = match (existing.peek(), admitted.peek()) {
                    (Some(existing), Some(admitted)) => admitted.pos < existing.pos,
                    (None, _) => true,
                    (Some(_), None) => false,
                };
                let next = if take_admitted {
                    admitted.next()
                } else {
                    existing.next()
                };
                let Some(node) = next else {
                    break;
                };
                graph.push_node(ctx, &mut index, node)?;
            }
        }
        *self = graph;
        storage.index = index;
        Ok(())
    }

    /// Append a node to its kind group and index it. Nodes of one kind arrive
    /// in physical order.
    fn push_node(
        &mut self,
        ctx: &DecodeContext<'_>,
        index: &mut ScopedReservation<'_>,
        node: Node,
    ) -> Result<(), CodecError> {
        let kind = node.kind;
        let key = (kind, node.xmt());
        let pos = node.pos;
        let group = &mut self.kinds[kind.ordinal()];
        let position = group.len();
        let keys = &mut self.keys;
        let by_pos = &mut self.by_pos;
        index.with_storage(|| {
            // discarded-value: candidate selection leaves one node per identity and offset.
            let _ = ctx.insert_btree_map(keys, key, position, INDEX_OPERATION)?;
            // discarded-value: candidate selection leaves one node per identity and offset.
            let _ = ctx.insert_btree_map(by_pos, pos, (kind, position), INDEX_OPERATION)?;
            ctx.push_vec(group, node, INDEX_OPERATION)
        })
    }

    /// Frame every fixed-record candidate once and keep the candidates valid
    /// under the baseline and the full identity domain.
    ///
    /// At most one candidate per offset survives framing, so each list is in
    /// strictly increasing offset order.
    fn scan_candidates<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        stream: &[u8],
    ) -> Result<([DomainCandidates; 2], ScopedReservation<'ctx>), CodecError> {
        const OPERATION: &str = "NX topology candidates";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut domains: [DomainCandidates; 2] = Default::default();
        let last = stream.len().saturating_sub(3);
        for pos in ctx.admit_iter(0..last, "scan NX topology candidates")? {
            if stream[pos] != 0 {
                continue;
            }
            let Ok(kind) = NodeKind::try_from(stream[pos + 1]) else {
                continue;
            };
            let frames = framed_record_candidates(stream, pos, kind);
            for (domain, full_node_id_domain) in domains.iter_mut().zip([false, true]) {
                let Some(candidate) =
                    Self::fixed_record_candidate(stream, pos, kind, &frames, full_node_id_domain)
                else {
                    continue;
                };
                let list = if matches!(kind, NodeKind::Body | NodeKind::Region) {
                    &mut domain.ownership
                } else {
                    &mut domain.typed
                };
                storage.with_storage(|| ctx.push_vec(list, candidate, OPERATION))?;
            }
        }
        Ok((domains, storage))
    }

    /// Select one domain's records and materialize them as a graph.
    fn select_graph<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        stream: &[u8],
        candidates: &DomainCandidates,
    ) -> Result<(Self, GraphStorage<'ctx>), CodecError> {
        // Resolve physical overlap before identity uniqueness. A candidate
        // that is wholly contained in a selected record is payload data, not
        // a second serialized node. Counting it first can invalidate the real
        // node and make otherwise stable identities depend on unrelated bytes.
        let (non_overlapping_candidate, non_overlapping_storage) =
            Self::select_non_overlapping_candidates(ctx, stream, &candidates.typed)?;
        let non_overlapping = non_overlapping_candidate;
        let (selected_candidate, selected_storage) =
            Self::select_unique_candidates(ctx, &non_overlapping)?;
        let selected = selected_candidate;
        drop(non_overlapping);
        drop(non_overlapping_storage);
        // BODY and REGION carry ownership identity only. Their opaque fixed
        // payloads can contain complete-looking typed tags, so they are
        // admitted after typed topology/carrier selection and never veto a
        // typed candidate. An ownership node that shares bytes with a typed
        // node is ambiguous and is omitted; shells retain the identity even
        // when the optional BODY or REGION record is absent.
        let (non_overlapping_candidate, ownership_nonoverlap_storage) =
            Self::select_non_overlapping_candidates(ctx, stream, &candidates.ownership)?;
        let non_overlapping_ownership = non_overlapping_candidate;
        let (ownership_candidate, ownership_unique_storage) =
            Self::select_unique_candidates(ctx, &non_overlapping_ownership)?;
        let ownership = ownership_candidate;
        drop(non_overlapping_ownership);
        drop(ownership_nonoverlap_storage);
        let mut storage = GraphStorage {
            nodes: ctx.reserve_scoped(0, "NX topology node bytes")?,
            index: ctx.reserve_scoped(0, INDEX_OPERATION)?,
        };
        let mut graph = Self::default();
        for candidate in ctx.admit_iter(&selected, "NX selected topology records")? {
            if let Some(node) = candidate.materialize(ctx, &mut storage.nodes, stream)? {
                graph.push_node(ctx, &mut storage.index, node)?;
            }
        }
        let (admitted_candidate, admitted_storage) =
            Self::admit_disjoint_ownership(ctx, &ownership, &selected)?;
        let admitted = admitted_candidate;
        drop(ownership);
        drop(ownership_unique_storage);
        drop(selected);
        drop(selected_storage);
        for candidate in ctx.admit_iter(&admitted, "NX admitted ownership records")? {
            if let Some(node) = candidate.materialize(ctx, &mut storage.nodes, stream)? {
                graph.push_node(ctx, &mut storage.index, node)?;
            }
        }
        drop(admitted);
        drop(admitted_storage);
        Ok((graph, storage))
    }

    /// Keep the ownership candidates that share no bytes with a selected
    /// typed record. `selected` holds disjoint spans in offset order, so the
    /// only span that can overlap a candidate is the first one ending after
    /// its start.
    fn admit_disjoint_ownership<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        ownership: &[NodeCandidate],
        selected: &[NodeCandidate],
    ) -> Result<(Vec<NodeCandidate>, ScopedReservation<'ctx>), CodecError> {
        const OPERATION: &str = "compare NX ownership overlaps";
        let mut storage = ctx.reserve_scoped(0, "NX admitted ownership candidates")?;
        let mut admitted = Vec::new();
        for candidate in ctx.admit_iter(ownership, OPERATION)? {
            let first_after = ctx.partition_point(
                selected,
                |selected| Ok(selected.end <= candidate.pos),
                OPERATION,
            )?;
            if !selected
                .get(first_after)
                .is_some_and(|selected| selected.overlaps(*candidate))
            {
                storage.with_storage(|| ctx.push_vec(&mut admitted, *candidate, OPERATION))?;
            }
        }
        Ok((admitted, storage))
    }

    /// The candidate one domain admits at a fixed-record tag.
    fn fixed_record_candidate(
        stream: &[u8],
        pos: usize,
        kind: NodeKind,
        frames: &[Option<FixedRecordFrame>; 2],
        full_node_id_domain: bool,
    ) -> Option<NodeCandidate> {
        let mut candidates = [None; 2];
        let mut count = 0;
        for frame in frames.iter().flatten() {
            if candidate_has_valid_family_framing(
                stream,
                pos,
                kind,
                frame.shift(),
                frame.end(),
                full_node_id_domain,
            )
            .is_none()
            {
                continue;
            }
            candidates[count] = Some(NodeCandidate {
                kind,
                xmt: frame.xmt(),
                pos,
                shift: frame.shift(),
                end: frame.end(),
            });
            count += 1;
        }
        if count < 2 {
            return candidates[0];
        }
        let mut boundary_candidates = candidates
            .iter()
            .flatten()
            .copied()
            .filter(|candidate| fixed_record_boundary(stream, candidate.end()));
        let candidate = boundary_candidates.next()?;
        boundary_candidates.next().is_none().then_some(candidate)
    }

    /// Keep one complete physical record for each serialized identity.
    ///
    /// A second record with the same `(kind, xmt)` is not a recoverable choice:
    /// the fixed-record grammar provides no discriminator that can make one
    /// authoritative. Invalidate the identity instead of ranking candidates
    /// by topology shape, reference counts, or scan position. The result keeps
    /// the input order.
    fn select_unique_candidates<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        candidates: &[NodeCandidate],
    ) -> Result<(Vec<NodeCandidate>, ScopedReservation<'ctx>), CodecError> {
        const OPERATION: &str = "NX topology unique candidates";
        let mut repeated_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut repeated = BTreeMap::<(NodeKind, u32), bool>::new();
        for candidate in ctx.admit_iter(candidates, OPERATION)? {
            let key = (candidate.kind, candidate.xmt());
            match ctx.get_mut_btree_map(&mut repeated, &key, OPERATION)? {
                Some(flag) => *flag = true,
                None => {
                    // discarded-value: the key was absent before the admitted insertion.
                    let _ = repeated_storage.with_storage(|| {
                        ctx.insert_btree_map(&mut repeated, key, false, OPERATION)
                    })?;
                }
            }
        }
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut selected = Vec::new();
        for candidate in ctx.admit_iter(candidates, OPERATION)? {
            let key = (candidate.kind, candidate.xmt());
            if ctx.get_btree_map(&repeated, &key, OPERATION)? == Some(&false) {
                storage.with_storage(|| ctx.push_vec(&mut selected, *candidate, OPERATION))?;
            }
        }
        Ok((selected, storage))
    }

    /// Discard overlapping candidates when no serialized ownership boundary
    /// identifies which record owns the bytes. `nodes` is in increasing
    /// offset order; the result keeps that order and holds disjoint spans.
    fn select_non_overlapping_candidates<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        stream: &[u8],
        nodes: &[NodeCandidate],
    ) -> Result<(Vec<NodeCandidate>, ScopedReservation<'ctx>), CodecError> {
        const OPERATION: &str = "select NX topology candidates";
        let mut storage = ctx.reserve_scoped(0, "NX topology nonoverlapping candidates")?;
        let mut selected = Vec::new();
        let mut start = 0;
        while let Some(first) = nodes.get(start).copied() {
            ctx.charge_work(1, OPERATION)?;
            let end = if fixed_record_boundary(stream, first.end()) {
                storage.with_storage(|| ctx.push_vec(&mut selected, first, OPERATION))?;
                // Candidates starting inside the selected record are its payload.
                let mut end = start + 1;
                while let Some(candidate) = nodes.get(end) {
                    ctx.charge_work(1, OPERATION)?;
                    if candidate.pos() >= first.end() {
                        break;
                    }
                    end += 1;
                }
                end
            } else {
                let mut end = start + 1;
                let mut cluster_end = first.end();
                while let Some(candidate) = nodes.get(end) {
                    ctx.charge_work(1, OPERATION)?;
                    if candidate.pos() >= cluster_end {
                        break;
                    }
                    cluster_end = cluster_end.max(candidate.end());
                    end += 1;
                }
                let cluster = &nodes[start..end];
                let unique_boundary = if let [node] = cluster {
                    Some(*node)
                } else {
                    let mut bounded = None;
                    let mut ambiguous = false;
                    let mut candidates = cluster.iter();
                    while !candidates.as_slice().is_empty() {
                        let Some(candidate) = ctx.next_charged(&mut candidates, OPERATION)? else {
                            break;
                        };
                        if fixed_record_boundary(stream, candidate.end()) {
                            ambiguous = bounded.is_some();
                            if ambiguous {
                                break;
                            }
                            bounded = Some(*candidate);
                        }
                    }
                    bounded.filter(|_| !ambiguous)
                };
                if let Some(node) = unique_boundary {
                    storage.with_storage(|| ctx.push_vec(&mut selected, node, OPERATION))?;
                }
                end
            };
            start = end;
        }
        Ok((selected, storage))
    }

    /// Look up a node by record type and XMT identifier.
    pub(crate) fn get(
        &self,
        ctx: &DecodeContext<'_>,
        kind: NodeKind,
        xmt: u32,
    ) -> Result<Option<&Node>, CodecError> {
        let Some(&position) =
            ctx.get_btree_map(&self.keys, &(kind, xmt), "NX topology node lookup")?
        else {
            return Ok(None);
        };
        Ok(self.of_kind(kind).get(position))
    }

    /// Look up a node by record type tag and XMT identifier; an unsupported
    /// tag has no node.
    pub(crate) fn get_by_tag(
        &self,
        ctx: &DecodeContext<'_>,
        tag: u8,
        xmt: u32,
    ) -> Result<Option<&Node>, CodecError> {
        match NodeKind::try_from(tag) {
            Ok(kind) => self.get(ctx, kind, xmt),
            Err(()) => Ok(None),
        }
    }

    /// Look up the node a typed reference targets; the null reference has none.
    pub(crate) fn get_target(
        &self,
        ctx: &DecodeContext<'_>,
        kind: NodeKind,
        target: Option<XmtTarget>,
    ) -> Result<Option<&Node>, CodecError> {
        match target {
            Some(target) => self.get(ctx, kind, u32::from(target)),
            None => Ok(None),
        }
    }

    /// Look up the node whose type tag starts at `pos`.
    pub(crate) fn at_pos(
        &self,
        ctx: &DecodeContext<'_>,
        pos: usize,
    ) -> Result<Option<&Node>, CodecError> {
        let Some(&(kind, position)) =
            ctx.get_btree_map(&self.by_pos, &pos, "NX topology offset lookup")?
        else {
            return Ok(None);
        };
        Ok(self.of_kind(kind).get(position))
    }

    /// Nodes of one record type in physical record order.
    pub(crate) fn of_kind(&self, kind: NodeKind) -> &[Node] {
        &self.kinds[kind.ordinal()]
    }

    /// Resolve one current XMT identity from a kernel node identity that occurs
    /// once among the nodes of its kind.
    pub(crate) fn unique_xmt_by_node_id(
        &self,
        ctx: &DecodeContext<'_>,
        kind: NodeKind,
        node_id: u32,
    ) -> Result<Option<u32>, CodecError> {
        const OPERATION: &str = "NX topology node identity index";
        let index = match self.node_ids.get() {
            Some(index) => index,
            None => {
                let mut index = BTreeMap::new();
                for group in &self.kinds {
                    for node in ctx.admit_iter(group, OPERATION)? {
                        let Some(id) = node.node_id() else {
                            continue;
                        };
                        let key = (node.kind, id);
                        match ctx.get_mut_btree_map(&mut index, &key, OPERATION)? {
                            Some(xmt) => *xmt = None,
                            None => {
                                // discarded-value: the key was absent before the admitted insertion.
                                let _ = ctx.insert_btree_map(
                                    &mut index,
                                    key,
                                    Some(node.xmt()),
                                    OPERATION,
                                )?;
                            }
                        }
                    }
                }
                self.node_ids.get_or_init(|| index)
            }
        };
        Ok(ctx
            .get_btree_map(index, &(kind, node_id), OPERATION)?
            .copied()
            .flatten())
    }

    /// Curve identities occupying typed curve-reference slots in the fixed
    /// topology and procedural graph.
    pub(crate) fn referenced_curve_xmts(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<BTreeSet<u32>, CodecError> {
        const OPERATION: &str = "NX topology carrier references";
        let mut references = BTreeSet::new();
        let mut insert = |reference: Option<u32>| -> Result<(), CodecError> {
            if let Some(reference) = reference.filter(|reference| *reference > 1) {
                ctx.insert_btree_set(&mut references, reference, OPERATION)?;
            }
            Ok(())
        };
        for node in ctx.admit_iter(self.of_kind(NodeKind::Edge), OPERATION)? {
            insert(
                node.edge_fields()
                    .and_then(|fields| fields.curve.map(u32::from)),
            )?;
        }
        for node in ctx.admit_iter(self.of_kind(NodeKind::Fin), OPERATION)? {
            insert(
                node.fin_fields()
                    .and_then(|fields| fields.curve_xmt.map(u32::from)),
            )?;
        }
        for node in ctx.admit_iter(self.of_kind(NodeKind::BlendSurface), OPERATION)? {
            insert(node.blend_spine())?;
        }
        for node in ctx.admit_iter(self.of_kind(NodeKind::TrimmedCurve), OPERATION)? {
            insert(node.compact_tail_references::<1>().map(|items| items[0]))?;
        }
        for node in ctx.admit_iter(self.of_kind(NodeKind::SpCurve), OPERATION)? {
            insert(node.compact_tail_references::<3>().map(|items| items[2]))?;
        }
        Ok(references)
    }

    /// Resolve the exact witnesses of the unique edge carrying a curve.
    pub(crate) fn unique_curve_edge_witness(
        &self,
        ctx: &DecodeContext<'_>,
        curve_xmt: u32,
    ) -> Result<Option<CurveEdgeWitness>, CodecError> {
        const OPERATION: &str = "NX topology curve edge index";
        let index = match self.curve_edges.get() {
            Some(index) => index,
            None => {
                let mut index = BTreeMap::new();
                for (position, node) in ctx
                    .admit_iter(self.of_kind(NodeKind::Edge), OPERATION)?
                    .enumerate()
                {
                    let Some(curve) = node.edge_fields().and_then(|fields| fields.curve) else {
                        continue;
                    };
                    let curve = u32::from(curve);
                    match ctx.get_mut_btree_map(&mut index, &curve, OPERATION)? {
                        Some(edge) => *edge = None,
                        None => {
                            // discarded-value: the key was absent before the admitted insertion.
                            let _ =
                                ctx.insert_btree_map(&mut index, curve, Some(position), OPERATION)?;
                        }
                    }
                }
                self.curve_edges.get_or_init(|| index)
            }
        };
        let Some(Some(position)) = ctx.get_btree_map(index, &curve_xmt, OPERATION)?.copied() else {
            return Ok(None);
        };
        let Some(edge) = self
            .of_kind(NodeKind::Edge)
            .get(position)
            .and_then(Node::edge_fields)
        else {
            return Ok(None);
        };
        let Some(first_fin) = self
            .get_target(ctx, NodeKind::Fin, edge.fin)?
            .and_then(Node::fin_fields)
        else {
            return Ok(None);
        };
        let Some(second_fin) = self
            .get_target(ctx, NodeKind::Fin, first_fin.forward)?
            .and_then(Node::fin_fields)
        else {
            return Ok(None);
        };
        let position = |vertex_xmt| -> Result<Option<FinitePoint3>, CodecError> {
            let Some(point_xmt) = self
                .get_target(ctx, NodeKind::Vertex, vertex_xmt)?
                .and_then(Node::vertex_fields)
                .map(|fields| fields.point)
            else {
                return Ok(None);
            };
            Ok(self
                .get_target(ctx, NodeKind::Point, point_xmt)?
                .and_then(Node::point_position))
        };
        let (Some(start), Some(end)) = (position(first_fin.vertex)?, position(second_fin.vertex)?)
        else {
            return Ok(None);
        };
        Ok(Some(CurveEdgeWitness {
            endpoints: [start, end],
            tolerance: edge.tolerance,
        }))
    }

    /// Carrier identities required by the surviving fixed topology image.
    pub(crate) fn referenced_carrier_xmts(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<BTreeSet<u32>, CodecError> {
        const OPERATION: &str = "NX topology carrier references";
        let mut references = self.referenced_curve_xmts(ctx)?;
        for node in ctx.admit_iter(self.of_kind(NodeKind::Face), OPERATION)? {
            if let Some(reference) = node
                .face_fields()
                .and_then(|fields| fields.surface.map(u32::from))
                .filter(|reference| *reference > 1)
            {
                ctx.insert_btree_set(&mut references, reference, OPERATION)?;
            }
        }
        for node in ctx.admit_iter(self.of_kind(NodeKind::Vertex), OPERATION)? {
            if let Some(reference) = node
                .vertex_fields()
                .and_then(|fields| fields.point.map(u32::from))
                .filter(|reference| *reference > 1)
            {
                ctx.insert_btree_set(&mut references, reference, OPERATION)?;
            }
        }
        Ok(references)
    }

    /// Visit each validated body-shape shell's body identity in physical shell
    /// order. A shared body identity may be visited more than once.
    pub(crate) fn visit_body_shape_body_ids(
        &self,
        ctx: &DecodeContext<'_>,
        mut visit_body_id: impl FnMut(u32) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let mut visit = |body_id, _| {
            visit_body_id(body_id)?;
            Ok(ControlFlow::Continue(()))
        };
        let (ControlFlow::Continue(()) | ControlFlow::Break(())) =
            self.visit_body_shape_shells(ctx, BodyShapeShellVisitor::Summary(&mut visit))?;
        Ok(())
    }

    /// Return whether any validated body-shape shell exists.
    pub(crate) fn has_body_shape_shell(&self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        let mut visit = |_, _| Ok(ControlFlow::Break(()));
        Ok(matches!(
            self.visit_body_shape_shells(ctx, BodyShapeShellVisitor::Summary(&mut visit))?,
            ControlFlow::Break(())
        ))
    }

    /// Traverse shells against one owner index. Both modes validate shell
    /// fields and face ownership. `Faces` stages the shell's owned face IDs
    /// and calls back only after the whole chain is valid; `Summary` visits
    /// only body and face counts. A visitor break stops the shell scan at
    /// that candidate.
    pub(crate) fn visit_body_shape_shells<'graph>(
        &'graph self,
        ctx: &DecodeContext<'_>,
        visitor: BodyShapeShellVisitor<'graph, '_>,
    ) -> Result<ControlFlow<()>, CodecError> {
        const FACE_INDEX: &str = "NX shell face index";
        const SHELL_VISIT: &str = "classify NX body shells";
        let mut index_storage = ctx.reserve_scoped(0, FACE_INDEX)?;
        let mut traversal = match visitor {
            BodyShapeShellVisitor::Faces(visit) => BodyShapeShellTraversal::Faces {
                owners: BTreeMap::new(),
                visit,
            },
            BodyShapeShellVisitor::Summary(visit) => BodyShapeShellTraversal::Summary {
                owner_counts: BTreeMap::new(),
                visit,
            },
        };
        let mut remaining_faces = self.of_kind(NodeKind::Face).iter();
        while !remaining_faces.as_slice().is_empty() {
            let Some(face) = ctx.next_charged(&mut remaining_faces, FACE_INDEX)? else {
                break;
            };
            let Some(shell) = face.face_fields().and_then(|fields| fields.shell) else {
                continue;
            };
            let shell = u32::from(shell);
            match &mut traversal {
                BodyShapeShellTraversal::Faces { owners, .. } => {
                    index_storage.with_storage(|| {
                        ctx.push_btree_group(owners, shell, face.xmt(), FACE_INDEX, FACE_INDEX)
                    })?;
                }
                BodyShapeShellTraversal::Summary { owner_counts, .. } => {
                    match ctx.get_mut_btree_map(owner_counts, &shell, FACE_INDEX)? {
                        // The count cannot exceed the graph's physical FACE count.
                        Some(count) => *count += 1,
                        None => {
                            index_storage.with_storage(|| {
                                ctx.insert_btree_map(owner_counts, shell, 1usize, FACE_INDEX)
                            })?;
                        }
                    }
                }
            }
        }
        let mut remaining_shells = self.of_kind(NodeKind::Shell).iter();
        while !remaining_shells.as_slice().is_empty() {
            let Some(shell) = ctx.next_charged(&mut remaining_shells, SHELL_VISIT)? else {
                break;
            };
            let Some(flow) = self.visit_body_shell_faces(ctx, shell, &mut traversal)? else {
                continue;
            };
            if let ControlFlow::Break(()) = flow {
                return Ok(ControlFlow::Break(()));
            }
        }
        Ok(ControlFlow::Continue(()))
    }

    /// Validate one shell, publishing a complete linked face chain only after
    /// every link resolves to a face owned by this shell.
    fn visit_body_shell_faces<'graph>(
        &'graph self,
        ctx: &DecodeContext<'_>,
        shell: &'graph Node,
        traversal: &mut BodyShapeShellTraversal<'graph, '_>,
    ) -> Result<Option<ControlFlow<()>>, CodecError> {
        const OPERATION: &str = "validate NX shell faces";
        let Some(fields) = shell.shell_fields() else {
            return Ok(None);
        };
        let Some(body_id) = fields.body.map(u32::from) else {
            return Ok(None);
        };
        if fields.attributes.is_some()
            || fields.next_shell.is_some()
            || fields.sentinel_0.is_some()
            || fields.sentinel_1.is_some()
            || body_id == 0
            || fields.region.is_none_or(|target| u32::from(target) == 0)
        {
            return Ok(None);
        }
        let owner_count = match traversal {
            BodyShapeShellTraversal::Faces { owners, .. } => ctx
                .get_btree_map(owners, &shell.xmt(), OPERATION)?
                .map_or(0, Vec::len),
            BodyShapeShellTraversal::Summary { owner_counts, .. } => ctx
                .get_btree_map(owner_counts, &shell.xmt(), OPERATION)?
                .copied()
                .unwrap_or(0),
        };
        if fields.last_face.is_some() {
            // The last-face anchor names the first face; ownership comes from
            // each face's shell reference.
            if fields.last_face != fields.first_face || owner_count == 0 {
                return Ok(None);
            }
            let anchored = self
                .get_target(ctx, NodeKind::Face, fields.first_face)?
                .and_then(Node::face_fields)
                .is_some_and(|face| face.shell.map(u32::from) == Some(shell.xmt()));
            if !anchored {
                return Ok(None);
            }
            return match traversal {
                BodyShapeShellTraversal::Faces { owners, visit } => {
                    let faces = ctx
                        .get_btree_map(owners, &shell.xmt(), OPERATION)?
                        .map_or(&[][..], Vec::as_slice);
                    Ok(Some(visit(shell, body_id, faces)?))
                }
                BodyShapeShellTraversal::Summary { visit, .. } => {
                    Ok(Some(visit(body_id, owner_count)?))
                }
            };
        }
        // Every FACE link must resolve to this shell. A chain longer than the
        // shell's owner count must repeat a face and cannot terminate.
        match traversal {
            BodyShapeShellTraversal::Faces { visit, .. } => {
                let mut face_storage = ctx.reserve_scoped(0, "NX shell face identities")?;
                let mut faces = Vec::new();
                let Some(face_count) = face_storage.with_storage(|| {
                    self.visit_shell_face_chain(
                        ctx,
                        shell,
                        fields.first_face,
                        owner_count,
                        |face| ctx.push_vec(&mut faces, face, "NX shell face identities"),
                    )
                })?
                else {
                    return Ok(None);
                };
                if face_count == 0 {
                    drop(faces);
                    drop(face_storage);
                    return Ok(None);
                }
                let flow = visit(shell, body_id, &faces)?;
                drop(faces);
                drop(face_storage);
                Ok(Some(flow))
            }
            BodyShapeShellTraversal::Summary { visit, .. } => {
                let Some(face_count) = self.visit_shell_face_chain(
                    ctx,
                    shell,
                    fields.first_face,
                    owner_count,
                    |_| Ok(()),
                )?
                else {
                    return Ok(None);
                };
                if face_count == 0 {
                    return Ok(None);
                }
                Ok(Some(visit(body_id, face_count)?))
            }
        }
    }

    fn visit_shell_face_chain(
        &self,
        ctx: &DecodeContext<'_>,
        shell: &Node,
        first_face: Option<XmtTarget>,
        owner_count: usize,
        mut visit_face: impl FnMut(u32) -> Result<(), CodecError>,
    ) -> Result<Option<usize>, CodecError> {
        const OPERATION: &str = "validate NX shell faces";
        let mut face_count = 0;
        let mut face_xmt = first_face;
        while let Some(target) = face_xmt {
            if face_count == owner_count {
                return Ok(None);
            }
            ctx.charge_work(1, OPERATION)?;
            let current = u32::from(target);
            let Some(face) = self
                .get(ctx, NodeKind::Face, current)?
                .and_then(Node::face_fields)
            else {
                return Ok(None);
            };
            if face.shell.map(u32::from) != Some(shell.xmt()) {
                return Ok(None);
            }
            visit_face(current)?;
            face_count += 1;
            face_xmt = face.next_face;
        }
        Ok((face_count != 0).then_some(face_count))
    }

    /// Return whether every body-shape face has a non-empty valid loop chain
    /// and every non-null radial FIN partner belongs to the same reachable
    /// body topology.
    pub(crate) fn has_complete_body_topology(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<bool, CodecError> {
        Ok(self.body_topology_census(ctx)?.0)
    }

    /// Count faces owned by validated body-shape shells.
    pub(crate) fn body_shape_face_count(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<usize, CodecError> {
        let mut face_count = 0;
        let mut visit = |_, shell_face_count| {
            // Shell face ownership partitions the graph's faces, and each
            // accepted chain stays within its owned set.
            face_count += shell_face_count;
            Ok(ControlFlow::Continue(()))
        };
        let (ControlFlow::Continue(()) | ControlFlow::Break(())) =
            self.visit_body_shape_shells(ctx, BodyShapeShellVisitor::Summary(&mut visit))?;
        Ok(face_count)
    }

    /// Whether the body topology is complete, and how many body-shape faces
    /// it has, from one classification of the shells.
    fn body_topology_census(&self, ctx: &DecodeContext<'_>) -> Result<(bool, usize), CodecError> {
        const OPERATION: &str = "NX reachable FIN identities";
        let mut reachable_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut reachable_fins = BTreeSet::new();
        let mut faces = 0usize;
        let mut rings_complete = true;
        {
            let mut visit_shell = |_, _, face_xmts: &[u32]| {
                faces += face_xmts.len();
                if rings_complete {
                    let mut remaining_faces = face_xmts.iter();
                    while !remaining_faces.as_slice().is_empty() {
                        let Some(&face_xmt) =
                            ctx.next_charged(&mut remaining_faces, "NX body shell faces")?
                        else {
                            break;
                        };
                        let mut ring_storage = ctx.reserve_scoped(0, "NX body face rings")?;
                        let rings = ring_storage.with_storage(|| {
                            match self.face_loop_rings(ctx, face_xmt) {
                                Ok(rings) => Ok(Ok(rings)),
                                Err(FaceLoopError::Invalid(failure)) => Ok(Err(failure)),
                                Err(FaceLoopError::Codec(error)) => Err(error),
                            }
                        })?;
                        match rings {
                            Ok(rings) if !rings.is_empty() => {
                                for (_, ring) in ctx.admit_iter(&rings, OPERATION)? {
                                    for &xmt in ctx.admit_iter(ring, OPERATION)? {
                                        reachable_storage.with_storage(|| {
                                            ctx.insert_btree_set(
                                                &mut reachable_fins,
                                                xmt,
                                                OPERATION,
                                            )
                                        })?;
                                    }
                                }
                                drop(rings);
                                drop(ring_storage);
                            }
                            Ok(_) | Err(_) => {
                                rings_complete = false;
                                break;
                            }
                        }
                    }
                }
                Ok(ControlFlow::Continue(()))
            };
            let (ControlFlow::Continue(()) | ControlFlow::Break(())) =
                self.visit_body_shape_shells(ctx, BodyShapeShellVisitor::Faces(&mut visit_shell))?;
        }
        if !rings_complete || faces == 0 {
            drop(reachable_fins);
            drop(reachable_storage);
            return Ok((false, faces));
        }
        let mut remaining_fins = reachable_fins.iter();
        while remaining_fins.len() != 0 {
            let Some(&xmt) = ctx.next_charged(&mut remaining_fins, OPERATION)? else {
                break;
            };
            let Some(fields) = self
                .get(ctx, NodeKind::Fin, xmt)?
                .and_then(Node::fin_fields)
            else {
                return Ok((false, faces));
            };
            if let Some(other) = fields.other {
                if !ctx.contains_btree_set(&reachable_fins, &u32::from(other), OPERATION)? {
                    drop(reachable_fins);
                    drop(reachable_storage);
                    return Ok((false, faces));
                }
            }
        }
        drop(reachable_fins);
        drop(reachable_storage);
        Ok((true, faces))
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
        const OPERATION: &str = "walk NX face loops";
        let face = self
            .get(ctx, NodeKind::Face, face_xmt)?
            .and_then(Node::face_fields)
            .ok_or(FaceLoopFailure::InvalidFace { face_xmt })?;
        let mut loop_xmt = face.loop_xmt;
        let mut seen_storage = ctx.reserve_scoped(0, "NX face loop identities")?;
        let mut seen_loops = BTreeSet::new();
        let mut rings = Vec::new();
        while let Some(target) = loop_xmt {
            let current = u32::from(target);
            if !seen_storage.with_storage(|| {
                ctx.insert_btree_set(&mut seen_loops, current, "NX face loop identities")
            })? {
                return Err(FaceLoopFailure::InvalidLoopChain { loop_xmt: current }.into());
            }
            let fields = self
                .get(ctx, NodeKind::Loop, current)?
                .and_then(Node::loop_fields)
                .ok_or(FaceLoopFailure::InvalidLoopChain { loop_xmt: current })?;
            if fields.face.map(u32::from) != Some(face_xmt) {
                return Err(FaceLoopFailure::InvalidLoopChain { loop_xmt: current }.into());
            }
            let first_fin = fields
                .fin
                .ok_or(FaceLoopFailure::InvalidLoopChain { loop_xmt: current })?;
            let ring = self.fin_ring(ctx, current, first_fin)?;
            ctx.push_vec(&mut rings, (current, ring), OPERATION)?;
            loop_xmt = fields.next_loop;
        }
        Ok(rings)
    }

    fn fin_ring(
        &self,
        ctx: &DecodeContext<'_>,
        loop_xmt: u32,
        first: XmtTarget,
    ) -> Result<Vec<u32>, FaceLoopError> {
        const OPERATION: &str = "walk NX FIN ring";
        let first = u32::from(first);
        let mut current = first;
        let mut previous = None;
        let mut seen_storage = ctx.reserve_scoped(0, "NX FIN ring identities")?;
        let mut seen = BTreeSet::new();
        let mut ring = Vec::new();
        loop {
            ctx.charge_work(1, OPERATION)?;
            if !seen_storage.with_storage(|| {
                ctx.insert_btree_set(&mut seen, current, "NX FIN ring identities")
            })? {
                return if current == first {
                    Ok(ring)
                } else {
                    Err(FaceLoopFailure::InvalidFinRing {
                        loop_xmt,
                        fin_xmt: current,
                    }
                    .into())
                };
            }
            ctx.push_vec(&mut ring, current, "NX FIN ring entries")?;
            let invalid_fin = FaceLoopFailure::InvalidFinRing {
                loop_xmt,
                fin_xmt: current,
            };
            let fields = self
                .get(ctx, NodeKind::Fin, current)?
                .and_then(Node::fin_fields)
                .ok_or(invalid_fin)?;
            let vertex_resolves = self
                .get_target(ctx, NodeKind::Vertex, fields.vertex)?
                .is_some()
                || (fields.vertex.is_none()
                    && fields.forward.map(u32::from) == Some(current)
                    && fields.backward.map(u32::from) == Some(current));
            if self.get_target(ctx, NodeKind::Edge, fields.edge)?.is_none() {
                return Err(FaceLoopFailure::UnresolvedFinEdge {
                    loop_xmt,
                    fin_xmt: current,
                    edge_xmt: fields.edge.map(u32::from),
                }
                .into());
            }
            if fields.loop_xmt.map(u32::from) != Some(loop_xmt) || !vertex_resolves {
                return Err(invalid_fin.into());
            }
            if let Some(other_xmt) = fields.other {
                let other = self
                    .get(ctx, NodeKind::Fin, u32::from(other_xmt))?
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
                .get_target(ctx, NodeKind::Fin, fields.forward)?
                .and_then(Node::fin_fields)
                .ok_or(invalid_fin)?;
            if next.backward.map(u32::from) != Some(current) {
                return Err(invalid_fin.into());
            }
            previous = Some(current);
            current = u32::from(fields.forward.ok_or(invalid_fin)?);
        }
    }
}

#[cfg(test)]
impl Graph {
    /// Every node, kind by kind, for test assertions.
    pub(crate) fn test_nodes(&self) -> impl Iterator<Item = &Node> {
        self.kinds.iter().flatten()
    }

    /// Look a node up for a test assertion, outside any decode budget.
    pub(crate) fn node(&self, kind: NodeKind, xmt: u32) -> Option<&Node> {
        let &position = self.keys.get(&(kind, xmt))?;
        self.of_kind(kind).get(position)
    }

    /// Look the node at a type-tag offset up for a test assertion.
    pub(crate) fn node_at(&self, pos: usize) -> Option<&Node> {
        let &(kind, position) = self.by_pos.get(&pos)?;
        self.of_kind(kind).get(position)
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
    xmt: NonNullXmt,
    pos: usize,
    shift: usize,
    end: usize,
}

impl NodeCandidate {
    fn xmt(self) -> u32 {
        u32::from(self.xmt)
    }
    fn pos(self) -> usize {
        self.pos
    }
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
        let owned = reservation.with_storage(|| ctx.copy_slice(bytes, "NX topology node bytes"))?;
        Ok(Some(Node {
            kind: self.kind,
            xmt: self.xmt,
            pos: self.pos,
            end: self.end,
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
