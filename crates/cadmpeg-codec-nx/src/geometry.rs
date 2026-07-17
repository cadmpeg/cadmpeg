// SPDX-License-Identifier: Apache-2.0
//! Decode point and analytic geometry records from Parasolid neutral-binary data.
//!
//! The scanners recognize points; planes, cylinders, cones, spheres, and tori;
//! and lines, circles, and ellipses. They validate record bounds, finite values,
//! plausible scanner magnitudes, radii, and direction vectors before returning a
//! carrier.
//!
//! Parasolid stores these fields as big-endian metre values. Returned coordinates
//! and radii are in millimetres; unit vectors and curve parameters are unchanged.
//! The scanners test supported field shifts caused by extended references and
//! omit candidates that fail validation. Use [`crate::topology`] to resolve
//! returned record offsets into topology.

use cadmpeg_ir::be::{f64_at as read_f64, vec3_at as read_vec3};
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

/// Candidate byte shifts from expanded leading references. Envelope escapes are
/// resolved by the fixed-record graph, where their independent shift is known.
const SHIFTS: [usize; 6] = [0, 2, 4, 6, 8, 10];

/// A decoded analytic surface and its source offset.
#[derive(Debug, Clone)]
pub struct DecodedSurface {
    /// Byte offset of the record's type tag within the stream.
    pub pos: usize,
    /// The decoded surface geometry.
    pub geometry: SurfaceGeometry,
}

/// A decoded analytic curve and its source offset.
#[derive(Debug, Clone)]
pub struct DecodedCurve {
    /// Byte offset of the record's type tag within the stream.
    pub pos: usize,
    /// The decoded curve geometry.
    pub geometry: CurveGeometry,
}

/// A decoded point and its source offset.
#[derive(Debug, Clone)]
pub struct DecodedPoint {
    /// Byte offset of the record's `00 1d` tag within the stream.
    pub pos: usize,
    /// Position in millimetres.
    pub position: Point3,
}

/// The analytic surface type tags and their fixed record lengths ([spec §4.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/siemens_nx.md#41-fixed-record-families)).
const SURFACE_TAGS: [(u8, usize); 5] = [
    (0x32, 91),  // plane
    (0x33, 99),  // cylinder
    (0x34, 115), // cone
    (0x35, 99),  // sphere
    (0x36, 107), // torus
];

/// The analytic curve type tags and their fixed record lengths ([spec §4.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/siemens_nx.md#41-fixed-record-families)).
const CURVE_TAGS: [(u8, usize); 3] = [
    (0x1e, 67),  // line
    (0x1f, 99),  // circle
    (0x20, 107), // ellipse
];

enum AnalyticRecord {
    Point(DecodedPoint),
    Surface(DecodedSurface),
    Curve(DecodedCurve),
}

#[derive(Clone, Copy)]
enum DecodeContext {
    Scanner,
    Graph,
}

/// Decode validated point records in source order.
///
/// Positions are returned in millimetres. Malformed and out-of-range candidates
/// are skipped.
pub fn points(stream: &[u8]) -> Vec<DecodedPoint> {
    analytic_records(stream)
        .into_iter()
        .filter_map(|record| match record {
            AnalyticRecord::Point(point) => Some(point),
            AnalyticRecord::Surface(_) | AnalyticRecord::Curve(_) => None,
        })
        .collect()
}

/// Decode validated analytic surface records in source order.
pub fn surfaces(stream: &[u8]) -> Vec<DecodedSurface> {
    analytic_records(stream)
        .into_iter()
        .filter_map(|record| match record {
            AnalyticRecord::Surface(surface) => Some(surface),
            AnalyticRecord::Point(_) | AnalyticRecord::Curve(_) => None,
        })
        .collect()
}

/// Decode validated analytic curve records in source order.
pub fn curves(stream: &[u8]) -> Vec<DecodedCurve> {
    analytic_records(stream)
        .into_iter()
        .filter_map(|record| match record {
            AnalyticRecord::Curve(curve) => Some(curve),
            AnalyticRecord::Point(_) | AnalyticRecord::Surface(_) => None,
        })
        .collect()
}

fn analytic_records(stream: &[u8]) -> Vec<AnalyticRecord> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 2 <= stream.len() {
        if stream[p] != 0x00 {
            p += 1;
            continue;
        }
        let kind = stream[p + 1];
        let candidate = if kind == 0x1d {
            decode_point(stream, p)
                .map(|(point, shift)| (AnalyticRecord::Point(point), p + 40 + shift))
        } else if let Some((_, len)) = SURFACE_TAGS.iter().find(|(tag, _)| *tag == kind) {
            decode_surface(stream, p, kind).map(|(geometry, shift)| {
                (
                    AnalyticRecord::Surface(DecodedSurface { pos: p, geometry }),
                    p + *len + shift,
                )
            })
        } else if let Some((_, len)) = CURVE_TAGS.iter().find(|(tag, _)| *tag == kind) {
            decode_curve(stream, p, kind).map(|(geometry, shift)| {
                (
                    AnalyticRecord::Curve(DecodedCurve { pos: p, geometry }),
                    p + *len + shift,
                )
            })
        } else {
            None
        };
        if let Some((record, end)) = candidate {
            out.push(record);
            p = end;
        } else {
            p += 1;
        }
    }
    out
}

fn decode_point(stream: &[u8], p: usize) -> Option<(DecodedPoint, usize)> {
    SHIFTS.into_iter().find_map(|shift| {
        let xyz = read_vec3(stream, p + 16 + shift)?;
        (xyz.iter().all(|value| {
            value.is_finite() && value.abs() < 1.0e3 && (*value == 0.0 || value.abs() >= 1.0e-100)
        }) && xyz.iter().any(|value| *value != 0.0))
        .then_some((
            DecodedPoint {
                pos: p,
                position: mm_point(xyz),
            },
            shift,
        ))
    })
}

/// Decode an analytic surface at tag position `p`, trying each candidate shift and
/// returning the first whose payload passes the kind's validation gate.
fn decode_surface(stream: &[u8], p: usize, kind: u8) -> Option<(SurfaceGeometry, usize)> {
    for sh in SHIFTS {
        let b = p + sh;
        let geom = match kind {
            0x32 => plane(stream, b, DecodeContext::Scanner),
            0x33 => cylinder(stream, b, DecodeContext::Scanner),
            0x34 => cone(stream, b, DecodeContext::Scanner),
            0x35 => sphere(stream, b, DecodeContext::Scanner),
            0x36 => torus(stream, b, DecodeContext::Scanner),
            _ => None,
        };
        if let Some(geometry) = geom {
            return Some((geometry, sh));
        }
    }
    None
}

/// Decode an analytic curve at tag position `p`, trying each candidate shift.
fn decode_curve(stream: &[u8], p: usize, kind: u8) -> Option<(CurveGeometry, usize)> {
    for sh in SHIFTS {
        let b = p + sh;
        let geom = match kind {
            0x1e => line(stream, b, DecodeContext::Scanner),
            0x1f => circle(stream, b, DecodeContext::Scanner),
            0x20 => ellipse(stream, b, DecodeContext::Scanner),
            _ => None,
        };
        if let Some(geometry) = geom {
            return Some((geometry, sh));
        }
    }
    None
}

/// Decode a graph-owned analytic surface at its resolved logical-field shift.
pub(crate) fn decode_surface_record(
    record: &[u8],
    kind: u8,
    shift: usize,
) -> Option<SurfaceGeometry> {
    let b = shift;
    match kind {
        0x32 => plane(record, b, DecodeContext::Graph),
        0x33 => cylinder(record, b, DecodeContext::Graph),
        0x34 => cone(record, b, DecodeContext::Graph),
        0x35 => sphere(record, b, DecodeContext::Graph),
        0x36 => torus(record, b, DecodeContext::Graph),
        _ => None,
    }
}

/// Decode a graph-owned analytic curve at its resolved logical-field shift.
pub(crate) fn decode_curve_record(record: &[u8], kind: u8, shift: usize) -> Option<CurveGeometry> {
    let b = shift;
    match kind {
        0x1e => line(record, b, DecodeContext::Graph),
        0x1f => circle(record, b, DecodeContext::Graph),
        0x20 => ellipse(record, b, DecodeContext::Graph),
        _ => None,
    }
}

// --- Surface decoders (offsets from the common header, [§5.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/siemens_nx.md#51-ownership-graph) / [§6.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/siemens_nx.md#61-analytic-curves-and-surfaces)) ---

fn plane(s: &[u8], b: usize, context: DecodeContext) -> Option<SurfaceGeometry> {
    let origin = read_vec3(s, b + 19)?;
    let normal = read_vec3(s, b + 43)?;
    let x_axis = read_vec3(s, b + 67)?;
    if !is_orthonormal_frame(normal, x_axis) || !valid_position(origin, context) {
        return None;
    }
    Some(SurfaceGeometry::Plane {
        origin: mm_point(origin),
        normal: vec3(normal),
        u_axis: vec3(x_axis),
    })
}

fn cylinder(s: &[u8], b: usize, context: DecodeContext) -> Option<SurfaceGeometry> {
    let origin = read_vec3(s, b + 19)?;
    let axis = read_vec3(s, b + 43)?;
    let radius = read_f64(s, b + 67)?;
    let x_axis = read_vec3(s, b + 75)?;
    if !is_orthonormal_frame(axis, x_axis)
        || !valid_position(origin, context)
        || !valid_radius(radius, context)
    {
        return None;
    }
    Some(SurfaceGeometry::Cylinder {
        origin: mm_point(origin),
        axis: vec3(axis),
        ref_direction: vec3(x_axis),
        radius: radius * 1000.0,
    })
}

fn cone(s: &[u8], b: usize, context: DecodeContext) -> Option<SurfaceGeometry> {
    let origin = read_vec3(s, b + 19)?;
    let axis = read_vec3(s, b + 43)?;
    let radius = read_f64(s, b + 67)?;
    let sin_half = read_f64(s, b + 75)?;
    let cos_half = read_f64(s, b + 83)?;
    let x_axis = read_vec3(s, b + 91)?;
    if !is_orthonormal_frame(axis, x_axis)
        || !valid_position(origin, context)
        || !valid_cone_radius(radius, context)
    {
        return None;
    }
    // The cone's half-angle is carried as its sine/cosine; the identity gate
    // rejects a coincidental offset that does not hold a real (sin, cos) pair.
    if !sin_half.is_finite()
        || !cos_half.is_finite()
        || sin_half == 0.0
        || cos_half == 0.0
        || (sin_half * sin_half + cos_half * cos_half - 1.0).abs() > 1.0e-6
    {
        return None;
    }
    Some(SurfaceGeometry::Cone {
        origin: mm_point(origin),
        axis: vec3(axis),
        ref_direction: vec3(x_axis),
        radius: radius * 1000.0,
        ratio: 1.0,
        half_angle: sin_half.abs().atan2(cos_half.abs()),
    })
}

fn sphere(s: &[u8], b: usize, context: DecodeContext) -> Option<SurfaceGeometry> {
    let center = read_vec3(s, b + 19)?;
    let radius = read_f64(s, b + 43)?;
    let axis = read_vec3(s, b + 51)?;
    let x_axis = read_vec3(s, b + 75)?;
    if !is_orthonormal_frame(axis, x_axis)
        || !valid_position(center, context)
        || !valid_radius(radius, context)
    {
        return None;
    }
    Some(SurfaceGeometry::Sphere {
        center: mm_point(center),
        axis: vec3(axis),
        ref_direction: vec3(x_axis),
        radius: radius * 1000.0,
    })
}

fn torus(s: &[u8], b: usize, context: DecodeContext) -> Option<SurfaceGeometry> {
    let center = read_vec3(s, b + 19)?;
    let axis = read_vec3(s, b + 43)?;
    let major = read_f64(s, b + 67)?;
    let minor = read_f64(s, b + 75)?;
    let x_axis = read_vec3(s, b + 83)?;
    // A horn torus (major == minor) is valid; both radii must be positive and
    // finite. A zero major radius is degenerate and rejected.
    if !is_orthonormal_frame(axis, x_axis)
        || !valid_position(center, context)
        || !valid_radius(major, context)
        || !valid_radius(minor, context)
    {
        return None;
    }
    Some(SurfaceGeometry::Torus {
        center: mm_point(center),
        axis: vec3(axis),
        ref_direction: vec3(x_axis),
        major_radius: major * 1000.0,
        minor_radius: minor * 1000.0,
    })
}

// --- Curve decoders ---

fn line(s: &[u8], b: usize, context: DecodeContext) -> Option<CurveGeometry> {
    let origin = read_vec3(s, b + 19)?;
    let direction = read_vec3(s, b + 43)?;
    if !is_unit(direction) || !valid_position(origin, context) {
        return None;
    }
    Some(CurveGeometry::Line {
        origin: mm_point(origin),
        direction: vec3(direction),
    })
}

fn circle(s: &[u8], b: usize, context: DecodeContext) -> Option<CurveGeometry> {
    let center = read_vec3(s, b + 19)?;
    let normal = read_vec3(s, b + 43)?;
    let x_axis = read_vec3(s, b + 67)?;
    let radius = read_f64(s, b + 91)?;
    if !is_orthonormal_frame(normal, x_axis)
        || !valid_position(center, context)
        || !valid_radius(radius, context)
    {
        return None;
    }
    Some(CurveGeometry::Circle {
        center: mm_point(center),
        axis: vec3(normal),
        ref_direction: vec3(x_axis),
        radius: radius * 1000.0,
    })
}

fn ellipse(s: &[u8], b: usize, context: DecodeContext) -> Option<CurveGeometry> {
    let center = read_vec3(s, b + 19)?;
    let normal = read_vec3(s, b + 43)?;
    let x_axis = read_vec3(s, b + 67)?;
    let major = read_f64(s, b + 91)?;
    let minor = read_f64(s, b + 99)?;
    if !is_orthonormal_frame(normal, x_axis) || !valid_position(center, context) {
        return None;
    }
    if !valid_radius(major, context) || !valid_radius(minor, context) || minor > major {
        return None;
    }
    Some(CurveGeometry::Ellipse {
        center: mm_point(center),
        axis: vec3(normal),
        major_direction: vec3(x_axis),
        major_radius: major * 1000.0,
        minor_radius: minor * 1000.0,
    })
}

// --- Primitives and gates ---

/// Return whether a finite vector has unit length within the decode tolerance.
fn is_unit(v: [f64; 3]) -> bool {
    if !v.iter().all(|c| c.is_finite()) {
        return false;
    }
    let n2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    (n2 - 1.0).abs() < 1.0e-6
}

/// Return whether two finite vectors form the serialized analytic normal/x-axis frame.
fn is_orthonormal_frame(axis: [f64; 3], x_axis: [f64; 3]) -> bool {
    is_unit(axis)
        && is_unit(x_axis)
        && (axis[0] * x_axis[0] + axis[1] * x_axis[1] + axis[2] * x_axis[2]).abs() < 1.0e-6
}

fn valid_position(v: [f64; 3], context: DecodeContext) -> bool {
    v.iter().all(|coordinate| {
        coordinate.is_finite()
            && (*coordinate * 1000.0).is_finite()
            && (matches!(context, DecodeContext::Graph) || coordinate.abs() < 1.0e3)
    })
}

fn valid_radius(radius: f64, context: DecodeContext) -> bool {
    radius.is_finite()
        && (radius * 1000.0).is_finite()
        && radius > 0.0
        && (matches!(context, DecodeContext::Graph) || (radius > 1.0e-9 && radius < 1.0e3))
}

fn valid_cone_radius(radius: f64, context: DecodeContext) -> bool {
    radius.is_finite()
        && (radius * 1000.0).is_finite()
        && radius >= 0.0
        && (matches!(context, DecodeContext::Graph) || radius <= 1.0e3)
}

fn mm_point(v: [f64; 3]) -> Point3 {
    Point3::new(v[0] * 1000.0, v[1] * 1000.0, v[2] * 1000.0)
}

fn vec3(v: [f64; 3]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}
