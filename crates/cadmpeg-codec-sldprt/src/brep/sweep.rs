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

use std::borrow::Cow;
use std::collections::HashMap;

use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsPoles3, NurbsSurface},
    CurveGeometry, SolvedCurveGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::SumSquaresUnitVector3;
use cadmpeg_parasolid::records::node_type::{SPUN_SURF, SWEPT_SURF};
use cadmpeg_parasolid::records::{NodeFields, PrimaryRecords};

use super::LEN_TO_MM;
use crate::layout::spun_surface_profile as spun_layout;

const CONIC_INTERVAL_TOLERANCE: f64 = 1.0e-9;

const ABSENT_PROFILE_PARAMETER: f64 = -3.14158e13;

/// Poles per profile pole across the ruling of [`swept_nurbs`].
pub(super) const SWEPT_RULING_POLES: usize = 2;
/// Knots of the linear ruling axis of [`swept_nurbs`].
pub(super) const SWEPT_RULING_KNOTS: usize = 4;
/// Poles per profile pole in the full-revolution ring of [`spun_nurbs`].
pub(super) const SPUN_RING_POLES: usize = 9;
/// Knots of the quadratic angular axis of [`spun_nurbs`].
pub(super) const SPUN_RING_KNOTS: usize = 12;

/// A parsed swept- or spun-surface carrier.
#[derive(Debug, Clone)]
pub(super) struct SweepCarrier {
    /// Stream-local attribute id of the record.
    attr: u32,
    /// Tag-byte offset in the stream.
    pub(super) offset: usize,
    /// Attribute of the profile curve carrier.
    pub(super) profile_attr: u32,
    pub(super) positive_sense: bool,
    /// Construction-specific fields.
    pub(super) kind: SweepKind,
}

/// The construction a [`SweepCarrier`] encodes.
#[derive(Debug, Clone)]
pub(super) enum SweepKind {
    /// `00 43`: translation of the profile along a unit direction.
    Swept {
        /// Unit sweep direction (dimensionless).
        direction: SumSquaresUnitVector3,
    },
    /// `00 44`: revolution of the profile about an axis.
    Spun {
        /// Point on the spin axis, in millimetres.
        base: Point3,
        /// Unit spin-axis direction.
        axis: SumSquaresUnitVector3,
        /// Profile parameters at the stored axis intersections.
        profile_bounds: [Option<f64>; 2],
    },
}

pub(super) fn native_profile_parameters(geometry: &CurveGeometry) -> bool {
    matches!(
        geometry,
        CurveGeometry::Solved(
            SolvedCurveGeometry::Nurbs(_)
                | SolvedCurveGeometry::Circle(_)
                | SolvedCurveGeometry::Ellipse(_)
        )
    )
}

fn triple(bytes: &[u8], at: usize) -> Option<[f64; 3]> {
    Some([
        View::f64_be_at(bytes, at)?,
        View::f64_be_at(bytes, at + 8)?,
        View::f64_be_at(bytes, at + 16)?,
    ])
}

fn unit3([x, y, z]: [f64; 3]) -> Option<SumSquaresUnitVector3> {
    SumSquaresUnitVector3::new(Vector3::new(x, y, z))
}

enum ProfileParameter {
    Absent,
    Present(f64),
}

impl ProfileParameter {
    fn into_option(self) -> Option<f64> {
        match self {
            Self::Absent => None,
            Self::Present(value) => Some(value),
        }
    }
}

fn profile_parameter(point: [f64; 3], value: f64) -> Option<ProfileParameter> {
    if value == ABSENT_PROFILE_PARAMETER {
        point
            .iter()
            .all(|coordinate| *coordinate == ABSENT_PROFILE_PARAMETER)
            .then_some(ProfileParameter::Absent)
    } else {
        point
            .iter()
            .all(|coordinate| coordinate.is_finite() && *coordinate != ABSENT_PROFILE_PARAMETER)
            .then_some(ProfileParameter::Present(value))
    }
}

fn point_mm([x, y, z]: [f64; 3]) -> Option<Point3> {
    if !(x.is_finite() && y.is_finite() && z.is_finite()) {
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
    let attr = super::word_at(bytes, p)?;
    if attr == 0 {
        return None;
    }
    if !matches!(bytes.get(p + 16), Some(0x2b | 0x2d)) {
        return None;
    }
    let profile_attr = super::word_at(bytes, p + 17)?;
    if profile_attr == 0 {
        return None;
    }
    let values = p + 19;
    let kind = if tt == 0x43 {
        SweepKind::Swept {
            direction: unit3(triple(bytes, values)?)?,
        }
    } else {
        SweepKind::Spun {
            base: point_mm(triple(bytes, values)?)?,
            axis: unit3(triple(bytes, values + spun_layout::AXIS)?)?,
            profile_bounds: [
                profile_parameter(
                    triple(bytes, values + spun_layout::START)?,
                    View::f64_be_at(bytes, values + spun_layout::START_PARAM)?,
                )?
                .into_option(),
                profile_parameter(
                    triple(bytes, values + spun_layout::END)?,
                    View::f64_be_at(bytes, values + spun_layout::END_PARAM)?,
                )?
                .into_option(),
            ],
        }
    };
    Some(SweepCarrier {
        attr,
        offset: off,
        profile_attr,
        positive_sense: false,
        kind,
    })
}

/// Scan a stream for swept/spun surface carriers, keyed by attribute.
pub(super) fn scan_sweep_carriers(bytes: &[u8]) -> HashMap<u32, SweepCarrier> {
    let mut out = HashMap::new();
    let Some(last_offset) = bytes.len().checked_sub(20) else {
        return out;
    };
    for off in 0..last_offset {
        if let Some(carrier) = parse_sweep(bytes, off) {
            out.entry(carrier.attr).or_insert(carrier);
        }
    }
    out
}

/// Read swept/spun surface carriers from `SWEPT_SURF` and `SPUN_SURF` nodes,
/// keyed by node index; `None` when a base field of a node does not read or two
/// nodes share an index.
pub(super) fn from_nodes(
    records: &PrimaryRecords<'_>,
    body_offset: usize,
) -> Option<HashMap<u32, SweepCarrier>> {
    from_nodes_of(&records.nodes().collect::<Vec<_>>(), body_offset)
}

/// [`from_nodes`] over any node instances, in stream order.
pub(super) fn from_nodes_of<N: NodeFields>(
    nodes: &[N],
    body_offset: usize,
) -> Option<HashMap<u32, SweepCarrier>> {
    let mut out = HashMap::new();
    for node in nodes
        .iter()
        .filter(|node| matches!(node.kind(), SWEPT_SURF | SPUN_SURF))
    {
        let sense = node.char("sense")?;
        let sensed = matches!(sense, b'+' | b'-');
        let (profile_attr, kind) = if node.kind() == SWEPT_SURF {
            let direction = node.vector("sweep")?;
            (
                node.pointer("section")?,
                unit3(direction).map(|direction| SweepKind::Swept { direction }),
            )
        } else {
            let base = node.vector("base")?;
            let axis = node.vector("axis")?;
            let profile_bounds = [
                profile_parameter(node.vector("start")?, node.real("start_param")?)?.into_option(),
                profile_parameter(node.vector("end")?, node.real("end_param")?)?.into_option(),
            ];
            let kind = point_mm(base)
                .zip(unit3(axis))
                .map(|(base, axis)| SweepKind::Spun {
                    base,
                    axis,
                    profile_bounds,
                });
            (node.pointer("profile")?, kind)
        };
        let offset = node.range().start.checked_sub(body_offset)?;
        let Some(kind) = kind.filter(|_| sensed && profile_attr > 1) else {
            continue;
        };
        let carrier = SweepCarrier {
            attr: node.index(),
            offset,
            profile_attr,
            positive_sense: sense == b'+',
            kind,
        };
        out.insert(node.index(), carrier).is_none().then_some(())?;
    }
    Some(out)
}

pub(super) fn stored_conic_interval(
    geometry: &CurveGeometry,
    bounds: [Option<f64>; 2],
) -> Option<[f64; 2]> {
    match (geometry.solved()?, bounds) {
        (
            SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_),
            [Some(low), Some(high)],
        ) if low.is_finite()
            && high.is_finite()
            && high > low
            && high - low <= std::f64::consts::TAU + CONIC_INTERVAL_TOLERANCE =>
        {
            Some([low, high])
        }
        _ => None,
    }
}

/// Convert an exact analytic sweep profile to its exact rational NURBS form.
pub(super) fn profile_nurbs<'a>(
    geometry: &'a CurveGeometry,
    interval: Option<[f64; 2]>,
    record: &dyn std::fmt::Display,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<Cow<'a, NurbsCurve>> {
    let (center, frame, major_radius, minor_radius) = match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) => {
            return Some(Cow::Borrowed(curve));
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let radius = circle_curve.radius().get();
            (
                circle_curve.center().get(),
                *circle_curve.frame(),
                radius,
                radius,
            )
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => (
            ellipse_curve.center().get(),
            *ellipse_curve.frame(),
            ellipse_curve.major_radius().get(),
            ellipse_curve.minor_radius().get(),
        ),
        _ => return None,
    };
    let interval = interval
        .and_then(|range| stored_conic_interval(geometry, range.map(Some)))
        .filter(|range| *range != [0.0, std::f64::consts::TAU]);
    let major = *frame.reference().as_raw();
    let minor = *frame.binormal().as_raw();
    let (control_points, weights, knots) = if interval.is_none() {
        let mut points = Vec::with_capacity(9);
        let mut weights = Vec::with_capacity(9);
        for index in 0_i32..9 {
            let angle = f64::from(index) * std::f64::consts::FRAC_PI_4;
            let (sin, cos) = angle.sin_cos();
            let scale = if index % 2 == 0 {
                1.0
            } else {
                std::f64::consts::SQRT_2
            };
            points.push(
                center
                    .translated(major, scale * major_radius * cos)
                    .translated(minor, scale * minor_radius * sin),
            );
            weights.push(if index % 2 == 0 {
                1.0
            } else {
                std::f64::consts::SQRT_2 / 2.0
            });
        }
        let mut knots = vec![0.0; 3];
        for quarter in 1..4_u32 {
            knots.extend([f64::from(quarter) * std::f64::consts::FRAC_PI_2; 2]);
        }
        knots.extend([std::f64::consts::TAU; 3]);
        (points, weights, knots)
    } else {
        let [low, high] = interval.unwrap_or([0.0, std::f64::consts::TAU]);
        let delta = high - low;
        let spans = (1..=4_u32)
            .find(|count| delta <= f64::from(*count) * std::f64::consts::FRAC_PI_2)
            .unwrap_or(4);
        let step = delta / f64::from(spans);
        let point = |angle: f64, scale: f64| {
            center
                .translated(major, scale * major_radius * angle.cos())
                .translated(minor, scale * minor_radius * angle.sin())
        };
        let mut control_points = vec![point(low, 1.0)];
        let mut weights = vec![1.0];
        let mut knots = vec![low, low, low];
        for span in 0..spans {
            let start = low + f64::from(span) * step;
            let end = if span + 1 == spans {
                high
            } else {
                low + f64::from(span + 1) * step
            };
            let middle = (start + end) * 0.5;
            let weight = ((end - start) * 0.5).cos();
            control_points.extend([point(middle, weight.recip()), point(end, 1.0)]);
            weights.extend([weight, 1.0]);
            knots.extend([end, end]);
        }
        knots.push(high);
        (control_points, weights, knots)
    };
    match cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        2,
        knots,
        control_points,
        Some(weights),
        false,
    ) {
        Ok(curve) => Some(Cow::Owned(curve)),
        Err(error) => {
            refusal.note(format_args!("sldprt sweep profile arc: {record}"), &error);
            None
        }
    }
}

pub(super) fn curve_projection_bounds(
    geometry: &CurveGeometry,
    direction: Vector3,
) -> Option<[f64; 2]> {
    let project =
        |point: Point3| point.x * direction.x + point.y * direction.y + point.z * direction.z;
    let (center, frame, radii) = match geometry.solved()? {
        SolvedCurveGeometry::Circle(circle) => (
            circle.center().get(),
            circle.frame(),
            [circle.radius().get(); 2],
        ),
        SolvedCurveGeometry::Ellipse(ellipse) => (
            ellipse.center().get(),
            ellipse.frame(),
            [ellipse.major_radius().get(), ellipse.minor_radius().get()],
        ),
        SolvedCurveGeometry::Nurbs(curve) => {
            let mut bounds = [f64::INFINITY, f64::NEG_INFINITY];
            for index in 0..curve.pole_count() {
                let point = match curve.pole_rows() {
                    NurbsPoles3::Polynomial { points } => points[index].get(),
                    NurbsPoles3::Rational { points } => {
                        if points[index].weight.get() <= 0.0 {
                            return None;
                        }
                        points[index].point.get()
                    }
                };
                let value = project(point);
                if !value.is_finite() {
                    return None;
                }
                bounds[0] = bounds[0].min(value);
                bounds[1] = bounds[1].max(value);
            }
            return bounds
                .iter()
                .all(|value| value.is_finite())
                .then_some(bounds);
        }
        _ => return None,
    };
    let along =
        |vector: &Vector3| vector.x * direction.x + vector.y * direction.y + vector.z * direction.z;
    let radius = (radii[0] * along(frame.reference().as_raw()))
        .hypot(radii[1] * along(frame.binormal().as_raw()));
    let center = project(center);
    let bounds = [center - radius, center + radius];
    bounds
        .iter()
        .all(|value| value.is_finite())
        .then_some(bounds)
}

/// Build the ruled NURBS patch of a swept surface over `v` in
/// `[v_start, v_end]` millimetres of travel along the unit direction.
pub(super) fn swept_nurbs(
    profile: &NurbsCurve,
    direction: SumSquaresUnitVector3,
    v_start: f64,
    v_end: f64,
    profile_closed: bool,
    record: &dyn std::fmt::Display,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<NurbsSurface> {
    let direction = direction.as_raw();
    if !(v_start.is_finite() && v_end.is_finite()) || v_end <= v_start {
        return None;
    }
    let n = profile.pole_count();
    let mut control = Vec::new();
    let mut weights = matches!(profile.pole_rows(), NurbsPoles3::Rational { .. }).then(Vec::new);
    for i in 0..n {
        let (pole, weight) = match profile.pole_rows() {
            NurbsPoles3::Polynomial { points } => (points[i].get(), None),
            NurbsPoles3::Rational { points } => (points[i].point.get(), Some(points[i].weight)),
        };
        for v in [v_start, v_end] {
            control.push(Point3::new(
                pole.x + v * direction.x,
                pole.y + v * direction.y,
                pole.z + v * direction.z,
            ));
            if let (Some(out), Some(weight)) = (&mut weights, weight) {
                out.push(weight);
            }
        }
    }
    let control_rows = control
        .chunks(SWEPT_RULING_POLES)
        .map(<[_]>::to_vec)
        .collect();
    let weight_rows = weights.map(|values| {
        values
            .chunks(SWEPT_RULING_POLES)
            .map(<[_]>::to_vec)
            .collect()
    });
    let knots = profile.knots().as_slice().to_vec();
    match NurbsSurface::from_checked_lanes(
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
            profile.degree(),
            knots,
            profile.periodic() || profile_closed,
        ),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
            1,
            vec![v_start, v_start, v_end, v_end],
            false,
        ),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(control_rows, weight_rows),
        false,
    ) {
        Ok(surface) => Some(surface),
        Err(error) => {
            refusal.note(
                format_args!("sldprt swept ruled surface patch: {record}"),
                &error,
            );
            None
        }
    }
}

/// Build the exact rational NURBS of a full surface of revolution: the
/// profile revolved `2π` about the axis through `base`, with the angular
/// parameter (`v`, radians) following `A × (C - Z)`.
pub(super) fn spun_nurbs(
    profile: &NurbsCurve,
    base: Point3,
    axis: SumSquaresUnitVector3,
    record: &dyn std::fmt::Display,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<NurbsSurface> {
    use std::f64::consts::{FRAC_PI_2, PI};
    let axis = axis.as_raw();
    let n = profile.pole_count();
    let half_sqrt2 = std::f64::consts::SQRT_2 / 2.0;
    let mut control = Vec::new();
    let mut weights = Vec::new();
    for i in 0..n {
        let (pole, pole_weight) = match profile.pole_rows() {
            NurbsPoles3::Polynomial { points } => (points[i].get(), 1.0),
            NurbsPoles3::Rational { points } => (points[i].point.get(), points[i].weight.get()),
        };
        let offset = [pole.x - base.x, pole.y - base.y, pole.z - base.z];
        let along = offset[0] * axis.x + offset[1] * axis.y + offset[2] * axis.z;
        let center = Point3::new(
            base.x + along * axis.x,
            base.y + along * axis.y,
            base.z + along * axis.z,
        );
        let radial = [pole.x - center.x, pole.y - center.y, pole.z - center.z];
        let radius = radial[0].hypot(radial[1]).hypot(radial[2]);
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
            let mut coordinates = [center.x, center.y, center.z];
            for (coordinate, (x, y)) in coordinates.iter_mut().zip(x_hat.into_iter().zip(y_hat)) {
                let component = cos * x + sin * y;
                let offset = if scale.is_finite() {
                    scale * component
                } else {
                    cadmpeg_ir::math::product_quotient(
                        [radius, std::f64::consts::SQRT_2, component],
                        [1.0],
                    )?
                    .get()
                };
                *coordinate += offset;
            }
            control.push(Point3::new(coordinates[0], coordinates[1], coordinates[2]));
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
    let knots = profile.knots().as_slice().to_vec();
    let control_rows = control.chunks(SPUN_RING_POLES).map(<[_]>::to_vec).collect();
    let weight_rows = weights.chunks(SPUN_RING_POLES).map(<[_]>::to_vec).collect();
    match cadmpeg_ir::geometry::nurbs::NurbsSurface::from_lanes(
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
            profile.degree(),
            knots,
            profile.periodic(),
        ),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(2, v_knots, true),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(control_rows, Some(weight_rows)),
        false,
    ) {
        Ok(surface) => Some(surface),
        Err(error) => {
            refusal.note(format_args!("sldprt spun surface patch: {record}"), &error);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, SQRT_2};

    use cadmpeg_core::decode::index_from_u32;

    use super::{profile_nurbs, spun_nurbs, swept_nurbs, SweepKind};
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::nurbs::NurbsSurface;
    use cadmpeg_ir::geometry::CurveGeometry;
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::units::SumSquaresUnitVector3;

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
        let carriers = super::scan_sweep_carriers(&bytes);
        let carrier = carriers.get(&9).expect("swept carrier");
        assert_eq!(carrier.profile_attr, 5);
        let SweepKind::Swept { direction } = &carrier.kind else {
            panic!("expected swept kind");
        };
        let direction = direction.as_raw();
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
        let carriers = super::scan_sweep_carriers(&bytes);
        let carrier = carriers.get(&12).expect("spun carrier");
        assert_eq!(carrier.profile_attr, 6);
        let SweepKind::Spun { base, axis, .. } = &carrier.kind else {
            panic!("expected spun kind");
        };
        assert_eq!((base.x, base.y, base.z), (0.0, 0.0, 0.0));
        let axis = axis.as_raw();
        assert_eq!((axis.x, axis.y, axis.z), (0.0, -1.0, 0.0));
    }

    #[test]
    fn rejects_non_unit_direction() {
        let mut bytes = header(0x43, 9, 5);
        for v in [0.0f64, 0.0, 2.0, 0.25] {
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        assert!(super::scan_sweep_carriers(&bytes).is_empty());
    }

    #[test]
    fn admits_square_sum_unit_boundary_even_when_hypot_rounds_outside() {
        // Platform hypot implementations round different boundary directions outward.
        let directions = [
            Vector3::new(-0.331_490_106_610_188_6, -0.943_458_696_085_613_4, 0.0),
            Vector3::new(
                0.630_485_132_526_264,
                0.666_809_318_792_633_2,
                0.397_308_233_031_539_17,
            ),
        ];
        let mut hypot_refuses = false;
        for direction in directions {
            assert!(SumSquaresUnitVector3::new(direction).is_some());
            hypot_refuses |= cadmpeg_ir::units::UnitVector3::new(direction).is_none();
            let mut bytes = header(0x43, 9, 5);
            for value in [direction.x, direction.y, direction.z, 0.25] {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
            assert!(super::scan_sweep_carriers(&bytes).contains_key(&9));
        }
        assert!(
            hypot_refuses,
            "a boundary direction must distinguish the admission gates"
        );
    }

    #[test]
    fn rejects_non_finite_direction() {
        let mut bytes = header(0x43, 9, 5);
        for v in [f64::NAN, 0.0, 1.0] {
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        assert!(super::scan_sweep_carriers(&bytes).is_empty());
    }

    #[test]
    fn accepts_finite_axis_points_without_a_coordinate_cutoff() {
        let mut bytes = header(0x44, 12, 6);
        for v in [2.0e6f64, -3.0e6, 4.0e6, 0.0, 0.0, 1.0] {
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        for _ in 0..8 {
            bytes.extend_from_slice(&super::ABSENT_PROFILE_PARAMETER.to_be_bytes());
        }
        let carriers = super::scan_sweep_carriers(&bytes);
        let carrier = carriers.get(&12).expect("spun carrier");
        let SweepKind::Spun { base, .. } = &carrier.kind else {
            panic!("expected spun kind");
        };
        assert_eq!(*base, Point3::new(2.0e9, -3.0e9, 4.0e9));
    }

    fn eval_curve(curve: &NurbsCurve, parameter: f64) -> Point3 {
        cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            curve,
            parameter,
        )
        .expect("evaluable curve")
        .get()
    }

    #[test]
    fn analytic_ellipse_profile_has_exact_rational_form() {
        let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
            cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                Point3::new(3.0, -2.0, 7.0),
                Vector3::new(0.0, 0.0, 2.0).unit().unwrap(),
                Vector3::new(4.0, 0.0, 0.0).unit().unwrap(),
                5.0,
                2.0,
            )
            .unwrap(),
        ));
        let curve = profile_nurbs(
            &geometry,
            None,
            &"test profile",
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect("ellipse NURBS");

        assert_eq!(curve.degree(), 2);
        assert_eq!(curve.control_points().len(), 9);
        assert!(!curve.periodic(), "the representation is endpoint-clamped");
        for parameter in [0.0, 0.3, FRAC_PI_2, 2.4, std::f64::consts::PI, 5.7] {
            let point = eval_curve(&curve, parameter);
            let center_y = -2.0;
            let ellipse = ((point.x - 3.0) / 5.0).powi(2) + ((point.y - center_y) / 2.0).powi(2);
            assert!((ellipse - 1.0).abs() < 1.0e-12, "ellipse value {ellipse}");
            assert!((point.z - 7.0).abs() < 1.0e-12);
        }
        assert_eq!(eval_curve(&curve, 0.0), Point3::new(8.0, -2.0, 7.0));
        let quarter = eval_curve(&curve, FRAC_PI_2);
        assert!((quarter.x - 3.0).abs() < 1.0e-12);
        assert!(quarter.y.abs() < 1.0e-12);
        assert!((quarter.z - 7.0).abs() < 1.0e-12);
    }

    #[test]
    fn analytic_circle_profile_has_exact_rational_form() {
        let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(-1.0, 2.0, 3.0),
                Vector3::new(0.0, 2.0, 0.0).unit().unwrap(),
                Vector3::new(0.0, 0.0, 4.0).unit().unwrap(),
                6.0,
            )
            .unwrap(),
        ));
        let curve = profile_nurbs(
            &geometry,
            None,
            &"test profile",
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect("circle NURBS");

        for parameter in [0.0, 0.7, FRAC_PI_2, 3.4, 5.9] {
            let point = eval_curve(&curve, parameter);
            let radius = ((point.x + 1.0).powi(2) + (point.z - 3.0).powi(2)).sqrt();
            assert!((radius - 6.0).abs() < 1.0e-12, "radius {radius}");
            assert!((point.y - 2.0).abs() < 1.0e-12);
        }
    }

    fn eval_surface(surface: &NurbsSurface, u_parameter: f64, v_parameter: f64) -> Point3 {
        // De Boor via basis functions (dense evaluation, test only).
        fn basis(knots: &[f64], degree: usize, count: usize, t: f64) -> Vec<f64> {
            // Find span.
            let mut out = vec![0.0f64; count];
            for (i, value) in out.iter_mut().enumerate() {
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
                *value = cox(knots, i, degree, t);
            }
            out
        }
        let u_basis = basis(
            surface.u_knots(),
            index_from_u32(surface.u_degree()),
            surface.u_count(),
            u_parameter,
        );
        let v_basis = basis(
            surface.v_knots(),
            index_from_u32(surface.v_degree()),
            surface.v_count(),
            v_parameter,
        );
        let mut acc = [0.0f64; 4];
        for (u_index, u_weight) in u_basis.iter().enumerate() {
            for (v_index, v_weight) in v_basis.iter().enumerate() {
                let weight = surface
                    .weight(u_index, v_index)
                    .map_or(1.0, cadmpeg_ir::scalar::NonZeroReal::get);
                let basis_weight = u_weight * v_weight * weight;
                let point = &surface.control_grid()[u_index][v_index];
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
        let profile = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)],
            None,
            false,
        )
        .expect("valid line profile");
        let surface = spun_nurbs(
            &profile,
            Point3::new(0.0, 0.0, 0.0),
            SumSquaresUnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
            &"test spun construction",
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect("valid spun surface");
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
            assert!((r - 2.0).abs() < 1.0e-12, "radius {r} at ({u}, {v})");
            assert!((p.z - u).abs() < 1.0e-12, "height {} at ({u}, {v})", p.z);
            // Angle follows A × x_hat, in the direction of revolution.
            let angle = p.y.atan2(p.x).rem_euclid(2.0 * std::f64::consts::PI);
            let want = expected_angle(v);
            assert!(
                (angle - want).abs() < 1.0e-12,
                "angle {angle} at ({u}, {v})"
            );
        }
    }

    #[test]
    fn swept_line_reproduces_ruled_plane() {
        let profile = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("valid line profile");
        let surface = swept_nurbs(
            &profile,
            SumSquaresUnitVector3::new(Vector3::new(0.0, 1.0, 0.0)).unwrap(),
            -2.0,
            3.0,
            false,
            &"test swept construction",
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect("swept surface");
        let p = eval_surface(&surface, 0.5, 1.5);
        assert!((p.x - 0.5).abs() < 1.0e-12);
        assert!((p.y - 1.5).abs() < 1.0e-12);
        assert!(p.z.abs() < 1.0e-12);
    }

    #[test]
    fn swept_line_profile_builds_a_surface() {
        let profile = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("valid line profile");
        let direction =
            SumSquaresUnitVector3::new(Vector3::new(0.0, 1.0, 0.0)).expect("unit direction");
        assert!(swept_nurbs(
            &profile,
            direction,
            0.0,
            1.0,
            false,
            &"sweep",
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .is_some());
    }

    #[test]
    fn spun_line_profile_builds_a_surface() {
        let profile = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)],
            None,
            false,
        )
        .expect("valid line profile");
        let axis = SumSquaresUnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).expect("unit axis");
        assert!(spun_nurbs(
            &profile,
            Point3::new(0.0, 0.0, 0.0),
            axis,
            &"spin",
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .is_some());
    }

    #[test]
    fn two_refused_swept_carriers_state_two_records_each_naming_its_carrier() {
        let profile = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                Point3::new(f64::MAX, 0.0, 0.0),
                Point3::new(f64::MAX, 1.0, 0.0),
            ],
            None,
            false,
        )
        .expect("valid profile");
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let first = swept_nurbs(
            &profile,
            SumSquaresUnitVector3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
            0.0,
            f64::MAX,
            false,
            &"sweep construction at byte 16 for surface attr 3",
            &mut refusal,
        );
        let second = swept_nurbs(
            &profile,
            SumSquaresUnitVector3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
            0.0,
            f64::MAX,
            false,
            &"sweep construction at byte 64 for surface attr 9",
            &mut refusal,
        );
        assert!(first.is_none(), "the refused ruling states no surface");
        assert!(second.is_none(), "the refused ruling states no surface");
        let records = refusal.take_records();
        assert_eq!(
            records.len(),
            2,
            "one record per refused ruling: {records:?}"
        );
        for record in &records {
            assert!(
                record.starts_with("sldprt swept ruled surface patch: "),
                "the record names the carrier that stated the lanes: {record}"
            );
        }
        assert!(
            refusal.take_records().is_empty(),
            "the sink is empty once its records are taken"
        );
    }
    #[test]
    fn audit_regression_spun_profile_retains_large_finite_radius() {
        for radius in [1e200, 1.6e308] {
            let profile = NurbsCurve::from_lanes(
                1,
                vec![0., 0., 1., 1.],
                vec![Point3::new(radius, 0., 0.), Point3::new(radius, 0., 1.)],
                None,
                false,
            )
            .unwrap();
            let surface = super::spun_nurbs(
                &profile,
                Point3::new(0., 0., 0.),
                SumSquaresUnitVector3::new(Vector3::new(0., 0., 1.)).unwrap(),
                &"audit",
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
            .unwrap();
            assert_eq!(surface.control_grid()[0][0], Point3::new(radius, 0., 0.));
        }
    }

    #[test]
    fn swept_and_spun_nodes_give_their_profile_and_frame() {
        use crate::brep::topology::nodes::sequence;
        use crate::test_support::parasolid::sequence::{
            sequence_of,
            F::{C, I, P, R, V},
            NULL_REAL,
        };
        let head = |profile: u16| vec![I(1), P(1), P(1), P(1), P(1), P(1), C(b'+'), P(profile)];
        let swept = |profile, direction| {
            let mut fields = head(profile);
            fields.extend([V(direction), R(NULL_REAL)]);
            fields
        };
        let mut spun = head(6);
        spun.extend([V([0.001, 0.0, 0.0]), V([0.0, -1.0, 0.0])]);
        spun.extend([V([NULL_REAL; 3]), V([NULL_REAL; 3])]);
        spun.extend([R(NULL_REAL), R(NULL_REAL), V([NULL_REAL; 3]), R(NULL_REAL)]);
        let (payload, body_offset) = sequence_of(&[
            (67, 9, swept(5, [0.0, 0.0, 1.0])),
            (67, 10, swept(5, [0.0, 0.0, 2.0])),
            (68, 12, spun),
        ]);
        let records = sequence(&payload).unwrap();
        let carriers = super::from_nodes(&records, body_offset).unwrap();
        assert_eq!(carriers.len(), 2);
        assert_eq!(carriers[&9].profile_attr, 5);
        let first = records.nodes().next().unwrap();
        assert_eq!(carriers[&9].offset, first.range().start - body_offset);
        let SweepKind::Swept { direction } = &carriers[&9].kind else {
            panic!("expected swept kind");
        };
        assert_eq!(*direction.as_raw(), Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(carriers[&12].profile_attr, 6);
        let SweepKind::Spun { base, axis, .. } = &carriers[&12].kind else {
            panic!("expected spun kind");
        };
        assert!((base.x - 1.0).abs() < 1.0e-12);
        assert_eq!(*axis.as_raw(), Vector3::new(0.0, -1.0, 0.0));
    }

    #[test]
    fn swept_nodes_without_a_sense_marker_or_with_a_repeated_index() {
        use crate::brep::topology::nodes::sequence;
        use crate::test_support::parasolid::sequence::{
            sequence_of,
            F::{C, I, P, R, V},
            NULL_REAL,
        };
        let swept = |sense| {
            vec![
                I(1),
                P(1),
                P(1),
                P(1),
                P(1),
                P(1),
                C(sense),
                P(5),
                V([0.0, 0.0, 1.0]),
                R(NULL_REAL),
            ]
        };
        let (payload, body_offset) = sequence_of(&[(67, 9, swept(b'?'))]);
        let records = sequence(&payload).unwrap();
        assert!(super::from_nodes(&records, body_offset).unwrap().is_empty());
        let (payload, body_offset) = sequence_of(&[(67, 9, swept(b'+')), (67, 9, swept(b'+'))]);
        let records = sequence(&payload).unwrap();
        assert!(super::from_nodes(&records, body_offset).is_none());
    }

    #[test]
    fn spun_nodes_keep_stored_profile_intersections() {
        use crate::test_support::parasolid::sequence::{
            sequence_of,
            F::{C, I, P, R, V},
            NULL_REAL,
        };
        let low = std::f64::consts::PI;
        let high = std::f64::consts::TAU;
        let fields = vec![
            I(1),
            P(1),
            P(1),
            P(1),
            P(1),
            P(1),
            C(b'+'),
            P(6),
            V([0.0; 3]),
            V([0.0, 0.0, 1.0]),
            V([0.0, 0.0, -0.007]),
            V([0.0, 0.0, 0.007]),
            R(low),
            R(high),
            V([1.0, 0.0, 0.0]),
            R(NULL_REAL),
        ];
        let (payload, offset) = sequence_of(&[(68, 12, fields)]);
        let records = crate::brep::topology::nodes::sequence(&payload).unwrap();
        let carriers = super::from_nodes(&records, offset).unwrap();
        let SweepKind::Spun { profile_bounds, .. } = carriers[&12].kind else {
            panic!("spun surface")
        };
        assert_eq!(profile_bounds, [Some(low), Some(high)]);
    }

    #[test]
    fn compact_spun_records_keep_present_and_absent_profile_bounds() {
        for bounds in [
            [Some(2.0), Some(5.0)],
            [Some(2.0), None],
            [None, Some(5.0)],
            [None, None],
        ] {
            let mut bytes = header(0x44, 12, 6);
            for value in [0.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0] {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
            for bound in bounds {
                let point = bound.map_or([super::ABSENT_PROFILE_PARAMETER; 3], |value| {
                    [0.0, 0.0, value]
                });
                for coordinate in point {
                    bytes.extend_from_slice(&coordinate.to_be_bytes());
                }
            }
            for bound in bounds {
                bytes.extend_from_slice(
                    &bound
                        .unwrap_or(super::ABSENT_PROFILE_PARAMETER)
                        .to_be_bytes(),
                );
            }
            let carriers = super::scan_sweep_carriers(&bytes);
            let SweepKind::Spun { profile_bounds, .. } = carriers[&12].kind else {
                panic!("spun surface")
            };
            assert_eq!(profile_bounds, bounds);
            if bounds[0].is_some() {
                let start = header(0x44, 12, 6).len() + 48;
                for lane in 0..3 {
                    bytes[start + lane * 8..start + (lane + 1) * 8]
                        .copy_from_slice(&super::ABSENT_PROFILE_PARAMETER.to_be_bytes());
                }
                assert!(super::scan_sweep_carriers(&bytes).is_empty());
            }
        }
    }

    #[test]
    fn a_bounded_conic_profile_keeps_the_selected_half_meridian() {
        let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
            cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                7.0,
                2.0,
            )
            .unwrap(),
        ));
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let low = std::f64::consts::PI;
        let high = std::f64::consts::TAU;
        let curve = profile_nurbs(
            &geometry,
            Some([low, high]),
            &"bounded meridian",
            &mut refusal,
        )
        .unwrap();
        assert_eq!(
            cadmpeg_ir::eval::nurbs_curve_parameter_domain(&curve)
                .unwrap()
                .endpoints(),
            [low, high]
        );
        assert_eq!(curve.pole_count(), 5);
        for (parameter, expected) in [
            (low, Point3::new(0.0, 0.0, -7.0)),
            ((low + high) * 0.5, Point3::new(-2.0, 0.0, 0.0)),
            (high, Point3::new(0.0, 0.0, 7.0)),
        ] {
            assert!(eval_curve(&curve, parameter).distance(expected) < 1.0e-12);
        }
        for range in [
            [high, low],
            [low, low],
            [0.0, 2.0 * high],
            [0.0, 4.0 * std::f64::consts::PI],
            [
                3.0 * std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
            ],
            [f64::NEG_INFINITY, high],
            [0.0, f64::INFINITY],
            [f64::NAN, high],
        ] {
            let curve =
                profile_nurbs(&geometry, Some(range), &"invalid meridian", &mut refusal).unwrap();
            assert_eq!(curve.pole_count(), 9);
            assert_eq!(
                cadmpeg_ir::eval::nurbs_curve_parameter_domain(&curve)
                    .unwrap()
                    .endpoints(),
                [0.0, high]
            );
        }
        let end = high + 1.0e-15;
        assert_eq!(
            super::stored_conic_interval(&geometry, [Some(0.0), Some(end)]),
            Some([0.0, end])
        );
        let curve = profile_nurbs(
            &geometry,
            Some([0.0, end]),
            &"roundoff meridian",
            &mut refusal,
        )
        .unwrap();
        assert_eq!(curve.pole_count(), 9);
        assert_eq!(
            cadmpeg_ir::eval::nurbs_curve_parameter_domain(&curve)
                .unwrap()
                .endpoints(),
            [0.0, end]
        );
        assert!(eval_curve(&curve, end).distance(eval_curve(&curve, 0.0)) < 1.0e-12);
        assert!(refusal.take_records().is_empty());
    }

    #[test]
    fn closed_boundary_carriers_bound_a_vertexless_swept_patch() {
        let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
            cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                Point3::new(0.0, 0.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                7.0,
                2.0,
            )
            .unwrap(),
        ));
        assert_eq!(
            super::curve_projection_bounds(&geometry, Vector3::new(0.0, 0.0, 1.0)),
            Some([3.0, 3.0])
        );
        assert_eq!(
            super::curve_projection_bounds(&geometry, Vector3::new(1.0, 0.0, 0.0)),
            Some([-7.0, 7.0])
        );
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let profile = profile_nurbs(&geometry, None, &"closed profile", &mut refusal).unwrap();
        let surface = swept_nurbs(
            &profile,
            SumSquaresUnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
            -1.0,
            4.0,
            true,
            &"closed ruling",
            &mut refusal,
        )
        .unwrap();
        assert!(surface.u_periodic());
        for v in [-1.0, 4.0] {
            let point = eval_surface(&surface, 0.0, v);
            assert!(point.distance(Point3::new(7.0, 0.0, 3.0 + v)) < 1.0e-12);
        }
    }

    #[test]
    fn spline_projection_requires_positive_convex_hull_weights() {
        for weights in [None, Some(vec![1.0, 2.0]), Some(vec![-1.0, -2.0])] {
            let positive = weights.as_ref().is_none_or(|values| values[0] > 0.0);
            let curve = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(0.0, 0.0, 2.0), Point3::new(1.0, 0.0, 5.0)],
                    weights,
                    false,
                )
                .unwrap(),
            ));
            assert_eq!(
                super::curve_projection_bounds(&curve, Vector3::new(0.0, 0.0, 1.0)),
                positive.then_some([2.0, 5.0])
            );
        }
    }
    #[test]
    fn nurbs_profile_keeps_its_whole_knot_domain() {
        let curve = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 2.0, 2.0],
            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 0.0, 2.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
        .unwrap();
        let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve.clone()));
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        for interval in [None, Some([0.5, 1.5]), Some([-1.0, 3.0])] {
            let profile =
                profile_nurbs(&geometry, interval, &"whole spline profile", &mut refusal).unwrap();
            assert_eq!(profile.as_ref(), &curve);
            assert!(matches!(profile, std::borrow::Cow::Borrowed(_)));
        }
        assert_eq!(
            super::stored_conic_interval(&geometry, [Some(0.5), Some(1.5)]),
            None
        );
        assert!(refusal.take_records().is_empty());
    }

    #[test]
    fn stored_conic_interval_requires_both_bounds() {
        let circle = cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .unwrap();
        let ellipse = cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            2.0,
        )
        .unwrap();
        for geometry in [
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)),
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse)),
        ] {
            let low = std::f64::consts::FRAC_PI_2;
            let high = std::f64::consts::PI;
            for bounds in [[Some(low), None], [None, Some(high)], [None, None]] {
                assert_eq!(super::stored_conic_interval(&geometry, bounds), None);
            }
            assert_eq!(
                super::stored_conic_interval(&geometry, [Some(low), Some(high)]),
                Some([low, high])
            );
        }
    }
}
