// SPDX-License-Identifier: Apache-2.0
//! Parasolid B-rep decode: compact analytic carriers, world points, and the
//! typed topology chain.
//!
//! The class-definition body of a Parasolid `partition`/`deltas` stream carries
//! two things this codec reads: **compact analytic carriers** (fixed-layout
//! surface/curve placement records, spec §8.1) and the **typed topology chain**
//! (`face → bridge → surface`, `loop → coedge → edge-use → curve`, `coedge →
//! vertex-use → world point`, spec §5). Carriers are located by their tag and a
//! `0x2b`/`0x2d` orientation-marker gate and keyed by their attribute id; the
//! topology records reference carriers and each other by attribute id resolved
//! within the record's site.
//!
//! Model-space lengths are metres and convert to millimetres (×1000) at the IR
//! boundary; directions, normals, axes, reference directions, and ratios are
//! dimensionless and never scaled (spec §12). What cannot be typed is preserved:
//! a face on an unrecognized surface keeps its topology with a
//! [`SurfaceGeometry::Unknown`] carrier linking to the record bytes, and the
//! omission is counted, never fabricated.

use std::collections::HashMap;

use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

mod entity;
mod spline;
mod topology;

/// Millimetres per Parasolid model-space length unit (metres), spec §12.
pub(crate) const LEN_TO_MM: f64 = 1000.0;

pub use self::graph::{decode, decode_bodies, Brep, Stats};
pub(crate) use self::spline::{patch_nurbs_curve, patch_nurbs_surface};

mod graph;

// ---- low-level readers -------------------------------------------------------

pub(crate) fn u16_be(bytes: &[u8], at: usize) -> Option<u16> {
    bytes
        .get(at..at + 2)
        .map(|s| u16::from_be_bytes([s[0], s[1]]))
}

pub(crate) fn u32_be(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

pub(crate) fn f64_be(bytes: &[u8], at: usize) -> Option<f64> {
    bytes
        .get(at..at + 8)
        .map(|s| f64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

/// Read `n` consecutive big-endian f64s starting at `at`.
pub(crate) fn f64_run(bytes: &[u8], at: usize, n: usize) -> Option<Vec<f64>> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(f64_be(bytes, at + i * 8)?);
    }
    Some(out)
}

fn scale_point(v: &[f64]) -> Point3 {
    Point3::new(v[0] * LEN_TO_MM, v[1] * LEN_TO_MM, v[2] * LEN_TO_MM)
}

fn norm3(v: &[f64]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn unit(v: &[f64]) -> Vector3 {
    let n = norm3(v);
    if n > f64::EPSILON {
        Vector3::new(v[0] / n, v[1] / n, v[2] / n)
    } else {
        Vector3::new(v[0], v[1], v[2])
    }
}

// ---- compact analytic carriers (spec §8.1) -----------------------------------

/// Analytic surface/curve tags and the count of trailing f64 values each holds.
///
/// The generic record is `00 TT [ff]? attr:u16 ordinal:u32 refs:u16[5]
/// marker:u8(0x2b|0x2d) values:f64[n]` (spec §8.1). Offsets below are measured
/// from the tag byte; the optional `0xff` shifts everything after it by one.
pub(crate) mod tag {
    pub const LINE: u8 = 0x1e;
    pub const CIRCLE: u8 = 0x1f;
    pub const ELLIPSE: u8 = 0x20;
    pub const PLANE: u8 = 0x32;
    pub const CYLINDER: u8 = 0x33;
    pub const CONE: u8 = 0x34;
    pub const SPHERE: u8 = 0x35;
    pub const TORUS: u8 = 0x36;
}

/// f64 count for each analytic tag; `None` if the tag is not an analytic carrier.
fn analytic_value_count(tt: u8) -> Option<usize> {
    Some(match tt {
        tag::LINE => 6,
        tag::CIRCLE => 10,
        tag::ELLIPSE => 11,
        tag::PLANE => 9,
        tag::CYLINDER => 10,
        tag::CONE => 12,
        tag::SPHERE => 10,
        tag::TORUS => 11,
        _ => return None,
    })
}

/// A parsed compact analytic carrier: its attribute id, byte extent, and decoded
/// geometry (either a surface or a curve).
#[derive(Debug, Clone)]
pub(crate) struct Carrier {
    pub attr: u16,
    pub offset: usize,
    pub end: usize,
    pub geometry: CarrierGeometry,
    /// True for the cone/torus layouts confirmed from a single field-order
    /// sample (spec §8.1 confidence caveat).
    pub single_sample: bool,
    pub frame: Option<(Point3, Vector3, Vector3)>,
}

#[derive(Debug, Clone)]
pub(crate) enum CarrierGeometry {
    Surface(SurfaceGeometry),
    Curve(CurveGeometry),
}

/// Try to parse a compact analytic carrier whose tag byte pair `00 TT` begins at
/// `off`. Validated by the `0x2b`/`0x2d` marker gate and by a complete f64 run;
/// returns `None` when the candidate does not frame as a carrier.
pub(crate) fn parse_carrier(body: &[u8], off: usize) -> Option<Carrier> {
    if body.get(off) != Some(&0x00) {
        return None;
    }
    let tt = *body.get(off + 1)?;
    let n = analytic_value_count(tt)?;

    // The optional 0xff after the tag shifts the fixed header by one byte.
    let has_ff = body.get(off + 2) == Some(&0xff);
    let hdr = off + 2 + usize::from(has_ff);
    let attr = u16_be(body, hdr)?;
    // Partition records use a fixed header. Deltas records insert tripled refs;
    // in that form the orientation marker is the first 2b/2d after a run whose
    // third byte is 01.
    let fixed_marker = body
        .get(hdr + 16)
        .copied()
        .filter(|marker| matches!(marker, 0x2b | 0x2d));
    let marker_at = if fixed_marker.is_some() {
        hdr + 16
    } else {
        (hdr + 8..(hdr + 64).min(body.len())).find(|at| {
            matches!(body.get(*at), Some(0x2b | 0x2d)) && body.get(at.saturating_sub(1)) == Some(&1)
        })?
    };
    let values_at = marker_at + 1;
    let vals = f64_run(body, values_at, n)?;
    // A misaligned candidate reads raw bytes as f64s; real metre-scale coords and
    // dimensionless components sit well under 1e6, so anything past that (or
    // non-finite) means this is not actually a carrier here.
    if vals.iter().any(|v| !v.is_finite() || v.abs() > 1e6) {
        return None;
    }
    let end = values_at + n * 8;

    let (geometry, single_sample) = decode_carrier_values(tt, &vals)?;
    let frame = surface_frame(tt, &vals);
    Some(Carrier {
        attr,
        offset: off,
        end,
        geometry,
        single_sample,
        frame,
    })
}

fn cross(a: Vector3, b: Vector3) -> Vector3 {
    unit(&[
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    ])
}

fn surface_frame(tag: u8, v: &[f64]) -> Option<(Point3, Vector3, Vector3)> {
    let (origin, axis, reference) = match tag {
        tag::PLANE => (scale_point(&v[0..3]), unit(&v[3..6]), unit(&v[6..9])),
        tag::CYLINDER => (scale_point(&v[0..3]), unit(&v[3..6]), unit(&v[7..10])),
        tag::CONE => (scale_point(&v[0..3]), unit(&v[3..6]), unit(&v[9..12])),
        tag::SPHERE => (scale_point(&v[0..3]), unit(&v[4..7]), unit(&v[7..10])),
        tag::TORUS => (scale_point(&v[0..3]), unit(&v[3..6]), unit(&v[8..11])),
        _ => return None,
    };
    Some((
        origin,
        reference,
        if tag == tag::PLANE {
            cross(axis, reference)
        } else {
            axis
        },
    ))
}

/// Map a tag's decoded f64 run to IR geometry, applying the ×1000 length rule to
/// coordinates and radii only. Returns the geometry and the single-sample flag.
fn decode_carrier_values(tt: u8, v: &[f64]) -> Option<(CarrierGeometry, bool)> {
    let g = match tt {
        tag::LINE => CarrierGeometry::Curve(CurveGeometry::Line {
            origin: scale_point(&v[0..3]),
            direction: unit(&v[3..6]),
        }),
        tag::CIRCLE => CarrierGeometry::Curve(CurveGeometry::Circle {
            center: scale_point(&v[0..3]),
            axis: unit(&v[3..6]),
            ref_direction: unit(&v[6..9]),
            radius: v[9] * LEN_TO_MM,
        }),
        tag::ELLIPSE => CarrierGeometry::Curve(CurveGeometry::Ellipse {
            center: scale_point(&v[0..3]),
            axis: unit(&v[3..6]),
            major_direction: unit(&v[6..9]),
            major_radius: v[9] * LEN_TO_MM,
            minor_radius: v[10] * LEN_TO_MM,
        }),
        tag::PLANE => CarrierGeometry::Surface(SurfaceGeometry::Plane {
            origin: scale_point(&v[0..3]),
            normal: unit(&v[3..6]),
            u_axis: unit(&v[6..9]),
        }),
        tag::CYLINDER => CarrierGeometry::Surface(SurfaceGeometry::Cylinder {
            origin: scale_point(&v[0..3]),
            axis: unit(&v[3..6]),
            ref_direction: unit(&v[7..10]),
            radius: v[6] * LEN_TO_MM,
        }),
        tag::CONE => {
            // origin(3) axis(3) radius sin cos refdir(3): half-angle from the
            // stored sine, which satisfies sin^2+cos^2=1 in the observed sample.
            let sin = v[7];
            return Some((
                CarrierGeometry::Surface(SurfaceGeometry::Cone {
                    origin: scale_point(&v[0..3]),
                    axis: unit(&v[3..6]),
                    ref_direction: unit(&v[9..12]),
                    radius: v[6] * LEN_TO_MM,
                    half_angle: sin.abs().clamp(0.0, 1.0).asin(),
                }),
                true,
            ));
        }
        tag::SPHERE => CarrierGeometry::Surface(SurfaceGeometry::Sphere {
            center: scale_point(&v[0..3]),
            axis: unit(&v[4..7]),
            ref_direction: unit(&v[7..10]),
            radius: v[3] * LEN_TO_MM,
        }),
        tag::TORUS => {
            return Some((
                CarrierGeometry::Surface(SurfaceGeometry::Torus {
                    center: scale_point(&v[0..3]),
                    axis: unit(&v[3..6]),
                    ref_direction: unit(&v[8..11]),
                    major_radius: v[6] * LEN_TO_MM,
                    minor_radius: v[7] * LEN_TO_MM,
                }),
                true,
            ));
        }
        _ => return None,
    };
    Some((g, false))
}

/// Scan the whole stream body for compact analytic carriers, keyed by attribute
/// id. When two carriers share an attr (a partition base and a deltas variant),
/// the first (partition-order) wins, matching the "weak deltas must not
/// overwrite a stronger partition record" rule (spec §4.2).
pub(crate) fn scan_carriers(body: &[u8]) -> HashMap<u16, Carrier> {
    let mut out: HashMap<u16, Carrier> = HashMap::new();
    let mut i = 0usize;
    while i + 2 <= body.len() {
        if body[i] == 0x00 {
            if let Some(c) = parse_carrier(body, i) {
                out.insert(c.attr, c.clone());
                i = c.end;
                continue;
            }
        }
        i += 1;
    }
    for (attr, carrier) in spline::scan_curve_carriers(body) {
        out.insert(attr, carrier);
    }
    for (attr, carrier) in spline::scan_surface_carriers(body) {
        out.insert(attr, carrier);
    }
    out
}
