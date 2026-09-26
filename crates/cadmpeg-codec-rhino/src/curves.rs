// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino point and simple-curve payload decoding.

use crate::loss::Diagnostics;
use std::f64::consts::{FRAC_PI_2, TAU};
use std::ops::Range;

use cadmpeg_core::decode::{alloc_filled, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal, PositiveReal};

use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
use crate::objects::parse_class_wrapper;
use crate::settings::{bbox, interval, plane, MillimeterScale, Point3 as NativePoint3};
use crate::wire::{vector, Uuid};

const EPS_CURVE_POSITION: f64 = 1.0e-8;
const EPS_CURVE_DEGENERATE: f64 = 1.0e-10;

/// Maximum embedded curve nesting depth.
const MAX_CURVE_DEPTH: usize = 32;
/// Maximum points or polycurve segments in one payload.
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

    fn warnings_mut(&mut self) -> &mut Diagnostics {
        match self {
            Self::Leaf { warnings, .. } | Self::Compound { warnings, .. } => warnings,
        }
    }

    pub(crate) fn reported_geometry(&self) -> CurveGeometry {
        match self {
            Self::Leaf { geometry, .. } => geometry.clone(),
            Self::Compound { .. } => {
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None })
            }
        }
    }
}

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

pub(crate) fn allocation_failed(operation: &'static str, bytes: u64) -> GeometryError {
    GeometryError::Codec(CodecError::ResourceLimit(
        cadmpeg_core::decode::ResourceLimit {
            dimension: cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            reason: cadmpeg_core::decode::ResourceFailure::AllocationFailed,
            limit: u64::MAX,
            used: 0,
            additional: bytes,
            operation,
        },
    ))
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
        let offset = error.location.offset as usize;
        match error.kind {
            ParseErrorKind::UnexpectedEof { needed, .. } => {
                Self::Malformed(FramingError::Truncated {
                    offset,
                    needed: needed as usize,
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
        return Err(GeometryError::malformed(
            range.start,
            "curve recursion limit exceeded",
        ));
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
            let position = read_point(&mut reader, scale)?;
            DecodedGeometry::Point {
                position,
                scaled: scale != MillimeterScale::IDENTITY,
            }
        }
        POINT_CLOUD => DecodedGeometry::PointCloud(read_cloud(ctx, &mut reader, scale)?),
        LINE => DecodedGeometry::Curve {
            curve: DecodedCurve::leaf(
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(read_line(
                    &mut reader,
                    scale,
                    None,
                )?)),
                Diagnostics::new(),
            ),
        },
        ARC => {
            let (geometry, warnings) = read_arc(&mut reader, scale, None, false)?;
            DecodedGeometry::Curve {
                curve: DecodedCurve::leaf(geometry, warnings),
            }
        }
        POLYLINE => DecodedGeometry::Curve {
            curve: DecodedCurve::leaf(
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(read_polyline(
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
        return Err(GeometryError::malformed(
            reader.position(),
            "curve recursion limit exceeded",
        ));
    }
    let start = reader.position();
    let wrapper = crate::chunks::chunk_at(data, start, reader.end(), archive, false)?;
    let mut wrapper_warnings = Diagnostics::new();
    let class = parse_class_wrapper(
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
        return Err(GeometryError::malformed(
            reader.position(),
            "plane-space curve recursion limit exceeded",
        ));
    }
    let start = reader.position();
    let wrapper = crate::chunks::chunk_at(data, start, reader.end(), archive, false)?;
    let mut wrapper_warnings = Diagnostics::new();
    let class = parse_class_wrapper(
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
    scale_decoded_curve(&mut curve, scale, start)?;
    curve.warnings_mut().prepend(wrapper_warnings);
    Ok(curve)
}

fn scale_decoded_curve(
    curve: &mut DecodedCurve,
    scale: MillimeterScale,
    offset: usize,
) -> Result<(), GeometryError> {
    match curve {
        DecodedCurve::Compound { children, .. } => {
            for (_, child) in children {
                scale_decoded_curve(child, scale, offset)?;
            }
            return Ok(());
        }
        DecodedCurve::Leaf { geometry, .. } => match geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                let scaled = nurbs
                    .control_points()
                    .into_iter()
                    .map(|point| {
                        scale_ir_point(point.get(), scale).ok_or_else(|| {
                            GeometryError::malformed(offset, "scaled plane-space curve is invalid")
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let mut scaled = scaled.into_iter();
                nurbs
                    .edit_control_points(|point| {
                        if let Some(value) = scaled.next() {
                            *point = value;
                        }
                        Ok(())
                    })
                    .map_err(|error| GeometryError::malformed(offset, error.to_string()))?;
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                let center = circle_curve.center().get();
                let radius = circle_curve.radius().get();
                let center = scale_ir_point(center, scale)
                    .and_then(cadmpeg_ir::features::FinitePoint3::new)
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
                let origin = line_curve.origin().get();
                let direction = line_curve.direction();
                let origin = scale_ir_point(origin, scale)
                    .and_then(cadmpeg_ir::features::FinitePoint3::new)
                    .ok_or_else(|| {
                        GeometryError::malformed(
                            offset,
                            "scaled plane-space curve point is invalid",
                        )
                    })?;
                *line_curve = cadmpeg_ir::geometry::analytic::LineCurve::new(origin, direction);
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(degenerate_curve)) => {
                let point = degenerate_curve.point().get();
                *degenerate_curve = cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(
                    scale_ir_point(point, scale).ok_or_else(|| {
                        GeometryError::malformed(
                            offset,
                            "scaled plane-space curve point is invalid",
                        )
                    })?,
                )
                .map_err(|message| GeometryError::malformed(offset, message))?;
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

fn scale_ir_point(value: Point3, scale: MillimeterScale) -> Option<Point3> {
    let point = Point3::new(
        value.x * scale.value(),
        value.y * scale.value(),
        value.z * scale.value(),
    );
    (point.is_finite()).then_some(point)
}

/// Converts a decoded curve tree to one exact NURBS curve when possible.
pub(crate) fn exact_nurbs(
    curve: &DecodedCurve,
    offset: usize,
) -> Result<NurbsCurve, GeometryError> {
    match curve {
        DecodedCurve::Leaf { geometry, .. } => match geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => Ok(nurbs.clone()),
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                let center = circle_curve.center().get();
                let axis = circle_curve.frame().axis().as_raw();
                let ref_direction = circle_curve.frame().reference().as_raw();
                let radius = circle_curve.radius().get();
                let yaxis = axis.cross(*ref_direction);
                let circle = Circle {
                    center,
                    axis: *axis,
                    xaxis: *ref_direction,
                    yaxis,
                    radius,
                };
                arc_nurbs(&circle, [0.0, TAU], [0.0, TAU], TAU, offset)
            }
            _ => Err(error(offset, "curve has no exact NURBS representation")),
        },
        DecodedCurve::Compound {
            children,
            end_parameter,
            ..
        } => {
            let mut segments = Vec::with_capacity(children.len());
            for (index, (start, child)) in children.iter().enumerate() {
                let end = children
                    .get(index + 1)
                    .map_or(*end_parameter, |(next, _)| *next);
                let target = [start.get(), end.get()];
                if target[0] >= target[1] {
                    return Err(error(offset, "polycurve segment domain is invalid"));
                }
                segments.push(remap_nurbs_domain(
                    exact_nurbs(child, offset)?,
                    target,
                    offset,
                )?);
            }
            Ok(join_nurbs_segments(segments, offset)?.curve)
        }
    }
}

pub(crate) fn remap_nurbs_domain(
    mut curve: NurbsCurve,
    target: [f64; 2],
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
    if !source[0].is_finite() || !source[1].is_finite() || source[0] >= source[1] {
        return Err(error(offset, "curve domain is invalid"));
    }
    if !target[0].is_finite() || !target[1].is_finite() || target[0] >= target[1] {
        return Err(error(offset, "curve target domain is invalid"));
    }
    let remapped = curve
        .knots()
        .iter()
        .copied()
        .map(|knot| {
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
            value
                .is_finite()
                .then_some(value)
                .ok_or_else(|| error(offset, "curve knot remap overflowed"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    curve
        .edit_knots(|knots| knots.copy_from_slice(&remapped))
        .map_err(|error| GeometryError::malformed(offset, error.to_string()))?;
    Ok(curve)
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

fn elevate_bezier(mut values: Vec<Homogeneous>, target: usize) -> Vec<Homogeneous> {
    while values.len() - 1 < target {
        let degree = values.len() - 1;
        let mut elevated = Vec::with_capacity(values.len() + 1);
        elevated.push(values[0]);
        for index in 1..=degree {
            elevated
                .push(values[index].blend(values[index - 1], index as f64 / (degree + 1) as f64));
        }
        elevated.push(values[degree]);
        values = elevated;
    }
    values
}

fn insert_knot_once(
    knots: &mut Vec<f64>,
    points: &mut Vec<Homogeneous>,
    degree: usize,
    value: f64,
) -> Result<(), ()> {
    let n = points.len() - 1;
    // Endpoint clamping can select a span beyond the last control point.
    let k = knots.iter().rposition(|knot| *knot <= value).ok_or(())?;
    let k = if degree == 0 { k.min(n) } else { k };
    let multiplicity = knots.iter().filter(|knot| **knot == value).count();
    if multiplicity > degree
        || k < degree
        || k - degree > n
        || k.checked_sub(multiplicity).is_none_or(|tail| tail > n)
    {
        return Err(());
    }
    let mut output = alloc_filled(
        points.len().checked_add(1).ok_or(())?,
        points[0],
        "Rhino polycurve knot insertion points",
    )
    .map_err(|_| ())?;
    output[..=k - degree].copy_from_slice(&points[..=k - degree]);
    output[k - multiplicity + 1..=n + 1].copy_from_slice(&points[k - multiplicity..=n]);
    for index in k - degree + 1..=k - multiplicity {
        let denominator = knots[index + degree] - knots[index];
        if denominator <= 0.0 || !denominator.is_finite() {
            return Err(());
        }
        let alpha = (value - knots[index]) / denominator;
        output[index] = points[index - 1].blend(points[index], alpha);
    }
    knots.insert(k + 1, value);
    *points = output;
    Ok(())
}

fn elevate_to_degree(
    curve: &NurbsCurve,
    target: usize,
    offset: usize,
) -> Result<NurbsCurve, GeometryError> {
    let degree =
        usize::try_from(curve.degree()).map_err(|_| error(offset, "curve degree overflow"))?;
    if degree > target || curve.periodic() {
        return Err(error(offset, "polycurve segment knot vector is invalid"));
    }
    let mut weights = match curve.pole_rows().weights() {
        Some(weights) => weights,
        None => alloc_filled(
            curve.control_points().len(),
            1.0,
            "Rhino polycurve segment weights",
        )
        .map_err(|error| {
            GeometryError::malformed(
                offset,
                format!("polycurve weight allocation refused: {error}"),
            )
        })?,
    };
    let rational = weights.iter().any(|weight| *weight != 1.0);
    let control_points = curve.control_points();
    if control_points.iter().zip(&weights).any(|(point, weight)| {
        [point.x, point.y, point.z].into_iter().any(|coordinate| {
            let product = coordinate * weight;
            !product.is_finite() || (product == 0.0 && coordinate != 0.0)
        })
    }) {
        let maximum_weight = weights.iter().copied().map(f64::abs).fold(0.0, f64::max);
        let exponent = cadmpeg_ir::math::power_of_two_bound(maximum_weight)
            .ok_or_else(|| error(offset, "polycurve weight scale is invalid"))?;
        for weight in &mut weights {
            *weight = cadmpeg_ir::math::scale_power_of_two(*weight, -exponent)
                .filter(|weight| weight.get() != 0.0)
                .ok_or_else(|| error(offset, "polycurve weight normalization lost its range"))?
                .get();
        }
    }

    let mut points = curve
        .control_points()
        .iter()
        .copied()
        .zip(weights)
        .map(|(point, weight)| {
            Homogeneous([point.x * weight, point.y * weight, point.z * weight, weight])
        })
        .collect::<Vec<_>>();
    let mut knots = curve.knots().to_vec();
    let domain = [knots[degree], knots[knots.len() - degree - 1]];
    for endpoint in domain {
        while knots.iter().filter(|value| **value == endpoint).count() < degree + 1 {
            insert_knot_once(&mut knots, &mut points, degree, endpoint)
                .map_err(|()| error(offset, "polycurve endpoint clamping failed"))?;
        }
    }
    let mut internal = knots
        .iter()
        .copied()
        .filter(|knot| *knot > domain[0] && *knot < domain[1])
        .collect::<Vec<_>>();
    internal.dedup();
    for knot in internal {
        while knots.iter().filter(|value| **value == knot).count() < degree {
            insert_knot_once(&mut knots, &mut points, degree, knot)
                .map_err(|()| error(offset, "polycurve knot insertion failed"))?;
        }
    }
    let spans = (degree..points.len())
        .filter(|&span| {
            knots[span] < knots[span + 1]
                && knots[span] >= domain[0]
                && knots[span + 1] <= domain[1]
        })
        .collect::<Vec<_>>();
    if spans.is_empty() {
        return Err(error(offset, "polycurve segment has no nonempty span"));
    }
    let mut elevated = Vec::new();
    let mut elevated_knots = alloc_filled(
        target
            .checked_add(1)
            .ok_or_else(|| error(offset, "polycurve elevated knot count overflow"))?,
        domain[0],
        "Rhino polycurve elevated knots",
    )
    .map_err(|cause| GeometryError::malformed(offset, cause.to_string()))?;
    for (index, span) in spans.into_iter().enumerate() {
        let bezier = elevate_bezier(points[span - degree..=span].to_vec(), target);
        let disconnected = knots.iter().filter(|knot| **knot == knots[span]).count() > degree;
        let skip = usize::from(index > 0 && !disconnected);
        if index > 0 {
            elevated_knots.extend(std::iter::repeat_n(
                knots[span],
                target + usize::from(disconnected),
            ));
        }
        elevated.extend(bezier.into_iter().skip(skip));
    }
    elevated_knots.extend(std::iter::repeat_n(domain[1], target + 1));
    let mut output_weights = Vec::with_capacity(elevated.len());
    let mut control_points = Vec::with_capacity(elevated.len());
    for point in elevated {
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
        output_weights.push(weight);
    }
    NurbsCurve::from_checked_lanes(
        target as u32,
        elevated_knots,
        control_points,
        rational.then_some(output_weights),
        false,
    )
    .map_err(|error| GeometryError::malformed(offset, error.to_string()))
}

pub(crate) fn join_nurbs_segments(
    mut segments: Vec<NurbsCurve>,
    offset: usize,
) -> Result<NurbsJoin, GeometryError> {
    let Some(_) = segments.first() else {
        return Err(error(offset, "polycurve has no segments"));
    };
    let degree = segments
        .iter()
        .map(NurbsCurve::degree)
        .max()
        .ok_or_else(|| error(offset, "polycurve has no segments"))?;
    let target = usize::try_from(degree).map_err(|_| error(offset, "curve degree overflow"))?;
    if target == 0 {
        return Err(error(offset, "polycurve segment degree must be positive"));
    }
    segments = segments
        .iter()
        .map(|segment| elevate_to_degree(segment, target, offset))
        .collect::<Result<_, _>>()?;
    if segments.len() == 1 {
        return Ok(NurbsJoin {
            curve: segments.remove(0),
            warnings: Diagnostics::new(),
        });
    }
    let multiplicity = usize::try_from(degree)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| error(offset, "curve degree overflow"))?;
    for segment in &segments {
        let start = segment.knots().get(multiplicity - 1).copied();
        let end = segment
            .knots()
            .len()
            .checked_sub(multiplicity)
            .and_then(|index| segment.knots().get(index))
            .copied();
        if start.is_none()
            || end.is_none()
            || segment.knots()[..multiplicity]
                .iter()
                .any(|value| Some(*value) != start)
            || segment.knots()[segment.knots().len() - multiplicity..]
                .iter()
                .any(|value| Some(*value) != end)
        {
            return Err(error(offset, "polycurve segment is not endpoint-clamped"));
        }
    }
    let rational = segments.iter().any(|segment| segment.weights().is_some());
    let control_count = segments
        .iter()
        .try_fold(0_usize, |total, segment| {
            total.checked_add(segment.control_points().len())
        })
        .ok_or_else(|| error(offset, "polycurve size overflow"))?;
    let knot_count = segments
        .iter()
        .try_fold(0_usize, |total, segment| {
            total.checked_add(segment.knots().len())
        })
        .and_then(|total| {
            (segments.len() - 1)
                .checked_mul(multiplicity + 1)
                .and_then(|duplicates| total.checked_sub(duplicates))
        })
        .ok_or_else(|| error(offset, "polycurve size overflow"))?;
    let mut control_points: Vec<Point3> = Vec::with_capacity(control_count);
    let mut knots: Vec<f64> = Vec::with_capacity(knot_count);
    let mut weights = rational.then(|| Vec::with_capacity(control_count));
    let mut warnings = Diagnostics::new();
    for (index, mut segment) in segments.into_iter().enumerate() {
        if index > 0 {
            let Some(previous) = control_points.last().copied() else {
                return Err(error(
                    offset,
                    "polycurve join has no previous segment endpoint",
                ));
            };
            let next = segment.control_points()[0];
            let midpoint = Point3::new(
                previous.x.midpoint(next.x),
                previous.y.midpoint(next.y),
                previous.z.midpoint(next.z),
            );
            let gap = previous.distance(next.get());
            if gap > 0.0 {
                warnings.push_coded(
                    crate::loss::RhinoLossCode::PolycurveJoinGap,
                    format!("polycurve join moved endpoints by half of gap {gap}"),
                );
            }
            let Some(previous) = control_points.last_mut() else {
                return Err(error(offset, "polycurve join has no previous endpoint"));
            };
            *previous = midpoint;
            let mut first = true;
            segment
                .edit_control_points(|point| {
                    if first {
                        *point = midpoint;
                        first = false;
                    }
                    Ok(())
                })
                .map_err(|error| GeometryError::malformed(offset, error.to_string()))?;
        }
        // Unequal endpoint weights are different homogeneous poles. Keep both
        // with a full-multiplicity knot so neither segment's rational shape changes.
        let unit = NonZeroReal::from(PositiveReal::ONE);
        let previous_weight = weights
            .as_ref()
            .and_then(|weights| weights.last())
            .copied()
            .unwrap_or(unit);
        let segment_weights = segment.weights();
        let next_weight = segment_weights
            .as_ref()
            .and_then(|weights| weights.first().copied())
            .unwrap_or(unit);
        let skip = usize::from(index > 0 && previous_weight.get() == next_weight.get());
        let segment_points = segment.pole_rows().raw_points();
        if let Some(target) = &mut weights {
            match segment_weights {
                Some(values) => target.extend(values.into_iter().skip(skip)),
                None => target.extend(std::iter::repeat_n(unit, segment_points.len() - skip)),
            }
        }
        control_points.extend(segment_points.iter().copied().skip(skip));
        let segment_start = segment.knots()[multiplicity - 1];
        let dk = if index == 0 {
            0.0
        } else {
            knots.last().copied().unwrap_or(0.0) - segment_start
        };
        if skip > 0 {
            knots.pop();
        }
        knots.extend(
            segment
                .knots()
                .iter()
                .copied()
                .map(|knot| knot + dk)
                .skip(if index == 0 { 0 } else { multiplicity }),
        );
    }
    Ok(NurbsJoin {
        curve: NurbsCurve::from_checked_lanes(degree, knots, control_points, weights, false)
            .map_err(|error| GeometryError::malformed(offset, error.to_string()))?,
        warnings,
    })
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
        return Err(GeometryError::malformed(
            range.start,
            "C2 curve recursion limit exceeded",
        ));
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
                    &mut reader,
                    MillimeterScale::IDENTITY,
                    Some(2),
                )?)),
                Diagnostics::new(),
            ),
        },
        ARC => {
            let (geometry, warnings) =
                read_arc(&mut reader, MillimeterScale::IDENTITY, Some(2), true)?;
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

fn read_polycurve_2d(
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
    let (parameters, end_parameter) =
        read_polycurve_parameters(reader, segment_count, "C2 polycurve")?;
    let mut children = Vec::with_capacity(segment_count);
    for parameter in parameters {
        let start = reader.position();
        let wrapper = crate::chunks::chunk_at(data, start, reader.end(), archive, false)?;
        let mut wrapper_warnings = Diagnostics::new();
        let class = parse_class_wrapper(
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

/// Consumes one legacy Brep C2 polycurve payload and returns its byte range.
pub(crate) fn consume_legacy_polycurve_2d(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Range<usize>, GeometryError> {
    let start = reader.position();
    // discarded-value: the payload is consumed for the byte range the caller returns; the decoded curve has no reader
    let _ = read_polycurve_2d(ctx, data, reader, archive, 0)?;
    Ok(start..reader.position())
}

fn read_point(
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
) -> Result<FinitePoint3, GeometryError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let point = native_point(reader)?;
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
    let point_count_u64 = u64::try_from(point_count)
        .map_err(|_| GeometryError::not_implemented("point-cloud count exceeds address space"))?;
    ctx.charge_collection_items(point_count_u64, "Rhino point-cloud points")?;
    let point_bytes = point_count_u64
        .checked_mul(
            u64::try_from(std::mem::size_of::<FinitePoint3>()).map_err(|_| {
                GeometryError::not_implemented("point-cloud storage exceeds address space")
            })?,
        )
        .ok_or_else(|| {
            GeometryError::not_implemented("point-cloud storage exceeds address space")
        })?;
    ctx.charge_retained(point_bytes, "Rhino point-cloud points")?;
    let mut points = Vec::new();
    points
        .try_reserve_exact(point_count)
        .map_err(|_| allocation_failed("Rhino point-cloud points", point_bytes))?;
    for _ in 0..point_count {
        let point = native_point(reader)?;
        points.push(
            crate::wire::scaled_point(point.0.get(), scale)
                .ok_or_else(|| error(reader.position(), "scaled point coordinate is invalid"))?,
        );
    }
    plane(reader)?;
    bbox(reader)?;
    reader.i32()?;
    let mut warnings = Diagnostics::new();
    if minor >= 1 {
        let normal_count = crate::wire::element_count(reader, 24)?;
        if normal_count != 0 && normal_count != point_count {
            warnings.push_coded(
                crate::loss::RhinoLossCode::RedundantFieldRepaired,
                "redundant point-cloud normal count mismatch; channel dropped",
            );
        }
        for _ in 0..normal_count {
            crate::settings::vector(reader)?;
        }
        let color_count = crate::wire::element_count(reader, 4)?;
        for _ in 0..color_count {
            reader.take(4)?;
        }
        if color_count != 0 && color_count != point_count {
            warnings.push_coded(
                crate::loss::RhinoLossCode::RedundantFieldRepaired,
                "redundant point-cloud color count mismatch; channel dropped",
            );
        }
    }
    if minor >= 2 {
        let value_count = crate::wire::element_count(reader, 8)?;
        for _ in 0..value_count {
            let value_offset = reader.position();
            let value = reader.f64()?;
            if !value.is_finite() {
                return Err(error(value_offset, "point-cloud value is not finite"));
            }
        }
        if value_count != 0 && value_count != point_count {
            warnings.push_coded(
                crate::loss::RhinoLossCode::RedundantFieldRepaired,
                "redundant point-cloud scalar count mismatch; channel dropped",
            );
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
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    expected_dimension: Option<i32>,
) -> Result<NurbsCurve, GeometryError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let from = crate::wire::scaled_point(native_point(reader)?.0.get(), scale)
        .ok_or_else(|| error(reader.position(), "scaled line coordinate is invalid"))?
        .get();
    let to = crate::wire::scaled_point(native_point(reader)?.0.get(), scale)
        .ok_or_else(|| error(reader.position(), "scaled line coordinate is invalid"))?
        .get();
    let domain = interval(reader)?.0.get();
    let dimension = reader.i32()?;
    if expected_dimension.is_some_and(|expected| dimension != expected)
        || !(dimension == 2 || dimension == 3)
        || from == to
        || domain[0] >= domain[1]
    {
        return Err(error(reader.position(), "invalid bounded line"));
    }
    NurbsCurve::from_lanes(
        1,
        vec![domain[0], domain[0], domain[1], domain[1]],
        vec![from, to],
        None,
        false,
    )
    .map_err(|error| GeometryError::malformed(reader.position(), error.to_string()))
}

fn read_polyline(
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
    let mut points = Vec::with_capacity(point_count);
    for _ in 0..point_count {
        let point = native_point(reader)?;
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
    let mut parameters = Vec::with_capacity(parameter_count);
    for _ in 0..parameter_count {
        let value = reader.f64()?;
        if !value.is_finite() || parameters.last().is_some_and(|previous| value <= *previous) {
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
    let mut knots = Vec::with_capacity(point_count + 2);
    knots.push(parameters[0]);
    knots.push(parameters[0]);
    knots.extend_from_slice(&parameters[1..point_count - 1]);
    knots.push(parameters[point_count - 1]);
    knots.push(parameters[point_count - 1]);
    NurbsCurve::from_lanes(1, knots, points, None, false)
        .map_err(|error| GeometryError::malformed(reader.position(), error.to_string()))
}

fn read_arc(
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    expected_dimension: Option<i32>,
    force_nurbs: bool,
) -> Result<(CurveGeometry, Diagnostics), GeometryError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let circle = read_circle(reader, scale)?;
    let angle = interval(reader)?.0.get();
    let domain = interval(reader)?.0.get();
    let dimension = reader.i32()?;
    let mut warnings = Diagnostics::new();
    if expected_dimension.is_some_and(|expected| dimension != expected) {
        return Err(error(reader.position(), "arc dimension is invalid"));
    }
    if dimension != 2 && dimension != 3 {
        warnings.push(format!("arc dimension {dimension} normalized to native 3D"));
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
                cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                    circle.center,
                    circle.axis,
                    circle.xaxis,
                    circle.radius,
                )
                .map_err(|message| error(reader.position(), message))?,
            )),
            warnings,
        ));
    }
    Ok((
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(arc_nurbs(
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
    center: Point3,
    axis: Vector3,
    xaxis: Vector3,
    yaxis: Vector3,
    radius: f64,
}

fn read_circle(
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
) -> Result<Circle, GeometryError> {
    let native = plane(reader)?;
    let radius = reader.f64()?;
    let zero = native_point(reader)?;
    let half_pi = native_point(reader)?;
    let at_pi = native_point(reader)?;
    let scaled_radius = radius * scale.value();
    if !radius.is_finite() || radius <= 0.0 || !scaled_radius.is_finite() || scaled_radius <= 0.0 {
        return Err(error(reader.position(), "circle radius is invalid"));
    }
    let xaxis = vector(native.xaxis);
    let yaxis = vector(native.yaxis);
    let axis = vector(native.zaxis);
    let center = crate::wire::scaled_point(native.origin, scale)
        .ok_or_else(|| error(reader.position(), "scaled circle center is invalid"))?
        .get();
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
        && close_native_point(zero.0.get(), native.origin, native.xaxis, radius)
        && close_native_point(half_pi.0.get(), native.origin, native.yaxis, radius)
        && close_native_point(at_pi.0.get(), native.origin, negate(native.xaxis), radius))
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

fn read_polycurve(
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
    let (parameters, end_parameter) =
        read_polycurve_parameters(reader, segment_count, "polycurve")?;
    let mut children = Vec::with_capacity(segment_count);
    for parameter in parameters {
        let start = reader.position();
        let wrapper = crate::chunks::chunk_at(data, start, reader.end(), archive, false)?;
        let mut wrapper_warnings = Diagnostics::new();
        let class = parse_class_wrapper(
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

/// Consumes one legacy Brep C3 polycurve payload and returns its byte range.
pub(crate) fn consume_legacy_polycurve(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
) -> Result<Range<usize>, GeometryError> {
    let start = reader.position();
    // discarded-value: the payload is consumed for the byte range the caller returns; the decoded curve has no reader
    let _ = read_polycurve(ctx, data, reader, scale, archive, 0)?;
    Ok(start..reader.position())
}

fn read_polycurve_parameters(
    reader: &mut BoundedReader<'_>,
    segment_count: usize,
    label: &str,
) -> Result<(Vec<FiniteReal>, FiniteReal), GeometryError> {
    let parameter_count = crate::wire::element_count(reader, 8)?;
    if parameter_count != segment_count + 1 {
        return Err(GeometryError::malformed(
            reader.position(),
            format!("{label} parameter count mismatch"),
        ));
    }
    let mut parameters = Vec::with_capacity(segment_count);
    for _ in 0..segment_count {
        let value = reader.f64()?;
        parameters.push(checked_polycurve_parameter(
            parameters.last().copied(),
            value,
            reader.position(),
            label,
        )?);
    }
    let value = reader.f64()?;
    let end_parameter =
        checked_polycurve_parameter(parameters.last().copied(), value, reader.position(), label)?;
    Ok((parameters, end_parameter))
}

fn checked_polycurve_parameter(
    previous: Option<FiniteReal>,
    value: f64,
    offset: usize,
    label: &str,
) -> Result<FiniteReal, GeometryError> {
    FiniteReal::new(value)
        .filter(|value| previous.is_none_or(|previous| value.get() > previous.get()))
        .ok_or_else(|| GeometryError::malformed(offset, format!("{label} parameters are invalid")))
}

fn arc_nurbs(
    circle: &Circle,
    angle: [f64; 2],
    domain: [f64; 2],
    delta: f64,
    offset: usize,
) -> Result<NurbsCurve, GeometryError> {
    let spans = (delta / FRAC_PI_2).ceil().max(1.0) as usize;
    let step = delta / spans as f64;
    let domain_at = |index: usize| -> Result<f64, GeometryError> {
        let fraction = index as f64 / spans as f64;
        let ordinary = domain[0] + (domain[1] - domain[0]) * index as f64 / spans as f64;
        if ordinary.is_finite() {
            Ok(ordinary)
        } else {
            cadmpeg_ir::math::interpolate(domain[0], domain[1], fraction)
                .map(cadmpeg_ir::scalar::FiniteReal::get)
                .ok_or_else(|| error(offset, "arc parameter domain is invalid"))
        }
    };
    let mut control_points = Vec::with_capacity(spans * 2 + 1);
    let mut weights = Vec::with_capacity(spans * 2 + 1);
    let mut knots = Vec::with_capacity(spans * 2 + 4);
    for span in 0..spans {
        let a0 = angle[0] + step * span as f64;
        let a1 = angle[0] + step * (span + 1) as f64;
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
    NurbsCurve::from_lanes(2, knots, control_points, Some(weights), false)
        .map_err(|error| GeometryError::malformed(offset, error.to_string()))
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
    Point3::new(
        circle.center.x + radial.x * circle.radius * radial_scale,
        circle.center.y + radial.y * circle.radius * radial_scale,
        circle.center.z + radial.z * circle.radius * radial_scale,
    )
}

fn native_point(reader: &mut BoundedReader<'_>) -> Result<NativePoint3, FramingError> {
    crate::settings::point(reader)
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
    fn numerical_audit_nurbs_elevation_preserves_active_spans_and_discontinuities() {
        use cadmpeg_ir::eval::curve_point_solved;
        use cadmpeg_ir::geometry::SolvedCurveGeometry;
        let cases = [
            NurbsCurve::from_lanes(
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
            .unwrap(),
            NurbsCurve::from_lanes(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0],
                (0..6)
                    .map(|x| Point3::new(f64::from(x), 0.0, 0.0))
                    .collect(),
                None,
                false,
            )
            .unwrap(),
        ];
        for curve in cases {
            for degree in [2, 3] {
                let elevated = super::elevate_to_degree(&curve, degree, 0).unwrap();
                let start = curve.knots()[curve.degree() as usize];
                let end = curve.knots()[curve.control_points().len()];
                assert_eq!(elevated.knots()[degree], start);
                assert_eq!(elevated.knots()[elevated.control_points().len()], end);
                for fraction in [0.0, 0.125, 0.25, 0.5, 0.625, 0.875, 1.0] {
                    let at = start + (end - start) * fraction;
                    let expected =
                        curve_point_solved(&SolvedCurveGeometry::Nurbs(curve.clone()), at).unwrap();
                    let actual =
                        curve_point_solved(&SolvedCurveGeometry::Nurbs(elevated.clone()), at)
                            .unwrap();
                    assert!(actual.distance(expected.get()) <= 64.0 * f64::EPSILON);
                }
            }
        }
    }

    #[test]
    fn numerical_audit_join_preserves_independently_scaled_rational_segments() {
        use cadmpeg_ir::eval::curve_point_solved;
        use cadmpeg_ir::geometry::SolvedCurveGeometry;
        let first = NurbsCurve::from_lanes(
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
        .unwrap();
        let second = NurbsCurve::from_lanes(
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
        .unwrap();
        let joined = super::join_nurbs_segments(vec![first, second], 0).unwrap();
        let actual = curve_point_solved(&SolvedCurveGeometry::Nurbs(joined.curve), 1.5).unwrap();
        assert_eq!(actual, Point3::new(0.25, 0.75, 0.0));
    }

    #[test]
    fn numerical_audit_remap_and_join_keep_large_finite_values() {
        let curve = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0e-200, 1.0e-200],
            vec![Point3::new(f64::MAX, 0.0, 0.0); 2],
            None,
            false,
        )
        .unwrap();
        let remapped = super::remap_nurbs_domain(curve, [0.0, 1.0e200], 0).unwrap();
        assert_eq!(remapped.knots().as_slice(), &[0.0, 0.0, 1.0e200, 1.0e200]);
        let joined = super::join_nurbs_segments(vec![remapped.clone(), remapped], 0).unwrap();
        assert!(joined
            .curve
            .control_points()
            .iter()
            .all(|point| point.x == f64::MAX));
    }

    use super::{
        arc_nurbs, canonical_circle, checked_polycurve_parameter, circle_point, exact_nurbs,
        join_nurbs_segments, read_line, read_polyline, scale_decoded_curve, Circle, DecodedCurve,
        GeometryError, CURVE_ON_SURFACE, MAX_CURVE_DEPTH,
    };
    use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
    use crate::loss::Diagnostics;
    use crate::settings::MillimeterScale;
    use cadmpeg_ir::geometry::{nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::scalar::FiniteReal;

    const EPS_EXACT_ARC: f64 = 1.0e-12;

    fn with_test_context<R>(f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> R) -> R {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("test context input fits service profile");
        f(&ctx)
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
    fn stored_count_above_legacy_limit_is_bounded_by_payload() {
        let item_count = 65_537_usize;
        let mut bytes = (item_count as i32).to_le_bytes().to_vec();
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
        assert!(error.to_string().contains("curve recursion limit exceeded"));
    }
    use std::f64::consts::{PI, TAU};

    fn unit_circle() -> Circle {
        Circle {
            center: Point3::new(2.0, -1.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            xaxis: Vector3::new(1.0, 0.0, 0.0),
            yaxis: Vector3::new(0.0, 1.0, 0.0),
            radius: 4.0,
        }
    }

    #[test]
    fn arc_nurbs_preserves_endpoints_midpoint_and_weights() {
        let circle = unit_circle();
        let arc = arc_nurbs(&circle, [0.0, PI], [10.0, 20.0], PI, 0).expect("valid arc");
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

    #[test]
    fn arc_nurbs_maps_a_wide_finite_parameter_domain() {
        let arc = arc_nurbs(&unit_circle(), [0.0, PI], [-f64::MAX, f64::MAX], PI, 0)
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
        let arc = arc_nurbs(&circle, [0.0, 3.0 * PI], [0.0, 3.0], 3.0 * PI, 0).expect("valid arc");
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
            let curve =
                read_line(&mut reader, MillimeterScale::IDENTITY, None).expect("valid line");
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
            let curve = read_polyline(&mut reader, MillimeterScale::IDENTITY, None)
                .expect("valid polyline");
            assert_eq!(curve.control_points().len(), 2);
            assert_eq!(curve.knots().as_slice(), vec![10.0, 10.0, 12.0, 12.0]);
        }
    }

    #[test]
    fn plane_space_nurbs_scaling_rejects_coordinate_overflow() {
        let curve = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(2.0, 0.0, 0.0), Point3::new(3.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("valid test curve");
        let mut decoded = DecodedCurve::leaf(
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            Diagnostics::new(),
        );
        let error = scale_decoded_curve(
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
        let previous = FiniteReal::new(1.0);
        assert!(checked_polycurve_parameter(previous, 1.0, 8, "polycurve").is_err());
    }

    #[test]
    fn c2_polycurve_rejects_equal_adjacent_boundaries() {
        let previous = FiniteReal::new(1.0);
        assert!(checked_polycurve_parameter(previous, 1.0, 8, "C2 polycurve").is_err());
    }

    #[test]
    fn analytic_full_circle_converts_to_exact_quadratic_nurbs() {
        let circle = unit_circle();
        let decoded = DecodedCurve::leaf(
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                    circle.center,
                    circle.axis,
                    circle.xaxis,
                    circle.radius,
                )
                .unwrap(),
            )),
            Diagnostics::new(),
        );
        let nurbs = exact_nurbs(&decoded, 0).expect("required invariant");
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
                        1,
                        vec![start, start, end, end],
                        vec![Point3::new(start, 0.0, 0.0), Point3::new(end, 0.0, 0.0)],
                        None,
                        false,
                    )
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
        let converted = exact_nurbs(&nested, 0).expect("required invariant");
        assert_eq!(converted.knots().as_slice(), vec![2.0, 2.0, 3.0, 5.0, 5.0]);
        assert_eq!(converted.control_points().len(), 3);
    }

    #[test]
    fn join_elevates_degree_and_midpoints_a_gap() {
        let line = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("valid test line");
        let quadratic = NurbsCurve::from_lanes(
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
        .expect("valid test quadratic");
        let joined = join_nurbs_segments(vec![line, quadratic], 0).expect("join");
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
                1,
                vec![0., 0., 1., 1.],
                vec![Point3::new(1e200, 0., 0.), Point3::new(1e200, 1., 0.)],
                Some(vec![weight, weight]),
                false,
            )
            .unwrap()
        };
        for degree in [1, 2] {
            let normalized = super::elevate_to_degree(&line(1.), degree, 0).unwrap();
            let rescaled = super::elevate_to_degree(&line(1e200), degree, 0).unwrap();
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
                1,
                vec![domain[0], domain[0], domain[1], domain[1]],
                vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
                None,
                false,
            )
            .unwrap();
            let r = super::remap_nurbs_domain(n, [0., 1.], 0);
            println!("Rhino remap{domain:?}: {r:?}");
            assert_eq!(r.unwrap().knots().as_slice(), &[0., 0., 1., 1.]);
        }
    }
}
