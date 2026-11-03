// SPDX-License-Identifier: Apache-2.0
//! Polynomial and rational B-spline approximations of native helix paths and
//! helix surfaces.

use crate::math::portable::Portable;
use cadmpeg_core::convert::f64_from_index;
use cadmpeg_core::decode::ResourceLimit;

use super::{finite_or_refusal, helix_differential, ModelCurveDifferential};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::nurbs::{
    NurbsCurve, NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis, WeightedPole3,
};
use crate::geometry::{
    HelixCurveConstruction, HelixFrame, HelixSurfaceConstruction, HelixSurfaceProfile,
};
use crate::math::{Point3, Vector3};
use crate::scalar::{NonZeroReal, PositiveLength};

/// Largest angle one cubic segment spans before its deviation is measured.
const MAX_SEGMENT_ANGLE: f64 = std::f64::consts::FRAC_PI_2;

/// Number of times the segment count doubles before the fit is refused.
const MAX_REFINEMENTS: u32 = 16;

/// Segment fractions at which a cubic segment is compared with the helix.
const DEVIATION_SAMPLES: [f64; 7] = [0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875];

/// Quarter-circle arcs that make up the swept circle of a circular profile.
const PROFILE_CIRCLE_ARCS: usize = 4;

/// Why a helix has no fitted B-spline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelixNurbsError {
    /// The helix has no finite point or tangent at a segment end or sample.
    NotFinite,
    /// No segment count within the refinement bound meets the tolerance.
    ToleranceNotMet,
    /// The evaluation was refused by its resource admission.
    Refused(ResourceLimit),
}

/// A point of a fitted track and its first derivative by the path angle.
#[derive(Debug, Clone, Copy)]
struct TrackSample {
    point: Point3,
    tangent: Vector3,
}

/// A curve over the path angle that the fit follows.
type Track<'a> = dyn Fn(f64) -> Result<TrackSample, HelixNurbsError> + 'a;

/// A cubic B-spline that stays within `tolerance` of `helix` over its whole
/// angle range.
///
/// Each segment is the cubic Hermite interpolant of the helix point and
/// tangent at both segment ends, so the spline passes through the helix at
/// every segment end and shares its parameter there. Interior knots carry
/// multiplicity three: the poles of each segment are its Bézier poles.
///
/// The deviation is measured between the spline and the helix at the same
/// parameter. The segment count starts at one and doubles until no segment
/// spans more than `MAX_SEGMENT_ANGLE` and every sampled deviation is within
/// `tolerance`. The fit is refused when `2^MAX_REFINEMENTS` segments do not
/// meet both conditions.
pub fn helix_nurbs(
    helix: &HelixCurveConstruction,
    tolerance: PositiveLength,
) -> Result<NurbsCurve, HelixNurbsError> {
    let [start, end] = helix.angle_range().finite_components();
    let track = |parameter: f64| helix_sample(helix, parameter);
    let (breaks, mut poles) = fit_tracks(&[&track], start.get(), end.get(), tolerance.get())?;
    let poles = poles.pop().ok_or(HelixNurbsError::NotFinite)?;
    let poles = poles
        .into_iter()
        .map(|pole| FinitePoint3::new(pole).ok_or(HelixNurbsError::NotFinite))
        .collect::<Result<Vec<_>, _>>()?;
    NurbsCurve::from_lanes(3, hermite_knots(&breaks)?, poles, None, false)
        .map_err(|_| HelixNurbsError::NotFinite)
}

/// A B-spline surface that stays within `tolerance` of the swept surface of
/// `surface` over its path angle interval.
///
/// The surface parameter `v` is the path angle, fitted with the cubic Hermite
/// segments of [`helix_nurbs`]. A line profile keeps its own parameter as a
/// degree-one `u` over the stored profile interval. A circle profile becomes
/// the whole circle as four rational quadratic arcs in `u`, so `u` is not the
/// native profile angle. Every pole row follows one track over the path, and
/// the fit refines the shared segments until every track is within
/// `tolerance`; a convex combination of the tracks stays within it as well.
pub fn helix_surface_nurbs(
    surface: &HelixSurfaceConstruction,
    tolerance: PositiveLength,
) -> Result<NurbsSurface, HelixNurbsError> {
    let path = path_helix(surface)?;
    let [start, end] = path.angle_range().finite_components();
    let (start, end) = (start.get(), end.get());
    let axis = unit(path.axis().get())?;
    match surface.profile() {
        HelixSurfaceProfile::Line(line) => {
            let direction = line.direction().get();
            let [first, second] = ordered(surface.angle_range().get());
            let row = |profile: f64| {
                move |parameter: f64| {
                    let sample = helix_sample(&path, parameter)?;
                    let (radial, tangential) = path_frame(&path, parameter);
                    let offset = sum(&[
                        (direction.x, radial),
                        (direction.y, tangential),
                        (direction.z, axis),
                    ]);
                    let turn = sum(&[(direction.x, tangential), (-direction.y, radial)]);
                    Ok(TrackSample {
                        point: shifted(sample.point, offset, profile),
                        tangent: sum(&[(1.0, sample.tangent), (profile, turn)]),
                    })
                }
            };
            let (first_row, second_row) = (row(first), row(second));
            let (breaks, rows) =
                fit_tracks(&[&first_row, &second_row], start, end, tolerance.get())?;
            let rows = rows
                .into_iter()
                .map(|row| {
                    row.into_iter()
                        .map(|pole| FinitePoint3::new(pole).ok_or(HelixNurbsError::NotFinite))
                        .collect()
                })
                .collect::<Result<Vec<Vec<_>>, _>>()?;
            NurbsSurface::new(
                NurbsSurfaceAxis::new(1, vec![first, first, second, second], false),
                NurbsSurfaceAxis::new(3, hermite_knots(&breaks)?, false),
                NurbsPoleGrid::Polynomial { rows },
                false,
            )
            .map_err(|_| HelixNurbsError::NotFinite)
        }
        HelixSurfaceProfile::Circle(circle) => {
            let radius = circle.radius().get().abs();
            let (profile_poles, profile_knots) = circle_arcs()?;
            let rows = profile_poles
                .iter()
                .map(|&ProfilePole { cosine, sine, .. }| {
                    move |parameter: f64| {
                        let sample = helix_sample(&path, parameter)?;
                        let (radial, tangential) = path_frame(&path, parameter);
                        let length = radial.norm();
                        if length == 0.0 {
                            return Err(HelixNurbsError::NotFinite);
                        }
                        let outward = radial.scale(1.0 / length);
                        let along = radial.dot(tangential) / length;
                        let outward_turn =
                            sum(&[(1.0 / length, tangential), (-along / length, outward)]);
                        Ok(TrackSample {
                            point: shifted(
                                sample.point,
                                sum(&[(-cosine, outward), (sine, axis)]),
                                radius,
                            ),
                            tangent: sum(&[
                                (1.0, sample.tangent),
                                (-cosine * radius, outward_turn),
                            ]),
                        })
                    }
                })
                .collect::<Vec<_>>();
            let tracks = rows
                .iter()
                .map(|row| {
                    let track: &Track<'_> = row;
                    track
                })
                .collect::<Vec<_>>();
            let (breaks, rows) = fit_tracks(&tracks, start, end, tolerance.get())?;
            let rows = rows
                .into_iter()
                .zip(&profile_poles)
                .map(|(row, &ProfilePole { weight, .. })| {
                    let weight = NonZeroReal::new(weight).ok_or(HelixNurbsError::NotFinite)?;
                    row.into_iter()
                        .map(|pole| {
                            Ok(WeightedPole3 {
                                point: FinitePoint3::new(pole).ok_or(HelixNurbsError::NotFinite)?,
                                weight,
                            })
                        })
                        .collect()
                })
                .collect::<Result<Vec<Vec<_>>, HelixNurbsError>>()?;
            NurbsSurface::new(
                NurbsSurfaceAxis::new(2, profile_knots, false),
                NurbsSurfaceAxis::new(3, hermite_knots(&breaks)?, false),
                NurbsPoleGrid::Rational { rows },
                false,
            )
            .map_err(|_| HelixNurbsError::NotFinite)
        }
    }
}

/// The surface path as a helix curve over the surface's path angle interval.
fn path_helix(
    surface: &HelixSurfaceConstruction,
) -> Result<HelixCurveConstruction, HelixNurbsError> {
    let path = surface.path();
    HelixCurveConstruction::try_new(
        ordered(surface.dimension_range().get()),
        HelixFrame {
            center: *path.center().as_raw(),
            major: path.major().get(),
            minor: path.minor().get(),
            pitch: path.pitch().get(),
            axis: path.axis().get(),
        },
        path.apex_factor().get(),
        None,
    )
    .map_err(|_| HelixNurbsError::NotFinite)
}

/// The radial vector `ρ(t)` and its derivative `τ(t)` of the path frame.
fn path_frame(path: &HelixCurveConstruction, parameter: f64) -> (Vector3, Vector3) {
    let (sine, cosine) = parameter.portable_sin_cos();
    let (major, minor) = (path.major().get(), path.minor().get());
    (
        sum(&[(cosine, major), (sine, minor)]),
        sum(&[(-sine, major), (cosine, minor)]),
    )
}

/// One pole of the unit profile circle and its rational weight.
#[derive(Debug, Clone, Copy)]
struct ProfilePole {
    cosine: f64,
    sine: f64,
    weight: f64,
}

/// The poles of the unit circle as four rational quadratic arcs, with the
/// knot vector of the arcs over `[0, 4]`.
fn circle_arcs() -> Result<(Vec<ProfilePole>, Vec<f64>), HelixNurbsError> {
    let half = std::f64::consts::FRAC_PI_4;
    let mut poles = Vec::with_capacity(2 * PROFILE_CIRCLE_ARCS + 1);
    let mut knots = vec![0.0; 3];
    for arc in 0..PROFILE_CIRCLE_ARCS {
        let index = f64_from_index(arc).ok_or(HelixNurbsError::NotFinite)?;
        let from = index * std::f64::consts::FRAC_PI_2;
        if poles.is_empty() {
            poles.push(ProfilePole {
                cosine: from.portable_cos(),
                sine: from.portable_sin(),
                weight: 1.0,
            });
        }
        let middle = from + half;
        poles.push(ProfilePole {
            cosine: middle.portable_cos() / half.portable_cos(),
            sine: middle.portable_sin() / half.portable_cos(),
            weight: half.portable_cos(),
        });
        let to = from + std::f64::consts::FRAC_PI_2;
        poles.push(ProfilePole {
            cosine: to.portable_cos(),
            sine: to.portable_sin(),
            weight: 1.0,
        });
        knots.extend([index + 1.0; 2]);
    }
    knots.push(f64_from_index(PROFILE_CIRCLE_ARCS).ok_or(HelixNurbsError::NotFinite)?);
    Ok((poles, knots))
}

fn helix_sample(
    helix: &HelixCurveConstruction,
    parameter: f64,
) -> Result<TrackSample, HelixNurbsError> {
    let differential = differential(helix, parameter)?;
    Ok(TrackSample {
        point: differential.point.get(),
        tangent: tangent(&differential)?.get(),
    })
}

fn differential(
    helix: &HelixCurveConstruction,
    parameter: f64,
) -> Result<ModelCurveDifferential, HelixNurbsError> {
    finite_or_refusal(helix_differential(helix, parameter))
        .map_err(HelixNurbsError::Refused)?
        .ok_or(HelixNurbsError::NotFinite)
}

fn tangent(differential: &ModelCurveDifferential) -> Result<FiniteVector3, HelixNurbsError> {
    finite_or_refusal(differential.tangent())
        .map_err(HelixNurbsError::Refused)?
        .ok_or(HelixNurbsError::NotFinite)
}

/// Shared segment breaks over `[start, end]` and the Hermite Bézier poles of
/// every track, refined until every track is within `tolerance`.
fn fit_tracks(
    tracks: &[&Track<'_>],
    start: f64,
    end: f64,
    tolerance: f64,
) -> Result<(Vec<f64>, Vec<Vec<Point3>>), HelixNurbsError> {
    let span = end - start;
    if span.is_nan() || span <= 0.0 {
        return Err(HelixNurbsError::NotFinite);
    }
    let mut segments: usize = 1;
    'refine: for _ in 0..=MAX_REFINEMENTS {
        let count = f64_from_index(segments).ok_or(HelixNurbsError::ToleranceNotMet)?;
        if span / count <= MAX_SEGMENT_ANGLE {
            let breaks = segment_breaks(start, end, segments)?;
            let mut rows = Vec::with_capacity(tracks.len());
            for track in tracks {
                let ends = breaks
                    .iter()
                    .map(|&parameter| track(parameter))
                    .collect::<Result<Vec<_>, _>>()?;
                let poles = hermite_poles(&breaks, &ends)?;
                if !within_tolerance(track, &breaks, &poles, tolerance)? {
                    segments = segments
                        .checked_mul(2)
                        .ok_or(HelixNurbsError::ToleranceNotMet)?;
                    continue 'refine;
                }
                rows.push(poles);
            }
            return Ok((breaks, rows));
        }
        segments = segments
            .checked_mul(2)
            .ok_or(HelixNurbsError::ToleranceNotMet)?;
    }
    Err(HelixNurbsError::ToleranceNotMet)
}

/// `segments + 1` parameters from `start` to `end` in equal steps, with both
/// ends exact.
fn segment_breaks(start: f64, end: f64, segments: usize) -> Result<Vec<f64>, HelixNurbsError> {
    let count = f64_from_index(segments).ok_or(HelixNurbsError::ToleranceNotMet)?;
    let capacity = segments
        .checked_add(1)
        .ok_or(HelixNurbsError::ToleranceNotMet)?;
    let mut breaks = Vec::with_capacity(capacity);
    for index in 0..segments {
        let fraction = f64_from_index(index).ok_or(HelixNurbsError::ToleranceNotMet)? / count;
        breaks.push(start + (end - start) * fraction);
    }
    breaks.push(end);
    Ok(breaks)
}

/// The Bézier poles of every segment, with each shared segment end once.
fn hermite_poles(breaks: &[f64], ends: &[TrackSample]) -> Result<Vec<Point3>, HelixNurbsError> {
    let capacity = ends
        .len()
        .checked_mul(3)
        .ok_or(HelixNurbsError::ToleranceNotMet)?;
    let mut poles = Vec::with_capacity(capacity);
    for (pair, parameters) in ends.windows(2).zip(breaks.windows(2)) {
        let [from, to] = [pair[0], pair[1]];
        let third = (parameters[1] - parameters[0]) / 3.0;
        if poles.is_empty() {
            poles.push(from.point);
        }
        poles.push(shifted(from.point, from.tangent, third));
        poles.push(shifted(to.point, to.tangent, -third));
        poles.push(to.point);
    }
    Ok(poles)
}

fn within_tolerance(
    track: &Track<'_>,
    breaks: &[f64],
    poles: &[Point3],
    tolerance: f64,
) -> Result<bool, HelixNurbsError> {
    for (segment, parameters) in poles.windows(4).step_by(3).zip(breaks.windows(2)) {
        for fraction in DEVIATION_SAMPLES {
            let parameter = parameters[0] + (parameters[1] - parameters[0]) * fraction;
            let exact = track(parameter)?.point;
            let fitted = bezier_point(segment, fraction);
            let deviation =
                Vector3::new(fitted.x - exact.x, fitted.y - exact.y, fitted.z - exact.z).norm();
            if deviation.is_nan() || deviation > tolerance {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// The clamped cubic knot vector whose interior breaks carry multiplicity
/// three.
fn hermite_knots(breaks: &[f64]) -> Result<Vec<f64>, HelixNurbsError> {
    let (Some(&first), Some(&last)) = (breaks.first(), breaks.last()) else {
        return Err(HelixNurbsError::NotFinite);
    };
    let last_index = breaks
        .len()
        .checked_sub(1)
        .ok_or(HelixNurbsError::NotFinite)?;
    let interior = breaks
        .get(1..last_index)
        .ok_or(HelixNurbsError::NotFinite)?;
    let mut knots = vec![first; 4];
    for &knot in interior {
        knots.extend([knot; 3]);
    }
    knots.extend([last; 4]);
    Ok(knots)
}

fn bezier_point(poles: &[Point3], fraction: f64) -> Point3 {
    let rest = 1.0 - fraction;
    let weights = [
        rest * rest * rest,
        3.0 * rest * rest * fraction,
        3.0 * rest * fraction * fraction,
        fraction * fraction * fraction,
    ];
    let mut point = Point3::new(0.0, 0.0, 0.0);
    for (pole, weight) in poles.iter().zip(weights) {
        point = shifted(point, Vector3::new(pole.x, pole.y, pole.z), weight);
    }
    point
}

fn shifted(point: Point3, direction: Vector3, scale: f64) -> Point3 {
    Point3::new(
        point.x + direction.x * scale,
        point.y + direction.y * scale,
        point.z + direction.z * scale,
    )
}

fn sum(terms: &[(f64, Vector3)]) -> Vector3 {
    terms
        .iter()
        .fold(Vector3::new(0.0, 0.0, 0.0), |total, &(scale, vector)| {
            Vector3::new(
                total.x + vector.x * scale,
                total.y + vector.y * scale,
                total.z + vector.z * scale,
            )
        })
}

fn unit(vector: Vector3) -> Result<Vector3, HelixNurbsError> {
    let length = vector.norm();
    if length.is_nan() || length == 0.0 {
        return Err(HelixNurbsError::NotFinite);
    }
    Ok(vector.scale(1.0 / length))
}

fn ordered([first, second]: [f64; 2]) -> [f64; 2] {
    if first <= second {
        [first, second]
    } else {
        [second, first]
    }
}

#[cfg(test)]
mod tests;
