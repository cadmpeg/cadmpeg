// SPDX-License-Identifier: Apache-2.0
//! Bounded polynomial approximation of a native rolling-ball face carrier.

use std::cell::OnceCell;

use super::parameter_cache::memoize;

use cadmpeg_core::convert::f64_from_index;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{self, admission::EvaluationAdmission};
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::Point3;

use crate::wire::records::{ConsolidatedFamily, ConsolidatedRecord};

const U_DEGREE: usize = 3;
const V_DEGREE: usize = 5;
const HOLDOUT_STEPS: usize = 20;
const MAX_FIT_ERROR_MM: f64 = 0.001;
const MIN_PIVOT: f64 = 1.0e-12;
const LOOP_CLOSURE_MM: f64 = 0.001;
const LOOP_ORIENTATION_SAMPLES: usize = 40;
const MIN_LOOP_UV_AREA: f64 = 1.0e-10;
const MAX_PCURVE_CANDIDATES: usize = 256;
const MAX_CHART_ENDPOINT_SLACK: f64 = 0.00001;
const EDGE_DEGREE: usize = 5;
const EDGE_HOLDOUT_STEPS: usize = 20;
const MAX_EDGE_ERROR_MM: f64 = 0.001;
const MAX_EDGE_SPLIT_DEPTH: usize = 12;
const MAX_EDGE_SPANS: usize = 128;

fn surface_evaluator<'a>(
    ir: &'a CadIr,
    carrier: &'a SurfaceGeometry,
) -> impl Fn(f64, f64) -> Option<[f64; 3]> + 'a {
    let index = OnceCell::new();
    move |u, v| {
        let point =
            if carrier.solved_cache().is_some() || carrier.procedural_construction().is_none() {
                eval::model_surface_point(EvaluationAdmission::Standard, ir, carrier, u, v)
            } else {
                eval::model_surface_point_in_index(
                    EvaluationAdmission::Standard,
                    index.get_or_init(|| ModelIndex::build(ir)),
                    carrier,
                    u,
                    v,
                )
            }
            .ok()?
            .get();
        Some([point.x, point.y, point.z])
    }
}

#[derive(Clone)]
pub(super) struct ClosedPcurve {
    pub(super) pos: usize,
    pub(super) geometry: PcurveGeometry,
    pub(super) range: [f64; 2],
    endpoints: [([f64; 2], [f64; 3]); 2],
}

pub(super) struct ClosedPatch {
    pub(super) u_range: [f64; 2],
    pub(super) v_range: [f64; 2],
    pub(super) edges: [ClosedPcurve; 2],
}

impl ClosedPatch {
    pub(super) fn shared_vertices(&self) -> Option<([Point3; 2], bool)> {
        let first = &self.edges[0].endpoints;
        let second = &self.edges[1].endpoints;
        let same_order = endpoint_relation([first[0].1, first[1].1], [second[0].1, second[1].1])?;
        Some((
            first.map(|(_, position)| Point3::new(position[0], position[1], position[2])),
            same_order,
        ))
    }

    pub(super) fn loop_is_forward(&self, same_order: bool) -> Option<bool> {
        let mut points = Vec::new();
        for (index, edge) in self.edges.iter().enumerate() {
            for step in 0..LOOP_ORIENTATION_SAMPLES {
                let fraction = f64_from_index(step)? / f64_from_index(LOOP_ORIENTATION_SAMPLES)?;
                let fraction = if index == 1 && same_order {
                    1.0 - fraction
                } else {
                    fraction
                };
                let parameter = edge.range[0] + (edge.range[1] - edge.range[0]) * fraction;
                let uv = eval::decode::pcurve_uv(
                    EvaluationAdmission::Standard,
                    &edge.geometry,
                    parameter,
                )
                .ok()?
                .get();
                points.push([uv.u, uv.v]);
            }
        }
        let area_twice = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .take(points.len())
            .map(|(first, second)| first[0] * second[1] - second[0] * first[1])
            .sum::<f64>();
        (area_twice.is_finite() && area_twice.abs() > 2.0 * MIN_LOOP_UV_AREA)
            .then_some(area_twice > 0.0)
    }
}

fn endpoint_relation(first: [[f64; 3]; 2], second: [[f64; 3]; 2]) -> Option<bool> {
    let distance = |left: &[f64; 3], right: &[f64; 3]| {
        (0..3)
            .map(|axis| (left[axis] - right[axis]).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let same = distance(&first[0], &second[0]).max(distance(&first[1], &second[1]));
    let opposite = distance(&first[0], &second[1]).max(distance(&first[1], &second[0]));
    (same.min(opposite) < LOOP_CLOSURE_MM).then_some(same <= opposite)
}

fn chart_v(value: f64, bounds: Option<[f64; 2]>) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    let Some([lower, upper]) = bounds else {
        return Some(value);
    };
    if value < lower - MAX_CHART_ENDPOINT_SLACK || value > upper + MAX_CHART_ENDPOINT_SLACK {
        return None;
    }
    Some(value.clamp(lower, upper))
}

fn bernstein(degree: usize, t: f64) -> Option<Vec<f64>> {
    let mut row = Vec::with_capacity(degree + 1);
    let mut coefficient = 1.0;
    for index in 0..=degree {
        let power = i32::try_from(index).ok()?;
        let complement = i32::try_from(degree - index).ok()?;
        row.push(coefficient * t.powi(power) * (1.0 - t).powi(complement));
        if index < degree {
            coefficient *= f64::from(complement) / f64_from_index(index + 1)?;
        }
    }
    Some(row)
}

fn solve_columns(matrix: &[Vec<f64>], values: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let count = values.len();
    let columns = values.first()?.len();
    if matrix.len() != count
        || matrix.iter().any(|row| row.len() != count)
        || values.iter().any(|row| row.len() != columns)
    {
        return None;
    }
    let mut rows = matrix
        .iter()
        .zip(values)
        .map(|(row, value)| {
            let mut augmented = row.clone();
            augmented.extend(value);
            augmented
        })
        .collect::<Vec<_>>();
    for column in 0..count {
        let pivot = (column..count).max_by(|&left, &right| {
            rows[left][column]
                .abs()
                .total_cmp(&rows[right][column].abs())
        })?;
        if rows[pivot][column].abs() < MIN_PIVOT {
            return None;
        }
        rows.swap(column, pivot);
        let scale = rows[column][column];
        for value in &mut rows[column] {
            *value /= scale;
        }
        let (before, tail) = rows.split_at_mut(column);
        let (pivot_row, after) = tail.split_first_mut()?;
        for row in before.iter_mut().chain(after) {
            let factor = row[column];
            for (entry, pivot_entry) in row[column..].iter_mut().zip(&pivot_row[column..]) {
                *entry -= factor * pivot_entry;
            }
        }
    }
    Some(rows.into_iter().map(|row| row[count..].to_vec()).collect())
}

pub(super) fn fit(
    ir: &CadIr,
    carrier: &SurfaceGeometry,
    u_range: [f64; 2],
    v_range: [f64; 2],
) -> Option<NurbsSurface> {
    if !u_range
        .iter()
        .chain(v_range.iter())
        .all(|value| value.is_finite())
        || u_range[0] >= u_range[1]
        || v_range[0] >= v_range[1]
    {
        return None;
    }
    let evaluate = surface_evaluator(ir, carrier);
    let u_basis = (0..=U_DEGREE)
        .map(|index| bernstein(U_DEGREE, f64_from_index(index)? / f64_from_index(U_DEGREE)?))
        .collect::<Option<Vec<_>>>()?;
    let v_basis = (0..=V_DEGREE)
        .map(|index| bernstein(V_DEGREE, f64_from_index(index)? / f64_from_index(V_DEGREE)?))
        .collect::<Option<Vec<_>>>()?;
    let mut samples = [[[0.0; 3]; V_DEGREE + 1]; U_DEGREE + 1];
    for (i, row) in samples.iter_mut().enumerate() {
        let u =
            u_range[0] + (u_range[1] - u_range[0]) * f64_from_index(i)? / f64_from_index(U_DEGREE)?;
        for (j, point) in row.iter_mut().enumerate() {
            let v = v_range[0]
                + (v_range[1] - v_range[0]) * f64_from_index(j)? / f64_from_index(V_DEGREE)?;
            *point = evaluate(u, v)?;
        }
    }
    let mut along_u = [[[0.0; 3]; V_DEGREE + 1]; U_DEGREE + 1];
    for j in 0..=V_DEGREE {
        let values = samples
            .iter()
            .map(|row| row[j].to_vec())
            .collect::<Vec<_>>();
        for (i, row) in solve_columns(&u_basis, &values)?.into_iter().enumerate() {
            along_u[i][j].copy_from_slice(&row);
        }
    }
    let mut poles = [[[0.0; 3]; V_DEGREE + 1]; U_DEGREE + 1];
    for i in 0..=U_DEGREE {
        let values = along_u[i]
            .iter()
            .map(|point| point.to_vec())
            .collect::<Vec<_>>();
        for (j, row) in solve_columns(&v_basis, &values)?.into_iter().enumerate() {
            poles[i][j].copy_from_slice(&row);
        }
    }
    for i in 0..=HOLDOUT_STEPS {
        let tu = f64_from_index(i)? / f64_from_index(HOLDOUT_STEPS)?;
        let u = u_range[0] + (u_range[1] - u_range[0]) * tu;
        let bu = bernstein(U_DEGREE, tu)?;
        for j in 0..=HOLDOUT_STEPS {
            let tv = f64_from_index(j)? / f64_from_index(HOLDOUT_STEPS)?;
            let v = v_range[0] + (v_range[1] - v_range[0]) * tv;
            let bv = bernstein(V_DEGREE, tv)?;
            let actual = evaluate(u, v)?;
            let mut predicted = [0.0; 3];
            for (row, weight_u) in poles.iter().zip(&bu) {
                for (point, weight_v) in row.iter().zip(&bv) {
                    for axis in 0..3 {
                        predicted[axis] += weight_u * weight_v * point[axis];
                    }
                }
            }
            let squared = (0..3)
                .map(|axis| (actual[axis] - predicted[axis]).powi(2))
                .sum::<f64>();
            if !squared.is_finite() || squared.sqrt() > MAX_FIT_ERROR_MM {
                return None;
            }
        }
    }
    let points = poles
        .iter()
        .map(|row| {
            row.iter()
                .map(|point| Point3::new(point[0], point[1], point[2]))
                .collect()
        })
        .collect();
    NurbsSurface::new(
        NurbsSurfaceAxis::new(
            u32::try_from(U_DEGREE).ok()?,
            vec![
                u_range[0], u_range[0], u_range[0], u_range[0], u_range[1], u_range[1], u_range[1],
                u_range[1],
            ],
            false,
        ),
        NurbsSurfaceAxis::new(
            u32::try_from(V_DEGREE).ok()?,
            vec![
                v_range[0], v_range[0], v_range[0], v_range[0], v_range[0], v_range[0], v_range[1],
                v_range[1], v_range[1], v_range[1], v_range[1], v_range[1],
            ],
            false,
        ),
        NurbsPoleGrid::Polynomial { rows: points },
        false,
    )
    .ok()
}

pub(super) fn fit_edge(
    ir: &CadIr,
    carrier: &SurfaceGeometry,
    edge: &ClosedPcurve,
    v_bounds: Option<[f64; 2]>,
) -> Option<NurbsCurve> {
    let evaluate = surface_evaluator(ir, carrier);
    fit_lifted_edge(edge.range, |parameter: f64| {
        let uv = eval::decode::pcurve_uv(EvaluationAdmission::Standard, &edge.geometry, parameter)
            .ok()?
            .get();
        let chart_v = chart_v(uv.v, v_bounds)?;
        evaluate(uv.u, chart_v)
    })
}

pub(super) fn fit_lifted_edge(
    range: [f64; 2],
    evaluate: impl Fn(f64) -> Option<[f64; 3]>,
) -> Option<NurbsCurve> {
    fit_edge_with_error(range, evaluate, MAX_EDGE_ERROR_MM)
}

pub(super) fn fit_edge_with_error(
    range: [f64; 2],
    evaluate: impl Fn(f64) -> Option<[f64; 3]>,
    tolerance: f64,
) -> Option<NurbsCurve> {
    let (knots, poles) = fit_bezier_lanes(range, evaluate, tolerance)?;
    let points = poles
        .into_iter()
        .map(|[x, y, z]| Point3::new(x, y, z))
        .collect();
    NurbsCurve::from_lanes(u32::try_from(EDGE_DEGREE).ok()?, knots, points, None, false).ok()
}

pub(super) fn fit_bezier_lanes<const N: usize>(
    range: [f64; 2],
    evaluate: impl Fn(f64) -> Option<[f64; N]>,
    tolerance: f64,
) -> Option<(Vec<f64>, Vec<[f64; N]>)> {
    let evaluate = memoize(evaluate);
    let basis = (0..=EDGE_DEGREE)
        .map(|index| {
            bernstein(
                EDGE_DEGREE,
                f64_from_index(index)? / f64_from_index(EDGE_DEGREE)?,
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let holdout_coefficients = (0..=EDGE_HOLDOUT_STEPS)
        .map(|index| {
            bernstein(
                EDGE_DEGREE,
                f64_from_index(index)? / f64_from_index(EDGE_HOLDOUT_STEPS)?,
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let mut spans = vec![range];
    for depth in 0..=MAX_EDGE_SPLIT_DEPTH {
        let mut next = Vec::new();
        let mut controls = Vec::new();
        let mut split = false;
        for &[lower, upper] in &spans {
            let samples = (0..=EDGE_DEGREE)
                .map(|index| {
                    let fraction = f64_from_index(index)? / f64_from_index(EDGE_DEGREE)?;
                    evaluate(interpolate_interval([lower, upper], fraction))
                })
                .collect::<Option<Vec<_>>>()?;
            let mut poles = [[0.0; N]; EDGE_DEGREE + 1];
            let values = samples
                .iter()
                .map(|point| point.to_vec())
                .collect::<Vec<_>>();
            for (pole, row) in poles.iter_mut().zip(solve_columns(&basis, &values)?) {
                pole.copy_from_slice(&row);
            }
            poles[0] = evaluate(lower)?;
            poles[EDGE_DEGREE] = evaluate(upper)?;
            let mut maximum_error = 0.0_f64;
            for (index, coefficients) in holdout_coefficients.iter().enumerate() {
                let fraction = f64_from_index(index)? / f64_from_index(EDGE_HOLDOUT_STEPS)?;
                let actual = evaluate(interpolate_interval([lower, upper], fraction))?;
                let mut predicted = [0.0; N];
                for (pole, coefficient) in poles.iter().zip(coefficients) {
                    for (value, control) in predicted.iter_mut().zip(pole) {
                        *value += coefficient * control;
                    }
                }
                let error = actual
                    .iter()
                    .zip(predicted)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    .sqrt();
                maximum_error = maximum_error.max(error);
            }
            if !maximum_error.is_finite() {
                return None;
            }
            if maximum_error > tolerance {
                if depth == MAX_EDGE_SPLIT_DEPTH || spans.len() >= MAX_EDGE_SPANS {
                    return None;
                }
                let midpoint = lower + (upper - lower) * 0.5;
                next.push([lower, midpoint]);
                next.push([midpoint, upper]);
                split = true;
            } else {
                next.push([lower, upper]);
            }
            let shared = usize::from(!controls.is_empty());
            controls.extend(poles.into_iter().skip(shared));
        }
        if !split {
            let mut knots = Vec::new();
            knots.extend([spans.first()?[0]; EDGE_DEGREE + 1]);
            for span in spans.iter().skip(1) {
                knots.extend([span[0]; EDGE_DEGREE]);
            }
            knots.extend([spans.last()?[1]; EDGE_DEGREE + 1]);
            return Some((knots, controls));
        }
        if next.len() > MAX_EDGE_SPANS {
            return None;
        }
        spans = next;
    }
    None
}

fn interpolate_interval([lo, hi]: [f64; 2], fraction: f64) -> f64 {
    if fraction == 0.0 {
        lo
    } else if fraction == 1.0 {
        hi
    } else {
        (hi - lo).mul_add(fraction, lo)
    }
}

pub(super) fn closed_patch(
    ir: &CadIr,
    carrier: &SurfaceGeometry,
    data: &[u8],
    records: &[ConsolidatedRecord],
    source_index: usize,
    v_bounds: Option<[f64; 2]>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<ClosedPatch> {
    let evaluate = surface_evaluator(ir, carrier);
    let mut candidates = Vec::new();
    let source_records = records
        .iter()
        .filter(|record| record.source_index() == source_index)
        .cloned()
        .collect::<Vec<_>>();
    for family in [ConsolidatedFamily::A, ConsolidatedFamily::B] {
        for pcurve in
            crate::wire::records::family_pcurves_from_records(data, &source_records, family)
        {
            let candidate = (|| {
                let (knots, points, first, second) = pcurve.native_lanes();
                let knots = knots
                    .into_iter()
                    .map(cadmpeg_ir::scalar::FiniteReal::get)
                    .collect::<Vec<_>>();
                let points = points
                    .into_iter()
                    .map(cadmpeg_ir::units::FiniteVector::get)
                    .collect::<Vec<_>>();
                let first = first
                    .into_iter()
                    .map(cadmpeg_ir::units::FiniteVector::get)
                    .collect::<Vec<_>>();
                let second = second
                    .into_iter()
                    .map(cadmpeg_ir::units::FiniteVector::get)
                    .collect::<Vec<_>>();
                let geometry = crate::assemble::quintic_jet_pcurve(
                    crate::wire::records::ConsolidatedPcurve::DEGREE,
                    &knots,
                    &points,
                    (&first, &second),
                    refusal,
                    pcurve.pos,
                )?;
                let endpoints = pcurve.range.endpoints();
                let mut lifted = Vec::new();
                for parameter in endpoints {
                    let uv = eval::decode::pcurve_uv(
                        EvaluationAdmission::Standard,
                        &geometry,
                        parameter,
                    )
                    .ok()?
                    .get();
                    let chart_v = chart_v(uv.v, v_bounds)?;
                    let point = evaluate(uv.u, chart_v)?;
                    lifted.push(([uv.u, chart_v], point));
                }
                Some(ClosedPcurve {
                    pos: pcurve.pos,
                    geometry,
                    range: endpoints,
                    endpoints: lifted.try_into().ok()?,
                })
            })();
            if let Some(pcurve) = candidate {
                candidates.push(pcurve);
                if candidates.len() > MAX_PCURVE_CANDIDATES {
                    return None;
                }
            }
        }
    }
    let mut closed = Vec::new();
    for (index, first) in candidates.iter().enumerate() {
        for second in candidates.iter().skip(index + 1) {
            let distance = |left: &[f64; 3], right: &[f64; 3]| {
                (0..3)
                    .map(|axis| (left[axis] - right[axis]).powi(2))
                    .sum::<f64>()
                    .sqrt()
            };
            let same = distance(&first.endpoints[0].1, &second.endpoints[0].1)
                .max(distance(&first.endpoints[1].1, &second.endpoints[1].1));
            let opposite = distance(&first.endpoints[0].1, &second.endpoints[1].1)
                .max(distance(&first.endpoints[1].1, &second.endpoints[0].1));
            if same.min(opposite) < LOOP_CLOSURE_MM {
                let u = first
                    .endpoints
                    .iter()
                    .chain(&second.endpoints)
                    .map(|row| row.0[0])
                    .collect::<Vec<_>>();
                let v = first
                    .endpoints
                    .iter()
                    .chain(&second.endpoints)
                    .map(|row| row.0[1])
                    .collect::<Vec<_>>();
                closed.push(ClosedPatch {
                    u_range: [
                        u.iter().copied().fold(f64::INFINITY, f64::min),
                        u.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    ],
                    v_range: [
                        v.iter().copied().fold(f64::INFINITY, f64::min),
                        v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    ],
                    edges: [first.clone(), second.clone()],
                });
            }
        }
    }
    (closed.len() == 1).then(|| closed.remove(0))
}

const RECTANGULAR_U_DEGREE: usize = 5;
const RECTANGULAR_V_DEGREE: usize = 5;
const RECTANGULAR_U_SPANS: usize = 4;
const RECTANGULAR_V_SPANS: usize = 2;
const MAX_NATIVE_FIT_STATIONS: usize = 64;
const RECTANGULAR_V_POLES: usize = 9;
const RECTANGULAR_HOLDOUT_U: usize = 60;
const RECTANGULAR_HOLDOUT_V: usize = 30;
pub(super) struct FittedRectangularPatch {
    pub(super) surface: NurbsSurface,
    pub(super) max_curve: NurbsCurve,
    pub(super) upper_curve: NurbsCurve,
    pub(super) min_curve: NurbsCurve,
    pub(super) lower_curve: NurbsCurve,
}

fn bspline_basis(knots: &[f64], degree: usize, mut t: f64, i: usize) -> f64 {
    if let Some(&last) = knots.last() {
        if t >= last {
            let Some(bits) = last.to_bits().checked_sub(1) else {
                return 0.0;
            };
            t = f64::from_bits(bits);
        }
    }
    let Some(n) = knots.len().checked_sub(degree + 2) else {
        return 0.0;
    };
    if i > n {
        return 0.0;
    }
    let mut values = Vec::with_capacity(n + degree + 1);
    for j in 0..=(n + degree) {
        let active = if knots[j] <= t && t < knots[j + 1] {
            1.0
        } else {
            0.0
        };
        values.push(active);
    }
    for p in 1..=degree {
        let Some(next_len) = values.len().checked_sub(1) else {
            break;
        };
        let mut next = Vec::with_capacity(next_len);
        for j in 0..next_len {
            let mut val = 0.0;
            let d1 = knots[j + p] - knots[j];
            if d1 > 0.0 {
                val += (t - knots[j]) / d1 * values[j];
            }
            let d2 = knots[j + p + 1] - knots[j + 1];
            if d2 > 0.0 {
                val += (knots[j + p + 1] - t) / d2 * values[j + 1];
            }
            next.push(val);
        }
        values = next;
    }
    values.get(i).copied().unwrap_or(0.0)
}

fn knot_vector(low: f64, high: f64, spans: usize) -> Option<Vec<f64>> {
    let mut knots = vec![low; 6];
    for i in 1..spans {
        let knot = low + (high - low) * (f64_from_index(i)?) / (f64_from_index(spans)?);
        knots.extend([knot; 3]);
    }
    knots.extend([high; 6]);
    Some(knots)
}

pub(super) fn fit_rectangular_patch(
    definition: &ProceduralSurfaceDefinition,
    [u_min, u_max]: [f64; 2],
) -> Option<FittedRectangularPatch> {
    let ProceduralSurfaceDefinition::RollingBallJet(stations) = definition else {
        return None;
    };
    let uniform = knot_vector(u_min, u_max, RECTANGULAR_U_SPANS)?;
    fit_patch_with_knots(definition, [u_min, u_max], uniform).or_else(|| {
        let interior = stations
            .stations()
            .iter()
            .map(|station| station.knot.get())
            .filter(|knot| *knot > u_min && *knot < u_max)
            .take(MAX_NATIVE_FIT_STATIONS + 1)
            .collect::<Vec<_>>();
        if interior.len() > MAX_NATIVE_FIT_STATIONS {
            return None;
        }
        let mut knots = Vec::new();
        knots.extend([u_min; 6]);
        for knot in interior {
            knots.extend([knot; 3]);
        }
        knots.extend([u_max; 6]);
        fit_patch_with_knots(definition, [u_min, u_max], knots)
    })
}

fn fit_patch_with_knots(
    definition: &ProceduralSurfaceDefinition,
    [u_min, u_max]: [f64; 2],
    ku: Vec<f64>,
) -> Option<FittedRectangularPatch> {
    let u_poles = ku.len().checked_sub(RECTANGULAR_U_DEGREE + 1)?;
    let eval_at = |u: f64, v: f64| -> Option<[f64; 3]> {
        let pt = eval::rolling_ball_jet_point(definition, u.clamp(u_min, u_max), v)
            .ok()?
            .get();
        Some([pt.x, pt.y, pt.z])
    };
    let kv = knot_vector(0.0, 1.0, RECTANGULAR_V_SPANS)?;
    let mut gu = Vec::with_capacity(u_poles);
    for i in 0..u_poles {
        let sum: f64 = ku[i + 1..=i + 5].iter().sum();
        gu.push(sum / 5.0);
    }
    let mut gv = Vec::with_capacity(RECTANGULAR_V_POLES);
    for j in 0..RECTANGULAR_V_POLES {
        let sum: f64 = kv[j + 1..=j + 5].iter().sum();
        gv.push(sum / 5.0);
    }
    let mut bu = Vec::with_capacity(u_poles);
    for u in &gu {
        let mut row = Vec::with_capacity(u_poles);
        for j in 0..u_poles {
            row.push(bspline_basis(&ku, RECTANGULAR_U_DEGREE, *u, j));
        }
        bu.push(row);
    }
    let mut bv = Vec::with_capacity(RECTANGULAR_V_POLES);
    for v in &gv {
        let mut row = Vec::with_capacity(RECTANGULAR_V_POLES);
        for j in 0..RECTANGULAR_V_POLES {
            row.push(bspline_basis(&kv, RECTANGULAR_V_DEGREE, *v, j));
        }
        bv.push(row);
    }
    let mut samples = (0..u_poles)
        .map(|_| [[0.0; 3]; RECTANGULAR_V_POLES])
        .collect::<Vec<_>>();
    for (i, u) in gu.iter().enumerate() {
        for (j, v) in gv.iter().enumerate() {
            samples[i][j] = eval_at(*u, *v)?;
        }
    }
    let values = samples
        .iter()
        .map(|row| row.iter().flatten().copied().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let intermediate = solve_columns(&bu, &values)?;
    let values = (0..RECTANGULAR_V_POLES)
        .map(|j| {
            intermediate
                .iter()
                .flat_map(|row| row[3 * j..3 * j + 3].iter().copied())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let along_v = solve_columns(&bv, &values)?;
    let poles = (0..u_poles)
        .map(|i| {
            std::array::from_fn::<_, RECTANGULAR_V_POLES, _>(|j| {
                std::array::from_fn::<_, 3, _>(|axis| along_v[j][3 * i + axis])
            })
        })
        .collect::<Vec<_>>();
    let holdout_v = (0..=RECTANGULAR_HOLDOUT_V)
        .map(|j| {
            let v = f64_from_index(j)? / f64_from_index(RECTANGULAR_HOLDOUT_V)?;
            let weights = (0..RECTANGULAR_V_POLES)
                .map(|k| bspline_basis(&kv, RECTANGULAR_V_DEGREE, v, k))
                .collect::<Vec<_>>();
            Some((v, weights))
        })
        .collect::<Option<Vec<_>>>()?;
    for i in 0..=RECTANGULAR_HOLDOUT_U {
        let u = u_min
            + (u_max - u_min) * (f64_from_index(i)?) / (f64_from_index(RECTANGULAR_HOLDOUT_U)?);
        let mut u_weights = Vec::with_capacity(u_poles);
        for k in 0..u_poles {
            u_weights.push(bspline_basis(&ku, RECTANGULAR_U_DEGREE, u, k));
        }
        for (v, v_weights) in &holdout_v {
            let v = *v;
            let actual = eval_at(u, v)?;
            let mut fitted = [0.0; 3];
            for (r, uw) in u_weights.iter().enumerate() {
                for (c, vw) in v_weights.iter().enumerate() {
                    let w = uw * vw;
                    for axis in 0..3 {
                        fitted[axis] += w * poles[r][c][axis];
                    }
                }
            }
            let err = (0..3)
                .map(|axis| (actual[axis] - fitted[axis]).powi(2))
                .sum::<f64>()
                .sqrt();
            if !err.is_finite() || err > MAX_FIT_ERROR_MM {
                return None;
            }
        }
    }
    let max_points = (0..RECTANGULAR_V_POLES)
        .map(|j| {
            Point3::new(
                poles[u_poles - 1][j][0],
                poles[u_poles - 1][j][1],
                poles[u_poles - 1][j][2],
            )
        })
        .collect();
    let upper_points = (0..u_poles)
        .map(|i| Point3::new(poles[i][8][0], poles[i][8][1], poles[i][8][2]))
        .collect();
    let min_points = (0..RECTANGULAR_V_POLES)
        .map(|j| Point3::new(poles[0][j][0], poles[0][j][1], poles[0][j][2]))
        .collect();
    let lower_points = (0..u_poles)
        .map(|i| Point3::new(poles[i][0][0], poles[i][0][1], poles[i][0][2]))
        .collect();

    let max_curve = NurbsCurve::from_lanes(
        u32::try_from(RECTANGULAR_V_DEGREE).ok()?,
        kv.clone(),
        max_points,
        None,
        false,
    )
    .ok()?;
    let upper_curve = NurbsCurve::from_lanes(
        u32::try_from(RECTANGULAR_U_DEGREE).ok()?,
        ku.clone(),
        upper_points,
        None,
        false,
    )
    .ok()?;
    let min_curve = NurbsCurve::from_lanes(
        u32::try_from(RECTANGULAR_V_DEGREE).ok()?,
        kv.clone(),
        min_points,
        None,
        false,
    )
    .ok()?;
    let lower_curve = NurbsCurve::from_lanes(
        u32::try_from(RECTANGULAR_U_DEGREE).ok()?,
        ku.clone(),
        lower_points,
        None,
        false,
    )
    .ok()?;

    let surface_points = poles
        .iter()
        .map(|row| {
            row.iter()
                .map(|point| Point3::new(point[0], point[1], point[2]))
                .collect()
        })
        .collect();
    let surface = NurbsSurface::new(
        NurbsSurfaceAxis::new(u32::try_from(RECTANGULAR_U_DEGREE).ok()?, ku, false),
        NurbsSurfaceAxis::new(u32::try_from(RECTANGULAR_V_DEGREE).ok()?, kv, false),
        NurbsPoleGrid::Polynomial {
            rows: surface_points,
        },
        false,
    )
    .ok()?;
    Some(FittedRectangularPatch {
        surface,
        max_curve,
        upper_curve,
        min_curve,
        lower_curve,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn simultaneous_coordinates_preserve_pivoted_solutions() {
        use super::solve_columns;
        let matrix = vec![
            vec![0.0, 2.0, 1.0],
            vec![1.0, -2.0, -3.0],
            vec![2.0, 3.0, 1.0],
        ];
        let values = vec![vec![1.0, -1.0], vec![-6.0, 7.0], vec![2.0, 3.5]];
        let expected = [[1.0, 2.0], [-1.0, 0.5], [3.0, -2.0]];
        let actual = solve_columns(&matrix, &values).expect("invertible system");
        for (actual, expected) in actual.iter().zip(expected) {
            for (&actual, expected) in actual.iter().zip(expected) {
                assert!((actual - expected).abs() < 1.0e-12);
            }
        }
        assert!(solve_columns(
            &[vec![1.0, 2.0], vec![2.0, 4.0]],
            &[vec![3.0, 6.0], vec![6.0, 12.0]]
        )
        .is_none());
        assert!(solve_columns(&matrix, &[vec![1.0], vec![2.0, 3.0], vec![4.0]]).is_none());
    }

    #[test]
    fn lifted_edge_shares_its_adaptive_span_endpoints() {
        use super::{
            eval, fit_lifted_edge, EvaluationAdmission, Point3, EDGE_DEGREE, MAX_EDGE_ERROR_MM,
        };
        let evaluate = |t: f64| Some([t, t.sin(), 0.0]);
        let curve = fit_lifted_edge([0.0, 8.0], evaluate).expect("valid fixture geometry");
        let runs = curve.knots().chunk_by(|a, b| a == b).collect::<Vec<_>>();
        assert!(runs.len() > 2, "the test curve must require subdivision");
        for run in &runs[1..runs.len() - 1] {
            assert_eq!(run.len(), EDGE_DEGREE);
            let t = run[0];
            let at = eval::decode::nurbs_curve_point_at(EvaluationAdmission::Standard, &curve, t)
                .expect("valid fixture geometry")
                .get();
            assert!(at.distance(Point3::new(t, t.sin(), 0.0)) <= MAX_EDGE_ERROR_MM);
        }
    }

    use super::{endpoint_relation, fit};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::Point3;

    fn surface(u_degree: u32, u_coordinates: &[f64]) -> SurfaceGeometry {
        let controls = u_coordinates
            .iter()
            .map(|coordinate| {
                vec![
                    Point3::new(*coordinate, 0.0, 0.0),
                    Point3::new(*coordinate, 1.0, 0.0),
                ]
            })
            .collect::<Vec<_>>();
        let nurbs = NurbsSurface::new(
            NurbsSurfaceAxis::new(
                u_degree,
                if u_degree == 3 {
                    vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]
                } else {
                    vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0]
                },
                false,
            ),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsPoleGrid::Polynomial { rows: controls },
            false,
        )
        .expect("synthetic surface");
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs))
    }

    #[test]
    fn bounded_patch_fit_accepts_cubic_and_rejects_quartic() {
        let ir = CadIr::empty();
        let cubic = surface(3, &[0.0, 0.0, 0.0, 1.0]);
        let quartic = surface(4, &[0.0, 0.0, 0.0, 0.0, 1.0]);
        assert!(fit(&ir, &cubic, [0.0, 1.0], [0.0, 1.0]).is_some());
        assert!(fit(&ir, &quartic, [0.0, 1.0], [0.0, 1.0]).is_none());
    }

    #[test]
    fn closed_edge_pair_has_two_endpoint_orders() {
        let first = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
        assert_eq!(endpoint_relation(first, first), Some(true));
        assert_eq!(endpoint_relation(first, [first[1], first[0]]), Some(false));
        assert_eq!(
            endpoint_relation(first, [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]]),
            None
        );
    }
}
