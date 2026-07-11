// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino point and simple-curve payload decoding.

use std::f64::consts::{FRAC_PI_2, TAU};
use std::ops::Range;

use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve};
use cadmpeg_ir::math::{Point3, Vector3};

use crate::chunks::{checked_count_bytes, BoundedReader, FramingError};
use crate::objects::{parse_class_wrapper, Uuid};
use crate::settings::{bbox, interval, plane, Point3 as NativePoint3};

/// Maximum embedded curve nesting depth.
pub(crate) const MAX_CURVE_DEPTH: usize = 32;
/// Maximum points or polycurve segments in one payload.
pub(crate) const MAX_CURVE_ITEMS: usize = 1 << 16;

const POINT: &str = "c3101a1d-f157-11d3-bfe7-0010830122f0";
const POINT_CLOUD: &str = "2488f347-f8fa-11d3-bfec-0010830122f0";
const LINE: &str = "4ed7d4db-e947-11d3-bfe5-0010830122f0";
const ARC: &str = "cf33be2a-09b4-11d4-bffb-0010830122f0";
const POLYLINE: &str = "4ed7d4e6-e947-11d3-bfe5-0010830122f0";
const POLYCURVE: &str = "4ed7d4e0-e947-11d3-bfe5-0010830122f0";

/// A decoded point or curve before it is inserted into the IR arenas.
#[derive(Debug, Clone)]
pub(crate) enum DecodedGeometry {
    /// One point.
    Point(Point3),
    /// One point cloud and its optional native channels.
    PointCloud(PointCloud),
    /// A curve and ordered embedded children.
    Curve {
        /// Solved carrier geometry.
        geometry: CurveGeometry,
        /// Compound construction, when this is a polycurve.
        compound: Option<Compound>,
    },
}

/// Point-cloud channels retained by the native namespace boundary.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct PointCloud {
    /// Ordered points.
    pub(crate) points: Vec<Point3>,
    /// Optional normals.
    pub(crate) normals: Vec<Vector3>,
    /// Optional direct colors.
    pub(crate) colors: Vec<[u8; 4]>,
    /// Optional scalar values.
    pub(crate) values: Vec<f64>,
    /// Native ordering flag.
    pub(crate) ordered: bool,
    /// Whether the native plane was set.
    pub(crate) plane_set: bool,
}

/// A validated polycurve construction.
#[derive(Debug, Clone)]
pub(crate) struct Compound {
    /// Child carrier geometries in source order.
    pub(crate) children: Vec<CurveGeometry>,
    /// Child segment parameters.
    pub(crate) parameters: Vec<f64>,
}

/// Dispatches a class UUID to the supported simple-geometry reader.
pub(crate) fn supported_class(uuid: Uuid) -> bool {
    matches!(
        uuid.to_string().as_str(),
        POINT | POINT_CLOUD | LINE | ARC | POLYLINE | POLYCURVE
    )
}

/// Decode one top-level class-data payload.
pub(crate) fn decode(
    data: &[u8],
    class_uuid: Uuid,
    range: Range<usize>,
    scale: f64,
) -> Result<DecodedGeometry, FramingError> {
    decode_inner(data, class_uuid, range, scale, 0)
}

fn decode_inner(
    data: &[u8],
    class_uuid: Uuid,
    range: Range<usize>,
    scale: f64,
    depth: usize,
) -> Result<DecodedGeometry, FramingError> {
    if depth > MAX_CURVE_DEPTH {
        return Err(error(range.start, "curve recursion limit exceeded"));
    }
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let name = class_uuid.to_string();
    let result = match name.as_str() {
        POINT => DecodedGeometry::Point(read_point(&mut reader, scale)?),
        POINT_CLOUD => DecodedGeometry::PointCloud(read_cloud(&mut reader, scale)?),
        LINE => DecodedGeometry::Curve {
            geometry: CurveGeometry::Nurbs(read_line(&mut reader, scale)?),
            compound: None,
        },
        ARC => DecodedGeometry::Curve {
            geometry: read_arc(&mut reader, scale)?,
            compound: None,
        },
        POLYLINE => DecodedGeometry::Curve {
            geometry: CurveGeometry::Nurbs(read_polyline(&mut reader, scale)?),
            compound: None,
        },
        POLYCURVE => {
            let (geometry, compound) = read_polycurve(data, &mut reader, scale, depth)?;
            DecodedGeometry::Curve {
                geometry,
                compound: Some(compound),
            }
        }
        _ => return Err(error(range.start, "unsupported Rhino geometry class")),
    };
    if reader.remaining() != 0 {
        return Err(error(
            reader.position(),
            "geometry payload has trailing bytes",
        ));
    }
    Ok(result)
}

fn read_point(reader: &mut BoundedReader<'_>, scale: f64) -> Result<Point3, FramingError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let point = native_point(reader)?;
    Ok(scale_point(point, scale))
}

fn read_cloud(reader: &mut BoundedReader<'_>, scale: f64) -> Result<PointCloud, FramingError> {
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
        points.push(scale_point(native_point(reader)?, scale));
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
        let mut values = Vec::with_capacity(color_count);
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
    Ok(PointCloud {
        points,
        normals,
        colors,
        values,
        ordered: flags & 1 != 0,
        plane_set: flags & 2 != 0 && native_plane.origin.0.iter().all(|value| value.is_finite()),
    })
}

fn read_line(reader: &mut BoundedReader<'_>, scale: f64) -> Result<NurbsCurve, FramingError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let from = scale_point(native_point(reader)?, scale);
    let to = scale_point(native_point(reader)?, scale);
    let domain = finite_interval(interval(reader)?, reader.position())?;
    let dimension = reader.i32()?;
    if dimension != 3 || from == to || domain[0] >= domain[1] {
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

fn read_polyline(reader: &mut BoundedReader<'_>, scale: f64) -> Result<NurbsCurve, FramingError> {
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
        points.push(scale_point(native_point(reader)?, scale));
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
    if dimension != 3 {
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

fn read_arc(reader: &mut BoundedReader<'_>, scale: f64) -> Result<CurveGeometry, FramingError> {
    let version = reader.u8()?;
    require_major(version, reader.position() - 1)?;
    let circle = read_circle(reader, scale)?;
    let angle = finite_interval(interval(reader)?, reader.position())?;
    let domain = finite_interval(interval(reader)?, reader.position())?;
    let _dimension = reader.i32()?;
    if domain[0] >= domain[1] || angle[0] >= angle[1] {
        return Err(error(reader.position(), "arc interval is not increasing"));
    }
    let delta = angle[1] - angle[0];
    if delta <= 0.0 || delta > TAU + 1.0e-10 {
        return Err(error(reader.position(), "arc angle span is invalid"));
    }
    if canonical_circle(&circle, angle, domain, delta) {
        return Ok(CurveGeometry::Circle {
            center: circle.center,
            axis: circle.axis,
            ref_direction: circle.xaxis,
            radius: circle.radius,
        });
    }
    Ok(CurveGeometry::Nurbs(arc_nurbs(
        &circle, angle, domain, delta,
    )))
}

#[derive(Debug, Clone, Copy)]
struct Circle {
    center: Point3,
    axis: Vector3,
    xaxis: Vector3,
    yaxis: Vector3,
    radius: f64,
}

fn read_circle(reader: &mut BoundedReader<'_>, scale: f64) -> Result<Circle, FramingError> {
    let native = plane(reader)?;
    let radius = reader.f64()?;
    let _zero = native_point(reader)?;
    let _half_pi = native_point(reader)?;
    let _pi = native_point(reader)?;
    if !radius.is_finite() || radius <= 0.0 {
        return Err(error(reader.position(), "circle radius is invalid"));
    }
    let xaxis = vector(native.xaxis);
    let yaxis = vector(native.yaxis);
    let axis = vector(native.zaxis);
    let center = scale_point(native.origin, scale);
    let norm_x = xaxis.norm();
    let norm_y = yaxis.norm();
    let norm_axis = axis.norm();
    if !(norm_x.is_finite()
        && norm_y.is_finite()
        && norm_axis.is_finite()
        && (norm_x - 1.0).abs() < 1.0e-10
        && (norm_y - 1.0).abs() < 1.0e-10
        && (norm_axis - 1.0).abs() < 1.0e-10
        && dot(xaxis, yaxis).abs() < 1.0e-10)
    {
        return Err(error(reader.position(), "circle plane axes are invalid"));
    }
    Ok(Circle {
        center,
        axis,
        xaxis,
        yaxis,
        radius: radius * scale,
    })
}

fn read_polycurve(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: f64,
    depth: usize,
) -> Result<(CurveGeometry, Compound), FramingError> {
    let _version = reader.u8()?;
    let segment_count = count(reader, 1)?;
    if segment_count == 0 {
        return Err(error(reader.position(), "polycurve has no segments"));
    }
    reader.i32()?;
    reader.i32()?;
    let _ = bbox(reader)?;
    let parameter_count = count(reader, 8)?;
    if parameter_count != segment_count + 1 {
        return Err(error(
            reader.position(),
            "polycurve parameter count mismatch",
        ));
    }
    let mut parameters = Vec::with_capacity(parameter_count);
    for _ in 0..parameter_count {
        let value = reader.f64()?;
        if !value.is_finite() || parameters.last().is_some_and(|previous| value < *previous) {
            return Err(error(reader.position(), "polycurve parameters are invalid"));
        }
        parameters.push(value);
    }
    let mut children = Vec::with_capacity(segment_count);
    for _ in 0..segment_count {
        let start = reader.position();
        let wrapper = crate::chunks::chunk_at(
            data,
            start,
            reader.end(),
            crate::chunks::ArchiveVersion::V8,
            false,
        )?;
        let class = parse_class_wrapper(
            data,
            start..wrapper.next_offset,
            crate::chunks::ArchiveVersion::V8,
            &mut Vec::new(),
        )?;
        reader.skip(wrapper.next_offset - start)?;
        if !supported_class(class.class_uuid)
            || matches!(class.class_uuid.to_string().as_str(), POINT | POINT_CLOUD)
        {
            return Err(error(start, "polycurve child is not a curve"));
        }
        let child = decode_inner(
            data,
            class.class_uuid,
            class.class_data_range,
            scale,
            depth + 1,
        )?;
        let DecodedGeometry::Curve { geometry, compound } = child else {
            return Err(error(start, "polycurve child is not a curve"));
        };
        let _ = compound;
        children.push(geometry);
    }
    if children.len() != segment_count {
        return Err(error(reader.position(), "polycurve child count changed"));
    }
    Ok((
        CurveGeometry::Unknown { record: None },
        Compound {
            children,
            parameters,
        },
    ))
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
) -> Result<Vec<Vector3>, FramingError> {
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

fn scale_point(value: NativePoint3, scale: f64) -> Point3 {
    Point3::new(value.0[0] * scale, value.0[1] * scale, value.0[2] * scale)
}

fn finite_interval(
    value: crate::settings::Interval,
    offset: usize,
) -> Result<[f64; 2], FramingError> {
    if value.0[0].is_finite() && value.0[1].is_finite() {
        Ok(value.0)
    } else {
        Err(error(offset, "interval contains a nonfinite value"))
    }
}

fn count(reader: &mut BoundedReader<'_>, width: usize) -> Result<usize, FramingError> {
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

fn require_major(version: u8, offset: usize) -> Result<(), FramingError> {
    if version >> 4 == 1 {
        Ok(())
    } else {
        Err(error(offset, "unsupported simple-geometry payload version"))
    }
}

fn dot(left: Vector3, right: Vector3) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}

fn error(offset: usize, message: &str) -> FramingError {
    FramingError::Structural {
        offset,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
