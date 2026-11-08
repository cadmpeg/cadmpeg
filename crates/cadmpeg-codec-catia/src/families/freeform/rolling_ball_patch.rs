// SPDX-License-Identifier: Apache-2.0
//! Bounded polynomial approximation of a native rolling-ball face carrier.

use cadmpeg_core::convert::f64_from_index;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{self, admission::EvaluationAdmission};
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SolvedSurfaceGeometry, SurfaceGeometry};
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

fn solve(matrix: &[Vec<f64>], values: &[f64]) -> Option<Vec<f64>> {
    let count = values.len();
    if matrix.len() != count || matrix.iter().any(|row| row.len() != count) {
        return None;
    }
    let mut rows = matrix
        .iter()
        .zip(values)
        .map(|(row, value)| {
            let mut augmented = row.clone();
            augmented.push(*value);
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
    Some(rows.into_iter().map(|row| row[count]).collect())
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
    let evaluate = |u: f64, v: f64| {
        let point = eval::model_surface_point(EvaluationAdmission::Standard, ir, carrier, u, v)
            .ok()?
            .get();
        Some([point.x, point.y, point.z])
    };
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
        for axis in 0..3 {
            let values = samples.iter().map(|row| row[j][axis]).collect::<Vec<_>>();
            for (i, value) in solve(&u_basis, &values)?.into_iter().enumerate() {
                along_u[i][j][axis] = value;
            }
        }
    }
    let mut poles = [[[0.0; 3]; V_DEGREE + 1]; U_DEGREE + 1];
    for i in 0..=U_DEGREE {
        for axis in 0..3 {
            let values = along_u[i]
                .iter()
                .map(|point| point[axis])
                .collect::<Vec<_>>();
            for (j, value) in solve(&v_basis, &values)?.into_iter().enumerate() {
                poles[i][j][axis] = value;
            }
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
    fit_lifted_edge(edge.range, |parameter: f64| {
        let uv = eval::decode::pcurve_uv(EvaluationAdmission::Standard, &edge.geometry, parameter)
            .ok()?
            .get();
        let chart_v = chart_v(uv.v, v_bounds)?;
        let point =
            eval::model_surface_point(EvaluationAdmission::Standard, ir, carrier, uv.u, chart_v)
                .ok()?
                .get();
        Some([point.x, point.y, point.z])
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
    let basis = (0..=EDGE_DEGREE)
        .map(|index| {
            bernstein(
                EDGE_DEGREE,
                f64_from_index(index)? / f64_from_index(EDGE_DEGREE)?,
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
            for axis in 0..N {
                let values = samples.iter().map(|point| point[axis]).collect::<Vec<_>>();
                for (index, value) in solve(&basis, &values)?.into_iter().enumerate() {
                    poles[index][axis] = value;
                }
            }
            poles[0] = evaluate(lower)?;
            poles[EDGE_DEGREE] = evaluate(upper)?;
            let mut maximum_error = 0.0_f64;
            for index in 0..=EDGE_HOLDOUT_STEPS {
                let fraction = f64_from_index(index)? / f64_from_index(EDGE_HOLDOUT_STEPS)?;
                let actual = evaluate(interpolate_interval([lower, upper], fraction))?;
                let coefficients = bernstein(EDGE_DEGREE, fraction)?;
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
                    let point = eval::model_surface_point(
                        EvaluationAdmission::Standard,
                        ir,
                        carrier,
                        uv.u,
                        chart_v,
                    )
                    .ok()?
                    .get();
                    lifted.push(([uv.u, chart_v], [point.x, point.y, point.z]));
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

const SIX_EDGE_U_DEGREE: usize = 5;
const SIX_EDGE_V_DEGREE: usize = 5;
const SIX_EDGE_U_SPANS: usize = 4;
const SIX_EDGE_V_SPANS: usize = 2;
const MAX_NATIVE_FIT_STATIONS: usize = 64;
const SIX_EDGE_V_POLES: usize = 9;
const SIX_EDGE_HOLDOUT_U: usize = 60;
const SIX_EDGE_HOLDOUT_V: usize = 30;
const RAIL_SCAN_STEPS: usize = 100;
const GOLDEN_SEARCH_STEPS: usize = 40;
const GOLDEN_RATIO_INV: f64 = 0.618_033_988_749_895;
const MAX_CORNER_RESIDUAL_MM: f64 = 0.001;
const RAIL_ENDPOINT_TOLERANCE_MM: f64 = 0.01;
const EDGE_DELIM_BYTES: [u8; 8] = [0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
const VERTEX_ROW_PREFIX: [u8; 3] = [0x05, 0x08, 0x01];
const ROSTER_PREFIX_LEN: usize = 61;
const ROSTER_CURVES_LEN: usize = 6 * 13;
const ROSTER_VERTICES_LEN: usize = 6 * 7;
const ROSTER_TOTAL_LEN: usize = ROSTER_PREFIX_LEN + ROSTER_CURVES_LEN + ROSTER_VERTICES_LEN;
const CURVE_ROW_LEN: usize = 13;
const VERTEX_ROW_LEN: usize = 7;
const TABLE_ROW_LEN: usize = 15;
const TABLE_ROWS_COUNT: usize = 6;
const TABLE_PAYLOAD_LEN: usize = TABLE_ROWS_COUNT * TABLE_ROW_LEN;

#[derive(Clone, Debug)]
pub(super) struct SixEdgeDomain {
    pub(super) u_min: f64,
    pub(super) u_max: f64,
    pub(super) u1: f64,
    pub(super) u4: f64,
}

pub(super) struct FittedSixEdgePatch {
    pub(super) surface: NurbsSurface,
    pub(super) max_curve: NurbsCurve,
    pub(super) upper_curve: NurbsCurve,
    pub(super) min_curve: NurbsCurve,
    pub(super) lower_curve: NurbsCurve,
}

pub(super) fn extract_boundary_corner_tables(data: &[u8]) -> Vec<[[f64; 3]; 6]> {
    let mut offset = 0;
    let mut tables = Vec::new();
    while let Some(pos) = data[offset..]
        .windows(EDGE_DELIM_BYTES.len())
        .position(|w| w == EDGE_DELIM_BYTES)
    {
        let delim_pos = offset + pos;
        let after = delim_pos + EDGE_DELIM_BYTES.len();
        if let Some(slice) = data.get(after..) {
            if slice.len() >= 3 && slice[0] == 0x01 && slice[1] == 0x06 && slice[2] == 0x06 {
                let rows_start = after + 3;
                let table_end = rows_start + TABLE_PAYLOAD_LEN;
                if let Some(table_slice) = data.get(rows_start..table_end) {
                    if data.get(table_end..table_end + EDGE_DELIM_BYTES.len())
                        == Some(&EDGE_DELIM_BYTES)
                    {
                        let mut corners = [[0.0; 3]; 6];
                        let mut valid = true;
                        for i in 0..6 {
                            let row = &table_slice[i * TABLE_ROW_LEN..(i + 1) * TABLE_ROW_LEN];
                            if row[..3] != VERTEX_ROW_PREFIX {
                                valid = false;
                                break;
                            }
                            let Some(x) = cadmpeg_core::decode::View::f32_le_at(row, 3) else {
                                valid = false;
                                break;
                            };
                            let Some(y) = cadmpeg_core::decode::View::f32_le_at(row, 7) else {
                                valid = false;
                                break;
                            };
                            let Some(z) = cadmpeg_core::decode::View::f32_le_at(row, 11) else {
                                valid = false;
                                break;
                            };
                            corners[i] = [f64::from(x), f64::from(y), f64::from(z)];
                        }
                        if valid {
                            tables.push(corners);
                            offset = table_end + EDGE_DELIM_BYTES.len();
                            continue;
                        }
                    }
                }
            }
        }
        offset = delim_pos + 1;
    }
    tables
}

#[cfg(test)]
pub(super) fn extract_boundary_corner_table(data: &[u8]) -> Option<[[f64; 3]; 6]> {
    let mut tables = extract_boundary_corner_tables(data);
    (tables.len() == 1).then(|| tables.remove(0))
}

pub(super) fn six_edge_rosters(data: &[u8]) -> Vec<(u32, [u32; 6])> {
    let mut rosters = Vec::new();
    let mut pos = 0;
    while let Some(slice) = data.get(pos..) {
        if slice.len() < ROSTER_TOTAL_LEN {
            break;
        }
        if slice[0] == 0xff && slice[1] == 0xb5 {
            let curves_start = pos + ROSTER_PREFIX_LEN;
            let mut valid = true;
            let mut curve_tags = [0u32; 6];
            for (i, tag_slot) in curve_tags.iter_mut().enumerate() {
                let cp = curves_start + i * CURVE_ROW_LEN;
                let Some(row) = data.get(cp..cp + CURVE_ROW_LEN) else {
                    valid = false;
                    break;
                };
                if row[0] != 0x60 || row[5..8] != [0, 0, 0] {
                    valid = false;
                    break;
                }
                let Some(tag) = cadmpeg_core::decode::View::u32_le_at(row, 1) else {
                    valid = false;
                    break;
                };
                *tag_slot = tag;
            }
            if valid {
                let vert_start = curves_start + ROSTER_CURVES_LEN;
                let mut vertex_tags = [0u32; 6];
                for (i, tag_slot) in vertex_tags.iter_mut().enumerate() {
                    let vp = vert_start + i * VERTEX_ROW_LEN;
                    let Some(row) = data.get(vp..vp + VERTEX_ROW_LEN) else {
                        valid = false;
                        break;
                    };
                    if row[0] != 0x54 || row[5..7] != [0, 0] {
                        valid = false;
                        break;
                    }
                    let Some(tag) = cadmpeg_core::decode::View::u32_le_at(row, 1) else {
                        valid = false;
                        break;
                    };
                    *tag_slot = tag;
                }
                if valid {
                    if let Some(face_tag) = cadmpeg_core::decode::View::u32_le_at(data, pos + 14) {
                        let mut sorted_verts = vertex_tags;
                        sorted_verts.sort_unstable();
                        let mut sorted_curves = curve_tags;
                        sorted_curves.sort_unstable();
                        let unique_verts = !sorted_verts.windows(2).any(|pair| pair[0] == pair[1]);
                        let unique_curves =
                            !sorted_curves.windows(2).any(|pair| pair[0] == pair[1]);
                        let non_zero = face_tag != 0
                            && vertex_tags.iter().all(|tag| *tag != 0)
                            && curve_tags.iter().all(|tag| *tag != 0 && *tag != face_tag);
                        if unique_verts && unique_curves && non_zero {
                            rosters.push((face_tag, curve_tags));
                            pos += ROSTER_TOTAL_LEN;
                            continue;
                        }
                    }
                }
            }
        }
        pos += 1;
    }
    rosters
}

#[cfg(test)]
pub(super) fn six_edge_roster(data: &[u8]) -> Option<(u32, [u32; 6])> {
    let mut rosters = six_edge_rosters(data);
    (rosters.len() == 1).then(|| rosters.remove(0))
}

pub(super) fn mfbrp_matches_six_edge_roster(
    data: &[u8],
    face_tag: u32,
    curve_tags: &[u32; 6],
) -> bool {
    const TOKEN: [u8; 4] = [0x01, 0x00, 0x04, 0x00];
    let position = |tag: u32| {
        let mut pattern = [0u8; 8];
        pattern[..4].copy_from_slice(&TOKEN);
        pattern[4..].copy_from_slice(&tag.to_le_bytes());
        let mut matches = data
            .windows(pattern.len())
            .enumerate()
            .filter_map(|(index, bytes)| (bytes == pattern).then_some(index));
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    };
    let Some(face_pos) = position(face_tag) else {
        return false;
    };
    let mut parent = None;
    let mut previous_member = None;
    for (index, tag) in curve_tags.iter().enumerate() {
        let Some(pos) = position(*tag) else {
            return false;
        };
        if face_pos.checked_sub(38 * (index + 1)) != Some(pos) {
            return false;
        }
        let Some(member) = cadmpeg_core::decode::View::u32_le_at(data, pos + 11) else {
            return false;
        };
        let Some(current_parent) = cadmpeg_core::decode::View::u32_le_at(data, pos + 19) else {
            return false;
        };
        if current_parent == 0
            || parent.is_some_and(|value| value != current_parent)
            || previous_member.is_some_and(|value: u32| value.checked_sub(1) != Some(member))
        {
            return false;
        }
        parent = Some(current_parent);
        previous_member = Some(member);
    }
    true
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

pub(super) fn bind_rolling_ball_six_edge(
    ir: &CadIr,
    carrier: &SurfaceGeometry,
    corners: &[[f64; 3]; 6],
) -> Option<SixEdgeDomain> {
    let SurfaceGeometry::Procedural { construction, .. } = carrier else {
        return None;
    };
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|p| &p.id == construction)?;
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet(jet) =
        procedural.definition()
    else {
        return None;
    };
    let stations = jet.stations();
    let t_min = stations.first()?.knot.get();
    let t_max = stations.last()?.knot.get();
    if !t_min.is_finite() || !t_max.is_finite() || t_min >= t_max {
        return None;
    }
    let eval_at = |u: f64, v: f64| -> Option<[f64; 3]> {
        let pt = eval::model_surface_point(EvaluationAdmission::Standard, ir, carrier, u, v)
            .ok()?
            .get();
        Some([pt.x, pt.y, pt.z])
    };
    let dist = |a: [f64; 3], b: [f64; 3]| -> f64 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    };
    let project_to_rail = |point: [f64; 3], s: f64| -> Option<(f64, f64)> {
        let mut best_t = t_min;
        let mut best_dist = f64::INFINITY;
        for step in 0..=RAIL_SCAN_STEPS {
            let t = t_min
                + (t_max - t_min) * (f64_from_index(step)?) / (f64_from_index(RAIL_SCAN_STEPS)?);
            let pt = eval_at(t, s)?;
            let d = dist(point, pt);
            if d < best_dist {
                best_dist = d;
                best_t = t;
            }
        }
        let step_size = (t_max - t_min) / (f64_from_index(RAIL_SCAN_STEPS)?);
        let mut lower = (best_t - step_size).max(t_min);
        let mut upper = (best_t + step_size).min(t_max);
        let mut t1 = upper - GOLDEN_RATIO_INV * (upper - lower);
        let mut t2 = lower + GOLDEN_RATIO_INV * (upper - lower);
        let mut f1 = dist(point, eval_at(t1, s)?);
        let mut f2 = dist(point, eval_at(t2, s)?);
        for _ in 0..GOLDEN_SEARCH_STEPS {
            if f1 < f2 {
                upper = t2;
                t2 = t1;
                f2 = f1;
                t1 = upper - GOLDEN_RATIO_INV * (upper - lower);
                f1 = dist(point, eval_at(t1, s)?);
            } else {
                lower = t1;
                t1 = t2;
                f1 = f2;
                t2 = lower + GOLDEN_RATIO_INV * (upper - lower);
                f2 = dist(point, eval_at(t2, s)?);
            }
        }
        Some(((lower + upper) * 0.5, f1.min(f2)))
    };

    let mut rail0_params = Vec::new();
    let mut rail1_params = Vec::new();
    for corner in corners {
        let (t0, d0) = project_to_rail(*corner, 0.0)?;
        let (t1, d1) = project_to_rail(*corner, 1.0)?;
        if d0 < d1 {
            if d0 > MAX_CORNER_RESIDUAL_MM {
                return None;
            }
            rail0_params.push(t0);
        } else {
            if d1 > MAX_CORNER_RESIDUAL_MM {
                return None;
            }
            rail1_params.push(t1);
        }
    }
    if rail0_params.len() != 3 || rail1_params.len() != 3 {
        return None;
    }
    rail0_params.sort_by(f64::total_cmp);
    rail1_params.sort_by(f64::total_cmp);
    if (rail0_params[0] - rail1_params[0]).abs() > RAIL_ENDPOINT_TOLERANCE_MM
        || (rail0_params[2] - rail1_params[2]).abs() > RAIL_ENDPOINT_TOLERANCE_MM
    {
        return None;
    }
    let u_min = (rail0_params[0] + rail1_params[0]) * 0.5;
    let u_max = (rail0_params[2] + rail1_params[2]) * 0.5;
    let u1 = rail0_params[1];
    let u4 = rail1_params[1];
    if u_min >= u1 || u1 >= u_max || u_min >= u4 || u4 >= u_max {
        return None;
    }
    Some(SixEdgeDomain {
        u_min,
        u_max,
        u1,
        u4,
    })
}

pub(super) fn evaluate_vertex_positions(
    ir: &CadIr,
    surface: &NurbsSurface,
    domain: &SixEdgeDomain,
) -> Option<[Point3; 6]> {
    let surface_geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface.clone()));
    let uv_coords = [
        (domain.u_max, 0.0),
        (domain.u_max, 1.0),
        (domain.u4, 1.0),
        (domain.u_min, 1.0),
        (domain.u_min, 0.0),
        (domain.u1, 0.0),
    ];
    let mut positions = [Point3::new(0.0, 0.0, 0.0); 6];
    for (index, &(u, v)) in uv_coords.iter().enumerate() {
        let pt =
            eval::model_surface_point(EvaluationAdmission::Standard, ir, &surface_geometry, u, v)
                .ok()?
                .get();
        positions[index] = Point3::new(pt.x, pt.y, pt.z);
    }
    Some(positions)
}

pub(super) fn fit_six_edge_patch(
    ir: &CadIr,
    carrier: &SurfaceGeometry,
    domain: &SixEdgeDomain,
) -> Option<FittedSixEdgePatch> {
    let SurfaceGeometry::Procedural {
        construction,
        cache: None,
    } = carrier
    else {
        return None;
    };
    let procedure = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|row| &row.id == construction)?;
    fit_rectangular_patch(procedure.definition(), [domain.u_min, domain.u_max])
}

pub(super) fn fit_rectangular_patch(
    definition: &ProceduralSurfaceDefinition,
    [u_min, u_max]: [f64; 2],
) -> Option<FittedSixEdgePatch> {
    let ProceduralSurfaceDefinition::RollingBallJet(stations) = definition else {
        return None;
    };
    let uniform = knot_vector(u_min, u_max, SIX_EDGE_U_SPANS)?;
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
) -> Option<FittedSixEdgePatch> {
    let u_poles = ku.len().checked_sub(SIX_EDGE_U_DEGREE + 1)?;
    let eval_at = |u: f64, v: f64| -> Option<[f64; 3]> {
        let pt = eval::rolling_ball_jet_point(definition, u.clamp(u_min, u_max), v)
            .ok()?
            .get();
        Some([pt.x, pt.y, pt.z])
    };
    let kv = knot_vector(0.0, 1.0, SIX_EDGE_V_SPANS)?;
    let mut gu = Vec::with_capacity(u_poles);
    for i in 0..u_poles {
        let sum: f64 = ku[i + 1..=i + 5].iter().sum();
        gu.push(sum / 5.0);
    }
    let mut gv = Vec::with_capacity(SIX_EDGE_V_POLES);
    for j in 0..SIX_EDGE_V_POLES {
        let sum: f64 = kv[j + 1..=j + 5].iter().sum();
        gv.push(sum / 5.0);
    }
    let mut bu = Vec::with_capacity(u_poles);
    for u in &gu {
        let mut row = Vec::with_capacity(u_poles);
        for j in 0..u_poles {
            row.push(bspline_basis(&ku, SIX_EDGE_U_DEGREE, *u, j));
        }
        bu.push(row);
    }
    let mut bv = Vec::with_capacity(SIX_EDGE_V_POLES);
    for v in &gv {
        let mut row = Vec::with_capacity(SIX_EDGE_V_POLES);
        for j in 0..SIX_EDGE_V_POLES {
            row.push(bspline_basis(&kv, SIX_EDGE_V_DEGREE, *v, j));
        }
        bv.push(row);
    }
    let mut samples = (0..u_poles)
        .map(|_| [[0.0; 3]; SIX_EDGE_V_POLES])
        .collect::<Vec<_>>();
    for (i, u) in gu.iter().enumerate() {
        for (j, v) in gv.iter().enumerate() {
            samples[i][j] = eval_at(*u, *v)?;
        }
    }
    let mut intermediate = (0..u_poles)
        .map(|_| [[0.0; 3]; SIX_EDGE_V_POLES])
        .collect::<Vec<_>>();
    for j in 0..SIX_EDGE_V_POLES {
        for axis in 0..3 {
            let values: Vec<f64> = (0..u_poles).map(|i| samples[i][j][axis]).collect();
            let sol = solve(&bu, &values)?;
            for (i, val) in sol.into_iter().enumerate() {
                intermediate[i][j][axis] = val;
            }
        }
    }
    let mut poles = (0..u_poles)
        .map(|_| [[0.0; 3]; SIX_EDGE_V_POLES])
        .collect::<Vec<_>>();
    for i in 0..u_poles {
        for axis in 0..3 {
            let values: Vec<f64> = (0..SIX_EDGE_V_POLES)
                .map(|j| intermediate[i][j][axis])
                .collect();
            let sol = solve(&bv, &values)?;
            for (j, val) in sol.into_iter().enumerate() {
                poles[i][j][axis] = val;
            }
        }
    }
    for i in 0..=SIX_EDGE_HOLDOUT_U {
        let u =
            u_min + (u_max - u_min) * (f64_from_index(i)?) / (f64_from_index(SIX_EDGE_HOLDOUT_U)?);
        let mut u_weights = Vec::with_capacity(u_poles);
        for k in 0..u_poles {
            u_weights.push(bspline_basis(&ku, SIX_EDGE_U_DEGREE, u, k));
        }
        for j in 0..=SIX_EDGE_HOLDOUT_V {
            let v = (f64_from_index(j)?) / (f64_from_index(SIX_EDGE_HOLDOUT_V)?);
            let mut v_weights = Vec::with_capacity(SIX_EDGE_V_POLES);
            for k in 0..SIX_EDGE_V_POLES {
                v_weights.push(bspline_basis(&kv, SIX_EDGE_V_DEGREE, v, k));
            }
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
    let max_points = (0..SIX_EDGE_V_POLES)
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
    let min_points = (0..SIX_EDGE_V_POLES)
        .map(|j| Point3::new(poles[0][j][0], poles[0][j][1], poles[0][j][2]))
        .collect();
    let lower_points = (0..u_poles)
        .map(|i| Point3::new(poles[i][0][0], poles[i][0][1], poles[i][0][2]))
        .collect();

    let max_curve = NurbsCurve::from_lanes(
        u32::try_from(SIX_EDGE_V_DEGREE).ok()?,
        kv.clone(),
        max_points,
        None,
        false,
    )
    .ok()?;
    let upper_curve = NurbsCurve::from_lanes(
        u32::try_from(SIX_EDGE_U_DEGREE).ok()?,
        ku.clone(),
        upper_points,
        None,
        false,
    )
    .ok()?;
    let min_curve = NurbsCurve::from_lanes(
        u32::try_from(SIX_EDGE_V_DEGREE).ok()?,
        kv.clone(),
        min_points,
        None,
        false,
    )
    .ok()?;
    let lower_curve = NurbsCurve::from_lanes(
        u32::try_from(SIX_EDGE_U_DEGREE).ok()?,
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
        NurbsSurfaceAxis::new(u32::try_from(SIX_EDGE_U_DEGREE).ok()?, ku, false),
        NurbsSurfaceAxis::new(u32::try_from(SIX_EDGE_V_DEGREE).ok()?, kv, false),
        NurbsPoleGrid::Polynomial {
            rows: surface_points,
        },
        false,
    )
    .ok()?;
    Some(FittedSixEdgePatch {
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

    #[test]
    fn six_edge_corner_table_extraction_positive_and_negative() {
        use super::{extract_boundary_corner_table, EDGE_DELIM_BYTES, VERTEX_ROW_PREFIX};
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&EDGE_DELIM_BYTES);
        buffer.extend_from_slice(&[0x01, 0x06, 0x06]);
        for i in 0_u16..6 {
            buffer.extend_from_slice(&VERTEX_ROW_PREFIX);
            let coord = f32::from(i) * 10.0;
            buffer.extend_from_slice(&coord.to_le_bytes());
            buffer.extend_from_slice(&coord.to_le_bytes());
            buffer.extend_from_slice(&coord.to_le_bytes());
        }
        buffer.extend_from_slice(&EDGE_DELIM_BYTES);

        let corners = extract_boundary_corner_table(&buffer);
        assert!(corners.is_some());
        let c = corners.expect("valid fixture geometry");
        assert_eq!(c[0], [0.0, 0.0, 0.0]);
        assert_eq!(c[5], [50.0, 50.0, 50.0]);

        // Negative: wrong delimiter
        let mut bad = buffer.clone();
        bad[0] = 0x00;
        assert!(extract_boundary_corner_table(&bad).is_none());

        // Negative: wrong vertex count
        let mut bad2 = buffer.clone();
        bad2[10] = 0x02;
        assert!(extract_boundary_corner_table(&bad2).is_none());

        let mut duplicate = buffer.clone();
        duplicate.extend_from_slice(&buffer);
        assert!(extract_boundary_corner_table(&duplicate).is_none());
        assert_eq!(super::extract_boundary_corner_tables(&duplicate).len(), 2);
    }

    #[test]
    fn six_edge_roster_requires_its_mfbrp_face_group() {
        use super::{mfbrp_matches_six_edge_roster, six_edge_roster, six_edge_rosters};
        let tags = [118_u32, 159, 180, 201, 211, 219];
        let mut surfacic = vec![0_u8; 61];
        surfacic[0..2].copy_from_slice(&[0xff, 0xb5]);
        surfacic[14..18].copy_from_slice(&102_u32.to_le_bytes());
        for tag in tags {
            let mut row = [0_u8; 13];
            row[0] = 0x60;
            row[1..5].copy_from_slice(&tag.to_le_bytes());
            surfacic.extend_from_slice(&row);
        }
        for tag in [121_u32, 141, 160, 181, 202, 212] {
            let mut row = [0_u8; 7];
            row[0] = 0x54;
            row[1..5].copy_from_slice(&tag.to_le_bytes());
            surfacic.extend_from_slice(&row);
        }
        assert_eq!(six_edge_roster(&surfacic), Some((102, tags)));
        assert_eq!(six_edge_rosters(&surfacic), vec![(102, tags)]);

        let tags2 = [255_u32, 285, 303, 317, 323, 328];
        let mut second = vec![0_u8; 61];
        second[0..2].copy_from_slice(&[0xff, 0xb5]);
        second[14..18].copy_from_slice(&239_u32.to_le_bytes());
        for tag in tags2 {
            let mut row = [0_u8; 13];
            row[0] = 0x60;
            row[1..5].copy_from_slice(&tag.to_le_bytes());
            second.extend_from_slice(&row);
        }
        for tag in [258_u32, 271, 286, 304, 318, 324] {
            let mut row = [0_u8; 7];
            row[0] = 0x54;
            row[1..5].copy_from_slice(&tag.to_le_bytes());
            second.extend_from_slice(&row);
        }

        let mut multi = surfacic.clone();
        multi.extend_from_slice(&second);
        let multi_rosters = six_edge_rosters(&multi);
        assert_eq!(multi_rosters.len(), 2);
        assert_eq!(multi_rosters[0], (102, tags));
        assert_eq!(multi_rosters[1], (239, tags2));
        assert!(six_edge_roster(&multi).is_none());

        let mut mfbrp = Vec::new();
        for index in (0..6).rev() {
            let mut row = [0_u8; 38];
            row[..4].copy_from_slice(&[1, 0, 4, 0]);
            row[4..8].copy_from_slice(&tags[index].to_le_bytes());
            row[11..15].copy_from_slice(
                &(35_u32 - u32::try_from(index).expect("fixture index fits")).to_le_bytes(),
            );
            row[19..23].copy_from_slice(&27_u32.to_le_bytes());
            mfbrp.extend_from_slice(&row);
        }
        let mut face = [0_u8; 38];
        face[..4].copy_from_slice(&[1, 0, 4, 0]);
        face[4..8].copy_from_slice(&102_u32.to_le_bytes());
        mfbrp.extend_from_slice(&face);
        assert!(mfbrp_matches_six_edge_roster(&mfbrp, 102, &tags));

        let mut wrong_parent = mfbrp.clone();
        wrong_parent[19] = 28;
        assert!(!mfbrp_matches_six_edge_roster(&wrong_parent, 102, &tags));
        let mut missing_face = mfbrp.clone();
        missing_face.truncate(missing_face.len() - 38);
        assert!(!mfbrp_matches_six_edge_roster(&missing_face, 102, &tags));
        surfacic[61 + 13 * 6] = 0;
        assert!(six_edge_roster(&surfacic).is_none());
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
