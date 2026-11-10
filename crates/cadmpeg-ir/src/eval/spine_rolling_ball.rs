// SPDX-License-Identifier: Apache-2.0
//! Constant rolling-ball sections solved from a spine and analytic supports.
//!
//! The surface parameter `u` runs along the spine and `v` runs across the
//! section, from the contact on the first support at zero to the contact on the
//! second support at one.

use super::admission;
use super::admit_point;
use super::minor_circular_arc_point;
use super::model_curve_differential_by_id_inner;
use super::offset;
use super::surface_first_order;
use super::surface_jet;
use super::EvaluationFailure;
use super::UNREACHED_POINT;
use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::nurbs::NurbsSurface;
use crate::geometry::surface_payloads::BlendSurfacePayload;
use crate::geometry::{
    BlendCrossSection, BlendRadiusLaw, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use crate::ids::{CurveId, SurfaceId};
use crate::math::portable::Portable;
use crate::math::{Point3, Vector3};
use cadmpeg_core::convert::f64_from_index;

const SECTION_ITERATIONS: usize = 64;
/// A well-conditioned section converges within this many plain steps.
const SECTION_PLAIN_ITERATIONS: usize = 8;
/// Distance residual in millimetres at which a section counts as solved.
const SECTION_RESIDUAL: f64 = 1.0e-10;
/// Distance in millimetres between a section's plane and its target at which an ill-conditioned
/// solve stops trying. This moves the section along the blend without taking it off the surface.
const SECTION_PLANE_RESIDUAL: f64 = 1.0e-7;
const SECTION_MIN_DETERMINANT: f64 = 1.0e-12;
/// Surfaces with a sine of crossing angle below this meet too tangentially to fix
/// a point to the residual limit.
const INTERSECTION_MIN_DETERMINANT: f64 = 1.0e-9;
const NURBS_ITERATIONS: usize = 32;
const SPINE_SEARCH_STEPS: usize = 128;
const SPINE_BISECTIONS: usize = 48;
/// Fraction of a spine's span that a face may run past either end of the spine.
const SPINE_END_REACH: f64 = 0.5;
/// Fraction of a face's span added on each side, so that its trims stay on the
/// carrier when an edge is a chordal approximation of the true rail.
const DOMAIN_PADDING: f64 = 0.02;

/// A section's center and its contact point on each support.
#[derive(Debug, Clone, Copy)]
struct RollingBallSection {
    center: Point3,
    contacts: [Point3; 2],
}

/// The surface point of a constant circular blend that stores no section cache.
pub(super) fn spine_rolling_ball_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    payload: &BlendSurfacePayload,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let no_value = EvaluationFailure::NoValue;
    let BlendRadiusLaw::Constant { signed_radius } = payload.radius() else {
        return Err(no_value);
    };
    if *payload.cross_section() != BlendCrossSection::Circular || !u.is_finite() || !v.is_finite() {
        return Err(no_value);
    }
    let radius = signed_radius.get().abs();
    let spine = payload.spine().as_ref().ok_or(no_value)?;
    let [first, second] = payload.supports();
    let first = support_geometry(admission, index, &first.as_ref().ok_or(no_value)?.surface)?;
    let second = support_geometry(admission, index, &second.as_ref().ok_or(no_value)?.surface)?;
    let (seed, tangent) = spine_seed(admission, index, spine, u)?;
    let section =
        supported_section(admission, [first, second], seed, tangent, radius)?.ok_or(no_value)?;
    let point = minor_circular_arc_point(
        section.center,
        section.contacts[0],
        section.contacts[1],
        radius,
        v,
    )
    .map_err(|failure| failure.map(|()| UNREACHED_POINT))?;
    admit_point(point)
}

/// The `[u, v]` box that covers `points` on a constant circular blend.
///
/// `whole_spine` states that the face runs all the way around a closed spine.
///
/// # Errors
///
/// Returns the failure of an evaluation that could not complete. A point with no
/// section leaves the box undefined and yields `Ok(None)`.
pub fn spine_rolling_ball_domain(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    payload: &BlendSurfacePayload,
    points: &[Point3],
    whole_spine: bool,
) -> Result<Option<[[f64; 2]; 2]>, EvaluationFailure<Point3>> {
    let no_value = EvaluationFailure::NoValue;
    let BlendRadiusLaw::Constant { signed_radius } = payload.radius() else {
        return Ok(None);
    };
    let radius = signed_radius.get().abs();
    let spine = payload.spine().as_ref().ok_or(no_value)?;
    let [first, second] = payload.supports();
    let first = support_geometry(admission, index, &first.as_ref().ok_or(no_value)?.surface)?;
    let second = support_geometry(admission, index, &second.as_ref().ok_or(no_value)?.surface)?;
    let Some((domain, periodic, closed)) = spine_domain(admission, index, spine)? else {
        return Ok(None);
    };
    let mut spine_parameters = Vec::with_capacity(points.len());
    let mut section_parameters = Vec::with_capacity(points.len());
    for point in points {
        let Some(u) = spine_parameter(admission, index, spine, domain, *point)? else {
            return Ok(None);
        };
        let (seed, tangent) = spine_seed(admission, index, spine, u)?;
        let Some(section) = supported_section(admission, [first, second], seed, tangent, radius)?
        else {
            return Ok(None);
        };
        let Some(v) = section_parameter(section, *point) else {
            return Ok(None);
        };
        spine_parameters.push(u);
        section_parameters.push(v);
    }
    // The contacts at zero and one bound every section, even when a rail's edge
    // has no curve to sample.
    section_parameters.extend([0.0, 1.0]);
    let Some(v_range) = padded_range(&section_parameters) else {
        return Ok(None);
    };
    // Sample points on closed rails can miss the seam of a closed spine, and a band that stops
    // short of its seam has no surface for the two closed rails to bound.
    let u_range = if closed && whole_spine {
        Some(domain)
    } else if periodic {
        padded_range(&unwrapped(&spine_parameters, domain[1] - domain[0]))
    } else {
        padded_range(&spine_parameters)
    };
    Ok(u_range.map(|u_range| [u_range, v_range]))
}

/// A solved surface under a solve. It keeps its NURBS parameters between solver
/// steps, so that each closest point starts from the previous one.
struct Support<'surface, 'ctx, 'arena> {
    admission: admission::EvaluationAdmission<'ctx, 'arena>,
    geometry: &'surface SurfaceGeometry,
    parameters: Option<[f64; 2]>,
}

impl<'surface, 'ctx, 'arena> Support<'surface, 'ctx, 'arena> {
    fn new(
        admission: admission::EvaluationAdmission<'ctx, 'arena>,
        geometry: &'surface SurfaceGeometry,
    ) -> Self {
        Self {
            admission,
            geometry,
            parameters: None,
        }
    }

    fn closest_foot(&mut self, point: Point3) -> Result<Option<Point3>, EvaluationFailure<Point3>> {
        match self.geometry.solved() {
            Some(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
                let found = closest_nurbs_point(
                    self.admission,
                    self.geometry,
                    nurbs,
                    point,
                    self.parameters,
                )?;
                self.parameters = found.map(|(_, uv)| uv);
                Ok(found.map(|(foot, _)| foot))
            }
            Some(solved) => Ok(closest_point(solved, point)),
            None => Ok(None),
        }
    }

    /// The closest point and the unit normal there.
    fn foot_and_normal(
        &mut self,
        point: Point3,
    ) -> Result<Option<(Point3, Vector3)>, EvaluationFailure<Point3>> {
        let Some(foot) = self.closest_foot(point)? else {
            return Ok(None);
        };
        let normal = match (self.geometry.solved(), self.parameters) {
            (Some(SolvedSurfaceGeometry::Nurbs(_)), Some([u, v])) => {
                let [du, dv] = surface_first_order(self.admission, self.geometry, u, v)?
                    .first
                    .map_err(|failure| failure.map(|()| UNREACHED_POINT))?
                    .map(FiniteVector3::get);
                unit(du.cross(dv))
            }
            (Some(solved), _) => surface_normal(solved, foot),
            (None, _) => None,
        };
        Ok(normal.map(|normal| (foot, normal)))
    }
}

/// The point at `u` on the exact intersection of two solved surfaces, seeded by a
/// degree-one chart of the same curve.
///
/// # Errors
///
/// Returns `NoValue` for another chart or surface, and where the surfaces meet
/// tangentially.
pub fn exact_intersection_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    supports: [&SurfaceGeometry; 2],
    chart: &crate::geometry::nurbs::NurbsCurve,
    u: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let no_value = EvaluationFailure::NoValue;
    if chart.degree() != 1 {
        return Err(no_value);
    }
    let parameters = chart_node_parameters(chart.knots().as_slice()).ok_or(no_value)?;
    let (seed, _) =
        smooth_chart_point(parameters, &chart.pole_rows().raw_points(), u).ok_or(no_value)?;
    let supports = supports.map(|geometry| Support::new(admission, geometry));
    admit_point(intersection_point(supports, seed)?.ok_or(no_value)?)
}

/// The point at `u` on the rail of a constant rolling ball where it touches support
/// `side`, seeded by a degree-one chart of the rail.
///
/// A blend meets its support tangentially, so the rail is not an intersection that
/// can be solved from the two surfaces. It is the ball's contact point instead.
///
/// # Errors
///
/// Returns `NoValue` for another chart or surface, and where no section exists.
pub fn exact_blend_rail_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    supports: [&SurfaceGeometry; 2],
    side: usize,
    radius: f64,
    chart: &crate::geometry::nurbs::NurbsCurve,
    u: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let no_value = EvaluationFailure::NoValue;
    if chart.degree() != 1 || side > 1 {
        return Err(no_value);
    }
    let parameters = chart_node_parameters(chart.knots().as_slice()).ok_or(no_value)?;
    let (seed, derivative) =
        smooth_chart_point(parameters, &chart.pole_rows().raw_points(), u).ok_or(no_value)?;
    let mut supports = supports.map(|geometry| Support::new(admission, geometry));
    let (foot, normal) = supports[side].foot_and_normal(seed)?.ok_or(no_value)?;
    let start = ball_center_on_supported_side(&mut supports[1 - side], foot, normal, radius)?
        .ok_or(no_value)?;
    let section = anchored_rolling_ball_section(
        |support, point| supports[support].closest_foot(point),
        start,
        seed,
        Some(side),
        derivative,
        radius,
    )?
    .ok_or(no_value)?;
    admit_point(*section.contacts.get(side).ok_or(no_value)?)
}

/// The ball sits on one side of the surface at `foot`; the other support's distance
/// from each candidate center selects that side.
fn ball_center_on_supported_side(
    other: &mut Support<'_, '_, '_>,
    foot: Point3,
    normal: Vector3,
    radius: f64,
) -> Result<Option<Point3>, EvaluationFailure<Point3>> {
    let mut best: Option<(Point3, f64)> = None;
    for signed in [radius, -radius] {
        let center = offset(foot, &[(signed, normal)]);
        let Some(contact) = other.closest_foot(center)? else {
            continue;
        };
        let miss = (difference(center, contact).norm() - radius).abs();
        if best.is_none_or(|(_, nearest)| miss < nearest) {
            best = Some((center, miss));
        }
    }
    Ok(best.map(|(center, _)| center))
}

/// The point at `u` on the intersection of a constant rolling ball's surface with
/// a solved surface that is not one of its supports, seeded by a degree-one chart.
///
/// # Errors
///
/// Returns `NoValue` for another chart or surface, and where no point exists.
pub fn exact_blend_cap_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    supports: [&SurfaceGeometry; 2],
    radius: f64,
    other: &SurfaceGeometry,
    chart: &crate::geometry::nurbs::NurbsCurve,
    u: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let no_value = EvaluationFailure::NoValue;
    if chart.degree() != 1 {
        return Err(no_value);
    }
    let parameters = chart_node_parameters(chart.knots().as_slice()).ok_or(no_value)?;
    let (seed, derivative) =
        smooth_chart_point(parameters, &chart.pole_rows().raw_points(), u).ok_or(no_value)?;
    let mut supports = supports.map(|geometry| Support::new(admission, geometry));
    let mut other = Support::new(admission, other);
    let mut point = seed;
    for _ in 0..SECTION_ITERATIONS {
        let (blend_foot, blend_normal) =
            closest_on_blend(&mut supports, radius, point, derivative)?.ok_or(no_value)?;
        let (other_foot, other_normal) = other.foot_and_normal(point)?.ok_or(no_value)?;
        let tangent = unit(blend_normal.cross(other_normal)).ok_or(no_value)?;
        let residuals = [
            difference(point, blend_foot).dot(blend_normal),
            difference(point, other_foot).dot(other_normal),
            difference(point, seed).dot(tangent),
        ];
        if residuals
            .iter()
            .all(|value| value.abs() <= SECTION_RESIDUAL)
        {
            return admit_point(point);
        }
        let other_cross_tangent = other_normal.cross(tangent);
        let tangent_cross_blend = tangent.cross(blend_normal);
        let blend_cross_other = blend_normal.cross(other_normal);
        let determinant = blend_normal.dot(other_cross_tangent);
        if determinant.abs() < INTERSECTION_MIN_DETERMINANT {
            return Err(no_value);
        }
        point = offset(
            point,
            &[
                (-residuals[0] / determinant, other_cross_tangent),
                (-residuals[1] / determinant, tangent_cross_blend),
                (-residuals[2] / determinant, blend_cross_other),
            ],
        );
    }
    Err(no_value)
}

/// The point of a rolling ball's surface nearest `point`, and the normal there.
fn closest_on_blend(
    supports: &mut [Support<'_, '_, '_>; 2],
    radius: f64,
    point: Point3,
    direction: Vector3,
) -> Result<Option<(Point3, Vector3)>, EvaluationFailure<Point3>> {
    let no_value = EvaluationFailure::NoValue;
    let (foot, normal) = supports[0].foot_and_normal(point)?.ok_or(no_value)?;
    let start =
        ball_center_on_supported_side(&mut supports[1], foot, normal, radius)?.ok_or(no_value)?;
    let Some(section) = anchored_rolling_ball_section(
        |side, position| supports[side].closest_foot(position),
        start,
        point,
        None,
        direction,
        radius,
    )?
    else {
        return Ok(None);
    };
    let outward = unit(difference(point, section.center)).ok_or(no_value)?;
    Ok(Some((
        offset(section.center, &[(radius, outward)]),
        outward,
    )))
}

/// The point of the intersection of two solved surfaces nearest `seed`.
fn intersection_point(
    mut supports: [Support<'_, '_, '_>; 2],
    seed: Point3,
) -> Result<Option<Point3>, EvaluationFailure<Point3>> {
    let mut point = seed;
    for _ in 0..SECTION_ITERATIONS {
        let [first_support, second_support] = &mut supports;
        let (Some((first, first_normal)), Some((second, second_normal))) = (
            first_support.foot_and_normal(point)?,
            second_support.foot_and_normal(point)?,
        ) else {
            return Ok(None);
        };
        // Surfaces that touch tangentially have no curve direction there, but a
        // point on both is still the answer.
        let tangent = unit(first_normal.cross(second_normal));
        let residuals = [
            difference(point, first).dot(first_normal),
            difference(point, second).dot(second_normal),
            tangent.map_or(0.0, |tangent| difference(point, seed).dot(tangent)),
        ];
        if residuals
            .iter()
            .all(|value| value.abs() <= SECTION_RESIDUAL)
        {
            return Ok(Some(point));
        }
        let Some(tangent) = tangent else {
            return Ok(None);
        };
        let second_cross_tangent = second_normal.cross(tangent);
        let tangent_cross_first = tangent.cross(first_normal);
        let first_cross_second = first_normal.cross(second_normal);
        let determinant = first_normal.dot(second_cross_tangent);
        if determinant.abs() < INTERSECTION_MIN_DETERMINANT {
            return Ok(None);
        }
        point = offset(
            point,
            &[
                (-residuals[0] / determinant, second_cross_tangent),
                (-residuals[1] / determinant, tangent_cross_first),
                (-residuals[2] / determinant, first_cross_second),
            ],
        );
    }
    Ok(None)
}

/// The unit normal of an analytic surface at a point on it.
fn surface_normal(surface: &SolvedSurfaceGeometry, point: Point3) -> Option<Vector3> {
    match surface {
        SolvedSurfaceGeometry::Plane(plane) => Some(*plane.frame().axis().as_raw()),
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            let axis = *cylinder.frame().axis().as_raw();
            axial_parts(point, cylinder.origin().get(), axis).map(|(_, radial)| radial)
        }
        SolvedSurfaceGeometry::Cone(cone) => {
            let axis = *cone.frame().axis().as_raw();
            let (_, radial) = axial_parts(point, cone.origin().get(), axis)?;
            let (sine, cosine) = cone.half_angle().get().portable_sin_cos();
            unit(Vector3::new(
                cosine * radial.x - sine * axis.x,
                cosine * radial.y - sine * axis.y,
                cosine * radial.z - sine * axis.z,
            ))
        }
        SolvedSurfaceGeometry::Sphere(sphere) => unit(difference(point, sphere.center().get())),
        SolvedSurfaceGeometry::Torus(torus) => {
            let center = torus.center().get();
            let axis = *torus.frame().axis().as_raw();
            let (_, radial) = axial_parts(point, center, axis)?;
            let tube_center = offset(center, &[(torus.major_radius().get(), radial)]);
            unit(difference(point, tube_center))
        }
        _ => None,
    }
}

type SpineDomain = ([f64; 2], bool, bool);

/// The parameter interval of a spine and whether it is periodic.
fn spine_domain(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    spine: &CurveId,
) -> Result<Option<SpineDomain>, EvaluationFailure<Point3>> {
    let curve = index
        .curves(spine.as_str(), admission)
        .map_err(EvaluationFailure::from)?
        .ok_or(EvaluationFailure::NoValue)?;
    Ok(match curve.geometry.solved() {
        Some(SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_)) => {
            Some(([0.0, std::f64::consts::TAU], true, true))
        }
        Some(SolvedCurveGeometry::Nurbs(nurbs)) => {
            let knots = nurbs.knots().as_slice();
            knots
                .first()
                .zip(knots.last())
                .map(|(start, end)| ([*start, *end], false, nurbs_closes_on_itself(nurbs)))
        }
        _ => None,
    })
}

fn nurbs_closes_on_itself(nurbs: &crate::geometry::nurbs::NurbsCurve) -> bool {
    const CLOSED_SPINE_GAP: f64 = 1e-9;
    let poles = nurbs.pole_rows().raw_points();
    poles
        .first()
        .zip(poles.last())
        .is_some_and(|(first, last)| first.distance(*last) <= CLOSED_SPINE_GAP)
}

/// The spine parameter whose section plane holds `point`, on the sheet of the
/// spine nearest to the point.
fn spine_parameter(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    spine: &CurveId,
    domain: [f64; 2],
    point: Point3,
) -> Result<Option<f64>, EvaluationFailure<Point3>> {
    let plane_distance = |u: f64| -> Result<(f64, f64), EvaluationFailure<Point3>> {
        let (seed, tangent) = spine_seed(admission, index, spine, u)?;
        let arm = difference(point, seed);
        let tangent = unit(tangent).ok_or(EvaluationFailure::NoValue)?;
        Ok((arm.dot(tangent), arm.norm()))
    };
    let at = |step: usize| {
        let fraction = f64_from_index(step).unwrap_or(f64::NAN)
            / f64_from_index(SPINE_SEARCH_STEPS).unwrap_or(f64::NAN);
        domain[0] + (domain[1] - domain[0]) * fraction
    };
    let mut best: Option<(f64, f64)> = None;
    let (mut before, _) = plane_distance(at(0))?;
    for step in 0..SPINE_SEARCH_STEPS {
        let (mut low, mut high) = (at(step), at(step + 1));
        let (after, _) = plane_distance(high)?;
        if before == 0.0 || before.signum() != after.signum() {
            let mut low_value = before;
            for _ in 0..SPINE_BISECTIONS {
                let middle = 0.5 * (low + high);
                let (value, _) = plane_distance(middle)?;
                if low_value.signum() == value.signum() {
                    low = middle;
                    low_value = value;
                } else {
                    high = middle;
                }
            }
            let u = 0.5 * (low + high);
            let (_, distance) = plane_distance(u)?;
            if best.is_none_or(|(_, nearest)| distance < nearest) {
                best = Some((u, distance));
            }
        }
        before = after;
    }
    if let Some((u, _)) = best {
        return Ok(Some(u));
    }
    // A stored spine can stop just short of the face's end section, so no section plane on the
    // spine holds a vertex there. The parameter then continues past the end at the end's speed.
    let reach = (domain[1] - domain[0]) * SPINE_END_REACH;
    let mut past: Option<(f64, f64)> = None;
    for (end, direction) in [(domain[0], -1.0), (domain[1], 1.0)] {
        let (_, tangent) = spine_seed(admission, index, spine, end)?;
        let (along, distance) = plane_distance(end)?;
        let beyond = along * direction / tangent.norm();
        if beyond > 0.0 && beyond <= reach && past.is_none_or(|(_, nearest)| distance < nearest) {
            past = Some((end + beyond * direction, distance));
        }
    }
    Ok(past.map(|(u, _)| u))
}

/// The fraction of the section arc at which `point` lies. It leaves `[0, 1]` for
/// a point past a contact.
fn section_parameter(section: RollingBallSection, point: Point3) -> Option<f64> {
    let first = difference(section.contacts[0], section.center);
    let second = difference(section.contacts[1], section.center);
    let arm = difference(point, section.center);
    let normal = first.cross(second);
    let axis = unit(normal)?;
    let sweep = normal.norm().portable_atan2(first.dot(second));
    let angle = axis.dot(first.cross(arm)).portable_atan2(first.dot(arm));
    (sweep > 0.0).then_some(angle / sweep)
}

/// Move periodic parameters onto one sheet, with the widest gap between them
/// outside the resulting interval.
fn unwrapped(parameters: &[f64], period: f64) -> Vec<f64> {
    let mut sorted = parameters.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mut start = sorted.first().copied().unwrap_or(0.0);
    let mut widest = sorted
        .first()
        .zip(sorted.last())
        .map_or(0.0, |(first, last)| first + period - last);
    for pair in sorted.windows(2) {
        if pair[1] - pair[0] > widest {
            widest = pair[1] - pair[0];
            start = pair[1];
        }
    }
    parameters
        .iter()
        .map(|parameter| {
            if *parameter < start {
                parameter + period
            } else {
                *parameter
            }
        })
        .collect()
}

fn padded_range(values: &[f64]) -> Option<[f64; 2]> {
    let low = values.iter().copied().reduce(f64::min)?;
    let high = values.iter().copied().reduce(f64::max)?;
    let pad = DOMAIN_PADDING * (high - low);
    (pad > 0.0).then_some([low - pad, high + pad])
}

/// What a rolling ball touches on one side. A blend of zero radius has collapsed onto its spine,
/// so a ball that uses it as a support passes through that curve: a cliff-edge blend.
#[derive(Clone, Copy)]
enum SectionSupport<'ir> {
    Surface(&'ir SurfaceGeometry),
    Edge(&'ir SolvedCurveGeometry),
}

fn support_geometry<'ir>(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'ir>,
    surface: &SurfaceId,
) -> Result<SectionSupport<'ir>, EvaluationFailure<Point3>> {
    let procedural = index
        .procedural_surface_for_surface(surface.as_str(), admission)
        .map_err(EvaluationFailure::from)?;
    if let Some(crate::geometry::ProceduralSurfaceDefinition::Blend(payload)) =
        procedural.map(crate::geometry::ProceduralSurface::definition)
    {
        if let (BlendRadiusLaw::Constant { signed_radius }, Some(spine)) =
            (payload.radius(), payload.spine())
        {
            if signed_radius.get() == 0.0 {
                let curve = index
                    .curves(spine.as_str(), admission)
                    .map_err(EvaluationFailure::from)?
                    .ok_or(EvaluationFailure::NoValue)?;
                return curve
                    .geometry
                    .solved()
                    .map(SectionSupport::Edge)
                    .ok_or(EvaluationFailure::NoValue);
            }
        }
    }
    index
        .surfaces(surface.as_str(), admission)
        .map_err(EvaluationFailure::from)?
        .map(|surface| SectionSupport::Surface(&surface.geometry))
        .ok_or(EvaluationFailure::NoValue)
}

fn closest_on_edge(edge: &SolvedCurveGeometry, point: Point3) -> Option<Point3> {
    match edge {
        SolvedCurveGeometry::Line(line) => {
            let origin = line.origin().get();
            let direction = *line.direction().as_raw();
            Some(offset(
                origin,
                &[(difference(point, origin).dot(direction), direction)],
            ))
        }
        SolvedCurveGeometry::Circle(circle) => {
            let center = circle.center().get();
            let axis = *circle.frame().axis().as_raw();
            let (_, radial) = axial_parts(point, center, axis)?;
            Some(offset(center, &[(circle.radius().get(), radial)]))
        }
        _ => None,
    }
}

fn supported_section(
    admission: admission::EvaluationAdmission<'_, '_>,
    supports: [SectionSupport<'_>; 2],
    seed: Point3,
    tangent: Vector3,
    radius: f64,
) -> Result<Option<RollingBallSection>, EvaluationFailure<Point3>> {
    let mut surfaces = supports.map(|support| match support {
        SectionSupport::Surface(geometry) => Some(Support::new(admission, geometry)),
        SectionSupport::Edge(_) => None,
    });
    rolling_ball_section(
        |side, point| match (&mut surfaces[side], supports[side]) {
            (Some(surface), _) => surface.closest_foot(point),
            (None, SectionSupport::Edge(edge)) => Ok(closest_on_edge(edge, point)),
            (None, SectionSupport::Surface(_)) => Ok(None),
        },
        seed,
        tangent,
        radius,
    )
}

/// The Greville parameters of the control point nearest `point`. A surface stays
/// close to its control net, and this seed spends no surface evaluations.
fn nearest_pole_parameters(nurbs: &NurbsSurface, point: Point3) -> Option<[f64; 2]> {
    let mut best: Option<(f64, usize)> = None;
    let poles = nurbs.poles();
    for (index, pole) in poles.iter().enumerate() {
        let distance = difference(point, pole.get()).norm();
        if best.is_none_or(|(nearest, _)| distance < nearest) {
            best = Some((distance, index));
        }
    }
    let (_, index) = best?;
    let row_length = nurbs
        .v_knots()
        .as_slice()
        .len()
        .checked_sub(usize::try_from(nurbs.v_degree()).ok()? + 1)?;
    let (row, column) = (
        index.checked_div(row_length)?,
        index.checked_rem(row_length)?,
    );
    let greville = |knots: &[f64], degree: u32, index: usize| {
        let degree = usize::try_from(degree).ok()?;
        let span = knots.get(index + 1..index + 1 + degree)?;
        if span.is_empty() {
            return knots.get(index).copied();
        }
        Some(span.iter().sum::<f64>() / f64_from_index(span.len())?)
    };
    Some([
        greville(nurbs.u_knots().as_slice(), nurbs.u_degree(), row)?,
        greville(nurbs.v_knots().as_slice(), nurbs.v_degree(), column)?,
    ])
}

/// The closest point on a NURBS surface and its parameters, refined by Gauss-Newton
/// steps from `start` or from the nearest control point.
fn closest_nurbs_point(
    admission: admission::EvaluationAdmission<'_, '_>,
    geometry: &SurfaceGeometry,
    nurbs: &NurbsSurface,
    point: Point3,
    start: Option<[f64; 2]>,
) -> Result<Option<(Point3, [f64; 2])>, EvaluationFailure<Point3>> {
    let domain = |knots: &[f64]| Some([*knots.first()?, *knots.last()?]);
    let (Some(u_domain), Some(v_domain)) = (
        domain(nurbs.u_knots().as_slice()),
        domain(nurbs.v_knots().as_slice()),
    ) else {
        return Ok(None);
    };
    let Some(mut uv) = start.or_else(|| nearest_pole_parameters(nurbs, point)) else {
        return Ok(None);
    };
    let mut current = nurbs_foot(admission, geometry, point, uv)?;
    for _ in 0..NURBS_ITERATIONS {
        let Some(at) = current else {
            return Ok(None);
        };
        let Some(step) = at.step() else {
            return Ok(None);
        };
        let target = [
            (uv[0] + step[0]).clamp(u_domain[0], u_domain[1]),
            (uv[1] + step[1]).clamp(v_domain[0], v_domain[1]),
        ];
        let moved =
            (target[0] - uv[0]).abs() * at.du.norm() + (target[1] - uv[1]).abs() * at.dv.norm();
        if moved <= SECTION_RESIDUAL * 0.01 {
            return Ok(Some((at.foot, target)));
        }
        // A full step can overshoot on a strongly curved surface, or land where the surface has
        // no derivatives, and then repeat forever, so a step that does not get closer is halved.
        let mut scale = 1.0;
        let accepted = loop {
            let trial = [
                uv[0] + (target[0] - uv[0]) * scale,
                uv[1] + (target[1] - uv[1]) * scale,
            ];
            let found = nurbs_foot(admission, geometry, point, trial)?;
            if let Some(found) = found {
                if found.arm.norm() <= at.arm.norm() {
                    break Some((trial, found));
                }
            }
            scale /= 2.0;
            if moved * scale <= SECTION_RESIDUAL * 0.01 {
                break None;
            }
        };
        let Some((trial, found)) = accepted else {
            return Ok(Some((at.foot, uv)));
        };
        uv = trial;
        current = Some(found);
    }
    Ok(None)
}

/// A NURBS surface point with what a closest-point step needs from it.
struct NurbsFoot {
    foot: Point3,
    arm: Vector3,
    du: Vector3,
    dv: Vector3,
    second: Option<[Vector3; 3]>,
}

impl NurbsFoot {
    /// The parameter step toward the closest point. It uses the surface's curvature where that
    /// describes a minimum of the distance, because the step without curvature slows to a crawl
    /// as the point nears a center of curvature.
    fn step(&self) -> Option<[f64; 2]> {
        let (du, dv, arm) = (self.du, self.dv, self.arm);
        let (right_u, right_v) = (arm.dot(du), arm.dot(dv));
        let solve = |uu: f64, uv: f64, vv: f64| {
            let determinant = uu * vv - uv * uv;
            (uu > 0.0 && determinant > f64::EPSILON * uu * vv).then(|| {
                [
                    (vv * right_u - uv * right_v) / determinant,
                    (uu * right_v - uv * right_u) / determinant,
                ]
            })
        };
        let (uu, uv, vv) = (du.dot(du), du.dot(dv), dv.dot(dv));
        self.second
            .and_then(|[duu, duv, dvv]| {
                solve(uu - arm.dot(duu), uv - arm.dot(duv), vv - arm.dot(dvv))
            })
            .or_else(|| solve(uu, uv, vv))
    }
}

/// The surface at `uv`, or `None` where it has no first derivatives, such as at a collapsed edge.
fn nurbs_foot(
    admission: admission::EvaluationAdmission<'_, '_>,
    geometry: &SurfaceGeometry,
    point: Point3,
    uv: [f64; 2],
) -> Result<Option<NurbsFoot>, EvaluationFailure<Point3>> {
    let jet = match surface_jet(admission, geometry, uv[0], uv[1]) {
        Ok(jet) => jet,
        Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(_)) => return Ok(None),
        Err(failure) => return Err(failure),
    };
    let foot = jet.point.get();
    let [du, dv] = match jet.first {
        Ok(first) => first.map(FiniteVector3::get),
        Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(())) => return Ok(None),
        Err(failure) => return Err(failure.map(|()| UNREACHED_POINT)),
    };
    let second = match jet.second {
        Ok(second) => Some(second.map(FiniteVector3::get)),
        Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(())) => None,
        Err(failure) => return Err(failure.map(|()| UNREACHED_POINT)),
    };
    Ok(Some(NurbsFoot {
        foot,
        arm: difference(point, foot),
        du,
        dv,
        second,
    }))
}

/// A point near the section center and the direction of the section's plane.
///
/// A degree-one spine is a coarse chart whose kinks would make the section vary
/// unevenly with `u`, so its poles seed a smooth curve through the same points.
fn spine_seed(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    spine: &CurveId,
    u: f64,
) -> Result<(Point3, Vector3), EvaluationFailure<Point3>> {
    let curve = index
        .curves(spine.as_str(), admission)
        .map_err(EvaluationFailure::from)?
        .ok_or(EvaluationFailure::NoValue)?;
    if let Some(SolvedCurveGeometry::Nurbs(nurbs)) = curve.geometry.solved() {
        if nurbs.degree() == 1 {
            let points = nurbs.pole_rows().raw_points();
            let parameters = chart_node_parameters(nurbs.knots().as_slice())
                .ok_or(EvaluationFailure::NoValue)?;
            return smooth_chart_point(parameters, &points, u).ok_or(EvaluationFailure::NoValue);
        }
    }
    let differential = model_curve_differential_by_id_inner(admission, index, spine, u)?;
    let tangent = differential
        .tangent
        .map_err(|failure| failure.map(|()| UNREACHED_POINT))?;
    Ok((differential.point.get(), tangent.get()))
}

/// The node parameters of a clamped degree-one chart: its knots without the repeated
/// first and last values.
fn chart_node_parameters(knots: &[f64]) -> Option<&[f64]> {
    let (_, rest) = knots.split_first()?;
    let (_, inner) = rest.split_last()?;
    Some(inner)
}

/// The point and derivative at `u` of the natural cubic spline through chart
/// nodes. Its continuous curvature keeps a solved section smooth across the nodes.
fn smooth_chart_point(parameters: &[f64], points: &[Point3], u: f64) -> Option<(Point3, Vector3)> {
    if parameters.len() != points.len() || parameters.len() < 2 {
        return None;
    }
    if parameters.windows(2).any(|pair| pair[0] >= pair[1]) {
        return None;
    }
    let coordinate = |select: fn(&Point3) -> f64| {
        let values = points.iter().map(select).collect::<Vec<_>>();
        let curvatures = natural_spline_curvatures(parameters, &values);
        spline_value(parameters, &values, &curvatures, u)
    };
    let (x, dx) = coordinate(|point| point.x);
    let (y, dy) = coordinate(|point| point.y);
    let (z, dz) = coordinate(|point| point.z);
    Some((Point3::new(x, y, z), Vector3::new(dx, dy, dz)))
}

/// Second derivatives of a natural cubic spline at its nodes, by the tridiagonal
/// sweep.
fn natural_spline_curvatures(parameters: &[f64], values: &[f64]) -> Vec<f64> {
    let count = parameters.len();
    let mut curvatures = vec![0.0; parameters.len()];
    let mut upper = vec![0.0; parameters.len()];
    let mut right = vec![0.0; parameters.len()];
    for node in 1..count - 1 {
        let before = parameters[node] - parameters[node - 1];
        let after = parameters[node + 1] - parameters[node];
        let slope_change =
            (values[node + 1] - values[node]) / after - (values[node] - values[node - 1]) / before;
        let pivot = 2.0 * (before + after) - before * upper[node - 1];
        upper[node] = after / pivot;
        right[node] = (6.0 * slope_change - before * right[node - 1]) / pivot;
    }
    for node in (1..count - 1).rev() {
        curvatures[node] = right[node] - upper[node] * curvatures[node + 1];
    }
    curvatures
}

fn spline_value(parameters: &[f64], values: &[f64], curvatures: &[f64], u: f64) -> (f64, f64) {
    let span = (parameters
        .partition_point(|parameter| *parameter <= u)
        .max(1)
        - 1)
    .min(parameters.len() - 2);
    let width = parameters[span + 1] - parameters[span];
    let after = (parameters[span + 1] - u) / width;
    let before = (u - parameters[span]) / width;
    let value = after * values[span]
        + before * values[span + 1]
        + ((after * after * after - after) * curvatures[span]
            + (before * before * before - before) * curvatures[span + 1])
            * width
            * width
            / 6.0;
    let slope = (values[span + 1] - values[span]) / width
        + ((3.0 * before * before - 1.0) * curvatures[span + 1]
            - (3.0 * after * after - 1.0) * curvatures[span])
            * width
            / 6.0;
    (value, slope)
}

/// Solve for the point at distance `radius` from both supports that is closest to
/// `seed`. `tangent` is the section direction where the supports are parallel.
fn rolling_ball_section(
    closest: impl FnMut(usize, Point3) -> Result<Option<Point3>, EvaluationFailure<Point3>>,
    seed: Point3,
    tangent: Vector3,
    radius: f64,
) -> Result<Option<RollingBallSection>, EvaluationFailure<Point3>> {
    anchored_rolling_ball_section(closest, seed, seed, None, tangent, radius)
}

/// Solve a section from the center `start`, holding `target` in the section's
/// plane through the center or, with `anchor`, through that side's contact point.
fn anchored_rolling_ball_section(
    mut closest: impl FnMut(usize, Point3) -> Result<Option<Point3>, EvaluationFailure<Point3>>,
    start: Point3,
    target: Point3,
    anchor: Option<usize>,
    tangent: Vector3,
    radius: f64,
) -> Result<Option<RollingBallSection>, EvaluationFailure<Point3>> {
    let Some(seed_tangent) = unit(tangent) else {
        return Ok(None);
    };
    let mut center = start;
    let mut plane_gain = 1.0;
    let mut last_plane_residual = 0.0;
    let mut settled = None;
    for iteration in 0..SECTION_ITERATIONS {
        let (Some(first), Some(second)) = (closest(0, center)?, closest(1, center)?) else {
            return Ok(None);
        };
        let first_arm = difference(center, first);
        let second_arm = difference(center, second);
        let (Some(first_normal), Some(second_normal)) = (unit(first_arm), unit(second_arm)) else {
            return Ok(None);
        };
        // The center curve's own tangent varies smoothly with the center, while
        // the tangent of a piecewise seed would put a kink in the section at
        // each chart node.
        let tangent = unit(first_normal.cross(second_normal)).unwrap_or(seed_tangent);
        let held = match anchor {
            Some(0) => first,
            Some(_) => second,
            None => center,
        };
        let residuals = [
            first_arm.norm() - radius,
            second_arm.norm() - radius,
            difference(held, target).dot(tangent),
        ];
        if residuals
            .iter()
            .all(|value| value.abs() <= SECTION_RESIDUAL)
        {
            return Ok(Some(RollingBallSection {
                center,
                contacts: [first, second],
            }));
        }
        settled = (residuals[0].abs() <= SECTION_RESIDUAL
            && residuals[1].abs() <= SECTION_RESIDUAL
            && residuals[2].abs() <= SECTION_PLANE_RESIDUAL)
            .then_some(RollingBallSection {
                center,
                contacts: [first, second],
            });
        let second_cross_tangent = second_normal.cross(tangent);
        let tangent_cross_first = tangent.cross(first_normal);
        let first_cross_second = first_normal.cross(second_normal);
        let determinant = first_normal.dot(second_cross_tangent);
        if determinant.abs() < SECTION_MIN_DETERMINANT {
            return Ok(None);
        }
        // The section plane turns with the center, so a target near the axis of the center curve
        // shrinks the plane's residual only slowly, and an overshooting one flips its sign. Past
        // the plain iterations, the residual's own response to the last step sizes the next one.
        if iteration >= SECTION_PLAIN_ITERATIONS && last_plane_residual != 0.0 {
            let response = (1.0 - residuals[2] / last_plane_residual) / plane_gain;
            plane_gain = if response > 0.0 { 1.0 / response } else { 1.0 };
            if residuals[2] != 0.0 {
                plane_gain = plane_gain.min((radius / residuals[2].abs()).max(1.0));
            }
        }
        last_plane_residual = residuals[2];
        center = offset(
            center,
            &[
                (-residuals[0] / determinant, second_cross_tangent),
                (-residuals[1] / determinant, tangent_cross_first),
                (-residuals[2] * plane_gain / determinant, first_cross_second),
            ],
        );
    }
    Ok(settled)
}

/// The closest point on an analytic surface, or `None` for any other surface and
/// for a point with no unique closest point.
fn closest_point(surface: &SolvedSurfaceGeometry, point: Point3) -> Option<Point3> {
    match surface {
        SolvedSurfaceGeometry::Plane(plane) => {
            let origin = plane.origin().get();
            let normal = *plane.frame().axis().as_raw();
            Some(offset(
                point,
                &[(-difference(point, origin).dot(normal), normal)],
            ))
        }
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            let origin = cylinder.origin().get();
            let axis = *cylinder.frame().axis().as_raw();
            let (along, radial) = axial_parts(point, origin, axis)?;
            Some(offset(
                origin,
                &[(along, axis), (cylinder.radius().get(), radial)],
            ))
        }
        SolvedSurfaceGeometry::Cone(cone) => {
            if (cone.ratio().get() - 1.0).abs() > f64::EPSILON {
                return None;
            }
            let origin = cone.origin().get();
            let axis = *cone.frame().axis().as_raw();
            let (along, radial) = axial_parts(point, origin, axis)?;
            let distance = difference(point, offset(origin, &[(along, axis)])).norm();
            let (sine, cosine) = cone.half_angle().get().portable_sin_cos();
            let radius = cone.radius().get();
            let slant = along * cosine + (distance - radius) * sine;
            Some(offset(
                origin,
                &[(slant * cosine, axis), (radius + slant * sine, radial)],
            ))
        }
        SolvedSurfaceGeometry::Sphere(sphere) => {
            let center = sphere.center().get();
            let radial = unit(difference(point, center))?;
            Some(offset(center, &[(sphere.radius().get(), radial)]))
        }
        SolvedSurfaceGeometry::Torus(torus) => {
            let center = torus.center().get();
            let axis = *torus.frame().axis().as_raw();
            let (_, radial) = axial_parts(point, center, axis)?;
            let tube_center = offset(center, &[(torus.major_radius().get(), radial)]);
            let tube_radial = unit(difference(point, tube_center))?;
            Some(offset(
                tube_center,
                &[(torus.minor_radius().get(), tube_radial)],
            ))
        }
        _ => None,
    }
}

/// The distance of `point` along an axis and the unit direction from the axis to
/// the point. A point on the axis has no direction.
fn axial_parts(point: Point3, origin: Point3, axis: Vector3) -> Option<(f64, Vector3)> {
    let arm = difference(point, origin);
    let along = arm.dot(axis);
    let radial = Vector3::new(
        arm.x - along * axis.x,
        arm.y - along * axis.y,
        arm.z - along * axis.z,
    );
    Some((along, unit(radial)?))
}

fn difference(to: Point3, from: Point3) -> Vector3 {
    Vector3::new(to.x - from.x, to.y - from.y, to.z - from.z)
}

fn unit(vector: Vector3) -> Option<Vector3> {
    let length = vector.norm();
    (length.is_finite() && length > 0.0)
        .then(|| Vector3::new(vector.x / length, vector.y / length, vector.z / length))
}

#[cfg(test)]
mod tests {
    use super::{
        closest_nurbs_point, closest_point, exact_blend_rail_point, exact_intersection_point,
        intersection_point, rolling_ball_section, smooth_chart_point, spine_parameter, Support,
    };
    use crate::eval::admission::EvaluationAdmission;
    use crate::geometry::analytic::{CylinderSurface, PlaneSurface};
    use crate::geometry::nurbs::{NurbsCurve, NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use crate::ids::CurveId;
    use crate::math::{Point3, Vector3};

    fn supports(geometry: &[SurfaceGeometry; 2]) -> [Support<'_, 'static, 'static>; 2] {
        [
            Support::new(EvaluationAdmission::Standard, &geometry[0]),
            Support::new(EvaluationAdmission::Standard, &geometry[1]),
        ]
    }

    /// The plane z = 0 and the same flat NURBS patch in the plane x = 0.
    fn plane_and_nurbs_wall() -> [SurfaceGeometry; 2] {
        let plane = SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        );
        let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 100.0, 100.0], false);
        let wall = NurbsSurface::new(
            axis(),
            axis(),
            NurbsPoleGrid::Polynomial {
                rows: vec![
                    vec![
                        Point3::new(0.0, -50.0, -50.0),
                        Point3::new(0.0, -50.0, 50.0),
                    ],
                    vec![Point3::new(0.0, 50.0, -50.0), Point3::new(0.0, 50.0, 50.0)],
                ],
            },
            false,
        )
        .unwrap();
        [
            SurfaceGeometry::Solved(plane),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(wall)),
        ]
    }

    #[test]
    fn point_just_past_a_spine_end_continues_the_parameter_and_a_far_one_has_none() {
        let mut ir = crate::examples::unit_cube().unwrap();
        let spine = CurveId::mint("test:spine:curve#chart").unwrap();
        let chart = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 5.0, 10.0, 10.0],
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(5.0, 0.0, 0.0),
                Point3::new(10.0, 0.0, 0.0),
            ],
            None,
            false,
        )
        .unwrap();
        ir.model.curves.push(crate::geometry::Curve {
            id: spine.clone(),
            geometry: crate::geometry::CurveGeometry::Solved(
                crate::geometry::SolvedCurveGeometry::Nurbs(chart),
            ),
            parameter_range: None,
            source_object: None,
        });
        let index = crate::index::ModelIndex::new_model_only(&ir);
        let parameter = |x: f64| {
            spine_parameter(
                EvaluationAdmission::Standard,
                &index,
                &spine,
                [0.0, 10.0],
                Point3::new(x, 1.0, 0.0),
            )
            .unwrap()
        };
        assert!((parameter(5.0).unwrap() - 5.0).abs() < 1.0e-9);
        assert!((parameter(10.4).unwrap() - 10.4).abs() < 1.0e-9);
        assert!((parameter(-0.4).unwrap() + 0.4).abs() < 1.0e-9);
        assert_eq!(parameter(16.0), None);
        assert_eq!(parameter(-6.0), None);
    }

    #[test]
    fn section_held_through_a_point_near_the_axis_of_the_center_curve_is_solved() {
        // A ball of radius 1 rolls on the plane z = 0 around a cylinder of radius 9, so its center
        // runs on a circle of radius 10. Every section plane holds the axis, so a target near the
        // axis fixes the plane only weakly, and each plain step gains only half a percent.
        let closest = |side: usize, point: Point3| {
            Ok(Some(if side == 0 {
                Point3::new(point.x, point.y, 0.0)
            } else {
                let scale = 9.0 / point.x.hypot(point.y);
                Point3::new(point.x * scale, point.y * scale, point.z)
            }))
        };
        let start = Point3::new(10.0 * 0.5_f64.cos(), 10.0 * 0.5_f64.sin(), 1.0);
        let target = Point3::new(0.05, 0.0, 1.0);
        let section = super::anchored_rolling_ball_section(
            closest,
            start,
            target,
            None,
            Vector3::new(0.0, 1.0, 0.0),
            1.0,
        )
        .unwrap()
        .unwrap();
        assert!((section.center.x - 10.0).abs() < 1.0e-7);
        assert!(section.center.y.abs() < 1.0e-7);
        assert!((section.center.z - 1.0).abs() < 1.0e-7);
    }

    #[test]
    fn closest_point_near_a_center_of_curvature_converges_to_the_foot() {
        // The parabola z = x^2 curves with radius 0.5 at its vertex, and the point lies 0.49 above
        // the vertex, where a step that ignores curvature shrinks the error by only two percent.
        let linear = NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
        let quadratic = NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false);
        let row = |x: f64, z: f64| vec![Point3::new(x, -1.0, z), Point3::new(x, 1.0, z)];
        let parabola = NurbsSurface::new(
            quadratic,
            linear,
            NurbsPoleGrid::Polynomial {
                rows: vec![row(-1.0, 1.0), row(0.0, -1.0), row(1.0, 1.0)],
            },
            false,
        )
        .unwrap();
        let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(parabola));
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) = &geometry else {
            unreachable!();
        };
        let (foot, parameters) = closest_nurbs_point(
            EvaluationAdmission::Standard,
            &geometry,
            nurbs,
            Point3::new(0.0, 0.0, 0.49),
            Some([0.6, 0.5]),
        )
        .unwrap()
        .unwrap();
        assert!(foot.x.abs() < 1.0e-9 && foot.y.abs() < 1.0e-9 && foot.z.abs() < 1.0e-9);
        assert!((parameters[0] - 0.5).abs() < 1.0e-9);
    }

    /// A straight degree-one chart along y, offset by `x` and `z`.
    #[test]
    fn ball_on_a_plane_and_a_cliff_edge_touches_the_plane_and_passes_through_the_edge() {
        let plane = SolvedSurfaceGeometry::Plane(
            crate::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        );
        let plane = SurfaceGeometry::Solved(plane);
        let edge = crate::geometry::SolvedCurveGeometry::Line(
            crate::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 6.0),
                Vector3::new(0.0, 1.0, 0.0),
            )
            .unwrap(),
        );
        let section = super::supported_section(
            crate::eval::admission::EvaluationAdmission::Standard,
            [
                super::SectionSupport::Edge(&edge),
                super::SectionSupport::Surface(&plane),
            ],
            Point3::new(3.0, 2.0, 4.0),
            Vector3::new(0.0, 1.0, 0.0),
            5.0,
        )
        .unwrap()
        .unwrap();

        assert!(distance(section.contacts[0], Point3::new(0.0, 2.0, 6.0)) < 1e-9);
        assert!(section.contacts[1].z.abs() < 1e-9);
        assert!((section.center.z - 5.0).abs() < 1e-9);
        assert!((distance(section.center, section.contacts[0]) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn only_a_spine_whose_end_poles_meet_closes_on_itself() {
        let ring = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 2.0, 3.0, 3.0],
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(0.0, 0.0, 0.0),
            ],
            None,
            false,
        )
        .unwrap();
        assert!(super::nurbs_closes_on_itself(&ring));
        assert!(!super::nurbs_closes_on_itself(&chart_along_y(0.0, 0.0)));
    }

    fn chart_along_y(x: f64, z: f64) -> NurbsCurve {
        NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 10.0, 10.0],
            vec![Point3::new(x, 0.0, z), Point3::new(x, 10.0, z)],
            None,
            false,
        )
        .unwrap()
    }

    #[test]
    fn intersection_with_a_nurbs_surface_lies_on_both_surfaces() {
        let geometry = plane_and_nurbs_wall();
        let point = exact_intersection_point(
            EvaluationAdmission::Standard,
            [&geometry[0], &geometry[1]],
            &chart_along_y(0.3, 0.2),
            4.0,
        )
        .unwrap()
        .get();

        assert!(point.x.abs() < 1.0e-9);
        assert!(point.z.abs() < 1.0e-9);
        assert!((point.y - 4.0).abs() < 1.0e-9);
    }

    #[test]
    fn rail_on_a_nurbs_support_touches_it_one_radius_from_the_other_support() {
        let geometry = plane_and_nurbs_wall();
        let point = exact_blend_rail_point(
            EvaluationAdmission::Standard,
            [&geometry[0], &geometry[1]],
            1,
            5.0,
            &chart_along_y(0.1, 5.2),
            4.0,
        )
        .unwrap()
        .get();

        assert!(point.x.abs() < 1.0e-9);
        assert!((point.z - 5.0).abs() < 1.0e-9);
        assert!((point.y - 4.0).abs() < 1.0e-9);
    }

    fn cylinder(axis: Vector3, reference: Vector3, radius: f64) -> SolvedSurfaceGeometry {
        SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(Point3::new(0.0, 0.0, 0.0), axis, reference, radius).unwrap(),
        )
    }

    fn distance(a: Point3, b: Point3) -> f64 {
        Vector3::new(a.x - b.x, a.y - b.y, a.z - b.z).norm()
    }

    #[test]
    fn section_between_crossing_cylinders_reaches_both_offsets_from_a_poor_seed() {
        let upright = cylinder(
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            30.0,
        );
        let across = cylinder(
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            20.0,
        );
        // The offsets meet at y = 20, x^2 = 35^2 - 20^2 and z^2 = 25^2 - 20^2.
        let seed = Point3::new(28.0, 20.5, 14.5);
        let tangent = Vector3::new(0.5, -0.7, 1.0);

        let supports = [&upright, &across];
        let section = rolling_ball_section(
            |side, point| Ok(closest_point(supports[side], point)),
            seed,
            tangent,
            5.0,
        )
        .unwrap()
        .unwrap();

        let center = section.center;
        assert!(((center.x * center.x + center.y * center.y).sqrt() - 35.0).abs() < 1.0e-9);
        assert!(((center.y * center.y + center.z * center.z).sqrt() - 25.0).abs() < 1.0e-9);
        assert!((distance(center, section.contacts[0]) - 5.0).abs() < 1.0e-9);
        assert!((distance(center, section.contacts[1]) - 5.0).abs() < 1.0e-9);
        let contact = section.contacts[0];
        assert!(((contact.x * contact.x + contact.y * contact.y).sqrt() - 30.0).abs() < 1.0e-9);
    }

    #[test]
    fn section_between_a_plane_and_an_oblique_cylinder_stays_in_the_seed_plane() {
        let plane = SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        );
        let axis = Vector3::new(0.5, 0.0, 0.75_f64.sqrt());
        let oblique = cylinder(axis, Vector3::new(0.0, 1.0, 0.0), 15.0);
        let seed = Point3::new(20.0, 3.0, 3.5);
        let tangent = Vector3::new(0.0, 1.0, 0.0);

        let supports = [&plane, &oblique];
        let section = rolling_ball_section(
            |side, point| Ok(closest_point(supports[side], point)),
            seed,
            tangent,
            3.0,
        )
        .unwrap()
        .unwrap();

        assert!((section.center.z - 3.0).abs() < 1.0e-9);
        assert!(section.contacts[0].z.abs() < 1.0e-9);
        // The center is the point of the offset curve nearest the seed: the arm
        // from the seed is normal to the curve there.
        let first = Vector3::new(0.0, 0.0, 1.0);
        let center = section.center;
        let along = center.x * axis.x + center.z * axis.z;
        let second = Vector3::new(
            center.x - along * axis.x,
            center.y,
            center.z - along * axis.z,
        );
        let curve_tangent = first.cross(second);
        let arm = Vector3::new(center.x - seed.x, center.y - seed.y, center.z - seed.z);
        assert!(arm.dot(curve_tangent).abs() < 1.0e-6 * curve_tangent.norm());
    }

    #[test]
    fn intersection_of_crossing_cylinders_lies_on_both_from_an_off_curve_seed() {
        let upright = cylinder(
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            30.0,
        );
        let across = cylinder(
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            20.0,
        );

        let geometry = [
            SurfaceGeometry::Solved(upright),
            SurfaceGeometry::Solved(across),
        ];
        let point = intersection_point(supports(&geometry), Point3::new(23.0, 12.5, 15.5))
            .unwrap()
            .unwrap();

        assert!(((point.x * point.x + point.y * point.y).sqrt() - 30.0).abs() < 1.0e-9);
        assert!(((point.y * point.y + point.z * point.z).sqrt() - 20.0).abs() < 1.0e-9);
    }

    #[test]
    fn tangent_contact_of_equal_cylinders_is_kept_as_an_intersection_point() {
        let upright = cylinder(
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            20.0,
        );
        let across = cylinder(
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            20.0,
        );
        let contact = Point3::new(0.0, 20.0, 0.0);

        let geometry = [
            SurfaceGeometry::Solved(upright),
            SurfaceGeometry::Solved(across),
        ];
        let point = intersection_point(supports(&geometry), contact)
            .unwrap()
            .unwrap();

        assert!(distance(point, contact) < 1.0e-12);
    }

    #[test]
    fn a_rail_contact_lies_on_its_support_and_keeps_the_seed_in_the_section_plane() {
        let upright = cylinder(
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            30.0,
        );
        let across = cylinder(
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            20.0,
        );
        let supports = [&upright, &across];
        let seed = Point3::new(24.0, 18.0, 15.0);
        let start = Point3::new(28.72, 20.0, 15.0);

        let section = super::anchored_rolling_ball_section(
            |side, point| Ok(closest_point(supports[side], point)),
            start,
            seed,
            Some(0),
            Vector3::new(0.0, 0.0, 1.0),
            5.0,
        )
        .unwrap()
        .unwrap();

        let contact = section.contacts[0];
        assert!(((contact.x * contact.x + contact.y * contact.y).sqrt() - 30.0).abs() < 1.0e-9);
        let center = section.center;
        assert!(((center.y * center.y + center.z * center.z).sqrt() - 25.0).abs() < 1.0e-9);
        assert!((distance(center, contact) - 5.0).abs() < 1.0e-9);
    }

    #[test]
    fn a_rolling_ball_surface_point_is_at_the_radius_from_its_section_center() {
        let upright = cylinder(
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            30.0,
        );
        let across = cylinder(
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            20.0,
        );
        let near_surface = Point3::new(27.0, 18.0, 16.0);
        let geometry = [
            SurfaceGeometry::Solved(upright),
            SurfaceGeometry::Solved(across),
        ];

        let (foot, normal) = super::closest_on_blend(
            &mut supports(&geometry),
            5.0,
            near_surface,
            Vector3::new(0.0, 0.0, 1.0),
        )
        .unwrap()
        .unwrap();

        // The arm from the point to its foot lies along the normal, and the normal
        // has unit length, so the foot is the nearest point on the ball's circle.
        assert!((normal.norm() - 1.0).abs() < 1.0e-12);
        let arm = Vector3::new(
            near_surface.x - foot.x,
            near_surface.y - foot.y,
            near_surface.z - foot.z,
        );
        assert!(arm.cross(normal).norm() < 1.0e-9);
    }

    #[test]
    fn a_free_form_support_has_no_closed_form_section() {
        let unsupported = SolvedSurfaceGeometry::Unknown { record: None };
        assert!(closest_point(&unsupported, Point3::new(1.0, 2.0, 3.0)).is_none());
    }

    #[test]
    fn smooth_chart_passes_through_its_nodes_without_a_kink() {
        let parameters = [0.0, 1.0, 3.0];
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(3.0, 1.0, 1.0),
        ];
        for (parameter, expected) in parameters.iter().zip(points) {
            let (point, _) = smooth_chart_point(&parameters, &points, *parameter).unwrap();
            assert!(distance(point, expected) < 1.0e-12);
        }

        let slope = |u: f64| smooth_chart_point(&parameters, &points, u).unwrap().1;
        let (before, after) = (slope(1.0 - 1.0e-9), slope(1.0 + 1.0e-9));
        let jump = Vector3::new(before.x - after.x, before.y - after.y, before.z - after.z);
        assert!(jump.norm() < 1.0e-6);

        // Continuous curvature: the slope changes at the same rate on both sides.
        let step = 1.0e-5;
        let (low, middle, high) = (slope(1.0 - step), slope(1.0), slope(1.0 + step));
        assert!(((middle.x - low.x) - (high.x - middle.x)).abs() < 1.0e-7);
        assert!(((middle.y - low.y) - (high.y - middle.y)).abs() < 1.0e-7);
        assert!(smooth_chart_point(&[0.0, 0.0], &points[..2], 0.0).is_none());
    }
}
