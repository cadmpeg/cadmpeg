// SPDX-License-Identifier: Apache-2.0
//! Isolated `ON_Brep` parsing and semantic validation.
//!
//! Stops at a validated native representation; no topology IDs or IR carriers.

use crate::loss::Diagnostics;
use std::collections::{BTreeMap, HashSet};
use std::ops::Range;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::scalar::{NonNegativeReal, PositiveReal};
use cadmpeg_ir::units::FiniteVector;

use crate::chunks::{
    chunk_at, verify_checksum, verify_checksum_ranges, ArchiveVersion, BoundedReader,
    ChecksumStatus, Chunk, FramingError,
};
use crate::curves::{error, GeometryError};
use crate::objects::{
    parse_class_wrapper, parse_class_wrapper_with_userdata, ClassUserdata, UserdataDescriptor,
};
use crate::settings::{bbox, interval, BoundingBox, CoordinateLane, Interval, Point3};
use crate::wire::{uuid, Uuid};

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
const MAX_BREP_ITEMS: usize = 1 << 20;
const ANONYMOUS: u32 = 0x4000_8000;
const ON_UNSET_VALUE: f64 = -1.234_321_012_343_21e308;
const ON_UNSET_POSITIVE_VALUE: f64 = -ON_UNSET_VALUE;

/// A Brep tolerance admitted with both exact source unset sentinels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BrepTolerance {
    Unset,
    NonNegative(NonNegativeReal),
}

impl BrepTolerance {
    pub(crate) fn new(value: f64) -> Option<Self> {
        if value == ON_UNSET_VALUE || value == ON_UNSET_POSITIVE_VALUE {
            Some(Self::Unset)
        } else {
            NonNegativeReal::new(value).map(Self::NonNegative)
        }
    }

    pub(crate) fn positive(self) -> Option<PositiveReal> {
        match self {
            Self::NonNegative(value) => value.positive(),
            Self::Unset => None,
        }
    }

    pub(crate) fn fit(self) -> Option<cadmpeg_ir::geometry::FitTolerance> {
        match self {
            Self::NonNegative(value) => value
                .positive()
                .map(|_| cadmpeg_ir::geometry::FitTolerance::from(value)),
            Self::Unset => None,
        }
    }
}
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
}

impl RawBrepChild {
    /// Returns the base-class family defined by the class UUID.
    fn base_type(&self) -> RawBrepBaseType {
        if crate::curves::curve_class(self.class_uuid) {
            RawBrepBaseType::Curve
        } else if crate::curves::surface_class(self.class_uuid) {
            RawBrepBaseType::Surface
        } else {
            RawBrepBaseType::Other
        }
    }
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
    /// Vertex point. A modern Brep reads it as an admitted finite point; a
    /// legacy Brep computes it as the mean of the curve endpoints merged into
    /// the vertex, and a non-finite coordinate carries through to the point,
    /// where the Brep validator owns it.
    pub(crate) point: CoordinateLane<3>,
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
    pub(crate) proxy_reversed: bool,
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

/// The `ON_BrepTrim` type value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawTrimKind {
    Unknown,
    Boundary,
    Mated,
    Seam,
    Singular,
    CurveOnSurface,
    PointOnSurface,
    Slit,
}

impl RawTrimKind {
    fn parse(value: i32) -> Option<Self> {
        Some(match value {
            0 => Self::Unknown,
            1 => Self::Boundary,
            2 => Self::Mated,
            3 => Self::Seam,
            4 => Self::Singular,
            5 => Self::CurveOnSurface,
            6 => Self::PointOnSurface,
            7 => Self::Slit,
            _ => return None,
        })
    }
}

/// The `ON_Surface::ISO` value a trim carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawTrimIso {
    None,
    X,
    Y,
    West,
    South,
    East,
    North,
}

impl RawTrimIso {
    fn parse(value: i32) -> Option<Self> {
        Some(match value {
            0 => Self::None,
            1 => Self::X,
            2 => Self::Y,
            3 => Self::West,
            4 => Self::South,
            5 => Self::East,
            6 => Self::North,
            _ => return None,
        })
    }
}

/// The `ON_BrepLoop` type value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawLoopKind {
    Unknown,
    Outer,
    Inner,
    Slit,
    CurveOnSurface,
    PointOnSurface,
}

impl RawLoopKind {
    fn parse(value: i32) -> Option<Self> {
        Some(match value {
            0 => Self::Unknown,
            1 => Self::Outer,
            2 => Self::Inner,
            3 => Self::Slit,
            4 => Self::CurveOnSurface,
            5 => Self::PointOnSurface,
            _ => return None,
        })
    }
}

/// A raw Brep trim.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepTrim {
    /// Positional record index.
    pub(crate) index: i32,
    /// C2 curve slot, absent on a point-on-surface trim.
    pub(crate) curve: Option<i32>,
    /// Proxy domain.
    pub(crate) proxy_domain: Interval,
    /// Edge index, absent on singular and point trims.
    pub(crate) edge: Option<i32>,
    /// Start and end vertex indexes.
    pub(crate) vertices: [i32; 2],
    /// Three-dimensional reversal flag.
    pub(crate) reversed_3d: bool,
    /// Trim type.
    pub(crate) trim_type: RawTrimKind,
    /// Trim ISO classification.
    pub(crate) iso: RawTrimIso,
    /// Loop index.
    pub(crate) loop_index: i32,
    /// Two-dimensional and three-dimensional tolerances.
    pub(crate) tolerances: [f64; 2],
    /// Native trim domain.
    pub(crate) domain: Interval,
    /// Proxy reversal byte.
    pub(crate) proxy_reversed: bool,
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
    /// Loop type.
    pub(crate) loop_type: RawLoopKind,
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
    pub(crate) reversed_surface: bool,
    /// Material channel.
    pub(crate) material_channel: i32,
    /// Optional face UUID.
    pub(crate) uuid: Option<Uuid>,
    /// Optional per-face color.
    pub(crate) color: Option<[u8; 4]>,
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// A present render or analysis mesh cache entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawBrepMesh {
    /// Mesh child that passed class validation.
    pub(crate) mesh: RawBrepChild,
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
    /// Raw region type.
    pub(crate) region_type: i32,
    /// Member face-side indexes.
    pub(crate) sides: Vec<i32>,
    /// Region bounds.
    pub(crate) bounds: BoundingBox,
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// The `ON_Brep` solid state a stored flag names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SolidState {
    Open,
    Closed,
    ClosedManifold,
}

/// The `ON_Brep` solid flag as the archive carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawSolidFlag {
    /// The archive minor version predates the flag, so no flag was written.
    Unstamped,
    Known(SolidState),
    /// A written flag outside the documented set, retained for native fidelity.
    OutOfRange(i32),
}

impl RawSolidFlag {
    fn parse(value: i32) -> Self {
        match value {
            0 => Self::Known(SolidState::Open),
            1 => Self::Known(SolidState::Closed),
            2 => Self::Known(SolidState::ClosedManifold),
            other => Self::OutOfRange(other),
        }
    }

    fn stored(self) -> Option<i32> {
        match self {
            Self::Unstamped => None,
            Self::Known(SolidState::Open) => Some(0),
            Self::Known(SolidState::Closed) => Some(1),
            Self::Known(SolidState::ClosedManifold) => Some(2),
            Self::OutOfRange(value) => Some(value),
        }
    }
}

/// Parsed Brep data before semantic validation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrep {
    /// Typed losses raised while selecting writer-version-dependent layouts.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
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
    pub(crate) render_meshes: Vec<Option<RawBrepMesh>>,
    /// Analysis mesh cache slots.
    pub(crate) analysis_meshes: Vec<Option<RawBrepMesh>>,
    /// Raw solid state, normalized only by validation.
    pub(crate) is_solid: RawSolidFlag,
    /// Region face sides.
    pub(crate) face_sides: Vec<RawBrepFaceSide>,
    /// Regions.
    pub(crate) regions: Vec<RawBrepRegion>,
    /// Complete payload range.
    pub(crate) source_range: Range<usize>,
}

/// One vertex's references, resolved to array positions by validation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedVertex {
    /// Incident edge positions.
    pub(crate) edges: Vec<usize>,
    /// Admitted source tolerance.
    pub(crate) tolerance: BrepTolerance,
}

/// One edge's references, resolved to array positions by validation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedEdge {
    /// C3 child slot position.
    pub(crate) curve: usize,
    /// Start and end vertex positions.
    pub(crate) vertices: [usize; 2],
    /// Trim positions using this edge.
    pub(crate) trims: Vec<usize>,
    /// Admitted source tolerance.
    pub(crate) tolerance: BrepTolerance,
}

/// One trim's references, resolved to array positions by validation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedTrim {
    /// C2 child slot position, absent on a point-on-surface trim.
    pub(crate) curve: Option<usize>,
    /// Edge position, absent on a singular or point-on-surface trim.
    pub(crate) edge: Option<usize>,
    /// Start and end vertex positions.
    pub(crate) vertices: [usize; 2],
    /// Owning loop position.
    pub(crate) loop_index: usize,
    /// Admitted two-dimensional and three-dimensional source tolerances.
    pub(crate) tolerances: [BrepTolerance; 2],
}

/// One loop's references, resolved to array positions by validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedLoop {
    /// Directed trim ring positions.
    pub(crate) trims: Vec<usize>,
    /// Owning face position.
    pub(crate) face: usize,
}

/// One face's references, resolved to array positions by validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedFace {
    /// Surface child slot position.
    pub(crate) surface: usize,
    /// Boundary loop positions, outer first.
    pub(crate) loops: Vec<usize>,
}

/// One region face side, resolved to array positions by validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedFaceSide {
    /// Face position this side belongs to.
    pub(crate) face: usize,
    /// Region position, absent when the side is unassigned.
    pub(crate) region: Option<usize>,
}

/// Every B-rep reference, resolved to an array position by validation.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ResolvedBrep {
    /// Resolved vertex references, positionally aligned with the raw vertices.
    pub(crate) vertices: Vec<ResolvedVertex>,
    /// Resolved edge references, positionally aligned with the raw edges.
    pub(crate) edges: Vec<ResolvedEdge>,
    /// Resolved trim references, positionally aligned with the raw trims.
    pub(crate) trims: Vec<ResolvedTrim>,
    /// Resolved loop references, positionally aligned with the raw loops.
    pub(crate) loops: Vec<ResolvedLoop>,
    /// Resolved face references, positionally aligned with the raw faces.
    pub(crate) faces: Vec<ResolvedFace>,
    /// Resolved region face sides, empty when region topology was discarded.
    pub(crate) face_sides: Vec<ResolvedFaceSide>,
}

/// A semantically validated raw Brep.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ValidatedRawBrep {
    /// Validated Brep payload.
    raw: RawBrep,
    /// Every reference of the payload, resolved to an array position.
    resolved: ResolvedBrep,
    /// Warnings for repaired positional fields or discarded optional data.
    warnings: Diagnostics,
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
    /// Validates a structurally decoded Brep for owned test fixtures.
    #[cfg(test)]
    pub(crate) fn try_new(
        ctx: &DecodeContext<'_>,
        mut raw: RawBrep,
    ) -> Result<Self, GeometryError> {
        let (resolved, warnings) = Self::validate(ctx, &mut raw)?;
        Ok(Self {
            raw,
            resolved,
            warnings,
        })
    }

    fn validate(
        ctx: &DecodeContext<'_>,
        raw: &mut RawBrep,
    ) -> Result<(ResolvedBrep, Diagnostics), GeometryError> {
        let mut warnings = Diagnostics::new();
        for (label, mismatch) in [
            (
                "vertex",
                positions_drifted(ctx, &raw.vertices, |value| value.index)?,
            ),
            (
                "edge",
                positions_drifted(ctx, &raw.edges, |value| value.index)?,
            ),
            (
                "trim",
                positions_drifted(ctx, &raw.trims, |value| value.index)?,
            ),
            (
                "loop",
                positions_drifted(ctx, &raw.loops, |value| value.index)?,
            ),
            (
                "face",
                positions_drifted(ctx, &raw.faces, |value| value.index)?,
            ),
            (
                "region face-side",
                positions_drifted(ctx, &raw.face_sides, |value| value.index)?,
            ),
        ] {
            if mismatch {
                warnings.push_coded_admitted(
                    ctx,
                    crate::loss::RhinoLossCode::RedundantFieldRepaired,
                    format_args!(
                    "redundant Brep {label} positional index mismatch; serialized array order used"
                ),
                )?;
            }
        }
        let mut resolved = ResolvedBrep {
            vertices: ctx
                .collection_vec(raw.vertices.len(), "Rhino resolved Brep vertices")
                .map_err(crate::curves::GeometryError::from)?,
            edges: ctx
                .collection_vec(raw.edges.len(), "Rhino resolved Brep edges")
                .map_err(crate::curves::GeometryError::from)?,
            trims: ctx
                .collection_vec(raw.trims.len(), "Rhino resolved Brep trims")
                .map_err(crate::curves::GeometryError::from)?,
            loops: ctx
                .collection_vec(raw.loops.len(), "Rhino resolved Brep loops")
                .map_err(crate::curves::GeometryError::from)?,
            faces: ctx
                .collection_vec(raw.faces.len(), "Rhino resolved Brep faces")
                .map_err(crate::curves::GeometryError::from)?,
            face_sides: Vec::new(),
        };
        for vertex in ctx
            .admit_iter(&raw.vertices[..], "Rhino validate traversal")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            let edges = slots(ctx, &vertex.edges, raw.edges.len(), "vertex edge")?;
            let tolerance = finite_tolerance(ctx, vertex.tolerance, "vertex tolerance")?;
            resolved.vertices.push(ResolvedVertex { edges, tolerance });
        }
        for (index, edge) in ctx
            .admit_iter(&raw.edges[..], "Rhino validate traversal")
            .map_err(cadmpeg_core::CodecError::from)?
            .enumerate()
        {
            let Some(curve) = child_slot(&raw.c3, edge.curve, RawBrepBaseType::Curve) else {
                return Err(error(
                    edge.source_range.start,
                    "edge C3 reference is invalid",
                ));
            };
            let vertices = slot_pair(ctx, edge.vertices, raw.vertices.len(), "edge vertex")?;
            let trims = slots(ctx, &edge.trims, raw.trims.len(), "edge trim")?;
            unique(ctx, &edge.trims, "edge trim")?;
            ordered_interval(ctx, edge.proxy_domain, "edge proxy domain")?;
            ordered_interval(ctx, edge.domain, "edge domain")?;
            let tolerance = finite_tolerance(ctx, edge.tolerance, "edge tolerance")?;
            for trim in ctx
                .admit_iter(&trims[..], "Rhino validate traversal")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                if position(raw.trims[*trim].edge) != Some(index) {
                    return Err(error(
                        edge.source_range.start,
                        "edge/trim reciprocity mismatch",
                    ));
                }
            }
            resolved.edges.push(ResolvedEdge {
                curve,
                vertices,
                trims,
                tolerance,
            });
        }
        let mut membership_storage = ctx.reserve_scoped(0, "Rhino Brep membership flags")?;
        let mut listed_trims = membership_storage.with_storage(|| {
            ctx.alloc_filled(raw.trims.len(), false, "Rhino Brep trim membership")
        })?;
        for (loop_index, loop_record) in ctx
            .admit_iter(&raw.loops[..], "Rhino Brep trim membership")
            .map_err(cadmpeg_core::CodecError::from)?
            .enumerate()
        {
            for value in ctx
                .admit_iter(&loop_record.trims[..], "Rhino Brep trim membership")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                if let Some(trim_index) =
                    position(Some(*value)).filter(|&index| index < raw.trims.len())
                {
                    if position(Some(raw.trims[trim_index].loop_index)) == Some(loop_index) {
                        listed_trims[trim_index] = true;
                    }
                }
            }
        }
        for (trim_index, trim) in ctx
            .admit_iter(&raw.trims[..], "Rhino validate traversal")
            .map_err(cadmpeg_core::CodecError::from)?
            .enumerate()
        {
            let curve = if trim.trim_type == RawTrimKind::PointOnSurface {
                if trim.curve.is_some() {
                    return Err(error(
                        trim.source_range.start,
                        "point-on-surface trim must not require C2",
                    ));
                }
                None
            } else {
                let Some(curve) = trim
                    .curve
                    .and_then(|curve| child_slot(&raw.c2, curve, RawBrepBaseType::Curve))
                else {
                    return Err(error(
                        trim.source_range.start,
                        "trim C2 reference is invalid",
                    ));
                };
                Some(curve)
            };
            let vertices = slot_pair(ctx, trim.vertices, raw.vertices.len(), "trim vertex")?;
            let loop_index = slot(ctx, trim.loop_index, raw.loops.len(), "trim loop")?;
            if !listed_trims[trim_index] {
                return Err(error(
                    trim.source_range.start,
                    "trim/loop reciprocity mismatch",
                ));
            }
            ordered_interval(ctx, trim.proxy_domain, "trim proxy domain")?;
            ordered_interval(ctx, trim.domain, "trim domain")?;
            let tolerances = [
                finite_tolerance(ctx, trim.tolerances[0], "trim tolerance")?,
                finite_tolerance(ctx, trim.tolerances[1], "trim tolerance")?,
            ];
            for tolerance in trim.legacy_tolerances {
                finite_tolerance(ctx, tolerance, "trim tolerance")?;
            }
            let edge = if matches!(
                trim.trim_type,
                RawTrimKind::Singular | RawTrimKind::PointOnSurface
            ) {
                if trim.edge.is_some() || trim.vertices[0] != trim.vertices[1] {
                    return Err(error(
                        trim.source_range.start,
                        "singular trim endpoints are invalid",
                    ));
                }
                None
            } else {
                let Some(edge) = trim.edge else {
                    return Err(error(
                        trim.source_range.start,
                        "trim edge reference is out of range",
                    ));
                };
                Some(slot(ctx, edge, raw.edges.len(), "trim edge")?)
            };
            resolved.trims.push(ResolvedTrim {
                curve,
                edge,
                vertices,
                loop_index,
                tolerances,
            });
        }
        validate_edge_incidences(ctx, raw, &resolved)?;
        for (index, vertex) in ctx
            .admit_iter(&resolved.vertices[..], "Rhino validate traversal")
            .map_err(cadmpeg_core::CodecError::from)?
            .enumerate()
        {
            for edge in ctx
                .admit_iter(&vertex.edges[..], "Rhino validate traversal")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                if !resolved.edges[*edge].vertices.contains(&index) {
                    return Err(error(
                        raw.vertices[index].source_range.start,
                        "vertex/edge reciprocity mismatch",
                    ));
                }
            }
        }
        let mut listed_loops = membership_storage.with_storage(|| {
            ctx.alloc_filled(raw.loops.len(), false, "Rhino Brep loop membership")
        })?;
        for (face_index, face) in ctx
            .admit_iter(&raw.faces[..], "Rhino Brep loop membership")
            .map_err(cadmpeg_core::CodecError::from)?
            .enumerate()
        {
            for value in ctx
                .admit_iter(&face.loops[..], "Rhino Brep loop membership")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                if let Some(loop_index) =
                    position(Some(*value)).filter(|&index| index < raw.loops.len())
                {
                    if position(Some(raw.loops[loop_index].face)) == Some(face_index) {
                        listed_loops[loop_index] = true;
                    }
                }
            }
        }
        for (index, loop_record) in ctx
            .admit_iter(&raw.loops[..], "Rhino validate traversal")
            .map_err(cadmpeg_core::CodecError::from)?
            .enumerate()
        {
            let trims = slots(ctx, &loop_record.trims, raw.trims.len(), "loop trim")?;
            unique(ctx, &loop_record.trims, "loop trim")?;
            let face = slot(ctx, loop_record.face, raw.faces.len(), "loop face")?;
            if !listed_loops[index] {
                return Err(error(
                    loop_record.source_range.start,
                    "loop/face reciprocity mismatch",
                ));
            }
            if loop_record.loop_type == RawLoopKind::Outer
                && position(raw.faces[face].loops.first().copied()) != Some(index)
            {
                return Err(error(
                    loop_record.source_range.start,
                    "outer loop is not first",
                ));
            }
            resolved.loops.push(ResolvedLoop { trims, face });
        }
        for (index, face) in ctx
            .admit_iter(&raw.faces[..], "Rhino validate traversal")
            .map_err(cadmpeg_core::CodecError::from)?
            .enumerate()
        {
            let Some(surface) = child_slot(&raw.surfaces, face.surface, RawBrepBaseType::Surface)
            else {
                return Err(error(
                    face.source_range.start,
                    "face surface reference is invalid",
                ));
            };
            let loops = slots(ctx, &face.loops, raw.loops.len(), "face loop")?;
            let Some(outer) = loops.first() else {
                return Err(error(face.source_range.start, "face has no loops"));
            };
            if raw.loops[*outer].loop_type != RawLoopKind::Outer {
                return Err(error(
                    face.source_range.start,
                    "face first loop is not outer",
                ));
            }
            for loop_index in ctx
                .admit_iter(&loops[1..], "Rhino validate traversal")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                let loop_type = raw.loops[*loop_index].loop_type;
                if matches!(loop_type, RawLoopKind::Unknown | RawLoopKind::Outer) {
                    return Err(error(
                        face.source_range.start,
                        "face boundary loop convention is invalid",
                    ));
                }
            }
            for loop_index in ctx
                .admit_iter(&loops[..], "Rhino validate traversal")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                if resolved.loops[*loop_index].face != index {
                    return Err(error(
                        face.source_range.start,
                        "face/loop reciprocity mismatch",
                    ));
                }
            }
            resolved.faces.push(ResolvedFace { surface, loops });
        }
        validate_rings(ctx, raw, &resolved)?;
        if raw.minor >= 3 && (!raw.face_sides.is_empty() || !raw.regions.is_empty()) {
            match validate_regions(ctx, raw) {
                Ok(face_sides) => resolved.face_sides = face_sides,
                Err(error @ GeometryError::Codec(_)) => return Err(error),
                Err(_) => {
                    raw.face_sides.clear();
                    raw.regions.clear();
                    warnings.push_coded_admitted(
                        ctx,
                        crate::loss::RhinoLossCode::RedundantFieldRepaired,
                        format_args!("invalid optional Brep region topology discarded"),
                    )?;
                }
            }
        }
        for face in ctx
            .admit_iter(&mut raw.faces[..], "Rhino repaired face traversal")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            if face.material_channel < 0 {
                face.material_channel = 0;
            }
        }
        Ok((resolved, warnings))
    }

    /// Returns every reference of the payload, resolved to an array position.
    pub(crate) fn resolved(&self) -> &ResolvedBrep {
        &self.resolved
    }

    /// Returns the validated and normalized raw payload.
    pub(crate) fn raw(&self) -> &RawBrep {
        &self.raw
    }

    /// Returns warnings produced while validation normalized optional data.
    pub(crate) fn warnings(&self) -> &Diagnostics {
        &self.warnings
    }

    /// Selects the serialized body kind and reports an unverified stamp-dependent gauge.
    pub(crate) fn body_kind(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        writer_version: Option<i64>,
    ) -> Result<(BrepBodyKind, Option<cadmpeg_ir::report::loss::LossNote>), cadmpeg_core::CodecError>
    {
        body_kind(ctx, &self.raw, &self.resolved, writer_version)
    }
}

/// Classifies one B-rep body, reporting whether a missing stamp decided it.
fn body_kind(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    raw: &RawBrep,
    resolved: &ResolvedBrep,
    writer_version: Option<i64>,
) -> Result<(BrepBodyKind, Option<cadmpeg_ir::report::loss::LossNote>), cadmpeg_core::CodecError> {
    // A stamped closed flag fixes the kind without a topology gauge.
    if writer_version.is_some_and(|version| version >= SOLID_FLAG_WRITER_VERSION)
        && matches!(
            raw.is_solid,
            RawSolidFlag::Known(SolidState::Closed | SolidState::ClosedManifold)
        )
    {
        return Ok((BrepBodyKind::Solid, None));
    }
    let mut closed = !raw.faces.is_empty();
    if closed && !resolved.edges.is_empty() {
        let mut storage = ctx.reserve_scoped(0, "Rhino body kind edge incidence")?;
        let mut counts = storage.with_storage(|| {
            ctx.alloc_filled(
                resolved.edges.len(),
                0_usize,
                "Rhino body kind edge incidence",
            )
        })?;
        for trim in ctx
            .admit_iter(
                &resolved.trims[..],
                "Rhino body kind trim incidence traversal",
            )
            .map_err(cadmpeg_core::CodecError::from)?
        {
            if let Some(count) = trim.edge.and_then(|edge| counts.get_mut(edge)) {
                *count += 1;
            }
        }
        closed = ctx.all_by(
            &counts[..],
            |count| Ok(*count == 2),
            "Rhino body kind edge incidence",
        )?;
    }
    let kind = serialized_body_kind(raw.is_solid, writer_version, closed);
    let loss = if body_kind_rests_on_missing_stamp(raw.is_solid, writer_version, closed) {
        let code = crate::loss::RhinoLossCode::TopologyBodyKindGaugeSubstituted;
        let operation = "Rhino Brep body-kind loss message";
        Some(match raw.is_solid.stored() {
            Some(stored) => crate::wire::admitted_loss(ctx, code, format_args!("Brep body kind gauge substituted: stored solid flag {stored} was trusted over the closed-shell gauge because the writer-version stamp is absent"), operation)?,
            None => crate::wire::admitted_loss(ctx, code, format_args!("Brep body kind gauge substituted: stored solid flag absent was trusted over the closed-shell gauge because the writer-version stamp is absent"), operation)?,
        })
    } else {
        None
    };
    Ok((kind, loss))
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
    is_solid: RawSolidFlag,
    writer_version: Option<i64>,
    closed: bool,
) -> bool {
    writer_version.is_none()
        && serialized_body_kind(is_solid, None, closed)
            != serialized_body_kind(is_solid, Some(SOLID_FLAG_WRITER_VERSION - 1), closed)
}

fn serialized_body_kind(
    is_solid: RawSolidFlag,
    writer_version: Option<i64>,
    closed: bool,
) -> BrepBodyKind {
    let trusted = writer_version.is_none_or(|version| version >= SOLID_FLAG_WRITER_VERSION);
    let shell_gauge = if closed {
        BrepBodyKind::Solid
    } else {
        BrepBodyKind::Sheet
    };
    match is_solid {
        RawSolidFlag::Known(state) if trusted => match state {
            SolidState::Closed | SolidState::ClosedManifold => BrepBodyKind::Solid,
            SolidState::Open => shell_gauge,
        },
        RawSolidFlag::Known(_) | RawSolidFlag::Unstamped | RawSolidFlag::OutOfRange(_) => {
            shell_gauge
        }
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
        warnings: Diagnostics,
    },
}

/// Parses and validates one `ON_Brep` class-data payload.
pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
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
        return parse_legacy_major2(ctx, bytes, range, archive, version, reader);
    }
    if version >> 4 != 3 {
        return Err(GeometryError::unsupported(
            version_offset,
            "unsupported ON_Brep major",
        ));
    }
    let minor = version & 0x0f;
    let mut warnings = Diagnostics::new();
    let mut losses = Vec::new();
    let c2 = read_children(
        ctx,
        bytes,
        &mut reader,
        archive,
        RawBrepBaseType::Curve,
        &mut warnings,
    )?;
    let c3 = read_children(
        ctx,
        bytes,
        &mut reader,
        archive,
        RawBrepBaseType::Curve,
        &mut warnings,
    )?;
    let surfaces = read_children(
        ctx,
        bytes,
        &mut reader,
        archive,
        RawBrepBaseType::Surface,
        &mut warnings,
    )?;
    let (vertices, _) = read_vertices(ctx, bytes, &mut reader, archive, &mut warnings)?;
    let (edges, _) = read_edges(
        ctx,
        bytes,
        &mut reader,
        archive,
        writer_version,
        &mut warnings,
        &mut losses,
    )?;
    let (trims, _) = read_trims(
        ctx,
        bytes,
        &mut reader,
        archive,
        writer_version,
        &mut warnings,
        &mut losses,
    )?;
    let (loops, _) = read_loops(ctx, bytes, &mut reader, archive, &mut warnings)?;
    let (faces, _) = read_faces(ctx, bytes, &mut reader, archive, &mut warnings)?;
    let bounds = bbox(ctx, &mut reader)?;
    let (render_meshes, analysis_meshes) = if minor >= 1 {
        let (render, _) =
            read_mesh_sides(ctx, bytes, &mut reader, archive, faces.len(), &mut warnings)?;
        let (analysis, _) =
            read_mesh_sides(ctx, bytes, &mut reader, archive, faces.len(), &mut warnings)?;
        (render, analysis)
    } else {
        (Vec::new(), Vec::new())
    };
    let is_solid = if minor >= 2 {
        let flag = RawSolidFlag::parse(reader.i32()?);
        if let RawSolidFlag::OutOfRange(value) = flag {
            warnings.push_coded_admitted(
                ctx,
                crate::loss::RhinoLossCode::EnumerationValueDegraded,
                format_args!("invalid Brep is_solid value {value}; retained for native fidelity"),
            )?;
        }
        flag
    } else {
        RawSolidFlag::Unstamped
    };
    let (mut face_sides, mut regions, _, inline_region_loaded) = if minor >= 3 {
        read_regions(ctx, bytes, &mut reader, archive, faces.len(), &mut warnings)?
    } else {
        (Vec::new(), Vec::new(), None, false)
    };
    if !inline_region_loaded {
        if let Some(extra) = ctx.find_map(
            userdata,
            |raw| {
                let Some(value) = UserdataDescriptor::known(raw) else {
                    return Ok(None);
                };
                Ok((value.class_uuid == V5_BREP_REGION_TOPOLOGY_USERDATA
                    && value.item_uuid == V5_BREP_REGION_TOPOLOGY_USERDATA
                    && (value.application_uuid.is_none()
                        || value.application_uuid == Some(OPENNURBS4)))
                .then_some(value))
            },
            "Rhino parse traversal",
        )? {
            match read_region_topology_userdata(
                ctx,
                bytes,
                extra,
                archive,
                faces.len(),
                &mut warnings,
            ) {
                Ok((sides, topology_regions, _, _)) => {
                    face_sides = sides;
                    regions = topology_regions;
                }
                Err(error @ GeometryError::Codec(_)) => return Err(error),
                Err(error) => warnings.push_coded_admitted(
                    ctx,
                    crate::loss::RhinoLossCode::RedundantFieldRepaired,
                    format_args!("invalid optional Brep region topology discarded: {error}"),
                )?,
            }
        }
    }
    let skipped = reader.skip_remaining()?;
    if skipped != 0 {
        warnings.push_admitted(
            ctx,
            format_args!("ON_Brep skipped {skipped} trailing bytes"),
        )?;
    }
    let mut raw = RawBrep {
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
        is_solid,
        face_sides,
        regions,
        source_range: range,
    };
    match ValidatedRawBrep::validate(ctx, &mut raw) {
        Ok((resolved, mut validation_warnings)) => {
            validation_warnings.prepend(warnings);
            let validated = ValidatedRawBrep {
                raw,
                resolved,
                warnings: validation_warnings,
            };
            Ok(BrepParse::Valid(validated))
        }
        Err(error @ GeometryError::Codec(_)) => Err(error),
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
    endpoints: [[f64; 3]; 2],
}

impl LegacyCurveMeta {
    fn into_child(self) -> RawBrepChild {
        RawBrepChild {
            class_uuid: crate::curves::POLYCURVE,
            class_data_range: self.range.clone(),
            source_range: self.range,
        }
    }
}

struct LegacyVertex {
    vertex: RawBrepVertex,
    point_sum: [f64; 3],
    point_scale: [f64; 3],
    point_count: usize,
}

impl LegacyVertex {
    /// Answer the vertex carrying the mean of the endpoints merged into it, or
    /// refuse a scaled sum that left the range its own construction states.
    fn into_vertex(mut self) -> Option<RawBrepVertex> {
        if self.point_count != 0 {
            let mut point = [0.0; 3];
            for (axis, coordinate) in point.iter_mut().enumerate() {
                *coordinate =
                    scaled_mean(self.point_sum[axis], self.point_count)? * self.point_scale[axis];
            }
            self.vertex.point = CoordinateLane::Derived(point);
        }
        Some(self.vertex)
    }

    // Sum relative coordinates before dividing by the endpoint count. Finite
    // equal endpoints can have an unrepresentable sum but a finite mean.
    fn add_point(&mut self, point: [f64; 3]) {
        for (axis, value) in point.into_iter().enumerate() {
            let scale = self.point_scale[axis].max(value.abs());
            if scale > 0.0 {
                self.point_sum[axis] =
                    self.point_sum[axis] * (self.point_scale[axis] / scale) + value / scale;
                self.point_scale[axis] = scale;
            }
        }
        self.point_count += 1;
    }
}

/// Answer the mean of the endpoint coordinates `add_point` divided by the
/// largest magnitude among them.
///
/// Every scaled coordinate lies in `[-1, 1]`, so the exact mean does too.
/// `add_point` rounds twice per endpoint, once rescaling the running sum and
/// once adding the new coordinate, and the division here rounds once more, so
/// the computed mean can leave the interval by `2 * count * f64::EPSILON` and
/// no more. Only that excess is mapped onto the interval end. A mean beyond it
/// does not come from that rounding: the scaled sum no longer states the
/// endpoints that were read, and it is refused. A non-finite coordinate
/// carries through to the point, where the Brep validator owns it.
fn scaled_mean(sum: f64, count: usize) -> Option<f64> {
    let mean = sum / cadmpeg_core::convert::f64_from_index(count)?;
    if mean.abs() > 1.0 + 2.0 * cadmpeg_core::convert::f64_from_index(count)? * f64::EPSILON {
        return None;
    }
    if mean < -1.0 {
        return Some(-1.0);
    }
    if mean > 1.0 {
        return Some(1.0);
    }
    Some(mean)
}

fn parse_legacy_major2(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
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
    let bounds = bbox(ctx, &mut reader)?;

    let mut legacy_storage = ctx.reserve_scoped(0, "Rhino legacy Brep topology scratch")?;
    let c2_start = reader.position();
    let mut c2_meta = legacy_storage
        .with_storage(|| ctx.collection_vec(trim_count, "Rhino legacy Brep C2 metadata"))?;
    for _ in 0..trim_count {
        ctx.charge_work(1, "Rhino brep parse_legacy_major2 records")?;
        let start = reader.position();
        let mut curve_storage = ctx.reserve_scoped(0, "Rhino legacy Brep curve scratch")?;
        let decoded = curve_storage.with_storage(|| {
            let _depth = ctx.enter_nested("Rhino C2 curve tree")?;
            crate::curves::read_polycurve_2d(ctx, bytes, &mut reader, archive, 0)
                .map(|curve| crate::curves::DecodedGeometry::Curve { curve })
        })?;
        let curve_range = start..reader.position();
        let (domain, endpoints) = legacy_curve_shape(ctx, &decoded, curve_range.start)?;
        c2_meta.push(LegacyCurveMeta {
            range: curve_range,
            domain,
            endpoints,
        });
    }
    let c2_range = c2_start..reader.position();

    let c3_start = reader.position();
    let mut c3_meta = legacy_storage
        .with_storage(|| ctx.collection_vec(edge_count, "Rhino legacy Brep C3 metadata"))?;
    for _ in 0..edge_count {
        ctx.charge_work(1, "Rhino brep parse_legacy_major2 records")?;
        let start = reader.position();
        let mut curve_storage = ctx.reserve_scoped(0, "Rhino legacy Brep curve scratch")?;
        let decoded = curve_storage.with_storage(|| {
            let _depth = ctx.enter_nested("Rhino curve tree")?;
            crate::curves::read_polycurve(
                ctx,
                bytes,
                &mut reader,
                crate::settings::MillimeterScale::IDENTITY,
                archive,
                0,
            )
            .map(|curve| crate::curves::DecodedGeometry::Curve { curve })
        })?;
        let curve_range = start..reader.position();
        let (domain, endpoints) = legacy_curve_shape(ctx, &decoded, curve_range.start)?;
        c3_meta.push(LegacyCurveMeta {
            range: curve_range,
            domain,
            endpoints,
        });
    }
    let c3_range = c3_start..reader.position();

    let surfaces_start = reader.position();
    let mut surface_slots = ctx
        .collection_vec(face_count, "Rhino legacy Brep surface slots")
        .map_err(crate::curves::GeometryError::from)?;
    for _ in 0..face_count {
        ctx.charge_work(1, "Rhino brep parse_legacy_major2 records")?;
        let start = reader.position();
        let mut surface_storage = ctx.reserve_scoped(0, "Rhino legacy Brep surface scratch")?;
        let _surface = surface_storage.with_storage(|| {
            crate::surfaces::read_nurbs_surface_prefix(
                ctx,
                &mut reader,
                crate::settings::MillimeterScale::IDENTITY,
            )
        })?;
        let surface_range = start..reader.position();
        surface_slots.push(Some(RawBrepChild {
            class_uuid: crate::surfaces::NURBS_SURFACE,
            class_data_range: surface_range.clone(),
            source_range: surface_range,
        }));
    }
    let surfaces_range = surfaces_start..reader.position();

    let mut loops = ctx
        .collection_vec(loop_count, "Rhino legacy Brep loops")
        .map_err(crate::curves::GeometryError::from)?;
    let mut trims = ctx
        .collection_vec(trim_count, "Rhino legacy Brep trims")
        .map_err(crate::curves::GeometryError::from)?;
    let mut faces = ctx
        .collection_vec(face_count, "Rhino legacy Brep faces")
        .map_err(crate::curves::GeometryError::from)?;
    let mut warnings = Diagnostics::new();
    for face_position in 0..face_count {
        ctx.charge_work(1, "Rhino brep parse_legacy_major2 records")?;
        let face_index = reader.i32()?;
        let _obsolete_material = reader.i32()?;
        let reversed_surface = reader.i32()?;
        let _face_type = reader.i32()?;
        let _face_bounds = bbox(ctx, &mut reader)?;
        let boundary_count = count(&mut reader, MAX_BREP_ITEMS)?;
        if boundary_count == 0 {
            return Err(error(
                reader.position(),
                "legacy Brep face has no boundary loops",
            ));
        }
        let mut face_loops = ctx
            .collection_vec(boundary_count, "Rhino legacy Brep face loops")
            .map_err(crate::curves::GeometryError::from)?;
        for _ in 0..boundary_count {
            ctx.charge_work(1, "Rhino brep parse_legacy_major2 records")?;
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
                -1 => RawLoopKind::Slit,
                0 => RawLoopKind::Outer,
                1 => RawLoopKind::Inner,
                _ => RawLoopKind::Unknown,
            };
            let mut loop_trim_indexes = ctx
                .collection_vec(trim_in_loop, "Rhino legacy Brep loop trims")
                .map_err(crate::curves::GeometryError::from)?;
            for _ in 0..trim_in_loop {
                ctx.charge_work(1, "Rhino brep parse_legacy_major2 records")?;
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
                let trim_position = trims.len();
                let trim_index = i32::try_from(trim_position)
                    .map_err(|_| error(trim_source_start, "legacy Brep trim index overflow"))?;
                if stored_trim_index != trim_index {
                    warnings.push_admitted(ctx, format_args!(
                        "legacy Brep trim index {stored_trim_index} disagrees with array position {trim_index}"
                    ))?;
                }
                let edge = match position(Some(edge_index)).filter(|edge| *edge < edge_count) {
                    Some(_) => Some(edge_index),
                    None => {
                        if has_edge != 0 {
                            return Err(error(
                                trim_source_start,
                                "legacy Brep managed trim edge is out of range",
                            ));
                        }
                        None
                    }
                };
                let curve = trim_index;
                let domain = c2_meta
                    .get(trim_position)
                    .ok_or_else(|| {
                        error(trim_source_start, "legacy Brep C2 index is out of range")
                    })?
                    .domain;
                if trims.len() >= trim_count {
                    ctx.reserve_vec(&mut trims, 1, "Rhino legacy Brep trims")?;
                }
                trims.push(RawBrepTrim {
                    index: trim_index,
                    curve: Some(curve),
                    proxy_domain: domain,
                    edge,
                    vertices: [-1, -1],
                    reversed_3d: reversed_3d != 0,
                    trim_type: if edge.is_none() {
                        RawTrimKind::Singular
                    } else {
                        RawTrimKind::Unknown
                    },
                    iso: RawTrimIso::None,
                    loop_index: actual_loop_index,
                    tolerances: [tolerance_2d, tolerance_2d],
                    domain,
                    proxy_reversed: false,
                    reserved: Vec::new(),
                    legacy_tolerances: [tolerance_2d, tolerance_3d],
                    source_range: trim_source_start..reader.position(),
                });
                loop_trim_indexes.push(trim_index);
            }
            if loops.len() >= loop_count {
                ctx.reserve_vec(&mut loops, 1, "Rhino legacy Brep loops")?;
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
            reversed_surface: reversed_surface != 0,
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

    let mut edge_trim_indexes = legacy_storage.with_storage(|| {
        ctx.collect_indexed_vec(edge_count, "Rhino legacy Brep edge-trim groups", |_| {
            Ok(Vec::<usize>::new())
        })
    })?;
    for (trim_index, trim) in ctx
        .admit_iter(&trims[..], "Rhino parse legacy major2 traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .enumerate()
    {
        if let Some(edge_index) = position(trim.edge).filter(|index| *index < edge_count) {
            let group = &mut edge_trim_indexes[edge_index];
            legacy_storage.with_storage(|| {
                ctx.reserve_vec(group, 1, "Rhino legacy Brep edge-trim indexes")
            })?;
            group.push(trim_index);
        }
    }
    let endpoint_count = trim_count.checked_mul(2).ok_or_else(|| {
        error(
            reader.position(),
            "legacy Brep trim endpoint count overflow",
        )
    })?;
    let mut parent_storage = ctx.reserve_scoped(0, "Rhino legacy Brep endpoint parents")?;
    let mut endpoint_parent = parent_storage.with_storage(|| {
        ctx.collect_indexed_vec(endpoint_count, "Rhino legacy Brep endpoint parents", Ok)
    })?;
    for loop_record in ctx
        .admit_iter(&loops[..], "Rhino parse legacy major2 traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        let Some(head) = loop_record.trims.first() else {
            continue;
        };
        for (index, last) in ctx
            .admit_iter(
                loop_record.trims.as_slice(),
                "Rhino legacy Brep trim ring traversal",
            )
            .map_err(cadmpeg_core::CodecError::from)?
            .enumerate()
        {
            let next = index + 1;
            let first = loop_record.trims.get(next).unwrap_or(head);
            let last = slot(ctx, *last, trims.len(), "legacy Brep loop trim")?;
            let first = slot(ctx, *first, trims.len(), "legacy Brep loop trim")?;
            legacy_union(
                ctx,
                &mut endpoint_parent,
                legacy_trim_endpoint(last, 1),
                legacy_trim_endpoint(first, 0),
            )?;
        }
    }
    for (trim_index, trim) in ctx
        .admit_iter(&trims[..], "Rhino parse legacy major2 traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .enumerate()
    {
        if trim.edge.is_none() {
            legacy_union(
                ctx,
                &mut endpoint_parent,
                legacy_trim_endpoint(trim_index, 0),
                legacy_trim_endpoint(trim_index, 1),
            )?;
        }
    }
    for trim_indexes in ctx
        .admit_iter(
            &(edge_trim_indexes)[..],
            "Rhino parse legacy major2 traversal",
        )
        .map_err(cadmpeg_core::CodecError::from)?
    {
        let Some(first) = trim_indexes.first() else {
            continue;
        };
        for trim_index in ctx
            .admit_iter(&trim_indexes[1..], "Rhino parse legacy major2 traversal")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            for edge_endpoint in 0..2 {
                legacy_union(
                    ctx,
                    &mut endpoint_parent,
                    legacy_trim_endpoint_for_edge(&trims[*first], *first, edge_endpoint),
                    legacy_trim_endpoint_for_edge(&trims[*trim_index], *trim_index, edge_endpoint),
                )?;
            }
        }
    }
    let mut root_vertices = legacy_storage.with_storage(|| {
        ctx.alloc_filled(endpoint_count, None, "Rhino legacy Brep root vertices")
    })?;
    let mut vertices = Vec::new();
    let mut endpoint_vertices = legacy_storage.with_storage(|| {
        ctx.collection_vec(endpoint_count, "Rhino legacy Brep endpoint vertices")
    })?;
    for endpoint in 0..endpoint_count {
        ctx.charge_work(1, "Rhino brep parse_legacy_major2 records")?;
        let root = legacy_find(ctx, &mut endpoint_parent, endpoint)?;
        let index = match root_vertices[root] {
            Some(index) => index,
            None => {
                let position_in_array = vertices.len();
                let index = i32::try_from(position_in_array)
                    .map_err(|_| error(reader.position(), "legacy Brep vertex index overflow"))?;
                legacy_storage.with_storage(|| {
                    ctx.reserve_vec(&mut vertices, 1, "Rhino legacy Brep vertices")
                })?;
                root_vertices[root] = Some(position_in_array);
                vertices.push(LegacyVertex {
                    vertex: RawBrepVertex {
                        index,
                        point: CoordinateLane::Derived([0.0; 3]),
                        edges: Vec::new(),
                        tolerance: 0.0,
                        source_range: 0..0,
                    },
                    point_sum: [0.0; 3],
                    point_scale: [0.0; 3],
                    point_count: 0,
                });
                position_in_array
            }
        };
        endpoint_vertices.push(index);
    }
    let mut vertex_index = None;
    let mut vertex_index_storage = ctx.reserve_scoped(0, "Rhino legacy Brep vertex index")?;
    let mut edges = ctx
        .collection_vec(edge_count, "Rhino legacy Brep edges")
        .map_err(crate::curves::GeometryError::from)?;
    for (edge_index, curve) in ctx
        .admit_iter(&c3_meta[..], "Rhino parse legacy major2 traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .enumerate()
    {
        let endpoints = if let Some(trim_index) = edge_trim_indexes[edge_index].first() {
            let trim = &trims[*trim_index];
            [
                endpoint_vertices[legacy_trim_endpoint_for_edge(trim, *trim_index, 0)],
                endpoint_vertices[legacy_trim_endpoint_for_edge(trim, *trim_index, 1)],
            ]
        } else {
            let start = legacy_storage.with_storage(|| {
                legacy_vertex(
                    ctx,
                    &mut vertices,
                    &mut vertex_index,
                    &mut vertex_index_storage,
                    curve.endpoints[0],
                    curve.range.start,
                )
            })?;
            let end = legacy_storage.with_storage(|| {
                legacy_vertex(
                    ctx,
                    &mut vertices,
                    &mut vertex_index,
                    &mut vertex_index_storage,
                    curve.endpoints[1],
                    curve.range.start,
                )
            })?;
            [start, end]
        };
        for (vertex, point) in endpoints
            .into_iter()
            .zip([curve.endpoints[0], curve.endpoints[1]])
        {
            let vertex = &mut vertices[vertex];
            vertex.add_point(point);
        }
        let edge_index_i32 = i32::try_from(edge_index)
            .map_err(|_| error(curve.range.start, "legacy Brep edge index overflow"))?;
        let trim_indexes = &edge_trim_indexes[edge_index];
        let tolerance = ctx
            .admit_iter(&trim_indexes[..], "Rhino parse legacy major2 traversal")
            .map_err(cadmpeg_core::CodecError::from)?
            .map(|trim| trims[*trim].legacy_tolerances[1])
            .filter(|value| value.is_finite() && *value >= 0.0)
            .fold(0.0, f64::max);
        let mut stored_trim_indexes = ctx
            .collection_vec(trim_indexes.len(), "Rhino legacy Brep edge trim references")
            .map_err(crate::curves::GeometryError::from)?;
        for trim in ctx
            .admit_iter(trim_indexes, "Rhino legacy edge trim traversal")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            stored_trim_indexes.push(
                i32::try_from(*trim)
                    .map_err(|_| error(curve.range.start, "legacy Brep trim index overflow"))?,
            );
        }
        let endpoints = [
            i32::try_from(endpoints[0])
                .map_err(|_| error(curve.range.start, "legacy Brep vertex index overflow"))?,
            i32::try_from(endpoints[1])
                .map_err(|_| error(curve.range.start, "legacy Brep vertex index overflow"))?,
        ];
        edges.push(RawBrepEdge {
            index: edge_index_i32,
            curve: edge_index_i32,
            proxy_reversed: false,
            proxy_domain: curve.domain,
            vertices: endpoints,
            trims: stored_trim_indexes,
            tolerance,
            domain: curve.domain,
            source_range: 0..0,
        });
    }
    let mut normalized_vertices = ctx
        .collection_vec(vertices.len(), "Rhino legacy Brep resolved vertices")
        .map_err(crate::curves::GeometryError::from)?;
    for vertex in ctx
        .admit_iter(vertices, "Rhino legacy normalized vertex traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        normalized_vertices.push(vertex.into_vertex().ok_or_else(|| {
            error(
                reader.position(),
                "legacy Brep vertex mean left the scaled endpoint range",
            )
        })?);
    }
    let mut vertices = normalized_vertices;
    for (trim_index, trim) in ctx
        .admit_iter(&mut trims[..], "Rhino legacy trim endpoint traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .enumerate()
    {
        trim.vertices = [
            i32::try_from(endpoint_vertices[legacy_trim_endpoint(trim_index, 0)])
                .map_err(|_| error(reader.position(), "legacy Brep vertex index overflow"))?,
            i32::try_from(endpoint_vertices[legacy_trim_endpoint(trim_index, 1)])
                .map_err(|_| error(reader.position(), "legacy Brep vertex index overflow"))?,
        ];
    }
    for edge in ctx
        .admit_iter(&edges[..], "Rhino parse legacy major2 traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        for vertex in edge.vertices {
            let vertex = slot(ctx, vertex, vertices.len(), "legacy Brep edge vertex")?;
            ctx.reserve_vec(
                &mut vertices[vertex].edges,
                1,
                "Rhino legacy Brep vertex edges",
            )?;
            vertices[vertex].edges.push(edge.index);
        }
    }
    for edge in ctx
        .admit_iter(&edges[..], "Rhino parse legacy major2 traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        let mut loop_counts = BTreeMap::new();
        let mut loop_storage = ctx.reserve_scoped(0, "Rhino legacy edge loop counts")?;
        for trim_index in ctx
            .admit_iter(&edge.trims[..], "Rhino legacy edge loop indexing")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            let trim_index = slot(ctx, *trim_index, trims.len(), "legacy Brep edge trim")?;
            loop_storage.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
                *ctx.entry_btree_map(
                    &mut loop_counts,
                    trims[trim_index].loop_index,
                    "Rhino legacy edge loop counts",
                )?
                .or_insert(0_usize) += 1;
                Ok(())
            })?;
        }
        for trim_index in ctx
            .admit_iter(&edge.trims[..], "Rhino parse legacy major2 traversal")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            let trim_index = slot(ctx, *trim_index, trims.len(), "legacy Brep edge trim")?;
            let same_loop = ctx
                .get_btree_map(
                    &loop_counts,
                    &trims[trim_index].loop_index,
                    "Rhino legacy edge loop counts",
                )?
                .copied()
                .unwrap_or(0);
            trims[trim_index].trim_type = if edge.trims.len() == 1 {
                RawTrimKind::Boundary
            } else if same_loop > 1 {
                RawTrimKind::Seam
            } else {
                RawTrimKind::Mated
            };
        }
    }
    for (vertex_index, vertex) in ctx
        .admit_iter(&mut vertices[..], "Rhino legacy vertex tolerance traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .enumerate()
    {
        let mut tolerance: f64 = 0.0;
        for edge_index in ctx
            .admit_iter(&vertex.edges[..], "Rhino parse legacy major2 traversal")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            let edge_index = slot(ctx, *edge_index, edges.len(), "legacy Brep vertex edge")?;
            let edge = &edges[edge_index];
            tolerance = tolerance.max(edge.tolerance);
            let endpoint = if position(Some(edge.vertices[0])) == Some(vertex_index) {
                0
            } else if position(Some(edge.vertices[1])) == Some(vertex_index) {
                1
            } else {
                continue;
            };
            let curve = slot(ctx, edge.curve, c3_meta.len(), "legacy Brep edge curve")?;
            let expected = c3_meta[curve].endpoints[endpoint];
            let delta = [
                vertex.point[0] - expected[0],
                vertex.point[1] - expected[1],
                vertex.point[2] - expected[2],
            ];
            tolerance = tolerance.max(delta[0].hypot(delta[1]).hypot(delta[2]));
        }
        vertex.tolerance = tolerance;
    }

    let (render_meshes, _) =
        read_legacy_mesh_sides(ctx, bytes, &mut reader, archive, face_count, &mut warnings)?;
    let analysis_meshes = if minor >= 1 {
        read_legacy_mesh_sides(ctx, bytes, &mut reader, archive, face_count, &mut warnings)?.0
    } else {
        Vec::new()
    };
    let skipped = reader.skip_remaining()?;
    if skipped != 0 {
        warnings.push_admitted(
            ctx,
            format_args!("legacy ON_Brep skipped {skipped} trailing bytes"),
        )?;
    }
    let mut c2_slots = ctx
        .collection_vec(c2_meta.len(), "Rhino legacy Brep C2 slots")
        .map_err(crate::curves::GeometryError::from)?;
    for curve in ctx
        .admit_iter(c2_meta, "Rhino legacy C2 slot traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        c2_slots.push(Some(curve.into_child()));
    }
    let mut c3_slots = ctx
        .collection_vec(c3_meta.len(), "Rhino legacy Brep C3 slots")
        .map_err(crate::curves::GeometryError::from)?;
    for curve in ctx
        .admit_iter(c3_meta, "Rhino legacy C3 slot traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        c3_slots.push(Some(curve.into_child()));
    }
    let mut raw = RawBrep {
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
        is_solid: RawSolidFlag::Unstamped,
        face_sides: Vec::new(),
        regions: Vec::new(),
        source_range: range,
    };
    match ValidatedRawBrep::validate(ctx, &mut raw) {
        Ok((resolved, mut validation_warnings)) => {
            validation_warnings.prepend(warnings);
            let validated = ValidatedRawBrep {
                raw,
                resolved,
                warnings: validation_warnings,
            };
            Ok(BrepParse::Valid(validated))
        }
        Err(error @ GeometryError::Codec(_)) => Err(error),
        Err(error) => Ok(BrepParse::SemanticInvalid {
            raw,
            error,
            warnings,
        }),
    }
}

fn legacy_curve_shape(
    ctx: &DecodeContext<'_>,
    decoded: &crate::curves::DecodedGeometry,
    offset: usize,
) -> Result<(Interval, [[f64; 3]; 2]), GeometryError> {
    let crate::curves::DecodedGeometry::Curve { curve } = decoded else {
        return Err(error(offset, "legacy Brep polycurve is not a curve"));
    };
    let domain = match curve {
        crate::curves::DecodedCurve::Compound {
            children,
            end_parameter,
            ..
        } => {
            let first = children
                .first()
                .ok_or_else(|| error(offset, "legacy Brep polycurve has no parameter range"))?;
            Interval(FiniteVector::from([first.0, *end_parameter]))
        }
        crate::curves::DecodedCurve::Leaf { .. } => {
            return Err(error(
                offset,
                "legacy Brep polycurve has no parameter range",
            ));
        }
    };
    let endpoints = legacy_decoded_curve_endpoints(ctx, curve, offset)?;
    Ok((domain, endpoints))
}

fn legacy_decoded_curve_endpoints(
    ctx: &DecodeContext<'_>,
    curve: &crate::curves::DecodedCurve,
    offset: usize,
) -> Result<[[f64; 3]; 2], GeometryError> {
    let mut endpoints = [[0.0; 3]; 2];
    for (side, endpoint) in endpoints.iter_mut().enumerate() {
        let mut cursor = curve;
        let geometry = loop {
            ctx.charge_work(1, "Rhino legacy Brep endpoint path")?;
            match cursor {
                crate::curves::DecodedCurve::Compound { children, .. } => {
                    cursor = if side == 0 {
                        &children
                            .first()
                            .ok_or_else(|| {
                                error(offset, "legacy Brep polycurve has no first segment")
                            })?
                            .1
                    } else {
                        &children
                            .last()
                            .ok_or_else(|| {
                                error(offset, "legacy Brep polycurve has no last segment")
                            })?
                            .1
                    };
                }
                crate::curves::DecodedCurve::Leaf { geometry, .. } => break geometry,
            }
        };
        *endpoint = match geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                let index = if side == 0 {
                    0
                } else {
                    nurbs.pole_count().saturating_sub(1)
                };
                let message = if side == 0 {
                    "legacy Brep curve has no first pole"
                } else {
                    "legacy Brep curve has no last pole"
                };
                let point = nurbs
                    .pole_rows()
                    .point_at(index)
                    .ok_or_else(|| error(offset, message))?;
                [point.x, point.y, point.z]
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                let center = circle_curve.center().get();
                let ref_direction = circle_curve.frame().reference().as_raw();
                let radius = circle_curve.radius().get();
                [
                    center.x + ref_direction.x * radius,
                    center.y + ref_direction.y * radius,
                    center.z + ref_direction.z * radius,
                ]
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(degenerate_curve)) => {
                let point = degenerate_curve.point().get();
                [point.x, point.y, point.z]
            }
            _ => return Err(error(offset, "legacy Brep curve has no finite endpoints")),
        };
    }
    Ok(endpoints)
}

fn legacy_trim_endpoint(trim_index: usize, endpoint: usize) -> usize {
    trim_index * 2 + endpoint
}

fn legacy_trim_endpoint_for_edge(
    trim: &RawBrepTrim,
    trim_index: usize,
    edge_endpoint: usize,
) -> usize {
    let trim_endpoint = if trim.reversed_3d {
        1 - edge_endpoint
    } else {
        edge_endpoint
    };
    legacy_trim_endpoint(trim_index, trim_endpoint)
}

fn legacy_find(
    ctx: &DecodeContext<'_>,
    parent: &mut [usize],
    mut index: usize,
) -> Result<usize, cadmpeg_core::CodecError> {
    while parent[index] != index {
        ctx.charge_work(1, "Rhino legacy Brep endpoint root walk")?;
        parent[index] = parent[parent[index]];
        index = parent[index];
    }
    Ok(index)
}

fn legacy_union(
    ctx: &DecodeContext<'_>,
    parent: &mut [usize],
    left: usize,
    right: usize,
) -> Result<(), cadmpeg_core::CodecError> {
    let left_root = legacy_find(ctx, parent, left)?;
    let right_root = legacy_find(ctx, parent, right)?;
    if left_root != right_root {
        parent[right_root] = left_root;
    }
    Ok(())
}

fn legacy_point_key(point: [f64; 3]) -> Option<[u64; 3]> {
    if point.into_iter().any(f64::is_nan) {
        return None;
    }
    Some(point.map(|value| if value == 0.0 { 0 } else { value.to_bits() }))
}

/// The index of the legacy Brep vertex at `point`, adding it when the archive
/// states no vertex there yet.
///
/// The vertex index lane of a legacy Brep archive is an `i32`, so the rule
/// [`cadmpeg_core::decode::id_from_index`] states applies at that width: a
/// position the lane cannot state is a refusal, and the refusal names the
/// instance it happened at. `position` is what names it — the archive offset
/// this vertex was read from. It has no other use, and it is a parameter
/// because the vertex list alone does not carry it.
fn legacy_vertex(
    ctx: &DecodeContext<'_>,
    vertices: &mut Vec<LegacyVertex>,
    point_index: &mut Option<BTreeMap<[u64; 3], usize>>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    point: [f64; 3],
    position: usize,
) -> Result<usize, GeometryError> {
    if point_index.is_none() {
        let mut points = BTreeMap::new();
        for (position, vertex) in ctx
            .admit_iter(&vertices[..], "Rhino legacy Brep vertex indexing")
            .map_err(cadmpeg_core::CodecError::from)?
            .enumerate()
        {
            if let Some(key) = legacy_point_key(vertex.vertex.point.get()) {
                storage.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
                    ctx.entry_btree_map(&mut points, key, "Rhino legacy Brep vertex index")?
                        .or_insert(position);
                    Ok(())
                })?;
            }
        }
        *point_index = Some(points);
    }
    let key = legacy_point_key(point);
    if let (Some(key), Some(points)) = (key.as_ref(), point_index.as_ref()) {
        if let Some(position) = ctx.get_btree_map(points, key, "Rhino legacy Brep vertex index")? {
            return Ok(*position);
        }
    }
    let index = vertices.len();
    let stored_index =
        i32::try_from(index).map_err(|_| error(position, "legacy Brep vertex index overflow"))?;
    ctx.reserve_vec(vertices, 1, "Rhino legacy Brep vertices")?;
    vertices.push(LegacyVertex {
        vertex: RawBrepVertex {
            index: stored_index,
            point: CoordinateLane::Derived(point),
            edges: Vec::new(),
            tolerance: 0.0,
            source_range: 0..0,
        },
        point_sum: [0.0; 3],
        point_scale: [0.0; 3],
        point_count: 0,
    });
    if let (Some(key), Some(points)) = (key, point_index.as_mut()) {
        storage.with_storage(|| {
            ctx.insert_btree_map(points, key, index, "Rhino legacy Brep vertex index")
        })?;
    }
    Ok(index)
}

fn read_legacy_mesh_sides(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    face_count: usize,
    warnings: &mut Diagnostics,
) -> Result<(Vec<Option<RawBrepMesh>>, Range<usize>), GeometryError> {
    let start = reader.position();
    let mut slot_storage = ctx.reserve_scoped(0, "Rhino legacy Brep mesh slot scratch")?;
    let mut slots = slot_storage
        .with_storage(|| ctx.collection_vec(face_count, "Rhino legacy Brep mesh slots"))?;
    for _ in 0..face_count {
        ctx.charge_work(1, "Rhino brep read_legacy_mesh_sides records")?;
        let present = match reader.u8() {
            Ok(value) => value != 0,
            Err(error) => {
                reader.skip_remaining()?;
                let degraded = empty_mesh_slots(ctx, face_count)?;
                warnings.push_coded_admitted(
                    ctx,
                    crate::loss::RhinoLossCode::BrepMeshCacheDegraded,
                    format_args!("legacy Brep mesh cache degraded: {error}"),
                )?;
                return Ok((degraded, start..reader.position()));
            }
        };
        let mesh = if present {
            let object_start = reader.position();
            let object = match chunk_at(bytes, object_start, reader.end(), archive, false) {
                Ok(object) => object,
                Err(error) => {
                    reader.skip_remaining()?;
                    let degraded = empty_mesh_slots(ctx, face_count)?;
                    warnings.push_coded_admitted(
                        ctx,
                        crate::loss::RhinoLossCode::BrepMeshCacheDegraded,
                        format_args!("legacy Brep mesh cache degraded: {error}"),
                    )?;
                    return Ok((degraded, start..reader.position()));
                }
            };
            if let Err(error) = reader.skip(object.next_offset() - object_start) {
                reader.skip_remaining()?;
                let degraded = empty_mesh_slots(ctx, face_count)?;
                warnings.push_coded_admitted(
                    ctx,
                    crate::loss::RhinoLossCode::BrepMeshCacheDegraded,
                    format_args!("legacy Brep mesh cache degraded: {error}"),
                )?;
                return Ok((degraded, start..reader.position()));
            }
            match parse_class_wrapper_with_userdata(ctx, bytes, object.range(), archive, warnings) {
                Ok((class, userdata)) if supported_mesh(class.class_uuid) => Some(RawBrepMesh {
                    mesh: RawBrepChild {
                        class_uuid: class.class_uuid,
                        class_data_range: class.class_data_range,
                        source_range: object_start..object.next_offset(),
                    },
                    userdata,
                }),
                Ok(_) => {
                    warnings.push_coded_admitted(
                        ctx,
                        crate::loss::RhinoLossCode::BrepMeshCacheDegraded,
                        format_args!("legacy Brep mesh cache slot has wrong class"),
                    )?;
                    None
                }
                Err(FramingError::Resource(limit)) => {
                    return Err(GeometryError::Codec(
                        cadmpeg_core::CodecError::ResourceLimit(limit),
                    ));
                }
                Err(error) => {
                    warnings.push_coded_admitted(
                        ctx,
                        crate::loss::RhinoLossCode::BrepMeshCacheDegraded,
                        format_args!("legacy Brep mesh cache slot degraded: {error}"),
                    )?;
                    None
                }
            }
        } else {
            None
        };
        slots.push(mesh);
    }
    slot_storage.commit()?;
    Ok((slots, start..reader.position()))
}

fn empty_mesh_slots(
    ctx: &DecodeContext<'_>,
    count: usize,
) -> Result<Vec<Option<RawBrepMesh>>, GeometryError> {
    Ok(ctx.collect_indexed_vec(count, "Rhino legacy Brep degraded mesh slots", |_| Ok(None))?)
}

/// Returns whether a UUID is `ON_Brep`.
pub(crate) fn supported_class(uuid: Uuid) -> bool {
    matches!(
        uuid,
        ON_BREP | LEGACY_TRIMMED_SURFACE | LEGACY_BREP | TL_BREP
    )
}

fn read_children(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    expected_type: RawBrepBaseType,
    warnings: &mut Diagnostics,
) -> Result<RawBrepChildren, GeometryError> {
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
    let mut range_storage = ctx.reserve_scoped(0, "Rhino Brep child ranges")?;
    let mut direct_ranges =
        range_storage.with_storage(|| ctx.collection_vec(count + 1, "Rhino Brep child ranges"))?;
    direct_ranges.push(version_offset..child_reader.position());
    let mut slots = ctx
        .collection_vec(count, "Rhino Brep child slots")
        .map_err(crate::curves::GeometryError::from)?;
    for _ in 0..count {
        ctx.charge_work(1, "Rhino brep read_children records")?;
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
                    parse_class_wrapper(ctx, bytes, child_chunk.range(), archive, warnings)?;
                child_reader.skip(child_end - child_start)?;
                slots.push(Some(RawBrepChild {
                    class_uuid: class.class_uuid,
                    class_data_range: class.class_data_range,
                    source_range: child_start..child_end,
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
        ctx,
        bytes,
        reader,
        &chunk,
        child_reader,
        direct_ranges.iter().map(Ok),
        warnings,
    )?;
    Ok(RawBrepChildren {
        slots,
        source_range: start..chunk.next_offset(),
        expected_type,
    })
}

fn read_vertices(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<(Vec<RawBrepVertex>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let count = raw_array_start(ctx, &mut child, "vertex", 40)?;
    let mut result = ctx
        .collection_vec(count, "Rhino Brep vertices")
        .map_err(crate::curves::GeometryError::from)?;
    for _ in 0..count {
        ctx.charge_work(1, "Rhino brep read_vertices records")?;
        let start = child.position();
        let index = child.i32()?;
        let point = point(&mut child)?;
        let edges = indexes(ctx, &mut child)?;
        let tolerance = child.f64()?;
        result.push(RawBrepVertex {
            index,
            point: CoordinateLane::Admitted(point.0),
            edges,
            tolerance,
            source_range: start..child.position(),
        });
    }
    let range = chunk.range();
    finish_anonymous(ctx, bytes, reader, &chunk, child, warnings)?;
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
    ctx: &DecodeContext<'_>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    count: usize,
    field: &str,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), GeometryError> {
    if archive.value() >= 3 && writer_version.is_none() && count > 0 {
        ctx.reserve_vec(losses, 1, "Rhino Brep unstamped layout losses")?;
        losses.push(crate::wire::admitted_loss(
            ctx,
            crate::loss::RhinoLossCode::SourceWriterStampUnverified,
            format_args!("Brep {field} read with the pre-2002 layout for {count} records because the archive has no writer-version stamp"),
            "Rhino Brep unstamped layout loss text",
        )?);
    }
    Ok(())
}

fn read_edges(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    warnings: &mut Diagnostics,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(Vec<RawBrepEdge>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let count = raw_array_start(ctx, &mut child, "edge", 44)?;
    let current = archive.value() >= 3 && writer_version.is_some_and(|v| v >= 200_206_180);
    unstamped_legacy_layout(ctx, archive, writer_version, count, "edge domains", losses)?;
    let mut result = ctx
        .collection_vec(count, "Rhino Brep edges")
        .map_err(crate::curves::GeometryError::from)?;
    for _ in 0..count {
        ctx.charge_work(1, "Rhino brep read_edges records")?;
        let start = child.position();
        let index = child.i32()?;
        let curve = child.i32()?;
        let proxy_reversed = match child.i32()? {
            0 => false,
            1 => true,
            _ => return Err(error(child.position() - 4, "invalid edge proxy reversal")),
        };
        let proxy_domain = interval(ctx, &mut child)?;
        let vertices = [child.i32()?, child.i32()?];
        let trims = indexes(ctx, &mut child)?;
        let tolerance = child.f64()?;
        let domain = if current {
            interval(ctx, &mut child)?
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
    finish_anonymous(ctx, bytes, reader, &chunk, child, warnings)?;
    Ok((result, range))
}

fn read_trims(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    warnings: &mut Diagnostics,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(Vec<RawBrepTrim>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let count = raw_array_start(ctx, &mut child, "trim", 132)?;
    let current = archive.value() >= 3 && writer_version.is_some_and(|v| v >= 200_206_180);
    unstamped_legacy_layout(
        ctx,
        archive,
        writer_version,
        count,
        "trim domains and proxy senses",
        losses,
    )?;
    let mut result = ctx
        .collection_vec(count, "Rhino Brep trims")
        .map_err(crate::curves::GeometryError::from)?;
    for _ in 0..count {
        ctx.charge_work(1, "Rhino brep read_trims records")?;
        let start = child.position();
        let index = child.i32()?;
        let curve = child.i32().map(|value| (value != -1).then_some(value))?;
        let proxy_domain = interval(ctx, &mut child)?;
        let edge = child.i32().map(|value| (value != -1).then_some(value))?;
        let vertices = [child.i32()?, child.i32()?];
        let reversed_3d = match child.i32()? {
            0 => false,
            1 => true,
            _ => return Err(error(child.position() - 4, "invalid trim reversal")),
        };
        let trim_type = RawTrimKind::parse(child.i32()?)
            .ok_or_else(|| error(child.position() - 4, "invalid trim enum value"))?;
        let iso = RawTrimIso::parse(child.i32()?)
            .ok_or_else(|| error(child.position() - 4, "invalid trim enum value"))?;
        let loop_index = child.i32()?;
        let tolerances = [child.f64()?, child.f64()?];
        let (domain, proxy_reversed, reserved) = if current {
            let domain = interval(ctx, &mut child)?;
            let proxy_reversed = match child.u8()? {
                0 => false,
                1 => true,
                _ => return Err(error(child.position() - 1, "invalid trim reversal")),
            };
            let reserved = ctx.copy_retained(child.take(31)?, "Rhino Brep trim reserved bytes")?;
            (domain, proxy_reversed, reserved)
        } else {
            child.skip(48)?;
            (proxy_domain, false, Vec::new())
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
    finish_anonymous(ctx, bytes, reader, &chunk, child, warnings)?;
    Ok((result, range))
}

fn read_loops(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<(Vec<RawBrepLoop>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let count = raw_array_start(ctx, &mut child, "loop", 20)?;
    let mut result = ctx
        .collection_vec(count, "Rhino Brep loops")
        .map_err(crate::curves::GeometryError::from)?;
    for _ in 0..count {
        ctx.charge_work(1, "Rhino brep read_loops records")?;
        let start = child.position();
        let index = child.i32()?;
        let trims = indexes(ctx, &mut child)?;
        let loop_type = RawLoopKind::parse(child.i32()?)
            .ok_or_else(|| error(child.position() - 4, "invalid loop enum value"))?;
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
    finish_anonymous(ctx, bytes, reader, &chunk, child, warnings)?;
    Ok((result, range))
}

fn read_faces(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
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
    let mut result = ctx
        .collection_vec(count, "Rhino Brep faces")
        .map_err(crate::curves::GeometryError::from)?;
    for _ in 0..count {
        ctx.charge_work(1, "Rhino brep read_faces records")?;
        let record_start = child.position();
        let index = child.i32()?;
        let loops = indexes(ctx, &mut child)?;
        let surface = child.i32()?;
        let reversed_surface = match child.i32()? {
            0 => false,
            1 => true,
            _ => return Err(error(child.position() - 4, "invalid face surface reversal")),
        };
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
        for face in ctx
            .admit_iter(&mut result[..], "Rhino face suffix traversal")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            face.uuid = Some(uuid(&mut child)?);
        }
    }
    if version & 0x0f >= 2 {
        let present = child.u8()?;
        if present > 1 {
            return Err(error(child.position() - 1, "invalid face-color presence"));
        }
        if present != 0 {
            for face in ctx
                .admit_iter(&mut result[..], "Rhino face suffix traversal")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                face.color = Some(child.array::<4>()?);
            }
        }
    }
    let range = chunk.range();
    finish_anonymous(ctx, bytes, reader, &chunk, child, warnings)?;
    Ok((result, range))
}

fn read_mesh_sides(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    face_count: usize,
    warnings: &mut Diagnostics,
) -> Result<(Vec<Option<RawBrepMesh>>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let mut slot_storage = ctx.reserve_scoped(0, "Rhino Brep mesh slot scratch")?;
    let parsed: Result<(Vec<Option<RawBrepMesh>>, Range<usize>), GeometryError> = (|| {
        let mut result = slot_storage
            .with_storage(|| ctx.collection_vec(face_count, "Rhino Brep mesh cache slots"))?;
        let mut children = Vec::new();
        let mut range_storage = ctx.reserve_scoped(0, "Rhino Brep mesh cache child ranges")?;
        for _ in 0..face_count {
            ctx.charge_work(1, "Rhino Brep mesh-side traversal")?;
            let present = child.bool()?;
            let mesh = if present {
                let start = child.position();
                let object = chunk_at(bytes, start, child.end(), archive, false)?;
                range_storage.with_storage(|| {
                    ctx.reserve_vec(&mut children, 1, "Rhino Brep mesh cache child ranges")
                })?;
                children.push(object.range());
                let class = parse_class_wrapper_with_userdata(
                    ctx,
                    bytes,
                    object.range(),
                    archive,
                    warnings,
                );
                child.skip(object.next_offset() - start)?;
                match class {
                    Ok((class, userdata)) if supported_mesh(class.class_uuid) => {
                        Some(RawBrepMesh {
                            mesh: RawBrepChild {
                                class_uuid: class.class_uuid,
                                class_data_range: class.class_data_range,
                                source_range: start..object.next_offset(),
                            },
                            userdata,
                        })
                    }
                    Ok(_) => {
                        warnings.push_coded_admitted(
                            ctx,
                            crate::loss::RhinoLossCode::BrepMeshCacheDegraded,
                            format_args!("Brep mesh cache slot has wrong class"),
                        )?;
                        None
                    }
                    Err(FramingError::Resource(limit)) => {
                        return Err(GeometryError::Codec(
                            cadmpeg_core::CodecError::ResourceLimit(limit),
                        ));
                    }
                    Err(error) => {
                        warnings.push_coded_admitted(
                            ctx,
                            crate::loss::RhinoLossCode::BrepMeshCacheDegraded,
                            format_args!("Brep mesh cache slot degraded: {error}"),
                        )?;
                        None
                    }
                }
            } else {
                None
            };
            result.push(mesh);
        }
        finish_anonymous_children(ctx, bytes, reader, &chunk, child, &children, warnings)?;
        Ok((result, chunk.range()))
    })();
    match parsed {
        Ok(result) => {
            slot_storage.commit()?;
            Ok(result)
        }
        Err(error @ GeometryError::Codec(_)) => Err(error),
        Err(error) => {
            let degraded =
                ctx.collect_indexed_vec(face_count, "Rhino Brep degraded mesh slots", |_| {
                    Ok(None)
                })?;
            reader.skip(chunk.next_offset() - reader.position())?;
            warnings.push_coded_admitted(
                ctx,
                crate::loss::RhinoLossCode::BrepMeshCacheDegraded,
                format_args!("Brep mesh cache degraded: {error}"),
            )?;
            Ok((degraded, chunk.range()))
        }
    }
}

fn read_regions(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    face_count: usize,
    warnings: &mut Diagnostics,
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
        let sides = read_region_sides(ctx, bytes, &mut topology, archive, warnings)?;
        let sides_range = sides_start..topology.position();
        let regions_start = topology.position();
        let regions = read_region_records(ctx, bytes, &mut topology, archive, warnings)?;
        let regions_range = regions_start..topology.position();
        finish_anonymous_children(
            ctx,
            bytes,
            &mut outer,
            &nested_chunk,
            topology,
            &[sides_range, regions_range],
            warnings,
        )?;
        if face_count.checked_mul(2) != Some(sides.len()) {
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
            let mut ranges = ctx.reserve_scoped(0, "Rhino region checksum ranges")?;
            let direct = ranges.with_storage(|| {
                crate::chunks::direct_checksum_ranges(ctx, &chunk.body(), nested.as_slice())
            })?;
            if matches!(
                verify_checksum_ranges(ctx, bytes, &chunk, &direct)?,
                ChecksumStatus::Mismatch { .. }
            ) {
                warnings.push_coded_admitted(
                    ctx,
                    crate::loss::RhinoLossCode::IntegrityFailure,
                    format_args!("Brep region wrapper checksum mismatch"),
                )?;
            }
            Ok((sides, regions, Some(chunk.range()), inline_region_loaded))
        }
        Err(error @ GeometryError::Codec(_)) => Err(error),
        Err(error) => {
            warnings.push_coded_admitted(
                ctx,
                crate::loss::RhinoLossCode::RedundantFieldRepaired,
                format_args!("invalid optional Brep region topology discarded: {error}"),
            )?;
            Ok((Vec::new(), Vec::new(), Some(chunk.range()), false))
        }
    }
}

fn read_region_topology_userdata(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    extra: &ClassUserdata,
    archive: ArchiveVersion,
    face_count: usize,
    warnings: &mut Diagnostics,
) -> Result<RegionRead, GeometryError> {
    let mut parent = BoundedReader::new(bytes, extra.payload_range.start, extra.payload_range.end)?;
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
    let sides = read_region_sides(ctx, bytes, &mut topology, archive, warnings)?;
    let sides_range = sides_start..topology.position();
    let regions_start = topology.position();
    let regions = read_region_records(ctx, bytes, &mut topology, archive, warnings)?;
    let regions_range = regions_start..topology.position();
    finish_anonymous_children(
        ctx,
        bytes,
        &mut parent,
        &topology_chunk,
        topology,
        &[sides_range, regions_range],
        warnings,
    )?;
    let skipped = parent.skip_remaining()?;
    if skipped != 0 {
        warnings.push_admitted(
            ctx,
            format_args!("Brep region-topology userdata skipped {skipped} trailing bytes"),
        )?;
    }
    if face_count.checked_mul(2) != Some(sides.len()) {
        return Err(error(
            extra.range.start,
            "redundant Brep region face-side count mismatch",
        ));
    }
    Ok((sides, regions, Some(extra.range.clone()), true))
}

fn read_region_sides<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<Vec<RawBrepFaceSide>, GeometryError> {
    let (chunk, mut child, count) = region_array(bytes, reader, archive)?;
    let mut result = ctx
        .collection_vec(count, "Rhino Brep region face sides")
        .map_err(crate::curves::GeometryError::from)?;
    let mut range_storage = ctx.reserve_scoped(0, "Rhino Brep region side ranges")?;
    let mut children = range_storage
        .with_storage(|| ctx.collection_vec(count, "Rhino Brep region side ranges"))?;
    for _ in 0..count {
        ctx.charge_work(1, "Rhino brep read_region_sides records")?;
        let (body, source) = region_element(ctx, bytes, &mut child, archive, ON_BREP_FACE_SIDE)?;
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
    finish_anonymous_children(ctx, bytes, reader, &chunk, child, &children, warnings)?;
    Ok(result)
}

fn read_region_records<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<Vec<RawBrepRegion>, GeometryError> {
    let (chunk, mut child, count) = region_array(bytes, reader, archive)?;
    let mut result = ctx
        .collection_vec(count, "Rhino Brep region records")
        .map_err(crate::curves::GeometryError::from)?;
    let mut range_storage = ctx.reserve_scoped(0, "Rhino Brep region record ranges")?;
    let mut children = range_storage
        .with_storage(|| ctx.collection_vec(count, "Rhino Brep region record ranges"))?;
    let mut index_mismatch = false;
    for position in 0..count {
        ctx.charge_work(1, "Rhino brep read_region_records records")?;
        let (body, source) = region_element(ctx, bytes, &mut child, archive, ON_BREP_REGION)?;
        children.push(source.clone());
        let mut child = BoundedReader::new(bytes, body.start, body.end)?;
        let index = child.i32()?;
        index_mismatch |= usize::try_from(index).ok() != Some(position);
        let region_type = child.i32()?;
        let sides = indexes(ctx, &mut child)?;
        let bounds = bbox(ctx, &mut child)?;
        child.skip_remaining()?;
        result.push(RawBrepRegion {
            region_type,
            sides,
            bounds,
            source_range: source,
        });
    }
    finish_anonymous_children(ctx, bytes, reader, &chunk, child, &children, warnings)?;
    if index_mismatch {
        warnings.push_coded_admitted(
            ctx,
            crate::loss::RhinoLossCode::RedundantFieldRepaired,
            format_args!(
                "redundant Brep region positional index mismatch; serialized array order used"
            ),
        )?;
    }
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
    ctx: &DecodeContext<'_>,
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
        let mut wrapper_storage = ctx.reserve_scoped(0, "Rhino region wrapper diagnostics")?;
        let class = wrapper_storage.with_storage(|| {
            parse_class_wrapper(ctx, bytes, chunk.range(), archive, &mut Diagnostics::new())
        })?;
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

fn validate_rings(
    ctx: &DecodeContext<'_>,
    raw: &RawBrep,
    resolved: &ResolvedBrep,
) -> Result<(), GeometryError> {
    for (loop_index, loop_record) in ctx
        .admit_iter(raw.loops.as_slice(), "Rhino Brep ring loop traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .enumerate()
    {
        let ring = &resolved.loops[loop_index].trims;
        let Some(first_trim) = ring.first() else {
            return Err(error(loop_record.source_range.start, "loop ring is empty"));
        };
        if matches!(
            loop_record.loop_type,
            RawLoopKind::CurveOnSurface | RawLoopKind::PointOnSurface
        ) {
            let trim = &raw.trims[*first_trim];
            let expected_trim_type = if loop_record.loop_type == RawLoopKind::CurveOnSurface {
                RawTrimKind::CurveOnSurface
            } else {
                RawTrimKind::PointOnSurface
            };
            if ring.len() != 1 || trim.trim_type != expected_trim_type {
                return Err(error(
                    loop_record.source_range.start,
                    "procedural Brep loop must contain its matching single trim",
                ));
            }
            continue;
        }
        for pair in ctx
            .admit_iter(ring.as_slice(), "Rhino Brep ring adjacency traversal")
            .map_err(cadmpeg_core::CodecError::from)?
            .windows(
                std::num::NonZeroUsize::new(2)
                    .ok_or_else(|| ctx.refuse_codec_limit("Rhino Brep ring window width", 0, 1))?,
            )
        {
            let left = &resolved.trims[pair[0]];
            let right = &resolved.trims[pair[1]];
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
        let first = &resolved.trims[*first_trim];
        let last_trim = ring
            .last()
            .copied()
            .ok_or_else(|| error(loop_record.source_range.start, "loop ring is empty"))?;
        let last = &resolved.trims[last_trim];
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

fn validate_regions(
    ctx: &DecodeContext<'_>,
    raw: &RawBrep,
) -> Result<Vec<ResolvedFaceSide>, GeometryError> {
    if raw.faces.len().checked_mul(2) != Some(raw.face_sides.len()) {
        return Err(error(
            raw.source_range.start,
            "region side count is invalid",
        ));
    }
    let mut infinite = 0;
    let mut side_storage = ctx.reserve_scoped(0, "Rhino resolved Brep region sides")?;
    let mut sides = side_storage.with_storage(|| {
        ctx.collection_vec(raw.face_sides.len(), "Rhino resolved Brep region sides")
    })?;
    for (index, side) in ctx
        .admit_iter(&raw.face_sides[..], "Rhino validate regions traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .enumerate()
    {
        let Some(face) = position(Some(side.face)).filter(|face| *face < raw.faces.len()) else {
            return Err(error(
                side.source_range.start,
                "region face-side index is invalid",
            ));
        };
        let expected = if index % 2 == 0 { 1 } else { -1 };
        if side.direction != expected {
            return Err(error(
                side.source_range.start,
                "region side direction is invalid",
            ));
        }
        if face != index / 2 {
            return Err(error(
                side.source_range.start,
                "region side face position is invalid",
            ));
        }
        let region = match side.region {
            -1 => None,
            value => {
                let Some(region) =
                    position(Some(value)).filter(|region| *region < raw.regions.len())
                else {
                    return Err(error(
                        side.source_range.start,
                        "region membership is invalid",
                    ));
                };
                Some(region)
            }
        };
        sides.push(ResolvedFaceSide { face, region });
    }
    let mut listed_storage = ctx.reserve_scoped(0, "Rhino Brep listed region sides")?;
    let mut listed_sides = listed_storage
        .with_storage(|| ctx.alloc_filled(sides.len(), false, "Rhino Brep listed region sides"))?;
    for (index, region) in ctx
        .admit_iter(&raw.regions[..], "Rhino validate regions traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .enumerate()
    {
        if !matches!(region.region_type, 0 | 1) {
            return Err(error(region.source_range.start, "region record is invalid"));
        }
        if region.region_type == 0 {
            infinite += 1;
        }
        for side in ctx
            .admit_iter(&region.sides[..], "Rhino validate regions traversal")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            let side = slot(ctx, *side, raw.face_sides.len(), "region side")?;
            if std::mem::replace(&mut listed_sides[side], true) || sides[side].region != Some(index)
            {
                return Err(error(
                    region.source_range.start,
                    "region membership is not reciprocal",
                ));
            }
        }
    }
    if ctx.any_by(
        sides[..].iter().enumerate(),
        |(index, side)| Ok(side.region.is_some() && !listed_sides[index]),
        "Rhino validate regions traversal",
    )? {
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
    side_storage.commit()?;
    Ok(sides)
}

fn raw_array_start(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    label: &str,
    minimum_record_bytes: usize,
) -> Result<usize, GeometryError> {
    let version = reader.u8()?;
    if version >> 4 != 1 {
        return Err(GeometryError::unsupported(
            reader.position() - 1,
            ctx.format_retained(
                format_args!("unsupported {label} array version"),
                "Rhino raw_array_start text",
            )?,
        ));
    }
    let count = count(reader, MAX_BREP_ITEMS)?;
    if count
        .checked_mul(minimum_record_bytes)
        .is_none_or(|bytes| bytes > reader.remaining())
    {
        return Err(error(
            reader.position(),
            ctx.format_retained(
                format_args!("{label} count exhausts payload before allocation"),
                "Rhino raw_array_start text",
            )?,
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

fn indexes(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
) -> Result<Vec<i32>, GeometryError> {
    let count = count(reader, MAX_BREP_ITEMS)?;
    let mut result = ctx
        .collection_vec(count, "Rhino Brep indexes")
        .map_err(crate::curves::GeometryError::from)?;
    for _ in 0..count {
        ctx.charge_work(1, "Rhino brep indexes records")?;
        result.push(reader.i32()?);
    }
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

/// The array position a stored reference names, or `None` when it names none.
fn position(value: Option<i32>) -> Option<usize> {
    value.and_then(|value| usize::try_from(value).ok())
}

/// Resolves one stored reference against an array of `len` records.
fn slot(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: i32,
    len: usize,
    label: &str,
) -> Result<usize, GeometryError> {
    position(Some(value))
        .filter(|slot| *slot < len)
        .map_or_else(
            || {
                Err(GeometryError::unpositioned(ctx.format_retained(
                    format_args!("{label} reference is out of range"),
                    "Rhino slot text",
                )?))
            },
            Ok,
        )
}

/// Resolves a list of stored references against an array of `len` records.
fn slots(
    ctx: &DecodeContext<'_>,
    values: &[i32],
    len: usize,
    label: &str,
) -> Result<Vec<usize>, GeometryError> {
    let mut result = ctx
        .collection_vec(values.len(), "Rhino resolved Brep references")
        .map_err(crate::curves::GeometryError::from)?;
    for value in ctx
        .admit_iter(values, "Rhino slots traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        result.push(slot(ctx, *value, len, label)?);
    }
    Ok(result)
}

/// Resolves an endpoint pair against an array of `len` records.
fn slot_pair(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    values: [i32; 2],
    len: usize,
    label: &str,
) -> Result<[usize; 2], GeometryError> {
    Ok([
        slot(ctx, values[0], len, label)?,
        slot(ctx, values[1], len, label)?,
    ])
}

/// Resolves one child slot reference, requiring the expected base type.
fn child_slot(array: &RawBrepChildren, index: i32, expected: RawBrepBaseType) -> Option<usize> {
    let slot = position(Some(index))?;
    array
        .slots
        .get(slot)
        .and_then(Option::as_ref)
        .filter(|child| child.base_type() == expected)
        .map(|_| slot)
}

/// Reports whether any stored positional index disagrees with the array order.
fn positions_drifted<T>(
    ctx: &DecodeContext<'_>,
    values: &[T],
    index: impl Fn(&T) -> i32,
) -> Result<bool, CodecError> {
    ctx.any_by(
        values.iter().enumerate(),
        |(position_in_array, value)| Ok(position(Some(index(value))) != Some(position_in_array)),
        "Rhino Brep positional index traversal",
    )
}

fn validate_edge_incidences(
    ctx: &DecodeContext<'_>,
    raw: &RawBrep,
    resolved: &ResolvedBrep,
) -> Result<(), GeometryError> {
    let mut storage = ctx.reserve_scoped(0, "Rhino Brep endpoint incidence counts")?;
    let mut counts = storage.with_storage(|| {
        ctx.alloc_filled(
            resolved.edges.len(),
            [0_usize; 2],
            "Rhino Brep endpoint incidence counts",
        )
    })?;
    for (vertex_index, vertex) in ctx
        .admit_iter(
            &resolved.vertices[..],
            "Rhino Brep incidence vertex traversal",
        )
        .map_err(CodecError::from)?
        .enumerate()
    {
        for edge_index in ctx
            .admit_iter(&vertex.edges[..], "Rhino Brep vertex incidence traversal")
            .map_err(CodecError::from)?
        {
            let endpoints = resolved.edges[*edge_index].vertices;
            for endpoint in 0..2 {
                if endpoints[endpoint] == vertex_index {
                    counts[*edge_index][endpoint] += 1;
                }
            }
        }
    }
    for (edge_index, edge) in ctx
        .admit_iter(
            resolved.edges.as_slice(),
            "Rhino Brep edge incidence traversal",
        )
        .map_err(cadmpeg_core::CodecError::from)?
        .enumerate()
    {
        for trim_index in ctx
            .admit_iter(
                edge.trims.as_slice(),
                "Rhino Brep edge trim incidence traversal",
            )
            .map_err(cadmpeg_core::CodecError::from)?
        {
            let trim = &resolved.trims[*trim_index];
            if trim.edge.is_some()
                && !((trim.vertices[0] == edge.vertices[0] && trim.vertices[1] == edge.vertices[1])
                    || (trim.vertices[0] == edge.vertices[1]
                        && trim.vertices[1] == edge.vertices[0]))
            {
                return Err(error(
                    raw.edges[edge_index].source_range.start,
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
            let count = counts[edge_index][endpoint];
            if count != expected {
                return Err(GeometryError::malformed(
                    raw.edges[edge_index].source_range.start,
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

fn unique(ctx: &DecodeContext<'_>, values: &[i32], label: &str) -> Result<(), GeometryError> {
    let mut seen = HashSet::new();
    let mut storage = ctx.reserve_scoped(0, "Rhino Brep unique references")?;
    for value in ctx
        .admit_iter(values, "Rhino unique traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        if !storage.with_storage(|| {
            ctx.insert_hash_set(&mut seen, *value, "Rhino Brep unique references")
        })? {
            return Err(GeometryError::unpositioned(ctx.format_retained(
                format_args!("{label} reference is duplicated"),
                "Rhino unique text",
            )?));
        }
    }
    Ok(())
}

/// Refuses a decoded interval that is neither an `ON_UNSET` pair nor ordered.
fn ordered_interval(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: Interval,
    label: &str,
) -> Result<(), GeometryError> {
    let [low, high] = value.0.get();
    let unset = (low == ON_UNSET_VALUE && high == ON_UNSET_VALUE)
        || (low == ON_UNSET_POSITIVE_VALUE && high == ON_UNSET_POSITIVE_VALUE);
    let empty = (low == ON_UNSET_VALUE && high == ON_UNSET_POSITIVE_VALUE)
        || (low == ON_UNSET_POSITIVE_VALUE && high == ON_UNSET_VALUE);
    if !(unset || empty || low < high) {
        return Err(GeometryError::unpositioned(ctx.format_retained(
            format_args!("{label} is invalid"),
            "Rhino ordered_interval text",
        )?));
    }
    Ok(())
}

fn finite_tolerance(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: f64,
    label: &str,
) -> Result<BrepTolerance, GeometryError> {
    BrepTolerance::new(value).map_or_else(
        || {
            Err(GeometryError::unpositioned(ctx.format_retained(
                format_args!("{label} is invalid"),
                "Rhino finite_tolerance text",
            )?))
        },
        Ok,
    )
}

fn point(reader: &mut BoundedReader<'_>) -> Result<Point3, GeometryError> {
    let point = [reader.f64()?, reader.f64()?, reader.f64()?];
    FiniteVector::new(point)
        .map(Point3)
        .ok_or_else(|| error(reader.position() - 24, "Brep point is not finite"))
}

fn supported_mesh(uuid: Uuid) -> bool {
    uuid == crate::mesh::ON_MESH
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
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    parent: &mut BoundedReader<'_>,
    chunk: &Chunk,
    child: BoundedReader<'_>,
    warnings: &mut Diagnostics,
) -> Result<(), GeometryError> {
    if child.remaining() != 0 {
        warnings.push_admitted(
            ctx,
            format_args!(
                "Brep anonymous chunk skipped {} trailing bytes",
                child.remaining()
            ),
        )?;
    }
    if matches!(
        verify_checksum(ctx, bytes, chunk)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push_coded_admitted(
            ctx,
            crate::loss::RhinoLossCode::IntegrityFailure,
            format_args!(
                "Brep anonymous CRC mismatch at offset {}",
                chunk.header_start
            ),
        )?;
    }
    parent.skip(chunk.next_offset() - parent.position())?;
    Ok(())
}

fn finish_anonymous_children(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    parent: &mut BoundedReader<'_>,
    chunk: &Chunk,
    child: BoundedReader<'_>,
    children: &[Range<usize>],
    warnings: &mut Diagnostics,
) -> Result<(), GeometryError> {
    let mut ranges = ctx.reserve_scoped(0, "Rhino brep checksum ranges")?;
    let direct = ranges
        .with_storage(|| crate::chunks::direct_checksum_ranges(ctx, &chunk.body(), children))?;
    finish_anonymous_ranges(ctx, bytes, parent, chunk, child, &direct, warnings)
}

fn finish_anonymous_ranges<I, R>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    parent: &mut BoundedReader<'_>,
    chunk: &Chunk,
    child: BoundedReader<'_>,
    direct_ranges: I,
    warnings: &mut Diagnostics,
) -> Result<(), GeometryError>
where
    I: Clone + IntoIterator<Item = Result<R, FramingError>>,
    R: std::borrow::Borrow<Range<usize>>,
{
    if child.remaining() != 0 {
        warnings.push_admitted(
            ctx,
            format_args!(
                "Brep anonymous chunk skipped {} trailing bytes",
                child.remaining()
            ),
        )?;
    }
    if matches!(
        verify_checksum_ranges(ctx, bytes, chunk, direct_ranges)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push_coded_admitted(
            ctx,
            crate::loss::RhinoLossCode::IntegrityFailure,
            format_args!(
                "Brep anonymous CRC mismatch at offset {}",
                chunk.header_start
            ),
        )?;
    }
    parent.skip(chunk.next_offset() - parent.position())?;
    Ok(())
}

#[cfg(test)]
mod tests;
