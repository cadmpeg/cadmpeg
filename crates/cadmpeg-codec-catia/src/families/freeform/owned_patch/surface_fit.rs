// SPDX-License-Identifier: Apache-2.0
//! Bounded spline fits of evaluated CATIA surface charts.

use std::collections::BTreeSet;

use cadmpeg_ir::eval::{self, admission::EvaluationAdmission};
use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::units::UnitVector3;

const DEGREE: usize = 3;
const FIT_ERROR_MM: f64 = 0.0002;
const MAX_SPANS: usize = 48;
const MAX_REFINEMENTS: usize = 9;
const HOLDOUTS: [f64; 3] = [0.2, 0.5, 0.8];

pub(super) fn offset_point(
    surface: &NurbsSurface,
    distance: f64,
    u: f64,
    v: f64,
) -> Option<Point3> {
    let partials =
        eval::nurbs_surface_partials(EvaluationAdmission::Standard, surface, u, v).ok()?;
    let normal =
        UnitVector3::normalized_by_largest_component(partials.du.get().cross(partials.dv.get()))?;
    Some(partials.point.get().translated(*normal.as_raw(), distance))
}

fn parameter([a, b]: [f64; 2], fraction: f64) -> f64 {
    if fraction == 0.0 {
        a
    } else if fraction == 1.0 {
        b
    } else {
        (b - a).mul_add(fraction, a)
    }
}

fn control_row([a, b, c, d]: [Point3; 4]) -> [Point3; 4] {
    let bc = |a: f64, b: f64, c: f64, d: f64| {
        let first_residual = 27.0 * (b - a) - (d - a);
        let second_residual = 27.0 * (c - a) - 8.0 * (d - a);
        [
            a + (2.0 * first_residual - second_residual) / 18.0,
            a + (2.0 * second_residual - first_residual) / 18.0,
        ]
    };
    let [x1, x2] = bc(a.x, b.x, c.x, d.x);
    let [y1, y2] = bc(a.y, b.y, c.y, d.y);
    let [z1, z2] = bc(a.z, b.z, c.z, d.z);
    [a, Point3::new(x1, y1, z1), Point3::new(x2, y2, z2), d]
}

type Cell = [[Point3; DEGREE + 1]; DEGREE + 1];

fn cell(evaluate: &impl Fn(f64, f64) -> Option<Point3>, u: [f64; 2], v: [f64; 2]) -> Option<Cell> {
    let sites = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    let mut rows = [[Point3::new(0.0, 0.0, 0.0); DEGREE + 1]; DEGREE + 1];
    for (row, f) in rows.iter_mut().zip(sites) {
        for (sample, g) in row.iter_mut().zip(sites) {
            *sample = evaluate(parameter(u, f), parameter(v, g))?;
        }
        *row = control_row(*row);
    }
    for j in 0..=DEGREE {
        let column = control_row(rows.map(|row| row[j]));
        for (row, pole) in rows.iter_mut().zip(column) {
            row[j] = pole;
        }
    }
    Some(rows)
}

fn evaluate_cell(poles: &Cell, u: f64, v: f64) -> Point3 {
    let lerp = |a: Point3, b: Point3, t| a.translated(b.vector_from(a), t);
    let curve = |[a, b, c, d]: [Point3; 4], t| {
        let [ab, bc, cd] = [lerp(a, b, t), lerp(b, c, t), lerp(c, d, t)];
        lerp(lerp(ab, bc, t), lerp(bc, cd, t), t)
    };
    curve(poles.map(|row| curve(row, v)), u)
}

fn seeds(knots: impl Iterator<Item = f64>, [lo, hi]: [f64; 2]) -> Vec<f64> {
    let mut points = vec![lo];
    for knot in knots {
        if knot > lo && knot < hi && points.last() != Some(&knot) {
            points.push(knot);
        }
    }
    points.push(hi);
    points
}

fn refined(points: &[f64], splits: &BTreeSet<usize>) -> Option<Vec<f64>> {
    let mut output = Vec::new();
    for (i, span) in points.windows(2).enumerate() {
        output.push(span[0]);
        if splits.contains(&i) {
            let mid = span[0].midpoint(span[1]);
            if mid <= span[0] || mid >= span[1] {
                return None;
            }
            output.push(mid);
        }
    }
    output.push(*points.last()?);
    (output.len() <= MAX_SPANS + 1).then_some(output)
}

fn spline_axis(points: &[f64]) -> NurbsSurfaceAxis {
    let mut knots = Vec::new();
    for (i, &t) in points.iter().enumerate() {
        let multiplicity = DEGREE + usize::from(i == 0 || i + 1 == points.len());
        knots.extend((0..multiplicity).map(|_| t));
    }
    NurbsSurfaceAxis::new(3, knots, false)
}

pub(super) fn offset(
    source: &NurbsSurface,
    distance: f64,
    [u, v]: [[f64; 2]; 2],
) -> Option<NurbsSurface> {
    fit(
        seeds(source.u_knots().iter().copied(), u),
        seeds(source.v_knots().iter().copied(), v),
        |u, v| offset_point(source, distance, u, v),
    )
}

pub(super) fn rolling(
    definition: &cadmpeg_ir::geometry::ProceduralSurfaceDefinition,
    [u, v]: [[f64; 2]; 2],
) -> Option<NurbsSurface> {
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet(jet) = definition else {
        return None;
    };
    fit(
        seeds(jet.stations().iter().map(|s| s.knot.get()), u),
        v.to_vec(),
        |u, v| {
            eval::rolling_ball_jet_point(definition, u, v)
                .ok()
                .map(cadmpeg_ir::features::FinitePoint3::get)
        },
    )
}

pub(super) fn bounded(
    u_knots: impl Iterator<Item = f64>,
    [u, v]: [[f64; 2]; 2],
    evaluate: impl Fn(f64, f64) -> Option<Point3>,
) -> Option<NurbsSurface> {
    fit(seeds(u_knots, u), v.to_vec(), evaluate)
}

fn fit(
    mut u_points: Vec<f64>,
    mut v_points: Vec<f64>,
    evaluate: impl Fn(f64, f64) -> Option<Point3>,
) -> Option<NurbsSurface> {
    for _ in 0..=MAX_REFINEMENTS {
        if u_points.len() > MAX_SPANS + 1 || v_points.len() > MAX_SPANS + 1 {
            return None;
        }
        let mut u_splits = BTreeSet::new();
        let mut v_splits = BTreeSet::new();
        let mut rows = Vec::new();
        for (i, u_span) in u_points.windows(2).enumerate() {
            let u = [u_span[0], u_span[1]];
            let mut strip: [Vec<_>; DEGREE + 1] = std::array::from_fn(|_| Vec::new());
            for (j, v_span) in v_points.windows(2).enumerate() {
                let v = [v_span[0], v_span[1]];
                let poles = cell(&evaluate, u, v)?;
                let holds = |s, t| {
                    Some(
                        evaluate(parameter(u, s), parameter(v, t))?
                            .distance(evaluate_cell(&poles, s, t))
                            <= FIT_ERROR_MM,
                    )
                };
                let mut split_u = false;
                let mut split_v = false;
                for t in HOLDOUTS {
                    split_u |= !holds(t, 0.0)? || !holds(t, 1.0)?;
                    split_v |= !holds(0.0, t)? || !holds(1.0, t)?;
                }
                if !split_u && !split_v {
                    for s in HOLDOUTS {
                        for t in HOLDOUTS {
                            if !holds(s, t)? {
                                split_u = true;
                                split_v = true;
                            }
                        }
                    }
                }
                if split_u {
                    u_splits.insert(i);
                }
                if split_v {
                    v_splits.insert(j);
                }
                for (row, segment) in strip.iter_mut().zip(poles) {
                    row.extend(segment.into_iter().skip(usize::from(j != 0)));
                }
            }
            rows.extend(strip.into_iter().skip(usize::from(i != 0)));
        }
        if u_splits.is_empty() && v_splits.is_empty() {
            return NurbsSurface::new(
                spline_axis(&u_points),
                spline_axis(&v_points),
                NurbsPoleGrid::Polynomial { rows },
                false,
            )
            .ok();
        }
        u_points = refined(&u_points, &u_splits)?;
        v_points = refined(&v_points, &v_splits)?;
    }
    None
}
