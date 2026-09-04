//! Shared NURBS, B-spline, and analytic-curve math utilities.
//!
//! Family-agnostic geometry math consumed across decode families and the
//! decode/transfer paths: knot expansion and pole counting, degree-5 jet to
//! B-spline conversion, tensor-product NURBS isocurve extraction, circular
//! interval canonicalization, and exact circular-helix fitting.

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_ir::geometry::{
    knots_nondecreasing, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, PcurveNurbs,
    ProceduralCurveDefinition,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

const EPS_NURBS_COARSE_GEOMETRY: f64 = 1.0e-6;
const EPS_NURBS_GEOMETRY: f64 = 1.0e-9;

const EPS_PERIODIC_SWEEP: f64 = EPS_NURBS_GEOMETRY;
const EPS_HELIX_FRAME: f64 = EPS_NURBS_GEOMETRY;
const EPS_HELIX_RADIUS: f64 = EPS_NURBS_GEOMETRY;
const EPS_HELIX_ORTHO: f64 = EPS_NURBS_GEOMETRY;
const EPS_HELIX_PITCH_ALIGNMENT: f64 = EPS_NURBS_GEOMETRY;
const EPS_RELATIVE_TOLERANCE: f64 = EPS_NURBS_COARSE_GEOMETRY;

fn finite_point2(point: Point2) -> bool {
    [point.u, point.v].into_iter().all(f64::is_finite)
}

fn finite_point3(point: Point3) -> bool {
    [point.x, point.y, point.z].into_iter().all(f64::is_finite)
}

fn finite_vector3(vector: Vector3) -> bool {
    [vector.x, vector.y, vector.z]
        .into_iter()
        .all(f64::is_finite)
}

fn valid_nurbs_curve(nurbs: &NurbsCurve) -> bool {
    nurbs.knots().iter().copied().all(f64::is_finite)
        && knots_nondecreasing(nurbs.knots())
        && nurbs.control_points().iter().copied().all(finite_point3)
        && nurbs.weights().is_none_or(|weights| {
            weights
                .iter()
                .copied()
                .all(|weight| weight.is_finite() && weight != 0.0)
        })
}

fn valid_pcurve_nurbs(nurbs: &PcurveNurbs) -> bool {
    nurbs.knots().iter().copied().all(f64::is_finite)
        && knots_nondecreasing(nurbs.knots())
        && nurbs.control_points().iter().copied().all(finite_point2)
        && nurbs.weights().is_none_or(|weights| {
            weights
                .iter()
                .copied()
                .all(|weight| weight.is_finite() && weight > 0.0)
        })
}

/// Reverse a line or NURBS pcurve over an unchanged increasing parameter range.
pub(crate) fn reverse_pcurve_geometry(
    geometry: &PcurveGeometry,
    range: [f64; 2],
) -> Option<PcurveGeometry> {
    if !range.into_iter().all(f64::is_finite)
        || range[0] >= range[1]
        || !(range[1] - range[0]).is_finite()
    {
        return None;
    }
    match geometry {
        PcurveGeometry::Line { origin, direction } => {
            if !finite_point2(*origin) || !finite_point2(*direction) {
                return None;
            }
            let sum = range[0] + range[1];
            if !sum.is_finite() {
                return None;
            }
            let origin = Point2::new(origin.u + sum * direction.u, origin.v + sum * direction.v);
            finite_point2(origin).then_some(PcurveGeometry::Line {
                origin,
                direction: Point2::new(-direction.u, -direction.v),
            })
        }
        PcurveGeometry::Nurbs { nurbs } => {
            if !valid_pcurve_nurbs(nurbs) {
                return None;
            }
            let sum = range[0] + range[1];
            if !sum.is_finite() {
                return None;
            }
            let mut reversed_knots = nurbs
                .knots()
                .iter()
                .rev()
                .map(|knot| sum - knot)
                .collect::<Vec<_>>();
            for knot in &mut reversed_knots {
                if *knot == -0.0 {
                    *knot = 0.0;
                }
            }
            if reversed_knots.iter().copied().any(|knot| !knot.is_finite()) {
                return None;
            }
            Some(PcurveGeometry::Nurbs {
                nurbs: PcurveNurbs::new(
                    nurbs.degree(),
                    reversed_knots,
                    nurbs.control_points().iter().rev().copied().collect(),
                    nurbs
                        .weights()
                        .map(|weights| weights.iter().rev().copied().collect()),
                    nurbs.periodic(),
                )
                .ok()?,
            })
        }
        _ => None,
    }
}

/// Reverse a supported model-space curve over an increasing native range.
pub(crate) fn reverse_curve_geometry(
    geometry: &CurveGeometry,
    range: [f64; 2],
) -> Option<(CurveGeometry, [f64; 2])> {
    if !range.into_iter().all(f64::is_finite)
        || range[0] > range[1]
        || !(range[1] - range[0]).is_finite()
    {
        return None;
    }
    match geometry {
        CurveGeometry::Line { origin, direction } => {
            if !finite_point3(*origin)
                || ![direction.x, direction.y, direction.z]
                    .into_iter()
                    .all(f64::is_finite)
            {
                return None;
            }
            let length = range[1] - range[0];
            let origin = (*origin).translated(*direction, range[1]);
            let direction = direction.scale(-1.0);
            if !finite_point3(origin)
                || ![direction.x, direction.y, direction.z]
                    .into_iter()
                    .all(f64::is_finite)
            {
                return None;
            }
            Some((CurveGeometry::Line { origin, direction }, [0.0, length]))
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            if !finite_point3(*center)
                || ![
                    axis.x,
                    axis.y,
                    axis.z,
                    ref_direction.x,
                    ref_direction.y,
                    ref_direction.z,
                ]
                .into_iter()
                .all(f64::is_finite)
                || !radius.is_finite()
            {
                return None;
            }
            let sweep = range[1] - range[0];
            let tangent = (*axis).cross(*ref_direction);
            let end = range[1];
            let ref_direction = (*ref_direction).scale(end.cos()) + tangent.scale(end.sin());
            if ![ref_direction.x, ref_direction.y, ref_direction.z]
                .into_iter()
                .all(f64::is_finite)
            {
                return None;
            }
            Some((
                CurveGeometry::Circle {
                    center: *center,
                    axis: (*axis).scale(-1.0),
                    ref_direction,
                    radius: *radius,
                },
                [0.0, sweep],
            ))
        }
        CurveGeometry::Nurbs(nurbs) => {
            if !valid_nurbs_curve(nurbs) {
                return None;
            }
            let sum = range[0] + range[1];
            if !sum.is_finite() {
                return None;
            }
            let knots = nurbs
                .knots()
                .iter()
                .rev()
                .map(|knot| sum - knot)
                .collect::<Vec<_>>();
            if knots.iter().copied().any(|knot| !knot.is_finite()) {
                return None;
            }
            Some((
                CurveGeometry::Nurbs(
                    NurbsCurve::new(
                        nurbs.degree(),
                        knots,
                        nurbs.control_points().iter().rev().copied().collect(),
                        nurbs
                            .weights()
                            .map(|weights| weights.iter().rev().copied().collect()),
                        nurbs.periodic(),
                    )
                    .ok()?,
                ),
                range,
            ))
        }
        _ => None,
    }
}

/// Normalize the parameter interval for a model-space carrier.
pub(crate) fn canonical_model_curve_range(
    geometry: &CurveGeometry,
    range: [f64; 2],
) -> Option<[f64; 2]> {
    if !range.into_iter().all(f64::is_finite) || range[0] > range[1] {
        return None;
    }
    match geometry {
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => {
            canonical_periodic_range(range)
        }
        CurveGeometry::Nurbs(nurbs) => {
            let [lower, upper] = cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs)?;
            let tolerance = 1.0e-9_f64.max((upper - lower).abs() * EPS_NURBS_GEOMETRY);
            if nurbs.periodic() {
                (range[1] - range[0] <= upper - lower + tolerance).then_some(range)
            } else if range[0] >= lower && range[1] <= upper {
                Some(range)
            } else {
                ((range[0] - lower).abs().max((range[1] - upper).abs()) <= tolerance)
                    .then_some([lower, upper])
            }
        }
        _ => Some(range),
    }
}

/// Reverse a cone-helix construction over its complete angular domain.
///
/// The construction uses the angle itself as the curve parameter. Reversing
/// the interval therefore changes the radial frame, axial rise, and handedness
/// while preserving the same increasing parameter range. Other procedural
/// curve families require family-specific support-side mappings and are not
/// admitted here.
pub(crate) fn reverse_helix_definition(
    definition: &ProceduralCurveDefinition,
    range: [f64; 2],
) -> Option<(ProceduralCurveDefinition, [f64; 2])> {
    let ProceduralCurveDefinition::Helix {
        angle_range,
        center,
        major,
        minor,
        pitch,
        apex_factor,
        axis,
    } = definition
    else {
        return None;
    };
    if range != *angle_range
        || !range.into_iter().all(f64::is_finite)
        || range[0] >= range[1]
        || ![center.x, center.y, center.z]
            .into_iter()
            .chain(
                [major, minor, pitch, axis]
                    .into_iter()
                    .flat_map(|vector| [vector.x, vector.y, vector.z]),
            )
            .chain([*apex_factor])
            .all(f64::is_finite)
    {
        return None;
    }
    let revolutions = (range[1] - range[0]) / std::f64::consts::TAU;
    let radial_scale_at_end = 1.0 + *apex_factor * revolutions;
    if !revolutions.is_finite() || !radial_scale_at_end.is_finite() || radial_scale_at_end == 0.0 {
        return None;
    }
    let angle_sum = range[0] + range[1];
    if !angle_sum.is_finite() {
        return None;
    }
    let major_at_end = major.scale(angle_sum.cos()) + minor.scale(angle_sum.sin());
    let minor_at_end = major.scale(angle_sum.sin()) - minor.scale(angle_sum.cos());
    let major = major_at_end.scale(radial_scale_at_end);
    let minor = minor_at_end.scale(radial_scale_at_end);
    let center = center.translated(*pitch, revolutions);
    let pitch = pitch.scale(-1.0);
    let apex_factor = -*apex_factor / radial_scale_at_end;
    let axis = axis.scale(-1.0);
    if ![center.x, center.y, center.z, apex_factor]
        .into_iter()
        .chain(
            [major, minor, pitch, axis]
                .into_iter()
                .flat_map(|vector| [vector.x, vector.y, vector.z]),
        )
        .all(f64::is_finite)
    {
        return None;
    }
    Some((
        ProceduralCurveDefinition::Helix {
            angle_range: *angle_range,
            center,
            major,
            minor,
            pitch,
            apex_factor,
            axis,
        },
        range,
    ))
}

/// Normalize an increasing circular interval to the canonical one-turn domain.
pub(crate) fn canonical_periodic_range(range: [f64; 2]) -> Option<[f64; 2]> {
    let sweep = range[1] - range[0];
    if !sweep.is_finite() || sweep <= 0.0 || sweep > std::f64::consts::TAU + EPS_PERIODIC_SWEEP {
        return None;
    }
    let mut start = range[0].rem_euclid(std::f64::consts::TAU);
    if std::f64::consts::TAU - start <= EPS_PERIODIC_SWEEP {
        start = 0.0;
    }
    Some([start, start + sweep])
}

/// Angle-parameterized degree-1 cache for an exact circular helix.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CircularHelixCache {
    /// Piecewise-linear curve cache on the construction's angle interval.
    pub curve: NurbsCurve,
    /// Maximum radial sagitta deviation in model length units.
    pub fit_tolerance: f64,
}

/// Fit a circular helix with a bounded angle-parameterized polyline cache.
pub(crate) fn circular_helix_cache(
    construction: &ProceduralCurveDefinition,
    requested_tolerance: f64,
) -> Option<CircularHelixCache> {
    let ProceduralCurveDefinition::Helix {
        angle_range,
        center,
        major,
        minor,
        pitch,
        apex_factor,
        axis,
    } = construction
    else {
        return None;
    };
    let axis_norm = axis.x.hypot(axis.y).hypot(axis.z);
    let radius = major.x.hypot(major.y).hypot(major.z);
    let minor_radius = minor.x.hypot(minor.y).hypot(minor.z);
    let pitch_norm = pitch.x.hypot(pitch.y).hypot(pitch.z);
    let frame_finite = finite_point3(*center)
        && finite_vector3(*major)
        && finite_vector3(*minor)
        && finite_vector3(*pitch)
        && finite_vector3(*axis);
    let normalized_dot = |left: &Vector3, right: &Vector3| {
        (left.x / left.x.hypot(left.y).hypot(left.z))
            * (right.x / right.x.hypot(right.y).hypot(right.z))
            + (left.y / left.x.hypot(left.y).hypot(left.z))
                * (right.y / right.x.hypot(right.y).hypot(right.z))
            + (left.z / left.x.hypot(left.y).hypot(left.z))
                * (right.z / right.x.hypot(right.y).hypot(right.z))
    };
    if !requested_tolerance.is_finite()
        || requested_tolerance <= 0.0
        || !frame_finite
        || !radius.is_finite()
        || radius <= 0.0
        || !minor_radius.is_finite()
        || minor_radius <= 0.0
        || !axis_norm.is_finite()
        || (axis_norm - 1.0).abs() > EPS_HELIX_FRAME
        || !pitch_norm.is_finite()
        || (radius - minor_radius).abs() > EPS_HELIX_RADIUS * radius.max(minor_radius)
        || !angle_range.iter().copied().all(f64::is_finite)
        || angle_range[0] >= angle_range[1]
        || *apex_factor != 0.0
    {
        return None;
    }
    let normalized_dot_major_minor = normalized_dot(major, minor);
    let normalized_dot_major_axis = normalized_dot(major, axis);
    let normalized_dot_minor_axis = normalized_dot(minor, axis);
    let normalized_dot_pitch_axis = if pitch_norm == 0.0 {
        1.0
    } else {
        normalized_dot(pitch, axis)
    };
    if !normalized_dot_major_minor.is_finite()
        || normalized_dot_major_minor.abs() > EPS_HELIX_ORTHO
        || !normalized_dot_major_axis.is_finite()
        || normalized_dot_major_axis.abs() > EPS_HELIX_ORTHO
        || !normalized_dot_minor_axis.is_finite()
        || normalized_dot_minor_axis.abs() > EPS_HELIX_ORTHO
        || !normalized_dot_pitch_axis.is_finite()
        || normalized_dot_pitch_axis.abs() < 1.0 - EPS_HELIX_PITCH_ALIGNMENT
    {
        return None;
    }
    let sweep = angle_range[1] - angle_range[0];
    if !sweep.is_finite() || sweep <= 0.0 {
        return None;
    }
    let relative_tolerance = requested_tolerance / radius;
    let max_step = if relative_tolerance < EPS_RELATIVE_TOLERANCE {
        2.0 * (2.0 * relative_tolerance).sqrt()
    } else {
        2.0 * (1.0 - relative_tolerance).clamp(-1.0, 1.0).acos()
    };
    if !max_step.is_finite() || max_step <= 0.0 {
        return None;
    }
    let segment_count = (sweep / max_step).ceil().max(1.0);
    if !segment_count.is_finite() || segment_count > crate::MAX_EXACT_ARC_SPANS as f64 {
        return None;
    }
    let segment_count = segment_count as usize;
    let step = sweep / segment_count as f64;
    let samples = (0..=segment_count)
        .map(|index| {
            let parameter = if index == segment_count {
                angle_range[1]
            } else {
                angle_range[0] + index as f64 * step
            };
            if !parameter.is_finite() {
                return None;
            }
            Some((parameter, circular_helix_point(construction, parameter)?))
        })
        .collect::<Option<Vec<_>>>()?;
    if !samples
        .windows(2)
        .all(|pair| pair[0].0.is_finite() && pair[0].0 < pair[1].0)
    {
        return None;
    }
    let fit_tolerance = 2.0 * radius * (step * 0.25).sin().powi(2);
    let mut knots = Vec::with_capacity(samples.len() + 2);
    knots.push(angle_range[0]);
    knots.extend(samples.iter().map(|(parameter, _)| *parameter));
    knots.push(angle_range[1]);
    let curve = NurbsCurve::new(
        1,
        knots,
        samples.into_iter().map(|(_, point)| point).collect(),
        None,
        false,
    )
    .ok()?;
    if !fit_tolerance.is_finite()
        || !valid_nurbs_curve(&curve)
        || !knots_nondecreasing(curve.knots())
    {
        return None;
    }
    Some(CircularHelixCache {
        curve,
        fit_tolerance,
    })
}

fn circular_helix_point(construction: &ProceduralCurveDefinition, angle: f64) -> Option<Point3> {
    let ProceduralCurveDefinition::Helix {
        angle_range,
        center,
        major,
        minor,
        pitch,
        ..
    } = construction
    else {
        return None;
    };
    if !angle.is_finite()
        || !angle_range.iter().copied().all(f64::is_finite)
        || angle_range[0] >= angle_range[1]
    {
        return None;
    }
    let revolution_fraction = (angle - angle_range[0]) / std::f64::consts::TAU;
    if !revolution_fraction.is_finite() {
        return None;
    }
    let point = Point3::new(
        center.x + major.x * angle.cos() + minor.x * angle.sin() + pitch.x * revolution_fraction,
        center.y + major.y * angle.cos() + minor.y * angle.sin() + pitch.y * revolution_fraction,
        center.z + major.z * angle.cos() + minor.z * angle.sin() + pitch.z * revolution_fraction,
    );
    finite_point3(point).then_some(point)
}

/// Convert degree-5 position/first/second-derivative knot jets into an exact
/// piecewise Bézier B-spline control net.
pub(crate) fn quintic_jet_bspline(
    degree: u32,
    knots: &[f64],
    points: &[[f64; 2]],
    first: &[[f64; 2]],
    second: &[[f64; 2]],
) -> Option<(Vec<f64>, Vec<[f64; 2]>)> {
    quintic_jet_bspline_nd(degree, knots, points, first, second)
}

/// Convert a 3D degree-5 position/derivative jet to an exact B-spline.
pub(crate) fn quintic_jet_bspline3(
    degree: u32,
    knots: &[f64],
    points: &[[f64; 3]],
    first: &[[f64; 3]],
    second: &[[f64; 3]],
) -> Option<(Vec<f64>, Vec<[f64; 3]>)> {
    quintic_jet_bspline_nd(degree, knots, points, first, second)
}

fn quintic_jet_bspline_nd<const N: usize>(
    degree: u32,
    knots: &[f64],
    points: &[[f64; N]],
    first: &[[f64; N]],
    second: &[[f64; N]],
) -> Option<(Vec<f64>, Vec<[f64; N]>)> {
    if degree != 5
        || knots.len() < 2
        || points.len() != knots.len()
        || first.len() != knots.len()
        || second.len() != knots.len()
        || !knots.iter().copied().all(f64::is_finite)
        || !points.iter().flatten().copied().all(f64::is_finite)
        || !first.iter().flatten().copied().all(f64::is_finite)
        || !second.iter().flatten().copied().all(f64::is_finite)
    {
        return None;
    }
    let mut controls = Vec::with_capacity(6 * (knots.len() - 1));
    let mut full_knots = vec![knots[0]; 6];
    for index in 0..knots.len() - 1 {
        let h = knots[index + 1] - knots[index];
        if !h.is_finite() || h <= 0.0 {
            return None;
        }
        let p0 = points[index];
        let p1 = points[index + 1];
        let d0 = first[index];
        let d1 = first[index + 1];
        let dd0 = second[index];
        let dd1 = second[index + 1];
        controls.extend([
            p0,
            std::array::from_fn(|axis| p0[axis] + h * d0[axis] / 5.0),
            std::array::from_fn(|axis| {
                p0[axis] + 2.0 * h * d0[axis] / 5.0 + h * h * dd0[axis] / 20.0
            }),
            std::array::from_fn(|axis| {
                p1[axis] - 2.0 * h * d1[axis] / 5.0 + h * h * dd1[axis] / 20.0
            }),
            std::array::from_fn(|axis| p1[axis] - h * d1[axis] / 5.0),
            p1,
        ]);
        full_knots.extend([knots[index + 1]; 6]);
    }
    if !full_knots.iter().copied().all(f64::is_finite)
        || !controls.iter().flatten().copied().all(f64::is_finite)
    {
        return None;
    }
    Some((full_knots, controls))
}

/// Contract one parameter of a tensor-product NURBS surface into its exact
/// rational isocurve.
pub(crate) fn nurbs_surface_isocurve(
    surface: &NurbsSurface,
    parameter: f64,
    fix_u: bool,
) -> Option<NurbsCurve> {
    if !parameter.is_finite()
        || !surface.u_knots().iter().copied().all(f64::is_finite)
        || !surface.v_knots().iter().copied().all(f64::is_finite)
        || !surface.control_points().iter().copied().all(finite_point3)
        || surface.weights().is_some_and(|weights| {
            weights
                .iter()
                .copied()
                .any(|weight| !weight.is_finite() || weight == 0.0)
        })
    {
        return None;
    }
    let u_count = usize::try_from(surface.u_count()).ok()?;
    let v_count = usize::try_from(surface.v_count()).ok()?;
    let u_degree = usize::try_from(surface.u_degree()).ok()?;
    let v_degree = usize::try_from(surface.v_degree()).ok()?;
    if !knots_nondecreasing(surface.u_knots()) || !knots_nondecreasing(surface.v_knots()) {
        return None;
    }
    let (fixed_basis, varying_count, degree, knots) = if fix_u {
        (
            nurbs_basis_values(surface.u_knots(), u_degree, parameter, u_count)?,
            v_count,
            surface.v_degree(),
            surface.v_knots().to_vec(),
        )
    } else {
        (
            nurbs_basis_values(surface.v_knots(), v_degree, parameter, v_count)?,
            u_count,
            surface.u_degree(),
            surface.u_knots().to_vec(),
        )
    };
    let mut control_points = Vec::with_capacity(varying_count);
    let mut weights = Vec::with_capacity(varying_count);
    for varying in 0..varying_count {
        let mut numerator = [0.0; 3];
        let mut denominator = 0.0;
        for (fixed, basis) in fixed_basis.iter().copied().enumerate() {
            let index = if fix_u {
                fixed.checked_mul(v_count)?.checked_add(varying)?
            } else {
                varying.checked_mul(v_count)?.checked_add(fixed)?
            };
            let point = surface.control_points().get(index)?;
            let weight = match surface.weights() {
                Some(values) => *values.get(index)?,
                None => 1.0,
            };
            let factor = basis * weight;
            numerator[0] += factor * point.x;
            numerator[1] += factor * point.y;
            numerator[2] += factor * point.z;
            denominator += factor;
        }
        if !denominator.is_finite()
            || denominator == 0.0
            || !numerator.into_iter().all(f64::is_finite)
        {
            return None;
        }
        let point = Point3::new(
            numerator[0] / denominator,
            numerator[1] / denominator,
            numerator[2] / denominator,
        );
        if !finite_point3(point) {
            return None;
        }
        control_points.push(point);
        weights.push(denominator);
    }
    if !knots.iter().copied().all(f64::is_finite)
        || !control_points.iter().copied().all(finite_point3)
        || !weights.iter().copied().all(f64::is_finite)
    {
        return None;
    }
    NurbsCurve::new(
        degree,
        knots,
        control_points,
        surface.weights().is_some().then_some(weights),
        if fix_u {
            surface.v_periodic()
        } else {
            surface.u_periodic()
        },
    )
    .ok()
}

fn nurbs_basis_values(
    knots: &[f64],
    degree: usize,
    parameter: f64,
    count: usize,
) -> Option<Vec<f64>> {
    if knots.len() != count.checked_add(degree)?.checked_add(1)? || count == 0 {
        return None;
    }
    if !parameter.is_finite()
        || !knots.iter().copied().all(f64::is_finite)
        || !knots_nondecreasing(knots)
    {
        return None;
    }
    let mut basis = alloc_filled(count + degree, 0.0, "catia NURBS basis values").ok()?;
    for (index, value) in basis.iter_mut().enumerate() {
        if (knots.get(index)? <= &parameter && &parameter < knots.get(index + 1)?)
            || (parameter == *knots.last()? && index + 1 == count)
        {
            *value = 1.0;
        }
    }
    for level in 1..=degree {
        for index in 0..count + degree - level {
            let left_denominator = knots[index + level] - knots[index];
            let right_denominator = knots[index + level + 1] - knots[index + 1];
            let left = if left_denominator == 0.0 {
                0.0
            } else {
                (parameter - knots[index]) / left_denominator * basis[index]
            };
            let right = if right_denominator == 0.0 {
                0.0
            } else {
                (knots[index + level + 1] - parameter) / right_denominator * basis[index + 1]
            };
            basis[index] = left + right;
        }
    }
    basis.truncate(count);
    basis.iter().all(|value| value.is_finite()).then_some(basis)
}

pub(crate) fn expand_knots(distinct: &[f64], multiplicities: &[u32]) -> Option<Vec<f64>> {
    let capacity = multiplicities
        .iter()
        .try_fold(0usize, |sum, value| sum.checked_add(*value as usize))?;
    let mut knots = Vec::with_capacity(capacity);
    for (&knot, &multiplicity) in distinct.iter().zip(multiplicities) {
        knots.extend(std::iter::repeat_n(knot, multiplicity as usize));
    }
    Some(knots)
}

pub(crate) fn pole_count(multiplicities: &[u32], degree: u32) -> Option<u32> {
    multiplicities
        .iter()
        .try_fold(0u32, |sum, value| sum.checked_add(*value))?
        .checked_sub(degree + 1)
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::eval::{curve_point, pcurve_uv};
    use cadmpeg_ir::geometry::{
        CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, ProceduralCurveDefinition,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};

    use super::*;

    #[test]
    fn canonical_nurbs_range_clamps_rounding_at_the_domain_boundary() {
        let geometry = CurveGeometry::Nurbs(
            NurbsCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        );

        assert_eq!(
            canonical_model_curve_range(&geometry, [-1.0e-12, 1.0 + 1.0e-12]),
            Some([0.0, 1.0])
        );
        assert_eq!(canonical_model_curve_range(&geometry, [-1.0e-4, 1.0]), None);
    }

    #[test]
    fn reversed_surface_pcurve_preserves_domain_and_swaps_endpoints() {
        let geometry = PcurveGeometry::Line {
            origin: Point2::new(2.0, -1.0),
            direction: Point2::new(3.0, 4.0),
        };
        let range = [5.0, 9.0];
        let reversed = reverse_pcurve_geometry(&geometry, range).expect("reversible line");
        for (parameter, source_parameter) in [(5.0, 9.0), (9.0, 5.0)] {
            let actual = pcurve_uv(&reversed, parameter).expect("reversed evaluation");
            let expected = pcurve_uv(&geometry, source_parameter).expect("source evaluation");
            assert!((actual.u - expected.u).abs() < 1.0e-12);
            assert!((actual.v - expected.v).abs() < 1.0e-12);
        }
    }

    #[test]
    fn reversed_model_carriers_preserve_endpoint_geometry() {
        let line = CurveGeometry::Line {
            origin: Point3::new(2.0, -1.0, 4.0),
            direction: Vector3::new(3.0, 4.0, -2.0),
        };
        let circle = CurveGeometry::Circle {
            center: Point3::new(2.0, -1.0, 4.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 3.0,
        };
        for (geometry, range) in [(line, [5.0, 9.0]), (circle, [0.25, 2.0])] {
            let (reversed, reversed_range) =
                reverse_curve_geometry(&geometry, range).expect("reversible model curve");
            for (parameter, source_parameter) in
                [(reversed_range[0], range[1]), (reversed_range[1], range[0])]
            {
                let actual = curve_point(&reversed, parameter).expect("reversed endpoint");
                let expected = curve_point(&geometry, source_parameter).expect("source endpoint");
                assert!(actual.distance(expected) < 1.0e-12);
            }
        }
    }

    #[test]
    fn reversed_nurbs_preserves_active_subrange() {
        let geometry = CurveGeometry::Nurbs(
            NurbsCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        );
        let range = [0.2, 0.8];
        let (reversed, reversed_range) =
            reverse_curve_geometry(&geometry, range).expect("reversible NURBS");
        for parameter in [range[0], 0.5, range[1]] {
            let actual = curve_point(&reversed, parameter).expect("reversed NURBS point");
            let expected = curve_point(&geometry, range[0] + range[1] - parameter)
                .expect("source NURBS point");
            assert!(actual.distance(expected) < 1.0e-12);
        }
        assert_eq!(reversed_range, range);
    }

    #[test]
    fn reversed_helix_preserves_conical_path() {
        let range = [0.25, 2.0];
        let definition = ProceduralCurveDefinition::Helix {
            angle_range: range,
            center: Point3::new(1.0, -2.0, 3.0),
            major: Vector3::new(2.0, 0.0, 0.0),
            minor: Vector3::new(0.0, 2.0, 0.0),
            pitch: Vector3::new(0.0, 0.0, 3.0),
            apex_factor: 0.4,
            axis: Vector3::new(0.0, 0.0, 1.0),
        };
        let (reversed, reversed_range) =
            reverse_helix_definition(&definition, range).expect("reversible helix");
        let evaluate = |definition: &ProceduralCurveDefinition, angle: f64| {
            let ProceduralCurveDefinition::Helix {
                angle_range,
                center,
                major,
                minor,
                pitch,
                apex_factor,
                ..
            } = definition
            else {
                panic!("helix definition")
            };
            let fraction = (angle - angle_range[0]) / std::f64::consts::TAU;
            let scale = 1.0 + apex_factor * fraction;
            center
                .translated(*major, scale * angle.cos())
                .translated(*minor, scale * angle.sin())
                .translated(*pitch, fraction)
        };
        for angle in [range[0], 0.75, range[1]] {
            let actual = evaluate(&reversed, angle);
            let expected = evaluate(&definition, range[0] + range[1] - angle);
            assert!(actual.distance(expected) < 1.0e-12);
        }
        assert_eq!(reversed_range, range);
    }

    #[test]
    fn surface_isocurve_preserves_tiny_weights_and_knot_domain() {
        let tiny = 1e-200;
        let surface = NurbsSurface::new(
            1,
            1,
            vec![0.0, 0.0, tiny, tiny],
            vec![0.0, 0.0, 1.0, 1.0],
            2,
            2,
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(2.0, 1.0, 0.0),
            ],
            Some(vec![tiny; 4]),
            false,
            false,
            false,
        )
        .unwrap();
        let curve = nurbs_surface_isocurve(&surface, tiny * 0.5, true)
            .expect("tiny rational surface isocurve");
        assert_eq!(
            curve.control_points(),
            [Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)]
        );
        assert_eq!(curve.weights(), Some([tiny, tiny].as_slice()));
    }

    #[test]
    fn surface_isocurve_rejects_invalid_weight_shape_and_output() {
        let surface = |control_points, weights| {
            NurbsSurface::new(
                1,
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
                2,
                2,
                control_points,
                weights,
                false,
                false,
                false,
            )
            .unwrap()
        };
        assert!(nurbs_surface_isocurve(
            &surface(
                vec![Point3::new(f64::MAX, 0.0, 0.0); 4],
                Some(vec![1.0e200; 4]),
            ),
            0.5,
            true,
        )
        .is_none());
        assert!(nurbs_surface_isocurve(
            &surface(vec![Point3::new(0.0, 0.0, 0.0); 4], Some(vec![0.0; 4])),
            0.5,
            true,
        )
        .is_none());
    }

    #[test]
    fn circular_helix_cache_preserves_exact_interval_endpoints() {
        let range = [0.125, 1.570_797_917_999_999_6];
        let definition = ProceduralCurveDefinition::Helix {
            angle_range: range,
            center: Point3::new(0.0, 0.0, 0.0),
            major: Vector3::new(1.0, 0.0, 0.0),
            minor: Vector3::new(0.0, 1.0, 0.0),
            pitch: Vector3::new(0.0, 0.0, 1.0),
            apex_factor: 0.0,
            axis: Vector3::new(0.0, 0.0, 1.0),
        };

        let cache = circular_helix_cache(&definition, 1.0e-4).expect("valid helix");
        assert_eq!(cache.curve.knots()[1], range[0]);
        assert_eq!(cache.curve.knots()[cache.curve.knots().len() - 2], range[1]);
        assert!(cache.fit_tolerance.is_finite());
        assert!(cache
            .curve
            .control_points()
            .iter()
            .copied()
            .all(super::finite_point3));
    }

    #[test]
    fn circular_helix_frame_validation_is_scale_independent() {
        let radius = 1e-200;
        let definition = |minor| ProceduralCurveDefinition::Helix {
            angle_range: [0.0, 1.0],
            center: Point3::new(0.0, 0.0, 0.0),
            major: Vector3::new(radius, 0.0, 0.0),
            minor,
            pitch: Vector3::new(0.0, 0.0, 1.0),
            apex_factor: 0.0,
            axis: Vector3::new(0.0, 0.0, 1.0),
        };

        assert!(
            circular_helix_cache(&definition(Vector3::new(0.0, radius, 0.0)), 1.0e-4).is_some()
        );
        assert!(
            circular_helix_cache(&definition(Vector3::new(0.0, 2.0 * radius, 0.0)), 1.0e-4)
                .is_none()
        );
        assert!(
            circular_helix_cache(&definition(Vector3::new(radius, 0.0, 0.0)), 1.0e-4).is_none()
        );
    }

    #[test]
    fn circular_helix_cache_rejects_invalid_frame_and_output() {
        let definition = ProceduralCurveDefinition::Helix {
            angle_range: [0.0, 1.0],
            center: Point3::new(0.0, 0.0, 0.0),
            major: Vector3::new(1.0, 0.0, 0.0),
            minor: Vector3::new(0.0, 1.0, 0.0),
            pitch: Vector3::new(0.0, 0.0, 1.0),
            apex_factor: 0.0,
            axis: Vector3::new(0.0, 0.0, 1.0),
        };
        let mut non_axial_pitch = definition.clone();
        if let ProceduralCurveDefinition::Helix { pitch, .. } = &mut non_axial_pitch {
            *pitch = Vector3::new(1.0, 0.0, 0.0);
        }
        assert!(circular_helix_cache(&non_axial_pitch, 1.0e-4).is_none());

        let overflowing_fit = ProceduralCurveDefinition::Helix {
            angle_range: [0.0, 1.0],
            center: Point3::new(0.0, 0.0, 0.0),
            major: Vector3::new(f64::MAX, 0.0, 0.0),
            minor: Vector3::new(0.0, f64::MAX, 0.0),
            pitch: Vector3::new(0.0, 0.0, 0.0),
            apex_factor: 0.0,
            axis: Vector3::new(0.0, 0.0, 1.0),
        };
        assert!(circular_helix_cache(&overflowing_fit, f64::MAX).is_none());
    }

    #[test]
    fn quintic_jet_rejects_nonfinite_control_net() {
        assert!(quintic_jet_bspline(
            5,
            &[0.0, 10.0],
            &[[0.0, 0.0], [1.0, 0.0]],
            &[[f64::MAX, 0.0], [f64::MAX, 0.0]],
            &[[0.0, 0.0], [0.0, 0.0]],
        )
        .is_none());
        assert!(quintic_jet_bspline(
            5,
            &[0.0, 1.0],
            &[[f64::NAN, 0.0], [1.0, 0.0]],
            &[[1.0, 0.0], [1.0, 0.0]],
            &[[0.0, 0.0], [0.0, 0.0]],
        )
        .is_none());
    }

    #[test]
    fn reversing_geometry_rejects_nonfinite_reconstruction() {
        let pcurve_line = PcurveGeometry::Line {
            origin: Point2::new(0.0, 0.0),
            direction: Point2::new(1.0, 0.0),
        };
        assert!(reverse_pcurve_geometry(&pcurve_line, [f64::MAX / 2.0, f64::MAX]).is_none());

        let model_line = CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(2.0, 0.0, 0.0),
        };
        assert!(reverse_curve_geometry(&model_line, [0.0, f64::MAX]).is_none());

        let pcurve_nurbs = PcurveGeometry::Nurbs {
            nurbs: cadmpeg_ir::geometry::PcurveNurbs::new(
                1,
                vec![-f64::MAX, 0.0, 1.0, 1.0],
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        };
        assert!(reverse_pcurve_geometry(&pcurve_nurbs, [0.0, f64::MAX]).is_none());

        let nonfinite_line = PcurveGeometry::Line {
            origin: Point2::new(f64::NAN, 0.0),
            direction: Point2::new(1.0, 0.0),
        };
        assert!(reverse_pcurve_geometry(&nonfinite_line, [0.0, 1.0]).is_none());

        let zero_weight_curve = CurveGeometry::Nurbs(
            NurbsCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                Some(vec![1.0, 0.0]),
                false,
            )
            .unwrap(),
        );
        assert!(reverse_curve_geometry(&zero_weight_curve, [0.0, 1.0]).is_none());
    }
}
