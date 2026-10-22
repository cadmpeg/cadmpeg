// SPDX-License-Identifier: Apache-2.0
//! Model-space charts of analytic surface carriers.
//!
//! Each chart reproduces the evaluator's parameterization of its carrier with
//! the frame mapped by the composed placement's rotation `Q = L/s`. Frames
//! are explicit triples; handedness is not assumed, so a reflecting placement
//! keeps the chart and reverses the carrier normal `S_u × S_v`.

use cadmpeg_ir::math::{Point3, Vector3};

use crate::geom::{combine, offset};

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

/// An analytic chart in model space.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Chart {
    Plane(PlaneChart),
    Cylinder(CylinderChart),
    Cone(ConeChart),
    Sphere(SphereChart),
    Torus(TorusChart),
}

impl Chart {
    /// The periods of `u` and `v`.
    pub(crate) fn periods(&self) -> [Option<f64>; 2] {
        let tau = Some(std::f64::consts::TAU);
        match self {
            Self::Plane(_) => [None, None],
            Self::Cylinder(_) | Self::Cone(_) | Self::Sphere(_) => [tau, None],
            Self::Torus(_) => [tau, tau],
        }
    }

    /// The point and first partials at `(u, v)`.
    pub(crate) fn partials(&self, u: f64, v: f64) -> Partials {
        match self {
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
        }
    }

    /// The raw parameters of a model point. Periodic parameters are in
    /// `(−π, π]`; the caller lifts them.
    pub(crate) fn invert(&self, point: Point3) -> [f64; 2] {
        match self {
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
        }
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
        })
    }
}

impl Chart {
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
            Self::Plane(_) | Self::Sphere(_) | Self::Torus(_) => None,
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

    fn charts() -> [Chart; 5] {
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
                let point = chart.partials(u, v).point;
                let [iu, iv] = chart.invert(point);
                let again = chart.partials(iu, iv).point;
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
                let partials = chart.partials(0.4, v);
                let density = partials.du.cross(partials.dv).norm();
                assert!(
                    (slope + density).abs() < 1.0e-6 * density.max(1.0),
                    "{chart:?}"
                );
            }
        }
    }
}
