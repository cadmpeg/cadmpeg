// SPDX-License-Identifier: Apache-2.0
//! Parameter alignment for two occurrences of the same native edge.

use cadmpeg_ir::eval::{self, admission::EvaluationAdmission};
use cadmpeg_ir::geometry::nurbs::NurbsSurface;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::Point2;

use super::super::rolling_ball_patch::fit_edge_with_error;
use super::{curve_point, interval_parameter, Use, GAP_MM};

const LOCUS_STEPS: u32 = 64;
const PROJECTION_STEPS: usize = 48;
const UV_FIT_TOLERANCE: f64 = 1.0e-6;
const UV_FIT_FALLBACK: f64 = 1.0e-4;
const UV_FIT_PACING_FALLBACK: f64 = 1.0e-3;
const MAX_ENDPOINT_SEED_GAP_MM: f64 = 0.005;

pub(super) fn edge_relation(first: &Use, second: &Use) -> Option<bool> {
    let same =
        first.vertices[0].0 == second.vertices[0].0 && first.vertices[1].0 == second.vertices[1].0;
    let reversed =
        first.vertices[0].0 == second.vertices[1].0 && first.vertices[1].0 == second.vertices[0].0;
    (same != reversed && (first.forward != second.forward) == same).then_some(same)
}

fn evaluate_uv(pcurve: &PcurveGeometry, t: f64) -> Option<Point2> {
    eval::decode::pcurve_uv(EvaluationAdmission::Standard, pcurve, t)
        .ok()
        .map(cadmpeg_ir::units::FinitePoint2::get)
}

pub(super) fn align(first: &Use, second: &mut Use, surface: &SolvedSurfaceGeometry) -> Option<()> {
    align_with_tolerance(first, second, surface, GAP_MM)
}

pub(super) fn align_seed(
    first: &Use,
    second: &mut Use,
    surface: &SolvedSurfaceGeometry,
) -> Option<()> {
    align_with_tolerance(first, second, surface, MAX_ENDPOINT_SEED_GAP_MM)
}

fn align_with_tolerance(
    first: &Use,
    second: &mut Use,
    surface: &SolvedSurfaceGeometry,
    max_endpoint_gap: f64,
) -> Option<()> {
    let same = edge_relation(first, second)?;
    let [lo, hi] = first.curve.full_knot_endpoints().endpoints();
    let [s0, s1] = second.curve.full_knot_endpoints().endpoints();
    let [start, end] = if same { [s0, s1] } else { [s1, s0] };
    let second_at = |fraction: f64| interval_parameter([start, end], fraction);
    let eval_second = |fraction| curve_point(&second.curve, second_at(fraction));
    let samples = (0..=LOCUS_STEPS)
        .map(|i| eval_second(f64::from(i) / f64::from(LOCUS_STEPS)))
        .collect::<Option<Vec<_>>>()?;
    let project = |parameter: f64| {
        let point = curve_point(&first.curve, parameter)?;
        if parameter == lo {
            if point.distance(*samples.first()?) <= GAP_MM {
                return Some(0.0);
            }
            if point.distance(*samples.first()?) > max_endpoint_gap {
                return None;
            }
        }
        if parameter == hi {
            if point.distance(*samples.last()?) <= GAP_MM {
                return Some(1.0);
            }
            if point.distance(*samples.last()?) > max_endpoint_gap {
                return None;
            }
        }
        let mut ranked = Vec::new();
        for (i, pair) in samples.windows(2).enumerate() {
            let direction = pair[1].vector_from(pair[0]);
            let norm = direction.dot(direction);
            if norm <= 0.0 || !norm.is_finite() {
                return None;
            }
            let fraction = (point.vector_from(pair[0]).dot(direction) / norm).clamp(0.0, 1.0);
            let distance = point.distance(pair[0].translated(direction, fraction));
            ranked.push((i, distance));
        }
        let &(index, _) = ranked.iter().min_by(|(_, a), (_, b)| a.total_cmp(b))?;
        if ranked.iter().any(|&(i, distance)| {
            i.abs_diff(index) > 2
                && distance <= GAP_MM
                && i.checked_sub(1)
                    .and_then(|j| ranked.get(j))
                    .is_none_or(|&(_, d)| distance < d)
                && ranked.get(i + 1).is_none_or(|&(_, d)| distance <= d)
        }) {
            return None;
        }
        let denominator = f64::from(LOCUS_STEPS);
        let index = f64::from(u32::try_from(index).ok()?);
        let mut a = (index - 1.0).max(0.0) / denominator;
        let mut b = (index + 2.0).min(denominator) / denominator;
        for _ in 0..PROJECTION_STEPS {
            let left = a + (b - a) / 3.0;
            let right = b - (b - a) / 3.0;
            if point.distance(eval_second(left)?) <= point.distance(eval_second(right)?) {
                b = right;
            } else {
                a = left;
            }
        }
        let fraction = a.midpoint(b);
        (point.distance(eval_second(fraction)?) <= GAP_MM).then_some(fraction)
    };
    let parameter_at =
        |step: u32| interval_parameter([lo, hi], f64::from(step) / f64::from(LOCUS_STEPS));
    let mut previous = 0.0;
    for i in 1..LOCUS_STEPS {
        let fraction = project(parameter_at(i))?;
        if fraction <= previous || fraction >= 1.0 {
            return None;
        }
        previous = fraction;
    }
    let evaluate = |t| {
        let uv = evaluate_uv(&second.pcurve, second_at(project(t)?))?;
        Some([uv.u, uv.v, 0.0])
    };
    let curve = fit_edge_with_error([lo, hi], evaluate, UV_FIT_TOLERANCE)
        .or_else(|| fit_edge_with_error([lo, hi], evaluate, UV_FIT_FALLBACK))
        .or_else(|| fit_edge_with_error([lo, hi], evaluate, UV_FIT_PACING_FALLBACK))?;
    let poles = curve
        .control_points()
        .into_iter()
        .map(|p| {
            let p = p.get();
            Point2::new(p.x, p.y)
        })
        .collect();
    let nurbs =
        PcurveNurbs::from_lanes(curve.degree(), curve.knots().clone(), poles, None, false).ok()?;
    let pcurve = PcurveGeometry::Nurbs { nurbs };
    for i in 0..=LOCUS_STEPS {
        let t = parameter_at(i);
        let uv = evaluate_uv(&pcurve, t)?;
        let lift =
            eval::decode::surface_point_solved(EvaluationAdmission::Standard, surface, uv.u, uv.v)
                .ok()?
                .get();
        if lift.distance(curve_point(&first.curve, t)?) > GAP_MM {
            return None;
        }
    }
    second.curve = first.curve.clone();
    second.vertices = first.vertices;
    second.forward = second.forward == same;
    second.pcurve = pcurve;
    Some(())
}

pub(super) fn reproject(
    edge: &Use,
    occurrence: &Use,
    surface: &SolvedSurfaceGeometry,
) -> Option<Use> {
    let same = edge_relation(edge, occurrence)?;
    let [lo, hi] = occurrence.curve.full_knot_endpoints().endpoints();
    let occurrence_range = if same { [lo, hi] } else { [hi, lo] };
    let pcurve = match surface {
        SolvedSurfaceGeometry::Nurbs(spline) => project_spline(edge, spline, |fraction| {
            evaluate_uv(
                &occurrence.pcurve,
                interval_parameter(occurrence_range, fraction),
            )
        })?,
        analytic => {
            let seed_fn = |fraction| {
                evaluate_uv(
                    &occurrence.pcurve,
                    interval_parameter(occurrence_range, fraction),
                )
            };
            let range = edge.curve.full_knot_endpoints().endpoints();
            let project = |parameter| {
                let fraction =
                    cadmpeg_ir::math::parameter_fraction(parameter, range[0], range[1])?.get();
                let seed = seed_fn(fraction);
                let point = curve_point(&edge.curve, parameter)?;
                let mut uv = eval::analytic_surface_parameters_solved(analytic, point)?.get();
                if matches!(
                    analytic,
                    SolvedSurfaceGeometry::Cylinder(_)
                        | SolvedSurfaceGeometry::Cone(_)
                        | SolvedSurfaceGeometry::Sphere(_)
                        | SolvedSurfaceGeometry::Torus(_)
                ) {
                    if let Some(seed) = seed {
                        uv.u += ((seed.u - uv.u) / std::f64::consts::TAU).round()
                            * std::f64::consts::TAU;
                        if matches!(analytic, SolvedSurfaceGeometry::Torus(_)) {
                            uv.v += ((seed.v - uv.v) / std::f64::consts::TAU).round()
                                * std::f64::consts::TAU;
                        }
                    }
                }
                let lifted = eval::decode::surface_point_solved(
                    EvaluationAdmission::Standard,
                    surface,
                    uv.u,
                    uv.v,
                )
                .ok()?
                .get();
                (point.distance(lifted) <= GAP_MM).then_some([uv.u, uv.v, 0.0])
            };
            let fit = fit_edge_with_error(range, project, UV_FIT_TOLERANCE)
                .or_else(|| fit_edge_with_error(range, project, UV_FIT_FALLBACK))?;
            let poles = fit
                .control_points()
                .iter()
                .map(|point| {
                    let point = point.get();
                    Point2::new(point.x, point.y)
                })
                .collect();
            let pcurve = PcurveGeometry::Nurbs {
                nurbs: PcurveNurbs::from_lanes(
                    fit.degree(),
                    fit.knots().clone(),
                    poles,
                    None,
                    false,
                )
                .ok()?,
            };
            for i in 0..=LOCUS_STEPS {
                let parameter = interval_parameter(range, f64::from(i) / f64::from(LOCUS_STEPS));
                let uv = evaluate_uv(&pcurve, parameter)?;
                let lift = eval::decode::surface_point_solved(
                    EvaluationAdmission::Standard,
                    surface,
                    uv.u,
                    uv.v,
                )
                .ok()?
                .get();
                if lift.distance(curve_point(&edge.curve, parameter)?) > GAP_MM {
                    return None;
                }
            }
            pcurve
        }
    };
    Some(Use {
        pcurve,
        forward: occurrence.forward == same,
        ..edge.clone()
    })
}

pub(super) fn project_spline(
    edge: &Use,
    spline: &NurbsSurface,
    seed: impl Fn(f64) -> Option<Point2>,
) -> Option<PcurveGeometry> {
    let range = edge.curve.full_knot_endpoints().endpoints();
    let project = |parameter| {
        let fraction = cadmpeg_ir::math::parameter_fraction(parameter, range[0], range[1])?.get();
        let seed = seed(fraction)?;
        let point = curve_point(&edge.curve, parameter)?;
        let uv = eval::nurbs_surface_parameter_near_point(
            EvaluationAdmission::Standard,
            spline,
            point,
            Some(seed),
        )
        .ok()??
        .get();
        let lifted =
            eval::decode::nurbs_surface_point(EvaluationAdmission::Standard, spline, uv.u, uv.v)
                .ok()?
                .get();
        (point.distance(lifted) <= GAP_MM).then_some([uv.u, uv.v, 0.0])
    };
    let fit = fit_edge_with_error(range, project, UV_FIT_TOLERANCE)
        .or_else(|| fit_edge_with_error(range, project, UV_FIT_FALLBACK))?;
    let poles = fit
        .control_points()
        .iter()
        .map(|point| {
            let point = point.get();
            Point2::new(point.x, point.y)
        })
        .collect();
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(fit.degree(), fit.knots().clone(), poles, None, false)
            .ok()?,
    };
    for i in 0..=LOCUS_STEPS {
        let parameter = interval_parameter(range, f64::from(i) / f64::from(LOCUS_STEPS));
        let uv = evaluate_uv(&pcurve, parameter)?;
        let lift =
            eval::decode::nurbs_surface_point(EvaluationAdmission::Standard, spline, uv.u, uv.v)
                .ok()?
                .get();
        if lift.distance(curve_point(&edge.curve, parameter)?) > GAP_MM {
            return None;
        }
    }
    Some(pcurve)
}

#[cfg(test)]
mod tests {
    use super::*;

    mod recovery;
    use cadmpeg_ir::geometry::{analytic::PlaneSurface, nurbs::NurbsCurve};
    use cadmpeg_ir::math::{Point3, Vector3};

    fn edge(points: Vec<Point2>, forward: bool) -> Use {
        let degree = u32::try_from(points.len() - 1).expect("valid test edge or pcurve");
        let knots = [0.0, 1.0]
            .into_iter()
            .flat_map(|t| std::iter::repeat_n(t, points.len()))
            .collect::<Vec<_>>();
        let lift = |p: &Point2| Point3::new(p.u, p.v, 0.0);
        let curve = NurbsCurve::from_lanes(
            degree,
            knots.clone(),
            points.iter().map(lift).collect(),
            None,
            false,
        )
        .expect("valid test edge or pcurve");
        let vertices = [
            (1, lift(&points[0])),
            (2, lift(points.last().expect("valid test edge or pcurve"))),
        ];
        let nurbs = PcurveNurbs::from_lanes(degree, knots, points, None, false)
            .expect("valid test edge or pcurve");
        Use {
            edge: 0,
            vertices,
            curve,
            pcurve: PcurveGeometry::Nurbs { nurbs },
            forward,
        }
    }

    #[test]
    fn seed_alignment_shortens_an_overextended_edge() {
        let plane = SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("fixture plane"),
        );
        let native = edge(vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)], true);
        let mut long = edge(vec![Point2::new(0.0, 0.0), Point2::new(1.003, 0.0)], false);
        long.vertices = native.vertices;

        assert!(align(&native, &mut long, &plane).is_none());
        align_seed(&native, &mut long, &plane).expect("align the corroborated trim");
        let end = evaluate_uv(&long.pcurve, 1.0).expect("aligned trim endpoint");
        assert!((end.u - 1.0).abs() < UV_FIT_TOLERANCE);
    }

    #[test]
    fn shared_edge_alignment_accepts_different_pacing_and_refuses_different_loci() {
        let plane = SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid test edge or pcurve"),
        );
        let first = edge(vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)], true);
        let mut same = edge(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(0.25, 0.0),
                Point2::new(1.0, 0.0),
            ],
            false,
        );
        assert!(
            (evaluate_uv(&same.pcurve, 0.5)
                .expect("valid test edge or pcurve")
                .u
                - 0.5)
                .abs()
                > GAP_MM
        );
        align(&first, &mut same, &plane).expect("valid test edge or pcurve");
        assert!(
            (evaluate_uv(&same.pcurve, 0.5)
                .expect("valid test edge or pcurve")
                .u
                - 0.5)
                .abs()
                <= UV_FIT_TOLERANCE
        );
        assert!(!same.forward);
        let mut different = edge(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(0.25, 0.1),
                Point2::new(1.0, 0.0),
            ],
            false,
        );
        assert!(align(&first, &mut different, &plane).is_none());
    }
}
