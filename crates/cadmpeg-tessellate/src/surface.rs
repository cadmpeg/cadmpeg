// SPDX-License-Identifier: Apache-2.0
//! The carrier surface of a face: evaluation, inversion, periods, and the
//! parameter domain of a face that has no edge boundary.

use cadmpeg_ir::math::portable::Portable;
use std::f64::consts::{FRAC_PI_2, TAU};

use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::compare::floats_agree;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::{
    analytic_surface_parameters_solved, finite_or_refusal, model_surface_partials_by_id,
    model_surface_point_by_id, nurbs_surface_parameter_near_point, SurfacePartials,
};
use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::index::{ModelIndex, StandardIndex};
use cadmpeg_ir::math::{Point2, Point3};

use crate::face::Refusal;
use crate::plane::{angle, cross, distance, dot, length, sub, Uv};

/// Gauss-Newton steps of one point projection.
const PROJECTION_STEPS: usize = 32;
/// Relative step size at which a projection stops.
const PROJECTION_CONVERGED: f64 = 1e-13;
/// Grid steps per parameter of the search that seeds a projection.
const SEED_GRID: u8 = 16;
/// Share of a face's parameter span across which finite-difference curvature
/// measures the turn of the unit normal.
const CURVATURE_STEP: f64 = 1e-3;
/// Ratio of a partial derivative's length to the surface size below which
/// the parameter is singular at a point.
const SINGULAR_RATIO: f64 = 1e-9;

/// The evaluation state shared by one face.
pub(crate) struct Carrier<'t, 'ir> {
    pub(crate) index: &'t ModelIndex<'ir>,
    pub(crate) work: &'t WorkBudget,
    pub(crate) id: &'ir SurfaceId,
    /// The solved geometry, when the carrier has one.
    solved: Option<&'ir SolvedSurfaceGeometry>,
    /// The period of each parameter.
    pub(crate) periods: [Option<f64>; 2],
    /// The parameter domain of the whole carrier, when it is bounded.
    pub(crate) domain: Option<[[f64; 2]; 2]>,
    /// A length that sets the scale of the surface.
    pub(crate) size: f64,
    /// `1` when the carrier's oriented normal is `∂S/∂u × ∂S/∂v`, `-1` when it
    /// is the opposite.
    pub(crate) normal_sign: f64,
    /// The `v` of a cone apex.
    pub(crate) apex: Option<f64>,
}

fn resource(limit: impl std::fmt::Debug) -> Refusal {
    Refusal::new(format!(
        "an evaluation exceeded a resource limit: {limit:?}"
    ))
}

/// The solved form of a carrier, looking through placements.
fn basis(geometry: &SolvedSurfaceGeometry) -> &SolvedSurfaceGeometry {
    match geometry {
        SolvedSurfaceGeometry::Transformed(placed) => basis(placed.basis()),
        other => other,
    }
}

impl<'t, 'ir> Carrier<'t, 'ir> {
    pub(crate) fn new(
        index: &'t ModelIndex<'ir>,
        work: &'t WorkBudget,
        id: &'ir SurfaceId,
    ) -> Result<Self, Refusal> {
        let surface = index
            .surfaces(id.as_str(), StandardIndex)
            .ok_or_else(|| Refusal::new(format!("the surface {id} is not in the model")))?;
        let solved = match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) => None,
            geometry => geometry.solved(),
        };
        let mut carrier = Self {
            index,
            work,
            id,
            solved,
            periods: [None, None],
            domain: None,
            size: 1.0,
            normal_sign: 1.0,
            apex: None,
        };
        match solved.map(basis) {
            Some(SolvedSurfaceGeometry::Cylinder(cylinder)) => {
                carrier.periods = [Some(TAU), None];
                carrier.size = cylinder.radius().get();
            }
            Some(SolvedSurfaceGeometry::Cone(cone)) => {
                carrier.periods = [Some(TAU), None];
                carrier.size = cone.radius().get().max(1.0);
                let slope = cone.half_angle().get().portable_tan();
                if slope != 0.0 && slope.is_finite() {
                    carrier.apex = Some(-cone.radius().get() / slope);
                }
            }
            Some(SolvedSurfaceGeometry::Sphere(sphere)) => {
                carrier.periods = [Some(TAU), None];
                carrier.domain = Some([[0.0, TAU], [-FRAC_PI_2, FRAC_PI_2]]);
                carrier.size = sphere.radius().get().abs();
            }
            Some(SolvedSurfaceGeometry::Torus(torus)) => {
                carrier.periods = [Some(TAU), Some(TAU)];
                carrier.domain = Some([[0.0, TAU], [0.0, TAU]]);
                carrier.size = torus.major_radius().get();
            }
            Some(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
                if nurbs.normal_reversed() {
                    carrier.normal_sign = -1.0;
                }
                let domain = |knots: &[f64], degree: u32, count: usize| -> Option<[f64; 2]> {
                    let degree = usize::try_from(degree).ok()?;
                    Some([*knots.get(degree)?, *knots.get(count)?])
                };
                let u = domain(
                    nurbs.u_knots().as_slice(),
                    nurbs.u_degree(),
                    nurbs.u_count(),
                );
                let v = domain(
                    nurbs.v_knots().as_slice(),
                    nurbs.v_degree(),
                    nurbs.v_count(),
                );
                if let (Some(u), Some(v)) = (u, v) {
                    carrier.domain = Some([u, v]);
                }
                let poles = nurbs.poles();
                let mut low = [f64::INFINITY; 3];
                let mut high = [f64::NEG_INFINITY; 3];
                for pole in &poles {
                    let point = pole.get();
                    for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
                        low[axis] = low[axis].min(value);
                        high[axis] = high[axis].max(value);
                    }
                }
                let diagonal = (0..3)
                    .map(|axis| (high[axis] - low[axis]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                if diagonal.is_finite() && diagonal > 0.0 {
                    carrier.size = diagonal;
                }
            }
            _ => {}
        }
        if solved.is_none() {
            if let Some(ProceduralSurfaceDefinition::AxisRevolution(_)) = index
                .procedural_surface_for_surface(id.as_str(), StandardIndex)
                .map(cadmpeg_ir::geometry::ProceduralSurface::definition)
            {
                carrier.periods = [Some(TAU), None];
            }
        }
        if matches!(solved.map(basis), Some(SolvedSurfaceGeometry::Nurbs(_))) {
            carrier.detect_closure()?;
        }
        Ok(carrier)
    }

    /// Marks a NURBS parameter periodic when its two domain ends meet.
    fn detect_closure(&mut self) -> Result<(), Refusal> {
        let Some(domain) = self.domain else {
            return Ok(());
        };
        let tolerance = self.size * SINGULAR_RATIO.sqrt();
        for axis in 0..2 {
            let other = 1 - axis;
            let mut closed = true;
            for step in 0..=4_u8 {
                let across = domain[other][0]
                    + (domain[other][1] - domain[other][0]) * f64::from(step) / 4.0;
                let at = |value: f64| -> Uv {
                    let mut uv = [0.0; 2];
                    uv[axis] = value;
                    uv[other] = across;
                    uv
                };
                let (Some(first), Some(last)) = (
                    self.point(at(domain[axis][0]))?,
                    self.point(at(domain[axis][1]))?,
                ) else {
                    closed = false;
                    break;
                };
                if distance(first, last) > tolerance {
                    closed = false;
                    break;
                }
            }
            if closed {
                self.periods[axis] = Some(domain[axis][1] - domain[axis][0]);
            }
        }
        Ok(())
    }

    /// Whether the carrier is a plane, which a boundary triangulation
    /// already meets with no interior vertex.
    pub(crate) fn planar(&self) -> bool {
        matches!(
            self.solved.map(basis),
            Some(SolvedSurfaceGeometry::Plane(_))
        )
    }

    pub(crate) fn point(&self, uv: Uv) -> Result<Option<Point3>, Refusal> {
        finite_or_refusal(
            EvaluationAdmission::Standard.within_work_slice(self.work, |admission| {
                model_surface_point_by_id(admission, self.index, self.id, uv[0], uv[1])
            }),
        )
        .map(|point| point.map(cadmpeg_ir::features::FinitePoint3::get))
        .map_err(resource)
    }

    pub(crate) fn partials(&self, uv: Uv) -> Result<Option<SurfacePartials>, Refusal> {
        finite_or_refusal(
            EvaluationAdmission::Standard.within_work_slice(self.work, |admission| {
                model_surface_partials_by_id(admission, self.index, self.id, uv[0], uv[1])
            }),
        )
        .map(|partials| partials.map(SurfacePartials::into_raw))
        .map_err(resource)
    }

    /// The unnormalized surface normal `∂S/∂u × ∂S/∂v` at `uv`.
    pub(crate) fn normal(&self, uv: Uv) -> Result<Option<cadmpeg_ir::math::Vector3>, Refusal> {
        if self.apex.is_some_and(|apex| floats_agree(uv[1], apex)) {
            return Ok(None);
        }
        Ok(self
            .partials(uv)?
            .map(|partials| cross(partials.du, partials.dv)))
    }

    /// Whether parameter `axis` is singular at `uv`: its partial derivative
    /// vanishes, as on a pole or an apex.
    pub(crate) fn singular(&self, uv: Uv, axis: usize) -> Result<bool, Refusal> {
        let Some(partials) = self.partials(uv)? else {
            return Ok(false);
        };
        let derivative = if axis == 0 { partials.du } else { partials.dv };
        Ok(length(derivative) <= self.size * SINGULAR_RATIO)
    }

    /// The parameters of the surface point nearest `point`, from the closed
    /// form of an analytic carrier, the NURBS projection, or a projection
    /// that starts at `seed`.
    pub(crate) fn invert(&self, point: Point3, seed: Option<Uv>) -> Result<Option<Uv>, Refusal> {
        let found = match self.solved {
            Some(SolvedSurfaceGeometry::Nurbs(nurbs)) => nurbs_surface_parameter_near_point(
                EvaluationAdmission::Standard,
                nurbs,
                point,
                seed.map(|[u, v]| Point2::new(u, v)),
            )
            .map_err(resource)?
            .map(|uv| [uv.get().u, uv.get().v]),
            Some(solved) => {
                analytic_surface_parameters_solved(solved, point).map(|uv| [uv.get().u, uv.get().v])
            }
            None => None,
        };
        match (found, seed) {
            (Some(uv), _) => Ok(Some(uv)),
            (None, Some(seed)) => self.project(point, seed),
            (None, None) if self.at_apex(point)? => Ok(self.apex.map(|apex| [0.0, apex])),
            (None, None) => match self.coarse_seed(point)? {
                Some(seed) => self.project(point, seed),
                None => Ok(None),
            },
        }
    }

    /// Whether `point` is the apex of a cone carrier.
    fn at_apex(&self, point: Point3) -> Result<bool, Refusal> {
        let Some(apex) = self.apex else {
            return Ok(false);
        };
        Ok(self
            .point([0.0, apex])?
            .is_some_and(|on| distance(on, point) <= self.size * SINGULAR_RATIO.sqrt()))
    }

    /// The grid point of the carrier domain nearest `point`.
    fn coarse_seed(&self, point: Point3) -> Result<Option<Uv>, Refusal> {
        let Some(domain) = self.domain else {
            return Ok(None);
        };
        let mut best: Option<(f64, Uv)> = None;
        for i in 0..=SEED_GRID {
            for j in 0..=SEED_GRID {
                let fraction = |step: u8| f64::from(step) / f64::from(SEED_GRID);
                let uv = [
                    domain[0][0] + (domain[0][1] - domain[0][0]) * fraction(i),
                    domain[1][0] + (domain[1][1] - domain[1][0]) * fraction(j),
                ];
                if let Some(on) = self.point(uv)? {
                    let gap = distance(on, point);
                    if best.is_none_or(|(nearest, _)| gap < nearest) {
                        best = Some((gap, uv));
                    }
                }
            }
        }
        Ok(best.map(|(_, uv)| uv))
    }

    /// The parameters of the surface point nearest `point` by Gauss-Newton
    /// steps from `seed`.
    pub(crate) fn project(&self, point: Point3, seed: Uv) -> Result<Option<Uv>, Refusal> {
        let mut uv = seed;
        for _ in 0..PROJECTION_STEPS {
            let Some(partials) = self.partials(uv)? else {
                return Ok(None);
            };
            let residual = sub(point, partials.point);
            let (a, b, c) = (
                dot(partials.du, partials.du),
                dot(partials.du, partials.dv),
                dot(partials.dv, partials.dv),
            );
            let (r1, r2) = (dot(partials.du, residual), dot(partials.dv, residual));
            let determinant = a * c - b * b;
            if !(determinant.is_finite() && determinant > 0.0) {
                return Ok(Some(uv));
            }
            let step = [
                (c * r1 - b * r2) / determinant,
                (a * r2 - b * r1) / determinant,
            ];
            uv = [uv[0] + step[0], uv[1] + step[1]];
            if let Some(domain) = self.domain {
                for axis in 0..2 {
                    if self.periods[axis].is_none() {
                        uv[axis] = uv[axis].clamp(domain[axis][0], domain[axis][1]);
                    }
                }
            }
            let scale = uv[0].abs().max(uv[1].abs()).max(1.0);
            if step[0].abs().max(step[1].abs()) <= PROJECTION_CONVERGED * scale {
                break;
            }
        }
        Ok(Some(uv))
    }

    /// The distance from `point` to the surface, measured from a projection
    /// that starts at `seed`, and the projected parameters.
    pub(crate) fn deviation(&self, point: Point3, seed: Uv) -> Result<Option<(f64, Uv)>, Refusal> {
        let Some(uv) = self.project(point, seed)? else {
            return Ok(None);
        };
        Ok(self.point(uv)?.map(|on| (distance(on, point), uv)))
    }

    /// Normal curvature along parameter `axis` at `uv`. A plane, sphere,
    /// circular cylinder, or circular cone uses the closed form; other
    /// carriers sample the normal across a small share of `span`.
    pub(crate) fn curvature(
        &self,
        uv: Uv,
        axis: usize,
        span: f64,
        scale: f64,
    ) -> Result<f64, Refusal> {
        if let Some(kappa) = self.analytic_curvature(axis, uv) {
            return Ok(kappa);
        }
        let step = span * CURVATURE_STEP;
        let (mut before, mut after) = (uv, uv);
        before[axis] -= step;
        after[axis] += step;
        let (Some(first), Some(second)) = (self.normal(before)?, self.normal(after)?) else {
            return Ok(0.0);
        };
        let length = 2.0 * step * scale;
        if length.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return Ok(0.0);
        }
        Ok(angle(first, second) / length)
    }

    fn analytic_curvature(&self, axis: usize, uv: Uv) -> Option<f64> {
        match self.solved.map(basis) {
            Some(SolvedSurfaceGeometry::Plane(_)) => Some(0.0),
            Some(SolvedSurfaceGeometry::Sphere(sphere)) => {
                let radius = sphere.radius().get().abs();
                (radius > 0.0).then_some(1.0 / radius)
            }
            Some(SolvedSurfaceGeometry::Cylinder(cylinder)) => Some(if axis == 0 {
                1.0 / cylinder.radius().get()
            } else {
                0.0
            }),
            Some(SolvedSurfaceGeometry::Cone(cone)) => {
                if !floats_agree(cone.ratio().get(), 1.0) {
                    return None;
                }
                circular_cone_normal_curvature(
                    axis,
                    uv[1],
                    cone.radius().get(),
                    cone.half_angle().get(),
                )
            }
            _ => None,
        }
    }

    /// `value` moved by whole periods of parameter `axis` to lie nearest
    /// `reference`.
    pub(crate) fn nearest_branch(&self, axis: usize, value: f64, reference: f64) -> f64 {
        match self.periods[axis] {
            Some(period) => value - ((value - reference) / period).round() * period,
            None => value,
        }
    }
}

/// Closed-form normal curvature of a circular cone. The generatrix (`axis`
/// 1) is a straight line.
fn circular_cone_normal_curvature(
    axis: usize,
    v: f64,
    radius: f64,
    half_angle: f64,
) -> Option<f64> {
    if axis != 0 {
        return Some(0.0);
    }
    let rho = radius + v * half_angle.portable_tan();
    if !rho.is_finite() || rho == 0.0 {
        return None;
    }
    Some(half_angle.portable_cos().abs() / rho.abs())
}

#[cfg(test)]
mod tests;
