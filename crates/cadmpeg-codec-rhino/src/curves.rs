// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino point and simple-curve payload decoding.

use std::f64::consts::{FRAC_PI_2, TAU};
use std::ops::Range;

use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve};
use cadmpeg_ir::math::{Point3, Vector3};

use crate::chunks::{checked_count_bytes, ArchiveVersion, BoundedReader, FramingError};
use crate::objects::parse_class_wrapper;
use crate::settings::{bbox, interval, plane, Point3 as NativePoint3};
use crate::wire::Uuid;

/// Maximum embedded curve nesting depth.
pub(crate) const MAX_CURVE_DEPTH: usize = 32;
/// Maximum points or polycurve segments in one payload.
pub(crate) const MAX_CURVE_ITEMS: usize = 1 << 16;

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
const POLYCURVE: Uuid = Uuid::from_canonical([
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
        position: Point3,
        /// Whether a unit conversion was applied.
        scaled: bool,
    },
    /// One point cloud and its optional native channels.
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

/// Point-cloud channels retained by the native namespace boundary.
#[derive(Debug, Clone)]
pub(crate) struct PointCloud {
    /// Ordered points.
    pub(crate) points: Vec<Point3>,
    /// Whether a unit conversion was applied.
    pub(crate) scaled: bool,
}

/// A validated polycurve construction.
#[derive(Debug, Clone)]
pub(crate) struct Compound {
    /// Child curve trees in source order.
    pub(crate) children: Vec<DecodedCurve>,
    /// Child segment parameters.
    pub(crate) parameters: Vec<f64>,
}

/// A curve carrier and its optional recursive construction.
#[derive(Debug, Clone)]
pub(crate) struct DecodedCurve {
    /// Solved carrier geometry.
    pub(crate) geometry: CurveGeometry,
    /// Compound construction, when this is a polycurve.
    pub(crate) compound: Option<Compound>,
    /// Non-fatal source warnings.
    pub(crate) warnings: Vec<String>,
}

/// A semantic geometry error.
#[derive(Debug)]
pub(crate) enum GeometryError {
    /// A bounded payload uses a future or unsupported version.
    UnsupportedVersion { offset: usize, message: String },
    /// A bounded payload is malformed.
    Malformed(FramingError),
}

impl std::fmt::Display for GeometryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion { offset, message } => {
                write!(formatter, "unsupported version at {offset}: {message}")
            }
            Self::Malformed(error) => error.fmt(formatter),
        }
    }
}

impl From<FramingError> for GeometryError {
    fn from(error: FramingError) -> Self {
        Self::Malformed(error)
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

/// Returns whether class data contains independently checksummed child chunks.
pub(crate) fn class_data_nests_chunks(uuid: Uuid) -> bool {
    matches!(uuid, CURVE_ON_SURFACE | POLYCURVE | POLYCURVE_LEGACY)
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
    use super::*;

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
}

/// Decode one top-level class-data payload.
pub(crate) fn decode(
    data: &[u8],
    class_uuid: Uuid,
    range: Range<usize>,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<DecodedGeometry, GeometryError> {
    decode_inner(data, class_uuid, range, scale, archive, 0)
}

/// Decodes a Brep C2 curve in surface parameter space.
pub(crate) fn decode_2d(
    data: &[u8],
    class_uuid: Uuid,
    range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<DecodedGeometry, GeometryError> {
    decode_inner_2d(data, class_uuid, range, archive, 0)
}

pub(crate) fn decode_inner(
    data: &[u8],
    class_uuid: Uuid,
    range: Range<usize>,
    scale: f64,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedGeometry, GeometryError> {
    if depth > MAX_CURVE_DEPTH {
        return Err(malformed(range.start, "curve recursion limit exceeded"));
    }
    if class_uuid == CURVE_ON_SURFACE {
        let construction = crate::curve_on_surface::decode(data, range, scale, archive, depth + 1)?;
        let Some(mut curve) = construction.model_curve else {
            return Err(unsupported(
                construction.source_range.start,
                "curve-on-surface has no stored model-space carrier",
            ));
        };
        curve.warnings.splice(0..0, construction.warnings);
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
            surface: crate::surfaces::decode(data, class_uuid, range, scale, archive, depth)?,
        });
    }
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let result = match class_uuid {
        POINT => {
            let position = read_point(&mut reader, scale)?;
            DecodedGeometry::Point {
                position,
                scaled: scale != 1.0,
            }
        }
        POINT_CLOUD => DecodedGeometry::PointCloud(read_cloud(&mut reader, scale)?),
        LINE => DecodedGeometry::Curve {
            curve: DecodedCurve {
                geometry: CurveGeometry::Nurbs(read_line(&mut reader, scale, Some(3))?),
                compound: None,
                warnings: Vec::new(),
            },
        },
        ARC => {
            let (geometry, warnings) = read_arc(&mut reader, scale, None, false)?;
            DecodedGeometry::Curve {
                curve: DecodedCurve {
                    geometry,
                    compound: None,
                    warnings,
                },
            }
        }
        POLYLINE => DecodedGeometry::Curve {
            curve: DecodedCurve {
                geometry: CurveGeometry::Nurbs(read_polyline(&mut reader, scale, Some(3))?),
                compound: None,
                warnings: Vec::new(),
            },
        },
        POLYCURVE | POLYCURVE_LEGACY => {
            let curve = read_polycurve(data, &mut reader, scale, archive, depth)?;
            DecodedGeometry::Curve { curve }
        }
        NURBS_CURVE | NURBS_CURVE_TL | NURBS_CURVE_LEGACY => DecodedGeometry::Curve {
            curve: DecodedCurve {
                geometry: CurveGeometry::Nurbs(crate::surfaces::read_nurbs_curve(
                    &mut reader,
                    scale,
                )?),
                compound: None,
                warnings: Vec::new(),
            },
        },
        _ => return Err(unsupported(range.start, "unsupported Rhino geometry class")),
    };
    if reader.remaining() != 0 {
        return Err(malformed(
            reader.position(),
            "geometry payload has trailing bytes",
        ));
    }
    Ok(result)
}

/// Reads one bounded polymorphic child and requires it to be a curve.
pub(crate) fn decode_embedded_curve(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: f64,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedCurve, GeometryError> {
    if depth > MAX_CURVE_DEPTH {
        return Err(malformed(
            reader.position(),
            "curve recursion limit exceeded",
        ));
    }
    let start = reader.position();
    let wrapper = crate::chunks::chunk_at(data, start, reader.end(), archive, false)?;
    let mut wrapper_warnings = Vec::new();
    let class = parse_class_wrapper(
        data,
        start..wrapper.next_offset,
        archive,
        &mut wrapper_warnings,
    )?;
    reader.skip(wrapper.next_offset - start)?;
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
        return Err(malformed(
            start,
            "embedded surface child is not a supported curve",
        ));
    }
    let decoded = decode_inner(
        data,
        class.class_uuid,
        class.class_data_range,
        scale,
        archive,
        depth,
    )?;
    let DecodedGeometry::Curve { mut curve } = decoded else {
        return Err(malformed(start, "embedded surface child is not a curve"));
    };
    curve.warnings.splice(0..0, wrapper_warnings);
    Ok(curve)
}

/// Reads one bounded polymorphic plane-space curve and applies length scaling.
pub(crate) fn decode_embedded_curve_2d(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: f64,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedCurve, GeometryError> {
    if depth > MAX_CURVE_DEPTH {
        return Err(malformed(
            reader.position(),
            "plane-space curve recursion limit exceeded",
        ));
    }
    let start = reader.position();
    let wrapper = crate::chunks::chunk_at(data, start, reader.end(), archive, false)?;
    let mut wrapper_warnings = Vec::new();
    let class = parse_class_wrapper(
        data,
        start..wrapper.next_offset,
        archive,
        &mut wrapper_warnings,
    )?;
    reader.skip(wrapper.next_offset - start)?;
    if !curve_class(class.class_uuid) || matches!(class.class_uuid, CURVE_PROXY | CURVE_ON_SURFACE)
    {
        return Err(malformed(
            start,
            "embedded plane-space object is not a supported curve",
        ));
    }
    let decoded = decode_inner_2d(
        data,
        class.class_uuid,
        class.class_data_range,
        archive,
        depth,
    )?;
    let DecodedGeometry::Curve { mut curve } = decoded else {
        return Err(malformed(
            start,
            "embedded plane-space object is not a curve",
        ));
    };
    scale_decoded_curve(&mut curve, scale, start)?;
    curve.warnings.splice(0..0, wrapper_warnings);
    Ok(curve)
}

fn scale_decoded_curve(
    curve: &mut DecodedCurve,
    scale: f64,
    offset: usize,
) -> Result<(), GeometryError> {
    if let Some(compound) = &mut curve.compound {
        for child in &mut compound.children {
            scale_decoded_curve(child, scale, offset)?;
        }
        return Ok(());
    }
    match &mut curve.geometry {
        CurveGeometry::Nurbs(nurbs) => {
            for point in &mut nurbs.control_points {
                *point = scale_ir_point(*point, scale)
                    .ok_or_else(|| malformed(offset, "scaled plane-space curve is invalid"))?;
            }
        }
        CurveGeometry::Circle { center, radius, .. } => {
            *center = scale_ir_point(*center, scale)
                .ok_or_else(|| malformed(offset, "scaled plane-space circle is invalid"))?;
            *radius *= scale;
            if !radius.is_finite() || *radius <= 0.0 {
                return Err(malformed(
                    offset,
                    "scaled plane-space circle radius is invalid",
                ));
            }
        }
        CurveGeometry::Line { origin, .. } => {
            *origin = scale_ir_point(*origin, scale)
                .ok_or_else(|| malformed(offset, "scaled plane-space line is invalid"))?;
        }
        CurveGeometry::Degenerate { point } => {
            *point = scale_ir_point(*point, scale)
                .ok_or_else(|| malformed(offset, "scaled plane-space point is invalid"))?;
        }
        CurveGeometry::Unknown { .. } => {
            return Err(malformed(offset, "plane-space curve has unknown geometry"));
        }
        _ => return Err(malformed(offset, "unsupported plane-space analytic curve")),
    }
    Ok(())
}

fn scale_ir_point(value: Point3, scale: f64) -> Option<Point3> {
    let point = Point3::new(value.x * scale, value.y * scale, value.z * scale);
    (point.x.is_finite() && point.y.is_finite() && point.z.is_finite()).then_some(point)
}

/// Converts a decoded curve tree to one exact NURBS curve when possible.
pub(crate) fn exact_nurbs(
    curve: &DecodedCurve,
    offset: usize,
) -> Result<NurbsCurve, GeometryError> {
    let Some(compound) = &curve.compound else {
        return match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => Ok(nurbs.clone()),
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                let yaxis = cross(*axis, *ref_direction);
                let circle = Circle {
                    center: *center,
                    axis: *axis,
                    xaxis: *ref_direction,
                    yaxis,
                    radius: *radius,
                };
                Ok(arc_nurbs(&circle, [0.0, TAU], [0.0, TAU], TAU))
            }
            _ => Err(error(offset, "curve has no exact NURBS representation")),
        };
    };
    if compound.children.len().checked_add(1) != Some(compound.parameters.len()) {
        return Err(error(offset, "polycurve parameter count mismatch"));
    }
    let mut segments = Vec::with_capacity(compound.children.len());
    for (index, child) in compound.children.iter().enumerate() {
        let target = [compound.parameters[index], compound.parameters[index + 1]];
        if !target[0].is_finite() || !target[1].is_finite() || target[0] >= target[1] {
            return Err(error(offset, "polycurve segment domain is invalid"));
        }
        segments.push(remap_nurbs_domain(
            exact_nurbs(child, offset)?,
            target,
            offset,
        )?);
    }
    merge_nurbs_segments(segments, offset)
}

fn remap_nurbs_domain(
    mut curve: NurbsCurve,
    target: [f64; 2],
    offset: usize,
) -> Result<NurbsCurve, GeometryError> {
    let degree =
        usize::try_from(curve.degree).map_err(|_| error(offset, "curve degree is too large"))?;
    let end_index = curve
        .knots
        .len()
        .checked_sub(degree + 1)
        .ok_or_else(|| error(offset, "curve knot vector is invalid"))?;
    let source = [
        *curve
            .knots
            .get(degree)
            .ok_or_else(|| error(offset, "curve knot vector is invalid"))?,
        curve.knots[end_index],
    ];
    let denominator = source[1] - source[0];
    if !denominator.is_finite() || denominator <= 0.0 {
        return Err(error(offset, "curve domain is invalid"));
    }
    let factor = (target[1] - target[0]) / denominator;
    for knot in &mut curve.knots {
        *knot = target[0] + (*knot - source[0]) * factor;
        if !knot.is_finite() {
            return Err(error(offset, "curve knot remap overflowed"));
        }
    }
    Ok(curve)
}

fn merge_nurbs_segments(
    mut segments: Vec<NurbsCurve>,
    offset: usize,
) -> Result<NurbsCurve, GeometryError> {
    let Some(first) = segments.first() else {
        return Err(error(offset, "polycurve has no segments"));
    };
    if segments.len() == 1 {
        return Ok(segments.remove(0));
    }
    let degree = first.degree;
    if segments.iter().any(|segment| segment.degree != degree) {
        return Err(error(offset, "polycurve segments have unequal degrees"));
    }
    let multiplicity = usize::try_from(degree)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| error(offset, "curve degree overflow"))?;
    for segment in &segments {
        if segment.knots.len() < multiplicity {
            return Err(error(offset, "polycurve segment knot vector is invalid"));
        }
        let start = segment.knots.get(multiplicity - 1).copied();
        let end = segment
            .knots
            .len()
            .checked_sub(multiplicity)
            .and_then(|index| segment.knots.get(index))
            .copied();
        if start.is_none()
            || end.is_none()
            || segment.knots[..multiplicity]
                .iter()
                .any(|value| Some(*value) != start)
            || segment.knots[segment.knots.len() - multiplicity..]
                .iter()
                .any(|value| Some(*value) != end)
        {
            return Err(error(offset, "polycurve segment is not endpoint-clamped"));
        }
    }
    let rational = segments.iter().any(|segment| segment.weights.is_some());
    let control_count = segments
        .iter()
        .try_fold(0_usize, |total, segment| {
            total.checked_add(segment.control_points.len())
        })
        .ok_or_else(|| error(offset, "polycurve size overflow"))?;
    let knot_count = segments
        .iter()
        .try_fold(0_usize, |total, segment| {
            total.checked_add(segment.knots.len())
        })
        .and_then(|total| {
            (segments.len() - 1)
                .checked_mul(multiplicity)
                .and_then(|duplicates| total.checked_sub(duplicates))
        })
        .ok_or_else(|| error(offset, "polycurve size overflow"))?;
    let mut control_points = Vec::with_capacity(control_count);
    let mut knots = Vec::with_capacity(knot_count);
    let mut weights = rational.then(|| Vec::with_capacity(control_count));
    for (index, segment) in segments.into_iter().enumerate() {
        if let Some(target) = &mut weights {
            match segment.weights {
                Some(values) => target.extend(values),
                None => target.extend(std::iter::repeat_n(1.0, segment.control_points.len())),
            }
        }
        control_points.extend(segment.control_points);
        knots.extend(
            segment
                .knots
                .into_iter()
                .skip(if index == 0 { 0 } else { multiplicity }),
        );
    }
    Ok(NurbsCurve {
        degree,
        knots,
        control_points,
        weights,
        periodic: false,
    })
}

pub(crate) fn decode_inner_2d(
    data: &[u8],
    class_uuid: Uuid,
    range: Range<usize>,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedGeometry, GeometryError> {
    if depth > MAX_CURVE_DEPTH {
        return Err(malformed(range.start, "C2 curve recursion limit exceeded"));
    }
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let result = match class_uuid {
        NURBS_CURVE | NURBS_CURVE_TL | NURBS_CURVE_LEGACY => DecodedGeometry::Curve {
            curve: DecodedCurve {
                geometry: CurveGeometry::Nurbs(crate::surfaces::read_nurbs_curve_2d(&mut reader)?),
                compound: None,
                warnings: Vec::new(),
            },
        },
        LINE => DecodedGeometry::Curve {
            curve: DecodedCurve {
                geometry: CurveGeometry::Nurbs(read_line(&mut reader, 1.0, Some(2))?),
                compound: None,
                warnings: Vec::new(),
            },
        },
        POLYLINE => DecodedGeometry::Curve {
            curve: DecodedCurve {
                geometry: CurveGeometry::Nurbs(read_polyline(&mut reader, 1.0, Some(2))?),
                compound: None,
                warnings: Vec::new(),
            },
        },
        ARC => {
            let (geometry, warnings) = read_arc(&mut reader, 1.0, Some(2), true)?;
            DecodedGeometry::Curve {
                curve: DecodedCurve {
                    geometry,
                    compound: None,
                    warnings,
                },
            }
        }
        POLYCURVE | POLYCURVE_LEGACY => {
            let curve = read_polycurve_2d(data, &mut reader, archive, depth)?;
            DecodedGeometry::Curve { curve }
        }
        _ => return Err(unsupported(range.start, "unsupported Rhino C2 curve class")),
    };
    if reader.remaining() != 0 {
        return Err(malformed(
            reader.position(),
            "C2 curve payload has trailing bytes",
        ));
    }
    Ok(result)
}

fn read_polycurve_2d(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedCurve, GeometryError> {
    let version = reader.u8()?;
    if version >> 4 != 1 {
        return Err(unsupported(
            reader.position() - 1,
            "unsupported C2 polycurve payload version",
        ));
    }
    let segment_count = count(reader, 1)?;
    if segment_count == 0 {
        return Err(malformed(reader.position(), "C2 polycurve has no segments"));
    }
    reader.i32()?;
    reader.i32()?;
    reader.skip(48)?;
    let parameter_count = count(reader, 8)?;
    if parameter_count != segment_count + 1 {
        return Err(malformed(
            reader.position(),
            "C2 polycurve parameter count mismatch",
        ));
    }
    let mut parameters = Vec::with_capacity(parameter_count);
    for _ in 0..parameter_count {
        let value = reader.f64()?;
        push_polycurve_parameter(&mut parameters, value, reader.position(), "C2 polycurve")?;
    }
    let mut children = Vec::with_capacity(segment_count);
    for _ in 0..segment_count {
        let start = reader.position();
        let wrapper = crate::chunks::chunk_at(data, start, reader.end(), archive, false)?;
        let mut wrapper_warnings = Vec::new();
        let class = parse_class_wrapper(
            data,
            start..wrapper.next_offset,
            archive,
            &mut wrapper_warnings,
        )?;
        reader.skip(wrapper.next_offset - start)?;
        let child = decode_inner_2d(
            data,
            class.class_uuid,
            class.class_data_range,
            archive,
            depth + 1,
        )?;
        let DecodedGeometry::Curve { mut curve } = child else {
            return Err(malformed(start, "C2 polycurve child is not a curve"));
        };
        curve.warnings.splice(0..0, wrapper_warnings);
        children.push(curve);
    }
    Ok(DecodedCurve {
        geometry: CurveGeometry::Unknown { record: None },
        compound: Some(Compound {
            children,
            parameters,
        }),
        warnings: Vec::new(),
    })
}

fn read_point(reader: &mut BoundedReader<'_>, scale: f64) -> Result<Point3, GeometryError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let point = native_point(reader)?;
    scale_point(point, scale)
        .ok_or_else(|| error(reader.position(), "scaled point coordinate is invalid"))
}

fn read_cloud(reader: &mut BoundedReader<'_>, scale: f64) -> Result<PointCloud, GeometryError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let minor = version & 0x0f;
    if minor > 2 {
        return Err(error(
            reader.position() - 1,
            "unsupported point-cloud payload minor version",
        ));
    }
    let point_count = count(reader, 24)?;
    let mut points = Vec::with_capacity(point_count);
    for _ in 0..point_count {
        let point = native_point(reader)?;
        points.push(
            scale_point(point, scale)
                .ok_or_else(|| error(reader.position(), "scaled point coordinate is invalid"))?,
        );
    }
    let native_plane = plane(reader)?;
    let _bounds = bbox(reader)?;
    let flags = reader.i32()?;
    let normals = if minor >= 1 {
        read_vectors(reader, point_count)?
    } else {
        Vec::new()
    };
    let colors = if minor >= 1 {
        let color_count = count(reader, 4)?;
        let mut values: Vec<[u8; 4]> = Vec::with_capacity(color_count);
        for _ in 0..color_count {
            values.push(reader.take(4)?.try_into().expect("color width checked"));
        }
        values
    } else {
        Vec::new()
    };
    let values = if minor >= 2 {
        let value_count = count(reader, 8)?;
        let mut values = Vec::with_capacity(value_count);
        for _ in 0..value_count {
            let value = reader.f64()?;
            if !value.is_finite() {
                return Err(error(reader.position(), "point-cloud value is not finite"));
            }
            values.push(value);
        }
        values
    } else {
        Vec::new()
    };
    if point_count == 0
        || (!normals.is_empty() && normals.len() != point_count)
        || (!colors.is_empty() && colors.len() != point_count)
        || (!values.is_empty() && values.len() != point_count)
    {
        return Err(error(
            reader.position(),
            "point-cloud channel count is invalid",
        ));
    }
    let _ = (normals, colors, values, flags, native_plane);
    Ok(PointCloud {
        points,
        scaled: scale != 1.0,
    })
}

fn read_line(
    reader: &mut BoundedReader<'_>,
    scale: f64,
    expected_dimension: Option<i32>,
) -> Result<NurbsCurve, GeometryError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let from = scale_point(native_point(reader)?, scale)
        .ok_or_else(|| error(reader.position(), "scaled line coordinate is invalid"))?;
    let to = scale_point(native_point(reader)?, scale)
        .ok_or_else(|| error(reader.position(), "scaled line coordinate is invalid"))?;
    let domain = finite_interval(interval(reader)?, reader.position())?;
    let dimension = reader.i32()?;
    if expected_dimension.is_some_and(|expected| dimension != expected)
        || !(dimension == 2 || dimension == 3)
        || from == to
        || domain[0] >= domain[1]
    {
        return Err(error(reader.position(), "invalid bounded line"));
    }
    Ok(NurbsCurve {
        degree: 1,
        knots: vec![domain[0], domain[0], domain[1], domain[1]],
        control_points: vec![from, to],
        weights: None,
        periodic: false,
    })
}

fn read_polyline(
    reader: &mut BoundedReader<'_>,
    scale: f64,
    expected_dimension: Option<i32>,
) -> Result<NurbsCurve, GeometryError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let point_count = count(reader, 24)?;
    if point_count < 2 {
        return Err(error(
            reader.position(),
            "polyline needs at least two points",
        ));
    }
    let mut points = Vec::with_capacity(point_count);
    for _ in 0..point_count {
        let point = native_point(reader)?;
        points
            .push(scale_point(point, scale).ok_or_else(|| {
                error(reader.position(), "scaled polyline coordinate is invalid")
            })?);
    }
    let parameter_count = count(reader, 8)?;
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
    Ok(NurbsCurve {
        degree: 1,
        knots,
        control_points: points,
        weights: None,
        periodic: false,
    })
}

fn read_arc(
    reader: &mut BoundedReader<'_>,
    scale: f64,
    expected_dimension: Option<i32>,
    force_nurbs: bool,
) -> Result<(CurveGeometry, Vec<String>), GeometryError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let circle = read_circle(reader, scale)?;
    let angle = finite_interval(interval(reader)?, reader.position())?;
    let domain = finite_interval(interval(reader)?, reader.position())?;
    let dimension = reader.i32()?;
    let mut warnings = Vec::new();
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
    if delta <= 0.0 || delta > TAU + 1.0e-10 {
        return Err(error(reader.position(), "arc angle span is invalid"));
    }
    if !force_nurbs && canonical_circle(&circle, angle, domain, delta) {
        return Ok((
            CurveGeometry::Circle {
                center: circle.center,
                axis: circle.axis,
                ref_direction: circle.xaxis,
                radius: circle.radius,
            },
            warnings,
        ));
    }
    Ok((
        CurveGeometry::Nurbs(arc_nurbs(&circle, angle, domain, delta)),
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

fn read_circle(reader: &mut BoundedReader<'_>, scale: f64) -> Result<Circle, GeometryError> {
    let native = plane(reader)?;
    let radius = reader.f64()?;
    let zero = native_point(reader)?;
    let half_pi = native_point(reader)?;
    let at_pi = native_point(reader)?;
    let scaled_radius = radius * scale;
    if !radius.is_finite() || radius <= 0.0 || !scaled_radius.is_finite() || scaled_radius <= 0.0 {
        return Err(error(reader.position(), "circle radius is invalid"));
    }
    let xaxis = vector(native.xaxis);
    let yaxis = vector(native.yaxis);
    let axis = vector(native.zaxis);
    let center = scale_point(native.origin, scale)
        .ok_or_else(|| error(reader.position(), "scaled circle center is invalid"))?;
    let norm_x = xaxis.norm();
    let norm_y = yaxis.norm();
    let norm_axis = axis.norm();
    if !(norm_x.is_finite()
        && norm_y.is_finite()
        && norm_axis.is_finite()
        && (norm_x - 1.0).abs() < 1.0e-10
        && (norm_y - 1.0).abs() < 1.0e-10
        && (norm_axis - 1.0).abs() < 1.0e-10
        && dot(xaxis, yaxis).abs() < 1.0e-10
        && dot(xaxis, axis).abs() < 1.0e-10
        && dot(yaxis, axis).abs() < 1.0e-10
        && close_vector(cross(xaxis, yaxis), axis, 1.0e-10)
        && close_native_point(zero, native.origin, native.xaxis, radius)
        && close_native_point(half_pi, native.origin, native.yaxis, radius)
        && close_native_point(at_pi, native.origin, negate(native.xaxis), radius))
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
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: f64,
    archive: ArchiveVersion,
    depth: usize,
) -> Result<DecodedCurve, GeometryError> {
    let version = reader.u8()?;
    if version >> 4 != 1 {
        return Err(unsupported(
            reader.position() - 1,
            "unsupported polycurve payload version",
        ));
    }
    let segment_count = count(reader, 1)?;
    if segment_count == 0 {
        return Err(malformed(reader.position(), "polycurve has no segments"));
    }
    reader.i32()?;
    reader.i32()?;
    reader.skip(48)?;
    let parameter_count = count(reader, 8)?;
    if parameter_count != segment_count + 1 {
        return Err(malformed(
            reader.position(),
            "polycurve parameter count mismatch",
        ));
    }
    let mut parameters = Vec::with_capacity(parameter_count);
    for _ in 0..parameter_count {
        let value = reader.f64()?;
        push_polycurve_parameter(&mut parameters, value, reader.position(), "polycurve")?;
    }
    let mut children = Vec::with_capacity(segment_count);
    for _ in 0..segment_count {
        let start = reader.position();
        let wrapper = crate::chunks::chunk_at(data, start, reader.end(), archive, false)?;
        let mut wrapper_warnings = Vec::new();
        let class = parse_class_wrapper(
            data,
            start..wrapper.next_offset,
            archive,
            &mut wrapper_warnings,
        )?;
        reader.skip(wrapper.next_offset - start)?;
        if !supported_class(class.class_uuid) || matches!(class.class_uuid, POINT | POINT_CLOUD) {
            return Err(malformed(start, "polycurve child is not a curve"));
        }
        let child = decode_inner(
            data,
            class.class_uuid,
            class.class_data_range,
            scale,
            archive,
            depth + 1,
        )?;
        let DecodedGeometry::Curve { mut curve } = child else {
            return Err(malformed(start, "polycurve child is not a curve"));
        };
        curve.warnings.splice(0..0, wrapper_warnings);
        children.push(curve);
    }
    if children.len() != segment_count {
        return Err(malformed(
            reader.position(),
            "polycurve child count changed",
        ));
    }
    Ok(DecodedCurve {
        geometry: CurveGeometry::Unknown { record: None },
        compound: Some(Compound {
            children,
            parameters,
        }),
        warnings: Vec::new(),
    })
}

fn push_polycurve_parameter(
    parameters: &mut Vec<f64>,
    value: f64,
    offset: usize,
    label: &str,
) -> Result<(), GeometryError> {
    if !value.is_finite() || parameters.last().is_some_and(|previous| value <= *previous) {
        return Err(malformed(
            offset,
            &format!("{label} parameters are invalid"),
        ));
    }
    parameters.push(value);
    Ok(())
}

fn arc_nurbs(circle: &Circle, angle: [f64; 2], domain: [f64; 2], delta: f64) -> NurbsCurve {
    let spans = (delta / FRAC_PI_2).ceil().max(1.0) as usize;
    let step = delta / spans as f64;
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
        let t0 = domain[0] + (domain[1] - domain[0]) * span as f64 / spans as f64;
        let t1 = domain[0] + (domain[1] - domain[0]) * (span + 1) as f64 / spans as f64;
        if span == 0 {
            knots.extend([t0, t0, t0]);
        } else {
            knots.extend([t0, t0]);
        }
        if span + 1 == spans {
            knots.extend([t1, t1, t1]);
        }
    }
    NurbsCurve {
        degree: 2,
        knots,
        control_points,
        weights: Some(weights),
        periodic: false,
    }
}

fn canonical_circle(circle: &Circle, angle: [f64; 2], domain: [f64; 2], delta: f64) -> bool {
    (delta - TAU).abs() < 1.0e-10
        && angle[0].abs() < 1.0e-10
        && (domain[0]).abs() < 1.0e-10
        && (domain[1] - TAU).abs() < 1.0e-10
        && circle.xaxis.norm() == 1.0
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

fn read_vectors(
    reader: &mut BoundedReader<'_>,
    expected: usize,
) -> Result<Vec<Vector3>, GeometryError> {
    let count = count(reader, 24)?;
    if count != 0 && count != expected {
        return Err(error(
            reader.position(),
            "point-cloud normal count mismatch",
        ));
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(vector(crate::settings::vector(reader)?));
    }
    Ok(values)
}

fn vector(value: crate::settings::Vector3) -> Vector3 {
    Vector3::new(value.0[0], value.0[1], value.0[2])
}

fn native_point(reader: &mut BoundedReader<'_>) -> Result<NativePoint3, FramingError> {
    crate::settings::point(reader)
}

fn scale_point(value: NativePoint3, scale: f64) -> Option<Point3> {
    Some(Point3::new(
        crate::wire::scaled_coordinate(value.0[0], scale)?,
        crate::wire::scaled_coordinate(value.0[1], scale)?,
        crate::wire::scaled_coordinate(value.0[2], scale)?,
    ))
}

fn finite_interval(
    value: crate::settings::Interval,
    offset: usize,
) -> Result<[f64; 2], GeometryError> {
    if value.0[0].is_finite() && value.0[1].is_finite() {
        Ok(value.0)
    } else {
        Err(error(offset, "interval contains a nonfinite value"))
    }
}

fn count(reader: &mut BoundedReader<'_>, width: usize) -> Result<usize, GeometryError> {
    let raw = reader.i32()?;
    let bytes = checked_count_bytes(
        raw,
        width,
        reader.remaining(),
        MAX_CURVE_ITEMS,
        reader.position() - 4,
    )?;
    Ok(bytes / width)
}

fn require_major(version: u8, offset: usize) -> Result<(), GeometryError> {
    if version >> 4 == 1 {
        Ok(())
    } else {
        Err(unsupported(
            offset,
            "unsupported simple-geometry payload version",
        ))
    }
}

fn dot(left: Vector3, right: Vector3) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}

fn cross(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(
        left.y * right.z - left.z * right.y,
        left.z * right.x - left.x * right.z,
        left.x * right.y - left.y * right.x,
    )
}

fn close_vector(left: Vector3, right: Vector3, tolerance: f64) -> bool {
    (left.x - right.x).abs() <= tolerance
        && (left.y - right.y).abs() <= tolerance
        && (left.z - right.z).abs() <= tolerance
}

fn negate(value: crate::settings::Vector3) -> crate::settings::Vector3 {
    crate::settings::Vector3([-value.0[0], -value.0[1], -value.0[2]])
}

fn close_native_point(
    point: NativePoint3,
    origin: NativePoint3,
    direction: crate::settings::Vector3,
    radius: f64,
) -> bool {
    let expected = [
        origin.0[0] + direction.0[0] * radius,
        origin.0[1] + direction.0[1] * radius,
        origin.0[2] + direction.0[2] * radius,
    ];
    point
        .0
        .iter()
        .zip(expected)
        .all(|(actual, expected)| (*actual - expected).abs() <= 1.0e-8)
}

pub(crate) fn error(offset: usize, message: &str) -> GeometryError {
    GeometryError::Malformed(framing_error(offset, message))
}

fn framing_error(offset: usize, message: &str) -> FramingError {
    FramingError::Structural {
        offset,
        message: message.to_string(),
    }
}

fn malformed(offset: usize, message: &str) -> GeometryError {
    error(offset, message)
}

pub(crate) fn unsupported(offset: usize, message: &str) -> GeometryError {
    GeometryError::UnsupportedVersion {
        offset,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_on_surface_obeys_the_shared_curve_recursion_limit() {
        let error = decode_inner(
            &[],
            CURVE_ON_SURFACE,
            0..0,
            1.0,
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
        let arc = arc_nurbs(&circle, [0.0, PI], [10.0, 20.0], PI);
        assert_eq!(arc.degree, 2);
        assert_eq!(
            arc.control_points.first(),
            Some(&circle_point(&circle, 0.0))
        );
        assert_eq!(arc.control_points.last(), Some(&circle_point(&circle, PI)));
        assert_eq!(
            arc.weights.as_ref().expect("rational arc")[1],
            2.0_f64.sqrt() / 2.0
        );
        let midpoint = circle_point(&circle, PI / 2.0);
        let pole = arc.control_points[2];
        let weight = arc.weights.as_ref().expect("rational arc")[2];
        assert!((pole.x * weight - midpoint.x).abs() < 1.0e-12);
        assert!((pole.y * weight - midpoint.y).abs() < 1.0e-12);
    }

    #[test]
    fn full_canonical_circle_is_analytic_but_shifted_circle_is_rational() {
        let circle = unit_circle();
        assert!(canonical_circle(&circle, [0.0, TAU], [0.0, TAU], TAU));
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
        let arc = arc_nurbs(&circle, [0.0, 3.0 * PI], [0.0, 3.0], 3.0 * PI);
        assert_eq!(arc.control_points.len(), 2 * 6 + 1);
        assert_eq!(arc.knots.len(), arc.control_points.len() + 3);
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
            let curve = read_line(&mut reader, 1.0, None).expect("valid line");
            assert_eq!(curve.knots, vec![2.0, 2.0, 5.0, 5.0]);
        }
    }

    #[test]
    fn future_polycurve_version_is_structured_as_unsupported() {
        let bytes = [0x20];
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded");
        let result = read_polycurve(&bytes, &mut reader, 1.0, ArchiveVersion::V5, 0);
        assert!(matches!(
            result,
            Err(GeometryError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn top_level_polycurve_rejects_equal_adjacent_boundaries() {
        let mut parameters = vec![1.0];
        assert!(push_polycurve_parameter(&mut parameters, 1.0, 8, "polycurve").is_err());
    }

    #[test]
    fn c2_polycurve_rejects_equal_adjacent_boundaries() {
        let mut parameters = vec![1.0];
        assert!(push_polycurve_parameter(&mut parameters, 1.0, 8, "C2 polycurve").is_err());
    }

    #[test]
    fn analytic_full_circle_converts_to_exact_quadratic_nurbs() {
        let circle = unit_circle();
        let decoded = DecodedCurve {
            geometry: CurveGeometry::Circle {
                center: circle.center,
                axis: circle.axis,
                ref_direction: circle.xaxis,
                radius: circle.radius,
            },
            compound: None,
            warnings: Vec::new(),
        };
        let nurbs = exact_nurbs(&decoded, 0).expect("required invariant");
        assert_eq!(nurbs.degree, 2);
        assert_eq!(nurbs.control_points.len(), 9);
        assert_eq!(nurbs.knots.len(), 12);
        assert_eq!(nurbs.knots[0], 0.0);
        assert_eq!(*nurbs.knots.last().expect("required invariant"), TAU);
        assert_eq!(
            nurbs.weights.expect("required invariant")[1],
            2.0_f64.sqrt() / 2.0
        );
    }

    #[test]
    fn recursive_compound_conversion_preserves_parent_domain_when_exact() {
        let line = |start: f64, end: f64| DecodedCurve {
            geometry: CurveGeometry::Nurbs(NurbsCurve {
                degree: 1,
                knots: vec![start, start, end, end],
                control_points: vec![Point3::new(start, 0.0, 0.0), Point3::new(end, 0.0, 0.0)],
                weights: None,
                periodic: false,
            }),
            compound: None,
            warnings: Vec::new(),
        };
        let nested = DecodedCurve {
            geometry: CurveGeometry::Unknown { record: None },
            compound: Some(Compound {
                children: vec![line(0.0, 1.0), line(0.0, 1.0)],
                parameters: vec![2.0, 3.0, 5.0],
            }),
            warnings: Vec::new(),
        };
        let converted = exact_nurbs(&nested, 0).expect("required invariant");
        assert_eq!(converted.knots, vec![2.0, 2.0, 3.0, 3.0, 5.0, 5.0]);
        assert_eq!(converted.control_points.len(), 4);
    }
}
