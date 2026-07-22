// SPDX-License-Identifier: Apache-2.0
//! Parasolid B-rep record decoding.
//!
//! This module resolves topology and geometry carriers by stream-local
//! attribute id. [`decode`] handles one stream; [`decode_bodies`] combines
//! related partition and deltas streams before building the graph.
//!
//! The decoded chain connects face bridges to support surfaces and loop heads,
//! coedges to edge uses and curves, and vertex uses to world points. Supported
//! carriers include lines, circles, ellipses, planes, cylinders, cones, spheres,
//! tori, and NURBS curves and surfaces. The decoder converts model-space metres
//! to millimetres and leaves dimensionless vectors and ratios unchanged.
//!
//! [`Brep::stats`] counts carriers and grouping that could not be transferred
//! directly. Untyped carriers use opaque IR geometry while resolvable topology
//! remains available.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::be::{f64_at as f64_be, f64s_at as f64_run, u16_at as u16_be, u32_at as u32_be};
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

mod entity;
mod intersection;
mod spline;
mod subset;
mod sweep;
mod topology;

/// Millimetres per Parasolid model-space length unit (metres), [spec §12](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#9-units).
pub(crate) const LEN_TO_MM: f64 = 1000.0;

pub use self::graph::{decode, decode_bodies, Brep, Stats};
pub(crate) use self::spline::{infer_surface_shape, patch_nurbs_curve, patch_nurbs_surface};
pub(crate) use self::topology::patch_point;

mod graph;

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

// ---- compact analytic carriers ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#71-compact-analytic-records)) -----------------------------------

/// Analytic surface/curve tags and the count of trailing f64 values each holds.
///
/// The generic record is `00 TT [ff]? attr:u16 ordinal:u32 refs:u16[5]
/// marker:u8(0x2b|0x2d) values:f64[n]` ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#71-compact-analytic-records)). Offsets below are measured
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

fn unit_length(values: &[f64]) -> bool {
    (norm3(values) - 1.0).abs() <= 1.0e-9
}

fn orthonormal(left: &[f64], right: &[f64]) -> bool {
    unit_length(left)
        && unit_length(right)
        && (left[0] * right[0] + left[1] * right[1] + left[2] * right[2]).abs() <= 1.0e-9
}

fn valid_carrier_frame(tt: u8, values: &[f64]) -> bool {
    match tt {
        tag::LINE => unit_length(&values[3..6]),
        tag::CIRCLE | tag::ELLIPSE | tag::PLANE => orthonormal(&values[3..6], &values[6..9]),
        tag::CYLINDER => orthonormal(&values[3..6], &values[7..10]),
        tag::CONE => orthonormal(&values[3..6], &values[9..12]),
        tag::SPHERE => orthonormal(&values[4..7], &values[7..10]),
        tag::TORUS => orthonormal(&values[3..6], &values[8..11]),
        _ => false,
    }
}

fn valid_carrier_scalars(tt: u8, values: &[f64]) -> bool {
    match tt {
        tag::LINE | tag::PLANE => true,
        tag::CIRCLE => values[9] > 0.0,
        tag::ELLIPSE => values[9] >= values[10] && values[10] > 0.0,
        tag::CYLINDER => values[6] > 0.0,
        tag::CONE => {
            values[6] >= 0.0
                && values[7].abs() > f64::EPSILON
                && values[8] > 0.0
                && (values[7] * values[7] + values[8] * values[8] - 1.0).abs() <= 1.0e-9
        }
        tag::SPHERE => values[3] > 0.0,
        tag::TORUS => values[6] > 0.0 && values[7] > 0.0,
        _ => false,
    }
}

/// A parsed compact analytic carrier: its attribute id, byte extent, and decoded
/// geometry (either a surface or a curve).
#[derive(Debug, Clone)]
pub(crate) struct Carrier {
    pub attr: u16,
    pub offset: usize,
    pub end: usize,
    pub geometry: CarrierGeometry,
    pub frame: Option<(Point3, Vector3, Vector3)>,
}

#[derive(Debug, Clone)]
pub(crate) enum CarrierGeometry {
    Surface(SurfaceGeometry),
    Curve(CurveGeometry),
}

#[derive(Default)]
pub(crate) struct CarrierIndex {
    curves: HashMap<u16, Carrier>,
    surfaces: HashMap<u16, Carrier>,
    /// Swept/spun surface constructions, resolved to a patch at face binding.
    sweeps: HashMap<u16, sweep::SweepCarrier>,
    /// Curve attrs whose geometry is a derived cache, not an exact carrier.
    derived_curves: HashSet<u16>,
}

impl CarrierIndex {
    fn insert(&mut self, carrier: Carrier) {
        match carrier.geometry {
            CarrierGeometry::Curve(_) => {
                self.curves.insert(carrier.attr, carrier);
            }
            CarrierGeometry::Surface(_) => {
                self.surfaces.insert(carrier.attr, carrier);
            }
        }
    }

    pub(crate) fn curve(&self, attr: u16) -> Option<&Carrier> {
        self.curves.get(&attr)
    }

    pub(crate) fn surface(&self, attr: u16) -> Option<&Carrier> {
        self.surfaces.get(&attr)
    }

    /// Swept/spun surface construction carried by one attribute.
    pub(crate) fn sweep(&self, attr: u16) -> Option<&sweep::SweepCarrier> {
        self.sweeps.get(&attr)
    }

    /// Whether a curve attr holds a derived solved cache rather than an
    /// exact carrier.
    pub(crate) fn curve_is_derived(&self, attr: u16) -> bool {
        self.derived_curves.contains(&attr)
    }

    pub(crate) fn merge_missing(&mut self, other: Self) {
        for (attr, carrier) in other.curves {
            if let std::collections::hash_map::Entry::Vacant(entry) = self.curves.entry(attr) {
                if other.derived_curves.contains(&attr) {
                    self.derived_curves.insert(attr);
                }
                entry.insert(carrier);
            }
        }
        for (attr, carrier) in other.surfaces {
            self.surfaces.entry(attr).or_insert(carrier);
        }
        for (attr, carrier) in other.sweeps {
            self.sweeps.entry(attr).or_insert(carrier);
        }
    }
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
    if !valid_carrier_frame(tt, &vals) || !valid_carrier_scalars(tt, &vals) {
        return None;
    }
    let end = values_at + n * 8;

    let geometry = decode_carrier_values(tt, &vals)?;
    let frame = surface_frame(tt, &vals);
    Some(Carrier {
        attr,
        offset: off,
        end,
        geometry,
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
/// coordinates and radii only.
fn decode_carrier_values(tt: u8, v: &[f64]) -> Option<CarrierGeometry> {
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
            return Some(CarrierGeometry::Surface(SurfaceGeometry::Cone {
                origin: scale_point(&v[0..3]),
                axis: unit(&v[3..6]),
                ref_direction: unit(&v[9..12]),
                radius: v[6] * LEN_TO_MM,
                ratio: 1.0,
                half_angle: sin.abs().clamp(0.0, 1.0).asin(),
            }));
        }
        tag::SPHERE => CarrierGeometry::Surface(SurfaceGeometry::Sphere {
            center: scale_point(&v[0..3]),
            axis: unit(&v[4..7]),
            ref_direction: unit(&v[7..10]),
            radius: v[3] * LEN_TO_MM,
        }),
        tag::TORUS => {
            return Some(CarrierGeometry::Surface(SurfaceGeometry::Torus {
                center: scale_point(&v[0..3]),
                axis: unit(&v[3..6]),
                ref_direction: unit(&v[8..11]),
                major_radius: v[6] * LEN_TO_MM,
                minor_radius: v[7] * LEN_TO_MM,
            }));
        }
        _ => return None,
    };
    Some(g)
}

/// Scan the whole stream body for compact analytic carriers, keyed by attribute
/// id. When two carriers share an attr (a partition base and a deltas variant),
/// the first (partition-order) wins, matching the "weak deltas must not
/// overwrite a stronger partition record" rule ([spec §4.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#42-deltas-encodings)).
pub(crate) fn scan_carriers(body: &[u8]) -> CarrierIndex {
    let mut out = CarrierIndex::default();
    let mut i = 0usize;
    while i + 2 <= body.len() {
        if body[i] == 0x00 {
            if let Some(c) = parse_carrier(body, i) {
                out.insert(c);
            }
        }
        i += 1;
    }
    for (attr, carrier) in spline::scan_curve_carriers(body) {
        debug_assert_eq!(attr, carrier.attr);
        out.insert(carrier);
    }
    for (attr, carrier) in spline::scan_surface_carriers(body) {
        debug_assert_eq!(attr, carrier.attr);
        out.insert(carrier);
    }
    for carrier in subset::scan(body, &out) {
        out.insert(carrier);
    }
    out.sweeps = sweep::scan_sweep_carriers(body);
    for (attr, carrier) in intersection::scan_intersection_carriers(body) {
        debug_assert_eq!(attr, carrier.attr);
        if let std::collections::hash_map::Entry::Vacant(entry) = out.curves.entry(attr) {
            entry.insert(carrier);
            out.derived_curves.insert(attr);
        }
    }
    out
}

/// Return the typed curve carried by one stream-local attribute.
pub(crate) fn curve_by_attr(body: &[u8], attr: u16) -> Option<CurveGeometry> {
    match &scan_carriers(body).curve(attr)?.geometry {
        CarrierGeometry::Curve(curve) => Some(curve.clone()),
        CarrierGeometry::Surface(_) => unreachable!("curve index contains only curve carriers"),
    }
}

/// Replace the scalar run of one compact analytic carrier.
pub(crate) fn patch_compact_values(body: &mut [u8], attr: u16, values: &[f64]) -> bool {
    let carriers = scan_carriers(body);
    let Some(carrier) = carriers.curve(attr) else {
        return false;
    };
    let Some(start) = carrier.end.checked_sub(values.len() * 8) else {
        return false;
    };
    let Some(bytes) = body.get_mut(start..carrier.end) else {
        return false;
    };
    for (slot, value) in bytes.chunks_exact_mut(8).zip(values) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
    true
}

/// Patch one stream-local NURBS curve without changing its storage shape.
pub(crate) fn patch_nurbs_by_attr(
    body: &mut [u8],
    attr: u16,
    new: &cadmpeg_ir::geometry::NurbsCurve,
) -> bool {
    let carriers = scan_carriers(body);
    let Some(carrier) = carriers.curve(attr) else {
        return false;
    };
    let CarrierGeometry::Curve(CurveGeometry::Nurbs(old)) = &carrier.geometry else {
        return false;
    };
    patch_nurbs_curve(body, carrier.offset, old, new, 0.001).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compact_carrier(tag: u8, attr: u16, values: &[f64]) -> Vec<u8> {
        let mut bytes = vec![0, tag];
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&[0; 14]);
        bytes.push(0x2b);
        for value in values {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn scan_does_not_skip_overlapping_carrier_starts() {
        let mut bytes = compact_carrier(tag::LINE, 7, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        bytes.truncate(60);
        bytes.extend(compact_carrier(
            tag::LINE,
            8,
            &[1.0, 2.0, 3.0, 0.0, 0.0, 1.0],
        ));

        let carriers = scan_carriers(&bytes);

        assert!(carriers.curve(7).is_some());
        assert!(carriers.curve(8).is_some());
    }

    #[test]
    fn parses_verified_cone_layout() {
        let root_half = std::f64::consts::FRAC_1_SQRT_2;
        let bytes = compact_carrier(
            tag::CONE,
            7,
            &[
                0.0, 0.0, 0.0067, 0.0, 0.0, -1.0, 0.0015, root_half, root_half, -1.0, 0.0, 0.0,
            ],
        );
        let carrier = parse_carrier(&bytes, 0).expect("required invariant");
        let CarrierGeometry::Surface(SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        }) = carrier.geometry
        else {
            panic!("expected cone");
        };
        assert_eq!(origin, Point3::new(0.0, 0.0, 6.7));
        assert_eq!(axis, Vector3::new(0.0, 0.0, -1.0));
        assert_eq!(ref_direction, Vector3::new(-1.0, 0.0, 0.0));
        assert!((radius - 1.5).abs() < 1e-12);
        assert_eq!(ratio, 1.0);
        assert!((half_angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    }

    #[test]
    fn parses_verified_torus_layout() {
        let bytes = compact_carrier(
            tag::TORUS,
            8,
            &[
                0.0, 0.0, 0.0002, 0.0, 0.0, -1.0, 0.0022, 0.0002, -1.0, 0.0, 0.0,
            ],
        );
        let carrier = parse_carrier(&bytes, 0).expect("required invariant");
        let CarrierGeometry::Surface(SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        }) = carrier.geometry
        else {
            panic!("expected torus");
        };
        assert_eq!(center, Point3::new(0.0, 0.0, 0.2));
        assert_eq!(axis, Vector3::new(0.0, 0.0, -1.0));
        assert_eq!(ref_direction, Vector3::new(-1.0, 0.0, 0.0));
        assert!((major_radius - 2.2).abs() < 1e-12);
        assert!((minor_radius - 0.2).abs() < 1e-12);
    }

    #[test]
    fn rejects_nonorthogonal_analytic_frame() {
        let bytes = compact_carrier(
            tag::CIRCLE,
            8,
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.002],
        );

        assert!(parse_carrier(&bytes, 0).is_none());
    }

    #[test]
    fn rejects_invalid_analytic_radii() {
        let ellipse = compact_carrier(
            tag::ELLIPSE,
            8,
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.001, 0.002],
        );
        let torus = compact_carrier(
            tag::TORUS,
            9,
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.001, 0.0, 1.0, 0.0, 0.0],
        );

        assert!(parse_carrier(&ellipse, 0).is_none());
        assert!(parse_carrier(&torus, 0).is_none());
    }

    #[test]
    fn parses_spindle_torus_with_minor_radius_over_major() {
        let bytes = compact_carrier(
            tag::TORUS,
            8,
            &[
                0.0, 0.0, 0.0002, 0.0, 0.0, -1.0, 0.0022, 0.0044, -1.0, 0.0, 0.0,
            ],
        );
        let carrier = parse_carrier(&bytes, 0).expect("spindle torus");
        let CarrierGeometry::Surface(SurfaceGeometry::Torus {
            major_radius,
            minor_radius,
            ..
        }) = carrier.geometry
        else {
            panic!("expected torus");
        };
        assert!((major_radius - 2.2).abs() < 1e-12);
        assert!((minor_radius - 4.4).abs() < 1e-12);
    }

    #[test]
    fn rejects_nonunit_analytic_frames() {
        let cases = [
            (tag::LINE, vec![0.0, 0.0, 0.0, 0.0, 0.0, 2.0]),
            (
                tag::CIRCLE,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 1.0, 0.0, 0.0, 0.002],
            ),
            (
                tag::ELLIPSE,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 0.0, 0.0, 0.002, 0.001],
            ),
            (
                tag::PLANE,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 1.0, 0.0, 0.0],
            ),
            (
                tag::CYLINDER,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.002, 2.0, 0.0, 0.0],
            ),
            (
                tag::CONE,
                vec![
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    0.001,
                    std::f64::consts::FRAC_1_SQRT_2,
                    std::f64::consts::FRAC_1_SQRT_2,
                    2.0,
                    0.0,
                    0.0,
                ],
            ),
            (
                tag::SPHERE,
                vec![0.0, 0.0, 0.0, 0.002, 0.0, 0.0, 1.0, 2.0, 0.0, 0.0],
            ),
            (
                tag::TORUS,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.002, 0.001, 1.0, 0.0, 0.0],
            ),
        ];

        for (tag, values) in cases {
            let bytes = compact_carrier(tag, 9, &values);
            assert!(
                parse_carrier(&bytes, 0).is_none(),
                "accepted tag {tag:#04x}"
            );
        }
    }

    #[test]
    fn rejects_invalid_cone_angle_pair() {
        let bytes = compact_carrier(
            tag::CONE,
            8,
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.001, 0.5, 0.5, 1.0, 0.0, 0.0],
        );

        assert!(parse_carrier(&bytes, 0).is_none());
    }
}
