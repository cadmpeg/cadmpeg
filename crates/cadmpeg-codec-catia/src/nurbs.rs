//! Shared NURBS, B-spline, and analytic-curve math utilities.
//!
//! Family-agnostic geometry math consumed across decode families and the
//! decode/transfer paths: knot expansion and pole counting, degree-5 jet to
//! B-spline conversion, tensor-product NURBS isocurve extraction, circular
//! interval canonicalization, and exact circular-helix fitting.

use cadmpeg_ir::geometry::{NurbsCurve, NurbsSurface, ProceduralCurveDefinition};
use cadmpeg_ir::math::Point3;

/// Normalize an increasing circular interval to the canonical one-turn domain.
pub(crate) fn canonical_periodic_range(range: [f64; 2]) -> Option<[f64; 2]> {
    let sweep = range[1] - range[0];
    if !sweep.is_finite() || sweep <= 0.0 || sweep > std::f64::consts::TAU + 1e-9 {
        return None;
    }
    let mut start = range[0].rem_euclid(std::f64::consts::TAU);
    if std::f64::consts::TAU - start <= 1e-9 {
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
        ..
    } = construction
    else {
        return None;
    };
    let radius = major.norm();
    let frame_finite = [center.x, center.y, center.z]
        .into_iter()
        .chain(
            [major, minor, pitch]
                .into_iter()
                .flat_map(|vector| [vector.x, vector.y, vector.z]),
        )
        .all(f64::is_finite);
    let major_minor_dot = major.x * minor.x + major.y * minor.y + major.z * minor.z;
    if !requested_tolerance.is_finite()
        || requested_tolerance <= 0.0
        || !frame_finite
        || !radius.is_finite()
        || radius <= f64::EPSILON
        || (radius - minor.norm()).abs() > 1e-9 * (1.0 + radius)
        || major_minor_dot.abs() > 1e-9 * (1.0 + radius * radius)
        || *apex_factor != 0.0
    {
        return None;
    }
    let sweep = angle_range[1] - angle_range[0];
    if !sweep.is_finite() || sweep <= 0.0 {
        return None;
    }
    let relative_tolerance = requested_tolerance / radius;
    let max_step = if relative_tolerance < 1e-6 {
        2.0 * (2.0 * relative_tolerance).sqrt()
    } else {
        2.0 * (1.0 - relative_tolerance).clamp(-1.0, 1.0).acos()
    };
    let segment_count = (sweep / max_step).ceil().max(1.0);
    if !segment_count.is_finite() || segment_count > crate::MAX_EXACT_ARC_SPANS as f64 {
        return None;
    }
    let segment_count = segment_count as usize;
    let step = sweep / segment_count as f64;
    let samples = (0..=segment_count)
        .map(|index| {
            let parameter = angle_range[0] + index as f64 * step;
            Some((parameter, circular_helix_point(construction, parameter)?))
        })
        .collect::<Option<Vec<_>>>()?;
    let fit_tolerance = 2.0 * radius * (step * 0.25).sin().powi(2);
    let mut knots = Vec::with_capacity(samples.len() + 2);
    knots.push(angle_range[0]);
    knots.extend(samples.iter().map(|(parameter, _)| *parameter));
    knots.push(angle_range[1]);
    Some(CircularHelixCache {
        curve: NurbsCurve {
            degree: 1,
            knots,
            control_points: samples.into_iter().map(|(_, point)| point).collect(),
            weights: None,
            periodic: false,
        },
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
    let revolution_fraction = (angle - angle_range[0]) / std::f64::consts::TAU;
    Some(Point3::new(
        center.x + major.x * angle.cos() + minor.x * angle.sin() + pitch.x * revolution_fraction,
        center.y + major.y * angle.cos() + minor.y * angle.sin() + pitch.y * revolution_fraction,
        center.z + major.z * angle.cos() + minor.z * angle.sin() + pitch.z * revolution_fraction,
    ))
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
    Some((full_knots, controls))
}

/// Contract one parameter of a tensor-product NURBS surface into its exact
/// rational isocurve.
pub(crate) fn nurbs_surface_isocurve(
    surface: &NurbsSurface,
    parameter: f64,
    fix_u: bool,
) -> Option<NurbsCurve> {
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    let (fixed_basis, varying_count, degree, knots) = if fix_u {
        (
            nurbs_basis_values(
                &surface.u_knots,
                usize::try_from(surface.u_degree).ok()?,
                parameter,
                u_count,
            )?,
            v_count,
            surface.v_degree,
            surface.v_knots.clone(),
        )
    } else {
        (
            nurbs_basis_values(
                &surface.v_knots,
                usize::try_from(surface.v_degree).ok()?,
                parameter,
                v_count,
            )?,
            u_count,
            surface.u_degree,
            surface.u_knots.clone(),
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
            let point = surface.control_points.get(index)?;
            let weight = surface
                .weights
                .as_ref()
                .and_then(|values| values.get(index))
                .copied()
                .unwrap_or(1.0);
            let factor = basis * weight;
            numerator[0] += factor * point.x;
            numerator[1] += factor * point.y;
            numerator[2] += factor * point.z;
            denominator += factor;
        }
        if !denominator.is_finite() || denominator.abs() <= f64::EPSILON {
            return None;
        }
        control_points.push(Point3::new(
            numerator[0] / denominator,
            numerator[1] / denominator,
            numerator[2] / denominator,
        ));
        weights.push(denominator);
    }
    Some(NurbsCurve {
        degree,
        knots,
        control_points,
        weights: surface.weights.is_some().then_some(weights),
        periodic: if fix_u {
            surface.v_periodic
        } else {
            surface.u_periodic
        },
    })
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
    let mut basis = vec![0.0; count + degree];
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
            let left = if left_denominator.abs() <= f64::EPSILON {
                0.0
            } else {
                (parameter - knots[index]) / left_denominator * basis[index]
            };
            let right = if right_denominator.abs() <= f64::EPSILON {
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
