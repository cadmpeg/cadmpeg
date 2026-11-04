// SPDX-License-Identifier: Apache-2.0
//! Bounded intersections of perpendicular cylinders.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::geometry::{
    analytic::CylinderSurface,
    nurbs::NurbsCurve,
    pcurve::{PcurveGeometry, PolarNurbsPole, PolarPcurveNurbs},
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3};

use crate::curve::TwoChartPcurveSamples;
use crate::decode::analytic::vertices::{finite_model_point, model_points_agree};
use crate::vecmath::{add, cross, dot, normalize, scale};

const EPS_AXIS: f64 = 1.0e-9;
const EPS_POSITION: f64 = 1.0e-9;
const EPS_PARAMETER: f64 = 1.0e-10;

type Homogeneous = [f64; 4];

fn subtract(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

fn split(points: [Homogeneous; 5], t: f64) -> ([Homogeneous; 5], [Homogeneous; 5]) {
    let mut level = points;
    let mut left = [[0.0; 4]; 5];
    let mut right = [[0.0; 4]; 5];
    left[0] = level[0];
    right[4] = level[4];
    for depth in 1..5 {
        for index in 0..5 - depth {
            level[index] = std::array::from_fn(|axis| {
                (1.0 - t).mul_add(level[index][axis], t * level[index + 1][axis])
            });
        }
        left[depth] = level[0];
        right[4 - depth] = level[4 - depth];
    }
    (left, right)
}

fn bernstein(power: [f64; 5]) -> [f64; 5] {
    let [constant, linear, quadratic, cubic, quartic] = power;
    [
        constant,
        constant + linear / 4.0,
        constant + linear / 2.0 + quadratic / 6.0,
        constant + 3.0 * linear / 4.0 + quadratic / 2.0 + cubic / 4.0,
        constant + linear + quadratic + cubic + quartic,
    ]
}

fn agrees(first: [f64; 3], second: [f64; 3]) -> bool {
    finite_model_point(first)
        .zip(finite_model_point(second))
        .is_some_and(|(a, b)| model_points_agree(a, b))
}

fn branch_sign(values: impl Iterator<Item = f64>, tolerance: f64) -> Option<f64> {
    let mut sign = None;
    for value in values {
        if value.abs() <= tolerance {
            continue;
        }
        let candidate = value.signum();
        if sign.is_some_and(|previous| previous != candidate) {
            return None;
        }
        sign = Some(candidate);
    }
    sign
}

fn bounded_curve(
    first: &CylinderSurface,
    second: &CylinderSurface,
    samples: &[[f64; 3]],
    endpoints: [[f64; 3]; 2],
) -> Option<NurbsCurve> {
    if samples.len() < 3 {
        return None;
    }
    let (small, large) = if first.radius().get() < second.radius().get() {
        (first, second)
    } else {
        (second, first)
    };
    let r = small.radius().get();
    let big_r = large.radius().get();
    let difference = big_r - r;
    (difference > 0.0).then_some(())?;
    let axis: [f64; 3] = (*small.frame().axis().as_raw()).into();
    let transverse: [f64; 3] = (*large.frame().axis().as_raw()).into();
    (dot(axis, transverse).abs() <= EPS_AXIS).then_some(())?;
    let small_origin: [f64; 3] = small.origin().get().into();
    let big_origin: [f64; 3] = large.origin().get().into();
    let displacement = subtract(small_origin, big_origin);
    let mut normal = normalize(cross(axis, transverse))?;
    let signed_offset = dot(displacement, normal);
    if signed_offset < 0.0 {
        normal = scale(normal, -1.0);
    }
    let offset = signed_offset.abs();
    let tolerance = EPS_POSITION * big_r.max(offset).max(1.0);
    ((offset - difference).abs() <= tolerance).then_some(())?;
    let base = add(big_origin, scale(transverse, dot(displacement, transverse)));
    let axial_sign = branch_sign(
        samples.iter().map(|p| dot(subtract(*p, base), axis)),
        tolerance,
    )?;
    let transverse_sign = branch_sign(
        samples.iter().map(|p| dot(subtract(*p, base), transverse)),
        tolerance,
    )?;
    let axis = scale(axis, axial_sign);
    let transverse = scale(transverse, transverse_sign);
    let parameter = |point: [f64; 3]| {
        let local = subtract(point, base);
        let cosine = (dot(local, normal) - offset) / r;
        let sine = dot(local, transverse) / r;
        ((-1.0 - EPS_AXIS..=1.0 + EPS_AXIS).contains(&cosine) && sine >= -EPS_AXIS).then_some(())?;
        let denominator = 1.0 + cosine;
        (denominator > EPS_PARAMETER).then_some(())?;
        let t = sine.max(0.0) / denominator;
        let q = difference.sqrt() * t / ((big_r + difference * t * t).sqrt() + big_r.sqrt());
        (q.is_finite() && (0.0..1.0).contains(&q)).then_some(q)
    };
    let first_q = parameter(*samples.first()?)?;
    let last_q = parameter(*samples.last()?)?;
    ((last_q - first_q).abs() > EPS_PARAMETER).then_some(())?;
    let increasing = last_q > first_q;
    let mut previous = first_q;
    for point in samples.iter().skip(1) {
        let next = parameter(*point)?;
        if (increasing && next <= previous + EPS_PARAMETER)
            || (!increasing && next >= previous - EPS_PARAMETER)
        {
            return None;
        }
        previous = next;
    }
    let sample_ends = [*samples.first()?, *samples.last()?];
    let reverse = match [
        agrees(sample_ends[0], endpoints[0]) && agrees(sample_ends[1], endpoints[1]),
        agrees(sample_ends[0], endpoints[1]) && agrees(sample_ends[1], endpoints[0]),
    ] {
        [true, false] => !increasing,
        [false, true] => increasing,
        _ => return None,
    };
    let q0 = first_q.min(last_q);
    let q1 = first_q.max(last_q);
    let k = (big_r / difference).sqrt();
    for point in samples {
        let q = parameter(*point)?;
        let q2 = q * q;
        let denominator = 1.0 + (4.0 * k * k - 2.0) * q2 + q2 * q2;
        let cosine = (1.0 - (2.0 + 4.0 * k * k) * q2 + q2 * q2) / denominator;
        let sine = 4.0 * k * q * (1.0 - q2) / denominator;
        let along = 4.0 * big_r * (r / difference).sqrt() * q * (1.0 + q2) / denominator;
        let expected = add(
            add(base, scale(normal, offset + r * cosine)),
            add(scale(transverse, r * sine), scale(axis, along)),
        );
        agrees(*point, expected).then_some(())?;
    }
    let denominator = [1.0, 0.0, 4.0 * k * k - 2.0, 0.0, 1.0];
    let cosine = [1.0, 0.0, -2.0 - 4.0 * k * k, 0.0, 1.0];
    let sine = [0.0, 4.0 * k, 0.0, -4.0 * k, 0.0];
    let axial = [
        0.0,
        4.0 * big_r * (r / difference).sqrt(),
        0.0,
        4.0 * big_r * (r / difference).sqrt(),
        0.0,
    ];
    let weight = bernstein(denominator);
    let coordinates: [[f64; 5]; 3] = std::array::from_fn(|component| {
        bernstein(std::array::from_fn(|i| {
            base[component] * denominator[i]
                + normal[component] * (offset * denominator[i] + r * cosine[i])
                + transverse[component] * r * sine[i]
                + axis[component] * axial[i]
        }))
    });
    let controls: [Homogeneous; 5] = std::array::from_fn(|i| {
        [
            coordinates[0][i],
            coordinates[1][i],
            coordinates[2][i],
            weight[i],
        ]
    });
    let (_, right) = split(controls, q0);
    let (mut interval, _) = split(right, (q1 - q0) / (1.0 - q0));
    if reverse {
        interval.reverse();
    }
    let mut poles = Vec::new();
    let mut weights = Vec::new();
    for [x, y, z, w] in interval {
        (w.is_finite() && w > 0.0).then_some(())?;
        poles.push(Point3::new(x / w, y / w, z / w));
        weights.push(w);
    }
    NurbsCurve::from_lanes(
        4,
        vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        poles,
        Some(weights),
        false,
    )
    .ok()
}

fn equal_cylinder_curve(
    first: &CylinderSurface,
    second: &CylinderSurface,
    samples: &[[f64; 3]],
    endpoints: [[f64; 3]; 2],
) -> Option<NurbsCurve> {
    (samples.len() >= 3).then_some(())?;
    let radius = first.radius().get();
    let tolerance = EPS_POSITION * radius.max(1.0);
    ((radius - second.radius().get()).abs() <= tolerance).then_some(())?;
    let first_axis: [f64; 3] = (*first.frame().axis().as_raw()).into();
    let second_axis: [f64; 3] = (*second.frame().axis().as_raw()).into();
    (dot(first_axis, second_axis).abs() <= EPS_AXIS).then_some(())?;
    let normal = normalize(cross(first_axis, second_axis))?;
    let first_origin: [f64; 3] = first.origin().get().into();
    let second_origin: [f64; 3] = second.origin().get().into();
    let displacement = subtract(second_origin, first_origin);
    (dot(displacement, normal).abs() <= tolerance).then_some(())?;
    let center = add(
        first_origin,
        scale(first_axis, dot(displacement, first_axis)),
    );
    let local = |point: [f64; 3]| {
        let displacement = subtract(point, center);
        [
            dot(displacement, first_axis),
            dot(displacement, second_axis),
            dot(displacement, normal),
        ]
    };
    let branch = branch_sign(
        samples.iter().map(|point| {
            let [first, second, _] = local(*point);
            first * second
        }),
        tolerance * tolerance,
    )?;
    let mut angles = samples.iter().map(|point| {
        let [first, second, normal] = local(*point);
        ((second - branch * first).abs() <= tolerance
            && (first.hypot(normal) - radius).abs() <= tolerance)
            .then_some(normal.atan2(first))
    });
    let start = angles.next()??;
    let mut previous = start;
    let mut sweep = 0.0;
    let mut direction = None;
    for angle in angles {
        let angle = angle?;
        let step = (angle - previous + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        (step.abs() > EPS_PARAMETER).then_some(())?;
        if direction.is_some_and(|sign| sign != step.signum()) {
            return None;
        }
        direction = Some(step.signum());
        sweep += step;
        previous = angle;
    }
    (sweep.abs() < std::f64::consts::PI - EPS_PARAMETER).then_some(())?;
    let sample_endpoints = [*samples.first()?, *samples.last()?];
    let (start, sweep) = match [
        agrees(sample_endpoints[0], endpoints[0]) && agrees(sample_endpoints[1], endpoints[1]),
        agrees(sample_endpoints[0], endpoints[1]) && agrees(sample_endpoints[1], endpoints[0]),
    ] {
        [true, false] => (start, sweep),
        [false, true] => (start + sweep, -sweep),
        _ => return None,
    };
    let radial = add(first_axis, scale(second_axis, branch));
    let point = |angle: f64, weight: f64| {
        add(
            center,
            scale(
                add(scale(radial, angle.cos()), scale(normal, angle.sin())),
                radius / weight,
            ),
        )
    };
    let weight = (sweep / 2.0).cos();
    (weight > 0.0).then_some(())?;
    let poles = [
        point(start, 1.0),
        point(start + sweep / 2.0, weight),
        point(start + sweep, 1.0),
    ]
    .map(|[x, y, z]| Point3::new(x, y, z));
    NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        poles.to_vec(),
        Some(vec![1.0, weight, 1.0]),
        false,
    )
    .ok()
}

pub(super) fn projected_pcurve(
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
) -> Option<PcurveGeometry> {
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) = surface else {
        return None;
    };
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = geometry else {
        return None;
    };
    let origin: [f64; 3] = cylinder.origin().get().into();
    let axis: [f64; 3] = (*cylinder.frame().axis().as_raw()).into();
    let reference: [f64; 3] = (*cylinder.frame().reference().as_raw()).into();
    let transverse = cross(axis, reference);
    let poles: Vec<_> = curve
        .control_points()
        .into_iter()
        .map(|point| {
            let relative = subtract(point.get().into(), origin);
            PolarNurbsPole {
                radial: Point2::new(dot(relative, reference), dot(relative, transverse)),
                axial: dot(relative, axis),
            }
        })
        .collect();
    let above = poles.iter().all(|pole| pole.radial.v >= 0.0);
    let below = poles.iter().all(|pole| pole.radial.v <= 0.0);
    (above || below).then_some(())?;
    (!poles
        .iter()
        .any(|pole| pole.radial.v == 0.0 && pole.radial.u < 0.0))
    .then_some(())?;
    let weights = curve
        .weights()?
        .into_iter()
        .map(cadmpeg_ir::scalar::NonZeroReal::get)
        .collect();
    let nurbs = PolarPcurveNurbs::from_lanes(
        curve.degree(),
        curve.knots().as_slice().to_vec(),
        poles,
        Some(weights),
        curve.periodic(),
    )
    .ok()?;
    Some(PcurveGeometry::PolarNurbs { nurbs })
}

pub(super) fn carrier(
    ctx: &DecodeContext<'_>,
    record: &TwoChartPcurveSamples,
    surfaces: [&SurfaceGeometry; 2],
    endpoints: [[f64; 3]; 2],
) -> Result<Option<CurveGeometry>, CodecError> {
    let [SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(first)), SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(second))] =
        surfaces
    else {
        return Ok(None);
    };
    let mut mapped = ctx.alloc_filled(
        record.samples.len(),
        [0.0; 3],
        "Creo paired cylinder samples",
    )?;
    for (sample, point) in record.samples.iter().zip(&mut mapped) {
        let mut values = [[0.0; 3]; 2];
        for index in 0..2 {
            let Some(value) = cadmpeg_ir::eval::finite_or_refusal(
                cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::surface_point(
                    ctx,
                    surfaces[index],
                    sample[index][0],
                    sample[index][1],
                ))?,
            )?
            else {
                return Ok(None);
            };
            values[index] = value.get().into();
        }
        if !agrees(values[0], values[1]) {
            return Ok(None);
        }
        *point = values[0];
    }
    let Some(curve) = bounded_curve(first, second, &mapped, endpoints)
        .or_else(|| equal_cylinder_curve(first, second, &mapped, endpoints))
    else {
        return Ok(None);
    };
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve));
    if surfaces
        .iter()
        .any(|surface| projected_pcurve(surface, &geometry).is_none())
    {
        return Ok(None);
    }
    Ok(Some(geometry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::math::Vector3;

    #[test]
    fn equal_cylinder_intersection_follows_the_sampled_ellipse() {
        let first = CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            1.0,
        )
        .expect("first cylinder");
        let second = CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            1.0,
        )
        .expect("second cylinder");
        for branch in [-1.0, 1.0] {
            let point = |angle: f64| [angle.cos(), branch * angle.cos(), angle.sin()];
            let samples = [
                point(0.0),
                point(std::f64::consts::FRAC_PI_4),
                point(std::f64::consts::FRAC_PI_2),
            ];
            for reverse in [false, true] {
                let ends = if reverse {
                    [samples[2], samples[0]]
                } else {
                    [samples[0], samples[2]]
                };
                let curve = equal_cylinder_curve(&first, &second, &samples, ends).expect("ellipse");
                let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve));
                let surfaces = [first, second].map(|cylinder| {
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder))
                });
                let pcurves = surfaces
                    .each_ref()
                    .map(|surface| projected_pcurve(surface, &geometry).expect("projection"));
                for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    let point = crate::decode::with_service_context(|ctx| {
                        cadmpeg_ir::eval::finite_or_refusal(
                            cadmpeg_ir::eval::decode::outer_refusal(
                                cadmpeg_ir::eval::decode::curve_point(ctx, &geometry, parameter),
                            )?,
                        )
                    })
                    .expect("evaluate curve")
                    .expect("finite point")
                    .get();
                    assert!((point.x * point.x + point.z * point.z - 1.0).abs() < 1.0e-10);
                    assert!((point.y - branch * point.x).abs() < 1.0e-10);
                    for (surface, pcurve) in surfaces.iter().zip(&pcurves) {
                        let mapped = crate::decode::with_service_context(|ctx| {
                            let uv = cadmpeg_ir::eval::finite_or_refusal(
                                cadmpeg_ir::eval::decode::outer_refusal(
                                    cadmpeg_ir::eval::decode::pcurve_uv(ctx, pcurve, parameter),
                                )?,
                            )?
                            .expect("parameter")
                            .get();
                            cadmpeg_ir::eval::finite_or_refusal(
                                cadmpeg_ir::eval::decode::outer_refusal(
                                    cadmpeg_ir::eval::decode::surface_point(
                                        ctx, surface, uv.u, uv.v,
                                    ),
                                )?,
                            )
                        })
                        .expect("evaluate projection")
                        .expect("finite point")
                        .get();
                        assert!(agrees(point.into(), mapped.into()));
                    }
                    if parameter == 0.0 {
                        assert!(agrees(point.into(), ends[0]));
                    }
                    if parameter == 1.0 {
                        assert!(agrees(point.into(), ends[1]));
                    }
                }
            }
            let mut inconsistent = samples;
            inconsistent[1][1] *= -1.0;
            assert!(
                equal_cylinder_curve(&first, &second, &inconsistent, [samples[0], samples[2]])
                    .is_none()
            );
            let nonmonotone = [samples[0], samples[2], samples[1], samples[2]];
            assert!(
                equal_cylinder_curve(&first, &second, &nonmonotone, [samples[0], samples[2]])
                    .is_none()
            );
            assert!(
                equal_cylinder_curve(&first, &second, &samples, [samples[0], samples[1]]).is_none()
            );
        }
    }

    #[test]
    fn equal_cylinder_intersection_requires_equal_radii_and_crossing_axes() {
        let cylinder = |origin, axis, radius| {
            CylinderSurface::try_new(origin, axis, Vector3::new(0.0, 0.0, 1.0), radius)
                .expect("cylinder")
        };
        let first = cylinder(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0), 1.0);
        let second_axis = Vector3::new(0.0, 1.0, 0.0);
        let diagonal = std::f64::consts::FRAC_1_SQRT_2;
        let samples = [
            [1.0, 1.0, 0.0],
            [diagonal, diagonal, diagonal],
            [0.0, 0.0, 1.0],
        ];
        let ends = [samples[0], samples[2]];
        let wrong_radius = cylinder(Point3::new(0.0, 0.0, 0.0), second_axis, 2.0);
        let skew_axes = cylinder(Point3::new(0.0, 0.0, 0.25), second_axis, 1.0);
        let parallel = cylinder(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0), 1.0);
        assert!(equal_cylinder_curve(&first, &wrong_radius, &samples, ends).is_none());
        assert!(equal_cylinder_curve(&first, &skew_axes, &samples, ends).is_none());
        assert!(equal_cylinder_curve(&first, &parallel, &samples, ends).is_none());
    }

    #[test]
    fn tangent_cylinder_branch_is_exact_and_follows_endpoints() {
        let small = CylinderSurface::try_new(
            Point3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("small cylinder");
        let large = CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("large cylinder");
        for axial_sign in [-1.0, 1.0] {
            for transverse_sign in [-1.0, 1.0] {
                let point = |angle: f64| {
                    let x = 1.0 + angle.cos();
                    [
                        x,
                        axial_sign * (4.0 - x * x).sqrt(),
                        transverse_sign * angle.sin(),
                    ]
                };
                let samples = [
                    point(0.0),
                    point(std::f64::consts::FRAC_PI_4),
                    point(std::f64::consts::FRAC_PI_2),
                ];
                for reverse in [false, true] {
                    let ends = if reverse {
                        [samples[2], samples[0]]
                    } else {
                        [samples[0], samples[2]]
                    };
                    let curve =
                        bounded_curve(&small, &large, &samples, ends).expect("bounded curve");
                    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve));
                    let surfaces = [small, large].map(|cylinder| {
                        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder))
                    });
                    let projections = surfaces.each_ref().map(|surface| {
                        projected_pcurve(surface, &geometry).expect("polar projection")
                    });
                    for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
                        let evaluated = crate::decode::with_service_context(|ctx| {
                            cadmpeg_ir::eval::finite_or_refusal(
                                cadmpeg_ir::eval::decode::outer_refusal(
                                    cadmpeg_ir::eval::decode::curve_point(ctx, &geometry, t),
                                )?,
                            )
                        })
                        .expect("evaluate")
                        .expect("point")
                        .get();
                        assert!(
                            ((evaluated.x - 1.0).powi(2) + evaluated.z.powi(2) - 1.0).abs()
                                < 1.0e-10
                        );
                        assert!((evaluated.x.powi(2) + evaluated.y.powi(2) - 4.0).abs() < 1.0e-10);
                        for (surface, pcurve) in surfaces.iter().zip(&projections) {
                            let projected = crate::decode::with_service_context(|ctx| {
                                let uv = cadmpeg_ir::eval::finite_or_refusal(
                                    cadmpeg_ir::eval::decode::outer_refusal(
                                        cadmpeg_ir::eval::decode::pcurve_uv(ctx, pcurve, t),
                                    )?,
                                )?
                                .expect("finite parameter")
                                .get();
                                cadmpeg_ir::eval::finite_or_refusal(
                                    cadmpeg_ir::eval::decode::outer_refusal(
                                        cadmpeg_ir::eval::decode::surface_point(
                                            ctx, surface, uv.u, uv.v,
                                        ),
                                    )?,
                                )
                            })
                            .expect("evaluate projection")
                            .expect("point")
                            .get();
                            assert!(agrees(evaluated.into(), projected.into()));
                        }
                        if t == 0.0 {
                            assert!(agrees(evaluated.into(), ends[0]));
                        }
                        if t == 1.0 {
                            assert!(agrees(evaluated.into(), ends[1]));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn rejects_cylinder_pairs_and_samples_without_the_required_branch() {
        let cylinder = |origin, axis, radius| {
            CylinderSurface::try_new(origin, axis, Vector3::new(1.0, 0.0, 0.0), radius)
                .expect("cylinder")
        };
        let small = cylinder(Point3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0), 1.0);
        let large = cylinder(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), 2.0);
        let point = |angle: f64| {
            let x = 1.0 + angle.cos();
            [x, (4.0 - x * x).sqrt(), angle.sin()]
        };
        let samples = [
            point(0.0),
            point(std::f64::consts::FRAC_PI_4),
            point(std::f64::consts::FRAC_PI_2),
        ];
        let ends = [samples[0], samples[2]];
        let parallel = cylinder(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0), 2.0);
        let separated = cylinder(
            Point3::new(-0.5, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            2.0,
        );
        assert!(bounded_curve(&small, &parallel, &samples, ends).is_none());
        assert!(bounded_curve(&small, &separated, &samples, ends).is_none());
        let mut wrong_branch = samples;
        wrong_branch[1][1] *= -1.0;
        assert!(bounded_curve(&small, &large, &wrong_branch, ends).is_none());
        let nonmonotone = [samples[0], samples[2], samples[1], samples[2]];
        assert!(bounded_curve(&small, &large, &nonmonotone, ends).is_none());
        assert!(bounded_curve(&small, &large, &samples, [samples[0], samples[1]]).is_none());
    }

    #[test]
    fn cylinder_projection_scales_axial_coordinates_with_the_model() {
        for scale in [1.0, 25.4] {
            let small = CylinderSurface::try_new(
                Point3::new(scale, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                scale,
            )
            .expect("small cylinder");
            let large = CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0 * scale,
            )
            .expect("large cylinder");
            let point = |angle: f64| {
                let x = 1.0 + angle.cos();
                [scale * x, scale * (4.0 - x * x).sqrt(), scale * angle.sin()]
            };
            let samples = [
                point(0.0),
                point(std::f64::consts::FRAC_PI_4),
                point(std::f64::consts::FRAC_PI_2),
            ];
            let curve = bounded_curve(&small, &large, &samples, [samples[0], samples[2]])
                .expect("bounded curve");
            let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve));
            for cylinder in [small, large] {
                let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder));
                let pcurve = projected_pcurve(&surface, &geometry).expect("polar projection");
                for parameter in [0.0, 0.5, 1.0] {
                    let (point, projected) = crate::decode::with_service_context(|ctx| {
                        let point = cadmpeg_ir::eval::finite_or_refusal(
                            cadmpeg_ir::eval::decode::outer_refusal(
                                cadmpeg_ir::eval::decode::curve_point(ctx, &geometry, parameter),
                            )?,
                        )?
                        .expect("curve point");
                        let uv = cadmpeg_ir::eval::finite_or_refusal(
                            cadmpeg_ir::eval::decode::outer_refusal(
                                cadmpeg_ir::eval::decode::pcurve_uv(ctx, &pcurve, parameter),
                            )?,
                        )?
                        .expect("surface parameter")
                        .get();
                        let projected = cadmpeg_ir::eval::finite_or_refusal(
                            cadmpeg_ir::eval::decode::outer_refusal(
                                cadmpeg_ir::eval::decode::surface_point(ctx, &surface, uv.u, uv.v),
                            )?,
                        )?
                        .expect("surface point");
                        Ok::<_, cadmpeg_core::decode::ResourceLimit>((point, projected))
                    })
                    .expect("evaluate curve and pcurve");
                    assert!(agrees(point.get().into(), projected.get().into()));
                }
            }
        }
    }
}
