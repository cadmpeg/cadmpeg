// SPDX-License-Identifier: Apache-2.0
//! Model-space charts of surface carriers.
//!
//! Each analytic chart reproduces the evaluator's parameterization of its
//! carrier with the frame mapped by the composed placement's rotation
//! `Q = L/s`. Frames are explicit triples; handedness is not assumed, so a
//! reflecting placement keeps the chart and reverses the carrier normal
//! `S_u × S_v`. A NURBS chart evaluates the stored surface in its own
//! coordinates and maps points and partials by the composed placement.

use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::geometry::nurbs::NurbsSurface;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

use super::placement::Placement;
use crate::context::Eval;
use crate::failure::{Measure, MeasureFailure};
use crate::geom::{combine, offset, Aabb};
use crate::reason::UnavailableReason;
use crate::tolerance::INVERSION_PARAMETER_RESOLUTION;

/// Gauss–Newton steps refining one point inverted onto a NURBS carrier.
const MAX_INVERSION_REFINEMENT_STEPS: usize = 16;

/// The point and first partials of a chart.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Partials {
    pub(crate) point: Point3,
    pub(crate) du: Vector3,
    pub(crate) dv: Vector3,
}

/// `P = o + h(u·e_u + v·e_v)`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlaneChart {
    pub(crate) origin: Point3,
    pub(crate) eu: Vector3,
    pub(crate) ev: Vector3,
    pub(crate) h: f64,
}

/// `P = o + R(cos u·e1 + sin u·e2) + h·v·a`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CylinderChart {
    pub(crate) origin: Point3,
    pub(crate) e1: Vector3,
    pub(crate) e2: Vector3,
    pub(crate) axis: Vector3,
    pub(crate) radius: f64,
    pub(crate) h: f64,
}

/// `P = o + (ρ0 + τ·v)(cos u·e1 + k·sin u·e2) + h·v·a`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ConeChart {
    pub(crate) origin: Point3,
    pub(crate) e1: Vector3,
    pub(crate) e2: Vector3,
    pub(crate) axis: Vector3,
    pub(crate) rho0: f64,
    pub(crate) tau: f64,
    pub(crate) ratio: f64,
    pub(crate) h: f64,
}

/// `P = c + R(cos v(cos u·e1 + sin u·e2) + sin v·a)` with signed `R`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SphereChart {
    pub(crate) center: Point3,
    pub(crate) e1: Vector3,
    pub(crate) e2: Vector3,
    pub(crate) axis: Vector3,
    pub(crate) radius: f64,
}

/// `P = c + (R + r cos v)(cos u·e1 + sin u·e2) + r sin v·a` with signed `r`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TorusChart {
    pub(crate) center: Point3,
    pub(crate) e1: Vector3,
    pub(crate) e2: Vector3,
    pub(crate) axis: Vector3,
    pub(crate) major: f64,
    pub(crate) minor: f64,
}

/// The axis of a cylinder or cone chart: `v` runs along `direction` from
/// `origin` with `h` millimetres per unit.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Axis {
    pub(crate) origin: Point3,
    pub(crate) direction: Vector3,
    pub(crate) h: f64,
}

/// A NURBS carrier: the stored surface in its own coordinates, mapped by
/// the composed placement.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NurbsChart<'ir> {
    pub(crate) surface: &'ir NurbsSurface,
    pub(crate) placement: Placement,
    /// The `u` and `v` knot domains.
    pub(crate) domain: [[f64; 2]; 2],
    pub(crate) periods: [Option<f64>; 2],
}

/// A chart in model space.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Chart<'ir> {
    Plane(PlaneChart),
    Cylinder(CylinderChart),
    Cone(ConeChart),
    Sphere(SphereChart),
    Torus(TorusChart),
    Nurbs(NurbsChart<'ir>),
}

fn no_value(message: &str) -> MeasureFailure {
    MeasureFailure::unavailable(
        UnavailableReason::EvaluationNoValue,
        None,
        message.to_owned(),
    )
}

impl<'ir> NurbsChart<'ir> {
    /// The chart of `surface` under `placement`, or `None` when a knot
    /// domain is empty.
    pub(crate) fn new(surface: &'ir NurbsSurface, placement: Placement) -> Option<Self> {
        let domain = |knots: &[f64], degree: u32, count: usize| -> Option<[f64; 2]> {
            let degree = usize::try_from(degree).ok()?;
            let low = *knots.get(degree)?;
            let high = *knots.get(count)?;
            (high > low).then_some([low, high])
        };
        let u = domain(
            surface.u_knots().as_slice(),
            surface.u_degree(),
            surface.u_count(),
        )?;
        let v = domain(
            surface.v_knots().as_slice(),
            surface.v_degree(),
            surface.v_count(),
        )?;
        Some(Self {
            surface,
            placement,
            domain: [u, v],
            periods: [
                surface.u_periodic().then_some(u[1] - u[0]),
                surface.v_periodic().then_some(v[1] - v[0]),
            ],
        })
    }

    fn partials(&self, eval: &Eval<'_, '_>, u: f64, v: f64) -> Measure<Partials> {
        let local = eval.work.evaluate(eval.ctx, |admission| {
            cadmpeg_ir::eval::nurbs_surface_partials(admission, self.surface, u, v)
        })?;
        Ok(Partials {
            point: self.placement.point(local.point.get()),
            du: self.placement.vector(local.du.get()),
            dv: self.placement.vector(local.dv.get()),
        })
    }

    /// The parameters of a model point within `tolerance`, seeded at
    /// `seed`, refined by Gauss–Newton steps until a step is below the
    /// parameter resolution.
    fn invert(
        &self,
        eval: &Eval<'_, '_>,
        point: Point3,
        seed: Option<[f64; 2]>,
        tolerance: f64,
    ) -> Measure<Option<[f64; 2]>> {
        let Some(local) = self.placement.inverse_point(point) else {
            return Ok(None);
        };
        let found = cadmpeg_ir::eval::nurbs_surface_parameter_within_tolerance_with_budget(
            eval.ctx,
            self.surface,
            local,
            seed.map(|[u, v]| Point2::new(u, v)),
            tolerance / self.placement.scale,
            eval.work.slice(),
        )?;
        let Some(found) = found else {
            return Ok(None);
        };
        let found = found.get();
        let mut uv = [found.u, found.v];
        let resolution = [0, 1].map(|axis| {
            INVERSION_PARAMETER_RESOLUTION * (self.domain[axis][1] - self.domain[axis][0])
        });
        for _ in 0..MAX_INVERSION_REFINEMENT_STEPS {
            let partials = self.partials(eval, uv[0], uv[1])?;
            let (Some(du), Some(dv)) = (
                FiniteVector3::new(partials.du),
                FiniteVector3::new(partials.dv),
            ) else {
                break;
            };
            let Some((step_u, step_v)) = cadmpeg_ir::math::solve::least_squares_step(
                du,
                dv,
                point.vector_from(partials.point),
            ) else {
                break;
            };
            let step = [step_u.get(), step_v.get()];
            for axis in 0..2 {
                uv[axis] += step[axis];
                if self.periods[axis].is_none() {
                    uv[axis] = uv[axis].clamp(self.domain[axis][0], self.domain[axis][1]);
                }
            }
            if step[0].abs() <= resolution[0] && step[1].abs() <= resolution[1] {
                break;
            }
        }
        Ok(Some(uv))
    }
}

impl NurbsChart<'_> {
    /// The box of the placed control points, which encloses the surface when
    /// every weight is positive.
    pub(crate) fn control_hull(&self) -> Result<Aabb, UnavailableReason> {
        if let Some(weights) = self.surface.pole_weights() {
            if weights.iter().any(|weight| weight.get() <= 0.0) {
                return Err(UnavailableReason::BboxNonpositiveWeights);
            }
        }
        let mut hull: Option<Aabb> = None;
        for pole in self.surface.poles() {
            let point = self.placement.point(pole.get());
            match hull.as_mut() {
                Some(hull) => hull.include(point),
                None => hull = Some(Aabb::of_point(point)),
            }
        }
        hull.ok_or(UnavailableReason::BboxInteriorUnbounded)
    }

    /// The knots of the `v` direction.
    pub(crate) fn v_breaks(&self) -> Vec<f64> {
        self.surface.v_knots().as_slice().to_vec()
    }

    /// The knots of the `u` direction.
    pub(crate) fn u_breaks(&self) -> Vec<f64> {
        self.surface.u_knots().as_slice().to_vec()
    }
}

impl Chart<'_> {
    /// The periods of `u` and `v`.
    pub(crate) fn periods(&self) -> [Option<f64>; 2] {
        let tau = Some(std::f64::consts::TAU);
        match self {
            Self::Plane(_) => [None, None],
            Self::Cylinder(_) | Self::Cone(_) | Self::Sphere(_) => [tau, None],
            Self::Torus(_) => [tau, tau],
            Self::Nurbs(nurbs) => nurbs.periods,
        }
    }

    /// The parameter domain of a NURBS chart.
    pub(crate) fn domain(&self) -> Option<[[f64; 2]; 2]> {
        match self {
            Self::Nurbs(nurbs) => Some(nurbs.domain),
            _ => None,
        }
    }

    /// The point and first partials at `(u, v)`.
    pub(crate) fn partials(&self, eval: &Eval<'_, '_>, u: f64, v: f64) -> Measure<Partials> {
        match self {
            Self::Nurbs(nurbs) => nurbs.partials(eval, u, v),
            analytic => analytic
                .closed_partials(u, v)
                .ok_or_else(|| no_value("the chart has no closed form")),
        }
    }

    /// The parameters of a model point: the closed form on an analytic
    /// chart, with periodic parameters in `(−π, π]` for the caller to lift;
    /// a refined inversion within `tolerance` near `seed` on a NURBS chart.
    pub(crate) fn invert(
        &self,
        eval: &Eval<'_, '_>,
        point: Point3,
        seed: Option<[f64; 2]>,
        tolerance: f64,
    ) -> Measure<Option<[f64; 2]>> {
        match self {
            Self::Nurbs(nurbs) => nurbs.invert(eval, point, seed, tolerance),
            analytic => Ok(analytic.closed_invert(point)),
        }
    }

    /// The point and first partials of an analytic chart at `(u, v)`.
    pub(crate) fn closed_partials(&self, u: f64, v: f64) -> Option<Partials> {
        Some(match self {
            Self::Plane(plane) => Partials {
                point: offset(
                    plane.origin,
                    combine(&[(plane.h * u, plane.eu), (plane.h * v, plane.ev)]),
                ),
                du: plane.eu.scale(plane.h),
                dv: plane.ev.scale(plane.h),
            },
            Self::Cylinder(cylinder) => {
                let (sine, cosine) = u.sin_cos();
                let r = cylinder.radius;
                Partials {
                    point: offset(
                        cylinder.origin,
                        combine(&[
                            (r * cosine, cylinder.e1),
                            (r * sine, cylinder.e2),
                            (cylinder.h * v, cylinder.axis),
                        ]),
                    ),
                    du: combine(&[(-r * sine, cylinder.e1), (r * cosine, cylinder.e2)]),
                    dv: cylinder.axis.scale(cylinder.h),
                }
            }
            Self::Cone(cone) => {
                let (sine, cosine) = u.sin_cos();
                let rho = cone.rho0 + cone.tau * v;
                let k = cone.ratio;
                Partials {
                    point: offset(
                        cone.origin,
                        combine(&[
                            (rho * cosine, cone.e1),
                            (rho * k * sine, cone.e2),
                            (cone.h * v, cone.axis),
                        ]),
                    ),
                    du: combine(&[(-rho * sine, cone.e1), (rho * k * cosine, cone.e2)]),
                    dv: combine(&[
                        (cone.tau * cosine, cone.e1),
                        (cone.tau * k * sine, cone.e2),
                        (cone.h, cone.axis),
                    ]),
                }
            }
            Self::Sphere(sphere) => {
                let (u_sine, u_cosine) = u.sin_cos();
                let (v_sine, v_cosine) = v.sin_cos();
                let r = sphere.radius;
                Partials {
                    point: offset(
                        sphere.center,
                        combine(&[
                            (r * v_cosine * u_cosine, sphere.e1),
                            (r * v_cosine * u_sine, sphere.e2),
                            (r * v_sine, sphere.axis),
                        ]),
                    ),
                    du: combine(&[
                        (-r * v_cosine * u_sine, sphere.e1),
                        (r * v_cosine * u_cosine, sphere.e2),
                    ]),
                    dv: combine(&[
                        (-r * v_sine * u_cosine, sphere.e1),
                        (-r * v_sine * u_sine, sphere.e2),
                        (r * v_cosine, sphere.axis),
                    ]),
                }
            }
            Self::Torus(torus) => {
                let (u_sine, u_cosine) = u.sin_cos();
                let (v_sine, v_cosine) = v.sin_cos();
                let ring = torus.major + torus.minor * v_cosine;
                Partials {
                    point: offset(
                        torus.center,
                        combine(&[
                            (ring * u_cosine, torus.e1),
                            (ring * u_sine, torus.e2),
                            (torus.minor * v_sine, torus.axis),
                        ]),
                    ),
                    du: combine(&[(-ring * u_sine, torus.e1), (ring * u_cosine, torus.e2)]),
                    dv: combine(&[
                        (-torus.minor * v_sine * u_cosine, torus.e1),
                        (-torus.minor * v_sine * u_sine, torus.e2),
                        (torus.minor * v_cosine, torus.axis),
                    ]),
                }
            }
            Self::Nurbs(_) => return None,
        })
    }

    /// The raw parameters of a model point on an analytic chart. Periodic
    /// parameters are in `(−π, π]`; the caller lifts them.
    fn closed_invert(&self, point: Point3) -> Option<[f64; 2]> {
        Some(match self {
            Self::Plane(plane) => {
                let d = point.vector_from(plane.origin);
                [d.dot(plane.eu) / plane.h, d.dot(plane.ev) / plane.h]
            }
            Self::Cylinder(cylinder) => {
                let d = point.vector_from(cylinder.origin);
                [
                    d.dot(cylinder.e2).atan2(d.dot(cylinder.e1)),
                    d.dot(cylinder.axis) / cylinder.h,
                ]
            }
            Self::Cone(cone) => {
                let offset = point.vector_from(cone.origin);
                let axial = offset.dot(cone.axis) / cone.h;
                let rho = cone.rho0 + cone.tau * axial;
                let along = offset.dot(cone.e1);
                let across = offset.dot(cone.e2) / cone.ratio;
                let angle = if rho >= 0.0 {
                    across.atan2(along)
                } else {
                    (-across).atan2(-along)
                };
                [angle, axial]
            }
            Self::Sphere(sphere) => {
                let d = point.vector_from(sphere.center);
                let x = d.dot(sphere.e1) / sphere.radius;
                let y = d.dot(sphere.e2) / sphere.radius;
                let z = d.dot(sphere.axis) / sphere.radius;
                [y.atan2(x), z.atan2(x.hypot(y))]
            }
            Self::Torus(torus) => {
                let d = point.vector_from(torus.center);
                let x = d.dot(torus.e1);
                let y = d.dot(torus.e2);
                let u = y.atan2(x);
                let (sine, cosine) = u.sin_cos();
                let radial = x * cosine + y * sine - torus.major;
                let axial = d.dot(torus.axis);
                [u, (axial / torus.minor).atan2(radial / torus.minor)]
            }
            Self::Nurbs(_) => return None,
        })
    }

    /// An antiderivative `G(v)` of `−|N|`, so that `F(u, v) = G(v) − G(v₀)`
    /// satisfies `∂F/∂v = −|N|`. The cone's is zero at the apex. `None` for an
    /// elliptic cone.
    pub(crate) fn area_antiderivative(&self, v: f64) -> Option<f64> {
        Some(match self {
            Self::Plane(plane) => -plane.h * plane.h * v,
            Self::Cylinder(cylinder) => -cylinder.radius * cylinder.h * v,
            Self::Cone(cone) => {
                if !cone_is_circular(cone) {
                    return None;
                }
                if cone.tau == 0.0 {
                    -cone.rho0.abs() * cone.h * v
                } else {
                    let rho = cone.rho0 + cone.tau * v;
                    -cone.tau.hypot(cone.h) * rho.signum() * rho * rho / (2.0 * cone.tau)
                }
            }
            Self::Sphere(sphere) => -sphere.radius * sphere.radius * v.sin(),
            Self::Torus(torus) => -torus.minor.abs() * (torus.major * v + torus.minor * v.sin()),
            Self::Nurbs(_) => return None,
        })
    }
}

impl Chart<'_> {
    /// The cone apex: its `v` and its model point. `None` on other charts
    /// and on a cone of zero half-angle.
    pub(crate) fn apex(&self) -> Option<(f64, Point3)> {
        match self {
            Self::Cone(cone) if cone.tau != 0.0 => {
                let v = -cone.rho0 / cone.tau;
                Some((v, offset(cone.origin, cone.axis.scale(cone.h * v))))
            }
            _ => None,
        }
    }

    /// Whether `point` lies within `tolerance` of a point where `u` is not
    /// defined.
    pub(crate) fn singular_at(&self, point: Point3, tolerance: f64) -> bool {
        self.apex()
            .is_some_and(|(_, apex)| apex.distance(point) <= tolerance)
    }

    /// The axis of a cylinder or cone.
    pub(crate) fn axis(&self) -> Option<Axis> {
        match self {
            Self::Cylinder(cylinder) => Some(Axis {
                origin: cylinder.origin,
                direction: cylinder.axis,
                h: cylinder.h,
            }),
            Self::Cone(cone) => Some(Axis {
                origin: cone.origin,
                direction: cone.axis,
                h: cone.h,
            }),
            Self::Plane(_) | Self::Sphere(_) | Self::Torus(_) | Self::Nurbs(_) => None,
        }
    }

    /// The cross-section radius at `v` of a cylinder or circular cone.
    pub(crate) fn section_radius(&self, v: f64) -> Option<f64> {
        match self {
            Self::Cylinder(cylinder) => Some(cylinder.radius.abs()),
            Self::Cone(cone) if cone_is_circular(cone) => Some((cone.rho0 + cone.tau * v).abs()),
            _ => None,
        }
    }

    /// Whether the chart is a cone with an elliptic cross-section.
    pub(crate) fn elliptic(&self) -> bool {
        matches!(self, Self::Cone(cone) if !cone_is_circular(cone))
    }
}

fn cone_is_circular(cone: &ConeChart) -> bool {
    cone.ratio == 1.0
}

#[cfg(test)]
mod tests {
    use super::{Chart, ConeChart, CylinderChart, PlaneChart, SphereChart, TorusChart};
    use cadmpeg_ir::math::{Point3, Vector3};

    fn x() -> Vector3 {
        Vector3::new(1.0, 0.0, 0.0)
    }
    fn y() -> Vector3 {
        Vector3::new(0.0, 1.0, 0.0)
    }
    fn z() -> Vector3 {
        Vector3::new(0.0, 0.0, 1.0)
    }

    fn charts() -> [Chart<'static>; 5] {
        let origin = Point3::new(1.0, -2.0, 3.0);
        [
            Chart::Plane(PlaneChart {
                origin,
                eu: x(),
                ev: y(),
                h: 2.0,
            }),
            Chart::Cylinder(CylinderChart {
                origin,
                e1: x(),
                e2: y(),
                axis: z(),
                radius: 5.0,
                h: 1.0,
            }),
            Chart::Cone(ConeChart {
                origin,
                e1: x(),
                e2: y(),
                axis: z(),
                rho0: 3.0,
                tau: 0.5,
                ratio: 1.0,
                h: 1.0,
            }),
            Chart::Sphere(SphereChart {
                center: origin,
                e1: x(),
                e2: y(),
                axis: z(),
                radius: -4.0,
            }),
            Chart::Torus(TorusChart {
                center: origin,
                e1: x(),
                e2: y(),
                axis: z(),
                major: 10.0,
                minor: -2.0,
            }),
        ]
    }

    #[test]
    fn inversion_recovers_the_chart_point() {
        for chart in charts() {
            for (u, v) in [(0.3, 0.2), (-2.0, 0.9), (2.9, -0.4)] {
                let point = chart.closed_partials(u, v).unwrap().point;
                let [iu, iv] = chart.closed_invert(point).unwrap();
                let again = chart.closed_partials(iu, iv).unwrap().point;
                assert!(again.distance(point) < 1.0e-12, "{chart:?} {u} {v}");
            }
        }
    }

    #[test]
    fn the_area_antiderivative_differentiates_to_the_area_density() {
        for chart in charts() {
            for v in [-0.7, 0.1, 0.8] {
                let step = 1.0e-5;
                let slope = (chart.area_antiderivative(v + step).unwrap()
                    - chart.area_antiderivative(v - step).unwrap())
                    / (2.0 * step);
                let partials = chart.closed_partials(0.4, v).unwrap();
                let density = partials.du.cross(partials.dv).norm();
                assert!(
                    (slope + density).abs() < 1.0e-6 * density.max(1.0),
                    "{chart:?}"
                );
            }
        }
    }
}
