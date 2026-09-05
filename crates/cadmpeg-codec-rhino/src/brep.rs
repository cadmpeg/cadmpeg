// SPDX-License-Identifier: Apache-2.0
//! Isolated `ON_Brep` parsing and semantic validation.
//!
//! Stops at a validated native representation; no topology IDs or IR carriers.

use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};
use std::ops::Range;

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_ir::geometry::CurveGeometry;

use crate::chunks::{
    chunk_at, verify_checksum, verify_checksum_ranges, ArchiveVersion, BoundedReader,
    ChecksumStatus, Chunk,
};
use crate::curves::{error, GeometryError};
use crate::objects::{parse_class_wrapper, parse_class_wrapper_with_userdata, UserdataDescriptor};
use crate::settings::{bbox, interval, BoundingBox, Interval, Point3};
use crate::wire::Uuid;

/// `ON_Brep` class UUID.
pub(crate) const ON_BREP: Uuid = Uuid::from_canonical([
    0x60, 0xb5, 0xdb, 0xc5, 0xe6, 0x60, 0x11, 0xd3, 0xbf, 0xe4, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
/// V5 class-userdata UUID for the Brep region-topology carrier.
pub(crate) const V5_BREP_REGION_TOPOLOGY_USERDATA: Uuid = Uuid::from_canonical([
    0x7f, 0xe2, 0x3d, 0x63, 0xe5, 0x36, 0x43, 0xf1, 0x98, 0xe2, 0xc8, 0x07, 0xa2, 0x62, 0x5a, 0xff,
]);
const OPENNURBS4: Uuid = Uuid::from_canonical([
    0x17, 0xb3, 0xec, 0xda, 0x17, 0xba, 0x4e, 0x45, 0x9e, 0x67, 0xa2, 0xb8, 0xd9, 0xbe, 0x52, 0x0d,
]);
const LEGACY_TRIMMED_SURFACE: Uuid = Uuid::from_canonical([
    0x07, 0x05, 0xfd, 0xef, 0x3e, 0x2a, 0x11, 0xd4, 0x80, 0x0e, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const LEGACY_BREP: Uuid = Uuid::from_canonical([
    0x2d, 0x4c, 0xfe, 0xdb, 0x3e, 0x2a, 0x11, 0xd4, 0x80, 0x0e, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const TL_BREP: Uuid = Uuid::from_canonical([
    0xf0, 0x6f, 0xc2, 0x43, 0xa3, 0x2a, 0x46, 0x08, 0x9d, 0xd8, 0xa7, 0xd2, 0xc4, 0xce, 0x2a, 0x36,
]);
/// Maximum number of records in one Brep array.
pub(crate) const MAX_BREP_ITEMS: usize = 1 << 20;
/// Maximum nesting depth used while reading polymorphic children.
pub(crate) const MAX_BREP_DEPTH: usize = 32;
const ANONYMOUS: u32 = 0x4000_8000;
const ON_UNSET_VALUE: f64 = -1.234_321_012_343_21e308;
const ON_UNSET_POSITIVE_VALUE: f64 = -ON_UNSET_VALUE;
const ON_BREP_FACE_SIDE: Uuid = Uuid::from_canonical([
    0x30, 0x93, 0x03, 0x70, 0x0d, 0x5b, 0x4e, 0xe4, 0x80, 0x83, 0xbd, 0x63, 0x5c, 0x73, 0x98, 0xa4,
]);
const ON_BREP_REGION: Uuid = Uuid::from_canonical([
    0xca, 0x7a, 0x00, 0x92, 0x7e, 0xe6, 0x4f, 0x99, 0xb9, 0xd2, 0xe1, 0xd6, 0xaa, 0x79, 0x8a, 0xa1,
]);
type RegionRead = (
    Vec<RawBrepFaceSide>,
    Vec<RawBrepRegion>,
    Option<Range<usize>>,
    bool,
);

/// The base class family expected by a polymorphic Brep slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawBrepBaseType {
    /// A curve-derived Rhino class.
    Curve,
    /// A surface-derived Rhino class.
    Surface,
    /// A class outside the expected family.
    Other,
}

/// A polymorphic Brep child slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawBrepChild {
    /// Class UUID, when the slot is present.
    pub(crate) class_uuid: Uuid,
    /// Class-data byte range.
    pub(crate) class_data_range: Range<usize>,
    /// Complete class-wrapper byte range.
    pub(crate) source_range: Range<usize>,
    /// Base-class family inferred from the class UUID.
    pub(crate) base_type: RawBrepBaseType,
}

/// A positional polymorphic Brep array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawBrepChildren {
    /// Child slots, including null slots.
    pub(crate) slots: Vec<Option<RawBrepChild>>,
    /// Anonymous wrapper byte range.
    pub(crate) source_range: Range<usize>,
    /// Base-class family required by this array.
    pub(crate) expected_type: RawBrepBaseType,
}

/// A raw Brep vertex.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepVertex {
    /// Positional record index.
    pub(crate) index: i32,
    /// Vertex point.
    pub(crate) point: Point3,
    /// Incident edge indexes.
    pub(crate) edges: Vec<i32>,
    /// Vertex tolerance.
    pub(crate) tolerance: f64,
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// A raw Brep edge.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepEdge {
    /// Positional record index.
    pub(crate) index: i32,
    /// C3 curve slot.
    pub(crate) curve: i32,
    /// Proxy reversal flag.
    pub(crate) proxy_reversed: i32,
    /// Proxy domain.
    pub(crate) proxy_domain: Interval,
    /// Endpoint vertex indexes.
    pub(crate) vertices: [i32; 2],
    /// Incident trim indexes.
    pub(crate) trims: Vec<i32>,
    /// Edge tolerance.
    pub(crate) tolerance: f64,
    /// Native edge domain.
    pub(crate) domain: Interval,
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// A raw Brep trim.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepTrim {
    /// Positional record index.
    pub(crate) index: i32,
    /// C2 curve slot.
    pub(crate) curve: i32,
    /// Proxy domain.
    pub(crate) proxy_domain: Interval,
    /// Edge index, or `-1` for singular and point trims.
    pub(crate) edge: i32,
    /// Start and end vertex indexes.
    pub(crate) vertices: [i32; 2],
    /// Three-dimensional reversal flag.
    pub(crate) reversed_3d: i32,
    /// Raw trim-type value.
    pub(crate) trim_type: i32,
    /// Raw ISO value.
    pub(crate) iso: i32,
    /// Loop index.
    pub(crate) loop_index: i32,
    /// Two-dimensional and three-dimensional tolerances.
    pub(crate) tolerances: [f64; 2],
    /// Native trim domain.
    pub(crate) domain: Interval,
    /// Proxy reversal byte.
    pub(crate) proxy_reversed: u8,
    /// Reserved bytes from the current layout.
    pub(crate) reserved: Vec<u8>,
    /// Legacy 2D and 3D tolerances appended after the proxy block.
    pub(crate) legacy_tolerances: [f64; 2],
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// A raw Brep loop.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepLoop {
    /// Positional record index.
    pub(crate) index: i32,
    /// Directed trim ring.
    pub(crate) trims: Vec<i32>,
    /// Raw loop-type value.
    pub(crate) loop_type: i32,
    /// Face index.
    pub(crate) face: i32,
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// A raw Brep face.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepFace {
    /// Positional record index.
    pub(crate) index: i32,
    /// Face loop indexes.
    pub(crate) loops: Vec<i32>,
    /// Surface slot.
    pub(crate) surface: i32,
    /// Surface reversal flag.
    pub(crate) reversed_surface: i32,
    /// Material channel.
    pub(crate) material_channel: i32,
    /// Optional face UUID.
    pub(crate) uuid: Option<Uuid>,
    /// Optional per-face color.
    pub(crate) color: Option<[u8; 4]>,
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// A render or analysis mesh cache slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawBrepMeshSlot {
    /// Present mesh child, if it passed class validation.
    pub(crate) mesh: Option<RawBrepChild>,
    /// Whether the archive supplied a nonzero presence byte.
    pub(crate) present: bool,
    /// Class-userdata descriptors attached to the mesh object wrapper.
    pub(crate) userdata: Vec<UserdataDescriptor>,
}

/// A raw region face side.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepFaceSide {
    /// Positional side index.
    pub(crate) index: i32,
    /// Region index, or `-1` when unassigned.
    pub(crate) region: i32,
    /// Face index.
    pub(crate) face: i32,
    /// Surface-normal direction.
    pub(crate) direction: i32,
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// A raw Brep region.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepRegion {
    /// Positional region index.
    pub(crate) index: i32,
    /// Raw region type.
    pub(crate) region_type: i32,
    /// Member face-side indexes.
    pub(crate) sides: Vec<i32>,
    /// Region bounds.
    pub(crate) bounds: BoundingBox,
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// Parsed Brep data before semantic validation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrep {
    /// Typed losses raised while selecting writer-version-dependent layouts.
    pub(crate) losses: Vec<cadmpeg_ir::report::LossNote>,
    /// Packed payload minor.
    pub(crate) minor: u8,
    /// C2 curve slots.
    pub(crate) c2: RawBrepChildren,
    /// C3 curve slots.
    pub(crate) c3: RawBrepChildren,
    /// Surface slots.
    pub(crate) surfaces: RawBrepChildren,
    /// Vertex records.
    pub(crate) vertices: Vec<RawBrepVertex>,
    /// Edge records.
    pub(crate) edges: Vec<RawBrepEdge>,
    /// Trim records.
    pub(crate) trims: Vec<RawBrepTrim>,
    /// Loop records.
    pub(crate) loops: Vec<RawBrepLoop>,
    /// Face records.
    pub(crate) faces: Vec<RawBrepFace>,
    /// Brep bounds.
    pub(crate) bounds: BoundingBox,
    /// Render mesh cache slots.
    pub(crate) render_meshes: Vec<RawBrepMeshSlot>,
    /// Analysis mesh cache slots.
    pub(crate) analysis_meshes: Vec<RawBrepMeshSlot>,
    /// Complete render-mesh side-wrapper range.
    pub(crate) render_mesh_array_range: Range<usize>,
    /// Complete analysis-mesh side-wrapper range.
    pub(crate) analysis_mesh_array_range: Range<usize>,
    /// Raw solid state, normalized only by validation.
    pub(crate) is_solid: Option<i32>,
    /// Region face sides.
    pub(crate) face_sides: Vec<RawBrepFaceSide>,
    /// Regions.
    pub(crate) regions: Vec<RawBrepRegion>,
    /// Complete region-topology wrapper range.
    pub(crate) region_wrapper_range: Option<Range<usize>>,
    /// Complete payload range.
    pub(crate) source_range: Range<usize>,
    /// Complete vertex-array wrapper range.
    pub(crate) vertex_array_range: Range<usize>,
    /// Complete edge-array wrapper range.
    pub(crate) edge_array_range: Range<usize>,
    /// Complete trim-array wrapper range.
    pub(crate) trim_array_range: Range<usize>,
    /// Complete loop-array wrapper range.
    pub(crate) loop_array_range: Range<usize>,
    /// Complete face-array wrapper range.
    pub(crate) face_array_range: Range<usize>,
}

/// A semantically validated raw Brep.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ValidatedRawBrep {
    /// Validated Brep payload.
    raw: RawBrep,
    /// Warnings for repaired positional fields or discarded optional data.
    warnings: Vec<String>,
}

/// Body kind selected from one validated serialized Brep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrepBodyKind {
    /// A closed volumetric body.
    Solid,
    /// An open sheet body.
    Sheet,
}

impl ValidatedRawBrep {
    /// Validates and normalizes one structurally decoded Brep.
    pub(crate) fn try_new(mut raw: RawBrep) -> Result<Self, GeometryError> {
        let mut warnings = Vec::new();
        for (label, mismatch) in [
            (
                "vertex",
                raw.vertices
                    .iter()
                    .enumerate()
                    .any(|(index, value)| value.index != index as i32),
            ),
            (
                "edge",
                raw.edges
                    .iter()
                    .enumerate()
                    .any(|(index, value)| value.index != index as i32),
            ),
            (
                "trim",
                raw.trims
                    .iter()
                    .enumerate()
                    .any(|(index, value)| value.index != index as i32),
            ),
            (
                "loop",
                raw.loops
                    .iter()
                    .enumerate()
                    .any(|(index, value)| value.index != index as i32),
            ),
            (
                "face",
                raw.faces
                    .iter()
                    .enumerate()
                    .any(|(index, value)| value.index != index as i32),
            ),
            (
                "region face-side",
                raw.face_sides
                    .iter()
                    .enumerate()
                    .any(|(index, value)| value.index != index as i32),
            ),
            (
                "region",
                raw.regions
                    .iter()
                    .enumerate()
                    .any(|(index, value)| value.index != index as i32),
            ),
        ] {
            if mismatch {
                warnings.push(format!(
                    "redundant Brep {label} positional index mismatch; serialized array order used"
                ));
            }
        }
        for vertex in &raw.vertices {
            refs(&vertex.edges, raw.edges.len(), "vertex edge")?;
            finite_tolerance(vertex.tolerance, "vertex tolerance")?;
        }
        for (index, edge) in raw.edges.iter().enumerate() {
            if !typed_slot(&raw.c3, edge.curve, RawBrepBaseType::Curve) {
                return Err(error(
                    edge.source_range.start,
                    "edge C3 reference is invalid",
                ));
            }
            refs(&edge.vertices, raw.vertices.len(), "edge vertex")?;
            refs(&edge.trims, raw.trims.len(), "edge trim")?;
            unique(&edge.trims, "edge trim")?;
            finite_interval(edge.proxy_domain, "edge proxy domain")?;
            finite_interval(edge.domain, "edge domain")?;
            finite_tolerance(edge.tolerance, "edge tolerance")?;
            if edge.proxy_reversed != 0 && edge.proxy_reversed != 1 {
                return Err(error(
                    edge.source_range.start,
                    "invalid edge proxy reversal",
                ));
            }
            for trim in &edge.trims {
                if !raw.trims[*trim as usize].edge.eq(&(index as i32)) {
                    return Err(error(
                        edge.source_range.start,
                        "edge/trim reciprocity mismatch",
                    ));
                }
            }
        }
        for (trim_index, trim) in raw.trims.iter().enumerate() {
            if trim.trim_type == 6 {
                if trim.curve != -1 {
                    return Err(error(
                        trim.source_range.start,
                        "point-on-surface trim must not require C2",
                    ));
                }
            } else if !typed_slot(&raw.c2, trim.curve, RawBrepBaseType::Curve) {
                return Err(error(
                    trim.source_range.start,
                    "trim C2 reference is invalid",
                ));
            }
            refs(&trim.vertices, raw.vertices.len(), "trim vertex")?;
            refs(&[trim.loop_index], raw.loops.len(), "trim loop")?;
            if !raw.loops[trim.loop_index as usize]
                .trims
                .contains(&(trim_index as i32))
            {
                return Err(error(
                    trim.source_range.start,
                    "trim/loop reciprocity mismatch",
                ));
            }
            finite_interval(trim.proxy_domain, "trim proxy domain")?;
            finite_interval(trim.domain, "trim domain")?;
            for tolerance in trim.tolerances.into_iter().chain(trim.legacy_tolerances) {
                finite_tolerance(tolerance, "trim tolerance")?;
            }
            if trim.proxy_reversed > 1 || trim.reversed_3d != 0 && trim.reversed_3d != 1 {
                return Err(error(trim.source_range.start, "invalid trim reversal"));
            }
            if !(0..=7).contains(&trim.trim_type) || !(0..=6).contains(&trim.iso) {
                return Err(error(trim.source_range.start, "invalid trim enum value"));
            }
            if matches!(trim.trim_type, 4 | 6) {
                if trim.edge != -1 || trim.vertices[0] != trim.vertices[1] {
                    return Err(error(
                        trim.source_range.start,
                        "singular trim endpoints are invalid",
                    ));
                }
            } else {
                refs(&[trim.edge], raw.edges.len(), "trim edge")?;
            }
        }
        validate_edge_incidences(&raw)?;
        for (index, vertex) in raw.vertices.iter().enumerate() {
            for edge in &vertex.edges {
                if !raw.edges[*edge as usize].vertices.contains(&(index as i32)) {
                    return Err(error(
                        vertex.source_range.start,
                        "vertex/edge reciprocity mismatch",
                    ));
                }
            }
        }
        for (index, loop_record) in raw.loops.iter().enumerate() {
            refs(&loop_record.trims, raw.trims.len(), "loop trim")?;
            unique(&loop_record.trims, "loop trim")?;
            refs(&[loop_record.face], raw.faces.len(), "loop face")?;
            if !(0..=5).contains(&loop_record.loop_type) {
                return Err(error(
                    loop_record.source_range.start,
                    "invalid loop enum value",
                ));
            }
            if !raw.faces[loop_record.face as usize]
                .loops
                .contains(&(index as i32))
            {
                return Err(error(
                    loop_record.source_range.start,
                    "loop/face reciprocity mismatch",
                ));
            }
            if loop_record.loop_type == 1
                && raw.faces[loop_record.face as usize]
                    .loops
                    .first()
                    .is_none_or(|first| *first != index as i32)
            {
                return Err(error(
                    loop_record.source_range.start,
                    "outer loop is not first",
                ));
            }
        }
        for face in &mut raw.faces {
            if face.material_channel < 0 {
                face.material_channel = 0;
            }
        }
        for (index, face) in raw.faces.iter().enumerate() {
            if !typed_slot(&raw.surfaces, face.surface, RawBrepBaseType::Surface) {
                return Err(error(
                    face.source_range.start,
                    "face surface reference is invalid",
                ));
            }
            if face.reversed_surface != 0 && face.reversed_surface != 1 {
                return Err(error(
                    face.source_range.start,
                    "invalid face surface reversal",
                ));
            }
            refs(&face.loops, raw.loops.len(), "face loop")?;
            if face.loops.is_empty() {
                return Err(error(face.source_range.start, "face has no loops"));
            }
            if raw.loops[face.loops[0] as usize].loop_type != 1 {
                return Err(error(
                    face.source_range.start,
                    "face first loop is not outer",
                ));
            }
            for loop_index in face.loops.iter().skip(1) {
                let loop_type = raw.loops[*loop_index as usize].loop_type;
                if loop_type == 0 || loop_type == 1 {
                    return Err(error(
                        face.source_range.start,
                        "face boundary loop convention is invalid",
                    ));
                }
            }
            for loop_index in &face.loops {
                if raw.loops[*loop_index as usize].face != index as i32 {
                    return Err(error(
                        face.source_range.start,
                        "face/loop reciprocity mismatch",
                    ));
                }
            }
        }
        validate_rings(&raw)?;
        if raw.minor >= 3
            && (!raw.face_sides.is_empty() || !raw.regions.is_empty())
            && validate_regions(&raw).is_err()
        {
            raw.face_sides.clear();
            raw.regions.clear();
            warnings.push("invalid optional Brep region topology discarded".to_string());
        }
        Ok(Self { raw, warnings })
    }

    /// Returns the validated and normalized raw payload.
    pub(crate) fn raw(&self) -> &RawBrep {
        &self.raw
    }

    /// Returns warnings produced while validation normalized optional data.
    pub(crate) fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Selects the serialized body kind and reports an unverified stamp-dependent gauge.
    pub(crate) fn body_kind(
        &self,
        writer_version: Option<i64>,
    ) -> (BrepBodyKind, Option<cadmpeg_ir::report::LossNote>) {
        body_kind(&self.raw, writer_version)
    }
}

/// Classifies one B-rep body, reporting whether a missing stamp decided it.
fn body_kind(
    raw: &RawBrep,
    writer_version: Option<i64>,
) -> (BrepBodyKind, Option<cadmpeg_ir::report::LossNote>) {
    let closed = !raw.faces.is_empty()
        && raw.edges.iter().enumerate().all(|(edge, _)| {
            raw.trims
                .iter()
                .filter(|trim| trim.edge == edge as i32)
                .count()
                == 2
        });
    let kind = serialized_body_kind(raw.minor, raw.is_solid, writer_version, closed);
    let loss = body_kind_rests_on_missing_stamp(raw.minor, raw.is_solid, writer_version, closed)
        .then(|| {
            crate::loss::RhinoLossCode::TopologyBodyKindGaugeSubstituted.note(format!(
                "Brep body kind gauge substituted: stored solid flag {} was trusted over the \
                 closed-shell gauge because the writer-version stamp is absent",
                raw.is_solid.unwrap_or(-1)
            ))
        });
    (kind, loss)
}

/// First openNURBS writer version whose `ON_Brep` stores a meaningful solid flag.
const SOLID_FLAG_WRITER_VERSION: i64 = 200_210_020;

/// True when a missing writer stamp is what decided the body kind.
///
/// The stored solid flag is trusted when the stamp is absent and ignored when
/// the stamp is older than [`SOLID_FLAG_WRITER_VERSION`], so an unstamped
/// archive is classified on an assumption the archive does not carry. This
/// compares the two readings of the same bytes and reports only a disagreement:
/// where both readings pick the same body kind nothing was substituted.
fn body_kind_rests_on_missing_stamp(
    minor: u8,
    is_solid: Option<i32>,
    writer_version: Option<i64>,
    closed: bool,
) -> bool {
    writer_version.is_none()
        && serialized_body_kind(minor, is_solid, None, closed)
            != serialized_body_kind(minor, is_solid, Some(SOLID_FLAG_WRITER_VERSION - 1), closed)
}

fn serialized_body_kind(
    minor: u8,
    is_solid: Option<i32>,
    writer_version: Option<i64>,
    closed: bool,
) -> BrepBodyKind {
    let stored = (minor >= 2
        && writer_version.is_none_or(|version| version >= SOLID_FLAG_WRITER_VERSION))
    .then_some(is_solid)
    .flatten();
    match stored {
        Some(1 | 2) => BrepBodyKind::Solid,
        Some(0) => {
            if closed {
                BrepBodyKind::Solid
            } else {
                BrepBodyKind::Sheet
            }
        }
        _ if closed => BrepBodyKind::Solid,
        _ => BrepBodyKind::Sheet,
    }
}

/// Result of parsing a structurally framed Brep payload.
#[derive(Debug)]
pub(crate) enum BrepParse {
    /// The payload passed semantic topology validation.
    Valid(ValidatedRawBrep),
    /// The payload was framed and decoded, but its topology is invalid.
    SemanticInvalid {
        /// The decoded raw payload retained for geometry fallback.
        raw: RawBrep,
        /// The semantic validation failure.
        error: GeometryError,
        /// Recoverable optional-channel warnings found before validation.
        warnings: Vec<String>,
    },
}

/// Parses and validates one `ON_Brep` class-data payload.
pub(crate) fn parse(
    bytes: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    userdata: &[UserdataDescriptor],
) -> Result<BrepParse, GeometryError> {
    let mut reader = BoundedReader::new(bytes, range.start, range.end)?;
    let version_offset = reader.position();
    let version = reader.u8()?;
    if version >> 4 == 2 {
        return parse_legacy_major2(
            bytes,
            range,
            archive,
            writer_version,
            userdata,
            version,
            reader,
        );
    }
    if version >> 4 != 3 {
        return Err(GeometryError::unsupported(
            version_offset,
            "unsupported ON_Brep major",
        ));
    }
    let minor = version & 0x0f;
    let mut warnings = Vec::new();
    let mut losses = Vec::new();
    let c2 = read_children(
        bytes,
        &mut reader,
        archive,
        RawBrepBaseType::Curve,
        0,
        &mut warnings,
    )?;
    let c3 = read_children(
        bytes,
        &mut reader,
        archive,
        RawBrepBaseType::Curve,
        0,
        &mut warnings,
    )?;
    let surfaces = read_children(
        bytes,
        &mut reader,
        archive,
        RawBrepBaseType::Surface,
        0,
        &mut warnings,
    )?;
    let (vertices, vertex_array_range) = read_vertices(bytes, &mut reader, archive, &mut warnings)?;
    let (edges, edge_array_range) = read_edges(
        bytes,
        &mut reader,
        archive,
        writer_version,
        &mut warnings,
        &mut losses,
    )?;
    let (trims, trim_array_range) = read_trims(
        bytes,
        &mut reader,
        archive,
        writer_version,
        &mut warnings,
        &mut losses,
    )?;
    let (loops, loop_array_range) = read_loops(bytes, &mut reader, archive, &mut warnings)?;
    let (faces, face_array_range) = read_faces(bytes, &mut reader, archive, &mut warnings)?;
    let bounds = bbox(&mut reader)?;
    let (render_meshes, render_mesh_array_range, analysis_meshes, analysis_mesh_array_range) =
        if minor >= 1 {
            let (render, render_range) =
                read_mesh_sides(bytes, &mut reader, archive, faces.len(), &mut warnings)?;
            let (analysis, analysis_range) =
                read_mesh_sides(bytes, &mut reader, archive, faces.len(), &mut warnings)?;
            (render, render_range, analysis, analysis_range)
        } else {
            (Vec::new(), 0..0, Vec::new(), 0..0)
        };
    let is_solid = if minor >= 2 {
        let value = reader.i32()?;
        if (0..=2).contains(&value) {
            Some(value)
        } else {
            warnings.push(format!(
                "invalid Brep is_solid value {value}; retained for native fidelity"
            ));
            Some(value)
        }
    } else {
        None
    };
    let (mut face_sides, mut regions, mut region_wrapper_range, inline_region_loaded) =
        if minor >= 3 {
            read_regions(bytes, &mut reader, archive, faces.len(), &mut warnings)?
        } else {
            (Vec::new(), Vec::new(), None, false)
        };
    if !inline_region_loaded {
        if let Some(extra) = userdata.iter().find(|value| {
            value.class_uuid() == V5_BREP_REGION_TOPOLOGY_USERDATA
                && value.item_uuid() == V5_BREP_REGION_TOPOLOGY_USERDATA
                && (value.application_uuid().is_none()
                    || value.application_uuid() == Some(OPENNURBS4))
        }) {
            match read_region_topology_userdata(bytes, extra, archive, faces.len(), &mut warnings) {
                Ok((sides, topology_regions, range, _)) => {
                    face_sides = sides;
                    regions = topology_regions;
                    region_wrapper_range = range;
                }
                Err(error) => warnings.push(format!(
                    "invalid optional Brep region topology discarded: {error}"
                )),
            }
        }
    }
    let skipped = reader.skip_remaining()?;
    if skipped != 0 {
        warnings.push(format!("ON_Brep skipped {skipped} trailing bytes"));
    }
    let raw = RawBrep {
        losses,
        minor,
        c2,
        c3,
        surfaces,
        vertices,
        edges,
        trims,
        loops,
        faces,
        bounds,
        render_meshes,
        analysis_meshes,
        render_mesh_array_range,
        analysis_mesh_array_range,
        is_solid,
        face_sides,
        regions,
        region_wrapper_range,
        source_range: range,
        vertex_array_range,
        edge_array_range,
        trim_array_range,
        loop_array_range,
        face_array_range,
    };
    match ValidatedRawBrep::try_new(raw.clone()) {
        Ok(mut validated) => {
            validated.warnings.splice(0..0, warnings);
            Ok(BrepParse::Valid(validated))
        }
        Err(error) => Ok(BrepParse::SemanticInvalid {
            raw,
            error,
            warnings,
        }),
    }
}

#[derive(Debug, Clone)]
struct LegacyCurveMeta {
    range: Range<usize>,
    domain: Interval,
    endpoints: [Point3; 2],
}

fn parse_legacy_major2(
    bytes: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    _writer_version: Option<i64>,
    _userdata: &[UserdataDescriptor],
    version: u8,
    mut reader: BoundedReader<'_>,
) -> Result<BrepParse, GeometryError> {
    let minor = version & 0x0f;
    let face_count = count(&mut reader, MAX_BREP_ITEMS)?;
    let edge_count = count(&mut reader, MAX_BREP_ITEMS)?;
    let loop_count = count(&mut reader, MAX_BREP_ITEMS)?;
    let trim_count = count(&mut reader, MAX_BREP_ITEMS)?;
    if face_count == 0 || edge_count == 0 || loop_count == 0 || trim_count == 0 {
        return Err(error(
            reader.position(),
            "legacy Brep major-2 arrays must be nonempty",
        ));
    }
    let _outer_flag = reader.i32()?;
    let bounds = bbox(&mut reader)?;

    let c2_start = reader.position();
    let mut c2_slots = Vec::with_capacity(trim_count);
    let mut c2_meta = Vec::with_capacity(trim_count);
    for _ in 0..trim_count {
        let curve_range = crate::curves::consume_legacy_polycurve_2d(bytes, &mut reader, archive)?;
        let decoded = crate::curves::decode_2d(
            bytes,
            crate::curves::POLYCURVE,
            curve_range.clone(),
            archive,
        )?;
        let (domain, endpoints) = legacy_curve_shape(&decoded, curve_range.start)?;
        c2_meta.push(LegacyCurveMeta {
            range: curve_range.clone(),
            domain,
            endpoints,
        });
        c2_slots.push(Some(RawBrepChild {
            class_uuid: crate::curves::POLYCURVE,
            class_data_range: curve_range.clone(),
            source_range: curve_range,
            base_type: RawBrepBaseType::Curve,
        }));
    }
    let c2_range = c2_start..reader.position();

    let c3_start = reader.position();
    let mut c3_slots = Vec::with_capacity(edge_count);
    let mut c3_meta = Vec::with_capacity(edge_count);
    for _ in 0..edge_count {
        let curve_range =
            crate::curves::consume_legacy_polycurve(bytes, &mut reader, 1.0, archive)?;
        let decoded = crate::curves::decode(
            bytes,
            crate::curves::POLYCURVE,
            curve_range.clone(),
            1.0,
            archive,
        )?;
        let (domain, endpoints) = legacy_curve_shape(&decoded, curve_range.start)?;
        c3_meta.push(LegacyCurveMeta {
            range: curve_range.clone(),
            domain,
            endpoints,
        });
        c3_slots.push(Some(RawBrepChild {
            class_uuid: crate::curves::POLYCURVE,
            class_data_range: curve_range.clone(),
            source_range: curve_range,
            base_type: RawBrepBaseType::Curve,
        }));
    }
    let c3_range = c3_start..reader.position();

    let surfaces_start = reader.position();
    let mut surface_slots = Vec::with_capacity(face_count);
    for _ in 0..face_count {
        let start = reader.position();
        let _surface = crate::surfaces::read_nurbs_surface_prefix(&mut reader, 1.0)?;
        let surface_range = start..reader.position();
        surface_slots.push(Some(RawBrepChild {
            class_uuid: crate::surfaces::NURBS_SURFACE,
            class_data_range: surface_range.clone(),
            source_range: surface_range,
            base_type: RawBrepBaseType::Surface,
        }));
    }
    let surfaces_range = surfaces_start..reader.position();

    let mut loops = Vec::with_capacity(loop_count);
    let mut trims = Vec::with_capacity(trim_count);
    let mut faces = Vec::with_capacity(face_count);
    let mut warnings = Vec::new();
    for face_position in 0..face_count {
        let face_index = reader.i32()?;
        let _obsolete_material = reader.i32()?;
        let reversed_surface = reader.i32()?;
        let _face_type = reader.i32()?;
        let _face_bounds = bbox(&mut reader)?;
        let boundary_count = count(&mut reader, MAX_BREP_ITEMS)?;
        if boundary_count == 0 {
            return Err(error(
                reader.position(),
                "legacy Brep face has no boundary loops",
            ));
        }
        let mut face_loops = Vec::with_capacity(boundary_count);
        for _ in 0..boundary_count {
            let loop_source_start = reader.position();
            let loop_index = reader.i32()?;
            let boundary_type = reader.i32()?;
            let _loop_bounds = [reader.f64()?, reader.f64()?, reader.f64()?, reader.f64()?];
            let trim_in_loop = count(&mut reader, MAX_BREP_ITEMS)?;
            if trim_in_loop == 0 {
                return Err(error(reader.position(), "legacy Brep loop has no trims"));
            }
            let actual_loop_index = i32::try_from(loops.len())
                .map_err(|_| error(loop_source_start, "legacy Brep loop index overflow"))?;
            let loop_type = match boundary_type {
                -1 => 3,
                0 => 1,
                1 => 2,
                _ => 0,
            };
            let mut loop_trim_indexes = Vec::with_capacity(trim_in_loop);
            for _ in 0..trim_in_loop {
                let trim_source_start = reader.position();
                let stored_trim_index = reader.i32()?;
                let _twin_index = reader.i32()?;
                let has_edge = reader.u8()?;
                let edge_index = reader.i32()?;
                let reversed_3d = reader.i32()?;
                let _gcon = reader.i32()?;
                let _mono = reader.i32()?;
                let tolerance_3d = reader.f64()?;
                let tolerance_2d = reader.f64()?;
                let trim_index = i32::try_from(trims.len())
                    .map_err(|_| error(trim_source_start, "legacy Brep trim index overflow"))?;
                if stored_trim_index != trim_index {
                    warnings.push(format!(
                        "legacy Brep trim index {stored_trim_index} disagrees with array position {trim_index}"
                    ));
                }
                let edge = if edge_index >= 0 && (edge_index as usize) < edge_count {
                    edge_index
                } else {
                    if has_edge != 0 {
                        return Err(error(
                            trim_source_start,
                            "legacy Brep managed trim edge is out of range",
                        ));
                    }
                    -1
                };
                let curve = trim_index;
                let domain = c2_meta
                    .get(trim_index as usize)
                    .ok_or_else(|| {
                        error(trim_source_start, "legacy Brep C2 index is out of range")
                    })?
                    .domain;
                trims.push(RawBrepTrim {
                    index: trim_index,
                    curve,
                    proxy_domain: domain,
                    edge,
                    vertices: [-1, -1],
                    reversed_3d: i32::from(reversed_3d != 0),
                    trim_type: if edge < 0 { 4 } else { 0 },
                    iso: 0,
                    loop_index: actual_loop_index,
                    tolerances: [tolerance_2d, tolerance_2d],
                    domain,
                    proxy_reversed: 0,
                    reserved: Vec::new(),
                    legacy_tolerances: [tolerance_2d, tolerance_3d],
                    source_range: trim_source_start..reader.position(),
                });
                loop_trim_indexes.push(trim_index);
            }
            loops.push(RawBrepLoop {
                index: loop_index,
                trims: loop_trim_indexes,
                loop_type,
                face: i32::try_from(face_position)
                    .map_err(|_| error(loop_source_start, "legacy Brep face index overflow"))?,
                source_range: loop_source_start..reader.position(),
            });
            face_loops.push(actual_loop_index);
        }
        faces.push(RawBrepFace {
            index: face_index,
            loops: face_loops,
            surface: i32::try_from(face_position)
                .map_err(|_| error(reader.position(), "legacy Brep surface index overflow"))?,
            reversed_surface: i32::from(reversed_surface != 0),
            material_channel: 0,
            uuid: None,
            color: None,
            source_range: 0..0,
        });
    }
    if trims.len() != trim_count || loops.len() != loop_count {
        return Err(error(
            reader.position(),
            "legacy Brep topology counts do not match the header",
        ));
    }

    let edge_trim_indexes = (0..edge_count)
        .map(|edge_index| {
            trims
                .iter()
                .filter_map(|trim| (trim.edge == edge_index as i32).then_some(trim.index))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let endpoint_count = trim_count.checked_mul(2).ok_or_else(|| {
        error(
            reader.position(),
            "legacy Brep trim endpoint count overflow",
        )
    })?;
    let mut endpoint_parent = (0..endpoint_count).collect::<Vec<_>>();
    for loop_record in &loops {
        for pair in loop_record.trims.windows(2) {
            legacy_union(
                &mut endpoint_parent,
                legacy_trim_endpoint(pair[0], 1),
                legacy_trim_endpoint(pair[1], 0),
            );
        }
        let first = *loop_record
            .trims
            .first()
            .expect("legacy loops have at least one trim");
        let last = *loop_record
            .trims
            .last()
            .expect("legacy loops have at least one trim");
        legacy_union(
            &mut endpoint_parent,
            legacy_trim_endpoint(last, 1),
            legacy_trim_endpoint(first, 0),
        );
    }
    for (trim_index, trim) in trims.iter().enumerate() {
        if trim.edge < 0 {
            legacy_union(
                &mut endpoint_parent,
                legacy_trim_endpoint(trim_index as i32, 0),
                legacy_trim_endpoint(trim_index as i32, 1),
            );
        }
    }
    for trim_indexes in &edge_trim_indexes {
        let Some(first) = trim_indexes.first() else {
            continue;
        };
        for trim_index in trim_indexes.iter().skip(1) {
            for edge_endpoint in 0..2 {
                legacy_union(
                    &mut endpoint_parent,
                    legacy_trim_endpoint_for_edge(&trims[*first as usize], edge_endpoint),
                    legacy_trim_endpoint_for_edge(&trims[*trim_index as usize], edge_endpoint),
                );
            }
        }
    }
    let mut root_vertices = BTreeMap::new();
    let mut vertices = Vec::new();
    for endpoint in 0..endpoint_count {
        let root = legacy_find(&mut endpoint_parent, endpoint);
        if let Entry::Vacant(entry) = root_vertices.entry(root) {
            let index = i32::try_from(vertices.len())
                .map_err(|_| error(reader.position(), "legacy Brep vertex index overflow"))?;
            entry.insert(index);
            vertices.push(RawBrepVertex {
                index,
                point: Point3([0.0, 0.0, 0.0]),
                edges: Vec::new(),
                tolerance: 0.0,
                source_range: 0..0,
            });
        }
    }
    let mut edge_endpoints = Vec::with_capacity(c3_meta.len());
    let mut point_sums = Vec::new();
    point_sums.try_reserve_exact(vertices.len()).map_err(|_| {
        error(
            reader.position(),
            "legacy Brep vertex sum allocation failed",
        )
    })?;
    point_sums.resize(vertices.len(), [0.0; 3]);
    let mut point_counts = Vec::new();
    point_counts
        .try_reserve_exact(vertices.len())
        .map_err(|_| {
            error(
                reader.position(),
                "legacy Brep vertex count allocation failed",
            )
        })?;
    point_counts.resize(vertices.len(), 0_usize);
    for (edge_index, curve) in c3_meta.iter().enumerate() {
        let endpoints = if let Some(trim_index) = edge_trim_indexes[edge_index].first() {
            let trim = &trims[*trim_index as usize];
            let start_root =
                legacy_find(&mut endpoint_parent, legacy_trim_endpoint_for_edge(trim, 0));
            let end_root =
                legacy_find(&mut endpoint_parent, legacy_trim_endpoint_for_edge(trim, 1));
            [
                *root_vertices
                    .get(&start_root)
                    .expect("legacy edge start root has a vertex"),
                *root_vertices
                    .get(&end_root)
                    .expect("legacy edge end root has a vertex"),
            ]
        } else {
            let start = legacy_vertex(&mut vertices, curve.endpoints[0]);
            let end = legacy_vertex(&mut vertices, curve.endpoints[1]);
            if vertices.len() > point_sums.len() {
                let additional = vertices.len() - point_sums.len();
                point_sums.try_reserve_exact(additional).map_err(|_| {
                    error(
                        reader.position(),
                        "legacy Brep vertex sum allocation failed",
                    )
                })?;
                point_sums.resize(vertices.len(), [0.0; 3]);
                point_counts.try_reserve_exact(additional).map_err(|_| {
                    error(
                        reader.position(),
                        "legacy Brep vertex count allocation failed",
                    )
                })?;
                point_counts.resize(vertices.len(), 0);
            }
            [start, end]
        };
        for (vertex, point) in endpoints
            .into_iter()
            .zip([curve.endpoints[0], curve.endpoints[1]])
        {
            let index = vertex as usize;
            point_sums[index][0] += point.0[0];
            point_sums[index][1] += point.0[1];
            point_sums[index][2] += point.0[2];
            point_counts[index] += 1;
        }
        edge_endpoints.push(endpoints);
    }
    for (index, vertex) in vertices.iter_mut().enumerate() {
        if point_counts[index] != 0 {
            let count = point_counts[index] as f64;
            vertex.point = Point3([
                point_sums[index][0] / count,
                point_sums[index][1] / count,
                point_sums[index][2] / count,
            ]);
        }
    }
    let mut edges = Vec::with_capacity(edge_count);
    for (edge_index, curve) in c3_meta.iter().enumerate() {
        let edge_index_i32 = i32::try_from(edge_index)
            .map_err(|_| error(curve.range.start, "legacy Brep edge index overflow"))?;
        let trim_indexes = edge_trim_indexes[edge_index].clone();
        let tolerance = trim_indexes
            .iter()
            .map(|trim| trims[*trim as usize].legacy_tolerances[1])
            .filter(|value| value.is_finite() && *value >= 0.0)
            .fold(0.0, f64::max);
        edges.push(RawBrepEdge {
            index: edge_index_i32,
            curve: edge_index_i32,
            proxy_reversed: 0,
            proxy_domain: curve.domain,
            vertices: edge_endpoints[edge_index],
            trims: trim_indexes,
            tolerance,
            domain: curve.domain,
            source_range: 0..0,
        });
    }
    for trim in &mut trims {
        let trim_index = trim.index as usize;
        let start_root = legacy_find(&mut endpoint_parent, legacy_trim_endpoint(trim.index, 0));
        let end_root = legacy_find(&mut endpoint_parent, legacy_trim_endpoint(trim.index, 1));
        trim.vertices = [
            *root_vertices
                .get(&start_root)
                .expect("legacy trim start root has a vertex"),
            *root_vertices
                .get(&end_root)
                .expect("legacy trim end root has a vertex"),
        ];
        debug_assert_eq!(trim.index as usize, trim_index);
    }
    for edge in &edges {
        for vertex in edge.vertices {
            vertices[vertex as usize].edges.push(edge.index);
        }
    }
    for edge in &edges {
        for trim_index in &edge.trims {
            let loop_index = trims[*trim_index as usize].loop_index;
            let same_loop = edge
                .trims
                .iter()
                .filter(|other| trims[**other as usize].loop_index == loop_index)
                .count();
            trims[*trim_index as usize].trim_type = if edge.trims.len() == 1 {
                1
            } else if same_loop > 1 {
                3
            } else {
                2
            };
        }
    }
    for (vertex_index, vertex) in vertices.iter_mut().enumerate() {
        let mut tolerance: f64 = 0.0;
        for edge_index in &vertex.edges {
            let edge = &edges[*edge_index as usize];
            tolerance = tolerance.max(edge.tolerance);
            let endpoint = if edge.vertices[0] == vertex_index as i32 {
                0
            } else if edge.vertices[1] == vertex_index as i32 {
                1
            } else {
                continue;
            };
            let expected = c3_meta[edge.curve as usize].endpoints[endpoint];
            let delta = [
                vertex.point.0[0] - expected.0[0],
                vertex.point.0[1] - expected.0[1],
                vertex.point.0[2] - expected.0[2],
            ];
            tolerance = tolerance
                .max((delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt());
        }
        vertex.tolerance = tolerance;
    }

    let (render_meshes, render_mesh_array_range) =
        read_legacy_mesh_sides(bytes, &mut reader, archive, face_count, &mut warnings)?;
    let (analysis_meshes, analysis_mesh_array_range) = if minor >= 1 {
        read_legacy_mesh_sides(bytes, &mut reader, archive, face_count, &mut warnings)?
    } else {
        (Vec::new(), 0..0)
    };
    let skipped = reader.skip_remaining()?;
    if skipped != 0 {
        warnings.push(format!("legacy ON_Brep skipped {skipped} trailing bytes"));
    }
    let raw = RawBrep {
        losses: Vec::new(),
        minor,
        c2: RawBrepChildren {
            slots: c2_slots,
            source_range: c2_range,
            expected_type: RawBrepBaseType::Curve,
        },
        c3: RawBrepChildren {
            slots: c3_slots,
            source_range: c3_range,
            expected_type: RawBrepBaseType::Curve,
        },
        surfaces: RawBrepChildren {
            slots: surface_slots,
            source_range: surfaces_range,
            expected_type: RawBrepBaseType::Surface,
        },
        vertices,
        edges,
        trims,
        loops,
        faces,
        bounds,
        render_meshes,
        analysis_meshes,
        render_mesh_array_range,
        analysis_mesh_array_range,
        is_solid: None,
        face_sides: Vec::new(),
        regions: Vec::new(),
        region_wrapper_range: None,
        source_range: range,
        vertex_array_range: 0..0,
        edge_array_range: 0..0,
        trim_array_range: 0..0,
        loop_array_range: 0..0,
        face_array_range: 0..0,
    };
    match ValidatedRawBrep::try_new(raw.clone()) {
        Ok(mut validated) => {
            validated.warnings.splice(0..0, warnings);
            Ok(BrepParse::Valid(validated))
        }
        Err(error) => Ok(BrepParse::SemanticInvalid {
            raw,
            error,
            warnings,
        }),
    }
}

fn legacy_curve_shape(
    decoded: &crate::curves::DecodedGeometry,
    offset: usize,
) -> Result<(Interval, [Point3; 2]), GeometryError> {
    let crate::curves::DecodedGeometry::Curve { curve } = decoded else {
        return Err(error(offset, "legacy Brep polycurve is not a curve"));
    };
    let parameters = curve
        .compound_parameters()
        .filter(|parameters| parameters.len() >= 2)
        .ok_or_else(|| error(offset, "legacy Brep polycurve has no parameter range"))?;
    let endpoints = legacy_decoded_curve_endpoints(curve, offset)?;
    Ok((
        Interval([
            parameters[0],
            *parameters.last().expect("range has two values"),
        ]),
        endpoints,
    ))
}

fn legacy_decoded_curve_endpoints(
    curve: &crate::curves::DecodedCurve,
    offset: usize,
) -> Result<[Point3; 2], GeometryError> {
    if let crate::curves::DecodedCurve::Compound { children, .. } = curve {
        let first = children
            .first()
            .ok_or_else(|| error(offset, "legacy Brep polycurve has no first segment"))?;
        let last = children
            .last()
            .ok_or_else(|| error(offset, "legacy Brep polycurve has no last segment"))?;
        return Ok([
            legacy_decoded_curve_endpoints(&first.1, offset)?[0],
            legacy_decoded_curve_endpoints(&last.1, offset)?[1],
        ]);
    }
    match curve.leaf_geometry().expect("leaf curve") {
        CurveGeometry::Nurbs(nurbs) => {
            let first = nurbs
                .control_points()
                .first()
                .ok_or_else(|| error(offset, "legacy Brep curve has no first pole"))?;
            let last = nurbs
                .control_points()
                .last()
                .ok_or_else(|| error(offset, "legacy Brep curve has no last pole"))?;
            Ok([
                Point3([first.x, first.y, first.z]),
                Point3([last.x, last.y, last.z]),
            ])
        }
        CurveGeometry::Circle {
            center,
            ref_direction,
            radius,
            ..
        } => {
            let endpoint = Point3([
                center.x + ref_direction.x * radius,
                center.y + ref_direction.y * radius,
                center.z + ref_direction.z * radius,
            ]);
            Ok([endpoint, endpoint])
        }
        CurveGeometry::Degenerate { point } => {
            let point = Point3([point.x, point.y, point.z]);
            Ok([point, point])
        }
        _ => Err(error(offset, "legacy Brep curve has no finite endpoints")),
    }
}

fn legacy_trim_endpoint(trim_index: i32, endpoint: usize) -> usize {
    trim_index as usize * 2 + endpoint
}

fn legacy_trim_endpoint_for_edge(trim: &RawBrepTrim, edge_endpoint: usize) -> usize {
    let trim_endpoint = if trim.reversed_3d == 0 {
        edge_endpoint
    } else {
        1 - edge_endpoint
    };
    legacy_trim_endpoint(trim.index, trim_endpoint)
}

fn legacy_find(parent: &mut [usize], mut index: usize) -> usize {
    while parent[index] != index {
        parent[index] = parent[parent[index]];
        index = parent[index];
    }
    index
}

fn legacy_union(parent: &mut [usize], left: usize, right: usize) {
    let left_root = legacy_find(parent, left);
    let right_root = legacy_find(parent, right);
    if left_root != right_root {
        parent[right_root] = left_root;
    }
}

fn legacy_vertex(vertices: &mut Vec<RawBrepVertex>, point: Point3) -> i32 {
    if let Some((index, _)) = vertices
        .iter()
        .enumerate()
        .find(|(_, value)| value.point == point)
    {
        return index as i32;
    }
    let index = vertices.len();
    vertices.push(RawBrepVertex {
        index: index as i32,
        point,
        edges: Vec::new(),
        tolerance: 0.0,
        source_range: 0..0,
    });
    index as i32
}

fn read_legacy_mesh_sides(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    face_count: usize,
    warnings: &mut Vec<String>,
) -> Result<(Vec<RawBrepMeshSlot>, Range<usize>), GeometryError> {
    let start = reader.position();
    let mut slots = Vec::with_capacity(face_count);
    for _ in 0..face_count {
        let present = match reader.u8() {
            Ok(value) => value != 0,
            Err(error) => {
                reader.skip_remaining()?;
                warnings.push(format!("legacy Brep mesh cache degraded: {error}"));
                return Ok((empty_mesh_slots(face_count), start..reader.position()));
            }
        };
        let (mesh, userdata) = if present {
            let object_start = reader.position();
            let object = match chunk_at(bytes, object_start, reader.end(), archive, false) {
                Ok(object) => object,
                Err(error) => {
                    reader.skip_remaining()?;
                    warnings.push(format!("legacy Brep mesh cache degraded: {error}"));
                    return Ok((empty_mesh_slots(face_count), start..reader.position()));
                }
            };
            if let Err(error) = reader.skip(object.next_offset() - object_start) {
                reader.skip_remaining()?;
                warnings.push(format!("legacy Brep mesh cache degraded: {error}"));
                return Ok((empty_mesh_slots(face_count), start..reader.position()));
            }
            match parse_class_wrapper_with_userdata(
                bytes,
                chunk_start_range(&object),
                archive,
                warnings,
            ) {
                Ok((class, userdata)) if supported_mesh(class.class_uuid) => (
                    Some(RawBrepChild {
                        class_uuid: class.class_uuid,
                        class_data_range: class.class_data_range,
                        source_range: object_start..object.next_offset(),
                        base_type: RawBrepBaseType::Other,
                    }),
                    userdata,
                ),
                Ok(_) => {
                    warnings.push("legacy Brep mesh cache slot has wrong class".to_string());
                    (None, Vec::new())
                }
                Err(error) => {
                    warnings.push(format!("legacy Brep mesh cache slot degraded: {error}"));
                    (None, Vec::new())
                }
            }
        } else {
            (None, Vec::new())
        };
        slots.push(RawBrepMeshSlot {
            mesh,
            present,
            userdata,
        });
    }
    Ok((slots, start..reader.position()))
}

fn empty_mesh_slots(count: usize) -> Vec<RawBrepMeshSlot> {
    let mut slots = Vec::with_capacity(count);
    for _ in 0..count {
        slots.push(RawBrepMeshSlot {
            mesh: None,
            present: false,
            userdata: Vec::new(),
        });
    }
    slots
}

/// Returns whether a UUID is `ON_Brep`.
pub(crate) fn supported_class(uuid: Uuid) -> bool {
    matches!(
        uuid,
        ON_BREP | LEGACY_TRIMMED_SURFACE | LEGACY_BREP | TL_BREP
    )
}

fn read_children(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    expected_type: RawBrepBaseType,
    depth: usize,
    warnings: &mut Vec<String>,
) -> Result<RawBrepChildren, GeometryError> {
    if depth > MAX_BREP_DEPTH {
        return Err(error(
            reader.position(),
            "Brep child recursion limit exceeded",
        ));
    }
    let start = reader.position();
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child_reader = body_reader(bytes, &chunk)?;
    let version_offset = child_reader.position();
    let version = child_reader.u8()?;
    if version >> 4 != 1 {
        return Err(GeometryError::unsupported(
            version_offset,
            "unsupported Brep polymorphic-array version",
        ));
    }
    let count = count(&mut child_reader, MAX_BREP_ITEMS)?;
    let mut direct_ranges = Vec::with_capacity(count + 1);
    direct_ranges.push(version_offset..child_reader.position());
    let mut slots = Vec::with_capacity(count);
    for _ in 0..count {
        let presence_start = child_reader.position();
        let present = child_reader.i32()?;
        direct_ranges.push(presence_start..child_reader.position());
        match present {
            0 => slots.push(None),
            1 => {
                let child_start = child_reader.position();
                let child_chunk = chunk_at(bytes, child_start, child_reader.end(), archive, false)?;
                let child_end = child_chunk.next_offset();
                let class =
                    parse_class_wrapper(bytes, chunk_start_range(&child_chunk), archive, warnings)?;
                child_reader.skip(child_end - child_start)?;
                let base_type = classify_base_type(class.class_uuid);
                slots.push(Some(RawBrepChild {
                    class_uuid: class.class_uuid,
                    class_data_range: class.class_data_range,
                    source_range: child_start..child_end,
                    base_type,
                }));
            }
            _ => {
                return Err(error(
                    child_reader.position() - 4,
                    "invalid Brep slot presence",
                ))
            }
        }
    }
    finish_anonymous_ranges(
        bytes,
        reader,
        &chunk,
        child_reader,
        &direct_ranges,
        warnings,
    )?;
    Ok(RawBrepChildren {
        slots,
        source_range: start..chunk.next_offset(),
        expected_type,
    })
}

fn read_vertices(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<(Vec<RawBrepVertex>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let count = raw_array_start(&mut child, "vertex", 40)?;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let start = child.position();
        let index = child.i32()?;
        let point = point(&mut child)?;
        let edges = indexes(&mut child, "vertex edge")?;
        let tolerance = child.f64()?;
        result.push(RawBrepVertex {
            index,
            point,
            edges,
            tolerance,
            source_range: start..child.position(),
        });
    }
    let range = chunk.range();
    finish_anonymous(bytes, reader, &chunk, child, warnings)?;
    Ok((result, range))
}

/// Reports a topology array read under the pre-2002 layout for want of a stamp.
///
/// On a V3-or-later archive the stored domain is read only when the writer
/// stamp vouches for it; without a stamp the proxy domain is substituted and the
/// stored one is never read, so the emitted geometry rests on an assumption the
/// archive does not carry. An empty array carries no such reading.
///
/// This needs no agreement guard, unlike the body-kind and material charges.
/// The two readings consume different byte counts per record, so the stamped
/// reading shifts every record after the first and the two cannot coincide.
/// `raw_array_start` cannot rule that out either: its record width is a minimum
/// size check for the allocation, not the stride of the reading that follows.
fn unstamped_legacy_layout(
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    count: usize,
    field: &str,
) -> Option<cadmpeg_ir::report::LossNote> {
    (archive.value() >= 3 && writer_version.is_none() && count > 0).then(|| {
        crate::loss::writer_stamp_unverified(format!(
            "Brep {field} read with the pre-2002 layout for {count} records because the archive has no writer-version stamp"
        ))
    })
}

fn read_edges(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    warnings: &mut Vec<String>,
    losses: &mut Vec<cadmpeg_ir::report::LossNote>,
) -> Result<(Vec<RawBrepEdge>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let count = raw_array_start(&mut child, "edge", 44)?;
    let current = archive.value() >= 3 && writer_version.is_some_and(|v| v >= 200_206_180);
    if let Some(loss) = unstamped_legacy_layout(archive, writer_version, count, "edge domains") {
        losses.push(loss);
    }
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let start = child.position();
        let index = child.i32()?;
        let curve = child.i32()?;
        let proxy_reversed = child.i32()?;
        let proxy_domain = interval(&mut child)?;
        let vertices = [child.i32()?, child.i32()?];
        let trims = indexes(&mut child, "edge trim")?;
        let tolerance = child.f64()?;
        let domain = if current {
            interval(&mut child)?
        } else {
            proxy_domain
        };
        result.push(RawBrepEdge {
            index,
            curve,
            proxy_reversed,
            proxy_domain,
            vertices,
            trims,
            tolerance,
            domain,
            source_range: start..child.position(),
        });
    }
    let range = chunk.range();
    finish_anonymous(bytes, reader, &chunk, child, warnings)?;
    Ok((result, range))
}

fn read_trims(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    warnings: &mut Vec<String>,
    losses: &mut Vec<cadmpeg_ir::report::LossNote>,
) -> Result<(Vec<RawBrepTrim>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let count = raw_array_start(&mut child, "trim", 132)?;
    let current = archive.value() >= 3 && writer_version.is_some_and(|v| v >= 200_206_180);
    if let Some(loss) = unstamped_legacy_layout(
        archive,
        writer_version,
        count,
        "trim domains and proxy senses",
    ) {
        losses.push(loss);
    }
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let start = child.position();
        let index = child.i32()?;
        let curve = child.i32()?;
        let proxy_domain = interval(&mut child)?;
        let edge = child.i32()?;
        let vertices = [child.i32()?, child.i32()?];
        let reversed_3d = child.i32()?;
        let trim_type = child.i32()?;
        let iso = child.i32()?;
        let loop_index = child.i32()?;
        let tolerances = [child.f64()?, child.f64()?];
        let (domain, proxy_reversed, reserved) = if current {
            let domain = interval(&mut child)?;
            let proxy_reversed = child.u8()?;
            let reserved = child.take(31)?.to_vec();
            (domain, proxy_reversed, reserved)
        } else {
            child.skip(48)?;
            (proxy_domain, 0, Vec::new())
        };
        let legacy_tolerances = [child.f64()?, child.f64()?];
        result.push(RawBrepTrim {
            index,
            curve,
            proxy_domain,
            edge,
            vertices,
            reversed_3d,
            trim_type,
            iso,
            loop_index,
            tolerances,
            domain,
            proxy_reversed,
            reserved,
            legacy_tolerances,
            source_range: start..child.position(),
        });
    }
    let range = chunk.range();
    finish_anonymous(bytes, reader, &chunk, child, warnings)?;
    Ok((result, range))
}

fn read_loops(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<(Vec<RawBrepLoop>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let count = raw_array_start(&mut child, "loop", 20)?;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let start = child.position();
        let index = child.i32()?;
        let trims = indexes(&mut child, "loop trim")?;
        let loop_type = child.i32()?;
        let face = child.i32()?;
        result.push(RawBrepLoop {
            index,
            trims,
            loop_type,
            face,
            source_range: start..child.position(),
        });
    }
    let range = chunk.range();
    finish_anonymous(bytes, reader, &chunk, child, warnings)?;
    Ok((result, range))
}

fn read_faces(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<(Vec<RawBrepFace>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let version = child.u8()?;
    if version >> 4 != 1 || version & 0x0f > 2 {
        return Err(GeometryError::unsupported(
            child.position() - 1,
            "unsupported Brep face-array version",
        ));
    }
    let count = count(&mut child, MAX_BREP_ITEMS)?;
    if count
        .checked_mul(20)
        .is_none_or(|bytes| bytes > child.remaining())
    {
        return Err(error(
            child.position(),
            "face count exhausts payload before allocation",
        ));
    }
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let record_start = child.position();
        let index = child.i32()?;
        let loops = indexes(&mut child, "face loop")?;
        let surface = child.i32()?;
        let reversed_surface = child.i32()?;
        let material_channel = child.i32()?;
        result.push(RawBrepFace {
            index,
            loops,
            surface,
            reversed_surface,
            material_channel,
            uuid: None,
            color: None,
            source_range: record_start..child.position(),
        });
    }
    if version & 0x0f >= 1 {
        for face in &mut result {
            face.uuid = Some(uuid(&mut child)?);
        }
    }
    if version & 0x0f >= 2 {
        let present = child.u8()?;
        if present > 1 {
            return Err(error(child.position() - 1, "invalid face-color presence"));
        }
        if present != 0 {
            for face in &mut result {
                face.color = Some(child.take(4)?.try_into().expect("color width checked"));
            }
        }
    }
    let range = chunk.range();
    finish_anonymous(bytes, reader, &chunk, child, warnings)?;
    Ok((result, range))
}

fn read_mesh_sides(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    face_count: usize,
    warnings: &mut Vec<String>,
) -> Result<(Vec<RawBrepMeshSlot>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let parsed: Result<(Vec<RawBrepMeshSlot>, Range<usize>), GeometryError> = (|| {
        let mut result = Vec::with_capacity(face_count);
        let mut children = Vec::new();
        for _ in 0..face_count {
            let present = child.bool()?;
            let mesh = if present {
                let start = child.position();
                let object = chunk_at(bytes, start, child.end(), archive, false)?;
                children.push(object.range());
                let class = parse_class_wrapper_with_userdata(
                    bytes,
                    chunk_start_range(&object),
                    archive,
                    warnings,
                );
                child.skip(object.next_offset() - start)?;
                match class {
                    Ok((class, userdata)) if supported_mesh(class.class_uuid) => {
                        result.push(RawBrepMeshSlot {
                            mesh: Some(RawBrepChild {
                                class_uuid: class.class_uuid,
                                class_data_range: class.class_data_range,
                                source_range: start..object.next_offset(),
                                base_type: RawBrepBaseType::Other,
                            }),
                            present: true,
                            userdata,
                        });
                        continue;
                    }
                    Ok(_) => {
                        warnings.push("Brep mesh cache slot has wrong class".to_string());
                        None
                    }
                    Err(error) => {
                        warnings.push(format!("Brep mesh cache slot degraded: {error}"));
                        None
                    }
                }
            } else {
                None
            };
            result.push(RawBrepMeshSlot {
                mesh,
                present,
                userdata: Vec::new(),
            });
        }
        finish_anonymous_children(bytes, reader, &chunk, child, &children, warnings)?;
        Ok((result, chunk.range()))
    })();
    match parsed {
        Ok(result) => Ok(result),
        Err(error) => {
            reader.skip(chunk.next_offset() - reader.position())?;
            warnings.push(format!("Brep mesh cache degraded: {error}"));
            Ok((
                alloc_filled(
                    face_count,
                    RawBrepMeshSlot {
                        mesh: None,
                        present: false,
                        userdata: Vec::new(),
                    },
                    "Rhino Brep degraded mesh slots",
                )
                .map_err(|allocation| {
                    GeometryError::malformed(
                        chunk.range().start,
                        format!("Brep degraded mesh allocation refused: {allocation}"),
                    )
                })?,
                chunk.range(),
            ))
        }
    }
}

fn read_regions(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    face_count: usize,
    warnings: &mut Vec<String>,
) -> Result<RegionRead, GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut outer = body_reader(bytes, &chunk)?;
    let parsed = (|| {
        if outer.i32()? != 1 || outer.i32()? < 0 {
            return Err(GeometryError::unsupported(
                outer.position() - 8,
                "unsupported Brep region wrapper",
            ));
        }
        if !outer.bool()? {
            outer.skip_remaining()?;
            return Ok((Vec::new(), Vec::new(), None, false));
        }
        let nested_chunk = anonymous_chunk(bytes, &mut outer, archive)?;
        let mut topology = body_reader(bytes, &nested_chunk)?;
        let topology_major = topology.i32()?;
        let topology_minor = topology.i32()?;
        if topology_major != 1 || topology_minor < 0 {
            return Err(GeometryError::unsupported(
                topology.position() - 8,
                "unsupported Brep region-topology version",
            ));
        }
        let sides_start = topology.position();
        let sides = read_region_sides(bytes, &mut topology, archive, warnings)?;
        let sides_range = sides_start..topology.position();
        let regions_start = topology.position();
        let regions = read_region_records(bytes, &mut topology, archive, warnings)?;
        let regions_range = regions_start..topology.position();
        finish_anonymous_children(
            bytes,
            &mut outer,
            &nested_chunk,
            topology,
            &[sides_range, regions_range],
            warnings,
        )?;
        if sides.len() != face_count.saturating_mul(2) {
            return Err(error(
                outer.position(),
                "redundant Brep region face-side count mismatch",
            ));
        }
        outer.skip_remaining()?;
        Ok((sides, regions, Some(nested_chunk.range()), true))
    })();
    reader.skip(chunk.next_offset() - reader.position())?;
    match parsed {
        Ok((sides, regions, nested, inline_region_loaded)) => {
            let direct = crate::chunks::direct_checksum_ranges(&chunk.body(), nested.as_slice())?;
            if matches!(
                verify_checksum_ranges(bytes, &chunk, &direct)?,
                ChecksumStatus::Mismatch { .. }
            ) {
                warnings.push("Brep region wrapper checksum mismatch".to_string());
            }
            Ok((sides, regions, Some(chunk.range()), inline_region_loaded))
        }
        Err(error) => {
            warnings.push(format!(
                "invalid optional Brep region topology discarded: {error}"
            ));
            Ok((Vec::new(), Vec::new(), Some(chunk.range()), false))
        }
    }
}

fn read_region_topology_userdata(
    bytes: &[u8],
    extra: &UserdataDescriptor,
    archive: ArchiveVersion,
    face_count: usize,
    warnings: &mut Vec<String>,
) -> Result<RegionRead, GeometryError> {
    let mut parent = BoundedReader::new(
        bytes,
        extra.payload_range().start,
        extra.payload_range().end,
    )?;
    let topology_chunk = anonymous_chunk(bytes, &mut parent, archive)?;
    let mut topology = body_reader(bytes, &topology_chunk)?;
    let major = topology.i32()?;
    let minor = topology.i32()?;
    if major != 1 || minor < 0 {
        return Err(GeometryError::unsupported(
            topology.position() - 8,
            "unsupported Brep userdata region-topology version",
        ));
    }
    let sides_start = topology.position();
    let sides = read_region_sides(bytes, &mut topology, archive, warnings)?;
    let sides_range = sides_start..topology.position();
    let regions_start = topology.position();
    let regions = read_region_records(bytes, &mut topology, archive, warnings)?;
    let regions_range = regions_start..topology.position();
    finish_anonymous_children(
        bytes,
        &mut parent,
        &topology_chunk,
        topology,
        &[sides_range, regions_range],
        warnings,
    )?;
    let skipped = parent.skip_remaining()?;
    if skipped != 0 {
        warnings.push(format!(
            "Brep region-topology userdata skipped {skipped} trailing bytes"
        ));
    }
    if sides.len() != face_count.saturating_mul(2) {
        return Err(error(
            extra.range().start,
            "redundant Brep region face-side count mismatch",
        ));
    }
    Ok((sides, regions, Some(extra.range().clone()), true))
}

fn read_region_sides<'a>(
    bytes: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<Vec<RawBrepFaceSide>, GeometryError> {
    let (chunk, mut child, count) = region_array(bytes, reader, archive)?;
    let mut result = Vec::with_capacity(count);
    let mut children = Vec::with_capacity(count);
    for _ in 0..count {
        let (body, source) = region_element(bytes, &mut child, archive, ON_BREP_FACE_SIDE)?;
        children.push(source.clone());
        let mut child = BoundedReader::new(bytes, body.start, body.end)?;
        result.push(RawBrepFaceSide {
            index: child.i32()?,
            region: child.i32()?,
            face: child.i32()?,
            direction: child.i32()?,
            source_range: source,
        });
        child.skip_remaining()?;
    }
    finish_anonymous_children(bytes, reader, &chunk, child, &children, warnings)?;
    Ok(result)
}

fn read_region_records<'a>(
    bytes: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<Vec<RawBrepRegion>, GeometryError> {
    let (chunk, mut child, count) = region_array(bytes, reader, archive)?;
    let mut result = Vec::with_capacity(count);
    let mut children = Vec::with_capacity(count);
    for _ in 0..count {
        let (body, source) = region_element(bytes, &mut child, archive, ON_BREP_REGION)?;
        children.push(source.clone());
        let mut child = BoundedReader::new(bytes, body.start, body.end)?;
        let index = child.i32()?;
        let region_type = child.i32()?;
        let sides = indexes(&mut child, "region side")?;
        let bounds = bbox(&mut child)?;
        child.skip_remaining()?;
        result.push(RawBrepRegion {
            index,
            region_type,
            sides,
            bounds,
            source_range: source,
        });
    }
    finish_anonymous_children(bytes, reader, &chunk, child, &children, warnings)?;
    Ok(result)
}

fn region_array<'a>(
    bytes: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
) -> Result<(Chunk, BoundedReader<'a>, usize), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let count = anonymous_array_start(&mut child)?;
    Ok((chunk, child, count))
}

fn region_element(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    expected_class: Uuid,
) -> Result<(Range<usize>, Range<usize>), GeometryError> {
    let start = reader.position();
    if archive.value() < 60 {
        let chunk = crate::chunks::chunk_at(bytes, start, reader.end(), archive, false)?;
        reader.skip(chunk.next_offset() - start)?;
        let mut child = BoundedReader::new(bytes, chunk.body().start, chunk.body().end)?;
        let major = child.i32()?;
        let minor = child.i32()?;
        if major != 1 || minor < 0 {
            return Err(GeometryError::unsupported(
                start,
                "unsupported raw region element version",
            ));
        }
        Ok((
            child.position()..chunk.body().end,
            start..chunk.next_offset(),
        ))
    } else {
        let chunk = crate::chunks::chunk_at(bytes, start, reader.end(), archive, false)?;
        let class =
            parse_class_wrapper(bytes, chunk_start_range(&chunk), archive, &mut Vec::new())?;
        if class.class_uuid != expected_class {
            return Err(error(start, "unexpected Brep region element class"));
        }
        reader.skip(chunk.next_offset() - start)?;
        let mut class_data = BoundedReader::new(
            bytes,
            class.class_data_range.start,
            class.class_data_range.end,
        )?;
        let payload = anonymous_chunk(bytes, &mut class_data, archive)?;
        let mut body = body_reader(bytes, &payload)?;
        let major = body.i32()?;
        let minor = body.i32()?;
        if major != 1 || minor < 0 {
            return Err(GeometryError::unsupported(
                payload.body().start,
                "unsupported Brep region element version",
            ));
        }
        Ok((
            body.position()..payload.body().end,
            start..chunk.next_offset(),
        ))
    }
}

fn validate_rings(raw: &RawBrep) -> Result<(), GeometryError> {
    for loop_record in &raw.loops {
        if loop_record.trims.is_empty() {
            return Err(error(loop_record.source_range.start, "loop ring is empty"));
        }
        if matches!(loop_record.loop_type, 4 | 5) {
            let trim = &raw.trims[loop_record.trims[0] as usize];
            let expected_trim_type = if loop_record.loop_type == 4 { 5 } else { 6 };
            if loop_record.trims.len() != 1 || trim.trim_type != expected_trim_type {
                return Err(error(
                    loop_record.source_range.start,
                    "procedural Brep loop must contain its matching single trim",
                ));
            }
            continue;
        }
        for pair in loop_record.trims.windows(2) {
            let left = &raw.trims[pair[0] as usize];
            let right = &raw.trims[pair[1] as usize];
            let left_end = left.vertices[1];
            let right_start = right.vertices[0];
            if left_end != right_start {
                return Err(GeometryError::malformed(
                    loop_record.source_range.start,
                    format!(
                        "loop ring is discontinuous between trims {} and {} ({} != {})",
                        pair[0], pair[1], left_end, right_start
                    ),
                ));
            }
        }
        let first = &raw.trims[loop_record.trims[0] as usize];
        let last = &raw.trims[*loop_record.trims.last().expect("nonempty") as usize];
        let first_start = first.vertices[0];
        let last_end = last.vertices[1];
        if first_start != last_end {
            return Err(error(
                loop_record.source_range.start,
                "loop ring is not closed",
            ));
        }
    }
    Ok(())
}

fn validate_regions(raw: &RawBrep) -> Result<(), GeometryError> {
    if raw.face_sides.len() != raw.faces.len().saturating_mul(2) {
        return Err(error(
            raw.source_range.start,
            "region side count is invalid",
        ));
    }
    let mut infinite = 0;
    for (index, side) in raw.face_sides.iter().enumerate() {
        if side.face < 0 || side.face as usize >= raw.faces.len() {
            return Err(error(
                side.source_range.start,
                "region face-side index is invalid",
            ));
        }
        let expected = if index % 2 == 0 { 1 } else { -1 };
        if side.direction != expected {
            return Err(error(
                side.source_range.start,
                "region side direction is invalid",
            ));
        }
        if side.face != (index / 2) as i32 {
            return Err(error(
                side.source_range.start,
                "region side face position is invalid",
            ));
        }
        if side.region < -1 || side.region as usize >= raw.regions.len() {
            return Err(error(
                side.source_range.start,
                "region membership is invalid",
            ));
        }
    }
    let mut listed_sides = BTreeSet::new();
    for (index, region) in raw.regions.iter().enumerate() {
        if !matches!(region.region_type, 0 | 1) {
            return Err(error(region.source_range.start, "region record is invalid"));
        }
        if region.region_type == 0 {
            infinite += 1;
        }
        for side in &region.sides {
            refs(&[*side], raw.face_sides.len(), "region side")?;
            if !listed_sides.insert(*side) || raw.face_sides[*side as usize].region != index as i32
            {
                return Err(error(
                    region.source_range.start,
                    "region membership is not reciprocal",
                ));
            }
        }
    }
    if raw
        .face_sides
        .iter()
        .enumerate()
        .any(|(index, side)| side.region >= 0 && !listed_sides.contains(&(index as i32)))
    {
        return Err(error(
            raw.source_range.start,
            "region membership is not reciprocal",
        ));
    }
    if infinite != 1 {
        return Err(error(
            raw.source_range.start,
            "region topology needs one infinite region",
        ));
    }
    Ok(())
}

fn raw_array_start(
    reader: &mut BoundedReader<'_>,
    label: &str,
    minimum_record_bytes: usize,
) -> Result<usize, GeometryError> {
    let version = reader.u8()?;
    if version >> 4 != 1 {
        return Err(GeometryError::unsupported(
            reader.position() - 1,
            format!("unsupported {label} array version"),
        ));
    }
    let count = count(reader, MAX_BREP_ITEMS)?;
    if count
        .checked_mul(minimum_record_bytes)
        .is_none_or(|bytes| bytes > reader.remaining())
    {
        return Err(error(
            reader.position(),
            &format!("{label} count exhausts payload before allocation"),
        ));
    }
    Ok(count)
}

fn anonymous_array_start(reader: &mut BoundedReader<'_>) -> Result<usize, GeometryError> {
    let major = reader.i32()?;
    let minor = reader.i32()?;
    if major != 1 || minor < 0 {
        return Err(GeometryError::unsupported(
            reader.position() - 8,
            "unsupported region array version",
        ));
    }
    count(reader, MAX_BREP_ITEMS)
}

fn indexes(reader: &mut BoundedReader<'_>, label: &str) -> Result<Vec<i32>, GeometryError> {
    let count = count(reader, MAX_BREP_ITEMS)?;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        result.push(reader.i32()?);
    }
    let _ = label;
    Ok(result)
}

fn count(reader: &mut BoundedReader<'_>, cap: usize) -> Result<usize, GeometryError> {
    let value = reader.i32()?;
    if value < 0 {
        return Err(error(reader.position() - 4, "Brep count exceeds cap"));
    }
    let count = usize::try_from(value).map_err(|_| error(reader.position(), "count overflow"))?;
    if count > cap {
        return Err(error(reader.position() - 4, "Brep count exceeds cap"));
    }
    let minimum = count
        .checked_mul(4)
        .ok_or_else(|| error(reader.position(), "count overflow"))?;
    if minimum > reader.remaining() {
        return Err(error(reader.position(), "Brep count exhausts payload"));
    }
    Ok(count)
}

fn refs(values: &[i32], len: usize, label: &str) -> Result<(), GeometryError> {
    if values
        .iter()
        .any(|value| *value < 0 || (*value as usize) >= len)
    {
        return Err(error(0, &format!("{label} reference is out of range")));
    }
    Ok(())
}

fn typed_slot(array: &RawBrepChildren, index: i32, expected: RawBrepBaseType) -> bool {
    index >= 0
        && array
            .slots
            .get(index as usize)
            .and_then(Option::as_ref)
            .is_some_and(|child| child.base_type == expected)
}

fn validate_edge_incidences(raw: &RawBrep) -> Result<(), GeometryError> {
    let mut actual = alloc_filled(
        raw.vertices.len(),
        Vec::<i32>::new(),
        "Rhino Brep vertex edge incidences",
    )
    .map_err(|error| {
        GeometryError::malformed(0, format!("Brep incidence allocation refused: {error}"))
    })?;
    for (vertex, record) in raw.vertices.iter().enumerate() {
        for edge in &record.edges {
            actual[vertex].push(*edge);
        }
    }
    for (edge_index, edge) in raw.edges.iter().enumerate() {
        for trim_index in &edge.trims {
            let trim = &raw.trims[*trim_index as usize];
            if trim.edge >= 0
                && !((trim.vertices[0] == edge.vertices[0] && trim.vertices[1] == edge.vertices[1])
                    || (trim.vertices[0] == edge.vertices[1]
                        && trim.vertices[1] == edge.vertices[0]))
            {
                return Err(error(
                    edge.source_range.start,
                    "edge/trim endpoint incidence mismatch",
                ));
            }
        }
        for (endpoint, vertex) in edge.vertices.iter().enumerate() {
            let expected = if edge.vertices[0] == edge.vertices[1] {
                2
            } else {
                1
            };
            let count = actual[*vertex as usize]
                .iter()
                .filter(|value| **value == edge_index as i32)
                .count();
            if count != expected {
                return Err(GeometryError::malformed(
                    edge.source_range.start,
                    if edge.vertices[0] == edge.vertices[1] && endpoint == 1 {
                        format!("closed edge incidence is duplicated incorrectly for edge {edge_index} ({},{}): expected {expected}, got {count}", edge.vertices[0], edge.vertices[1])
                    } else {
                        format!("edge/vertex incidence mismatch for edge {edge_index} ({},{}), vertex {vertex}: expected {expected}, got {count}", edge.vertices[0], edge.vertices[1])
                    },
                ));
            }
        }
    }
    Ok(())
}

fn unique(values: &[i32], label: &str) -> Result<(), GeometryError> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(*value) {
            return Err(error(0, &format!("{label} reference is duplicated")));
        }
    }
    Ok(())
}

fn finite_interval(value: Interval, label: &str) -> Result<(), GeometryError> {
    let [low, high] = value.0;
    let unset = (low == ON_UNSET_VALUE && high == ON_UNSET_VALUE)
        || (low == ON_UNSET_POSITIVE_VALUE && high == ON_UNSET_POSITIVE_VALUE);
    let empty = (low == ON_UNSET_VALUE && high == ON_UNSET_POSITIVE_VALUE)
        || (low == ON_UNSET_POSITIVE_VALUE && high == ON_UNSET_VALUE);
    if !(unset || empty || low.is_finite() && high.is_finite() && low < high) {
        return Err(error(0, &format!("{label} is invalid")));
    }
    Ok(())
}

fn finite_tolerance(value: f64, label: &str) -> Result<(), GeometryError> {
    if !(value == ON_UNSET_VALUE
        || value == ON_UNSET_POSITIVE_VALUE
        || value.is_finite() && value >= 0.0)
    {
        return Err(error(0, &format!("{label} is invalid")));
    }
    Ok(())
}

fn point(reader: &mut BoundedReader<'_>) -> Result<Point3, GeometryError> {
    let point = Point3([reader.f64()?, reader.f64()?, reader.f64()?]);
    if point.0.iter().any(|value| !value.is_finite()) {
        return Err(error(reader.position() - 24, "Brep point is not finite"));
    }
    Ok(point)
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, GeometryError> {
    Ok(Uuid::from_wire(
        reader.take(16)?.try_into().expect("UUID width checked"),
    ))
}

fn supported_mesh(uuid: Uuid) -> bool {
    uuid == crate::mesh::ON_MESH
}

fn chunk_start_range(chunk: &crate::chunks::Chunk) -> Range<usize> {
    chunk.range()
}

fn anonymous_chunk(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Chunk, GeometryError> {
    let chunk = chunk_at(bytes, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(error(
            chunk.header_start,
            "expected bounded anonymous Brep chunk",
        ));
    }
    Ok(chunk)
}

fn body_reader<'a>(bytes: &'a [u8], chunk: &Chunk) -> Result<BoundedReader<'a>, GeometryError> {
    Ok(BoundedReader::new(
        bytes,
        chunk.body().start,
        chunk.body().end,
    )?)
}

fn finish_anonymous(
    bytes: &[u8],
    parent: &mut BoundedReader<'_>,
    chunk: &Chunk,
    child: BoundedReader<'_>,
    warnings: &mut Vec<String>,
) -> Result<(), GeometryError> {
    if child.remaining() != 0 {
        warnings.push(format!(
            "Brep anonymous chunk skipped {} trailing bytes",
            child.remaining()
        ));
    }
    if matches!(
        verify_checksum(bytes, chunk)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push(format!(
            "Brep anonymous CRC mismatch at offset {}",
            chunk.header_start
        ));
    }
    parent.skip(chunk.next_offset() - parent.position())?;
    Ok(())
}

fn finish_anonymous_children(
    bytes: &[u8],
    parent: &mut BoundedReader<'_>,
    chunk: &Chunk,
    child: BoundedReader<'_>,
    children: &[Range<usize>],
    warnings: &mut Vec<String>,
) -> Result<(), GeometryError> {
    let direct = crate::chunks::direct_checksum_ranges(&chunk.body(), children)?;
    finish_anonymous_ranges(bytes, parent, chunk, child, &direct, warnings)
}

fn finish_anonymous_ranges(
    bytes: &[u8],
    parent: &mut BoundedReader<'_>,
    chunk: &Chunk,
    child: BoundedReader<'_>,
    direct_ranges: &[Range<usize>],
    warnings: &mut Vec<String>,
) -> Result<(), GeometryError> {
    if child.remaining() != 0 {
        warnings.push(format!(
            "Brep anonymous chunk skipped {} trailing bytes",
            child.remaining()
        ));
    }
    if matches!(
        verify_checksum_ranges(bytes, chunk, direct_ranges)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push(format!(
            "Brep anonymous CRC mismatch at offset {}",
            chunk.header_start
        ));
    }
    parent.skip(chunk.next_offset() - parent.position())?;
    Ok(())
}

fn classify_base_type(uuid: Uuid) -> RawBrepBaseType {
    if crate::curves::curve_class(uuid) {
        RawBrepBaseType::Curve
    } else if crate::curves::surface_class(uuid) {
        RawBrepBaseType::Surface
    } else {
        RawBrepBaseType::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_brep_aliases_share_the_brep_payload_reader() {
        assert!(supported_class(ON_BREP));
        assert!(supported_class(LEGACY_TRIMMED_SURFACE));
        assert!(supported_class(LEGACY_BREP));
        assert!(supported_class(TL_BREP));
    }

    fn anonymous(body: &[u8]) -> Vec<u8> {
        let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&i64::try_from(body.len() + 4).expect("length").to_le_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(&crc32fast::hash(body).to_le_bytes());
        bytes
    }

    fn anonymous_mixed(parts: &[(&[u8], bool)]) -> Vec<u8> {
        let body = parts
            .iter()
            .flat_map(|(bytes, _)| bytes.iter().copied())
            .collect::<Vec<_>>();
        let mut checksum = crc32fast::Hasher::new();
        for (bytes, nested) in parts {
            if !nested {
                checksum.update(bytes);
            }
        }
        let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&i64::try_from(body.len() + 4).expect("length").to_le_bytes());
        bytes.extend_from_slice(&body);
        bytes.extend_from_slice(&checksum.finalize().to_le_bytes());
        bytes
    }

    fn packed_array(count: i32, records: &[u8]) -> Vec<u8> {
        let mut body = vec![0x10];
        body.extend_from_slice(&count.to_le_bytes());
        body.extend_from_slice(records);
        anonymous(&body)
    }

    fn region_face_side(index: i32, region: i32, face: i32, direction: i32) -> Vec<u8> {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(0_i32.to_le_bytes());
        body.extend(index.to_le_bytes());
        body.extend(region.to_le_bytes());
        body.extend(face.to_le_bytes());
        body.extend(direction.to_le_bytes());
        anonymous(&body)
    }

    fn region_record(index: i32, region_type: i32, sides: &[i32], bounds: [f64; 6]) -> Vec<u8> {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(0_i32.to_le_bytes());
        body.extend(index.to_le_bytes());
        body.extend(region_type.to_le_bytes());
        body.extend((sides.len() as i32).to_le_bytes());
        body.extend(sides.iter().flat_map(|value| value.to_le_bytes()));
        body.extend(bounds.into_iter().flat_map(f64::to_le_bytes));
        anonymous(&body)
    }

    fn region_array(entries: &[u8], count: i32) -> Vec<u8> {
        let mut header = 1_i32.to_le_bytes().to_vec();
        header.extend(0_i32.to_le_bytes());
        header.extend(count.to_le_bytes());
        anonymous_mixed(&[(&header, false), (entries, true)])
    }

    fn region_topology_userdata_payload() -> Vec<u8> {
        let sides = [region_face_side(0, 0, 0, 1), region_face_side(1, 0, 0, -1)].concat();
        let side_array = region_array(&sides, 2);
        let region = region_record(0, 0, &[0, 1], [-1.0, -1.0, 0.0, 2.0, 2.0, 1.0]);
        let region_array = region_array(&region, 1);
        let mut header = 1_i32.to_le_bytes().to_vec();
        header.extend(0_i32.to_le_bytes());
        anonymous_mixed(&[(&header, false), (&side_array, true), (&region_array, true)])
    }

    fn region_topology_v6_payload() -> Vec<u8> {
        let sides = [
            class_wrapper_for(ON_BREP_FACE_SIDE, &region_face_side(0, 0, 0, 1)),
            class_wrapper_for(ON_BREP_FACE_SIDE, &region_face_side(1, 0, 0, -1)),
        ]
        .concat();
        let side_array = region_array(&sides, 2);
        let region = class_wrapper_for(
            ON_BREP_REGION,
            &region_record(0, 0, &[0, 1], [-1.0, -1.0, 0.0, 2.0, 2.0, 1.0]),
        );
        let region_array = region_array(&region, 1);
        let mut header = 1_i32.to_le_bytes().to_vec();
        header.extend(0_i32.to_le_bytes());
        anonymous_mixed(&[(&header, false), (&side_array, true), (&region_array, true)])
    }

    fn region_topology_userdata_descriptor(range: Range<usize>) -> UserdataDescriptor {
        UserdataDescriptor::Known {
            range: range.clone(),
            version: (2, 2),
            class_uuid: V5_BREP_REGION_TOPOLOGY_USERDATA,
            item_uuid: V5_BREP_REGION_TOPOLOGY_USERDATA,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: Some(OPENNURBS4),
            last_saved_as_goo: None,
            archive_version: None,
            writer_version: None,
            payload_range: range,
        }
    }

    #[test]
    fn v5_region_topology_userdata_decodes_the_v5_array_grammar() {
        let payload = region_topology_userdata_payload();
        let descriptor = region_topology_userdata_descriptor(0..payload.len());
        let mut warnings = Vec::new();
        let (sides, regions, source_range, loaded) = read_region_topology_userdata(
            &payload,
            &descriptor,
            ArchiveVersion::V5,
            1,
            &mut warnings,
        )
        .expect("V5 region topology userdata");

        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        assert!(loaded);
        assert_eq!(source_range, Some(0..payload.len()));
        assert_eq!(sides.len(), 2);
        assert_eq!(sides[0].index, 0);
        assert_eq!(sides[0].region, 0);
        assert_eq!(sides[0].face, 0);
        assert_eq!(sides[0].direction, 1);
        assert_eq!(sides[1].direction, -1);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].index, 0);
        assert_eq!(regions[0].region_type, 0);
        assert_eq!(regions[0].sides, vec![0, 1]);
        assert_eq!(regions[0].bounds.minimum, Point3([-1.0, -1.0, 0.0]));
        assert_eq!(regions[0].bounds.maximum, Point3([2.0, 2.0, 1.0]));
    }

    #[test]
    fn v6_region_topology_arrays_unwrap_polymorphic_records() {
        let payload = region_topology_v6_payload();
        let descriptor = region_topology_userdata_descriptor(0..payload.len());
        let mut warnings = Vec::new();
        let (sides, regions, _, loaded) = read_region_topology_userdata(
            &payload,
            &descriptor,
            ArchiveVersion::V6,
            1,
            &mut warnings,
        )
        .expect("V6 region topology userdata");

        assert!(loaded);
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        assert_eq!(
            sides.iter().map(|side| side.direction).collect::<Vec<_>>(),
            [1, -1]
        );
        assert_eq!(regions[0].sides, vec![0, 1]);
    }

    fn class_wrapper(data: &[u8]) -> Vec<u8> {
        class_wrapper_for(Uuid::from_canonical([9; 16]), data)
    }

    fn class_wrapper_for(class_uuid: Uuid, data: &[u8]) -> Vec<u8> {
        let mut uuid = 0x0002_fffb_u32.to_le_bytes().to_vec();
        uuid.extend_from_slice(&20_i64.to_le_bytes());
        uuid.extend(class_uuid.to_wire());
        uuid.extend_from_slice(&crc32fast::hash(&class_uuid.to_wire()).to_le_bytes());
        let mut class_data = 0x0002_fffc_u32.to_le_bytes().to_vec();
        class_data.extend_from_slice(&i64::try_from(data.len() + 4).expect("length").to_le_bytes());
        class_data.extend_from_slice(data);
        class_data.extend_from_slice(&crc32fast::hash(data).to_le_bytes());
        let mut end = 0x8002_7fff_u32.to_le_bytes().to_vec();
        end.extend_from_slice(&0_i64.to_le_bytes());
        let mut body = uuid;
        body.extend(class_data);
        body.extend(end);
        let mut wrapper = 0x0002_7ffa_u32.to_le_bytes().to_vec();
        wrapper.extend_from_slice(&i64::try_from(body.len()).expect("length").to_le_bytes());
        wrapper.extend(body);
        wrapper
    }

    fn long_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
        let mut bytes = typecode.to_le_bytes().to_vec();
        bytes.extend_from_slice(&i64::try_from(body.len() + 4).expect("length").to_le_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(&crc32fast::hash(body).to_le_bytes());
        bytes
    }

    fn mesh_class_wrapper_with_userdata() -> Vec<u8> {
        let class_uuid = crate::mesh::ON_MESH;
        let item_uuid = crate::mesh::V5_MESH_DOUBLE_VERTICES;
        let uuid = long_chunk(0x0002_fffb, &class_uuid.to_wire());
        let class_data = long_chunk(0x0002_fffc, &[]);

        let mut header_body = class_uuid.to_wire().to_vec();
        header_body.extend(item_uuid.to_wire());
        header_body.extend(1_i32.to_le_bytes());
        header_body.extend([0_u8; 16 * 8]);
        header_body.extend(Uuid::nil().to_wire());
        header_body.push(0);
        header_body.extend(50_i32.to_le_bytes());
        header_body.extend(202_400_i32.to_le_bytes());
        let header = long_chunk(0x0002_fff9, &header_body);
        let payload = anonymous(&[0]);
        let mut userdata_body = vec![0x22];
        userdata_body.extend(header);
        userdata_body.extend(payload);
        let mut userdata = 0x0002_7ffd_u32.to_le_bytes().to_vec();
        userdata.extend_from_slice(
            &i64::try_from(userdata_body.len() + 4)
                .expect("length")
                .to_le_bytes(),
        );
        userdata.extend(userdata_body);
        userdata.extend_from_slice(&crc32fast::hash(&[0x22]).to_le_bytes());

        let mut end = 0x8002_7fff_u32.to_le_bytes().to_vec();
        end.extend_from_slice(&0_i64.to_le_bytes());
        let mut body = uuid;
        body.extend(class_data);
        body.extend(userdata);
        body.extend(end);
        let mut wrapper = 0x0002_7ffa_u32.to_le_bytes().to_vec();
        wrapper.extend_from_slice(&i64::try_from(body.len()).expect("length").to_le_bytes());
        wrapper.extend(body);
        wrapper
    }

    fn interval_bytes() -> Vec<u8> {
        [0.0_f64, 1.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect()
    }

    fn trim_record(current: bool) -> Vec<u8> {
        let mut record = Vec::new();
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend(interval_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&1_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&1_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend([0.0_f64, 0.0].into_iter().flat_map(f64::to_le_bytes));
        if current {
            record.extend(interval_bytes());
            record.push(0);
            record.extend([0; 31]);
        } else {
            record.extend([0_u8; 48]);
        }
        record.extend([0.0_f64, 0.0].into_iter().flat_map(f64::to_le_bytes));
        record
    }

    fn raw_child(base_type: RawBrepBaseType) -> RawBrepChild {
        RawBrepChild {
            class_uuid: Uuid::nil(),
            class_data_range: 0..0,
            source_range: 0..0,
            base_type,
        }
    }

    fn one_face_raw() -> RawBrep {
        let interval = Interval([0.0, 1.0]);
        let vertices = [[0, 2], [0, 1], [1, 2]]
            .into_iter()
            .enumerate()
            .map(|(index, edges)| RawBrepVertex {
                index: i32::try_from(index).expect("index"),
                point: Point3([
                    f64::from((index == 1) as u8),
                    f64::from((index == 2) as u8),
                    0.0,
                ]),
                edges: edges.into_iter().collect(),
                tolerance: 0.0,
                source_range: 0..0,
            })
            .collect();
        let endpoints = [[0, 1], [1, 2], [2, 0]];
        let edges = endpoints
            .into_iter()
            .enumerate()
            .map(|(index, vertices)| RawBrepEdge {
                index: i32::try_from(index).expect("index"),
                curve: 0,
                proxy_reversed: 0,
                proxy_domain: interval,
                vertices,
                trims: vec![i32::try_from(index).expect("index")],
                tolerance: 0.0,
                domain: interval,
                source_range: 0..0,
            })
            .collect();
        let trims = endpoints
            .into_iter()
            .enumerate()
            .map(|(index, vertices)| RawBrepTrim {
                index: i32::try_from(index).expect("index"),
                curve: 0,
                proxy_domain: interval,
                edge: i32::try_from(index).expect("index"),
                vertices,
                reversed_3d: 0,
                trim_type: 1,
                iso: 0,
                loop_index: 0,
                tolerances: [0.0, 0.0],
                domain: interval,
                proxy_reversed: 0,
                reserved: Vec::new(),
                legacy_tolerances: [0.0, 0.0],
                source_range: 0..0,
            })
            .collect();
        RawBrep {
            losses: Vec::new(),
            minor: 0,
            c2: RawBrepChildren {
                slots: vec![Some(raw_child(RawBrepBaseType::Curve))],
                source_range: 0..0,
                expected_type: RawBrepBaseType::Curve,
            },
            c3: RawBrepChildren {
                slots: vec![Some(raw_child(RawBrepBaseType::Curve))],
                source_range: 0..0,
                expected_type: RawBrepBaseType::Curve,
            },
            surfaces: RawBrepChildren {
                slots: vec![Some(raw_child(RawBrepBaseType::Surface))],
                source_range: 0..0,
                expected_type: RawBrepBaseType::Surface,
            },
            vertices,
            edges,
            trims,
            loops: vec![RawBrepLoop {
                index: 0,
                trims: vec![0, 1, 2],
                loop_type: 1,
                face: 0,
                source_range: 0..0,
            }],
            faces: vec![RawBrepFace {
                index: 0,
                loops: vec![0],
                surface: 0,
                reversed_surface: 0,
                material_channel: 0,
                uuid: None,
                color: None,
                source_range: 0..0,
            }],
            bounds: BoundingBox {
                minimum: Point3([0.0, 0.0, 0.0]),
                maximum: Point3([1.0, 1.0, 0.0]),
            },
            render_meshes: Vec::new(),
            analysis_meshes: Vec::new(),
            render_mesh_array_range: 0..0,
            analysis_mesh_array_range: 0..0,
            is_solid: None,
            face_sides: Vec::new(),
            regions: Vec::new(),
            region_wrapper_range: None,
            source_range: 0..0,
            vertex_array_range: 0..0,
            edge_array_range: 0..0,
            trim_array_range: 0..0,
            loop_array_range: 0..0,
            face_array_range: 0..0,
        }
    }

    #[test]
    fn serialized_solid_state_uses_valid_values_and_topology_fallback() {
        assert_eq!(
            serialized_body_kind(2, Some(1), Some(200_210_020), false),
            BrepBodyKind::Solid
        );
        assert_eq!(
            serialized_body_kind(2, Some(2), Some(200_210_020), false),
            BrepBodyKind::Solid
        );
        assert_eq!(
            serialized_body_kind(2, Some(3), Some(200_210_020), false),
            BrepBodyKind::Sheet
        );
        assert_eq!(
            serialized_body_kind(2, Some(3), Some(200_210_020), true),
            BrepBodyKind::Solid
        );
        assert_eq!(
            serialized_body_kind(2, Some(0), Some(200_210_020), true),
            BrepBodyKind::Solid
        );
        assert_eq!(
            serialized_body_kind(2, Some(0), Some(200_210_020), false),
            BrepBodyKind::Sheet
        );
        assert_eq!(
            serialized_body_kind(1, Some(1), Some(200_210_020), true),
            BrepBodyKind::Solid
        );
        assert_eq!(
            serialized_body_kind(2, Some(1), Some(200_210_019), false),
            BrepBodyKind::Sheet
        );
    }

    /// A missing stamp trusts the stored solid flag; the loss follows that reading.
    ///
    /// The same bytes classify as `Sheet` under any stamp older than the flag, so an
    /// unstamped archive that reads `Solid` was classified on an assumption it does
    /// not carry. Where the two readings agree nothing was substituted.
    #[test]
    fn body_kind_gauge_charges_only_when_a_missing_stamp_changes_the_kind() {
        assert_eq!(
            serialized_body_kind(2, Some(1), None, false),
            BrepBodyKind::Solid
        );
        assert!(body_kind_rests_on_missing_stamp(2, Some(1), None, false));
        assert!(body_kind_rests_on_missing_stamp(2, Some(2), None, false));

        // A modern stamp vouches for the same flag: the reading is verified.
        assert!(!body_kind_rests_on_missing_stamp(
            2,
            Some(1),
            Some(200_210_020),
            false
        ));
        // Both readings agree, so no kind was substituted.
        assert!(!body_kind_rests_on_missing_stamp(2, Some(1), None, true));
        assert!(!body_kind_rests_on_missing_stamp(2, Some(0), None, false));
        assert!(!body_kind_rests_on_missing_stamp(2, None, None, false));
        assert!(!body_kind_rests_on_missing_stamp(1, Some(1), None, false));

        // The whole-record path reports the substitution as a typed loss.
        let mut raw = one_face_raw();
        raw.minor = 2;
        raw.is_solid = Some(1);
        let validated = ValidatedRawBrep::try_new(raw).expect("valid Brep");
        let (kind, substituted) = validated.body_kind(None);
        assert_eq!(kind, BrepBodyKind::Solid);
        assert_eq!(
            substituted.as_ref().map(|loss| &loss.code),
            Some(&crate::loss::RhinoLossCode::TopologyBodyKindGaugeSubstituted.kind())
        );
        assert_eq!(validated.body_kind(Some(200_210_020)).1, None);
    }

    fn degenerate_trim_raw(trim_type: i32, curve: i32) -> RawBrep {
        let interval = Interval([0.0, 1.0]);
        RawBrep {
            losses: Vec::new(),
            minor: 0,
            c2: RawBrepChildren {
                slots: vec![Some(raw_child(RawBrepBaseType::Curve))],
                source_range: 0..0,
                expected_type: RawBrepBaseType::Curve,
            },
            c3: RawBrepChildren {
                slots: Vec::new(),
                source_range: 0..0,
                expected_type: RawBrepBaseType::Curve,
            },
            surfaces: RawBrepChildren {
                slots: vec![Some(raw_child(RawBrepBaseType::Surface))],
                source_range: 0..0,
                expected_type: RawBrepBaseType::Surface,
            },
            vertices: vec![RawBrepVertex {
                index: 0,
                point: Point3([0.0, 0.0, 0.0]),
                edges: Vec::new(),
                tolerance: 0.0,
                source_range: 0..0,
            }],
            edges: Vec::new(),
            trims: vec![RawBrepTrim {
                index: 0,
                curve,
                proxy_domain: interval,
                edge: -1,
                vertices: [0, 0],
                reversed_3d: 0,
                trim_type,
                iso: 0,
                loop_index: 0,
                tolerances: [0.0, 0.0],
                domain: interval,
                proxy_reversed: 0,
                reserved: Vec::new(),
                legacy_tolerances: [0.0, 0.0],
                source_range: 0..0,
            }],
            loops: vec![RawBrepLoop {
                index: 0,
                trims: vec![0],
                loop_type: 1,
                face: 0,
                source_range: 0..0,
            }],
            faces: vec![RawBrepFace {
                index: 0,
                loops: vec![0],
                surface: 0,
                reversed_surface: 0,
                material_channel: 0,
                uuid: None,
                color: None,
                source_range: 0..0,
            }],
            bounds: BoundingBox {
                minimum: Point3([0.0, 0.0, 0.0]),
                maximum: Point3([0.0, 0.0, 0.0]),
            },
            render_meshes: Vec::new(),
            analysis_meshes: Vec::new(),
            render_mesh_array_range: 0..0,
            analysis_mesh_array_range: 0..0,
            is_solid: None,
            face_sides: Vec::new(),
            regions: Vec::new(),
            region_wrapper_range: None,
            source_range: 0..0,
            vertex_array_range: 0..0,
            edge_array_range: 0..0,
            trim_array_range: 0..0,
            loop_array_range: 0..0,
            face_array_range: 0..0,
        }
    }

    #[test]
    fn legacy_brep_major_two_requires_its_payload() {
        let error = parse(&[0x20], 0..1, ArchiveVersion::V5, None, &[])
            .expect_err("truncated major two must fail");
        assert!(matches!(error, GeometryError::Malformed(_)));
    }

    #[test]
    fn legacy_curve_endpoints_cover_analytic_and_degenerate_children() {
        let circle = crate::curves::DecodedCurve::leaf(
            CurveGeometry::Circle {
                center: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
                axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            Vec::new(),
        );
        assert_eq!(
            legacy_decoded_curve_endpoints(&circle, 0).expect("circle endpoints"),
            [Point3([3.0, 2.0, 3.0]); 2]
        );
        let point = cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0);
        let degenerate =
            crate::curves::DecodedCurve::leaf(CurveGeometry::Degenerate { point }, Vec::new());
        assert_eq!(
            legacy_decoded_curve_endpoints(&degenerate, 0).expect("degenerate endpoints"),
            [Point3([4.0, 5.0, 6.0]); 2]
        );
    }

    #[test]
    fn negative_array_count_is_rejected_before_allocation() {
        let mut bytes = vec![0x30, 0x10];
        bytes.extend_from_slice(&(-1_i32).to_le_bytes());
        let error = parse(&bytes, 0..bytes.len(), ArchiveVersion::V5, None, &[])
            .expect_err("negative C2 count must fail");
        assert!(matches!(error, GeometryError::Malformed(_)));
    }

    #[test]
    fn raw_arrays_consume_complete_anonymous_wrappers() {
        let bytes = packed_array(0, &[]);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let (_, range) = read_vertices(&bytes, &mut reader, ArchiveVersion::V5, &mut Vec::new())
            .expect("vertex");
        assert_eq!(range, 0..bytes.len());
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn raw_array_crc_mismatch_warns_and_consumes_wrapper() {
        let mut bytes = packed_array(0, &[]);
        let crc = bytes.len() - 1;
        bytes[crc] ^= 1;
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Vec::new();
        read_vertices(&bytes, &mut reader, ArchiveVersion::V5, &mut warnings)
            .expect("recoverable vertex wrapper");
        assert_eq!(reader.remaining(), 0);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Brep anonymous CRC mismatch"));
    }

    #[test]
    fn face_reader_accepts_all_packed_minors() {
        for version in [0x10_u8, 0x11, 0x12] {
            let mut body = vec![version, 0, 0, 0, 0];
            if version == 0x12 {
                body.push(0);
            }
            let bytes = anonymous(&body);
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
            let (faces, _) = read_faces(&bytes, &mut reader, ArchiveVersion::V5, &mut Vec::new())
                .expect("faces");
            assert!(faces.is_empty());
        }
    }

    #[test]
    fn trim_gate_preserves_legacy_tail_and_wrapper_range() {
        for writer in [200_000_000_i64, 200_206_180] {
            let record = trim_record(writer >= 200_206_180);
            assert_eq!(record.len(), 132);
            let bytes = packed_array(1, &record);
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
            let (trims, range) = read_trims(
                &bytes,
                &mut reader,
                ArchiveVersion::V5,
                Some(writer),
                &mut Vec::new(),
                &mut Vec::new(),
            )
            .expect("trims");
            assert_eq!(range, 0..bytes.len());
            assert_eq!(trims[0].legacy_tolerances, [0.0, 0.0]);
        }
    }

    #[test]
    fn tolerance_accepts_explicit_signed_unset_values() {
        assert!(finite_tolerance(ON_UNSET_VALUE, "tolerance").is_ok());
        assert!(finite_tolerance(ON_UNSET_POSITIVE_VALUE, "tolerance").is_ok());
        assert!(finite_tolerance(-1.0, "tolerance").is_err());
    }

    #[test]
    fn interval_accepts_explicit_signed_unset_values() {
        for value in [
            Interval([ON_UNSET_VALUE, ON_UNSET_VALUE]),
            Interval([ON_UNSET_POSITIVE_VALUE, ON_UNSET_POSITIVE_VALUE]),
            Interval([ON_UNSET_VALUE, ON_UNSET_POSITIVE_VALUE]),
            Interval([ON_UNSET_POSITIVE_VALUE, ON_UNSET_VALUE]),
        ] {
            assert!(finite_interval(value, "interval").is_ok());
        }
        assert!(finite_interval(Interval([0.0, 0.0]), "interval").is_err());
    }

    #[test]
    fn procedural_loops_use_one_matching_trim_without_ring_closure() {
        let mut curve_loop = degenerate_trim_raw(5, 0);
        curve_loop.loops[0].loop_type = 4;
        assert!(validate_rings(&curve_loop).is_ok());

        let mut point_loop = degenerate_trim_raw(6, -1);
        point_loop.loops[0].loop_type = 5;
        assert!(validate_rings(&point_loop).is_ok());

        let mut mismatched = degenerate_trim_raw(6, -1);
        mismatched.loops[0].loop_type = 4;
        assert!(validate_rings(&mismatched).is_err());
    }

    #[test]
    fn mesh_side_wrapper_degrades_truncated_present_slot_without_losing_parent() {
        let bytes = anonymous(&[1]);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Vec::new();
        let (slots, _) = read_mesh_sides(&bytes, &mut reader, ArchiveVersion::V5, 1, &mut warnings)
            .expect("degraded cache");
        assert!(slots[0].mesh.is_none());
        assert!(!warnings.is_empty());
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn legacy_mesh_side_degrades_truncated_present_slot() {
        let bytes = [1_u8];
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Vec::new();
        let (slots, range) =
            read_legacy_mesh_sides(&bytes, &mut reader, ArchiveVersion::V5, 1, &mut warnings)
                .expect("legacy cache degradation");
        assert_eq!(range, 0..bytes.len());
        assert_eq!(slots.len(), 1);
        assert!(!slots[0].present);
        assert!(!warnings.is_empty());
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn mesh_side_wrapper_starts_with_face_zero_presence() {
        let bytes = anonymous(&[0]);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Vec::new();
        let (slots, _) = read_mesh_sides(&bytes, &mut reader, ArchiveVersion::V5, 1, &mut warnings)
            .expect("empty cache slot");
        assert_eq!(slots.len(), 1);
        assert!(!slots[0].present);
        assert!(warnings.is_empty());
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn mesh_side_wrapper_retains_nested_class_userdata() {
        let presence = [1_u8];
        let wrapper = mesh_class_wrapper_with_userdata();
        let bytes = anonymous_mixed(&[(&presence, false), (&wrapper, true)]);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Vec::new();
        let (slots, _) = read_mesh_sides(&bytes, &mut reader, ArchiveVersion::V5, 1, &mut warnings)
            .expect("mesh cache with userdata");
        assert_eq!(slots.len(), 1);
        assert!(slots[0].present);
        assert!(slots[0].mesh.is_some(), "warnings: {warnings:?}");
        assert_eq!(slots[0].userdata.len(), 1);
        assert_eq!(
            slots[0].userdata[0].item_uuid(),
            crate::mesh::V5_MESH_DOUBLE_VERTICES
        );
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn polymorphic_array_preserves_null_and_classifies_wrong_base() {
        let mut body = vec![0x10];
        body.extend_from_slice(&2_i32.to_le_bytes());
        body.extend_from_slice(&0_i32.to_le_bytes());
        body.extend_from_slice(&1_i32.to_le_bytes());
        body.extend(class_wrapper(&[]));
        let bytes = anonymous(&body);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let array = read_children(
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            RawBrepBaseType::Curve,
            0,
            &mut Vec::new(),
        )
        .expect("children");
        assert!(array.slots[0].is_none());
        assert_eq!(
            array.slots[1].as_ref().expect("wrong class").base_type,
            RawBrepBaseType::Other
        );
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn region_outer_wrapper_preserves_v5_raw_element_boundaries() {
        let mut region_record = Vec::new();
        region_record.extend_from_slice(&0_i32.to_le_bytes());
        region_record.extend_from_slice(&0_i32.to_le_bytes());
        region_record.extend_from_slice(&0_i32.to_le_bytes());
        region_record.extend([0.0_f64; 6].into_iter().flat_map(f64::to_le_bytes));
        let raw_element = anonymous(&{
            let mut body = 1_i32.to_le_bytes().to_vec();
            body.extend_from_slice(&0_i32.to_le_bytes());
            body.extend(region_record);
            body
        });
        let mut region_prefix = 1_i32.to_le_bytes().to_vec();
        region_prefix.extend_from_slice(&0_i32.to_le_bytes());
        region_prefix.extend_from_slice(&1_i32.to_le_bytes());
        let region_array = anonymous_mixed(&[(&region_prefix, false), (&raw_element, true)]);
        let side_array = anonymous(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let mut topology_prefix = 1_i32.to_le_bytes().to_vec();
        topology_prefix.extend_from_slice(&0_i32.to_le_bytes());
        let nested = anonymous_mixed(&[
            (&topology_prefix, false),
            (&side_array, true),
            (&region_array, true),
        ]);
        let mut outer_prefix = 1_i32.to_le_bytes().to_vec();
        outer_prefix.extend_from_slice(&1_i32.to_le_bytes());
        outer_prefix.push(1);
        let outer = anonymous_mixed(&[(&outer_prefix, false), (&nested, true)]);
        let mut reader = BoundedReader::new(&outer, 0, outer.len()).expect("reader");
        let mut warnings = Vec::new();
        let (_, regions, _, loaded) =
            read_regions(&outer, &mut reader, ArchiveVersion::V5, 0, &mut warnings)
                .expect("regions");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(loaded);
        assert_eq!(regions.len(), 1);
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn valid_one_face_raw_brep_validates_all_reciprocal_links() {
        assert!(ValidatedRawBrep::try_new(one_face_raw()).is_ok());
    }

    #[test]
    fn positional_indexes_are_diagnostic_and_array_order_remains_authoritative() {
        let mut raw = one_face_raw();
        raw.vertices[0].index = 9;
        raw.edges[1].index = 9;
        raw.trims[2].index = 9;
        raw.loops[0].index = 9;
        raw.faces[0].index = 9;
        let validated = ValidatedRawBrep::try_new(raw).expect("positional indexes are redundant");
        assert_eq!(validated.warnings().len(), 5);
    }

    #[test]
    fn singular_trim_accepts_c2_without_a_real_edge() {
        assert!(ValidatedRawBrep::try_new(degenerate_trim_raw(4, 0)).is_ok());
    }

    #[test]
    fn point_on_surface_trim_accepts_no_c2_or_real_edge() {
        assert!(ValidatedRawBrep::try_new(degenerate_trim_raw(6, -1)).is_ok());
    }

    #[test]
    fn point_on_surface_trim_rejects_an_attributed_c2() {
        assert!(ValidatedRawBrep::try_new(degenerate_trim_raw(6, 0)).is_err());
    }

    #[test]
    fn valid_region_topology_survives_semantic_validation() {
        let mut raw = one_face_raw();
        raw.minor = 3;
        raw.face_sides = vec![
            RawBrepFaceSide {
                index: 0,
                region: 1,
                face: 0,
                direction: 1,
                source_range: 0..0,
            },
            RawBrepFaceSide {
                index: 1,
                region: 0,
                face: 0,
                direction: -1,
                source_range: 0..0,
            },
        ];
        raw.regions = vec![
            RawBrepRegion {
                index: 0,
                region_type: 0,
                sides: vec![1],
                bounds: raw.bounds,
                source_range: 0..0,
            },
            RawBrepRegion {
                index: 1,
                region_type: 1,
                sides: vec![0],
                bounds: raw.bounds,
                source_range: 0..0,
            },
        ];
        let validated = ValidatedRawBrep::try_new(raw).expect("valid regions");
        assert_eq!(validated.raw().regions.len(), 2);
        assert!(validated.warnings().is_empty());
    }

    #[test]
    fn invalid_region_reciprocity_degrades_to_incidence_without_topology_failure() {
        let mut raw = one_face_raw();
        raw.minor = 3;
        raw.face_sides = vec![
            RawBrepFaceSide {
                index: 0,
                region: 1,
                face: 0,
                direction: 1,
                source_range: 0..0,
            },
            RawBrepFaceSide {
                index: 1,
                region: 0,
                face: 0,
                direction: -1,
                source_range: 0..0,
            },
        ];
        raw.regions = vec![
            RawBrepRegion {
                index: 0,
                region_type: 0,
                sides: vec![0],
                bounds: raw.bounds,
                source_range: 0..0,
            },
            RawBrepRegion {
                index: 1,
                region_type: 1,
                sides: vec![1],
                bounds: raw.bounds,
                source_range: 0..0,
            },
        ];
        let validated = ValidatedRawBrep::try_new(raw).expect("optional regions degrade");
        assert!(validated.raw().regions.is_empty());
        assert_eq!(validated.warnings().len(), 1);
    }
}
