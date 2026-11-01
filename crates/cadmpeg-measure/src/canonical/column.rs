// SPDX-License-Identifier: Apache-2.0
//! A chart reduced to its `v`-isoline at one `u`, for inner integrals
//! across `v`.

use cadmpeg_core::convert::f64_from_index;
use cadmpeg_ir::geometry::nurbs::NurbsSurface;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::NonZeroReal;

use super::placement::Placement;
use super::surface::{Chart, Partials};
use crate::context::Eval;
use crate::failure::Measure;

/// The highest degree a column evaluates with stack basis buffers.
const MAX_COLUMN_DEGREE: usize = 15;

/// Basis values of one degree, on the stack.
type Basis = [f64; MAX_COLUMN_DEGREE + 1];

/// A chart along its `v`-isoline at one `u`.
pub(crate) struct Column<'a, 'ir> {
    chart: &'a Chart<'ir>,
    u: f64,
    isoline: Option<Isoline<'a>>,
}

/// A NURBS isoline in homogeneous form, with its `u`-partial.
struct Isoline<'a> {
    knots: &'a [f64],
    degree: usize,
    domain: [f64; 2],
    /// `Σᵢ Nᵢ(u) Pʷᵢⱼ` for each `j`.
    controls: Vec<[f64; 4]>,
    /// `Σᵢ Nᵢ'(u) Pʷᵢⱼ` for each `j`.
    across: Vec<[f64; 4]>,
    placement: Placement,
}

impl<'a, 'ir> Column<'a, 'ir> {
    /// The column of `chart` at `u`.
    pub(crate) fn new(eval: &Eval<'_, '_>, chart: &'a Chart<'ir>, u: f64) -> Measure<Self> {
        let isoline = match chart {
            Chart::Nurbs(nurbs) => {
                Isoline::new(eval, nurbs.surface, u, nurbs.domain, nurbs.placement)?
            }
            _ => None,
        };
        Ok(Self { chart, u, isoline })
    }

    /// The point and first partials at `(u, s)`.
    pub(crate) fn partials(&self, eval: &Eval<'_, '_>, s: f64) -> Measure<Partials> {
        if let Some(isoline) = &self.isoline {
            if let Some(partials) = isoline.partials(eval, s)? {
                return Ok(partials);
            }
        }
        self.chart.partials(eval, self.u, s)
    }
}

impl<'a> Isoline<'a> {
    fn new(
        eval: &Eval<'_, '_>,
        surface: &'a NurbsSurface,
        u: f64,
        [[u0, u1], v]: [[f64; 2]; 2],
        placement: Placement,
    ) -> Measure<Option<Self>> {
        if surface.u_periodic() || surface.v_periodic() || !(u0..=u1).contains(&u) {
            return Ok(None);
        }
        let (Ok(u_degree), Ok(v_degree)) = (
            usize::try_from(surface.u_degree()),
            usize::try_from(surface.v_degree()),
        ) else {
            return Ok(None);
        };
        if u_degree > MAX_COLUMN_DEGREE || v_degree > MAX_COLUMN_DEGREE {
            return Ok(None);
        }
        let u_knots = surface.u_knots().as_slice();
        let (u_count, v_count) = (surface.u_count(), surface.v_count());
        let Some(span) = span(u_knots, u_degree, u_count, u) else {
            return Ok(None);
        };
        let mut values: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
        let mut slopes: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
        if !basis(u_knots, u_degree, span, u, &mut values, &mut slopes) {
            return Ok(None);
        }
        eval.charge((u_degree + 1) * (v_count + u_degree + 1))?;
        let mut controls = Vec::new();
        let mut across = Vec::new();
        for j in 0..v_count {
            let mut control = [0.0; 4];
            let mut partial = [0.0; 4];
            for r in 0..=u_degree {
                let row = span + r - u_degree;
                let Some(pole) = surface.pole(row, j) else {
                    return Ok(None);
                };
                let weight = surface.weight(row, j).map_or(1.0, NonZeroReal::get);
                let pole = pole.get();
                let homogeneous = [pole.x * weight, pole.y * weight, pole.z * weight, weight];
                for lane in 0..4 {
                    control[lane] += values[r] * homogeneous[lane];
                    partial[lane] += slopes[r] * homogeneous[lane];
                }
            }
            controls.push(control);
            across.push(partial);
        }
        Ok(Some(Self {
            knots: surface.v_knots().as_slice(),
            degree: v_degree,
            domain: v,
            controls,
            across,
            placement,
        }))
    }

    /// The point and first partials at `s`, or `None` outside the domain or
    /// where the isoline has no finite value.
    fn partials(&self, eval: &Eval<'_, '_>, s: f64) -> Measure<Option<Partials>> {
        let [v0, v1] = self.domain;
        if !(v0..=v1).contains(&s) {
            return Ok(None);
        }
        let Some(span) = span(self.knots, self.degree, self.controls.len(), s) else {
            return Ok(None);
        };
        let mut values: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
        let mut slopes: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
        if !basis(self.knots, self.degree, span, s, &mut values, &mut slopes) {
            return Ok(None);
        }
        eval.charge((self.degree + 1) * (self.degree + 2))?;
        let mut point = [0.0; 4];
        let mut along = [0.0; 4];
        let mut across = [0.0; 4];
        for r in 0..=self.degree {
            let index = span + r - self.degree;
            let (control, partial) = (self.controls[index], self.across[index]);
            for lane in 0..4 {
                point[lane] += values[r] * control[lane];
                along[lane] += slopes[r] * control[lane];
                across[lane] += values[r] * partial[lane];
            }
        }
        let weight = point[3];
        if !(weight.is_finite() && weight != 0.0) {
            return Ok(None);
        }
        let at = Point3::new(point[0] / weight, point[1] / weight, point[2] / weight);
        let derivative = |lanes: [f64; 4]| {
            Vector3::new(
                (lanes[0] - at.x * lanes[3]) / weight,
                (lanes[1] - at.y * lanes[3]) / weight,
                (lanes[2] - at.z * lanes[3]) / weight,
            )
        };
        let (du, dv) = (derivative(across), derivative(along));
        let finite =
            |vector: Vector3| vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite();
        if !(at.x.is_finite() && at.y.is_finite() && at.z.is_finite() && finite(du) && finite(dv)) {
            return Ok(None);
        }
        Ok(Some(Partials {
            point: self.placement.point(at),
            du: self.placement.vector(du),
            dv: self.placement.vector(dv),
        }))
    }
}

/// The knot span of `t` in a clamped knot vector, or `None` outside the
/// domain.
fn span(knots: &[f64], degree: usize, count: usize, t: f64) -> Option<usize> {
    let low = *knots.get(degree)?;
    let high = *knots.get(count)?;
    if !(low..=high).contains(&t) || count <= degree {
        return None;
    }
    if t >= high {
        return (degree..count).rev().find(|&i| knots[i] < knots[i + 1]);
    }
    let found = knots[degree..=count].partition_point(|knot| *knot <= t) + degree;
    found.checked_sub(1)
}

/// The nonzero basis values of `degree` at `t` in `span` and their first
/// derivatives; `false` where one is not finite.
fn basis(
    knots: &[f64],
    degree: usize,
    span: usize,
    t: f64,
    values: &mut Basis,
    slopes: &mut Basis,
) -> bool {
    let p = degree;
    let mut left = [0.0; MAX_COLUMN_DEGREE + 1];
    let mut right = [0.0; MAX_COLUMN_DEGREE + 1];
    // `table[j][r]` for `r <= j` holds knot differences; above it, basis
    // values of each degree.
    let mut table = [[0.0; MAX_COLUMN_DEGREE + 1]; MAX_COLUMN_DEGREE + 1];
    table[0][0] = 1.0;
    for j in 1..=p {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            table[j][r] = right[r + 1] + left[j - r];
            let ratio = if table[j][r] == 0.0 {
                0.0
            } else {
                table[r][j - 1] / table[j][r]
            };
            table[r][j] = saved + right[r + 1] * ratio;
            saved = left[j - r] * ratio;
        }
        table[j][j] = saved;
    }
    let quotient = |numerator: f64, denominator: f64| {
        if denominator == 0.0 {
            0.0
        } else {
            numerator / denominator
        }
    };
    let Some(scale) = f64_from_index(p) else {
        return false;
    };
    for r in 0..=p {
        values[r] = table[r][p];
        slopes[r] = if p == 0 {
            0.0
        } else {
            let rising = if r >= 1 {
                quotient(table[r - 1][p - 1], table[p][r - 1])
            } else {
                0.0
            };
            let falling = if r < p {
                quotient(table[r][p - 1], table[p][r])
            } else {
                0.0
            };
            scale * (rising - falling)
        };
        if !(values[r].is_finite() && slopes[r].is_finite()) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::topology::{BodyKind, Sense};

    use super::Column;
    use crate::budget::{PassAllowance, WorkProfile};
    use crate::canonical::placement::Placement;
    use crate::canonical::surface::{Chart, NurbsChart};
    use crate::context::Eval;
    use crate::test_support::builders::{service_context, Builder};

    /// A rational patch of degree two by three with an interior knot in each
    /// direction, sampled at its domain ends, its knots and between them.
    #[test]
    fn a_column_matches_the_chart_along_its_isoline() {
        let mut builder = Builder::new();
        let rows: Vec<Vec<([f64; 3], f64)>> = (0..4)
            .map(|i| {
                (0..5)
                    .map(|j| {
                        let (x, y) = (f64::from(i), f64::from(j));
                        let z = (x * 0.7 + y * 1.3).sin();
                        ([x, y, z], 1.0 + 0.25 * f64::from((i + j) % 3))
                    })
                    .collect()
            })
            .collect();
        let patch = builder.nurbs(
            (2, vec![0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0]),
            (3, vec![0.0, 0.0, 0.0, 0.0, 0.6, 1.0, 1.0, 1.0, 1.0]),
            &rows,
        );
        builder.face(&patch, Sense::Forward, &[], false);
        let ir = builder.finish(BodyKind::Sheet);
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) =
            &ir.model.surfaces[0].geometry
        else {
            panic!("a NURBS surface");
        };
        let chart = Chart::Nurbs(NurbsChart::new(surface, Placement::identity()).unwrap());
        let ctx = service_context();
        let index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir);
        let pass = PassAllowance::new(WorkProfile::Desktop.pass_allowance(1).unwrap());
        let slice = pass.face_slice();
        let eval = Eval {
            ctx: &ctx,
            index: &index,
            work: &slice,
        };
        for u in [0.0, 0.17, 0.4, 0.83, 1.0] {
            let column = Column::new(&eval, &chart, u).unwrap();
            assert!(column.isoline.is_some());
            for s in [0.0, 0.31, 0.6, 0.94, 1.0] {
                let fast = column.partials(&eval, s).unwrap();
                let exact = chart.partials(&eval, u, s).unwrap();
                assert!(fast.point.distance(exact.point) < 1.0e-13, "{u} {s}");
                assert!((fast.du - exact.du).norm() < 1.0e-12, "{u} {s}");
                assert!((fast.dv - exact.dv).norm() < 1.0e-12, "{u} {s}");
            }
        }
    }
}
