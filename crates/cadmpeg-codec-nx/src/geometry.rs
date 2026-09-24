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

use crate::framing::node_kind::NodeKind;
use cadmpeg_core::decode::View;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, EllipseCurve, LineCurve, PlaneSurface,
    SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{
    Angle, FiniteReal, Magnification, NonNegativeLength, PositiveLength, PositiveReal,
};
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};

use crate::framing::{
    fixed_len, fixed_record_boundary, fixed_record_candidates, skip_sequence_at, FixedRecordFrame,
};
use crate::vec3_at::vec3_be_at;

const EPS_GEOMETRY_CONE_E6: f64 = 1.0e-6;

/// A decoded analytic surface and its source offset.
#[derive(Debug, Clone)]
pub(crate) struct DecodedSurface {
    /// Byte offset of the record's type tag within the stream.
    pub(crate) pos: usize,
    /// The decoded surface geometry.
    pub(crate) geometry: SurfaceGeometry,
}

/// A decoded analytic curve and its source offset.
#[derive(Debug, Clone)]
pub(crate) struct DecodedCurve {
    /// Byte offset of the record's type tag within the stream.
    pub(crate) pos: usize,
    /// The decoded curve geometry.
    pub(crate) geometry: CurveGeometry,
}

/// A decoded point and its source offset.
#[derive(Debug, Clone)]
pub(crate) struct DecodedPoint {
    /// Byte offset of the record's `00 1d` tag within the stream.
    pub(crate) pos: usize,
    /// Position in millimetres.
    pub(crate) position: FinitePoint3,
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
pub(crate) fn points(stream: &[u8]) -> Vec<DecodedPoint> {
    analytic_records(stream)
        .into_iter()
        .filter_map(|record| match record {
            AnalyticRecord::Point(point) => Some(point),
            AnalyticRecord::Surface(_) | AnalyticRecord::Curve(_) => None,
        })
        .collect()
}

/// Decode validated analytic surface records in source order.
pub(crate) fn surfaces(stream: &[u8]) -> Vec<DecodedSurface> {
    analytic_records(stream)
        .into_iter()
        .filter_map(|record| match record {
            AnalyticRecord::Surface(surface) => Some(surface),
            AnalyticRecord::Point(_) | AnalyticRecord::Curve(_) => None,
        })
        .collect()
}

/// Decode validated analytic curve records in source order.
pub(crate) fn curves(stream: &[u8]) -> Vec<DecodedCurve> {
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
        let Ok(kind) = NodeKind::try_from(stream[p + 1]) else {
            p += 1;
            continue;
        };
        let len = fixed_len(kind);
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

fn is_analytic_kind(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Point
            | NodeKind::Line
            | NodeKind::Circle
            | NodeKind::Ellipse
            | NodeKind::Plane
            | NodeKind::Cylinder
            | NodeKind::Cone
            | NodeKind::Sphere
            | NodeKind::Torus
    )
}

struct AnalyticCandidate {
    frame: FixedRecordFrame,
    record: AnalyticRecord,
}

fn analytic_candidate(
    stream: &[u8],
    pos: usize,
    kind: NodeKind,
    frame: FixedRecordFrame,
) -> Option<AnalyticCandidate> {
    let record_bytes = stream.get(pos..frame.end)?;
    let record = match kind {
        NodeKind::Point => {
            let mut at = pos + 8 + frame.shift;
            skip_sequence_at(stream, &mut at, 4)?;
            let xyz = vec3_be_at(stream, at)?;
            let position = mm_position(xyz)?;
            AnalyticRecord::Point(DecodedPoint { pos, position })
        }
        NodeKind::Plane
        | NodeKind::Cylinder
        | NodeKind::Cone
        | NodeKind::Sphere
        | NodeKind::Torus => {
            decode_surface_record(record_bytes, kind, frame.shift + frame.payload_shift)
                .map(|geometry| AnalyticRecord::Surface(DecodedSurface { pos, geometry }))?
        }
        NodeKind::Line | NodeKind::Circle | NodeKind::Ellipse => {
            decode_curve_record(record_bytes, kind, frame.shift + frame.payload_shift)
                .map(|geometry| AnalyticRecord::Curve(DecodedCurve { pos, geometry }))?
        }
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
    kind: NodeKind,
    shift: usize,
) -> Option<SurfaceGeometry> {
    let b = shift;
    match kind {
        NodeKind::Plane => plane(record, b),
        NodeKind::Cylinder => cylinder(record, b),
        NodeKind::Cone => cone(record, b),
        NodeKind::Sphere => sphere(record, b),
        NodeKind::Torus => torus(record, b),
        _ => None,
    }
}

/// Decode a graph-owned analytic curve at its resolved payload shift.
pub(crate) fn decode_curve_record(
    record: &[u8],
    kind: NodeKind,
    shift: usize,
) -> Option<CurveGeometry> {
    let b = shift;
    match kind {
        NodeKind::Line => line(record, b),
        NodeKind::Circle => circle(record, b),
        NodeKind::Ellipse => ellipse(record, b),
        _ => None,
    }
}

// --- Surface decoders (offsets from the common header, [§5.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/siemens_nx.md#51-ownership-graph) / [§6.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/siemens_nx.md#61-analytic-curves-and-surfaces)) ---

fn plane(s: &[u8], b: usize) -> Option<SurfaceGeometry> {
    let origin = vec3_be_at(s, b + 19)?;
    let normal = vec3_be_at(s, b + 43)?;
    let x_axis = vec3_be_at(s, b + 67)?;
    let frame = frame(normal, x_axis)?;
    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        PlaneSurface::new(mm_position(origin)?, frame),
    )))
}

fn cylinder(s: &[u8], b: usize) -> Option<SurfaceGeometry> {
    let origin = vec3_be_at(s, b + 19)?;
    let axis = vec3_be_at(s, b + 43)?;
    let radius = View::f64_be_at(s, b + 67)?;
    let x_axis = vec3_be_at(s, b + 75)?;
    let frame = frame(axis, x_axis)?;
    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::new(mm_position(origin)?, frame, mm_radius(radius)?),
    )))
}

fn cone(s: &[u8], b: usize) -> Option<SurfaceGeometry> {
    let origin = vec3_be_at(s, b + 19)?;
    let axis = vec3_be_at(s, b + 43)?;
    let radius = View::f64_be_at(s, b + 67)?;
    let sin_half = View::f64_be_at(s, b + 75)?;
    let cos_half = View::f64_be_at(s, b + 83)?;
    let x_axis = vec3_be_at(s, b + 91)?;
    let frame = frame(axis, x_axis)?;
    let origin = mm_position(origin)?;
    let radius = NonNegativeLength::new(radius * MILLIMETRES_PER_METRE)?;
    // The cone's half-angle is carried as its sine/cosine; the identity gate
    // rejects a coincidental offset that does not hold a real (sin, cos) pair.
    let (Some(sin_half), Some(cos_half)) = (FiniteReal::new(sin_half), FiniteReal::new(cos_half))
    else {
        return None;
    };
    let (sine, cosine) = (sin_half.get(), cos_half.get());
    if sine == 0.0
        || cosine == 0.0
        || (sine * sine + cosine * cosine - 1.0).abs() > EPS_GEOMETRY_CONE_E6
    {
        return None;
    }
    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        ConeSurface::new(
            origin,
            frame,
            radius,
            PositiveReal::ONE,
            Angle::from_assigned_real(sin_half.abs().atan2(cos_half.abs())),
        ),
    )))
}

fn sphere(s: &[u8], b: usize) -> Option<SurfaceGeometry> {
    let center = vec3_be_at(s, b + 19)?;
    let radius = View::f64_be_at(s, b + 43)?;
    let axis = vec3_be_at(s, b + 51)?;
    let x_axis = vec3_be_at(s, b + 75)?;
    let frame = frame(axis, x_axis)?;
    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        SphereSurface::new(mm_position(center)?, frame, mm_radius(radius)?.into()),
    )))
}

fn torus(s: &[u8], b: usize) -> Option<SurfaceGeometry> {
    let center = vec3_be_at(s, b + 19)?;
    let axis = vec3_be_at(s, b + 43)?;
    let major = View::f64_be_at(s, b + 67)?;
    let minor = View::f64_be_at(s, b + 75)?;
    let x_axis = vec3_be_at(s, b + 83)?;
    // A horn torus (major == minor) is valid; both radii must be positive and
    // finite. A zero major radius is degenerate and rejected.
    let frame = frame(axis, x_axis)?;
    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        TorusSurface::new(
            mm_position(center)?,
            frame,
            mm_radius(major)?,
            mm_radius(minor)?.into(),
        ),
    )))
}

// --- Curve decoders ---

fn line(s: &[u8], b: usize) -> Option<CurveGeometry> {
    let origin = vec3_be_at(s, b + 19)?;
    let direction = vec3_be_at(s, b + 43)?;
    let direction = UnitVector3::new(vec3(direction))?;
    Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::new(mm_position(origin)?, direction),
    )))
}

fn circle(s: &[u8], b: usize) -> Option<CurveGeometry> {
    let center = vec3_be_at(s, b + 19)?;
    let normal = vec3_be_at(s, b + 43)?;
    let x_axis = vec3_be_at(s, b + 67)?;
    let radius = View::f64_be_at(s, b + 91)?;
    let frame = frame(normal, x_axis)?;
    Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        CircleCurve::new(mm_position(center)?, frame, mm_radius(radius)?),
    )))
}

fn ellipse(s: &[u8], b: usize) -> Option<CurveGeometry> {
    let center = vec3_be_at(s, b + 19)?;
    let normal = vec3_be_at(s, b + 43)?;
    let x_axis = vec3_be_at(s, b + 67)?;
    let major = View::f64_be_at(s, b + 91)?;
    let minor = View::f64_be_at(s, b + 99)?;
    let frame = frame(normal, x_axis)?;
    let center = mm_position(center)?;
    // The order is read from the metre radii. Scaling can round a minor
    // radius above the major one onto it, which the millimetre order admits.
    Some(CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
        EllipseCurve::from_source_radii(
            center,
            frame,
            [major, minor],
            PositiveReal::from(Magnification::MILLIMETERS_PER_METER),
        )?,
    )))
}

// --- Primitives and gates ---

const MILLIMETRES_PER_METRE: f64 = Magnification::MILLIMETERS_PER_METER.get();

/// Admit the serialized analytic axis/reference frame. The admission refuses
/// every non-finite component, since a non-finite component has no norm
/// within the unit tolerance.
fn frame(axis: [f64; 3], reference: [f64; 3]) -> Option<OrthonormalFrame3> {
    OrthonormalFrame3::new(vec3(axis), vec3(reference))
}

/// Admit a metre position in millimetres. A scaled coordinate is finite only
/// when the metre coordinate is, so the one admission states both.
fn mm_position(v: [f64; 3]) -> Option<FinitePoint3> {
    FinitePoint3::new(Point3::new(
        v[0] * MILLIMETRES_PER_METRE,
        v[1] * MILLIMETRES_PER_METRE,
        v[2] * MILLIMETRES_PER_METRE,
    ))
}

/// Admit a positive metre radius in millimetres. Scaling a finite value by a
/// thousand keeps its sign and cannot reach zero, so the one admission states
/// the metre and millimetre conditions.
fn mm_radius(radius: f64) -> Option<PositiveLength> {
    PositiveLength::new(radius * MILLIMETRES_PER_METRE)
}

fn vec3(v: [f64; 3]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}

#[cfg(test)]
mod tests;
