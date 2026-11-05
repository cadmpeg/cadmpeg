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
#![deny(clippy::disallowed_methods)]

use crate::framing::node_kind::NodeKind;
use cadmpeg_core::decode::View;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::Magnification;

use crate::framing::{
    fixed_record_boundary, fixed_record_candidates, skip_sequence_at, FixedRecordFrame,
};
use crate::vec3_at::vec3_be_at;
use cadmpeg_parasolid::geometry::Analytic;

/// The analytic surface type tags and their fixed record lengths ([spec §4.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/siemens_nx.md#41-fixed-record-families)).
enum AnalyticRecord {
    Point(FinitePoint3),
    Surface(SurfaceGeometry),
    Curve(CurveGeometry),
}

/// Decode validated point records in source order.
///
/// Positions are returned in millimetres. Malformed candidates are skipped.
pub(crate) fn points(stream: &[u8]) -> Vec<cadmpeg_ir::features::FinitePoint3> {
    analytic_records(stream, |record| match record {
        AnalyticRecord::Point(point) => Some(point),
        AnalyticRecord::Surface(_) | AnalyticRecord::Curve(_) => None,
    })
}

/// Decode validated analytic surface records in source order.
pub(crate) fn surfaces(stream: &[u8]) -> Vec<cadmpeg_ir::geometry::SurfaceGeometry> {
    analytic_records(stream, |record| match record {
        AnalyticRecord::Surface(surface) => Some(surface),
        AnalyticRecord::Point(_) | AnalyticRecord::Curve(_) => None,
    })
}

/// Decode validated analytic curve records in source order.
pub(crate) fn curves(stream: &[u8]) -> Vec<cadmpeg_ir::geometry::CurveGeometry> {
    analytic_records(stream, |record| match record {
        AnalyticRecord::Curve(curve) => Some(curve),
        AnalyticRecord::Point(_) | AnalyticRecord::Surface(_) => None,
    })
}

fn analytic_records<T>(
    stream: &[u8],
    mut project: impl FnMut(AnalyticRecord) -> Option<T>,
) -> Vec<T> {
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
        if !is_analytic_kind(kind) {
            p += 1;
            continue;
        }
        let frames = fixed_record_candidates(stream, p, kind);
        let mut candidates = [None, None];
        for (slot, frame) in frames.iter().enumerate() {
            if let Some(frame) = frame {
                candidates[slot] = analytic_candidate(stream, p, kind, *frame);
            }
        }
        if let Some((record, end)) = select_analytic_candidate(stream, candidates) {
            if let Some(record) = project(record) {
                out.push(record);
            }
            p = end;
        } else if let Some(end) = frames.iter().flatten().map(|frame| frame.end()).max() {
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
    let record_bytes = stream.get(pos..frame.end())?;
    let record = match kind {
        NodeKind::Point => {
            let mut at = pos + 8 + frame.shift();
            skip_sequence_at(stream, &mut at, 4)?;
            let xyz = vec3_be_at(stream, at)?;
            let position = mm_position(xyz)?;
            AnalyticRecord::Point(position)
        }
        NodeKind::Plane
        | NodeKind::Cylinder
        | NodeKind::Cone
        | NodeKind::Sphere
        | NodeKind::Torus => {
            decode_surface_record(record_bytes, kind, frame.shift() + frame.payload_shift())
                .map(AnalyticRecord::Surface)?
        }
        NodeKind::Line | NodeKind::Circle | NodeKind::Ellipse => {
            decode_curve_record(record_bytes, kind, frame.shift() + frame.payload_shift())
                .map(AnalyticRecord::Curve)?
        }
        _ => return None,
    };
    Some(AnalyticCandidate { frame, record })
}

fn select_analytic_candidate(
    stream: &[u8],
    candidates: [Option<AnalyticCandidate>; 2],
) -> Option<(AnalyticRecord, usize)> {
    match candidates {
        [Some(first), None] | [None, Some(first)] => Some((first.record, first.frame.end())),
        [Some(first), Some(second)] => {
            let first_boundary = fixed_record_boundary(stream, first.frame.end());
            let second_boundary = fixed_record_boundary(stream, second.frame.end());
            match (first_boundary, second_boundary) {
                (true, false) => Some((first.record, first.frame.end())),
                (false, true) => Some((second.record, second.frame.end())),
                _ => None,
            }
        }
        [None, None] => None,
    }
}

fn analytic(record: &[u8], kind: NodeKind, shift: usize) -> Option<Analytic> {
    let code = u16::from(kind.code());
    let count = cadmpeg_parasolid::geometry::value_count(code)?;
    let mut values = [0.0; 12];
    for (index, value) in values.iter_mut().take(count).enumerate() {
        *value = View::f64_be_at(record, shift + 19 + index * 8)?;
    }
    cadmpeg_parasolid::geometry::analytic(code, values.get(..count)?)
}

/// Decode a graph-owned analytic surface at its resolved payload shift.
pub(crate) fn decode_surface_record(
    record: &[u8],
    kind: NodeKind,
    shift: usize,
) -> Option<SurfaceGeometry> {
    match analytic(record, kind, shift)? {
        Analytic::Surface {
            geometry,
            negative_major: false,
        } => Some(geometry),
        Analytic::Surface { .. } | Analytic::Curve(_) => None,
    }
}

/// Decode a graph-owned analytic curve at its resolved payload shift.
pub(crate) fn decode_curve_record(
    record: &[u8],
    kind: NodeKind,
    shift: usize,
) -> Option<CurveGeometry> {
    match analytic(record, kind, shift)? {
        Analytic::Curve(geometry) => Some(geometry),
        Analytic::Surface { .. } => None,
    }
}

// --- Primitives and gates ---

const MILLIMETRES_PER_METRE: f64 = Magnification::MILLIMETERS_PER_METER.get();

/// Admit a metre position in millimetres. A scaled coordinate is finite only
/// when the metre coordinate is, so the one admission states both.
fn mm_position(v: [f64; 3]) -> Option<FinitePoint3> {
    FinitePoint3::new(Point3::new(
        v[0] * MILLIMETRES_PER_METRE,
        v[1] * MILLIMETRES_PER_METRE,
        v[2] * MILLIMETRES_PER_METRE,
    ))
}

#[cfg(test)]
mod tests;
