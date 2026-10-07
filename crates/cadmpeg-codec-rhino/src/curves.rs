// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino point and simple-curve payload decoding.

use crate::loss::Diagnostics;
use std::f64::consts::{FRAC_PI_2, TAU};
use std::ops::Range;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal, PositiveLength, PositiveReal};
use cadmpeg_ir::units::OrthonormalFrame3;

use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
use crate::objects::parse_class_wrapper;
use crate::settings::{bbox, interval, plane, MillimeterScale, Point3 as NativePoint3};
use crate::wire::{vector, Uuid};

const EPS_CURVE_POSITION: f64 = 1.0e-8;
const EPS_CURVE_DEGENERATE: f64 = 1.0e-10;

/// Maximum embedded curve nesting depth.
const MAX_CURVE_DEPTH: usize = 32;
/// Acceptance tolerance for circle plane axes and evaluated radius points.
const CIRCLE_TOLERANCE: f64 = EPS_CURVE_DEGENERATE;

const POINT: Uuid = Uuid::from_canonical([
    0xc3, 0x10, 0x1a, 0x1d, 0xf1, 0x57, 0x11, 0xd3, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const POINT_CLOUD: Uuid = Uuid::from_canonical([
    0x24, 0x88, 0xf3, 0x47, 0xf8, 0xfa, 0x11, 0xd3, 0xbf, 0xec, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const CURVE_PROXY: Uuid = Uuid::from_canonical([
    0x4e, 0xd7, 0xd4, 0xd9, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const CURVE_ON_SURFACE: Uuid = Uuid::from_canonical([
    0x4e, 0xd7, 0xd4, 0xd8, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const LINE: Uuid = Uuid::from_canonical([
    0x4e, 0xd7, 0xd4, 0xdb, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const ARC: Uuid = Uuid::from_canonical([
    0xcf, 0x33, 0xbe, 0x2a, 0x09, 0xb4, 0x11, 0xd4, 0xbf, 0xfb, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const POLYLINE: Uuid = Uuid::from_canonical([
    0x4e, 0xd7, 0xd4, 0xe6, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const POLYCURVE: Uuid = Uuid::from_canonical([
    0x4e, 0xd7, 0xd4, 0xe0, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const POLYCURVE_LEGACY: Uuid = Uuid::from_canonical([
    0xef, 0x63, 0x83, 0x17, 0x15, 0x4b, 0x11, 0xd4, 0x80, 0x00, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const NURBS_CURVE: Uuid = crate::surfaces::NURBS_CURVE;
const NURBS_CURVE_TL: Uuid = Uuid::from_canonical([
    0x5e, 0xaf, 0x11, 0x19, 0x0b, 0x51, 0x11, 0xd4, 0xbf, 0xfe, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const NURBS_CURVE_LEGACY: Uuid = Uuid::from_canonical([
    0x76, 0xa7, 0x09, 0xd5, 0x15, 0x50, 0x11, 0xd4, 0x80, 0x00, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const NURBS_SURFACE: Uuid = crate::surfaces::NURBS_SURFACE;
const NURBS_SURFACE_TL: Uuid = crate::surfaces::NURBS_SURFACE_TL;
const NURBS_SURFACE_LEGACY: Uuid = crate::surfaces::NURBS_SURFACE_LEGACY;
const PLANE_SURFACE: Uuid = crate::surfaces::PLANE_SURFACE;
const CLIPPING_PLANE_SURFACE: Uuid = crate::surfaces::CLIPPING_PLANE_SURFACE;
const REV_SURFACE: Uuid = crate::surfaces::REV_SURFACE;
const REV_SURFACE_LEGACY: Uuid = crate::surfaces::REV_SURFACE_LEGACY;
const SUM_SURFACE: Uuid = crate::surfaces::SUM_SURFACE;

/// A decoded point or curve before it is inserted into the IR arenas.
#[derive(Debug, Clone)]
pub(crate) enum DecodedGeometry {
    /// One point.
    Point {
        /// Decoded coordinates.
        position: FinitePoint3,
        /// Whether a unit conversion was applied.
        scaled: bool,
    },
    /// One point cloud; optional native channels are consumed by the bounded
    /// reader and retained by the source record rather than mapped to points.
    PointCloud(PointCloud),
    /// A curve and ordered embedded children.
    Curve {
        /// Decoded curve tree.
        curve: DecodedCurve,
    },
    /// A decoded surface carrier.
    Surface {
        /// Decoded surface geometry.
        surface: crate::surfaces::DecodedSurface,
    },
}

/// Point-cloud geometry transferred to the neutral model.
#[derive(Debug, Clone)]
pub(crate) struct PointCloud {
    /// Ordered points.
    pub(crate) points: Vec<FinitePoint3>,
    /// Whether a unit conversion was applied.
    pub(crate) scaled: bool,
    /// Repairs applied to optional channels that do not match the point count.
    pub(crate) warnings: Diagnostics,
}

/// A curve carrier or a recursive polycurve construction.
#[derive(Debug, Clone)]
pub(crate) enum DecodedCurve {
    /// Solved leaf geometry.
    Leaf {
        /// Solved carrier geometry.
        geometry: CurveGeometry,
        /// Non-fatal source warnings.
        warnings: Diagnostics,
    },
    /// Polycurve with one start parameter per child and a closing end parameter.
    Compound {
        /// Child curve trees with their start parameters.
        children: Vec<(FiniteReal, DecodedCurve)>,
        /// End parameter of the last child.
        end_parameter: FiniteReal,
        /// Non-fatal source warnings.
        warnings: Diagnostics,
    },
}

impl DecodedCurve {
    pub(crate) fn leaf(geometry: CurveGeometry, warnings: Diagnostics) -> Self {
        Self::Leaf { geometry, warnings }
    }

    pub(crate) fn warnings(&self) -> &Diagnostics {
        match self {
            Self::Leaf { warnings, .. } | Self::Compound { warnings, .. } => warnings,
        }
    }

    pub(crate) fn into_warnings(self) -> Diagnostics {
        match self {
            Self::Leaf { warnings, .. } | Self::Compound { warnings, .. } => warnings,
        }
    }

    fn warnings_mut(&mut self) -> &mut Diagnostics {
        match self {
            Self::Leaf { warnings, .. } | Self::Compound { warnings, .. } => warnings,
        }
    }

    pub(crate) fn reported_geometry(&self) -> &CurveGeometry {
        match self {
            Self::Leaf { geometry, .. } => geometry,
            Self::Compound { .. } => &UNKNOWN_REPORTED_CURVE,
        }
    }
}

static UNKNOWN_REPORTED_CURVE: CurveGeometry =
    CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None });

/// A semantic geometry error.
#[derive(Debug)]
pub(crate) enum GeometryError {
    /// A bounded payload uses a future or unsupported version.
    UnsupportedVersion { offset: usize, message: String },
    /// A bounded payload is malformed.
    Malformed(FramingError),
    /// A codec-level refusal that must leave geometry fallback.
    Codec(CodecError),
}

impl GeometryError {
    pub(crate) fn malformed(offset: usize, message: impl Into<String>) -> Self {
        Self::Malformed(FramingError::structural(offset, message))
    }

    /// Refuses a derived or already-decoded value that has no byte position.
    pub(crate) fn unpositioned(message: impl Into<String>) -> Self {
        Self::Malformed(FramingError::unpositioned(message))
    }

    pub(crate) fn unsupported(offset: usize, message: impl Into<String>) -> Self {
        Self::UnsupportedVersion {
            offset,
            message: message.into(),
        }
    }

    pub(crate) fn not_implemented(message: impl Into<String>) -> Self {
        Self::Codec(CodecError::NotImplemented(message.into()))
    }
}

impl std::fmt::Display for GeometryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion { offset, message } => {
                write!(formatter, "unsupported version at {offset}: {message}")
            }
            Self::Malformed(error) => error.fmt(formatter),
            Self::Codec(error) => error.fmt(formatter),
        }
    }
}

impl From<CodecError> for GeometryError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<FramingError> for GeometryError {
    fn from(error: FramingError) -> Self {
        match error {
            FramingError::Resource(limit) => Self::Codec(CodecError::ResourceLimit(limit)),
            other => Self::Malformed(other),
        }
    }
}

impl From<cadmpeg_core::decode::ParseError> for GeometryError {
    fn from(error: cadmpeg_core::decode::ParseError) -> Self {
        use cadmpeg_core::decode::ParseErrorKind;
        let Ok(offset) = usize::try_from(error.location.offset) else {
            return Self::Codec(cadmpeg_core::decode::refuse_local_limit(
                "Rhino parse error offset",
                cadmpeg_core::decode::u64_from_index(usize::MAX),
                error.location.offset,
            ));
        };
        match error.kind {
            ParseErrorKind::UnexpectedEof { needed, .. } => {
                Self::Malformed(FramingError::Truncated {
                    offset,
                    needed: match usize::try_from(needed) {
                        Ok(needed) => needed,
                        Err(_) => {
                            return Self::Codec(cadmpeg_core::decode::refuse_local_limit(
                                "Rhino parse error length",
                                cadmpeg_core::decode::u64_from_index(usize::MAX),
                                needed,
                            ))
                        }
                    },
                })
            }
            ParseErrorKind::InvalidValue => Self::Malformed(FramingError::Structural {
                offset,
                message: format!("invalid value during {}", error.operation),
            }),
            ParseErrorKind::InvalidFraming => Self::Malformed(FramingError::Structural {
                offset,
                message: format!("invalid framing during {}", error.operation),
            }),
        }
    }
}

/// Dispatches a class UUID to the supported simple-geometry reader.
pub(crate) fn supported_class(uuid: Uuid) -> bool {
    matches!(
        uuid,
        POINT
            | POINT_CLOUD
            | CURVE_ON_SURFACE
            | LINE
            | ARC
            | POLYLINE
            | POLYCURVE
            | POLYCURVE_LEGACY
            | NURBS_CURVE
            | NURBS_CURVE_TL
            | NURBS_CURVE_LEGACY
            | NURBS_SURFACE
            | NURBS_SURFACE_TL
            | NURBS_SURFACE_LEGACY
            | PLANE_SURFACE
            | CLIPPING_PLANE_SURFACE
            | REV_SURFACE
            | REV_SURFACE_LEGACY
            | SUM_SURFACE
    )
}

/// Returns whether a class derives from the curve carrier family.
pub(crate) fn curve_class(uuid: Uuid) -> bool {
    matches!(
        uuid,
        CURVE_PROXY
            | CURVE_ON_SURFACE
            | LINE
            | ARC
            | POLYLINE
            | POLYCURVE
            | POLYCURVE_LEGACY
            | NURBS_CURVE
            | NURBS_CURVE_TL
            | NURBS_CURVE_LEGACY
    )
}

/// Returns whether a class derives from the surface carrier family.
pub(crate) fn surface_class(uuid: Uuid) -> bool {
    matches!(
        uuid,
        NURBS_SURFACE
            | NURBS_SURFACE_TL
            | NURBS_SURFACE_LEGACY
            | PLANE_SURFACE
            | CLIPPING_PLANE_SURFACE
            | REV_SURFACE
            | REV_SURFACE_LEGACY
            | SUM_SURFACE
    )
}

#[cfg(test)]
mod alias_tests {
    use super::{
        curve_class, supported_class, surface_class, NURBS_CURVE_LEGACY, NURBS_CURVE_TL,
        NURBS_SURFACE_LEGACY, NURBS_SURFACE_TL, POLYCURVE_LEGACY,
    };
    use crate::chunks::BoundedReader;
    use crate::settings::MillimeterScale;
    use cadmpeg_ir::math::Point3;

    fn read_cloud(
        reader: &mut BoundedReader<'_>,
        scale: MillimeterScale,
    ) -> Result<super::PointCloud, super::GeometryError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("test input fits service profile");
        super::read_cloud(&ctx, reader, scale)
    }

    #[test]
    fn registered_aliases_keep_their_base_and_dispatch_families() {
        for class in [POLYCURVE_LEGACY, NURBS_CURVE_TL, NURBS_CURVE_LEGACY] {
            assert!(supported_class(class));
            assert!(curve_class(class));
        }
        for class in [NURBS_SURFACE_TL, NURBS_SURFACE_LEGACY] {
            assert!(supported_class(class));
            assert!(surface_class(class));
        }
    }

    #[test]
    fn point_cloud_keeps_points_when_an_optional_channel_count_is_redundant() {
        let mut bytes = vec![0x11];
        bytes.extend_from_slice(&2_i32.to_le_bytes());
        for point in [[0.0_f64, 0.0, 0.0], [1.0, 0.0, 0.0]] {
            for coordinate in point {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
        bytes.extend(std::iter::repeat_n(0_u8, 16 * 8 + 6 * 8));
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0_u8, 3 * 8));
        bytes.extend_from_slice(&0_i32.to_le_bytes());

        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("point-cloud reader");
        let cloud = read_cloud(&mut reader, MillimeterScale::IDENTITY)
            .expect("optional channel is recoverable");
        assert_eq!(cloud.points.len(), 2);
        assert_eq!(cloud.warnings.len(), 1);
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn point_cloud_consumes_matching_native_channels_before_neutral_transfer() {
        let mut bytes = vec![0x12];
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        for coordinate in [1.0_f64, 2.0, 3.0] {
            bytes.extend_from_slice(&coordinate.to_le_bytes());
        }
        for value in [
            0.0_f64, 0.0, 0.0, // plane origin
            1.0, 0.0, 0.0, // plane X
            0.0, 1.0, 0.0, // plane Y
            0.0, 0.0, 1.0, // plane Z
            0.0, 0.0, 1.0, 0.0, // plane equation
            0.0, 0.0, 0.0, // bounding-box minimum
            1.0, 1.0, 1.0, // bounding-box maximum
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&3_i32.to_le_bytes());
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        for value in [0.0_f64, 0.0, 1.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        bytes.extend_from_slice(&[11, 22, 33, 44]);
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        bytes.extend_from_slice(&12.5_f64.to_le_bytes());

        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("point-cloud reader");
        let cloud = read_cloud(&mut reader, MillimeterScale::IDENTITY)
            .expect("matching channels are recoverable");
        assert_eq!(
            cloud
                .points
                .iter()
                .map(|point| point.get())
                .collect::<Vec<_>>(),
            vec![Point3::new(1.0, 2.0, 3.0)]
        );
        assert!(cloud.warnings.is_empty());
        assert_eq!(reader.remaining(), 0);
    }
}

/// Decode one top-level class-data payload.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    class_uuid: Uuid,
    range: Range<usize>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
) -> Result<DecodedGeometry, GeometryError> {
    decode_inner(ctx, data, class_uuid, range, scale, archive, 0)
}

/// Decodes a Brep C2 curve in surface parameter space.
pub(crate) fn decode_2d(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    class_uuid: Uuid,
    range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<DecodedGeometry, GeometryError> {
    decode_inner_2d(ctx, data, class_uuid, range, archive, 0)
}

pub(crate) fn decode_inner(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    class_uuid: Uuid,
    range: Range<usize>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedGeometry, GeometryError> {
    let _depth_guard = ctx.enter_nested("Rhino curve tree")?;
    if depth > MAX_CURVE_DEPTH {
        return Err(ctx
            .refuse_codec_limit(
                "Rhino curve depth limit",
                cadmpeg_core::decode::u64_from_index(MAX_CURVE_DEPTH),
                cadmpeg_core::decode::u64_from_index(depth),
            )
            .into());
    }
    if class_uuid == CURVE_ON_SURFACE {
        let construction =
            crate::curve_on_surface::decode(ctx, data, range, scale, archive, depth + 1)?;
        let Some(mut curve) = construction.model_curve else {
            return Err(GeometryError::unsupported(
                construction.source_range.start,
                "curve-on-surface has no stored model-space carrier",
            ));
        };
        curve.warnings_mut().prepend(construction.warnings);
        return Ok(DecodedGeometry::Curve { curve });
    }
    if matches!(
        class_uuid,
        NURBS_SURFACE
            | NURBS_SURFACE_TL
            | NURBS_SURFACE_LEGACY
            | PLANE_SURFACE
            | CLIPPING_PLANE_SURFACE
            | REV_SURFACE
            | REV_SURFACE_LEGACY
            | SUM_SURFACE
    ) {
        return Ok(DecodedGeometry::Surface {
            surface: crate::surfaces::decode(ctx, data, class_uuid, range, scale, archive, depth)?,
        });
    }
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let result = match class_uuid {
        POINT => {
            let position = read_point(ctx, &mut reader, scale)?;
            DecodedGeometry::Point {
                position,
                scaled: scale != MillimeterScale::IDENTITY,
            }
        }
        POINT_CLOUD => DecodedGeometry::PointCloud(read_cloud(ctx, &mut reader, scale)?),
        LINE => DecodedGeometry::Curve {
            curve: DecodedCurve::leaf(
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(read_line(
                    ctx,
                    &mut reader,
                    scale,
                    None,
                )?)),
                Diagnostics::new(),
            ),
        },
        ARC => {
            let (geometry, warnings) = read_arc(ctx, &mut reader, scale, None, false)?;
            DecodedGeometry::Curve {
                curve: DecodedCurve::leaf(geometry, warnings),
            }
        }
        POLYLINE => DecodedGeometry::Curve {
            curve: DecodedCurve::leaf(
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(read_polyline(
                    ctx,
                    &mut reader,
                    scale,
                    None,
                )?)),
                Diagnostics::new(),
            ),
        },
        POLYCURVE | POLYCURVE_LEGACY => {
            let curve = read_polycurve(ctx, data, &mut reader, scale, archive, depth)?;
            DecodedGeometry::Curve { curve }
        }
        NURBS_CURVE | NURBS_CURVE_TL | NURBS_CURVE_LEGACY => DecodedGeometry::Curve {
            curve: DecodedCurve::leaf(
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                    crate::surfaces::read_nurbs_curve(ctx, &mut reader, scale)?,
                )),
                Diagnostics::new(),
            ),
        },
        _ => {
            return Err(GeometryError::unsupported(
                range.start,
                "unsupported Rhino geometry class",
            ));
        }
    };
    reader.skip_remaining()?;
    Ok(result)
}

/// Reads one bounded polymorphic child and requires it to be a curve.
pub(crate) fn decode_embedded_curve(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedCurve, GeometryError> {
    if depth > MAX_CURVE_DEPTH {
        return Err(ctx
            .refuse_codec_limit(
                "Rhino embedded curve depth limit",
                cadmpeg_core::decode::u64_from_index(MAX_CURVE_DEPTH),
                cadmpeg_core::decode::u64_from_index(depth),
            )
            .into());
    }
    let start = reader.position();
    let wrapper = crate::chunks::chunk_at(data, start, reader.end(), archive, false)?;
    let mut wrapper_warnings = Diagnostics::new();
    let class = parse_class_wrapper(
        ctx,
        data,
        start..wrapper.next_offset(),
        archive,
        &mut wrapper_warnings,
    )?;
    reader.skip(wrapper.next_offset() - start)?;
    if !matches!(
        class.class_uuid,
        LINE | ARC
            | POLYLINE
            | POLYCURVE
            | POLYCURVE_LEGACY
            | NURBS_CURVE
            | NURBS_CURVE_TL
            | NURBS_CURVE_LEGACY
    ) {
        return Err(GeometryError::malformed(
            start,
            "embedded surface child is not a supported curve",
        ));
    }
    let decoded = decode_inner(
        ctx,
        data,
        class.class_uuid,
        class.class_data_range,
        scale,
        archive,
        depth,
    )?;
    let DecodedGeometry::Curve { mut curve } = decoded else {
        return Err(GeometryError::malformed(
            start,
            "embedded surface child is not a curve",
        ));
    };
    curve.warnings_mut().prepend(wrapper_warnings);
    Ok(curve)
}

/// Reads one bounded polymorphic plane-space curve and applies length scaling.
pub(crate) fn decode_embedded_curve_2d(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedCurve, GeometryError> {
    if depth > MAX_CURVE_DEPTH {
        return Err(ctx
            .refuse_codec_limit(
                "Rhino embedded C2 curve depth limit",
                cadmpeg_core::decode::u64_from_index(MAX_CURVE_DEPTH),
                cadmpeg_core::decode::u64_from_index(depth),
            )
            .into());
    }
    let start = reader.position();
    let wrapper = crate::chunks::chunk_at(data, start, reader.end(), archive, false)?;
    let mut wrapper_warnings = Diagnostics::new();
    let class = parse_class_wrapper(
        ctx,
        data,
        start..wrapper.next_offset(),
        archive,
        &mut wrapper_warnings,
    )?;
    reader.skip(wrapper.next_offset() - start)?;
    if !curve_class(class.class_uuid) || matches!(class.class_uuid, CURVE_PROXY | CURVE_ON_SURFACE)
    {
        return Err(GeometryError::malformed(
            start,
            "embedded plane-space object is not a supported curve",
        ));
    }
    let decoded = decode_inner_2d(
        ctx,
        data,
        class.class_uuid,
        class.class_data_range,
        archive,
        depth,
    )?;
    let DecodedGeometry::Curve { mut curve } = decoded else {
        return Err(GeometryError::malformed(
            start,
            "embedded plane-space object is not a curve",
        ));
    };
    scale_decoded_curve(ctx, &mut curve, scale, start)?;
    curve.warnings_mut().prepend(wrapper_warnings);
    Ok(curve)
}

fn scale_decoded_curve(
    ctx: &DecodeContext<'_>,
    curve: &mut DecodedCurve,
    scale: MillimeterScale,
    offset: usize,
) -> Result<(), GeometryError> {
    match curve {
        DecodedCurve::Compound { children, .. } => {
            let _depth = ctx.enter_nested("Rhino plane-space curve scaling nesting")?;
            for (_, child) in ctx.admit_iter(&mut children[..], "Rhino plane-space curve scaling visit").map_err(CodecError::from)? {
                scale_decoded_curve(ctx, child, scale, offset)?;
            }
            return Ok(());
        }
        DecodedCurve::Leaf { geometry, .. } => match geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                if let Err(message) = nurbs.try_map_control_points(
                    |_, point| {
                        point
                            .scaled(scale.positive())
                            .ok_or("scaled plane-space curve is invalid")
                    },
                    ctx,
                )? {
                    return Err(GeometryError::malformed(
                        offset,
                        ctx.copy_retained_text(message, "Rhino pole mapping refusal")?,
                    ));
                }
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                let radius = circle_curve.radius().get();
                let center = circle_curve
                    .center()
                    .scaled(scale.positive())
                    .ok_or_else(|| {
                        GeometryError::malformed(
                            offset,
                            "scaled plane-space curve point is invalid",
                        )
                    })?;
                let radius = cadmpeg_ir::scalar::PositiveLength::new(radius * scale.value())
                    .ok_or_else(|| {
                        GeometryError::malformed(
                            offset,
                            "CircleCurve.radius must be positive and finite",
                        )
                    })?;
                *circle_curve = cadmpeg_ir::geometry::analytic::CircleCurve::new(
                    center,
                    *circle_curve.frame(),
                    radius,
                );
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                let direction = line_curve.direction();
                let origin = line_curve
                    .origin()
                    .scaled(scale.positive())
                    .ok_or_else(|| {
                        GeometryError::malformed(
                            offset,
                            "scaled plane-space curve point is invalid",
                        )
                    })?;
                *line_curve = cadmpeg_ir::geometry::analytic::LineCurve::new(origin, direction);
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(degenerate_curve)) => {
                let point = degenerate_curve
                    .point()
                    .scaled(scale.positive())
                    .ok_or_else(|| {
                        GeometryError::malformed(
                            offset,
                            "scaled plane-space curve point is invalid",
                        )
                    })?;
                *degenerate_curve = cadmpeg_ir::geometry::analytic::DegenerateCurve::new(point);
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. }) => {
                return Err(GeometryError::malformed(
                    offset,
                    "plane-space curve has unknown geometry",
                ));
            }
            _ => {
                return Err(GeometryError::malformed(
                    offset,
                    "unsupported plane-space analytic curve",
                ));
            }
        },
    }
    Ok(())
}

/// Converts a decoded curve tree to one exact NURBS curve when possible.
pub(crate) fn exact_nurbs(
    ctx: &DecodeContext<'_>,
    curve: &DecodedCurve,
    offset: usize,
) -> Result<NurbsCurve, GeometryError> {
    match curve {
        DecodedCurve::Leaf { geometry, .. } => match geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                Ok(nurbs.try_clone_for_decode(ctx, "Rhino exact NURBS copy")?)
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                let center = circle_curve.center();
                let axis = circle_curve.frame().axis().as_raw();
                let ref_direction = circle_curve.frame().reference().as_raw();
                let radius = circle_curve.radius();
                let yaxis = axis.cross(*ref_direction);
                let circle = Circle {
                    center,
                    axis: *axis,
                    xaxis: *ref_direction,
                    yaxis,
                    radius,
                };
                arc_nurbs(ctx, &circle, [0.0, TAU], [0.0, TAU], TAU, offset)
            }
            _ => Err(error(offset, "curve has no exact NURBS representation")),
        },
        DecodedCurve::Compound {
            children,
            end_parameter,
            ..
        } => {
            let mut segment_storage = ctx.reserve_scoped(0, "Rhino exact NURBS segment scratch")?;
            let mut segments = segment_storage.with_storage(|| ctx.collection_vec(children.len(), "Rhino exact NURBS segments"))?;
            for (index, (start, child)) in ctx
                .admit_iter(&children[..], "Rhino exact nurbs traversal")
                .map_err(cadmpeg_core::CodecError::from)?
                .enumerate()
            {
                let end = children
                    .get(index + 1)
                    .map_or(*end_parameter, |(next, _)| *next);
                let target = [*start, end];
                if target[0] >= target[1] {
                    return Err(error(offset, "polycurve segment domain is invalid"));
                }
                segments.push(segment_storage.with_storage(|| remap_nurbs_domain(ctx, exact_nurbs(ctx, child, offset)?, target, offset))?);
            }
            join_nurbs_curves(ctx, segments, offset, None)
        }
    }
}

pub(crate) fn remap_nurbs_domain(
    ctx: &DecodeContext<'_>,
    curve: NurbsCurve,
    target: [FiniteReal; 2],
    offset: usize,
) -> Result<NurbsCurve, GeometryError> {
    let degree =
        usize::try_from(curve.degree()).map_err(|_| error(offset, "curve degree is too large"))?;
    let end_index = curve
        .knots()
        .len()
        .checked_sub(degree + 1)
        .ok_or_else(|| error(offset, "curve knot vector is invalid"))?;
    let source = [
        *curve
            .knots()
            .get(degree)
            .ok_or_else(|| error(offset, "curve knot vector is invalid"))?,
        curve.knots()[end_index],
    ];
    if source[0] >= source[1] {
        return Err(error(offset, "curve domain is invalid"));
    }
    if target[0] >= target[1] {
        return Err(error(offset, "curve target domain is invalid"));
    }
    let target = target.map(FiniteReal::get);
    let mut remapped = ctx
        .collection_vec(curve.knots().len(), "Rhino remapped NURBS knots")
        .map_err(crate::curves::GeometryError::from)?;
    for knot in ctx
        .admit_iter(&(curve.knots())[..], "Rhino remap nurbs domain traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .copied()
    {
        let fraction = cadmpeg_ir::math::parameter_fraction(knot, source[0], source[1])
            .map(cadmpeg_ir::scalar::FiniteReal::get)
            .ok_or_else(|| error(offset, "curve knot remap overflowed"))?;
        let value = if fraction == 0.0 {
            target[0]
        } else if fraction == 1.0 {
            target[1]
        } else {
            (1.0 - fraction) * target[0] + fraction * target[1]
        };
        remapped.push(
            value
                .is_finite()
                .then_some(value)
                .ok_or_else(|| error(offset, "curve knot remap overflowed"))?,
        );
    }
    curve.with_knots(ctx, remapped)?.or_else(|error| {
        Err(GeometryError::malformed(
            offset,
            ctx.format_retained(format_args!("{}", error), "Rhino remap_nurbs_domain text")?,
        ))
    })
}

/// Exact joined curve and recoverable join diagnostics.
pub(crate) struct NurbsJoin {
    pub(crate) curve: NurbsCurve,
    pub(crate) warnings: Diagnostics,
}

#[derive(Clone, Copy)]
struct Homogeneous([f64; 4]);

impl Homogeneous {
    fn blend(self, other: Self, alpha: f64) -> Self {
        Self(std::array::from_fn(|index| {
            (1.0 - alpha) * self.0[index] + alpha * other.0[index]
        }))
    }
}

fn elevate_bezier<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'ctx>,
    mut values: Vec<Homogeneous>,
    target: usize,
) -> Result<Vec<Homogeneous>, GeometryError> {
    while values.len() - 1 < target {
        ctx.charge_work(1, "Rhino Bezier degree steps")?;
        let degree = values.len() - 1;
        let count = values.len() + 1;
        let mut next_storage = ctx.reserve_scoped(0, "Rhino polycurve Bezier elevation scratch")?;
        let mut elevated = next_storage.with_storage(|| ctx.collection_vec(count, "Rhino polycurve Bezier elevation"))?;
        elevated.push(values[0]);
        for index in ctx.admit_iter(1..=degree, "Rhino Bezier elevation pole traversal").map_err(CodecError::from)? {
            elevated.push(values[index].blend(
                values[index - 1],
                cadmpeg_core::convert::f64_from_index(index).ok_or_else(|| {
                    GeometryError::unpositioned("geometry index exceeds exact float range")
                })? / cadmpeg_core::convert::f64_from_index(degree + 1).ok_or_else(|| {
                    GeometryError::unpositioned("geometry index exceeds exact float range")
                })?,
            ));
        }
        elevated.push(values[degree]);
        values = elevated;
        *storage = next_storage;
    }
    Ok(values)
}

fn clamp_endpoint<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'ctx>,
    knots: &mut Vec<f64>,
    points: &mut Vec<Homogeneous>,
    degree: usize,
    value: f64,
    offset: usize,
) -> Result<(), GeometryError> {
    let multiplicity = ctx.admit_iter(&knots[..], "Rhino endpoint knot multiplicity").map_err(CodecError::from)?.filter(|knot| **knot == value).count();
    if multiplicity >= degree + 1 { return Ok(()); }
    let k = ctx.rposition_by(&knots[..], |knot| Ok(*knot <= value), "Rhino knot span search")?
        .ok_or_else(|| error(offset, "polycurve endpoint clamping failed"))?;
    let k = if degree == 0 { k.min(points.len() - 1) } else { k };
    if k < degree || k - degree >= points.len() || k.checked_sub(multiplicity).is_none_or(|tail| tail >= points.len()) {
        return Err(error(offset, "polycurve endpoint clamping failed"));
    }
    let first = k - degree;
    let last = k - multiplicity;
    let count = degree + 1 - multiplicity;
    let mut local_storage = ctx.reserve_scoped(0, "Rhino endpoint clamping local scratch")?;
    let mut local_points = local_storage.with_storage(|| ctx.copy_slice(&points[first..=last], "Rhino endpoint local poles"))?;
    let knot_end = (k + degree - multiplicity).min(knots.len() - 1);
    let mut local_knots = local_storage.with_storage(|| ctx.copy_slice(&knots[first..=knot_end], "Rhino endpoint local knots"))?;
    for step in ctx.admit_iter(0..count, "Rhino endpoint clamping steps").map_err(CodecError::from)? {
        let span = degree + step;
        let tail = degree - multiplicity;
        let duplicate = local_points[tail];
        local_storage.with_storage(|| ctx.insert_vec(&mut local_points, tail + 1, duplicate, "Rhino polycurve knot insertion points"))?;
        for index in ctx.admit_iter(step + 1..=tail, "Rhino knot insertion blends").map_err(CodecError::from)?.rev() {
            let denominator = local_knots[index + degree] - local_knots[index];
            if denominator <= 0.0 || !denominator.is_finite() { return Err(error(offset, "polycurve endpoint clamping failed")); }
            let alpha = (value - local_knots[index]) / denominator;
            local_points[index] = local_points[index - 1].blend(local_points[index], alpha);
        }
        local_storage.with_storage(|| ctx.insert_vec(&mut local_knots, span + 1, value, "Rhino polycurve inserted knot"))?;
    }
    let mut output_storage = ctx.reserve_scoped(0, "Rhino endpoint clamped scratch")?;
    let mut output = output_storage.with_storage(|| ctx.collection_vec(points.len() + count, "Rhino endpoint clamped poles"))?;
    output.extend(ctx.admit_iter(&points[..first], "Rhino endpoint pole prefix").map_err(CodecError::from)?.copied());
    output.extend(ctx.admit_iter(&local_points[..], "Rhino endpoint local pole copy").map_err(CodecError::from)?.copied());
    output.extend(ctx.admit_iter(&points[last + 1..], "Rhino endpoint pole suffix").map_err(CodecError::from)?.copied());
    let mut output_knots = output_storage.with_storage(|| ctx.collection_vec(knots.len() + count, "Rhino endpoint clamped knots"))?;
    output_knots.extend(ctx.admit_iter(&knots[..=k], "Rhino endpoint knot prefix").map_err(CodecError::from)?.copied());
    output_knots.extend(ctx.admit_iter(0..count, "Rhino endpoint knot repeats").map_err(CodecError::from)?.map(|_| value));
    output_knots.extend(ctx.admit_iter(&knots[k + 1..], "Rhino endpoint knot suffix").map_err(CodecError::from)?.copied());
    *points = output;
    *knots = output_knots;
    *storage = output_storage;
    Ok(())
}

fn elevate_to_degree(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    target: usize,
    offset: usize,
) -> Result<NurbsCurve, GeometryError> {
    let degree =
        usize::try_from(curve.degree()).map_err(|_| error(offset, "curve degree overflow"))?;
    if degree > target || curve.periodic() {
        return Err(error(offset, "polycurve segment knot vector is invalid"));
    }
    let pole_count = curve.pole_rows().count();
    let source_weight_at = |index: usize| curve.pole_rows().weight_at(index).unwrap_or(1.0);
    let rational = ctx.any_by(0..pole_count, |index| Ok(source_weight_at(index) != 1.0), "Rhino elevation rational weight search")?;
    let point_at = |index: usize| match curve.pole_rows() {
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => points[index].get(),
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => points[index].point.get(),
    };
    let normalize = ctx.any_by(0..pole_count, |index| {
        let point = point_at(index);
        let weight = source_weight_at(index);
        Ok([point.x, point.y, point.z].into_iter().any(|coordinate| {
            let product = coordinate * weight;
            !product.is_finite() || (product == 0.0 && coordinate != 0.0)
        }))
    }, "Rhino elevation weight normalization search")?;
    let exponent = if normalize {
        let maximum_weight = ctx.admit_iter(0..pole_count, "Rhino elevation weight scale").map_err(CodecError::from)?
            .map(|index| source_weight_at(index).abs())
            .fold(0.0, f64::max);
        let exponent = cadmpeg_ir::math::power_of_two_bound(maximum_weight)
            .ok_or_else(|| error(offset, "polycurve weight scale is invalid"))?;
        for index in 0..pole_count {
            ctx.charge_work(1, "Rhino curves elevate_to_degree records")?;
            cadmpeg_ir::math::scale_power_of_two(source_weight_at(index), -exponent)
                .filter(|weight| weight.get() != 0.0)
                .ok_or_else(|| error(offset, "polycurve weight normalization lost its range"))?;
        }
        Some(exponent)
    } else {
        None
    };

    let source_knots = curve.knots();
    let domain = [
        source_knots[degree],
        source_knots[source_knots.len() - degree - 1],
    ];
    let mut elevated_knots = ctx.alloc_filled(
        target
            .checked_add(1)
            .ok_or_else(|| error(offset, "polycurve elevated knot count overflow"))?,
        domain[0],
        "Rhino polycurve elevated knots",
    )?;

    let mut scratch = ctx.reserve_scoped(0, "Rhino degree elevation scratch")?;
    let mut control_storage = ctx.reserve_scoped(0, "Rhino degree elevation source scratch")?;
    let mut points = control_storage.with_storage(|| ctx.collection_vec(pole_count, "Rhino polycurve homogeneous points"))?;
    for index in ctx.admit_iter(0..pole_count, "Rhino homogeneous pole traversal").map_err(CodecError::from)? {
        let point = point_at(index);
        let weight = match exponent {
            Some(exponent) => cadmpeg_ir::math::scale_power_of_two(source_weight_at(index), -exponent)
                .ok_or_else(|| error(offset, "polycurve weight normalization lost its range"))?.get(),
            None => source_weight_at(index),
        };
        points.push(Homogeneous([point.x * weight, point.y * weight, point.z * weight, weight]));
    }
    let mut knots = control_storage.with_storage(|| ctx.copy_slice(source_knots.as_slice(), "Rhino polycurve knots"))?;
    for endpoint in domain {
        clamp_endpoint(ctx, &mut control_storage, &mut knots, &mut points, degree, endpoint, offset)?;
    }
    let mut a = ctx.rposition_by(&knots, |value| Ok(*value == domain[0]), "Rhino first Bezier span")?
        .ok_or_else(|| error(offset, "polycurve segment has no nonempty span"))?;
    let mut b = a + 1;
    let mut start = domain[0];
    let mut current = scratch.with_storage(|| ctx.copy_slice(&points[a - degree..=a], "Rhino polycurve Bezier span"))?;
    let mut next = scratch.with_storage(|| ctx.collection_vec(degree + 1, "Rhino next Bezier span"))?;
    let mut alphas = scratch.with_storage(|| ctx.collection_vec(degree, "Rhino Bezier boundary coefficients"))?;
    let mut elevated = Vec::new();
    let mut first_span = true;
    let mut disconnected = false;
    while start < domain[1] {
        ctx.charge_work(1, "Rhino Bezier span traversal")?;
        let run_start = b;
        while b + 1 < knots.len() && knots[b + 1] == knots[b] {
            ctx.charge_work(1, "Rhino Bezier knot run")?;
            b += 1;
        }
        let multiplicity = b - run_start + 1;
        let end = knots[b];
        if end <= start || end > domain[1] {
            return Err(error(offset, "polycurve segment has no nonempty span"));
        }
        if multiplicity < degree {
            let remaining = degree - multiplicity;
            let numerator = end - knots[a];
            alphas.clear();
            for index in ctx.admit_iter(multiplicity + 1..=degree, "Rhino Bezier boundary coefficients").map_err(CodecError::from)? {
                let denominator = knots[a + index] - knots[a];
                if denominator <= 0.0 || !denominator.is_finite() {
                    return Err(error(offset, "polycurve knot insertion failed"));
                }
                alphas.push(numerator / denominator);
            }
            for step in ctx.admit_iter(1..=remaining, "Rhino Bezier boundary steps").map_err(CodecError::from)? {
                let first = multiplicity + step;
                for index in ctx.admit_iter(first..=degree, "Rhino Bezier boundary blends").map_err(CodecError::from)?.rev() {
                    current[index] = current[index - 1].blend(current[index], alphas[index - first]);
                }
                next.push(current[degree]);
            }
        }
        ctx.reverse(&mut next[..], "Rhino Bezier overlap order")?;
        let mut span_storage = ctx.reserve_scoped(0, "Rhino elevated Bezier scratch")?;
        let values = span_storage.with_storage(|| ctx.copy_slice(&current, "Rhino polycurve Bezier span"))?;
        let bezier = elevate_bezier(ctx, &mut span_storage, values, target)?;
        let skip = usize::from(!first_span && !disconnected);
        if !first_span {
            let added = target + usize::from(disconnected);
            ctx.reserve_vec(&mut elevated_knots, added, "Rhino polycurve elevated knots")?;
            elevated_knots.extend(ctx.admit_iter(0..added, "Rhino elevated boundary knots").map_err(CodecError::from)?.map(|_| start));
        }
        scratch.with_storage(|| ctx.reserve_vec(&mut elevated, bezier.len() - skip, "Rhino polycurve elevated points"))?;
        elevated.extend(ctx.admit_iter(&bezier[skip..], "Rhino elevated Bezier pole copy").map_err(CodecError::from)?.copied());
        first_span = false;
        if end == domain[1] { break; }
        let first = degree.saturating_sub(multiplicity);
        for index in ctx.admit_iter(first..=degree, "Rhino next Bezier pole copy").map_err(CodecError::from)? {
            next.push(points[b - degree + index]);
        }
        current.clear();
        std::mem::swap(&mut current, &mut next);
        disconnected = multiplicity > degree;
        start = if multiplicity < degree { knots[run_start] } else { knots[b] };
        a = b;
        b += 1;
    }
    ctx.reserve_vec(
        &mut elevated_knots,
        target + 1,
        "Rhino polycurve final knots",
    )?;
    elevated_knots.extend(ctx.admit_iter(0..target + 1, "Rhino elevated endpoint knots").map_err(CodecError::from)?.map(|_| domain[1]));
    let mut output_weights = if rational { Some(scratch.with_storage(|| ctx.collection_vec(elevated.len(), "Rhino polycurve output weights"))?) } else { None };
    let mut control_points = scratch.with_storage(|| ctx.collection_vec(elevated.len(), "Rhino polycurve output points"))?;
    for point in ctx.admit_iter(elevated, "Rhino elevated pole traversal").map_err(CodecError::from)? {
        let Some(weight) = NonZeroReal::new(point.0[3]) else {
            return Err(error(
                offset,
                "polycurve degree elevation produced an invalid weight",
            ));
        };
        control_points.push(Point3::new(
            point.0[0] / weight.get(),
            point.0[1] / weight.get(),
            point.0[2] / weight.get(),
        ));
        if let Some(weights) = &mut output_weights { weights.push(weight); }
    }
    let target = u32::try_from(target)
        .map_err(|_| GeometryError::unpositioned("polycurve degree exceeds u32"))?;
    NurbsCurve::from_checked_lanes(
        ctx,
        target,
        elevated_knots,
        control_points,
        output_weights,
        false,
    )?
    .or_else(|error| {
        Err(GeometryError::malformed(
            offset,
            ctx.format_retained(format_args!("{}", error), "Rhino elevate_to_degree text")?,
        ))
    })
}

pub(crate) fn join_nurbs_segments(
    ctx: &DecodeContext<'_>,
    segments: Vec<NurbsCurve>,
    offset: usize,
) -> Result<NurbsJoin, GeometryError> {
    let mut warnings = Diagnostics::new();
    let curve = join_nurbs_curves(ctx, segments, offset, Some(&mut warnings))?;
    Ok(NurbsJoin { curve, warnings })
}

fn join_nurbs_curves(
    ctx: &DecodeContext<'_>,
    mut segments: Vec<NurbsCurve>,
    offset: usize,
    mut warnings: Option<&mut Diagnostics>,
) -> Result<NurbsCurve, GeometryError> {
    let Some(_) = segments.first() else {
        return Err(error(offset, "polycurve has no segments"));
    };
    let degree = ctx.admit_iter(&segments[..], "Rhino joined maximum degree").map_err(CodecError::from)?
        .map(NurbsCurve::degree)
        .max()
        .ok_or_else(|| error(offset, "polycurve has no segments"))?;
    let target = usize::try_from(degree).map_err(|_| error(offset, "curve degree overflow"))?;
    if target == 0 {
        return Err(error(offset, "polycurve segment degree must be positive"));
    }
    if segments.len() == 1 {
        return elevate_to_degree(ctx, &segments[0], target, offset);
    }
    let mut container_storage = ctx.reserve_scoped(0, "Rhino elevated segment container")?;
    let mut segment_storage = ctx.reserve_scoped(0, "Rhino elevated segment geometry")?;
    let mut elevated_segments = container_storage.with_storage(|| ctx.collection_vec(segments.len(), "Rhino elevated polycurve segments"))?;
    for segment in ctx
        .admit_iter(&segments[..], "Rhino join nurbs segments traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        elevated_segments.push(segment_storage.with_storage(|| elevate_to_degree(ctx, segment, target, offset))?);
    }
    segments = elevated_segments;
    let multiplicity = usize::try_from(degree)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| error(offset, "curve degree overflow"))?;
    for segment in ctx
        .admit_iter(&segments[..], "Rhino join nurbs segments traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        let start = segment.knots().get(multiplicity - 1).copied();
        let end = segment
            .knots()
            .len()
            .checked_sub(multiplicity)
            .and_then(|index| segment.knots().get(index))
            .copied();
        if start.is_none()
            || end.is_none()
            || ctx.any_by(
                &(segment.knots()[..multiplicity])[..],
                |value| Ok(Some(*value) != start),
                "Rhino join nurbs segments traversal",
            )?
            || ctx.any_by(
                &(segment.knots()[segment.knots().len() - multiplicity..])[..],
                |value| Ok(Some(*value) != end),
                "Rhino join nurbs segments traversal",
            )?
        {
            return Err(error(offset, "polycurve segment is not endpoint-clamped"));
        }
    }
    let rational = ctx.any_by(
        &segments[..],
        |segment| {
            Ok(matches!(
                segment.pole_rows(),
                cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { .. }
            ))
        },
        "Rhino join nurbs segments traversal",
    )?;
    let mut output_storage = ctx.reserve_scoped(0, "Rhino joined pole lanes")?;
    let mut control_points: Vec<Point3> = Vec::new();
    let mut knots: Vec<f64> = Vec::new();
    let mut weights = rational.then(Vec::new);
    for (index, segment) in ctx.admit_iter(segments, "Rhino joined segment traversal").map_err(CodecError::from)?.enumerate() {
        let midpoint = if index > 0 {
            let Some(previous) = control_points.last().copied() else {
                return Err(error(
                    offset,
                    "polycurve join has no previous segment endpoint",
                ));
            };
            let next = segment
                .pole_rows()
                .point_at(0)
                .ok_or_else(|| error(offset, "polycurve segment has no control point"))?
                .get();
            let midpoint = Point3::new(
                previous.x.midpoint(next.x),
                previous.y.midpoint(next.y),
                previous.z.midpoint(next.z),
            );
            let gap = previous.distance(next);
            if let Some(warnings) = warnings.as_mut().filter(|_| gap > 0.0) {
                warnings.push_coded_admitted(
                    ctx,
                    crate::loss::RhinoLossCode::PolycurveJoinGap,
                    format_args!("polycurve join moved endpoints by half of gap {gap}"),
                )?;
            }
            let Some(previous) = control_points.last_mut() else {
                return Err(error(offset, "polycurve join has no previous endpoint"));
            };
            *previous = midpoint;
            Some(midpoint)
        } else {
            None
        };
        // Unequal endpoint weights are different homogeneous poles. Keep both
        // with a full-multiplicity knot so neither segment's rational shape changes.
        let unit = NonZeroReal::from(PositiveReal::ONE);
        let previous_weight = weights
            .as_ref()
            .and_then(|weights| weights.last())
            .copied()
            .unwrap_or(unit);
        let next_weight = match segment.pole_rows() {
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { .. } => unit,
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
                points.first().map_or(unit, |point| point.weight)
            }
        };
        let skip = usize::from(index > 0 && previous_weight.get() == next_weight.get());
        let count = segment.pole_rows().count() - skip;
        if let Some(target) = &mut weights {
            output_storage.with_storage(|| ctx.reserve_vec(target, count, "Rhino joined polycurve weights"))?;
        }
        output_storage.with_storage(|| ctx.reserve_vec(&mut control_points, count, "Rhino joined polycurve points"))?;
        for point_index in ctx.admit_iter(skip..segment.pole_rows().count(), "Rhino joined pole traversal").map_err(CodecError::from)? {
            if let Some(target) = &mut weights {
                let weight = match segment.pole_rows() {
                    cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { .. } => unit,
                    cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => points[point_index].weight,
                };
                target.push(weight);
            }
            let point = segment
                .pole_rows()
                .point_at(point_index)
                .ok_or_else(|| error(offset, "polycurve segment has no control point"))?
                .get();
            control_points.push(if point_index == 0 {
                midpoint.unwrap_or(point)
            } else {
                point
            });
        }
        let segment_start = segment.knots()[multiplicity - 1];
        let dk = if index == 0 {
            0.0
        } else {
            knots.last().copied().unwrap_or(0.0) - segment_start
        };
        if skip > 0 {
            knots.pop();
        }
        let knot_skip = if index == 0 { 0 } else { multiplicity };
        let knot_added = segment.knots().len() - knot_skip;
        ctx.reserve_vec(&mut knots, knot_added, "Rhino joined polycurve knots")?;
        knots.extend(
            ctx.admit_iter(&segment.knots()[knot_skip..], "Rhino joined knot traversal")
                .map_err(CodecError::from)?.copied().map(|knot| knot + dk),
        );
    }
    Ok(NurbsCurve::from_checked_lanes(ctx, degree, knots, control_points, weights, false)?
            .or_else(|error| {
                Err(GeometryError::malformed(
                    offset,
                    ctx.format_retained(
                        format_args!("{}", error),
                        "Rhino join_nurbs_segments text",
                    )?,
                ))
            })?)
}

pub(crate) fn decode_inner_2d(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    class_uuid: Uuid,
    range: Range<usize>,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedGeometry, GeometryError> {
    let _depth_guard = ctx.enter_nested("Rhino C2 curve tree")?;
    if depth > MAX_CURVE_DEPTH {
        return Err(ctx
            .refuse_codec_limit(
                "Rhino C2 curve depth limit",
                cadmpeg_core::decode::u64_from_index(MAX_CURVE_DEPTH),
                cadmpeg_core::decode::u64_from_index(depth),
            )
            .into());
    }
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let result = match class_uuid {
        NURBS_CURVE | NURBS_CURVE_TL | NURBS_CURVE_LEGACY => DecodedGeometry::Curve {
            curve: DecodedCurve::leaf(
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                    crate::surfaces::read_nurbs_curve_2d(ctx, &mut reader)?,
                )),
                Diagnostics::new(),
            ),
        },
        LINE => DecodedGeometry::Curve {
            curve: DecodedCurve::leaf(
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(read_line(
                    ctx,
                    &mut reader,
                    MillimeterScale::IDENTITY,
                    Some(2),
                )?)),
                Diagnostics::new(),
            ),
        },
        POLYLINE => DecodedGeometry::Curve {
            curve: DecodedCurve::leaf(
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(read_polyline(
                    ctx,
                    &mut reader,
                    MillimeterScale::IDENTITY,
                    Some(2),
                )?)),
                Diagnostics::new(),
            ),
        },
        ARC => {
            let (geometry, warnings) =
                read_arc(ctx, &mut reader, MillimeterScale::IDENTITY, Some(2), true)?;
            DecodedGeometry::Curve {
                curve: DecodedCurve::leaf(geometry, warnings),
            }
        }
        POLYCURVE | POLYCURVE_LEGACY => {
            let curve = read_polycurve_2d(ctx, data, &mut reader, archive, depth)?;
            DecodedGeometry::Curve { curve }
        }
        _ => {
            return Err(GeometryError::unsupported(
                range.start,
                "unsupported Rhino C2 curve class",
            ));
        }
    };
    reader.skip_remaining()?;
    Ok(result)
}

pub(crate) fn read_polycurve_2d(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedCurve, GeometryError> {
    let version = reader.u8()?;
    if version >> 4 != 1 {
        return Err(GeometryError::unsupported(
            reader.position() - 1,
            "unsupported C2 polycurve payload version",
        ));
    }
    let segment_count = crate::wire::element_count(reader, 1)?;
    if segment_count == 0 {
        return Err(GeometryError::malformed(
            reader.position(),
            "C2 polycurve has no segments",
        ));
    }
    reader.i32()?;
    reader.i32()?;
    reader.skip(48)?;
    let mut parameter_storage = ctx.reserve_scoped(0, "Rhino polycurve parameter scratch")?;
    let (parameters, end_parameter) = parameter_storage.with_storage(|| read_polycurve_parameters(ctx, reader, segment_count, "C2 polycurve"))?;
    let mut children = ctx
        .collection_vec(segment_count, "Rhino C2 polycurve children")
        .map_err(crate::curves::GeometryError::from)?;
    for parameter in ctx.admit_iter(parameters, "Rhino polycurve child traversal").map_err(CodecError::from)? {
        let start = reader.position();
        let wrapper = crate::chunks::chunk_at(data, start, reader.end(), archive, false)?;
        let mut wrapper_warnings = Diagnostics::new();
        let class = parse_class_wrapper(
            ctx,
            data,
            start..wrapper.next_offset(),
            archive,
            &mut wrapper_warnings,
        )?;
        reader.skip(wrapper.next_offset() - start)?;
        let child = decode_inner_2d(
            ctx,
            data,
            class.class_uuid,
            class.class_data_range,
            archive,
            depth + 1,
        )?;
        let DecodedGeometry::Curve { mut curve } = child else {
            return Err(GeometryError::malformed(
                start,
                "C2 polycurve child is not a curve",
            ));
        };
        curve.warnings_mut().prepend(wrapper_warnings);
        children.push((parameter, curve));
    }
    Ok(DecodedCurve::Compound {
        children,
        end_parameter,
        warnings: Diagnostics::new(),
    })
}

fn read_point(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
) -> Result<FinitePoint3, GeometryError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let point = native_point(ctx, reader)?;
    crate::wire::scaled_point(point.0.get(), scale)
        .ok_or_else(|| error(reader.position(), "scaled point coordinate is invalid"))
}

fn read_cloud(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
) -> Result<PointCloud, GeometryError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let minor = version & 0x0f;
    let point_count = crate::wire::element_count(reader, 24)?;
    let mut points = ctx.collection_vec(point_count, "Rhino point-cloud points")?;
    for _ in 0..point_count {
        ctx.charge_work(1, "Rhino curves read_cloud records")?;
        let point = native_point(ctx, reader)?;
        points.push(
            crate::wire::scaled_point(point.0.get(), scale)
                .ok_or_else(|| error(reader.position(), "scaled point coordinate is invalid"))?,
        );
    }
    plane(ctx, reader)?;
    bbox(ctx, reader)?;
    reader.i32()?;
    let mut warnings = Diagnostics::new();
    if minor >= 1 {
        let normal_count = crate::wire::element_count(reader, 24)?;
        if normal_count != 0 && normal_count != point_count {
            warnings.push_coded_admitted(
                ctx,
                crate::loss::RhinoLossCode::RedundantFieldRepaired,
                format_args!("redundant point-cloud normal count mismatch; channel dropped"),
            )?;
        }
        for _ in 0..normal_count {
            ctx.charge_work(1, "Rhino curves read_cloud records")?;
            crate::settings::vector(ctx, reader)?;
        }
        let color_count = crate::wire::element_count(reader, 4)?;
        reader.take(color_count * 4)?;
        if color_count != 0 && color_count != point_count {
            warnings.push_coded_admitted(
                ctx,
                crate::loss::RhinoLossCode::RedundantFieldRepaired,
                format_args!("redundant point-cloud color count mismatch; channel dropped"),
            )?;
        }
    }
    if minor >= 2 {
        let value_count = crate::wire::element_count(reader, 8)?;
        for _ in 0..value_count {
            ctx.charge_work(1, "Rhino curves read_cloud records")?;
            let value_offset = reader.position();
            let value = reader.f64()?;
            if !value.is_finite() {
                return Err(error(value_offset, "point-cloud value is not finite"));
            }
        }
        if value_count != 0 && value_count != point_count {
            warnings.push_coded_admitted(
                ctx,
                crate::loss::RhinoLossCode::RedundantFieldRepaired,
                format_args!("redundant point-cloud scalar count mismatch; channel dropped"),
            )?;
        }
    }
    if point_count == 0 {
        return Err(error(
            reader.position(),
            "point-cloud point count is invalid",
        ));
    }
    reader.skip_remaining()?;
    Ok(PointCloud {
        points,
        scaled: scale != MillimeterScale::IDENTITY,
        warnings,
    })
}

fn read_line(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    expected_dimension: Option<i32>,
) -> Result<NurbsCurve, GeometryError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let from = crate::wire::scaled_point(native_point(ctx, reader)?.0.get(), scale)
        .ok_or_else(|| error(reader.position(), "scaled line coordinate is invalid"))?
        ;
    let to = crate::wire::scaled_point(native_point(ctx, reader)?.0.get(), scale)
        .ok_or_else(|| error(reader.position(), "scaled line coordinate is invalid"))?
        ;
    let domain = interval(ctx, reader)?.0.get();
    let dimension = reader.i32()?;
    if expected_dimension.is_some_and(|expected| dimension != expected)
        || !(dimension == 2 || dimension == 3)
        || from == to
        || domain[0] >= domain[1]
    {
        return Err(error(reader.position(), "invalid bounded line"));
    }
    let mut knots = ctx.collection_vec(4, "Rhino line knots")?;
    knots.extend([domain[0], domain[0], domain[1], domain[1]]);
    let mut points = ctx.collection_vec(2, "Rhino line control points")?;
    points.extend([from, to]);
    cadmpeg_ir::geometry::nurbs::NurbsCurve::from_checked_lanes(
        ctx,
        1,
        knots,
        points,
        None,
        false,
    )
    .map_err(GeometryError::from)?
    .or_else(|error| {
        Err(GeometryError::malformed(
            reader.position(),
            ctx.format_retained(format_args!("{}", error), "Rhino read_line text")?,
        ))
    })
}

fn read_polyline(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    expected_dimension: Option<i32>,
) -> Result<NurbsCurve, GeometryError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let point_count = crate::wire::element_count(reader, 24)?;
    if point_count < 2 {
        return Err(error(
            reader.position(),
            "polyline needs at least two points",
        ));
    }
    let mut points = ctx
        .collection_vec(point_count, "Rhino polyline points")
        .map_err(crate::curves::GeometryError::from)?;
    for _ in 0..point_count {
        ctx.charge_work(1, "Rhino curves read_polyline records")?;
        let point = native_point(ctx, reader)?;
        points.push(
            crate::wire::scaled_point(point.0.get(), scale)
                .ok_or_else(|| error(reader.position(), "scaled polyline coordinate is invalid"))?,
        );
    }
    let parameter_count = crate::wire::element_count(reader, 8)?;
    if parameter_count != point_count {
        return Err(error(
            reader.position(),
            "polyline parameter count mismatch",
        ));
    }
    let mut parameter_storage = ctx.reserve_scoped(0, "Rhino polyline parameter scratch")?;
    let mut parameters = parameter_storage.with_storage(|| ctx.collection_vec(parameter_count, "Rhino polyline parameters"))?;
    for _ in 0..parameter_count {
        ctx.charge_work(1, "Rhino curves read_polyline records")?;
        let value = reader.f64()?;
        let Some(value) = FiniteReal::new(value) else {
            return Err(error(
                reader.position(),
                "polyline parameters are not increasing",
            ));
        };
        if parameters
            .last()
            .is_some_and(|previous: &FiniteReal| value.get() <= previous.get())
        {
            return Err(error(
                reader.position(),
                "polyline parameters are not increasing",
            ));
        }
        parameters.push(value);
    }
    let dimension = reader.i32()?;
    if expected_dimension.is_some_and(|expected| dimension != expected)
        || dimension != 2 && dimension != 3
    {
        return Err(error(reader.position(), "polyline dimension is invalid"));
    }
    let knot_count = point_count
        .checked_add(2)
        .ok_or_else(|| error(reader.position(), "polyline knot count overflow"))?;
    let mut knots = ctx
        .collection_vec(knot_count, "Rhino polyline knots")
        .map_err(crate::curves::GeometryError::from)?;
    knots.push(parameters[0].get());
    knots.push(parameters[0].get());
    knots.extend(ctx.admit_iter(&parameters[1..point_count - 1], "Rhino polyline interior knot copy").map_err(CodecError::from)?.map(|value| value.get()));
    knots.push(parameters[point_count - 1].get());
    knots.push(parameters[point_count - 1].get());
    NurbsCurve::from_checked_lanes(ctx, 1, knots, points, None, false)?.or_else(|error| {
        Err(GeometryError::malformed(
            reader.position(),
            ctx.format_retained(format_args!("{}", error), "Rhino read_polyline text")?,
        ))
    })
}

fn read_arc(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    expected_dimension: Option<i32>,
    force_nurbs: bool,
) -> Result<(CurveGeometry, Diagnostics), GeometryError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let circle = read_circle(ctx, reader, scale)?;
    let angle = interval(ctx, reader)?.0.get();
    let domain = interval(ctx, reader)?.0.get();
    let dimension = reader.i32()?;
    let mut warnings = Diagnostics::new();
    if expected_dimension.is_some_and(|expected| dimension != expected) {
        return Err(error(reader.position(), "arc dimension is invalid"));
    }
    if dimension != 2 && dimension != 3 {
        warnings.push_admitted(
            ctx,
            format_args!("arc dimension {dimension} normalized to native 3D"),
        )?;
    }
    if domain[0] >= domain[1] || angle[0] >= angle[1] {
        return Err(error(reader.position(), "arc interval is not increasing"));
    }
    let delta = angle[1] - angle[0];
    if delta <= 0.0 || delta > TAU + EPS_CURVE_DEGENERATE {
        return Err(error(reader.position(), "arc angle span is invalid"));
    }
    if !force_nurbs && canonical_circle(&circle, angle, domain, delta) {
        return Ok((
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::new(
                    circle.center,
                    OrthonormalFrame3::new(circle.axis, circle.xaxis).ok_or_else(|| {
                        error(
                            reader.position(),
                            "CircleCurve.axis/ref_direction must form an orthonormal frame",
                        )
                    })?,
                    circle.radius,
                ),
            )),
            warnings,
        ));
    }
    Ok((
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(arc_nurbs(
            ctx,
            &circle,
            angle,
            domain,
            delta,
            reader.position(),
        )?)),
        warnings,
    ))
}

#[derive(Debug, Clone, Copy)]
struct Circle {
    center: FinitePoint3,
    // The source circle uses a stricter unit and orthogonality tolerance than
    // the IR frame. These axes keep their source values until frame assembly.
    axis: Vector3,
    xaxis: Vector3,
    yaxis: Vector3,
    radius: PositiveLength,
}

fn read_circle(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
) -> Result<Circle, GeometryError> {
    let native = plane(ctx, reader)?;
    let radius = reader.f64()?;
    let zero = native_point(ctx, reader)?;
    let half_pi = native_point(ctx, reader)?;
    let at_pi = native_point(ctx, reader)?;
    let radius = PositiveReal::new(radius)
        .ok_or_else(|| error(reader.position(), "circle radius is invalid"))?;
    let scaled_radius = PositiveLength::new(radius.get() * scale.value())
        .ok_or_else(|| error(reader.position(), "circle radius is invalid"))?;
    let xaxis = vector(native.xaxis.get());
    let yaxis = vector(native.yaxis.get());
    let axis = vector(native.zaxis.get());
    let center = crate::wire::scaled_point(native.origin.get(), scale)
        .ok_or_else(|| error(reader.position(), "scaled circle center is invalid"))?;
    let norm_x = xaxis.norm();
    let norm_y = yaxis.norm();
    let norm_axis = axis.norm();
    if !(norm_x.is_finite()
        && norm_y.is_finite()
        && norm_axis.is_finite()
        && (norm_x - 1.0).abs() < CIRCLE_TOLERANCE
        && (norm_y - 1.0).abs() < CIRCLE_TOLERANCE
        && (norm_axis - 1.0).abs() < CIRCLE_TOLERANCE
        && xaxis.dot(yaxis).abs() < CIRCLE_TOLERANCE
        && xaxis.dot(axis).abs() < CIRCLE_TOLERANCE
        && yaxis.dot(axis).abs() < CIRCLE_TOLERANCE
        && crate::wire::close_vector(xaxis.cross(yaxis), axis, CIRCLE_TOLERANCE)
        && close_native_point(
            zero.0.get(),
            native.origin.get(),
            native.xaxis.get(),
            radius.get(),
        )
        && close_native_point(
            half_pi.0.get(),
            native.origin.get(),
            native.yaxis.get(),
            radius.get(),
        )
        && close_native_point(
            at_pi.0.get(),
            native.origin.get(),
            negate(native.xaxis.get()),
            radius.get(),
        ))
    {
        return Err(error(reader.position(), "circle plane axes are invalid"));
    }
    Ok(Circle {
        center,
        axis,
        xaxis,
        yaxis,
        radius: scaled_radius,
    })
}

pub(crate) fn read_polycurve(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedCurve, GeometryError> {
    let version = reader.u8()?;
    if version >> 4 != 1 {
        return Err(GeometryError::unsupported(
            reader.position() - 1,
            "unsupported polycurve payload version",
        ));
    }
    let segment_count = crate::wire::element_count(reader, 1)?;
    if segment_count == 0 {
        return Err(GeometryError::malformed(
            reader.position(),
            "polycurve has no segments",
        ));
    }
    reader.i32()?;
    reader.i32()?;
    reader.skip(48)?;
    let mut parameter_storage = ctx.reserve_scoped(0, "Rhino polycurve parameter scratch")?;
    let (parameters, end_parameter) = parameter_storage.with_storage(|| read_polycurve_parameters(ctx, reader, segment_count, "polycurve"))?;
    let mut children = ctx
        .collection_vec(segment_count, "Rhino polycurve children")
        .map_err(crate::curves::GeometryError::from)?;
    for parameter in ctx.admit_iter(parameters, "Rhino polycurve child traversal").map_err(CodecError::from)? {
        let start = reader.position();
        let wrapper = crate::chunks::chunk_at(data, start, reader.end(), archive, false)?;
        let mut wrapper_warnings = Diagnostics::new();
        let class = parse_class_wrapper(
            ctx,
            data,
            start..wrapper.next_offset(),
            archive,
            &mut wrapper_warnings,
        )?;
        reader.skip(wrapper.next_offset() - start)?;
        if !supported_class(class.class_uuid) || matches!(class.class_uuid, POINT | POINT_CLOUD) {
            return Err(GeometryError::malformed(
                start,
                "polycurve child is not a curve",
            ));
        }
        let child = decode_inner(
            ctx,
            data,
            class.class_uuid,
            class.class_data_range,
            scale,
            archive,
            depth + 1,
        )?;
        let DecodedGeometry::Curve { mut curve } = child else {
            return Err(GeometryError::malformed(
                start,
                "polycurve child is not a curve",
            ));
        };
        curve.warnings_mut().prepend(wrapper_warnings);
        children.push((parameter, curve));
    }
    Ok(DecodedCurve::Compound {
        children,
        end_parameter,
        warnings: Diagnostics::new(),
    })
}

fn read_polycurve_parameters(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    segment_count: usize,
    label: &str,
) -> Result<(Vec<FiniteReal>, FiniteReal), GeometryError> {
    let parameter_count = crate::wire::element_count(reader, 8)?;
    if parameter_count != segment_count + 1 {
        return Err(GeometryError::malformed(
            reader.position(),
            ctx.format_retained(
                format_args!("{label} parameter count mismatch"),
                "Rhino read_polycurve_parameters text",
            )?,
        ));
    }
    let mut parameters = ctx
        .collection_vec(segment_count, "Rhino polycurve parameters")
        .map_err(crate::curves::GeometryError::from)?;
    for _ in 0..segment_count {
        ctx.charge_work(1, "Rhino curves read_polycurve_parameters records")?;
        let value = reader.f64()?;
        parameters.push(checked_polycurve_parameter(
            ctx,
            parameters.last().copied(),
            value,
            reader.position(),
            label,
        )?);
    }
    let value = reader.f64()?;
    let end_parameter = checked_polycurve_parameter(
        ctx,
        parameters.last().copied(),
        value,
        reader.position(),
        label,
    )?;
    Ok((parameters, end_parameter))
}

fn checked_polycurve_parameter(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    previous: Option<FiniteReal>,
    value: f64,
    offset: usize,
    label: &str,
) -> Result<FiniteReal, GeometryError> {
    FiniteReal::new(value)
        .filter(|value| previous.is_none_or(|previous| value.get() > previous.get()))
        .map_or_else(
            || {
                Err(GeometryError::malformed(
                    offset,
                    ctx.format_retained(
                        format_args!("{label} parameters are invalid"),
                        "Rhino checked_polycurve_parameter text",
                    )?,
                ))
            },
            Ok,
        )
}

fn arc_nurbs(
    ctx: &DecodeContext<'_>,
    circle: &Circle,
    angle: [f64; 2],
    domain: [f64; 2],
    delta: f64,
    offset: usize,
) -> Result<NurbsCurve, GeometryError> {
    let spans = cadmpeg_core::convert::truncate_f64_to_usize((delta / FRAC_PI_2).ceil().max(1.0))
        .ok_or_else(|| error(offset, "arc span count exceeds address space"))?;
    let step = delta
        / cadmpeg_core::convert::f64_from_index(spans)
            .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?;
    let domain_at = |index: usize| -> Result<f64, GeometryError> {
        let fraction = cadmpeg_core::convert::f64_from_index(index)
            .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?
            / cadmpeg_core::convert::f64_from_index(spans)
                .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?;
        let ordinary = domain[0]
            + (domain[1] - domain[0])
                * cadmpeg_core::convert::f64_from_index(index)
                    .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?
                / cadmpeg_core::convert::f64_from_index(spans)
                    .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?;
        if ordinary.is_finite() {
            Ok(ordinary)
        } else {
            cadmpeg_ir::math::interpolate(domain[0], domain[1], fraction)
                .map(cadmpeg_ir::scalar::FiniteReal::get)
                .ok_or_else(|| error(offset, "arc parameter domain is invalid"))
        }
    };
    let point_count = spans
        .checked_mul(2)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| error(offset, "arc control point count overflow"))?;
    let knot_count = spans
        .checked_mul(2)
        .and_then(|count| count.checked_add(4))
        .ok_or_else(|| error(offset, "arc knot count overflow"))?;
    let mut lane_storage = ctx.reserve_scoped(0, "Rhino arc input lanes")?;
    let mut control_points = lane_storage.with_storage(|| ctx.collection_vec(point_count, "Rhino arc control points"))?;
    let mut weights = lane_storage.with_storage(|| ctx.collection_vec(point_count, "Rhino arc weights"))?;
    let mut knots = ctx
        .collection_vec(knot_count, "Rhino arc knots")
        .map_err(crate::curves::GeometryError::from)?;
    for span in 0..spans {
        let a0 = angle[0]
            + step
                * cadmpeg_core::convert::f64_from_index(span)
                    .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?;
        let a1 = angle[0]
            + step
                * cadmpeg_core::convert::f64_from_index(span + 1)
                    .ok_or_else(|| error(offset, "geometry index exceeds exact float range"))?;
        let amid = (a0 + a1) * 0.5;
        let weight = ((a1 - a0) * 0.5).cos();
        let p0 = circle_point(circle, a0);
        let pm = circle_point_scaled(circle, amid, 1.0 / weight);
        let p1 = circle_point(circle, a1);
        if span == 0 {
            control_points.push(p0);
            weights.push(1.0);
        }
        control_points.push(pm);
        weights.push(weight);
        control_points.push(p1);
        weights.push(1.0);
        let t0 = domain_at(span)?;
        let t1 = domain_at(span + 1)?;
        if span == 0 {
            knots.extend([t0, t0, t0]);
        } else {
            knots.extend([t0, t0]);
        }
        if span + 1 == spans {
            knots.extend([t1, t1, t1]);
        }
    }
    cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        ctx,
        2,
        knots,
        control_points,
        Some(weights),
        false,
    )
    .map_err(GeometryError::from)?
    .or_else(|error| {
        Err(GeometryError::malformed(
            offset,
            ctx.format_retained(format_args!("{}", error), "Rhino arc_nurbs text")?,
        ))
    })
}

fn canonical_circle(circle: &Circle, angle: [f64; 2], domain: [f64; 2], delta: f64) -> bool {
    (delta - TAU).abs() < CIRCLE_TOLERANCE
        && angle[0].abs() < CIRCLE_TOLERANCE
        && (domain[0]).abs() < CIRCLE_TOLERANCE
        && (domain[1] - TAU).abs() < CIRCLE_TOLERANCE
        && (circle.xaxis.norm() - 1.0).abs() < CIRCLE_TOLERANCE
}

fn circle_point(circle: &Circle, angle: f64) -> Point3 {
    circle_point_scaled(circle, angle, 1.0)
}

fn circle_point_scaled(circle: &Circle, angle: f64, radial_scale: f64) -> Point3 {
    let radial = Vector3::new(
        circle.xaxis.x * angle.cos() + circle.yaxis.x * angle.sin(),
        circle.xaxis.y * angle.cos() + circle.yaxis.y * angle.sin(),
        circle.xaxis.z * angle.cos() + circle.yaxis.z * angle.sin(),
    );
    let center = circle.center.get();
    let radius = circle.radius.get();
    Point3::new(
        center.x + radial.x * radius * radial_scale,
        center.y + radial.y * radius * radial_scale,
        center.z + radial.z * radius * radial_scale,
    )
}

fn native_point(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
) -> Result<NativePoint3, FramingError> {
    crate::settings::point(ctx, reader)
}

fn require_major(version: u8, offset: usize) -> Result<(), GeometryError> {
    if version >> 4 == 1 {
        Ok(())
    } else {
        Err(GeometryError::unsupported(
            offset,
            "unsupported simple-geometry payload version",
        ))
    }
}

fn negate(value: [f64; 3]) -> [f64; 3] {
    [-value[0], -value[1], -value[2]]
}

fn close_native_point(point: [f64; 3], origin: [f64; 3], direction: [f64; 3], radius: f64) -> bool {
    let expected = [
        origin[0] + direction[0] * radius,
        origin[1] + direction[1] * radius,
        origin[2] + direction[2] * radius,
    ];
    point
        .iter()
        .zip(expected)
        .all(|(actual, expected)| (*actual - expected).abs() <= EPS_CURVE_POSITION)
}

pub(crate) fn error(offset: usize, message: impl Into<String>) -> GeometryError {
    GeometryError::malformed(offset, message)
}

#[cfg(test)]
mod tests {
    #[test]
    fn degree_elevation_preserves_nonuniform_rational_spans_and_unclamped_ends() {
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(), 3,
            vec![-3., -2., -1., 0., 0.2, 0.2, 0.5, 0.7, 0.7, 0.7, 1., 2., 3., 4.],
            (0..10).map(|index| Point3::new(f64::from(index), f64::from(index % 4), 0.)).collect(),
            Some((0..10).map(|index| 1. + 0.125 * f64::from(index % 3)).collect()), false,
        ).expect("fixture admission").expect("valid nonuniform rational curve");
        let elevated = with_test_context(|ctx| super::elevate_to_degree(ctx, &curve, 5, 0)).unwrap();
        assert_eq!(elevated.degree(), 5);
        assert_eq!(elevated.knots()[5], 0.);
        assert_eq!(elevated.knots()[elevated.pole_count()], 1.);
        for [start, end] in [[0., 0.2], [0.2, 0.5], [0.5, 0.7], [0.7, 1.]] {
            for fraction in [0., 0.125, 0.5, 0.875, 1.] {
                let at = start + fraction * (end - start);
                let expected = cadmpeg_ir::eval::decode::curve_point_solved(
                    cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                    &SolvedCurveGeometry::Nurbs(curve.clone()), at,
                ).unwrap();
                let actual = cadmpeg_ir::eval::decode::curve_point_solved(
                    cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                    &SolvedCurveGeometry::Nurbs(elevated.clone()), at,
                ).unwrap();
                assert!(actual.distance(expected.get()) <= 2048. * f64::EPSILON);
            }
        }
    }

    #[test]
    fn endpoint_clamping_work_refusal_preserves_source_lanes() {
        let knots = vec![-1., -1., 0., 1., 2., 2.];
        let points = vec![[0., 0., 0., 1.], [1., 1., 0., 1.], [2., 0., 0., 1.]];
        cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "Rhino endpoint local pole copy", |cap| {
                let arena = cadmpeg_core::decode::DecodeArena::new();
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut source_knots = knots.clone();
                let mut source_points = points.iter().copied().map(super::Homogeneous).collect();
                let mut storage = ctx.reserve_scoped(0, "Rhino clamping test source").unwrap();
                let result = super::clamp_endpoint(&ctx, &mut storage, &mut source_knots, &mut source_points, 2, 0., 0);
                assert_eq!(source_knots, knots);
                assert_eq!(source_points.iter().map(|point| point.0).collect::<Vec<_>>(), points);
                drop(storage);
                if let Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) = &result {
                    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit));
                }
                result.map_err(|error| match error { GeometryError::Codec(error) => error, error => panic!("unexpected clamping refusal: {error:?}") })
            },
        );
    }
    #[test]
    fn line_allocations_refuse_at_the_retained_output_lanes() {
        let mut bytes = vec![0x10];
        for value in [0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 5.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(3_i32.to_le_bytes());
        for operation in ["Rhino line knots", "Rhino line control points"] {
            cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes, operation, |cap| {
                    let arena = cadmpeg_core::decode::DecodeArena::new();
                    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                    policy.limits.max_retained_bytes = cap;
                    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
                    let result = super::read_line(&ctx, &mut reader, MillimeterScale::IDENTITY, None);
                    if let Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) = &result {
                        assert_eq!(ctx.resource_refusal(), Some(*limit));
                    }
                    result.map_err(|error| match error { GeometryError::Codec(error) => error, error => panic!("unexpected line refusal: {error:?}") })
                },
            );
        }
    }

    #[test]
    fn polyline_interior_knot_copy_refuses_before_variable_copy() {
        let mut bytes = vec![0x10];
        bytes.extend(3_i32.to_le_bytes());
        for value in [0.0_f64, 0.0, 0.0, 1.0, 2.0, 0.0, 2.0, 3.0, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(3_i32.to_le_bytes());
        for value in [10.0_f64, 12.0, 15.0] { bytes.extend(value.to_le_bytes()); }
        bytes.extend(3_i32.to_le_bytes());
        // Three vertices leave one interior parameter to copy into the output knots.
        cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "Rhino polyline interior knot copy", |cap| {
                let arena = cadmpeg_core::decode::DecodeArena::new();
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
                let result = super::read_polyline(&ctx, &mut reader, MillimeterScale::IDENTITY, None);
                if let Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) = &result {
                    assert_eq!(limit.additional, 1);
                    assert_eq!(ctx.resource_refusal(), Some(*limit));
                }
                result.map_err(|error| match error { GeometryError::Codec(error) => error, error => panic!("unexpected polyline refusal: {error:?}") })
            },
        );
        let curve = with_test_context(|ctx| {
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
            super::read_polyline(ctx, &mut reader, MillimeterScale::IDENTITY, None)
        }).unwrap();
        assert_eq!(curve.knots().as_slice(), [10., 10., 12., 15., 15.]);
        assert_eq!(curve.pole_count(), 3);
    }

    #[test]
    fn numerical_audit_nurbs_elevation_preserves_active_spans_and_discontinuities() {
        use cadmpeg_ir::geometry::SolvedCurveGeometry;
        let cases = [
            NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                2,
                vec![-1.0, -1.0, 0.0, 1.0, 2.0, 2.0],
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 1.0, 0.0),
                    Point3::new(2.0, 0.0, 0.0),
                ],
                None,
                false,
            )
            .expect("fixture constructor admission")
            .unwrap(),
            NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0],
                (0..6)
                    .map(|x| Point3::new(f64::from(x), 0.0, 0.0))
                    .collect(),
                None,
                false,
            )
            .expect("fixture constructor admission")
            .unwrap(),
        ];
        for curve in cases {
            for degree in [2, 3] {
                let elevated =
                    with_test_context(|ctx| super::elevate_to_degree(ctx, &curve, degree, 0))
                        .unwrap();
                let start = curve.knots()
                    [usize::try_from(curve.degree()).expect("fixture value fits usize")];
                let end = curve.knots()[curve.control_points().len()];
                assert_eq!(elevated.knots()[degree], start);
                assert_eq!(elevated.knots()[elevated.control_points().len()], end);
                for fraction in [0.0, 0.125, 0.25, 0.5, 0.625, 0.875, 1.0] {
                    let at = start + (end - start) * fraction;
                    let expected = cadmpeg_ir::eval::decode::curve_point_solved(
                        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                        &SolvedCurveGeometry::Nurbs(curve.clone()),
                        at,
                    )
                    .unwrap();
                    let actual = cadmpeg_ir::eval::decode::curve_point_solved(
                        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                        &SolvedCurveGeometry::Nurbs(elevated.clone()),
                        at,
                    )
                    .unwrap();
                    assert!(actual.distance(expected.get()) <= 64.0 * f64::EPSILON);
                }
            }
        }
    }

    #[test]
    fn numerical_audit_join_preserves_independently_scaled_rational_segments() {
        use cadmpeg_ir::geometry::SolvedCurveGeometry;
        let first = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point3::new(-2.0, 0.0, 0.0),
                Point3::new(-1.0, 0.0, 0.0),
                Point3::new(0.0, 0.0, 0.0),
            ],
            Some(vec![2.0; 3]),
            false,
        )
        .expect("fixture constructor admission")
        .unwrap();
        let second = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            Some(vec![1.0; 3]),
            false,
        )
        .expect("fixture constructor admission")
        .unwrap();
        let joined =
            with_test_context(|ctx| super::join_nurbs_segments(ctx, vec![first, second], 0))
                .unwrap();
        let actual = cadmpeg_ir::eval::decode::curve_point_solved(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &SolvedCurveGeometry::Nurbs(joined.curve),
            1.5,
        )
        .unwrap();
        assert_eq!(actual, Point3::new(0.25, 0.75, 0.0));
    }

    #[test]
    fn numerical_audit_remap_and_join_keep_large_finite_values() {
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0e-200, 1.0e-200],
            vec![Point3::new(f64::MAX, 0.0, 0.0); 2],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .unwrap();
        let remapped = with_test_context(|ctx| {
            super::remap_nurbs_domain(
                ctx,
                curve,
                [FiniteReal::ZERO, FiniteReal::new(1.0e200).unwrap()],
                0,
            )
        })
        .unwrap();
        assert_eq!(remapped.knots().as_slice(), &[0.0, 0.0, 1.0e200, 1.0e200]);
        let joined = with_test_context(|ctx| {
            super::join_nurbs_segments(ctx, vec![remapped.clone(), remapped], 0)
        })
        .unwrap();
        assert!(joined
            .curve
            .control_points()
            .iter()
            .all(|point| point.x == f64::MAX));
    }

    use super::{
        arc_nurbs, canonical_circle, checked_polycurve_parameter, circle_point, exact_nurbs,
        join_nurbs_segments, read_line, read_polyline, scale_decoded_curve, Circle, DecodedCurve,
        GeometryError, CURVE_ON_SURFACE, LINE, MAX_CURVE_DEPTH,
    };
    use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
    use crate::loss::Diagnostics;
    use crate::settings::MillimeterScale;
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::analytic::{CircleCurve, DegenerateCurve, LineCurve};
    use cadmpeg_ir::geometry::{nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::scalar::{FiniteReal, PositiveLength};
    use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};

    const EPS_EXACT_ARC: f64 = 1.0e-12;

    fn with_test_context<R>(f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> R) -> R {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("test context input fits service profile");
        f(&ctx)
    }

    fn with_collection_limit<R>(
        limit: u64,
        f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> R,
    ) -> R {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("test context input fits service profile");
        f(&ctx)
    }

    fn with_retained_limit<R>(
        limit: u64,
        f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> R,
    ) -> R {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty test input fits service profile");
        f(&ctx)
    }

    fn rational_line_for_limits() -> NurbsCurve {
        NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            Some(vec![2.0, 1.0]),
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid rational line")
    }

    fn exact_line_for_limits() -> DecodedCurve {
        DecodedCurve::leaf(
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(rational_line_for_limits())),
            Diagnostics::new(),
        )
    }

    #[test]
    fn exact_nurbs_knots_refuse_collection_limit() {
        let error = with_collection_limit(3, |ctx| exact_nurbs(ctx, &exact_line_for_limits(), 0))
            .expect_err("four knot copies exceed three collection items");
        assert!(matches!(
            error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.operation == "Rhino exact NURBS copy"
        ));
    }

    #[test]
    fn exact_nurbs_poles_refuse_collection_limit() {
        let error = with_collection_limit(5, |ctx| exact_nurbs(ctx, &exact_line_for_limits(), 0))
            .expect_err("two copied poles exceed five collection items after four knots");
        assert!(matches!(
            error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.operation == "Rhino exact NURBS copy"
        ));
        assert!(with_test_context(|ctx| exact_nurbs(ctx, &exact_line_for_limits(), 0)).is_ok());
    }

    #[test]
    fn exact_nurbs_copy_refuses_retained_limit_one_byte_below_full_copy() {
        let curve = rational_line_for_limits();
        let bytes = curve.knots().len() * std::mem::size_of::<f64>()
            + curve.pole_count()
                * std::mem::size_of::<cadmpeg_ir::geometry::nurbs::WeightedPole3<FinitePoint3>>();
        let limit = u64::try_from(bytes - 1).expect("copy fits in u64");
        let error = with_retained_limit(limit, |ctx| exact_nurbs(ctx, &exact_line_for_limits(), 0))
            .expect_err("the full copy exceeds the retained limit by one byte");
        assert!(matches!(
            error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.operation == "Rhino exact NURBS copy"
                    && refusal.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
        ));
    }

    #[test]
    fn remapped_nurbs_knots_refuse_collection_limit() {
        let target = [
            FiniteReal::new(2.0).expect("finite"),
            FiniteReal::new(3.0).expect("finite"),
        ];
        let error = with_collection_limit(3, |ctx| {
            super::remap_nurbs_domain(ctx, rational_line_for_limits(), target, 0)
        })
        .expect_err("four remapped knots exceed three collection items");
        assert!(matches!(
            error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.operation == "Rhino remapped NURBS knots"
        ));
        let remapped = with_test_context(|ctx| {
            super::remap_nurbs_domain(ctx, rational_line_for_limits(), target, 0)
        })
        .expect("service profile admits remapped knots");
        assert_eq!(remapped.knots().as_slice(), &[2.0, 2.0, 3.0, 3.0]);
    }

    fn assert_elevation_refusal(target: usize, operation: &'static str) {
        let curve = rational_line_for_limits();
        cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            operation,
            |cap| with_collection_limit(cap, |ctx| super::elevate_to_degree(ctx, &curve, target, 0))
                .map_err(|error| match error { GeometryError::Codec(error) => error, error => panic!("unexpected elevation refusal: {error:?}") }),
        );
    }

    fn two_point_polyline_bytes() -> Vec<u8> {
        let mut bytes = vec![0x10];
        bytes.extend(2_i32.to_le_bytes());
        for value in [0.0_f64, 0.0, 0.0, 1.0, 2.0, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(2_i32.to_le_bytes());
        bytes.extend(10.0_f64.to_le_bytes());
        bytes.extend(12.0_f64.to_le_bytes());
        bytes.extend(3_i32.to_le_bytes());
        bytes
    }

    fn assert_polyline_refusal(limit: u64, operation: &str) {
        let bytes = two_point_polyline_bytes();
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded polyline");
        let error = with_collection_limit(limit, |ctx| {
            read_polyline(ctx, &mut reader, MillimeterScale::IDENTITY, None)
        })
        .expect_err("collection limit is below the polyline's need");
        assert!(matches!(
            error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.operation == operation
        ));
    }

    fn assert_join_refusal(operation: &'static str) {
        let segments = vec![rational_line_for_limits(), rational_line_for_limits()];
        cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            operation,
            |cap| with_collection_limit(cap, |ctx| {
                let result = super::join_nurbs_segments(ctx, segments.clone(), 0);
                if let Err(GeometryError::Codec(CodecError::ResourceLimit(first))) = &result {
                    assert_eq!(ctx.resource_refusal(), Some(*first));
                }
                result.map_err(|error| match error { GeometryError::Codec(error) => error, error => panic!("unexpected join refusal: {error:?}") })
            }),
        );
    }

    #[test]
    fn elevated_polycurve_segments_refuse_collection_limit() {
        assert_join_refusal("Rhino elevated polycurve segments");
    }

    #[test]
    fn joined_polycurve_weights_refuse_collection_limit() {
        assert_join_refusal("Rhino joined polycurve weights");
    }

    #[test]
    fn joined_polycurve_points_refuse_collection_limit() {
        assert_join_refusal("Rhino joined polycurve points");
    }

    #[test]
    fn joined_polycurve_knots_refuse_collection_limit() {
        assert_join_refusal("Rhino joined polycurve knots");
    }

    #[test]
    fn polyline_points_refuse_collection_limit() {
        assert_polyline_refusal(1, "Rhino polyline points");
    }

    #[test]
    fn polyline_parameters_refuse_collection_limit() {
        assert_polyline_refusal(3, "Rhino polyline parameters");
    }

    #[test]
    fn polyline_knots_refuse_collection_limit() {
        assert_polyline_refusal(7, "Rhino polyline knots");
    }

    #[test]
    fn polycurve_parameters_refuse_collection_limit() {
        let mut bytes = 2_i32.to_le_bytes().to_vec();
        bytes.extend(0.0_f64.to_le_bytes());
        bytes.extend(1.0_f64.to_le_bytes());
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded parameters");
        let error = with_collection_limit(0, |ctx| {
            super::read_polycurve_parameters(ctx, &mut reader, 1, "polycurve")
        })
        .expect_err("one polycurve parameter exceeds zero collection items");
        assert!(matches!(
            error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.operation == "Rhino polycurve parameters"
        ));
    }

    #[test]
    fn polycurve_homogeneous_points_refuse_collection_limit() {
        assert_elevation_refusal(1, "Rhino polycurve homogeneous points");
    }

    #[test]
    fn polycurve_knots_refuse_collection_limit() {
        assert_elevation_refusal(1, "Rhino polycurve knots");
    }


    #[test]
    fn polycurve_bezier_span_refuses_collection_limit() {
        assert_elevation_refusal(1, "Rhino polycurve Bezier span");
    }

    #[test]
    fn polycurve_bezier_elevation_refuses_collection_limit() {
        assert_elevation_refusal(2, "Rhino polycurve Bezier elevation");
    }

    #[test]
    fn polycurve_elevated_points_refuse_collection_limit() {
        assert_elevation_refusal(1, "Rhino polycurve elevated points");
    }

    #[test]
    fn polycurve_final_knots_refuse_collection_limit() {
        assert_elevation_refusal(1, "Rhino polycurve final knots");
    }

    #[test]
    fn polycurve_output_weights_refuse_collection_limit() {
        assert_elevation_refusal(1, "Rhino polycurve output weights");
    }

    #[test]
    fn polycurve_output_points_refuse_collection_limit() {
        assert_elevation_refusal(1, "Rhino polycurve output points");
    }



    #[test]
    fn polycurve_elevated_knots_refuse_collection_limit() {
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            Some(vec![1.0, 1.0]),
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid rational line");
        let error = with_collection_limit(1, |ctx| super::elevate_to_degree(ctx, &curve, 1, 0))
            .expect_err("two initial knots exceed one collection item");
        assert!(matches!(
            error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "Rhino polycurve elevated knots"
        ));
        with_test_context(|ctx| super::elevate_to_degree(ctx, &curve, 1, 0))
            .expect("service profile admits elevated knots");
    }

    fn read_cloud(
        reader: &mut BoundedReader<'_>,
        scale: MillimeterScale,
    ) -> Result<super::PointCloud, GeometryError> {
        with_test_context(|ctx| super::read_cloud(ctx, reader, scale))
    }

    fn decode_inner(
        data: &[u8],
        class: crate::wire::Uuid,
        range: std::ops::Range<usize>,
        scale: MillimeterScale,
        archive: ArchiveVersion,
        depth: usize,
    ) -> Result<super::DecodedGeometry, GeometryError> {
        with_test_context(|ctx| super::decode_inner(ctx, data, class, range, scale, archive, depth))
    }

    fn read_polycurve(
        data: &[u8],
        reader: &mut BoundedReader<'_>,
        scale: MillimeterScale,
        archive: ArchiveVersion,
        depth: usize,
    ) -> Result<DecodedCurve, GeometryError> {
        with_test_context(|ctx| super::read_polycurve(ctx, data, reader, scale, archive, depth))
    }

    fn read_polycurve_2d(
        data: &[u8],
        reader: &mut BoundedReader<'_>,
        archive: ArchiveVersion,
        depth: usize,
    ) -> Result<DecodedCurve, GeometryError> {
        with_test_context(|ctx| super::read_polycurve_2d(ctx, data, reader, archive, depth))
    }

    /// A point cloud whose optional channels disagree with the point count.
    fn mismatched_point_cloud_payload() -> Vec<u8> {
        let mut payload = vec![0x12];
        payload.extend(2_i32.to_le_bytes());
        for point in [[0.0_f64, 0.0, 0.0], [1.0, 1.0, 1.0]] {
            payload.extend(point.into_iter().flat_map(f64::to_le_bytes));
        }
        payload.extend(
            [
                0.0_f64, 0.0, 0.0, // origin
                1.0, 0.0, 0.0, // x
                0.0, 1.0, 0.0, // y
                0.0, 0.0, 1.0, // z
                0.0, 0.0, 1.0, 0.0, // equation
            ]
            .into_iter()
            .flat_map(f64::to_le_bytes),
        );
        payload.extend([0.0_f64; 6].into_iter().flat_map(f64::to_le_bytes));
        payload.extend(0_i32.to_le_bytes());
        payload.extend(1_i32.to_le_bytes());
        payload.extend([0.0_f64, 0.0, 1.0].into_iter().flat_map(f64::to_le_bytes));
        payload.extend(1_i32.to_le_bytes());
        payload.extend([0_u8; 4]);
        payload.extend(1_i32.to_le_bytes());
        payload.extend(0.5_f64.to_le_bytes());
        payload
    }

    #[test]
    fn point_cloud_points_refuse_collection_limit_before_reserve() {
        let payload = mismatched_point_cloud_payload();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy)
                .expect("point-cloud input fits service profile");
        let mut reader =
            BoundedReader::new(&payload, 0, payload.len()).expect("point-cloud bounds");
        let refusal = super::read_cloud(&ctx, &mut reader, MillimeterScale::IDENTITY)
            .expect_err("two points exceed one collection item");
        assert!(
            matches!(refusal, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "Rhino point-cloud points")
        );
        let mut reader =
            BoundedReader::new(&payload, 0, payload.len()).expect("point-cloud bounds");
        assert!(read_cloud(&mut reader, MillimeterScale::IDENTITY).is_ok());
    }

    /// A refused scalar names its own first byte, not the byte after it.
    #[test]
    fn nonfinite_point_cloud_value_is_refused_at_the_value_first_byte() {
        let mut payload = mismatched_point_cloud_payload();
        let value_offset = payload.len() - 8;
        payload[value_offset..].copy_from_slice(&f64::NAN.to_le_bytes());
        let mut reader = BoundedReader::new(&payload, 0, payload.len()).expect("reader");
        let error = read_cloud(&mut reader, MillimeterScale::IDENTITY)
            .expect_err("nonfinite point-cloud value");
        assert!(matches!(
            error,
            GeometryError::Malformed(FramingError::Structural { offset, ref message })
                if offset == value_offset && message == "point-cloud value is not finite"
        ));
    }

    /// Every redundant point-cloud channel repair carries the repair code itself.
    #[test]
    fn point_cloud_channel_repairs_carry_the_redundant_field_code() {
        let payload = mismatched_point_cloud_payload();
        let mut reader = BoundedReader::new(&payload, 0, payload.len()).expect("reader");
        let cloud = read_cloud(&mut reader, MillimeterScale::IDENTITY).expect("point cloud");
        assert_eq!(
            cloud
                .warnings
                .iter()
                .map(|diagnostic| (diagnostic.code, diagnostic.message.as_str()))
                .collect::<Vec<_>>(),
            [
                (
                    Some(crate::loss::RhinoLossCode::RedundantFieldRepaired),
                    "redundant point-cloud normal count mismatch; channel dropped"
                ),
                (
                    Some(crate::loss::RhinoLossCode::RedundantFieldRepaired),
                    "redundant point-cloud color count mismatch; channel dropped"
                ),
                (
                    Some(crate::loss::RhinoLossCode::RedundantFieldRepaired),
                    "redundant point-cloud scalar count mismatch; channel dropped"
                ),
            ]
        );
    }

    #[test]
    fn point_cloud_channel_diagnostic_refuses_collection_limit() {
        let payload = mismatched_point_cloud_payload();
        let refusal = with_collection_limit(2, |ctx| {
            let mut reader = BoundedReader::new(&payload, 0, payload.len()).expect("reader");
            super::read_cloud(ctx, &mut reader, MillimeterScale::IDENTITY)
                .expect_err("the first channel diagnostic exceeds two collection items")
        });
        assert!(matches!(
            refusal,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "Rhino diagnostics"
        ));
    }

    #[test]
    fn stored_count_above_legacy_limit_is_bounded_by_payload() {
        let item_count = 65_537_usize;
        let mut bytes = (i32::try_from(item_count).expect("fixture value fits i32"))
            .to_le_bytes()
            .to_vec();
        bytes.resize(4 + item_count, 0);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        assert_eq!(
            crate::wire::element_count(&mut reader, 1).expect("payload-bounded count"),
            item_count
        );
    }

    #[test]
    fn curve_on_surface_obeys_the_shared_curve_recursion_limit() {
        let error = decode_inner(
            &[],
            CURVE_ON_SURFACE,
            0..0,
            MillimeterScale::IDENTITY,
            ArchiveVersion::V8,
            MAX_CURVE_DEPTH + 1,
        )
        .expect_err("excessive cross-family recursion must stop before payload parsing");
        assert!(
            matches!(error, GeometryError::Codec(CodecError::ResourceLimit(refusal))
            if refusal.operation == "Rhino curve depth limit" && refusal.limit == 32 && refusal.additional == 1)
        );
    }
    #[test]
    fn curve_depth_gates_preserve_context_refusal() {
        for gate in 0..4 {
            with_test_context(|ctx| {
                let mut reader = BoundedReader::new(&[], 0, 0).expect("empty reader");
                let depth = MAX_CURVE_DEPTH + 1;
                let error = match gate {
                    0 => super::decode_inner(
                        ctx,
                        &[],
                        LINE,
                        0..0,
                        MillimeterScale::IDENTITY,
                        ArchiveVersion::V8,
                        depth,
                    )
                    .expect_err("depth"),
                    1 => super::decode_inner_2d(ctx, &[], LINE, 0..0, ArchiveVersion::V8, depth)
                        .expect_err("depth"),
                    2 => super::decode_embedded_curve(
                        ctx,
                        &[],
                        &mut reader,
                        MillimeterScale::IDENTITY,
                        ArchiveVersion::V8,
                        depth,
                    )
                    .expect_err("depth"),
                    _ => super::decode_embedded_curve_2d(
                        ctx,
                        &[],
                        &mut reader,
                        MillimeterScale::IDENTITY,
                        ArchiveVersion::V8,
                        depth,
                    )
                    .expect_err("depth"),
                };
                let GeometryError::Codec(CodecError::ResourceLimit(refusal)) = error else {
                    panic!("depth must be a resource refusal");
                };
                assert_eq!(refusal.limit, 32);
                assert_eq!(refusal.additional, 1);
                assert_eq!(ctx.resource_refusal(), Some(refusal));
            });
        }
    }
    use std::f64::consts::{PI, TAU};

    fn unit_circle() -> Circle {
        Circle {
            center: FinitePoint3::new(Point3::new(2.0, -1.0, 3.0)).expect("finite center"),
            axis: Vector3::new(0.0, 0.0, 1.0),
            xaxis: Vector3::new(1.0, 0.0, 0.0),
            yaxis: Vector3::new(0.0, 1.0, 0.0),
            radius: PositiveLength::new(4.0).expect("positive radius"),
        }
    }

    #[test]
    fn source_circle_keeps_checked_center_and_radius() {
        let arena = cadmpeg_core::decode::DecodeArena::default();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("context");
        let values = [
            1.0_f64, 2.0, 3.0, // plane origin
            1.0, 0.0, 0.0, // x axis
            0.0, 1.0, 0.0, // y axis
            0.0, 0.0, 1.0, // z axis
            0.0, 0.0, 1.0, -3.0, // plane equation
            2.0,  // radius
            3.0, 2.0, 3.0, // zero angle
            1.0, 4.0, 3.0, // quarter turn
            -1.0, 2.0, 3.0, // half turn
        ];
        let mut bytes = values
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("circle reader");
        let circle = super::read_circle(
            &ctx,
            &mut reader,
            crate::test_support::millimeter_scale(2.0),
        )
        .expect("valid circle");
        assert_eq!(circle.center.get(), Point3::new(2.0, 4.0, 6.0));
        assert_eq!(circle.radius.get(), 4.0);

        bytes[128..136].copy_from_slice(&0.0_f64.to_le_bytes());
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("circle reader");
        let error = super::read_circle(&ctx, &mut reader, MillimeterScale::IDENTITY)
            .expect_err("zero circle radius");
        assert!(error.to_string().contains("circle radius is invalid"));
    }

    #[test]
    fn arc_nurbs_preserves_endpoints_midpoint_and_weights() {
        let circle = unit_circle();
        let arc = with_test_context(|ctx| arc_nurbs(ctx, &circle, [0.0, PI], [10.0, 20.0], PI, 0))
            .expect("valid arc");
        assert_eq!(arc.degree(), 2);
        assert_eq!(
            arc.pole_rows().raw_points().first(),
            Some(&circle_point(&circle, 0.0))
        );
        assert_eq!(
            arc.pole_rows().raw_points().last(),
            Some(&circle_point(&circle, PI))
        );
        assert_eq!(
            arc.weights().expect("rational arc")[1].get(),
            2.0_f64.sqrt() / 2.0
        );
        let midpoint = circle_point(&circle, PI / 2.0);
        let pole = arc.control_points()[2];
        let weight = arc.weights().expect("rational arc")[2].get();
        assert!((pole.x * weight - midpoint.x).abs() < EPS_EXACT_ARC);
        assert!((pole.y * weight - midpoint.y).abs() < EPS_EXACT_ARC);
    }

    fn arc_collection_refusal(limit: u64, operation: &str) {
        let error = with_collection_limit(limit, |ctx| {
            arc_nurbs(ctx, &unit_circle(), [0.0, PI], [10.0, 20.0], PI, 0)
        })
        .expect_err("arc arrays exceed the collection limit");
        assert!(matches!(
            error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                if refusal.operation == operation
        ));
    }

    #[test]
    fn arc_control_points_refuse_collection_limit() {
        arc_collection_refusal(4, "Rhino arc control points");
    }

    #[test]
    fn arc_weights_refuse_collection_limit() {
        arc_collection_refusal(9, "Rhino arc weights");
    }

    #[test]
    fn arc_knots_refuse_collection_limit() {
        arc_collection_refusal(17, "Rhino arc knots");
    }

    #[test]
    fn arc_nurbs_maps_a_wide_finite_parameter_domain() {
        let arc = with_test_context(|ctx| {
            arc_nurbs(ctx, &unit_circle(), [0.0, PI], [-f64::MAX, f64::MAX], PI, 0)
        })
        .expect("wide arc domain has finite knots");
        let knots = arc.knots().as_slice();
        assert_eq!(knots.first(), Some(&-f64::MAX));
        assert!(knots.contains(&0.0));
        assert_eq!(knots.last(), Some(&f64::MAX));
    }

    #[test]
    fn full_canonical_circle_is_analytic_but_shifted_circle_is_rational() {
        let circle = unit_circle();
        assert!(canonical_circle(&circle, [0.0, TAU], [0.0, TAU], TAU));
        let rounded = Circle {
            xaxis: Vector3::new(1.0 - f64::EPSILON, 0.0, 0.0),
            ..circle
        };
        assert!(canonical_circle(&rounded, [0.0, TAU], [0.0, TAU], TAU));
        assert!(!canonical_circle(
            &circle,
            [0.25, 0.25 + TAU],
            [0.0, TAU],
            TAU
        ));
        assert!(!canonical_circle(&circle, [0.0, TAU], [2.0, 4.0], TAU));
    }

    #[test]
    fn arc_spans_never_exceed_quarter_turn() {
        let circle = unit_circle();
        let arc = with_test_context(|ctx| {
            arc_nurbs(ctx, &circle, [0.0, 3.0 * PI], [0.0, 3.0], 3.0 * PI, 0)
        })
        .expect("valid arc");
        assert_eq!(arc.control_points().len(), 2 * 6 + 1);
        assert_eq!(arc.knots().len(), arc.control_points().len() + 3);
    }

    #[test]
    fn bounded_line_accepts_both_serialized_dimensions() {
        for dimension in [2_i32, 3] {
            let mut bytes = vec![0x10];
            for value in [0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 5.0] {
                bytes.extend(value.to_le_bytes());
            }
            bytes.extend(dimension.to_le_bytes());
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded");
            let curve = ({
                let arena = cadmpeg_core::decode::DecodeArena::new();
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &arena,
                    &cadmpeg_core::decode::DecodePolicy::service(),
                )
                .expect("root");
                read_line(&ctx, &mut reader, MillimeterScale::IDENTITY, None)
            })
            .expect("valid line");
            assert_eq!(curve.knots().as_slice(), vec![2.0, 2.0, 5.0, 5.0]);
        }
    }

    #[test]
    fn bounded_polyline_accepts_both_serialized_dimensions() {
        for dimension in [2_i32, 3] {
            let mut bytes = vec![0x10];
            bytes.extend(2_i32.to_le_bytes());
            for value in [0.0_f64, 0.0, 0.0, 1.0, 2.0, 0.0] {
                bytes.extend(value.to_le_bytes());
            }
            bytes.extend(2_i32.to_le_bytes());
            bytes.extend(10.0_f64.to_le_bytes());
            bytes.extend(12.0_f64.to_le_bytes());
            bytes.extend(dimension.to_le_bytes());
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded");
            let curve = with_test_context(|ctx| {
                read_polyline(ctx, &mut reader, MillimeterScale::IDENTITY, None)
            })
            .expect("valid polyline");
            assert_eq!(curve.control_points().len(), 2);
            assert_eq!(curve.knots().as_slice(), vec![10.0, 10.0, 12.0, 12.0]);
        }
    }

    #[test]
    fn bounded_polyline_refuses_nonfinite_and_decreasing_parameters_at_source() {
        for (parameters, refused_offset) in [([f64::NAN, 12.0], 65), ([10.0, 9.0], 73)] {
            let mut bytes = vec![0x10];
            bytes.extend(2_i32.to_le_bytes());
            for value in [0.0_f64, 0.0, 0.0, 1.0, 2.0, 0.0] {
                bytes.extend(value.to_le_bytes());
            }
            bytes.extend(2_i32.to_le_bytes());
            for value in parameters {
                bytes.extend(value.to_le_bytes());
            }
            bytes.extend(3_i32.to_le_bytes());
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded");
            let result = with_test_context(|ctx| {
                read_polyline(ctx, &mut reader, MillimeterScale::IDENTITY, None)
            });
            assert!(matches!(
                result,
                Err(GeometryError::Malformed(FramingError::Structural { offset, message }))
                    if offset == refused_offset && message == "polyline parameters are not increasing"
            ));
        }
    }

    #[test]
    fn plane_space_curve_scaling_preserves_work_and_depth_refusals() {
        let original = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0., 0., 1., 1.],
            vec![Point3::new(1., 2., 3.); 2],
            None,
            false,
        )
        .expect("admission")
        .expect("curve");
        let leaf = || {
            DecodedCurve::leaf(
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(original.clone())),
                Diagnostics::new(),
            )
        };
        // Two poles are visited once for validation and once for mutation.
        // Each probe locates its pass without a function-entry work unit.
        for operation in ["IR pole edit validation", "IR pole edit mutation"] {
            cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                operation,
                |cap| {
                    let arena = cadmpeg_core::decode::DecodeArena::new();
                    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                    policy.limits.max_work_units = cap;
                    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                    let mut decoded = leaf();
                    let Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) = scale_decoded_curve(&ctx, &mut decoded, crate::test_support::millimeter_scale(2.), 17) else {
                        panic!("work refusal must escape geometry fallback");
                    };
                    assert_eq!(decoded.reported_geometry(), &CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(original.clone())));
                    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
                    assert_eq!(limit.additional, 2);
                    Err::<(), _>(CodecError::ResourceLimit(limit))
                },
            );
        }
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("root");
        let mut decoded = DecodedCurve::Compound {
            children: vec![(FiniteReal::ZERO, leaf())],
            end_parameter: FiniteReal::ONE,
            warnings: Diagnostics::new(),
        };
        let Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) = scale_decoded_curve(
            &ctx,
            &mut decoded,
            crate::test_support::millimeter_scale(2.),
            17,
        ) else {
            panic!("session depth refusal must escape");
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::RecursionDepth
        );
        let DecodedCurve::Compound { children, .. } = decoded else {
            panic!("compound retained");
        };
        assert_eq!(
            children[0].1.reported_geometry(),
            &CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(original))
        );
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }

    #[test]
    fn plane_space_nurbs_scaling_rejects_coordinate_overflow() {
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(2.0, 0.0, 0.0), Point3::new(3.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid test curve");
        let mut decoded = DecodedCurve::leaf(
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            Diagnostics::new(),
        );
        let error = scale_decoded_curve(
            &cadmpeg_test_support::service_decode_context(),
            &mut decoded,
            crate::test_support::millimeter_scale(f64::MAX),
            17,
        )
        .expect_err("scaling overflow must reject the NURBS curve");
        assert!(error
            .to_string()
            .contains("scaled plane-space curve is invalid"));
        let DecodedCurve::Leaf {
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            ..
        } = decoded
        else {
            unreachable!("test retains the NURBS curve carrier");
        };
        assert_eq!(curve.control_points()[0], Point3::new(2.0, 0.0, 0.0));
    }

    #[test]
    fn plane_space_circle_scaling_keeps_admitted_center_and_frame() {
        let center = FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("finite center");
        let circle = CircleCurve::new(
            center,
            OrthonormalFrame3::IDENTITY,
            PositiveLength::new(2.0).expect("positive radius"),
        );
        let mut decoded = DecodedCurve::leaf(
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)),
            Diagnostics::new(),
        );
        scale_decoded_curve(
            &cadmpeg_test_support::service_decode_context(),
            &mut decoded,
            crate::test_support::millimeter_scale(2.0),
            17,
        )
        .expect("scaled circle");
        let DecodedCurve::Leaf {
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)),
            ..
        } = decoded
        else {
            panic!("circle remains solved");
        };
        assert_eq!(circle.center().get(), Point3::new(2.0, 4.0, 6.0));
        assert_eq!(circle.radius().get(), 4.0);
        assert_eq!(*circle.frame(), OrthonormalFrame3::IDENTITY);
    }

    #[test]
    fn plane_space_line_scaling_keeps_admitted_origin_and_direction() {
        let origin = FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("finite origin");
        let line = LineCurve::new(origin, UnitVector3::Z_AXIS);
        let mut decoded = DecodedCurve::leaf(
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line)),
            Diagnostics::new(),
        );
        scale_decoded_curve(
            &cadmpeg_test_support::service_decode_context(),
            &mut decoded,
            crate::test_support::millimeter_scale(2.0),
            17,
        )
        .expect("scaled line");
        let DecodedCurve::Leaf {
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(line)),
            ..
        } = decoded
        else {
            panic!("line remains solved");
        };
        assert_eq!(line.origin().get(), Point3::new(2.0, 4.0, 6.0));
        assert_eq!(line.direction(), UnitVector3::Z_AXIS);
    }

    #[test]
    fn plane_space_degenerate_scaling_keeps_admitted_point() {
        let point = FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("finite point");
        let curve = DegenerateCurve::new(point);
        let mut decoded = DecodedCurve::leaf(
            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(curve)),
            Diagnostics::new(),
        );
        scale_decoded_curve(
            &cadmpeg_test_support::service_decode_context(),
            &mut decoded,
            crate::test_support::millimeter_scale(2.0),
            17,
        )
        .expect("scaled degenerate curve");
        let DecodedCurve::Leaf {
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(curve)),
            ..
        } = decoded
        else {
            panic!("degenerate curve remains solved");
        };
        assert_eq!(curve.point().get(), Point3::new(2.0, 4.0, 6.0));
    }

    #[test]
    fn cadir_rejects_future_polycurve_major_for_typed_admission() {
        let bytes = [0x20];
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded");
        let result = read_polycurve(
            &bytes,
            &mut reader,
            MillimeterScale::IDENTITY,
            ArchiveVersion::V5,
            0,
        );
        assert!(matches!(
            result,
            Err(GeometryError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn cadir_rejects_future_c2_polycurve_major_for_typed_admission() {
        let bytes = [0x20];
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded");
        let result = read_polycurve_2d(&bytes, &mut reader, ArchiveVersion::V5, 0);
        assert!(matches!(
            result,
            Err(GeometryError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn top_level_polycurve_rejects_equal_adjacent_boundaries() {
        let arena = cadmpeg_core::decode::DecodeArena::default();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("context");
        let previous = FiniteReal::new(1.0);
        assert!(checked_polycurve_parameter(&ctx, previous, 1.0, 8, "polycurve").is_err());
    }

    #[test]
    fn c2_polycurve_rejects_equal_adjacent_boundaries() {
        let arena = cadmpeg_core::decode::DecodeArena::default();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("context");
        let previous = FiniteReal::new(1.0);
        assert!(checked_polycurve_parameter(&ctx, previous, 1.0, 8, "C2 polycurve").is_err());
    }

    #[test]
    fn analytic_full_circle_converts_to_exact_quadratic_nurbs() {
        let circle = unit_circle();
        let decoded = DecodedCurve::leaf(
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                    circle.center.get(),
                    circle.axis,
                    circle.xaxis,
                    circle.radius.get(),
                )
                .unwrap(),
            )),
            Diagnostics::new(),
        );
        let nurbs =
            with_test_context(|ctx| exact_nurbs(ctx, &decoded, 0)).expect("required invariant");
        assert_eq!(nurbs.degree(), 2);
        assert_eq!(nurbs.control_points().len(), 9);
        assert_eq!(nurbs.knots().len(), 12);
        assert_eq!(nurbs.knots()[0], 0.0);
        assert_eq!(*nurbs.knots().last().expect("nonempty knots"), TAU);
        assert_eq!(
            nurbs.weights().expect("rational circle")[1].get(),
            2.0_f64.sqrt() / 2.0
        );
    }

    #[test]
    fn recursive_compound_conversion_preserves_parent_domain_when_exact() {
        let line = |start: f64, end: f64| {
            DecodedCurve::leaf(
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                    NurbsCurve::from_lanes(
                        &cadmpeg_test_support::service_decode_context(),
                        1,
                        vec![start, start, end, end],
                        vec![Point3::new(start, 0.0, 0.0), Point3::new(end, 0.0, 0.0)],
                        None,
                        false,
                    )
                    .expect("fixture constructor admission")
                    .expect("valid test line"),
                )),
                Diagnostics::new(),
            )
        };
        let finite = |value: f64| FiniteReal::new(value).expect("finite parameter");
        let nested = DecodedCurve::Compound {
            children: vec![(finite(2.0), line(0.0, 1.0)), (finite(3.0), line(0.0, 1.0))],
            end_parameter: finite(5.0),
            warnings: Diagnostics::new(),
        };
        let converted =
            with_test_context(|ctx| {
                let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
                    "Rhino diagnostics", None,
                );
                exact_nurbs(ctx, &nested, 0)
            }).expect("required invariant");
        assert_eq!(converted.knots().as_slice(), vec![2.0, 2.0, 3.0, 5.0, 5.0]);
        assert_eq!(converted.control_points().len(), 3);
    }

    #[test]
    fn join_elevates_degree_and_midpoints_a_gap() {
        let line = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid test line");
        let quadratic = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            2,
            vec![5.0, 5.0, 5.0, 7.0, 7.0, 7.0],
            vec![
                Point3::new(3.0, 0.0, 0.0),
                Point3::new(4.0, 1.0, 0.0),
                Point3::new(5.0, 0.0, 0.0),
            ],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid test quadratic");
        let joined = with_test_context(|ctx| join_nurbs_segments(ctx, vec![line, quadratic], 0))
            .expect("join");
        assert_eq!(joined.curve.degree(), 2);
        assert_eq!(
            joined.curve.knots().as_slice(),
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 3.0, 3.0, 3.0]
        );
        assert_eq!(joined.curve.control_points().len(), 5);
        assert_eq!(joined.curve.control_points()[2], Point3::new(2.0, 0.0, 0.0));
        assert_eq!(joined.warnings.len(), 1);
        assert!(joined.warnings[0].contains("gap 2"));
    }
    #[test]
    fn audit_regression_degree_elevation_normalizes_large_common_weights() {
        let line = |weight| {
            NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![0., 0., 1., 1.],
                vec![Point3::new(1e200, 0., 0.), Point3::new(1e200, 1., 0.)],
                Some(vec![weight, weight]),
                false,
            )
            .expect("fixture constructor admission")
            .unwrap()
        };
        for degree in [1, 2] {
            let normalized =
                with_test_context(|ctx| super::elevate_to_degree(ctx, &line(1.), degree, 0))
                    .unwrap();
            let rescaled =
                with_test_context(|ctx| super::elevate_to_degree(ctx, &line(1e200), degree, 0))
                    .unwrap();
            for (a, b) in normalized
                .control_points()
                .iter()
                .zip(rescaled.control_points())
            {
                assert!((a.x / b.x - 1.).abs() <= 8. * f64::EPSILON);
                assert!((a.y - b.y).abs() <= 8. * f64::EPSILON);
            }
        }
    }
    #[test]
    fn numerical_0922b_wide_curve_domain_remap() {
        for domain in [[0., 1.], [-1e308, 1e308]] {
            let n = NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![domain[0], domain[0], domain[1], domain[1]],
                vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
                None,
                false,
            )
            .expect("fixture constructor admission")
            .unwrap();
            let r = with_test_context(|ctx| {
                super::remap_nurbs_domain(ctx, n, [FiniteReal::ZERO, FiniteReal::ONE], 0)
            });
            println!("Rhino remap{domain:?}: {r:?}");
            assert_eq!(r.unwrap().knots().as_slice(), &[0., 0., 1., 1.]);
        }
    }
}
