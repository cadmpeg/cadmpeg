// SPDX-License-Identifier: Apache-2.0
//! Trimmed positive-weight surface bounds from homogeneous Bézier patches.

use cadmpeg_ir::geometry::nurbs::bezier::homogeneous_spans;
use cadmpeg_ir::math::Point3;

use crate::boundary::ResolvedFace;
use crate::canonical::surface::{Chart, NurbsChart};
use crate::context::Eval;
use crate::extent::boundary_parameter_ranges;
use crate::failure::{Measure, MeasureFailure};
use crate::geom::Aabb;
use crate::reason::UnavailableReason;
use crate::region::{winding_number, LoopShape, Region};

const BOX_RELATIVE_RESOLUTION: f64 = 1.0e-8;
const MAX_NURBS_BOX_WORK: usize = 1 << 20;
const MAX_NURBS_BOX_PATCHES: usize = 65_536;

type Control = [f64; 4];

#[derive(Clone, Copy)]
pub(super) struct Trims<'a, 'ir> {
    pub(super) shapes: &'a [Option<LoopShape>],
    pub(super) resolved: Option<&'a ResolvedFace<'ir>>,
}

#[derive(Clone)]
struct Patch {
    domain: [[f64; 2]; 2],
    rows: Vec<Vec<Control>>,
    axis: usize,
}

fn no_bound() -> MeasureFailure {
    MeasureFailure::unavailable(
        UnavailableReason::BboxInteriorUnbounded,
        None,
        "the trimmed surface bound did not converge within MAX_NURBS_BOX_PATCHES",
    )
}

fn charge(eval: &Eval<'_, '_>, factors: &[usize]) -> Measure<()> {
    let units = factors
        .iter()
        .try_fold(1_usize, |work, factor| work.checked_mul(*factor))
        .ok_or_else(no_bound)?;
    eval.charge(units)
}

fn within(outer: Aabb, inner: Aabb, precision: f64) -> bool {
    (0..3).all(|axis| {
        outer.min[axis] >= inner.min[axis] - precision
            && outer.max[axis] <= inner.max[axis] + precision
    })
}

fn split_polygon(points: &[Control], fraction: f64) -> Measure<(Vec<Control>, Vec<Control>)> {
    let mut work = points.to_vec();
    let mut left = vec![work[0]];
    let mut right = vec![work[work.len() - 1]];
    for length in (1..work.len()).rev() {
        for index in 0..length {
            let mut point = [0.0; 4];
            for ((value, start), end) in point.iter_mut().zip(work[index]).zip(work[index + 1]) {
                *value = cadmpeg_ir::math::interpolate(start, end, fraction)
                    .ok_or_else(no_bound)?
                    .get();
            }
            if point[3] <= 0.0 {
                return Err(no_bound());
            }
            work[index] = point;
        }
        left.push(work[0]);
        right.push(work[length - 1]);
    }
    right.reverse();
    Ok((left, right))
}

impl Patch {
    fn split(self) -> Measure<(Self, Self)> {
        let middle = self.domain[self.axis][0].midpoint(self.domain[self.axis][1]);
        self.split_at(middle)
    }

    fn split_at(self, parameter: f64) -> Measure<(Self, Self)> {
        let fraction = cadmpeg_ir::math::parameter_fraction(
            parameter,
            self.domain[self.axis][0],
            self.domain[self.axis][1],
        )
        .ok_or_else(no_bound)?
        .get();
        if fraction <= 0.0 || fraction >= 1.0 {
            return Err(no_bound());
        }
        let mut left = self.rows.clone();
        let mut right = self.rows.clone();
        if self.axis == 0 {
            for v in 0..self.rows[0].len() {
                let column: Vec<_> = self.rows.iter().map(|row| row[v]).collect();
                let (a, b) = split_polygon(&column, fraction)?;
                for u in 0..self.rows.len() {
                    left[u][v] = a[u];
                    right[u][v] = b[u];
                }
            }
        } else {
            for (v, row) in self.rows.iter().enumerate() {
                (left[v], right[v]) = split_polygon(row, fraction)?;
            }
        }
        let mut low = self.domain;
        let mut high = self.domain;
        low[self.axis][1] = parameter;
        high[self.axis][0] = parameter;
        Ok((
            Self {
                domain: low,
                rows: left,
                axis: 1 - self.axis,
            },
            Self {
                domain: high,
                rows: right,
                axis: 1 - self.axis,
            },
        ))
    }

    fn charge(&self, eval: &Eval<'_, '_>) -> Measure<()> {
        charge(
            eval,
            &[
                4,
                self.rows.len(),
                self.rows[0].len(),
                self.rows
                    .len()
                    .checked_add(self.rows[0].len())
                    .ok_or_else(no_bound)?,
            ],
        )
    }

    fn clip(mut self, domain: [[f64; 2]; 2]) -> Measure<Option<Self>> {
        for (axis, range) in domain.iter().enumerate() {
            let low = range[0].max(self.domain[axis][0]);
            let high = range[1].min(self.domain[axis][1]);
            if low >= high {
                return Ok(None);
            }
            self.axis = axis;
            if high < self.domain[axis][1] {
                self = self.split_at(high)?.0;
            }
            self.axis = axis;
            if low > self.domain[axis][0] {
                self = self.split_at(low)?.1;
            }
        }
        Ok(Some(self))
    }

    fn hull(&self) -> Aabb {
        let point = |p: &Control| Point3::new(p[0] / p[3], p[1] / p[3], p[2] / p[3]);
        let mut result = Aabb::of_point(point(&self.rows[0][0]));
        for p in self.rows.iter().flatten() {
            result.include(point(p));
        }
        result
    }
}

fn patches(eval: &Eval<'_, '_>, chart: &NurbsChart<'_>) -> Measure<Vec<Patch>> {
    let surface = chart.surface;
    let mut columns = Vec::new();
    let weights = surface.pole_weights();
    let weight_scale = weights.as_ref().map_or(1.0, |weights| {
        weights
            .iter()
            .map(|weight| weight.get())
            .fold(0.0, f64::max)
    });
    for v in 0..surface.v_count() {
        let mut controls = Vec::new();
        for u in 0..surface.u_count() {
            let p = chart
                .placement
                .point(surface.pole(u, v).ok_or_else(no_bound)?.get());
            let w = surface
                .weight(u, v)
                .map_or(1.0, cadmpeg_ir::scalar::NonZeroReal::get)
                / weight_scale;
            let control = [p.x * w, p.y * w, p.z * w, w];
            if [p.x, p.y, p.z]
                .iter()
                .zip(control)
                .any(|(raw, value)| *raw != 0.0 && value == 0.0)
            {
                return Err(no_bound());
            }
            controls.push(control);
        }
        charge(
            eval,
            &[
                surface.u_knots().as_slice().len(),
                controls.len(),
                controls.len(),
            ],
        )?;
        if controls
            .iter()
            .any(|p| p[3] <= 0.0 || p.iter().any(|value| !value.is_finite()))
        {
            return Err(no_bound());
        }
        columns.push(
            homogeneous_spans(
                usize::try_from(surface.u_degree()).map_err(|_| no_bound())?,
                surface.u_knots().as_slice(),
                &controls,
            )
            .ok_or_else(no_bound)?,
        );
    }
    let mut result = Vec::new();
    for span in 0..columns[0].len() {
        let mut rows = Vec::new();
        for u in 0..columns[0][span].controls.len() {
            let controls: Vec<_> = columns
                .iter()
                .map(|column| column[span].controls[u])
                .collect();
            charge(
                eval,
                &[
                    surface.v_knots().as_slice().len(),
                    controls.len(),
                    controls.len(),
                ],
            )?;
            rows.push(
                homogeneous_spans(
                    usize::try_from(surface.v_degree()).map_err(|_| no_bound())?,
                    surface.v_knots().as_slice(),
                    &controls,
                )
                .ok_or_else(no_bound)?,
            );
        }
        for v in 0..rows[0].len() {
            charge(eval, &[rows.len(), rows[0][v].controls.len()])?;
            if result.len() == MAX_NURBS_BOX_PATCHES {
                return Err(no_bound());
            }
            result.push(Patch {
                domain: [columns[0][span].domain, rows[0][v].domain],
                rows: rows.iter().map(|row| row[v].controls.clone()).collect(),
                axis: 0,
            });
        }
    }
    Ok(result)
}

fn crosses(domain: [[f64; 2]; 2], a: [f64; 2], b: [f64; 2]) -> bool {
    let mut range = [0.0_f64, 1.0_f64];
    for axis in 0..2 {
        let delta = b[axis] - a[axis];
        if delta == 0.0 {
            if a[axis] < domain[axis][0] || a[axis] > domain[axis][1] {
                return false;
            }
        } else {
            let first = (domain[axis][0] - a[axis]) / delta;
            let last = (domain[axis][1] - a[axis]) / delta;
            range[0] = range[0].max(first.min(last));
            range[1] = range[1].min(first.max(last));
            if range[0] > range[1] {
                return false;
            }
        }
    }
    true
}

pub(super) fn bound(
    eval: &Eval<'_, '_>,
    chart: &Chart<'_>,
    region: &Region,
    trims: Trims<'_, '_>,
    boundary: Option<Aabb>,
    tolerance: f64,
    complement: bool,
) -> Measure<(Aabb, f64)> {
    let slice = eval
        .work
        .local_slice(MAX_NURBS_BOX_WORK.min(eval.work.remaining() / 16));
    let local = Eval {
        work: &slice,
        ..*eval
    };
    refine(
        &local, chart, region, trims, boundary, tolerance, complement,
    )
}

fn refine(
    eval: &Eval<'_, '_>,
    chart: &Chart<'_>,
    region: &Region,
    trims: Trims<'_, '_>,
    boundary: Option<Aabb>,
    tolerance: f64,
    complement: bool,
) -> Measure<(Aabb, f64)> {
    let shapes = trims.shapes;
    let Chart::Nurbs(nurbs) = chart else {
        return Err(no_bound());
    };
    if nurbs.evaluation.is_some()
        || nurbs.offset_reach != 0.0
        || nurbs.periods.iter().any(Option::is_some)
            && region.loops.iter().any(|role| role.factor != 0.0)
    {
        return Err(no_bound());
    }
    charge(eval, &[nurbs.surface.u_count(), nurbs.surface.v_count()])?;
    let hull = nurbs.control_hull().map_err(|reason| {
        MeasureFailure::unavailable(reason, None, "the carrier has no positive control hull")
    })?;
    let precision = tolerance.max(hull.diagonal() * BOX_RELATIVE_RESOLUTION);
    if !precision.is_finite() {
        return Err(no_bound());
    }
    if boundary.is_some_and(|inner| within(hull, inner, precision)) {
        return Ok((boundary.ok_or_else(no_bound)?, precision));
    }
    let member = |uv| -> Measure<bool> {
        eval.charge(
            shapes
                .iter()
                .flatten()
                .try_fold(0_usize, |sum, shape| sum.checked_add(shape.points.len()))
                .ok_or_else(no_bound)?,
        )?;
        let mut indicator = region.exterior;
        for (shape, role) in shapes.iter().zip(&region.loops) {
            if let Some(shape) = shape {
                let winding = winding_number(&shape.points, uv);
                indicator += role.factor
                    * cadmpeg_core::convert::f64_from_i64(winding).ok_or_else(no_bound)?;
            }
        }
        Ok(if complement {
            indicator == 0.0
        } else {
            indicator == 1.0
        })
    };
    let active = |domain: [[f64; 2]; 2]| -> Measure<bool> {
        if member(domain.map(|range| range[0].midpoint(range[1])))? {
            return Ok(true);
        }
        for (shape, role) in shapes.iter().zip(&region.loops) {
            if role.factor == 0.0 {
                continue;
            }
            if let Some(shape) = shape {
                eval.charge(shape.points.len())?;
                if shape.points.iter().enumerate().any(|(index, a)| {
                    crosses(domain, *a, shape.points[(index + 1) % shape.points.len()])
                }) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    };
    let mut pending = patches(eval, nurbs)?;
    if let (true, Some(resolved)) = (
        region.exterior == if complement { 1.0 } else { 0.0 },
        trims.resolved,
    ) {
        let ranges = boundary_parameter_ranges(eval, chart, resolved)?;
        let domain = ranges.map(|range| {
            [
                range.min - range.numeric_error,
                range.max + range.numeric_error,
            ]
        });
        let mut clipped = Vec::new();
        for patch in pending {
            patch.charge(eval)?;
            if let Some(patch) = patch.clip(domain)? {
                clipped.push(patch);
            }
        }
        pending = clipped;
    }
    let mut result = boundary;
    let mut visited = 0;
    while let Some(patch) = pending.pop() {
        visited += 1;
        if visited > MAX_NURBS_BOX_PATCHES {
            return Err(no_bound());
        }
        patch.charge(eval)?;
        let middle = patch.domain.map(|range| range[0].midpoint(range[1]));
        if !active(patch.domain)? {
            continue;
        }
        for uv in [
            middle,
            [patch.domain[0][0], patch.domain[1][0]],
            [patch.domain[0][0], patch.domain[1][1]],
            [patch.domain[0][1], patch.domain[1][0]],
            [patch.domain[0][1], patch.domain[1][1]],
        ] {
            if member(uv)? {
                let point = chart.partials(eval, uv[0], uv[1])?.point;
                result = Some(result.map_or_else(
                    || Aabb::of_point(point),
                    |mut aabb| {
                        aabb.include(point);
                        aabb
                    },
                ));
            }
        }
        let outer = patch.hull();
        if result.is_some_and(|inner| within(outer, inner, precision)) {
            continue;
        }
        if patch.domain.iter().any(|range| {
            range[0].midpoint(range[1]) == range[0] || range[0].midpoint(range[1]) == range[1]
        }) {
            return Err(no_bound());
        }
        let mut alternate = patch.clone();
        alternate.axis = 1 - patch.axis;
        let first = patch.split()?;
        let second = alternate.split()?;
        let score = |pair: &(Patch, Patch)| -> Measure<f64> {
            let inner = result.unwrap_or(hull);
            let mut score = 0.0_f64;
            for child in [&pair.0, &pair.1] {
                if active(child.domain)? {
                    let outer = child.hull();
                    let excess = (0..3)
                        .map(|axis| {
                            (inner.min[axis] - outer.min[axis]).max(0.0)
                                + (outer.max[axis] - inner.max[axis]).max(0.0)
                        })
                        .sum::<f64>();
                    score = score.max(excess);
                }
            }
            Ok(score)
        };
        let (a, b) = if score(&second)? < score(&first)? {
            second
        } else {
            first
        };
        pending.push(a);
        pending.push(b);
    }
    Ok((result.ok_or_else(no_bound)?, precision))
}

#[cfg(test)]
mod tests;
