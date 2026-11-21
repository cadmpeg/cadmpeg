// SPDX-License-Identifier: Apache-2.0
//! Native conic trim parameters and their STEP spline representation.

use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::topology::IncreasingParameterInterval;

use crate::families::b2::records::B2Circle;
use crate::layout::sphere_pcurve_coordinate_selector as sphere_selector;
use crate::wire::bytes::compact_int;

use super::super::rolling_ball_patch::fit_edge_with_error;
use super::near;

const CHART_FIT_ERROR: f64 = 1.0e-7;

fn fit_curve(
    range: [f64; 2],
    evaluate: impl Fn(f64) -> [f64; 2],
) -> Option<(PcurveGeometry, [f64; 2])> {
    let curve = fit_edge_with_error(
        range,
        |t| {
            let [u, v] = evaluate(t);
            (u.is_finite() && v.is_finite()).then_some([u, v, 0.0])
        },
        CHART_FIT_ERROR,
    )?;
    let points = curve
        .control_points()
        .into_iter()
        .map(|point| {
            let p = point.get();
            Point2::new(p.x, p.y)
        })
        .collect();
    let nurbs =
        PcurveNurbs::from_lanes(curve.degree(), curve.knots().clone(), points, None, false).ok()?;
    Some((PcurveGeometry::Nurbs { nurbs }, range))
}

pub(super) fn circle(circle: &B2Circle) -> Option<(PcurveGeometry, [f64; 2])> {
    let [u, v] = circle.center_pair.get();
    fit_circle([u, v], circle.radius.get(), circle.range.endpoints())
}

pub(super) fn owned_circle(bytes: &[u8]) -> Option<(PcurveGeometry, [f64; 2])> {
    let mut at = 0;
    compact_int(bytes, &mut at)?;
    let mut view = View::over_retained(bytes.get(at..)?);
    let center = [view.f64_le()?, view.f64_le()?];
    match view.u8()? {
        0x05 if view.u8()? == 0x05 => {}
        0x0d => {}
        _ => return None,
    }
    let radius = PositiveReal::new(view.f64_le()?)?.get();
    let range = IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?;
    if view.u8()? != 1 || !view.f64_le()?.is_finite() || !view.is_empty() {
        return None;
    }
    fit_circle(center, radius, range.endpoints())
}

fn fit_circle([u, v]: [f64; 2], r: f64, range: [f64; 2]) -> Option<(PcurveGeometry, [f64; 2])> {
    fit_curve(range, |t| {
        let (sin, cos) = (t / r).sin_cos();
        [r.mul_add(cos, u), r.mul_add(sin, v)]
    })
}

#[derive(Clone, Copy)]
pub(super) struct SphereCircle {
    pub(super) range: [f64; 2],
    pub(super) equator_reflected: bool,
    radius: f64,
    slope: f64,
    phase: f64,
    shift: f64,
    direction: f64,
}

impl SphereCircle {
    pub(super) fn chart_station(self, station: f64) -> f64 {
        if self.equator_reflected {
            self.direction.mul_add(station, self.shift)
        } else {
            station
        }
    }

    pub(super) fn native_range(self) -> Option<[f64; 2]> {
        if !self.equator_reflected {
            return Some(self.range);
        }
        let mut range = self.range.map(|u| (u - self.shift) / self.direction);
        if self.direction < 0.0 {
            range.reverse();
        }
        IncreasingParameterInterval::new(range).map(IncreasingParameterInterval::endpoints)
    }

    pub(super) fn fit(self, range: [f64; 2]) -> Option<PcurveGeometry> {
        fit_curve(range, |t| {
            [
                t,
                self.radius * (self.slope * (t / self.radius - self.phase).cos()).atan(),
            ]
        })
        .map(|(curve, _)| curve)
    }
}

pub(super) fn sphere_circle(bytes: &[u8], radius: f64) -> Option<SphereCircle> {
    let mut at = 0;
    compact_int(bytes, &mut at)?;
    let mut view = View::over_retained(bytes.get(at..)?);
    let u = IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?;
    let v = IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?;
    let mut prefix = view;
    let reflected = prefix.take(sphere_selector::TAG_VALUE.len())? == sphere_selector::TAG_VALUE;
    let mut circle = if reflected {
        sphere_coordinate(&mut view, [1.0, 0.0, 0.0])?;
        let mut first = view;
        let circle = sphere_equation(&mut view, u.endpoints(), radius, v.upper() - v.lower())?;
        let length = view.position() - first.position();
        sphere_coordinate(&mut view, [0.0, -1.0, 0.0])?;
        let mut second = view;
        sphere_equation(&mut view, u.endpoints(), radius, v.upper() - v.lower())?;
        if view.position() - second.position() != length
            || first.take(length)? != second.take(length)?
        {
            return None;
        }
        circle
    } else {
        sphere_equation(&mut view, u.endpoints(), radius, v.upper() - v.lower())?
    };
    if !view.is_empty() {
        return None;
    }
    if reflected {
        circle.slope = -circle.slope;
        circle.equator_reflected = true;
    }
    Some(circle)
}

fn sphere_coordinate(view: &mut View<'_>, coefficients: [f64; 3]) -> Option<()> {
    if view.take(sphere_selector::TAG_VALUE.len())? != sphere_selector::TAG_VALUE {
        return None;
    }
    for coefficient in coefficients {
        if view.f64_le()? != coefficient {
            return None;
        }
    }
    Some(())
}

fn sphere_equation(
    view: &mut View<'_>,
    range: [f64; 2],
    radius: f64,
    latitude_span: f64,
) -> Option<SphereCircle> {
    if view.take(3)? != [0x41, 0x05, 0x81] {
        return None;
    }
    let shift = view.f64_le()?;
    let direction = view.f64_le()?;
    if direction.abs() != 1.0 || view.f64_le()? != 0.0 || view.take(2)? != [0x41, 0x1d] {
        return None;
    }
    let scale = PositiveReal::new(view.f64_le()?)?.get();
    let slope = view.f64_le()?;
    let reciprocal = view.f64_le()?;
    let phase = view.f64_le()? + shift / scale;
    if !near(scale, radius)
        || !near(reciprocal, -direction / scale)
        || !near(latitude_span, std::f64::consts::TAU * scale)
        || view.f64_le()? != 0.0
        || !phase.is_finite()
        || !slope.is_finite()
    {
        return None;
    }
    Some(SphereCircle {
        range,
        equator_reflected: false,
        radius: scale,
        slope,
        phase,
        shift,
        direction,
    })
}

pub(super) fn spline(bytes: &[u8]) -> Option<(PcurveGeometry, [f64; 2])> {
    let mut at = 0;
    compact_int(bytes, &mut at)?;
    let mut view = View::over_retained(bytes.get(at..)?);
    let inline = |view: &mut View<'_>| {
        let token = view.u8()?;
        (token % 4 == 1).then_some(u32::from(token / 4))
    };
    let kind = view.u8()?;
    if kind == 0x0e {
        return explicit_domain_cubic(&mut view);
    }
    let spans = match kind {
        0x1e | 0x3e | 0x7e => 1,
        0x1a | 0x3a => usize::try_from(inline(&mut view)?).ok()?,
        0x16 => {
            let center = view.f64_le()?;
            if !center.is_finite() || center <= 0.0 {
                return None;
            }
            1
        }
        _ => return None,
    };
    let stored_span = view.f64_le()?;
    let degree = inline(&mut view)?;
    let count = inline(&mut view)?;
    if !matches!(
        (kind, degree),
        (0x1e, 1 | 4 | 5) | (0x1a | 0x3a | 0x3e | 0x7e, 5) | (0x16, 11)
    ) || count < 2
        || spans == 0
    {
        return None;
    }
    let extended = matches!(kind, 0x3a | 0x3e | 0x7e);
    let leading = match (extended, view.u8()?) {
        (true, 0x08) => usize::try_from(inline(&mut view)?).ok()?,
        (_, 0x0c) => 0,
        _ => return None,
    };
    let distinct = (0..count)
        .map(|_| view.f64_le())
        .collect::<Option<Vec<_>>>()?;
    if !cadmpeg_ir::geometry::nurbs::knots_strictly_increasing(&distinct) {
        return None;
    }

    let points = (0..degree + 1 + 3 * (count - 2))
        .map(|_| Some(Point2::new(view.f64_le()?, view.f64_le()?)))
        .collect::<Option<Vec<_>>>()?;
    let last = distinct.len() - 1;
    let trailing = if extended {
        usize::try_from(inline(&mut view)?).ok()?
    } else {
        0
    };
    if kind == 0x7e && inline(&mut view)? != 1 {
        return None;
    }
    let stop = leading.checked_add(spans)?;
    if stop.checked_add(trailing)? != last
        || !near(distinct[stop] - distinct[stop - 1], stored_span)
        || !view.is_empty()
    {
        return None;
    }
    let range = [distinct[leading], distinct[stop]];
    let knots: Vec<_> = distinct
        .into_iter()
        .enumerate()
        .flat_map(|(i, t)| {
            let multiplicity = if i == 0 || i == last { degree + 1 } else { 3 };
            (0..multiplicity).map(move |_| t)
        })
        .collect();
    let nurbs = PcurveNurbs::from_lanes(degree, knots, points, None, false).ok()?;
    Some((PcurveGeometry::Nurbs { nurbs }, range))
}

fn explicit_domain_cubic(view: &mut View<'_>) -> Option<(PcurveGeometry, [f64; 2])> {
    let span = view.f64_le()?;
    let upper = view.f64_le()?;
    let lower = view.f64_le()?;
    let range = IncreasingParameterInterval::new([lower, upper])?.endpoints();
    if !near(upper - lower, span) || view.take(3)? != [0x0d, 0x09, 0x0e] {
        return None;
    }
    if view.f64_le()? != lower || view.f64_le()? != upper {
        return None;
    }
    let points = (0..4)
        .map(|_| Some(Point2::new(view.f64_le()?, view.f64_le()?)))
        .collect::<Option<Vec<_>>>()?;
    if !view.is_empty() {
        return None;
    }
    let knots: Vec<_> = range.into_iter().flat_map(|t| [t; 4]).collect();
    let nurbs = PcurveNurbs::from_lanes(3, knots, points, None, false).ok()?;
    Some((PcurveGeometry::Nurbs { nurbs }, range))
}

#[cfg(test)]
mod tests {
    use super::{owned_circle, sphere_circle, spline, CHART_FIT_ERROR};
    use cadmpeg_ir::eval::admission::EvaluationAdmission;
    use cadmpeg_ir::eval::decode::pcurve_uv;

    #[test]
    fn explicit_domain_cubic_uses_the_stored_interval() {
        let mut bytes = vec![5, 0x0e];
        for value in [2.0_f64, 5.0, 3.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([0x0d, 0x09, 0x0e]);
        for value in [3.0_f64, 5.0, 0.0, 2.0, 1.0, 2.0, 2.0, 2.0, 3.0, 2.0] {
            bytes.extend(value.to_le_bytes());
        }
        let (curve, range) = spline(&bytes).expect("bounded trim fixture");
        assert_eq!(range, [3.0, 5.0]);
        let midpoint = pcurve_uv(EvaluationAdmission::Standard, &curve, 4.0)
            .expect("bounded trim fixture")
            .get();
        assert!((midpoint.u - 1.5).abs() < CHART_FIT_ERROR);
        assert!((midpoint.v - 2.0).abs() < CHART_FIT_ERROR);

        bytes.push(0);
        assert!(spline(&bytes).is_none());
    }

    #[test]
    fn extended_spline_keeps_poles_outside_the_active_trim() {
        for (header, marker, knots, suffix, active) in [
            (
                &b"\x3a\x09"[..],
                &b"\x08\x05"[..],
                &[-2.0_f64, 0.0, 0.5, 1.5, 3.0][..],
                &b"\x05"[..],
                [0.0, 1.5],
            ),
            (
                &b"\x3e"[..],
                &b"\x0c"[..],
                &[0.0_f64, 1.0, 3.0][..],
                &b"\x05"[..],
                [0.0, 1.0],
            ),
            (
                &b"\x3e"[..],
                &b"\x08\x01"[..],
                &[0.0_f64, 1.0, 3.0][..],
                &b"\x05"[..],
                [0.0, 1.0],
            ),
            (
                &b"\x7e"[..],
                &b"\x08\x09"[..],
                &[-3.0, -1.0, 0.0, 1.0][..],
                &b"\x01\x05"[..],
                [0.0, 1.0],
            ),
        ] {
            let mut bytes = vec![5];
            bytes.extend(header);
            bytes.extend(1.0_f64.to_le_bytes());
            bytes.extend([
                21,
                u8::try_from(4 * knots.len() + 1).expect("bounded trim fixture"),
            ]);
            bytes.extend(marker);
            for knot in knots {
                bytes.extend(knot.to_le_bytes());
            }
            for index in 0..6 + 3 * (knots.len() - 2) {
                bytes.extend(
                    f64::from(u32::try_from(index).expect("bounded trim fixture")).to_le_bytes(),
                );
                bytes.extend(0.0_f64.to_le_bytes());
            }
            bytes.extend(suffix);
            let (curve, range) = spline(&bytes).expect("extended trim");
            assert_eq!(range, active);
            for endpoint in [knots[0], knots[knots.len() - 1]] {
                assert!(pcurve_uv(EvaluationAdmission::Standard, &curve, endpoint).is_ok());
            }
        }
    }

    #[test]
    fn extended_single_span_requires_its_terminal_control() {
        let mut bytes = vec![5, 0x7e];
        bytes.extend(1.0_f64.to_le_bytes());
        bytes.extend([21, 9, 0x0c]);
        for knot in [0.0_f64, 1.0] {
            bytes.extend(knot.to_le_bytes());
        }
        for index in 0..6 {
            bytes.extend(f64::from(index).to_le_bytes());
            bytes.extend(0.0_f64.to_le_bytes());
        }
        bytes.extend([1, 5]);
        assert!(spline(&bytes).is_some());
        for control in [1, 9, 0xfd] {
            *bytes.last_mut().expect("terminal control") = control;
            assert!(spline(&bytes).is_none());
        }
        bytes.pop();
        assert!(spline(&bytes).is_none());
    }

    #[test]
    fn owned_circle_keeps_its_chart_across_compact_operand_widths() {
        let operands: &[&[u8]] = &[
            &[5],
            &[4, 64],
            &[8, 0, 1],
            &[12, 0, 0, 1],
            &[16, 0, 0, 0, 1],
        ];
        let markers: &[&[u8]] = &[&[0x0d], &[0x05, 0x05]];
        for operand in operands {
            for marker in markers {
                let mut bytes = operand.to_vec();
                for value in [3.0_f64, -2.0] {
                    bytes.extend(value.to_le_bytes());
                }
                bytes.extend_from_slice(marker);
                for value in [2.0_f64, 0.5, 2.5] {
                    bytes.extend(value.to_le_bytes());
                }
                bytes.push(1);
                bytes.extend(0.0_f64.to_le_bytes());
                let (curve, range) = owned_circle(&bytes).expect("complete circle");
                assert_eq!(range, [0.5, 2.5]);
                for t in [0.5_f64, 1.5, 2.5] {
                    let point = pcurve_uv(EvaluationAdmission::Standard, &curve, t)
                        .expect("circle point")
                        .get();
                    let (sin, cos) = (t / 2.0).sin_cos();
                    assert!(
                        (point.u - (3.0 + 2.0 * cos)).hypot(point.v - (-2.0 + 2.0 * sin))
                            <= CHART_FIT_ERROR
                    );
                }
                bytes.push(0);
                assert!(owned_circle(&bytes).is_none());
            }
        }
    }

    #[test]
    fn owned_circle_requires_a_complete_marked_layout() {
        let mut prefix = vec![5];
        for value in [3.0_f64, -2.0] {
            prefix.extend(value.to_le_bytes());
        }
        let mut tail = Vec::new();
        for value in [2.0_f64, 0.5, 2.5] {
            tail.extend(value.to_le_bytes());
        }
        tail.push(1);
        tail.extend(0.0_f64.to_le_bytes());
        let mut unmarked = prefix.clone();
        unmarked.extend_from_slice(&tail);
        assert!(owned_circle(&unmarked).is_none());
        for marker in [&[0x0d][..], &[0x05, 0x05][..]] {
            let mut corrupt = prefix.clone();
            let mut truncated_tail = tail.clone();
            truncated_tail[..marker.len()].copy_from_slice(marker);
            corrupt.extend(truncated_tail);
            assert!(owned_circle(&corrupt).is_none());
        }
    }

    fn equation(slope: f64, direction: f64) -> Vec<u8> {
        let mut bytes = vec![0x41, 0x05, 0x81];
        for value in [0.6_f64, direction, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([0x41, 0x1d]);
        for value in [2.0_f64, slope, -direction / 2.0, 0.25, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes
    }

    fn sphere_trim(reflected: bool, second_slope: f64, direction: f64) -> Vec<u8> {
        let mut bytes = vec![5];
        for value in [-2.0_f64, 3.0, -std::f64::consts::TAU, std::f64::consts::TAU] {
            bytes.extend(value.to_le_bytes());
        }
        if reflected {
            bytes.extend([0x41, 0x11, 0x82]);
            for value in [1.0_f64, 0.0, 0.0] {
                bytes.extend(value.to_le_bytes());
            }
        }
        bytes.extend(equation(0.75, direction));
        if reflected {
            bytes.extend([0x41, 0x11, 0x82]);
            for value in [0.0_f64, -1.0, 0.0] {
                bytes.extend(value.to_le_bytes());
            }
            bytes.extend(equation(second_slope, direction));
        }
        bytes
    }

    #[test]
    fn sphere_circle_coordinate_reflection_lifts_to_the_reflected_great_circle() {
        for reflected in [false, true] {
            for direction in [-1.0, 1.0] {
                let bytes = sphere_trim(reflected, 0.75, direction);
                let circle = sphere_circle(&bytes, 2.0).expect("complete sphere expression");
                let curve = circle.fit(circle.range).expect("fitted sphere trim");
                let sign = if reflected { -1.0 } else { 1.0 };
                for sample in 0..=40 {
                    let t = -2.0 + 5.0 * f64::from(sample) / 40.0;
                    let uv = pcurve_uv(EvaluationAdmission::Standard, &curve, t)
                        .expect("finite chart point")
                        .get();
                    let azimuth = uv.u / 2.0;
                    let latitude = uv.v / 2.0;
                    let expected = sign * (0.75 * (t / 2.0 - 0.55).cos()).atan();
                    assert!((uv.u - t).abs() < 1.0e-7);
                    assert!((latitude - expected).abs() < 1.0e-7);
                    let x = latitude.cos() * azimuth.cos();
                    let y = latitude.cos() * azimuth.sin();
                    let z = latitude.sin();
                    let plane = z - sign * 0.75 * (0.55_f64.cos() * x + 0.55_f64.sin() * y);
                    assert!(plane.abs() < 1.0e-7);
                }
            }
        }
    }

    #[test]
    fn composed_sphere_stations_preserve_the_intrinsic_circle_equation() {
        for direction in [-1.0, 1.0] {
            let circle = sphere_circle(&sphere_trim(true, 0.75, direction), 2.0)
                .expect("composed sphere expression");
            let curve = circle.fit(circle.range).expect("fitted sphere trim");
            let [lo, hi] = circle.native_range().expect("intrinsic interval");
            let expected_range = if direction > 0.0 {
                [-2.6, 2.4]
            } else {
                [-2.4, 2.6]
            };
            assert_eq!([lo, hi], expected_range);
            for sample in 0..=40 {
                let station = (hi - lo).mul_add(f64::from(sample) / 40.0, lo);
                let u = circle.chart_station(station);
                let point = pcurve_uv(EvaluationAdmission::Standard, &curve, u)
                    .expect("finite chart point")
                    .get();
                let expected_u = 0.6 + direction * station;
                let expected_v = -2.0 * (0.75 * (-direction * station / 2.0 + 0.25).cos()).atan();
                assert!((point.u - expected_u).abs() < 1.0e-7);
                assert!((point.v - expected_v).abs() < 1.0e-7);
            }
        }
    }

    #[test]
    fn sphere_circle_reflection_requires_equal_complete_operands() {
        assert!(sphere_circle(&sphere_trim(true, 0.8, 1.0), 2.0).is_none());
        let bytes = sphere_trim(true, 0.75, 1.0);
        for length in 0..bytes.len() {
            assert!(sphere_circle(&bytes[..length], 2.0).is_none());
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(sphere_circle(&trailing, 2.0).is_none());
    }

    #[test]
    fn class21_spline_uses_the_stored_knot_origin() {
        let mut bytes = vec![0x05, 0x1e];
        bytes.extend(3.0_f64.to_le_bytes());
        bytes.extend([0x05, 0x09, 0x0c]);
        for value in [-1.0_f64, 2.0, 0.0, 0.0, 1.0, 1.0] {
            bytes.extend(value.to_le_bytes());
        }
        let (_, range) = spline(&bytes).expect("valid fixture trim");
        assert_eq!(range, [-1.0, 2.0]);
    }
}
