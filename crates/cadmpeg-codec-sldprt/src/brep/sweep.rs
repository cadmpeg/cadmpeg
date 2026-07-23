// SPDX-License-Identifier: Apache-2.0
//! Swept and spun surface carriers.
//!
//! A `00 43` swept-surface record carries a surface formed by translating a
//! profile curve along a unit direction: `R(u, v) = C(u) + v·D`. A `00 44`
//! spun-surface record carries a surface of revolution of a profile curve
//! about an axis. Both share the compact header shape and name their profile
//! curve by attribute directly after the orientation marker. Absent scalar
//! and vector fields hold the `-3.14158e13` sentinel.
//!
//! The exact construction references the profile carrier; the solved surface
//! emitted for a face is a NURBS patch built from the profile's NURBS form
//! (degree-one ruling for a swept surface, a full rational circle ring for a
//! spun surface).

use std::collections::HashMap;

use cadmpeg_ir::geometry::{NurbsCurve, NurbsSurface};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::wire::be::{f64_at, u16_at};

use super::LEN_TO_MM;

/// A parsed swept- or spun-surface carrier.
#[derive(Debug, Clone)]
pub(crate) struct SweepCarrier {
    /// Stream-local attribute id of the record.
    pub attr: u16,
    /// Tag-byte offset in the stream.
    pub offset: usize,
    /// Attribute of the profile curve carrier.
    pub profile_attr: u16,
    /// Construction-specific fields.
    pub kind: SweepKind,
}

/// The construction a [`SweepCarrier`] encodes.
#[derive(Debug, Clone)]
pub(crate) enum SweepKind {
    /// `00 43`: translation of the profile along a unit direction.
    Swept {
        /// Unit sweep direction (dimensionless).
        direction: Vector3,
    },
    /// `00 44`: revolution of the profile about an axis.
    Spun {
        /// Point on the spin axis, in millimetres.
        base: Point3,
        /// Unit spin-axis direction.
        axis: Vector3,
    },
}

fn unit3(bytes: &[u8], at: usize) -> Option<Vector3> {
    let x = f64_at(bytes, at)?;
    let y = f64_at(bytes, at + 8)?;
    let z = f64_at(bytes, at + 16)?;
    let norm = (x * x + y * y + z * z).sqrt();
    if (norm - 1.0).abs() > 1.0e-9 {
        return None;
    }
    Some(Vector3::new(x, y, z))
}

fn point_mm(bytes: &[u8], at: usize) -> Option<Point3> {
    let x = f64_at(bytes, at)?;
    let y = f64_at(bytes, at + 8)?;
    let z = f64_at(bytes, at + 16)?;
    if !(x.is_finite() && y.is_finite() && z.is_finite()) || x.abs().max(y.abs()).max(z.abs()) > 1e6
    {
        return None;
    }
    Some(Point3::new(x * LEN_TO_MM, y * LEN_TO_MM, z * LEN_TO_MM))
}

/// Parse one swept/spun record whose `00 43`/`00 44` tag starts at `off`.
fn parse_sweep(bytes: &[u8], off: usize) -> Option<SweepCarrier> {
    if bytes.get(off) != Some(&0x00) {
        return None;
    }
    let tt = *bytes.get(off + 1)?;
    if tt != 0x43 && tt != 0x44 {
        return None;
    }
    let mut p = off + 2;
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    let attr = u16_at(bytes, p)?;
    if attr == 0 {
        return None;
    }
    if !matches!(bytes.get(p + 16), Some(0x2b | 0x2d)) {
        return None;
    }
    let profile_attr = u16_at(bytes, p + 17)?;
    if profile_attr == 0 {
        return None;
    }
    let values = p + 19;
    let kind = if tt == 0x43 {
        SweepKind::Swept {
            direction: unit3(bytes, values)?,
        }
    } else {
        SweepKind::Spun {
            base: point_mm(bytes, values)?,
            axis: unit3(bytes, values + 24)?,
        }
    };
    Some(SweepCarrier {
        attr,
        offset: off,
        profile_attr,
        kind,
    })
}

/// Scan a stream for swept/spun surface carriers, keyed by attribute.
pub(crate) fn scan_sweep_carriers(bytes: &[u8]) -> HashMap<u16, SweepCarrier> {
    let mut out = HashMap::new();
    for off in 0..bytes.len().saturating_sub(20) {
        if let Some(carrier) = parse_sweep(bytes, off) {
            out.entry(carrier.attr).or_insert(carrier);
        }
    }
    out
}

/// Build the ruled NURBS patch of a swept surface over `v` in
/// `[v_start, v_end]` millimetres of travel along the unit direction.
pub(crate) fn swept_nurbs(
    profile: &NurbsCurve,
    direction: Vector3,
    v_start: f64,
    v_end: f64,
) -> Option<NurbsSurface> {
    if !(v_start.is_finite() && v_end.is_finite()) || v_end <= v_start {
        return None;
    }
    let n = profile.control_points.len();
    let mut control = Vec::with_capacity(n * 2);
    let mut weights = profile.weights.is_some().then(|| Vec::with_capacity(n * 2));
    for (i, pole) in profile.control_points.iter().enumerate() {
        for v in [v_start, v_end] {
            control.push(Point3::new(
                pole.x + v * direction.x,
                pole.y + v * direction.y,
                pole.z + v * direction.z,
            ));
            if let (Some(out), Some(w)) = (&mut weights, &profile.weights) {
                out.push(w[i]);
            }
        }
    }
    Some(NurbsSurface {
        u_degree: profile.degree,
        v_degree: 1,
        u_knots: profile.knots.clone(),
        v_knots: vec![v_start, v_start, v_end, v_end],
        u_count: n as u32,
        v_count: 2,
        control_points: control,
        weights,
        u_periodic: false,
        v_periodic: false,
    })
}

/// Build the exact rational NURBS of a full surface of revolution: the
/// profile revolved `2π` about the axis through `base`, with the angular
/// parameter (`v`, radians) following `A × (C - Z)`.
pub(crate) fn spun_nurbs(profile: &NurbsCurve, base: Point3, axis: Vector3) -> NurbsSurface {
    use std::f64::consts::{FRAC_PI_2, PI};
    let n = profile.control_points.len();
    let half_sqrt2 = std::f64::consts::SQRT_2 / 2.0;
    let mut control = Vec::with_capacity(n * 9);
    let mut weights = Vec::with_capacity(n * 9);
    for (i, pole) in profile.control_points.iter().enumerate() {
        let pole_weight = profile.weights.as_ref().map_or(1.0, |w| w[i]);
        let offset = [pole.x - base.x, pole.y - base.y, pole.z - base.z];
        let along = offset[0] * axis.x + offset[1] * axis.y + offset[2] * axis.z;
        let center = Point3::new(
            base.x + along * axis.x,
            base.y + along * axis.y,
            base.z + along * axis.z,
        );
        let radial = [pole.x - center.x, pole.y - center.y, pole.z - center.z];
        let radius = (radial[0] * radial[0] + radial[1] * radial[1] + radial[2] * radial[2]).sqrt();
        if radius <= f64::EPSILON {
            // Degenerate ring: the pole sits on the axis.
            for k in 0..9 {
                control.push(center);
                weights.push(if k % 2 == 1 {
                    pole_weight * half_sqrt2
                } else {
                    pole_weight
                });
            }
            continue;
        }
        let x_hat = [radial[0] / radius, radial[1] / radius, radial[2] / radius];
        let y_hat = [
            axis.y * x_hat[2] - axis.z * x_hat[1],
            axis.z * x_hat[0] - axis.x * x_hat[2],
            axis.x * x_hat[1] - axis.y * x_hat[0],
        ];
        for k in 0..9u32 {
            let angle = f64::from(k) * (FRAC_PI_2 / 2.0);
            let (scale, weight) = if k % 2 == 1 {
                (radius * std::f64::consts::SQRT_2, pole_weight * half_sqrt2)
            } else {
                (radius, pole_weight)
            };
            let (sin, cos) = angle.sin_cos();
            control.push(Point3::new(
                center.x + scale * (cos * x_hat[0] + sin * y_hat[0]),
                center.y + scale * (cos * x_hat[1] + sin * y_hat[1]),
                center.z + scale * (cos * x_hat[2] + sin * y_hat[2]),
            ));
            weights.push(weight);
        }
    }
    let v_knots = vec![
        0.0,
        0.0,
        0.0,
        FRAC_PI_2,
        FRAC_PI_2,
        PI,
        PI,
        3.0 * FRAC_PI_2,
        3.0 * FRAC_PI_2,
        2.0 * PI,
        2.0 * PI,
        2.0 * PI,
    ];
    NurbsSurface {
        u_degree: profile.degree,
        v_degree: 2,
        u_knots: profile.knots.clone(),
        v_knots,
        u_count: n as u32,
        v_count: 9,
        control_points: control,
        weights: Some(weights),
        u_periodic: false,
        v_periodic: true,
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, SQRT_2};

    use super::*;

    fn header(tt: u8, attr: u16, profile: u16) -> Vec<u8> {
        let mut bytes = vec![0x00, tt];
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&7u32.to_be_bytes());
        for r in [1u16, 2, 3, 4, 1] {
            bytes.extend_from_slice(&r.to_be_bytes());
        }
        bytes.push(0x2b);
        bytes.extend_from_slice(&profile.to_be_bytes());
        bytes
    }

    #[test]
    fn parses_swept_record() {
        let mut bytes = header(0x43, 9, 5);
        for v in [0.0f64, 0.0, 1.0, 0.25] {
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        let carriers = scan_sweep_carriers(&bytes);
        let carrier = carriers.get(&9).expect("swept carrier");
        assert_eq!(carrier.profile_attr, 5);
        let SweepKind::Swept { direction } = &carrier.kind else {
            panic!("expected swept kind");
        };
        assert_eq!((direction.x, direction.y, direction.z), (0.0, 0.0, 1.0));
    }

    #[test]
    fn parses_spun_record_with_sentinel_tail() {
        const MISSING: u64 = 0xc2bc_928f_996e_0000;

        let mut bytes = header(0x44, 12, 6);
        for v in [0.0f64, 0.0, 0.0, 0.0, -1.0, 0.0] {
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        for _ in 0..8 {
            bytes.extend_from_slice(&MISSING.to_be_bytes());
        }
        let carriers = scan_sweep_carriers(&bytes);
        let carrier = carriers.get(&12).expect("spun carrier");
        assert_eq!(carrier.profile_attr, 6);
        let SweepKind::Spun { base, axis } = &carrier.kind else {
            panic!("expected spun kind");
        };
        assert_eq!((base.x, base.y, base.z), (0.0, 0.0, 0.0));
        assert_eq!((axis.x, axis.y, axis.z), (0.0, -1.0, 0.0));
    }

    #[test]
    fn rejects_non_unit_direction() {
        let mut bytes = header(0x43, 9, 5);
        for v in [0.0f64, 0.0, 2.0, 0.25] {
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        assert!(scan_sweep_carriers(&bytes).is_empty());
    }

    fn eval_surface(surface: &NurbsSurface, u_parameter: f64, v_parameter: f64) -> Point3 {
        // De Boor via basis functions (dense evaluation, test only).
        fn basis(knots: &[f64], degree: usize, count: usize, t: f64) -> Vec<f64> {
            let mut n = vec![0.0f64; count];
            // Find span.
            let mut out = vec![0.0f64; count];
            for i in 0..count {
                // Cox-de Boor recursive (inefficient but fine for tests).
                fn cox(knots: &[f64], i: usize, p: usize, t: f64) -> f64 {
                    if p == 0 {
                        let last = knots.len() - 1;
                        let hi = knots[i + 1];
                        if (knots[i] <= t && t < hi)
                            || (t >= hi && hi == knots[last] && knots[i] < hi)
                        {
                            1.0
                        } else {
                            0.0
                        }
                    } else {
                        let mut value = 0.0;
                        let d1 = knots[i + p] - knots[i];
                        if d1 > 0.0 {
                            value += (t - knots[i]) / d1 * cox(knots, i, p - 1, t);
                        }
                        let d2 = knots[i + p + 1] - knots[i + 1];
                        if d2 > 0.0 {
                            value += (knots[i + p + 1] - t) / d2 * cox(knots, i + 1, p - 1, t);
                        }
                        value
                    }
                }
                out[i] = cox(knots, i, degree, t);
                n[i] = out[i];
            }
            out
        }
        let u_basis = basis(
            &surface.u_knots,
            surface.u_degree as usize,
            surface.u_count as usize,
            u_parameter,
        );
        let v_basis = basis(
            &surface.v_knots,
            surface.v_degree as usize,
            surface.v_count as usize,
            v_parameter,
        );
        let mut acc = [0.0f64; 4];
        for u_index in 0..surface.u_count as usize {
            for v_index in 0..surface.v_count as usize {
                let weight = surface.weights.as_ref().map_or(1.0, |weights| {
                    weights[u_index * surface.v_count as usize + v_index]
                });
                let basis_weight = u_basis[u_index] * v_basis[v_index] * weight;
                let point = &surface.control_points[u_index * surface.v_count as usize + v_index];
                acc[0] += basis_weight * point.x;
                acc[1] += basis_weight * point.y;
                acc[2] += basis_weight * point.z;
                acc[3] += basis_weight;
            }
        }
        Point3::new(acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3])
    }

    #[test]
    fn spun_line_reproduces_cylinder() {
        // Profile: vertical line x=2, from z=0 to z=1 (degree 1).
        let profile = NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)],
            weights: None,
            periodic: false,
        };
        let surface = spun_nurbs(
            &profile,
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        );
        // The revolution is the standard rational quadratic NURBS circle: four
        // 90-degree Bézier segments with corner weights √2/2 and breakpoint
        // knots at 0, π/2, π, 3π/2, 2π. Within a segment the parameter `v` is
        // not the geometric angle — a quadratic cannot parameterize a circle by
        // arc length — so the swept angle equals `v` only at the breakpoints.
        // The expected angle is the exact segment map of the rational Bézier.
        let expected_angle = |v: f64| {
            let seg = (v / FRAC_PI_2).floor();
            let t = (v - seg * FRAC_PI_2) / FRAC_PI_2;
            let w = SQRT_2 / 2.0;
            let nx = (1.0 - t).powi(2) + 2.0 * w * t * (1.0 - t);
            let ny = 2.0 * w * t * (1.0 - t) + t * t;
            seg * FRAC_PI_2 + ny.atan2(nx)
        };
        for &(u, v) in &[(0.25, 0.3), (0.5, 2.0), (0.9, 5.5)] {
            let p = eval_surface(&surface, u, v);
            let r = (p.x * p.x + p.y * p.y).sqrt();
            assert!((r - 2.0).abs() < 1e-12, "radius {r} at ({u}, {v})");
            assert!((p.z - u).abs() < 1e-12, "height {} at ({u}, {v})", p.z);
            // Angle follows A × x_hat, in the direction of revolution.
            let angle = p.y.atan2(p.x).rem_euclid(2.0 * std::f64::consts::PI);
            let want = expected_angle(v);
            assert!((angle - want).abs() < 1e-12, "angle {angle} at ({u}, {v})");
        }
    }

    #[test]
    fn swept_line_reproduces_ruled_plane() {
        let profile = NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            weights: None,
            periodic: false,
        };
        let surface =
            swept_nurbs(&profile, Vector3::new(0.0, 1.0, 0.0), -2.0, 3.0).expect("swept surface");
        let p = eval_surface(&surface, 0.5, 1.5);
        assert!((p.x - 0.5).abs() < 1e-12);
        assert!((p.y - 1.5).abs() < 1e-12);
        assert!(p.z.abs() < 1e-12);
    }
}
