// SPDX-License-Identifier: Apache-2.0
//! Decode point and analytic geometry records from Parasolid neutral-binary data.
//!
//! The scanners recognize points; planes, cylinders, cones, spheres, and tori;
//! and lines, circles, and ellipses. They validate record bounds, finite values,
//! model scale, radii, and direction vectors before returning a carrier.
//!
//! Parasolid stores these fields as big-endian metre values. Returned coordinates
//! and radii are in millimetres; unit vectors and curve parameters are unchanged.
//! The scanners test supported field shifts caused by extended references and
//! omit candidates that fail validation. Use [`crate::topology`] to resolve
//! returned record offsets into topology.
#![deny(clippy::disallowed_methods)]

use cadmpeg_ir::be::{f64_at as read_f64, vec3_at as read_vec3};
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

/// Candidate byte shifts applied to a record's fixed payload offsets, covering a
/// record whose leading pointer slots each consumed the 4-byte large-index form.
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

/// Decode validated point records in source order.
///
/// Positions are returned in millimetres. Malformed and out-of-range candidates
/// are skipped.
pub fn points(stream: &[u8]) -> Vec<DecodedPoint> {
    let mut out = Vec::new();
    let mut occupied_end = 0usize;
    let mut p = 0usize;
    while p + 2 <= stream.len() {
        if stream[p] == 0x00 && stream[p + 1] == 0x1d && p >= occupied_end {
            if let Some((xyz, shift)) = SHIFTS.into_iter().find_map(|shift| {
                read_vec3(stream, p + 16 + shift).and_then(|xyz| {
                    (xyz.iter().all(|v| {
                        v.is_finite() && v.abs() < 1.0e3 && (*v == 0.0 || v.abs() >= 1.0e-100)
                    }) && xyz.iter().any(|v| *v != 0.0))
                    .then_some((xyz, shift))
                })
            }) {
                out.push(DecodedPoint {
                    pos: p,
                    position: mm_point(xyz),
                });
                occupied_end = p + 40 + shift; // POINT record length plus XMT expansion
                p += 2;
                continue;
            }
        }
        p += 1;
    }
    out
}

/// Decode validated analytic surface records in source order.
pub fn surfaces(stream: &[u8]) -> Vec<DecodedSurface> {
    let mut out = Vec::new();
    let mut occupied_end = 0usize;
    let mut p = 0usize;
    while p + 2 <= stream.len() {
        if stream[p] == 0x00 && p >= occupied_end {
            if let Some((_, len)) = SURFACE_TAGS.iter().find(|(t, _)| *t == stream[p + 1]) {
                if let Some(geom) = decode_surface(stream, p, stream[p + 1]) {
                    out.push(DecodedSurface {
                        pos: p,
                        geometry: geom,
                    });
                    occupied_end = p + *len;
                    p += 2;
                    continue;
                }
            }
        }
        p += 1;
    }
    out
}

/// Decode validated analytic curve records in source order.
pub fn curves(stream: &[u8]) -> Vec<DecodedCurve> {
    let mut out = Vec::new();
    let mut occupied_end = 0usize;
    let mut p = 0usize;
    while p + 2 <= stream.len() {
        if stream[p] == 0x00 && p >= occupied_end {
            if let Some((_, len)) = CURVE_TAGS.iter().find(|(t, _)| *t == stream[p + 1]) {
                if let Some(geom) = decode_curve(stream, p, stream[p + 1]) {
                    out.push(DecodedCurve {
                        pos: p,
                        geometry: geom,
                    });
                    occupied_end = p + *len;
                    p += 2;
                    continue;
                }
            }
        }
        p += 1;
    }
    out
}

/// Decode an analytic surface at tag position `p`, trying each candidate shift and
/// returning the first whose payload passes the kind's validation gate.
fn decode_surface(stream: &[u8], p: usize, kind: u8) -> Option<SurfaceGeometry> {
    for sh in SHIFTS {
        let b = p + sh;
        let geom = match kind {
            0x32 => plane(stream, b),
            0x33 => cylinder(stream, b),
            0x34 => cone(stream, b),
            0x35 => sphere(stream, b),
            0x36 => torus(stream, b),
            _ => None,
        };
        if geom.is_some() {
            return geom;
        }
    }
    None
}

/// Decode an analytic curve at tag position `p`, trying each candidate shift.
fn decode_curve(stream: &[u8], p: usize, kind: u8) -> Option<CurveGeometry> {
    for sh in SHIFTS {
        let b = p + sh;
        let geom = match kind {
            0x1e => line(stream, b),
            0x1f => circle(stream, b),
            0x20 => ellipse(stream, b),
            _ => None,
        };
        if geom.is_some() {
            return geom;
        }
    }
    None
}

// --- Surface decoders (offsets from the common header, [§5.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/siemens_nx.md#51-ownership-graph) / [§6.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/siemens_nx.md#61-analytic-curves-and-surfaces)) ---

fn plane(s: &[u8], b: usize) -> Option<SurfaceGeometry> {
    let origin = read_vec3(s, b + 19)?;
    let normal = read_vec3(s, b + 43)?;
    let x_axis = read_vec3(s, b + 67)?;
    if !is_unit(normal) || !is_unit(x_axis) || !model_scale(origin) {
        return None;
    }
    Some(SurfaceGeometry::Plane {
        origin: mm_point(origin),
        normal: vec3(normal),
        u_axis: vec3(x_axis),
    })
}

fn cylinder(s: &[u8], b: usize) -> Option<SurfaceGeometry> {
    let origin = read_vec3(s, b + 19)?;
    let axis = read_vec3(s, b + 43)?;
    let radius = read_f64(s, b + 67)?;
    let x_axis = read_vec3(s, b + 75)?;
    if !is_unit(axis) || !is_unit(x_axis) || !model_scale(origin) || !in_radius(radius) {
        return None;
    }
    Some(SurfaceGeometry::Cylinder {
        origin: mm_point(origin),
        axis: vec3(axis),
        ref_direction: vec3(x_axis),
        radius: radius * 1000.0,
    })
}

fn cone(s: &[u8], b: usize) -> Option<SurfaceGeometry> {
    let origin = read_vec3(s, b + 19)?;
    let axis = read_vec3(s, b + 43)?;
    let radius = read_f64(s, b + 67)?;
    let sin_half = read_f64(s, b + 75)?;
    let cos_half = read_f64(s, b + 83)?;
    let x_axis = read_vec3(s, b + 91)?;
    if !is_unit(axis)
        || !is_unit(x_axis)
        || !model_scale(origin)
        || !(0.0..=1.0e3).contains(&radius)
    {
        return None;
    }
    // The cone's half-angle is carried as its sine/cosine; the identity gate
    // rejects a coincidental offset that does not hold a real (sin, cos) pair.
    if (sin_half * sin_half + cos_half * cos_half - 1.0).abs() > 1.0e-6 {
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

fn sphere(s: &[u8], b: usize) -> Option<SurfaceGeometry> {
    let center = read_vec3(s, b + 19)?;
    let radius = read_f64(s, b + 43)?;
    let axis = read_vec3(s, b + 51)?;
    let x_axis = read_vec3(s, b + 75)?;
    if !is_unit(axis) || !is_unit(x_axis) || !model_scale(center) || !in_radius(radius) {
        return None;
    }
    Some(SurfaceGeometry::Sphere {
        center: mm_point(center),
        axis: vec3(axis),
        ref_direction: vec3(x_axis),
        radius: radius * 1000.0,
    })
}

fn torus(s: &[u8], b: usize) -> Option<SurfaceGeometry> {
    let center = read_vec3(s, b + 19)?;
    let axis = read_vec3(s, b + 43)?;
    let major = read_f64(s, b + 67)?;
    let minor = read_f64(s, b + 75)?;
    let x_axis = read_vec3(s, b + 83)?;
    // A horn torus (major == minor) is valid; both radii must be positive and in
    // range. `major` may be zero only for degenerate records, which are rejected.
    if !is_unit(axis)
        || !is_unit(x_axis)
        || !model_scale(center)
        || !in_radius(major)
        || !in_radius(minor)
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

fn line(s: &[u8], b: usize) -> Option<CurveGeometry> {
    let origin = read_vec3(s, b + 19)?;
    let direction = read_vec3(s, b + 43)?;
    if !is_unit(direction) || !model_scale(origin) {
        return None;
    }
    Some(CurveGeometry::Line {
        origin: mm_point(origin),
        direction: vec3(direction),
    })
}

fn circle(s: &[u8], b: usize) -> Option<CurveGeometry> {
    let center = read_vec3(s, b + 19)?;
    let normal = read_vec3(s, b + 43)?;
    let x_axis = read_vec3(s, b + 67)?;
    let radius = read_f64(s, b + 91)?;
    if !is_unit(normal) || !is_unit(x_axis) || !model_scale(center) || !in_radius(radius) {
        return None;
    }
    Some(CurveGeometry::Circle {
        center: mm_point(center),
        axis: vec3(normal),
        ref_direction: vec3(x_axis),
        radius: radius * 1000.0,
    })
}

fn ellipse(s: &[u8], b: usize) -> Option<CurveGeometry> {
    let center = read_vec3(s, b + 19)?;
    let normal = read_vec3(s, b + 43)?;
    let x_axis = read_vec3(s, b + 67)?;
    let major = read_f64(s, b + 91)?;
    let minor = read_f64(s, b + 99)?;
    if !is_unit(normal) || !is_unit(x_axis) || !model_scale(center) {
        return None;
    }
    if !in_radius(major) || !in_radius(minor) || minor > major + 1.0e-9 {
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

/// An origin/centre is model-scale when finite and under one kilometre in metres.
fn model_scale(v: [f64; 3]) -> bool {
    v.iter().all(|c| c.is_finite() && c.abs() < 1.0e3)
}

/// Return whether a metre radius is finite and within the accepted model range.
fn in_radius(r: f64) -> bool {
    r.is_finite() && r > 1.0e-9 && r < 1.0e3
}

fn mm_point(v: [f64; 3]) -> Point3 {
    Point3::new(v[0] * 1000.0, v[1] * 1000.0, v[2] * 1000.0)
}

fn vec3(v: [f64; 3]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}
