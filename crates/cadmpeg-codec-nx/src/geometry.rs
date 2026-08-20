// SPDX-License-Identifier: Apache-2.0
//! Decode point and analytic geometry records from Parasolid neutral-binary data.
//!
//! The scanners recognize complete fixed records for points; planes, cylinders,
//! cones, spheres, and tori; and lines, circles, and ellipses. They validate
//! record bounds, finite values, radii, and direction vectors before returning a
//! carrier.
//!
//! Parasolid stores these fields as big-endian metre values. Returned coordinates
//! and radii are in millimetres; unit vectors and curve parameters are unchanged.
//! Fixed-record framing resolves the optional envelope escape, every extended
//! XMT in the common header, and the record boundary before geometry validation.
//! Use [`crate::topology`] to resolve returned record offsets into topology.
#![deny(clippy::disallowed_methods)]

use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

use crate::framing::{
    fixed_len, fixed_record_boundary, fixed_record_candidates, skip_sequence_at, FixedRecordFrame,
};
use crate::vec3_at::vec3_be_at;

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
#[derive(Clone)]
enum AnalyticRecord {
    Point(DecodedPoint),
    Surface(DecodedSurface),
    Curve(DecodedCurve),
}

/// Decode validated point records in source order.
///
/// Positions are returned in millimetres. Malformed candidates are skipped.
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
        let Some(len) = fixed_len(kind) else {
            p += 1;
            continue;
        };
        if !is_analytic_kind(kind) {
            p += 1;
            continue;
        }
        let frames = fixed_record_candidates(stream, p, kind, len);
        let mut candidates = [None, None];
        for (slot, frame) in frames.iter().enumerate() {
            if let Some(frame) = frame {
                candidates[slot] = analytic_candidate(stream, p, kind, *frame);
            }
        }
        if let Some((record, end)) = select_analytic_candidate(stream, &candidates) {
            out.push(record);
            p = end;
        } else if let Some(end) = frames.iter().flatten().map(|frame| frame.end).max() {
            // A complete structural frame owns its bytes even when its analytic
            // payload fails validation. Do not rescan those bytes as another
            // carrier; an unresolved or ambiguous frame is skipped atomically.
            p = end;
        } else {
            p += 1;
        }
    }
    out
}

fn is_analytic_kind(kind: u8) -> bool {
    matches!(kind, 0x1d | 0x1e..=0x20 | 0x32..=0x36)
}

struct AnalyticCandidate {
    frame: FixedRecordFrame,
    record: AnalyticRecord,
}

fn analytic_candidate(
    stream: &[u8],
    pos: usize,
    kind: u8,
    frame: FixedRecordFrame,
) -> Option<AnalyticCandidate> {
    let record_bytes = stream.get(pos..frame.end)?;
    let record = match kind {
        0x1d => {
            let mut at = pos + 8 + frame.shift;
            skip_sequence_at(stream, &mut at, 4)?;
            let xyz = vec3_be_at(stream, at)?;
            xyz.iter()
                .all(|value| value.is_finite() && (*value * 1000.0).is_finite())
                .then_some(AnalyticRecord::Point(DecodedPoint {
                    pos,
                    position: mm_point(xyz),
                }))?
        }
        0x32..=0x36 => decode_surface_record(record_bytes, kind, frame.shift + frame.payload_shift)
            .map(|geometry| AnalyticRecord::Surface(DecodedSurface { pos, geometry }))?,
        0x1e..=0x20 => decode_curve_record(record_bytes, kind, frame.shift + frame.payload_shift)
            .map(|geometry| AnalyticRecord::Curve(DecodedCurve { pos, geometry }))?,
        _ => return None,
    };
    Some(AnalyticCandidate { frame, record })
}

fn select_analytic_candidate(
    stream: &[u8],
    candidates: &[Option<AnalyticCandidate>; 2],
) -> Option<(AnalyticRecord, usize)> {
    let mut valid = candidates.iter().flatten();
    let first = valid.next()?;
    let Some(second) = valid.next() else {
        return Some((first.record.clone(), first.frame.end));
    };
    if valid.next().is_some() {
        return None;
    }
    let first_boundary = fixed_record_boundary(stream, first.frame.end);
    let second_boundary = fixed_record_boundary(stream, second.frame.end);
    match (first_boundary, second_boundary) {
        (true, false) => Some((first.record.clone(), first.frame.end)),
        (false, true) => Some((second.record.clone(), second.frame.end)),
        _ => None,
    }
}

/// Decode a graph-owned analytic surface at its resolved payload shift.
pub(crate) fn decode_surface_record(
    record: &[u8],
    kind: u8,
    shift: usize,
) -> Option<SurfaceGeometry> {
    let b = shift;
    match kind {
        0x32 => plane(record, b),
        0x33 => cylinder(record, b),
        0x34 => cone(record, b),
        0x35 => sphere(record, b),
        0x36 => torus(record, b),
        _ => None,
    }
}

/// Decode a graph-owned analytic curve at its resolved payload shift.
pub(crate) fn decode_curve_record(record: &[u8], kind: u8, shift: usize) -> Option<CurveGeometry> {
    let b = shift;
    match kind {
        0x1e => line(record, b),
        0x1f => circle(record, b),
        0x20 => ellipse(record, b),
        _ => None,
    }
}

// --- Surface decoders (offsets from the common header, [§5.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/siemens_nx.md#51-ownership-graph) / [§6.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/siemens_nx.md#61-analytic-curves-and-surfaces)) ---

fn plane(s: &[u8], b: usize) -> Option<SurfaceGeometry> {
    let origin = vec3_be_at(s, b + 19)?;
    let normal = vec3_be_at(s, b + 43)?;
    let x_axis = vec3_be_at(s, b + 67)?;
    if !is_orthonormal_frame(normal, x_axis) || !valid_position(origin) {
        return None;
    }
    Some(SurfaceGeometry::Plane {
        origin: mm_point(origin),
        normal: vec3(normal),
        u_axis: vec3(x_axis),
    })
}

fn cylinder(s: &[u8], b: usize) -> Option<SurfaceGeometry> {
    let origin = vec3_be_at(s, b + 19)?;
    let axis = vec3_be_at(s, b + 43)?;
    let radius = View::f64_be_at(s, b + 67)?;
    let x_axis = vec3_be_at(s, b + 75)?;
    if !is_orthonormal_frame(axis, x_axis) || !valid_position(origin) || !valid_radius(radius) {
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
    let origin = vec3_be_at(s, b + 19)?;
    let axis = vec3_be_at(s, b + 43)?;
    let radius = View::f64_be_at(s, b + 67)?;
    let sin_half = View::f64_be_at(s, b + 75)?;
    let cos_half = View::f64_be_at(s, b + 83)?;
    let x_axis = vec3_be_at(s, b + 91)?;
    if !is_orthonormal_frame(axis, x_axis) || !valid_position(origin) || !valid_cone_radius(radius)
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

fn sphere(s: &[u8], b: usize) -> Option<SurfaceGeometry> {
    let center = vec3_be_at(s, b + 19)?;
    let radius = View::f64_be_at(s, b + 43)?;
    let axis = vec3_be_at(s, b + 51)?;
    let x_axis = vec3_be_at(s, b + 75)?;
    if !is_orthonormal_frame(axis, x_axis) || !valid_position(center) || !valid_radius(radius) {
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
    let center = vec3_be_at(s, b + 19)?;
    let axis = vec3_be_at(s, b + 43)?;
    let major = View::f64_be_at(s, b + 67)?;
    let minor = View::f64_be_at(s, b + 75)?;
    let x_axis = vec3_be_at(s, b + 83)?;
    // A horn torus (major == minor) is valid; both radii must be positive and
    // finite. A zero major radius is degenerate and rejected.
    if !is_orthonormal_frame(axis, x_axis)
        || !valid_position(center)
        || !valid_radius(major)
        || !valid_radius(minor)
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
    let origin = vec3_be_at(s, b + 19)?;
    let direction = vec3_be_at(s, b + 43)?;
    if !is_unit(direction) || !valid_position(origin) {
        return None;
    }
    Some(CurveGeometry::Line {
        origin: mm_point(origin),
        direction: vec3(direction),
    })
}

fn circle(s: &[u8], b: usize) -> Option<CurveGeometry> {
    let center = vec3_be_at(s, b + 19)?;
    let normal = vec3_be_at(s, b + 43)?;
    let x_axis = vec3_be_at(s, b + 67)?;
    let radius = View::f64_be_at(s, b + 91)?;
    if !is_orthonormal_frame(normal, x_axis) || !valid_position(center) || !valid_radius(radius) {
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
    let center = vec3_be_at(s, b + 19)?;
    let normal = vec3_be_at(s, b + 43)?;
    let x_axis = vec3_be_at(s, b + 67)?;
    let major = View::f64_be_at(s, b + 91)?;
    let minor = View::f64_be_at(s, b + 99)?;
    if !is_orthonormal_frame(normal, x_axis) || !valid_position(center) {
        return None;
    }
    if !valid_radius(major) || !valid_radius(minor) || minor > major {
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

fn valid_position(v: [f64; 3]) -> bool {
    v.iter()
        .all(|coordinate| coordinate.is_finite() && (*coordinate * 1000.0).is_finite())
}

fn valid_radius(radius: f64) -> bool {
    radius.is_finite() && (radius * 1000.0).is_finite() && radius > 0.0
}

fn valid_cone_radius(radius: f64) -> bool {
    radius.is_finite() && (radius * 1000.0).is_finite() && radius >= 0.0
}

fn mm_point(v: [f64; 3]) -> Point3 {
    Point3::new(v[0] * 1000.0, v[1] * 1000.0, v[2] * 1000.0)
}

fn vec3(v: [f64; 3]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}

#[cfg(test)]
mod tests;
