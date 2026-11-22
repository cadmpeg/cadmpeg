// SPDX-License-Identifier: Apache-2.0
//! The carrier surface of a face: evaluation, inversion, periods, and the
//! parameter domain of a face that has no edge boundary.

use cadmpeg_ir::math::portable::Portable;
use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, TAU};

use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::compare::floats_agree;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::{
    analytic_surface_parameters_solved, finite_or_refusal, model_curve_point_by_id,
    model_surface_partials_by_id, model_surface_point_by_id, nurbs_surface_parameter_near_point,
    SurfacePartials,
};
use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;
use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::index::{ModelIndex, StandardIndex};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

use crate::face::Refusal;
use crate::plane::{angle, cross, distance, dot, length, scaled, sub, Uv};

/// Ratio of a parameter's distance from a periodic domain end to the domain
/// span within which a projection stopped at the seam.
const SEAM_RATIO: f64 = 1e-9;
/// Ratio of an unseeded NURBS projection's distance from its point to the
/// surface size above which the projection missed.
const MISS_RATIO: f64 = 1e-6;
/// Ratio of a seeded NURBS projection's distance from its point to the
/// surface size above which the projection stopped short. A seed on the
/// right turn of a wound surface keeps a smaller gap from an edge that lies
/// off the surface.
const SEEDED_MISS_RATIO: f64 = 1e-3;
/// Grid steps per parameter of the search that restarts a missed
/// projection.
const DENSE_GRID: u16 = 64;
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
    /// The `v` of each pole circle, where a self-intersecting torus meets
    /// its axis and the whole `u` circle collapses to one point.
    pub(crate) pole_circles: Vec<f64>,
    /// The swept directrix of a procedural carrier with no bounded domain.
    sweep: Option<Sweep<'ir>>,
    /// Surface points already evaluated, by the bits of their parameters.
    points: RefCell<HashMap<[u64; 2], Option<Point3>>>,
    /// Surface partials already evaluated, by the bits of their parameters.
    partials: RefCell<HashMap<[u64; 2], Option<SurfacePartials>>>,
}

/// The cache key of the parameters `uv`.
fn bits(uv: Uv) -> [u64; 2] {
    uv.map(f64::to_bits)
}

impl Sweep<'_> {
    /// The surface parameter that runs along the directrix.
    const fn directrix_axis(&self) -> usize {
        match self.motion {
            Motion::Linear { .. } => 0,
            Motion::Revolution { .. } => 1,
        }
    }
}

/// How a procedural carrier moves its directrix `C`.
#[derive(Clone, Copy)]
enum Motion {
    /// `S(u, v) = C(u) + v·direction`.
    Linear { direction: Vector3 },
    /// `S(u, v)` turns `C(v)` by the angle `u` about the axis.
    Revolution { origin: Point3, axis: Vector3 },
}

/// A directrix, its parameter domain, and its motion.
#[derive(Clone, Copy)]
struct Sweep<'ir> {
    directrix: &'ir CurveId,
    domain: [f64; 2],
    motion: Motion,
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
        let solved = surface
            .geometry
            .solved()
            .filter(|solved| !matches!(solved, SolvedSurfaceGeometry::Unknown { .. }));
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
            pole_circles: Vec::new(),
            sweep: None,
            points: RefCell::new(HashMap::new()),
            partials: RefCell::new(HashMap::new()),
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
                let (major, minor) = (torus.major_radius().get(), torus.minor_radius().get());
                if major < minor.abs() {
                    let pole = (-major / minor).portable_acos();
                    carrier.pole_circles = vec![pole, TAU - pole];
                }
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
            match index
                .procedural_surface_for_surface(id.as_str(), StandardIndex)
                .map(cadmpeg_ir::geometry::ProceduralSurface::definition)
            {
                Some(ProceduralSurfaceDefinition::AxisRevolution(revolution)) => {
                    carrier.periods = [Some(TAU), None];
                    if let Some((domain, periodic)) =
                        directrix_domain(index, revolution.directrix())
                    {
                        if periodic {
                            carrier.periods[1] = Some(domain[1] - domain[0]);
                        }
                        carrier.sweep = Some(Sweep {
                            directrix: revolution.directrix(),
                            domain,
                            motion: Motion::Revolution {
                                origin: revolution.axis_origin().get(),
                                axis: Vector3::from(revolution.axis_direction()),
                            },
                        });
                    }
                }
                Some(ProceduralSurfaceDefinition::Subset(subset)) => {
                    let spans = subset.parameter_ranges().map(|range| {
                        let [start, end] = range
                            .finite_endpoints()
                            .map(cadmpeg_ir::scalar::FiniteReal::get);
                        (end - start).abs()
                    });
                    carrier.domain = Some([[0.0, spans[0]], [0.0, spans[1]]]);
                }
                Some(ProceduralSurfaceDefinition::LinearSweep(linear)) => {
                    if let Some((domain, periodic)) = directrix_domain(index, linear.directrix()) {
                        if periodic {
                            carrier.periods[0] = Some(domain[1] - domain[0]);
                        }
                        carrier.sweep = Some(Sweep {
                            directrix: linear.directrix(),
                            domain,
                            motion: Motion::Linear {
                                direction: linear.direction().get(),
                            },
                        });
                    }
                }
                _ => {}
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

    /// `uv` with each periodic parameter of a NURBS carrier moved by whole
    /// periods into the knot domain, outside which the carrier extrapolates
    /// instead of repeating. A trace past the seam lies on another branch.
    fn in_domain(&self, uv: Uv) -> Uv {
        let (Some(SolvedSurfaceGeometry::Nurbs(_)), Some(domain)) =
            (self.solved.map(basis), self.domain)
        else {
            return uv;
        };
        let mut moved = uv;
        for axis in 0..2 {
            if let Some(period) = self.periods[axis] {
                let [low, high] = domain[axis];
                if moved[axis] < low || moved[axis] > high {
                    moved[axis] = low + (moved[axis] - low).rem_euclid(period);
                }
            }
        }
        moved
    }

    pub(crate) fn point(&self, uv: Uv) -> Result<Option<Point3>, Refusal> {
        if let Some(known) = self.points.borrow().get(&bits(uv)) {
            return Ok(*known);
        }
        let point = finite_or_refusal(EvaluationAdmission::Standard.within_work_slice(
            self.work,
            |admission| {
                let [u, v] = self.in_domain(uv);
                model_surface_point_by_id(admission, self.index, self.id, u, v)
            },
        ))
        .map(|point| point.map(cadmpeg_ir::features::geometry::FinitePoint3::get))
        .map_err(resource)?;
        self.points.borrow_mut().insert(bits(uv), point);
        Ok(point)
    }

    pub(crate) fn partials(&self, uv: Uv) -> Result<Option<SurfacePartials>, Refusal> {
        if let Some(known) = self.partials.borrow().get(&bits(uv)) {
            return Ok(*known);
        }
        let partials = finite_or_refusal(EvaluationAdmission::Standard.within_work_slice(
            self.work,
            |admission| {
                let [u, v] = self.in_domain(uv);
                model_surface_partials_by_id(admission, self.index, self.id, u, v)
            },
        ))
        .map(|partials| partials.map(SurfacePartials::into_raw))
        .map_err(resource)?;
        self.partials.borrow_mut().insert(bits(uv), partials);
        Ok(partials)
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
        // A projection that starts on a pole cannot leave it.
        let seed = match seed {
            Some(uv) if self.at_pole(uv)? => None,
            seed => seed,
        };
        let found = match self.solved {
            Some(SolvedSurfaceGeometry::Nurbs(nurbs)) => self.invert_nurbs(nurbs, point, seed)?,
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

    /// The parameters of the NURBS point nearest `point` from the bounded
    /// projection, on the branch of each periodic parameter nearest `seed`.
    /// The projection stays inside the knot domain, so a projection that
    /// stops at a periodic seam also starts from the other side, and the
    /// nearer result stands.
    fn invert_nurbs(
        &self,
        nurbs: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
        point: Point3,
        seed: Option<Uv>,
    ) -> Result<Option<Uv>, Refusal> {
        let project = |seed: Option<Uv>| -> Result<Option<Uv>, Refusal> {
            Ok(nurbs_surface_parameter_near_point(
                EvaluationAdmission::Standard,
                nurbs,
                point,
                seed.map(|[u, v]| Point2::new(u, v)),
            )
            .map_err(resource)?
            .map(|uv| [uv.get().u, uv.get().v]))
        };
        let Some(domain) = self.domain else {
            return project(seed);
        };
        let wrapped = seed.map(|seed| {
            let mut wrapped = seed;
            for axis in 0..2 {
                if let Some(period) = self.periods[axis] {
                    wrapped[axis] =
                        domain[axis][0] + (seed[axis] - domain[axis][0]).rem_euclid(period);
                }
            }
            wrapped
        });
        let Some(mut found) = project(wrapped)? else {
            return Ok(None);
        };
        let gap = |uv: Uv| -> Result<f64, Refusal> {
            Ok(self
                .point(uv)?
                .map_or(f64::INFINITY, |on| distance(on, point)))
        };
        // A projection can stop short of its point: from its seed in a local
        // minimum, or with no seed from a coarse grid in the wrong turn of a
        // surface wound many times. A miss starts again from the nearest
        // point of a dense grid.
        let miss = if seed.is_some() {
            SEEDED_MISS_RATIO
        } else {
            MISS_RATIO
        };
        if gap(found)? > miss * self.size {
            let mut best: Option<(f64, Uv)> = None;
            let samples = f64::from(DENSE_GRID);
            for i in 0..=DENSE_GRID {
                for j in 0..=DENSE_GRID {
                    let uv = [
                        domain[0][0] + (domain[0][1] - domain[0][0]) * f64::from(i) / samples,
                        domain[1][0] + (domain[1][1] - domain[1][0]) * f64::from(j) / samples,
                    ];
                    let distance = gap(uv)?;
                    if best.is_none_or(|(nearest, _)| distance < nearest) {
                        best = Some((distance, uv));
                    }
                }
            }
            if let Some((_, start)) = best {
                if let Some(again) = project(Some(start))? {
                    if gap(again)? < gap(found)? {
                        found = again;
                    }
                }
            }
        }

        for axis in 0..2 {
            if self.periods[axis].is_none() {
                continue;
            }
            let [low, high] = domain[axis];
            let scale = (high - low).abs().max(1.0);
            let at_end = |value: f64, end: f64| (value - end).abs() <= SEAM_RATIO * scale;
            let opposite = if at_end(found[axis], high) {
                Some(low)
            } else if at_end(found[axis], low) {
                Some(high)
            } else {
                None
            };
            let Some(opposite) = opposite else {
                continue;
            };
            let mut restart = found;
            restart[axis] = opposite;
            if let Some(other) = project(Some(restart))? {
                let gap = |uv: Uv| -> Result<f64, Refusal> {
                    Ok(self
                        .point(uv)?
                        .map_or(f64::INFINITY, |on| distance(on, point)))
                };
                if gap(other)? < gap(found)? {
                    found = other;
                }
            }
        }
        if let Some(seed) = seed {
            for axis in 0..2 {
                found[axis] = self.nearest_branch(axis, found[axis], seed[axis]);
            }
        }
        Ok(Some(found))
    }

    /// Whether a partial derivative vanishes at `uv`.
    fn at_pole(&self, uv: Uv) -> Result<bool, Refusal> {
        let Some(partials) = self.partials(uv)? else {
            return Ok(false);
        };
        let limit = self.size * SINGULAR_RATIO;
        Ok(length(partials.du) <= limit || length(partials.dv) <= limit)
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
            return match self.sweep {
                Some(sweep) => self.sweep_seed(sweep, point),
                None => Ok(None),
            };
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

    /// The parameters nearest `point` among samples of the directrix of
    /// `sweep`, each moved to the closest place its motion reaches.
    fn sweep_seed(&self, sweep: Sweep<'_>, point: Point3) -> Result<Option<Uv>, Refusal> {
        let mut best: Option<(f64, Uv)> = None;
        let samples = u32::from(SEED_GRID) * u32::from(SEED_GRID);
        for step in 0..=samples {
            let t = sweep.domain[0]
                + (sweep.domain[1] - sweep.domain[0]) * f64::from(step) / f64::from(samples);
            let on = finite_or_refusal(
                EvaluationAdmission::Standard.within_work_slice(self.work, |admission| {
                    model_curve_point_by_id(admission, self.index, sweep.directrix, t)
                }),
            )
            .map_err(resource)?;
            let Some(on) = on.map(cadmpeg_ir::features::geometry::FinitePoint3::get) else {
                continue;
            };
            let (gap, uv) = match sweep.motion {
                Motion::Linear { direction } => {
                    let v = dot(sub(point, on), direction) / dot(direction, direction);
                    let reached = sub(point, on) - scaled(direction, v);
                    (length(reached), [t, v])
                }
                Motion::Revolution { origin, axis } => {
                    let split = |at: Point3| {
                        let relative = sub(at, origin);
                        let along = dot(relative, axis);
                        (along, relative - scaled(axis, along))
                    };
                    let (point_along, point_radial) = split(point);
                    let (on_along, on_radial) = split(on);
                    let gap = (point_along - on_along)
                        .portable_hypot(length(point_radial) - length(on_radial));
                    let turn = dot(axis, cross(on_radial, point_radial))
                        .portable_atan2(dot(on_radial, point_radial));
                    (gap, [turn, t])
                }
            };
            if gap.is_finite() && best.is_none_or(|(nearest, _)| gap < nearest) {
                best = Some((gap, uv));
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
            } else if let Some(sweep) = self.sweep {
                let axis = sweep.directrix_axis();
                if self.periods[axis].is_none() {
                    uv[axis] = uv[axis].clamp(sweep.domain[0], sweep.domain[1]);
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

/// The parameter domain of a directrix and whether it is periodic. A
/// trimmed curve runs from zero to its span; another curve has the natural
/// domain of its form.
fn directrix_domain(index: &ModelIndex<'_>, curve: &CurveId) -> Option<([f64; 2], bool)> {
    if let Some(procedural) = index
        .procedural_curves_for_curve(curve.as_str(), StandardIndex)
        .and_then(|procedurals| procedurals.first().copied())
    {
        let ProceduralCurveDefinition::Subset(subset) = procedural.definition() else {
            return None;
        };
        let [start, end] = subset.parameter_range().endpoints();
        return Some(([0.0, (end - start).abs()], false));
    }
    let geometry = index
        .curves(curve.as_str(), StandardIndex)
        .and_then(|curve| curve.geometry.solved())?;
    crate::boundary::natural_domain(geometry)
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
