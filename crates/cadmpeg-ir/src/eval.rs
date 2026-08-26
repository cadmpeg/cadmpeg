// SPDX-License-Identifier: Apache-2.0
//! Point evaluation of geometry carriers.
//!
//! Evaluators map carrier parameters to model-space (or parameter-space)
//! points using the carriers' own parameterizations: conic parameters are
//! angles from the reference/major direction, line parameters are signed
//! distances along the unit direction, and B-splines evaluate by Cox–de Boor
//! over their stored knot vectors. [`model_surface_point`] resolves construction-
//! backed carriers that require other model entities. Carriers without a typed
//! parameterization ([`CurveGeometry::Unknown`], [`CurveGeometry::Composite`],
//! [`SurfaceGeometry::Unknown`], parabolas, and hyperbolas) evaluate to `None`.
//! [`model_curve_point_by_id`] resolves construction-backed curves whose
//! parameterization is established by model entities.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::geometry::{
    knots_nondecreasing, CurveGeometry, LawExpression, LawFormula, NurbsCurve, NurbsSurface,
    PcurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
    SurfaceParameterAxis, SweepSurfaceLayout,
};
use crate::math::{Point2, Point3, Vector3};
use crate::transform::Transform;
use crate::CadIr;
use cadmpeg_core::decode::alloc_filled;

const EPS_EVAL_SPATIAL_POINTS_ARE_REFLECTIONS_E12: f64 = 1.0e-12;
const EPS_EVAL_SPATIAL_POINTS_ARE_REFLECTIONS_E9: f64 = 1.0e-9;
const EPS_EVAL_REFINE_NURBS_SURFACE_PARAMETERS_E12: f64 = 1.0e-12;
const EPS_EVAL_CLAMPED_NURBS_PCURVE_ENDPOINT_FRAMES_E12: f64 = 1.0e-12;
const EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E12: f64 = 1.0e-12;
const EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9: f64 = 1.0e-9;
const EPS_EVAL_MODEL_CURVE_PARAMETER_NEAR_POINT_WITH_TOLERANCE_E12: f64 = 1.0e-12;
const EPS_EVAL_SWEEP_PROFILE_FRAME_ALIGNMENT_E9: f64 = 1.0e-9;

/// Test whether two model-space points are reflections across a line carrier.
///
/// The line is unbounded for the reflection operation but its two stored
/// endpoints must define a finite, nondegenerate direction.
pub fn spatial_points_are_reflections(
    first: Point3,
    second: Point3,
    axis_start: Point3,
    axis_end: Point3,
) -> bool {
    let axis = Vector3::new(
        axis_end.x - axis_start.x,
        axis_end.y - axis_start.y,
        axis_end.z - axis_start.z,
    );
    let axis_length = axis.norm();
    if !axis_length.is_finite() || axis_length <= EPS_EVAL_SPATIAL_POINTS_ARE_REFLECTIONS_E12 {
        return false;
    }
    let midpoint = Point3::new(
        0.5 * (first.x + second.x),
        0.5 * (first.y + second.y),
        0.5 * (first.z + second.z),
    );
    let from_axis = Vector3::new(
        midpoint.x - axis_start.x,
        midpoint.y - axis_start.y,
        midpoint.z - axis_start.z,
    );
    let separation = Vector3::new(second.x - first.x, second.y - first.y, second.z - first.z);
    let scale = 1.0
        + axis_length
            .max(from_axis.norm())
            .max(separation.norm())
            .max(first.x.abs())
            .max(first.y.abs())
            .max(first.z.abs())
            .max(second.x.abs())
            .max(second.y.abs())
            .max(second.z.abs());
    axis.cross(from_axis).norm() <= EPS_EVAL_SPATIAL_POINTS_ARE_REFLECTIONS_E9 * axis_length * scale
        && axis.dot(separation).abs()
            <= EPS_EVAL_SPATIAL_POINTS_ARE_REFLECTIONS_E9 * axis_length * scale
}

/// Recover native parameters for an analytic surface point.
pub fn analytic_surface_parameters(geometry: &SurfaceGeometry, point: Point3) -> Option<Point2> {
    let components = |origin: Point3, axis: Vector3, reference: Vector3| {
        let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
        let transverse = axis.cross(reference);
        (
            delta.x * reference.x + delta.y * reference.y + delta.z * reference.z,
            delta.x * transverse.x + delta.y * transverse.y + delta.z * transverse.z,
            delta.x * axis.x + delta.y * axis.y + delta.z * axis.z,
        )
    };
    let result = match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let (u, v, _) = components(*origin, *normal, *u_axis);
            Point2::new(u, v)
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            if *radius == 0.0 {
                return None;
            }
            let (x, y, v) = components(*origin, *axis, *ref_direction);
            Point2::new((y / radius).atan2(x / radius), v)
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => {
            let (x, y, v) = components(*origin, *axis, *ref_direction);
            let local_radius = radius + v * half_angle.tan();
            if local_radius == 0.0 || *ratio == 0.0 {
                return None;
            }
            Point2::new((y / (local_radius * ratio)).atan2(x / local_radius), v)
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            if *radius == 0.0 {
                return None;
            }
            let (x, y, z) = components(*center, *axis, *ref_direction);
            Point2::new(y.atan2(x), z.atan2(x.hypot(y)))
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            if *minor_radius == 0.0 {
                return None;
            }
            let (x, y, z) = components(*center, *axis, *ref_direction);
            Point2::new(
                y.atan2(x),
                (z / minor_radius).atan2((x.hypot(y) - major_radius) / minor_radius),
            )
        }
        _ => return None,
    };
    (result.u.is_finite() && result.v.is_finite()).then_some(result)
}

#[derive(Clone)]
struct RationalBezierSurfacePatch {
    u_domain: [f64; 2],
    v_domain: [f64; 2],
    u_degree: usize,
    v_degree: usize,
    controls: Vec<[f64; 4]>,
}

struct SurfacePatchQueueEntry {
    lower_bound: f64,
    diameter: f64,
    sequence: usize,
    patch: RationalBezierSurfacePatch,
}

impl PartialEq for SurfacePatchQueueEntry {
    fn eq(&self, other: &Self) -> bool {
        self.sequence == other.sequence
    }
}

impl Eq for SurfacePatchQueueEntry {}

impl PartialOrd for SurfacePatchQueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SurfacePatchQueueEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // BinaryHeap is a max-heap. Reverse the lower-bound order so the patch
        // with the strongest minimum-distance promise is examined first.
        other
            .lower_bound
            .total_cmp(&self.lower_bound)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

#[derive(Clone)]
struct HomogeneousBezierSpan {
    domain: [f64; 2],
    controls: Vec<[f64; 4]>,
}

type HomogeneousBezierSplit = (Vec<[f64; 4]>, Vec<[f64; 4]>);

fn insert_homogeneous_knot(
    degree: usize,
    knots: &mut Vec<f64>,
    controls: &mut Vec<[f64; 4]>,
    knot: f64,
) -> Option<()> {
    let count = controls.len();
    let span = knots
        .windows(2)
        .position(|pair| pair[0] <= knot && knot < pair[1])?;
    let multiplicity = knots.iter().filter(|candidate| **candidate == knot).count();
    if multiplicity >= degree {
        return Some(());
    }
    let mut inserted = alloc_filled(
        count.checked_add(1)?,
        [0.0; 4],
        "IR homogeneous knot insertion",
    )
    .ok()?;
    inserted[..=span - degree].copy_from_slice(&controls[..=span - degree]);
    inserted[span - multiplicity + 1..].copy_from_slice(&controls[span - multiplicity..]);
    for index in span - degree + 1..=span - multiplicity {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return None;
        }
        let alpha = (knot - knots[index]) / denominator;
        inserted[index] = std::array::from_fn(|axis| {
            alpha * controls[index][axis] + (1.0 - alpha) * controls[index - 1][axis]
        });
    }
    knots.insert(span + 1, knot);
    *controls = inserted;
    Some(())
}

fn homogeneous_bezier_spans(
    degree: usize,
    knots: &[f64],
    mut controls: Vec<[f64; 4]>,
) -> Option<Vec<HomogeneousBezierSpan>> {
    if degree == 0 {
        let mut spans = Vec::new();
        for (index, window) in knots.windows(2).enumerate() {
            if window[0] < window[1] {
                spans.push(HomogeneousBezierSpan {
                    domain: [window[0], window[1]],
                    controls: vec![*controls.get(index)?],
                });
            }
        }
        return (!spans.is_empty()).then_some(spans);
    }

    let mut knots = knots.to_vec();
    let domain = [*knots.get(degree)?, *knots.get(controls.len())?];
    let mut internal = knots[degree + 1..controls.len()]
        .iter()
        .copied()
        .filter(|knot| domain[0] < *knot && *knot < domain[1])
        .collect::<Vec<_>>();
    internal.sort_by(f64::total_cmp);
    internal.dedup();
    for knot in internal {
        while knots.iter().filter(|candidate| **candidate == knot).count() < degree {
            insert_homogeneous_knot(degree, &mut knots, &mut controls, knot)?;
        }
    }
    let mut boundaries = knots[degree..=controls.len()].to_vec();
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup();
    let spans = boundaries
        .windows(2)
        .enumerate()
        .filter_map(|(index, domain)| {
            (domain[0] < domain[1]).then(|| {
                let start = index.checked_mul(degree)?;
                Some(HomogeneousBezierSpan {
                    domain: [domain[0], domain[1]],
                    controls: controls.get(start..=start + degree)?.to_vec(),
                })
            })?
        })
        .collect::<Vec<_>>();
    (!spans.is_empty()).then_some(spans)
}

fn rational_surface_patches(surface: &NurbsSurface) -> Option<Vec<RationalBezierSurfacePatch>> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    let control_count = u_count.checked_mul(v_count)?;
    if u_degree >= u_count
        || v_degree >= v_count
        || surface.control_points.len() != control_count
        || surface.u_knots.len() != u_count.checked_add(u_degree)?.checked_add(1)?
        || surface.v_knots.len() != v_count.checked_add(v_degree)?.checked_add(1)?
        || surface
            .u_knots
            .iter()
            .chain(&surface.v_knots)
            .any(|knot| !knot.is_finite())
        || !knots_nondecreasing(&surface.u_knots)
        || !knots_nondecreasing(&surface.v_knots)
        || surface.control_points.iter().any(|control| {
            !control.x.is_finite() || !control.y.is_finite() || !control.z.is_finite()
        })
    {
        return None;
    }
    let weights = match &surface.weights {
        Some(weights)
            if weights.len() == control_count
                && weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0) =>
        {
            weights.clone()
        }
        Some(_) => return None,
        None => alloc_filled(control_count, 1.0, "ir_nurbs_surface_weights").ok()?,
    };
    let homogeneous_controls = surface
        .control_points
        .iter()
        .zip(weights)
        .map(|(control, weight)| {
            [
                weight * control.x,
                weight * control.y,
                weight * control.z,
                weight,
            ]
        })
        .collect::<Vec<_>>();
    if homogeneous_controls
        .iter()
        .flatten()
        .any(|value| !value.is_finite())
    {
        return None;
    }
    let u_spans_by_v = (0..v_count)
        .map(|v| {
            homogeneous_bezier_spans(
                u_degree,
                &surface.u_knots,
                (0..u_count)
                    .map(|u| homogeneous_controls[u * v_count + v])
                    .collect(),
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let u_span_count = u_spans_by_v.first()?.len();
    if u_span_count == 0 || u_spans_by_v.iter().any(|spans| spans.len() != u_span_count) {
        return None;
    }
    let mut patches = Vec::new();
    for u_span in 0..u_span_count {
        let u_domain = u_spans_by_v[0][u_span].domain;
        if u_spans_by_v
            .iter()
            .any(|spans| spans[u_span].domain != u_domain)
        {
            return None;
        }
        let v_spans_by_u = (0..=u_degree)
            .map(|u_control| {
                homogeneous_bezier_spans(
                    v_degree,
                    &surface.v_knots,
                    (0..v_count)
                        .map(|v| u_spans_by_v[v][u_span].controls[u_control])
                        .collect(),
                )
            })
            .collect::<Option<Vec<_>>>()?;
        let v_span_count = v_spans_by_u.first()?.len();
        if v_span_count == 0 || v_spans_by_u.iter().any(|spans| spans.len() != v_span_count) {
            return None;
        }
        for v_span in 0..v_span_count {
            let v_domain = v_spans_by_u[0][v_span].domain;
            if v_spans_by_u
                .iter()
                .any(|spans| spans[v_span].domain != v_domain)
            {
                return None;
            }
            patches.push(RationalBezierSurfacePatch {
                u_domain,
                v_domain,
                u_degree,
                v_degree,
                controls: (0..=u_degree)
                    .flat_map(|u| v_spans_by_u[u][v_span].controls.iter().copied())
                    .collect(),
            });
        }
    }
    (!patches.is_empty()).then_some(patches)
}

fn rational_surface_residual_patches(
    surface: &NurbsSurface,
    point: Point3,
) -> Option<Vec<RationalBezierSurfacePatch>> {
    if !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite() {
        return None;
    }
    let mut patches = rational_surface_patches(surface)?;
    for patch in &mut patches {
        for control in &mut patch.controls {
            for (axis, coordinate) in [point.x, point.y, point.z].into_iter().enumerate() {
                control[axis] -= control[3] * coordinate;
            }
        }
    }
    Some(patches)
}

fn split_homogeneous_bezier(
    controls: &[[f64; 4]],
    parameter: f64,
) -> Option<HomogeneousBezierSplit> {
    if controls.is_empty() || !parameter.is_finite() || !(0.0..=1.0).contains(&parameter) {
        return None;
    }
    let mut levels = vec![controls.to_vec()];
    while levels.last()?.len() > 1 {
        levels.push(
            levels
                .last()?
                .windows(2)
                .map(|pair| {
                    std::array::from_fn(|axis| {
                        (1.0 - parameter) * pair[0][axis] + parameter * pair[1][axis]
                    })
                })
                .collect(),
        );
    }
    let left = levels.iter().map(|level| level[0]).collect();
    let right = levels
        .iter()
        .rev()
        .map(|level| *level.last().expect("nonempty de Casteljau level"))
        .collect();
    Some((left, right))
}

fn restrict_homogeneous_bezier(
    controls: &[[f64; 4]],
    start: f64,
    end: f64,
) -> Option<Vec<[f64; 4]>> {
    if start > end {
        let mut restricted = restrict_homogeneous_bezier(controls, end, start)?;
        restricted.reverse();
        return Some(restricted);
    }
    if start == end {
        let point = split_homogeneous_bezier(controls, start)?.0.pop()?;
        return alloc_filled(controls.len(), point, "ir_bezier_collapsed_controls").ok();
    }
    let left = split_homogeneous_bezier(controls, end)?.0;
    if start == 0.0 {
        return Some(left);
    }
    let relative_start = start / end;
    split_homogeneous_bezier(&left, relative_start).map(|(_, right)| right)
}

fn binomial_coefficient(degree: usize, index: usize) -> f64 {
    let index = index.min(degree - index);
    (1..=index).fold(1.0, |value, factor| {
        value * (degree - index + factor) as f64 / factor as f64
    })
}

fn rational_patch_parameter_segment(
    patch: &RationalBezierSurfacePatch,
    start: Point2,
    end: Point2,
) -> Option<Vec<[f64; 4]>> {
    let normalize = |value: f64, domain: [f64; 2]| {
        let parameter = (value - domain[0]) / (domain[1] - domain[0]);
        parameter.is_finite().then(|| parameter.clamp(0.0, 1.0))
    };
    let u_range = [
        normalize(start.u, patch.u_domain)?,
        normalize(end.u, patch.u_domain)?,
    ];
    let v_range = [
        normalize(start.v, patch.v_domain)?,
        normalize(end.v, patch.v_domain)?,
    ];
    let u_lines = (0..=patch.v_degree)
        .map(|v| {
            restrict_homogeneous_bezier(
                &(0..=patch.u_degree)
                    .map(|u| patch.controls[u * (patch.v_degree + 1) + v])
                    .collect::<Vec<_>>(),
                u_range[0],
                u_range[1],
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let restricted = (0..=patch.u_degree)
        .map(|u| {
            restrict_homogeneous_bezier(
                &(0..=patch.v_degree)
                    .map(|v| u_lines[v][u])
                    .collect::<Vec<_>>(),
                v_range[0],
                v_range[1],
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let degree = patch.u_degree + patch.v_degree;
    let mut diagonal = alloc_filled(
        degree.checked_add(1)?,
        [0.0; 4],
        "IR rational surface diagonal",
    )
    .ok()?;
    for (u, row) in restricted.iter().enumerate() {
        for (v, control) in row.iter().enumerate() {
            let index = u + v;
            let factor = binomial_coefficient(patch.u_degree, u)
                * binomial_coefficient(patch.v_degree, v)
                / binomial_coefficient(degree, index);
            for axis in 0..4 {
                diagonal[index][axis] += factor * control[axis];
            }
        }
    }
    diagonal
        .iter()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(diagonal)
}

fn point_on_chord(chord: [Point3; 2], parameter: f64) -> Point3 {
    Point3::new(
        chord[0].x + parameter * (chord[1].x - chord[0].x),
        chord[0].y + parameter * (chord[1].y - chord[0].y),
        chord[0].z + parameter * (chord[1].z - chord[0].z),
    )
}

fn rational_curve_chord_bound(controls: &[[f64; 4]], chord: [Point3; 2]) -> Option<f64> {
    let degree = controls.len().checked_sub(1)?;
    let elevated_degree = degree + 1;
    let mut bound = 0.0_f64;
    let mut coordinate_scale = chord
        .iter()
        .flat_map(|point| [point.x, point.y, point.z])
        .fold(1.0_f64, |scale, coordinate| scale.max(coordinate.abs()));
    for index in 0..=elevated_degree {
        let previous = index.checked_sub(1).and_then(|index| controls.get(index));
        let current = controls.get(index);
        let previous_factor = index as f64 / elevated_degree as f64;
        let current_factor = 1.0 - previous_factor;
        let weight = previous_factor * previous.map_or(0.0, |control| control[3])
            + current_factor * current.map_or(0.0, |control| control[3]);
        if !weight.is_finite() || weight <= 0.0 {
            return None;
        }
        let mut squared_residual = 0.0;
        for (axis, chord_coordinates) in [
            [chord[0].x, chord[1].x],
            [chord[0].y, chord[1].y],
            [chord[0].z, chord[1].z],
        ]
        .into_iter()
        .enumerate()
        {
            let coordinate = previous_factor * previous.map_or(0.0, |control| control[axis])
                + current_factor * current.map_or(0.0, |control| control[axis]);
            let weighted_chord =
                current_factor * current.map_or(0.0, |control| control[3]) * chord_coordinates[0]
                    + previous_factor
                        * previous.map_or(0.0, |control| control[3])
                        * chord_coordinates[1];
            let residual = (coordinate - weighted_chord) / weight;
            if !residual.is_finite() {
                return None;
            }
            squared_residual += residual * residual;
            coordinate_scale = coordinate_scale.max((coordinate / weight).abs());
        }
        bound = bound.max(squared_residual.sqrt());
    }
    let rounding_margin = 256.0 * f64::EPSILON * coordinate_scale.max(bound);
    (bound.is_finite() && rounding_margin.is_finite()).then_some(bound + rounding_margin)
}

/// Conservatively bound the separation between a NURBS surface image of a
/// linear parameter segment and a model-space chord with the same parameter.
///
/// The segment is split at every surface knot. Each rational Bézier piece is
/// restricted exactly to the parameter line, and its positive-weight residual
/// control hull bounds the complete piece rather than selected samples.
pub fn nurbs_surface_parameter_segment_chord_bound(
    surface: &NurbsSurface,
    parameters: [Point2; 2],
    chord: [Point3; 2],
) -> Option<f64> {
    if parameters
        .iter()
        .any(|point| !point.u.is_finite() || !point.v.is_finite())
        || chord
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
    {
        return None;
    }
    let patches = rational_surface_patches(surface)?;
    let mut splits = vec![0.0, 1.0];
    for patch in &patches {
        for (boundary, start, end) in [
            (patch.u_domain[0], parameters[0].u, parameters[1].u),
            (patch.u_domain[1], parameters[0].u, parameters[1].u),
            (patch.v_domain[0], parameters[0].v, parameters[1].v),
            (patch.v_domain[1], parameters[0].v, parameters[1].v),
        ] {
            if start != end {
                let parameter = (boundary - start) / (end - start);
                if parameter.is_finite() && 0.0 < parameter && parameter < 1.0 {
                    splits.push(parameter);
                }
            }
        }
    }
    splits.sort_by(f64::total_cmp);
    splits.dedup();
    splits.windows(2).try_fold(0.0_f64, |bound, range| {
        let middle = 0.5 * (range[0] + range[1]);
        let parameter_point = |parameter: f64| {
            Point2::new(
                parameters[0].u + parameter * (parameters[1].u - parameters[0].u),
                parameters[0].v + parameter * (parameters[1].v - parameters[0].v),
            )
        };
        let midpoint = parameter_point(middle);
        let patch = patches.iter().find(|patch| {
            patch.u_domain[0] <= midpoint.u
                && midpoint.u <= patch.u_domain[1]
                && patch.v_domain[0] <= midpoint.v
                && midpoint.v <= patch.v_domain[1]
        })?;
        let controls = rational_patch_parameter_segment(
            patch,
            parameter_point(range[0]),
            parameter_point(range[1]),
        )?;
        let piece_bound = rational_curve_chord_bound(
            &controls,
            [
                point_on_chord(chord, range[0]),
                point_on_chord(chord, range[1]),
            ],
        )?;
        Some(bound.max(piece_bound))
    })
}

fn rational_patch_distance_bounds(patch: &RationalBezierSurfacePatch) -> Option<(f64, f64)> {
    let mut minimum = [f64::INFINITY; 3];
    let mut maximum = [f64::NEG_INFINITY; 3];
    for control in &patch.controls {
        if !control[3].is_finite() || control[3] <= 0.0 {
            return None;
        }
        for axis in 0..3 {
            let coordinate = control[axis] / control[3];
            if !coordinate.is_finite() {
                return None;
            }
            minimum[axis] = minimum[axis].min(coordinate);
            maximum[axis] = maximum[axis].max(coordinate);
        }
    }
    let lower = (0..3)
        .map(|axis| {
            if minimum[axis] > 0.0 {
                minimum[axis] * minimum[axis]
            } else if maximum[axis] < 0.0 {
                maximum[axis] * maximum[axis]
            } else {
                0.0
            }
        })
        .sum::<f64>();
    let diameter = (0..3)
        .map(|axis| (maximum[axis] - minimum[axis]).powi(2))
        .sum::<f64>();
    (lower.is_finite() && diameter.is_finite()).then_some((lower, diameter))
}

fn split_rational_surface_patch(
    patch: &RationalBezierSurfacePatch,
    split_u: bool,
) -> Option<[RationalBezierSurfacePatch; 2]> {
    let (degree, line_count) = if split_u {
        (patch.u_degree, patch.v_degree + 1)
    } else {
        (patch.v_degree, patch.u_degree + 1)
    };
    let mut first_lines = Vec::with_capacity(line_count);
    let mut second_lines = Vec::with_capacity(line_count);
    for line in 0..line_count {
        let controls = if split_u {
            (0..=degree)
                .map(|index| patch.controls[index * (patch.v_degree + 1) + line])
                .collect::<Vec<_>>()
        } else {
            patch.controls[line * (patch.v_degree + 1)..(line + 1) * (patch.v_degree + 1)].to_vec()
        };
        let mut levels = vec![controls];
        while levels.last()?.len() > 1 {
            levels.push(
                levels
                    .last()?
                    .windows(2)
                    .map(|pair| std::array::from_fn(|axis| 0.5 * (pair[0][axis] + pair[1][axis])))
                    .collect(),
            );
        }
        first_lines.push(levels.iter().map(|level| level[0]).collect::<Vec<_>>());
        second_lines.push(
            levels
                .iter()
                .rev()
                .map(|level| *level.last().expect("nonempty de Casteljau level"))
                .collect::<Vec<_>>(),
        );
    }
    let assemble = |lines: Vec<Vec<[f64; 4]>>| {
        if split_u {
            (0..=patch.u_degree)
                .flat_map(|u| {
                    (0..=patch.v_degree).map({
                        let lines = &lines;
                        move |v| lines[v][u]
                    })
                })
                .collect()
        } else {
            lines.into_iter().flatten().collect()
        }
    };
    let u_middle = patch.u_domain[0] + (patch.u_domain[1] - patch.u_domain[0]) * 0.5;
    let v_middle = patch.v_domain[0] + (patch.v_domain[1] - patch.v_domain[0]) * 0.5;
    let (first_u, second_u, first_v, second_v) = if split_u {
        (
            [patch.u_domain[0], u_middle],
            [u_middle, patch.u_domain[1]],
            patch.v_domain,
            patch.v_domain,
        )
    } else {
        (
            patch.u_domain,
            patch.u_domain,
            [patch.v_domain[0], v_middle],
            [v_middle, patch.v_domain[1]],
        )
    };
    if split_u && (u_middle == patch.u_domain[0] || u_middle == patch.u_domain[1])
        || !split_u && (v_middle == patch.v_domain[0] || v_middle == patch.v_domain[1])
    {
        return None;
    }
    Some([
        RationalBezierSurfacePatch {
            u_domain: first_u,
            v_domain: first_v,
            u_degree: patch.u_degree,
            v_degree: patch.v_degree,
            controls: assemble(first_lines),
        },
        RationalBezierSurfacePatch {
            u_domain: second_u,
            v_domain: second_v,
            u_degree: patch.u_degree,
            v_degree: patch.v_degree,
            controls: assemble(second_lines),
        },
    ])
}

fn refine_nurbs_surface_parameters(
    surface: &NurbsSurface,
    point: Point3,
    mut parameters: Point2,
    u_domain: [f64; 2],
    v_domain: [f64; 2],
) -> Option<Point2> {
    let squared_distance = |position: Point3| {
        (position.x - point.x).powi(2)
            + (position.y - point.y).powi(2)
            + (position.z - point.z).powi(2)
    };
    parameters.u = parameters.u.clamp(u_domain[0], u_domain[1]);
    parameters.v = parameters.v.clamp(v_domain[0], v_domain[1]);
    for _ in 0..32 {
        let position = nurbs_surface_point(surface, parameters.u, parameters.v)?;
        let residual = Vector3::new(
            position.x - point.x,
            position.y - point.y,
            position.z - point.z,
        );
        let partials = nurbs_surface_partials(surface, parameters.u, parameters.v)?;
        let (du, dv) = (partials.du, partials.dv);
        let du_squared = du.dot(du);
        let mixed = du.dot(dv);
        let dv_squared = dv.dot(dv);
        let determinant = du_squared * dv_squared - mixed * mixed;
        if !determinant.is_finite()
            || determinant.abs() <= f64::EPSILON * du_squared.max(dv_squared).powi(2)
        {
            break;
        }
        let du_residual = du.dot(residual);
        let dv_residual = dv.dot(residual);
        let step = Point2::new(
            (dv_squared * du_residual - mixed * dv_residual) / determinant,
            (du_squared * dv_residual - mixed * du_residual) / determinant,
        );
        let current_distance = squared_distance(position);
        let mut scale = 1.0;
        let mut accepted = None;
        for _ in 0..16 {
            let candidate = Point2::new(
                (parameters.u - scale * step.u).clamp(u_domain[0], u_domain[1]),
                (parameters.v - scale * step.v).clamp(v_domain[0], v_domain[1]),
            );
            let candidate_position = nurbs_surface_point(surface, candidate.u, candidate.v)?;
            if squared_distance(candidate_position) <= current_distance {
                accepted = Some(candidate);
                break;
            }
            scale *= 0.5;
        }
        let Some(candidate) = accepted else {
            break;
        };
        parameters = candidate;
        if scale * step.u.abs()
            <= EPS_EVAL_REFINE_NURBS_SURFACE_PARAMETERS_E12 * (1.0 + parameters.u.abs())
            && scale * step.v.abs()
                <= EPS_EVAL_REFINE_NURBS_SURFACE_PARAMETERS_E12 * (1.0 + parameters.v.abs())
        {
            break;
        }
    }
    Some(parameters)
}

fn complete_nurbs_surface_starts(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
) -> Option<Vec<Point2>> {
    const MAX_PATCHES: usize = 1_000_000;

    let patches = rational_surface_residual_patches(surface, point)?;
    let coordinate_scale =
        patches
            .iter()
            .flat_map(|patch| &patch.controls)
            .try_fold(1.0_f64, |scale, control| {
                let weight = control[3];
                if !weight.is_finite() || weight <= 0.0 {
                    return None;
                }
                control[..3].iter().try_fold(scale, |scale, coordinate| {
                    let coordinate = (coordinate / weight).abs();
                    coordinate.is_finite().then(|| scale.max(coordinate))
                })
            })?;
    let requested_tolerance = match fit_tolerance {
        Some(tolerance) if tolerance.is_finite() && tolerance >= 0.0 => tolerance,
        Some(_) => return None,
        None => 0.0,
    };
    let distance_tolerance = requested_tolerance.max(256.0 * f64::EPSILON * coordinate_scale);
    let squared_tolerance = distance_tolerance * distance_tolerance;
    let squared_distance = |parameters: Point2| {
        let position = nurbs_surface_point(surface, parameters.u, parameters.v)?;
        let distance = (position.x - point.x)
            .hypot(position.y - point.y)
            .hypot(position.z - point.z);
        distance.is_finite().then_some(distance * distance)
    };
    let center = |patch: &RationalBezierSurfacePatch| {
        Point2::new(
            patch.u_domain[0] + (patch.u_domain[1] - patch.u_domain[0]) * 0.5,
            patch.v_domain[0] + (patch.v_domain[1] - patch.v_domain[0]) * 0.5,
        )
    };
    let surface_u_domain = [
        *surface
            .u_knots
            .get(usize::try_from(surface.u_degree).ok()?)?,
        *surface
            .u_knots
            .get(usize::try_from(surface.u_count).ok()?)?,
    ];
    let surface_v_domain = [
        *surface
            .v_knots
            .get(usize::try_from(surface.v_degree).ok()?)?,
        *surface
            .v_knots
            .get(usize::try_from(surface.v_count).ok()?)?,
    ];
    let refined_upper = |start, u_domain, v_domain| {
        let parameters = refine_nurbs_surface_parameters(surface, point, start, u_domain, v_domain)
            .unwrap_or(start);
        Some((parameters, squared_distance(parameters)?))
    };
    let mut best_distance = f64::INFINITY;
    let mut best_upper_parameters = Vec::new();
    {
        let mut consider_upper = |(parameters, distance): (Point2, f64)| {
            if !best_distance.is_finite() {
                best_distance = distance;
                best_upper_parameters.push(parameters);
                return;
            }
            let tolerance = 128.0
                * f64::EPSILON
                * distance
                    .abs()
                    .max(best_distance.abs())
                    .max(squared_tolerance);
            if distance < best_distance && best_distance - distance > tolerance {
                best_distance = distance;
                best_upper_parameters.clear();
            }
            if (distance - best_distance).abs() <= tolerance {
                best_upper_parameters.push(parameters);
            }
        };
        if let Some(candidate) =
            seed.and_then(|seed| refined_upper(seed, surface_u_domain, surface_v_domain))
        {
            consider_upper(candidate);
        }
        for patch in &patches {
            consider_upper(refined_upper(
                center(patch),
                patch.u_domain,
                patch.v_domain,
            )?);
        }
    }
    best_distance.is_finite().then_some(())?;
    // A tolerance-bounded inverse needs a constructive fitting parameter, not
    // a proof of the global minimum. Every upper candidate is surface-evaluated.
    if fit_tolerance.is_some() && best_distance <= squared_tolerance {
        return (!best_upper_parameters.is_empty()).then_some(best_upper_parameters);
    }
    let mut queue = BinaryHeap::new();
    let mut sequence = 0usize;
    for patch in patches {
        let (lower_bound, diameter) = rational_patch_distance_bounds(&patch)?;
        queue.push(SurfacePatchQueueEntry {
            lower_bound,
            diameter,
            sequence,
            patch,
        });
        sequence += 1;
    }
    let mut terminal = Vec::<(Point2, f64)>::new();
    let mut examined = 0usize;
    while let Some(entry) = queue.pop() {
        examined += 1;
        if examined > MAX_PATCHES {
            return None;
        }
        let SurfacePatchQueueEntry {
            lower_bound,
            diameter,
            patch,
            ..
        } = entry;
        let comparison_tolerance = 128.0
            * f64::EPSILON
            * lower_bound
                .abs()
                .max(best_distance.abs())
                .max(squared_tolerance);
        if lower_bound > best_distance + comparison_tolerance {
            break;
        }
        let parameters = center(&patch);
        let (upper_parameters, center_distance) =
            refined_upper(parameters, patch.u_domain, patch.v_domain)?;
        if fit_tolerance.is_some() && center_distance <= squared_tolerance {
            return Some(vec![upper_parameters]);
        }
        let upper_tolerance = 128.0
            * f64::EPSILON
            * center_distance
                .abs()
                .max(best_distance.abs())
                .max(squared_tolerance);
        if center_distance < best_distance && best_distance - center_distance > upper_tolerance {
            best_distance = center_distance;
            best_upper_parameters.clear();
        }
        if (center_distance - best_distance).abs() <= upper_tolerance {
            best_upper_parameters.push(upper_parameters);
        }
        let indivisible = parameters.u == patch.u_domain[0]
            || parameters.u == patch.u_domain[1]
            || parameters.v == patch.v_domain[0]
            || parameters.v == patch.v_domain[1];
        if diameter <= squared_tolerance
            || center_distance - lower_bound <= squared_tolerance
            || indivisible
        {
            terminal.push((upper_parameters, lower_bound));
            continue;
        }
        let control = |u: usize, v: usize| {
            let homogeneous = patch.controls[u * (patch.v_degree + 1) + v];
            [
                homogeneous[0] / homogeneous[3],
                homogeneous[1] / homogeneous[3],
                homogeneous[2] / homogeneous[3],
            ]
        };
        let u_variation = (0..patch.u_degree)
            .flat_map(|u| (0..=patch.v_degree).map(move |v| (u, v)))
            .map(|(u, v)| {
                let first = control(u, v);
                let second = control(u + 1, v);
                (0..3)
                    .map(|axis| (second[axis] - first[axis]).powi(2))
                    .sum::<f64>()
            })
            .fold(0.0_f64, f64::max);
        let v_variation = (0..=patch.u_degree)
            .flat_map(|u| (0..patch.v_degree).map(move |v| (u, v)))
            .map(|(u, v)| {
                let first = control(u, v);
                let second = control(u, v + 1);
                (0..3)
                    .map(|axis| (second[axis] - first[axis]).powi(2))
                    .sum::<f64>()
            })
            .fold(0.0_f64, f64::max);
        let children = split_rational_surface_patch(&patch, u_variation >= v_variation)?;
        for patch in children {
            let (lower_bound, diameter) = rational_patch_distance_bounds(&patch)?;
            queue.push(SurfacePatchQueueEntry {
                lower_bound,
                diameter,
                sequence,
                patch,
            });
            sequence += 1;
        }
    }
    let final_tolerance = 128.0 * f64::EPSILON * best_distance.abs().max(squared_tolerance);
    let mut starts = terminal
        .into_iter()
        .filter_map(|(parameters, lower)| {
            (lower <= best_distance + final_tolerance).then_some(parameters)
        })
        .collect::<Vec<_>>();
    starts.extend(best_upper_parameters);
    (!starts.is_empty()).then_some(starts)
}

fn solve_nurbs_surface_parameter(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
) -> Option<(Point2, f64)> {
    let seed = seed.filter(|seed| seed.u.is_finite() && seed.v.is_finite());
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    let u_domain = [
        *surface.u_knots.get(u_degree)?,
        *surface.u_knots.get(u_count)?,
    ];
    let v_domain = [
        *surface.v_knots.get(v_degree)?,
        *surface.v_knots.get(v_count)?,
    ];
    if u_domain[0] >= u_domain[1] || v_domain[0] >= v_domain[1] {
        return None;
    }
    let starts = complete_nurbs_surface_starts(surface, point, seed, fit_tolerance)?;
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    let mut best_seed_distance = f64::INFINITY;
    for start in starts {
        let Some(parameters) =
            refine_nurbs_surface_parameters(surface, point, start, u_domain, v_domain)
        else {
            continue;
        };
        let Some(position) = nurbs_surface_point(surface, parameters.u, parameters.v) else {
            continue;
        };
        let distance = (position.x - point.x)
            .hypot(position.y - point.y)
            .hypot(position.z - point.z);
        let seed_distance = seed.map_or(parameters.u.abs() + parameters.v.abs(), |seed| {
            (parameters.u - seed.u).hypot(parameters.v - seed.v)
        });
        let same_point = (distance - best_distance).abs()
            <= f64::EPSILON * 64.0 * distance.abs().max(best_distance.abs()).max(1.0);
        if distance < best_distance && !same_point
            || same_point && seed_distance < best_seed_distance
        {
            best = Some(parameters);
            best_distance = distance;
            best_seed_distance = seed_distance;
        }
    }
    best.map(|parameters| (parameters, best_distance))
}

/// Find a globally closest parameter pair on a finite NURBS surface.
///
/// When equivalent closest pairs exist, `seed` selects the nearest parameter
/// branch. Without a seed, the pair nearest the parameter-space origin wins.
pub fn nurbs_surface_closest_parameter(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
) -> Option<Point2> {
    solve_nurbs_surface_parameter(surface, point, seed, None).map(|(parameters, _)| parameters)
}

/// Find a NURBS surface parameter pair whose image is within `tolerance` of
/// `point`. The result is forward-evaluated before it is returned.
///
/// When multiple fitting pairs exist, `seed` selects the nearest parameter
/// branch. Without a seed, the pair nearest the parameter-space origin wins.
pub fn nurbs_surface_parameter_within_tolerance(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    tolerance: f64,
) -> Option<Point2> {
    if !tolerance.is_finite() || tolerance < 0.0 {
        return None;
    }
    let (parameters, distance) =
        solve_nurbs_surface_parameter(surface, point, seed, Some(tolerance))?;
    (distance.is_finite() && distance <= tolerance).then_some(parameters)
}

/// `base + Σ factorᵢ · directionᵢ` in model space.
fn offset(base: Point3, terms: &[(f64, Vector3)]) -> Point3 {
    let mut out = base;
    for (factor, direction) in terms {
        out.x += factor * direction.x;
        out.y += factor * direction.y;
        out.z += factor * direction.z;
    }
    out
}

/// Knot span index of `t` for a clamped B-spline basis, or `None` when the
/// knot vector cannot support `count` poles of the given degree.
fn bspline_span(knots: &[f64], degree: usize, count: usize, t: f64) -> Option<usize> {
    if knots.len() < count + degree + 1 || count <= degree {
        return None;
    }
    if t >= knots[count] {
        return Some(count - 1);
    }
    if t <= knots[degree] {
        return Some(degree);
    }
    let mut lo = degree;
    let mut hi = count;
    while lo < hi {
        let mid = usize::midpoint(lo, hi);
        if t < knots[mid] {
            hi = mid;
        } else if t >= knots[mid + 1] {
            lo = mid + 1;
        } else {
            return Some(mid);
        }
    }
    Some(lo)
}

/// Non-zero basis function values at `t` for the given span (Cox–de Boor).
fn bspline_basis(knots: &[f64], degree: usize, span: usize, t: f64) -> Option<Vec<f64>> {
    let mut values = vec![1.0];
    let mut left = alloc_filled(degree.checked_add(1)?, 0.0, "IR B-spline basis left").ok()?;
    let mut right = alloc_filled(degree.checked_add(1)?, 0.0, "IR B-spline basis right").ok()?;
    for j in 1..=degree {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        let mut next = alloc_filled(j.checked_add(1)?, 0.0, "IR B-spline basis level").ok()?;
        for (r, &value) in values.iter().enumerate().take(j) {
            let denominator = right[r + 1] + left[j - r];
            let factor = if denominator == 0.0 {
                0.0
            } else {
                value / denominator
            };
            next[r] = saved + right[r + 1] * factor;
            saved = left[j - r] * factor;
        }
        next[j] = saved;
        values = next;
    }
    Some(values)
}

fn bspline_basis_derivative(knots: &[f64], degree: usize, span: usize, t: f64) -> Option<Vec<f64>> {
    if degree == 0 {
        return Some(vec![0.0]);
    }
    let lower = bspline_basis(knots, degree - 1, span, t)?;
    let lower_start = span - (degree - 1);
    (0..=degree)
        .map(|local| {
            let index = span - degree + local;
            let lower_at = |global: usize| {
                global
                    .checked_sub(lower_start)
                    .and_then(|at| lower.get(at))
                    .copied()
                    .unwrap_or(0.0)
            };
            let left_denominator = knots[index + degree] - knots[index];
            let right_denominator = knots[index + degree + 1] - knots[index + 1];
            let left = if left_denominator == 0.0 {
                0.0
            } else {
                degree as f64 * lower_at(index) / left_denominator
            };
            let right = if right_denominator == 0.0 {
                0.0
            } else {
                degree as f64 * lower_at(index + 1) / right_denominator
            };
            left - right
        })
        .collect::<Vec<_>>()
        .into()
}

fn bspline_basis_second_derivative(
    knots: &[f64],
    degree: usize,
    span: usize,
    t: f64,
) -> Option<Vec<f64>> {
    if degree < 2 {
        return alloc_filled(
            degree.checked_add(1)?,
            0.0,
            "IR B-spline second-derivative basis",
        )
        .ok();
    }
    let lower = bspline_basis_derivative(knots, degree - 1, span, t)?;
    let lower_start = span - (degree - 1);
    (0..=degree)
        .map(|local| {
            let index = span - degree + local;
            let lower_at = |global: usize| {
                global
                    .checked_sub(lower_start)
                    .and_then(|at| lower.get(at))
                    .copied()
                    .unwrap_or(0.0)
            };
            let left_denominator = knots[index + degree] - knots[index];
            let right_denominator = knots[index + degree + 1] - knots[index + 1];
            let left = if left_denominator == 0.0 {
                0.0
            } else {
                degree as f64 * lower_at(index) / left_denominator
            };
            let right = if right_denominator == 0.0 {
                0.0
            } else {
                degree as f64 * lower_at(index + 1) / right_denominator
            };
            left - right
        })
        .collect::<Vec<_>>()
        .into()
}

/// Evaluate a possibly-rational B-spline curve over 3D poles.
pub fn nurbs_curve_point(
    degree: u32,
    knots: &[f64],
    control_points: &[Point3],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<Point3> {
    let degree = usize::try_from(degree).ok()?;
    let span = bspline_span(knots, degree, control_points.len(), t)?;
    let basis = bspline_basis(knots, degree, span, t)?;
    let mut x = 0.0;
    let mut y = 0.0;
    let mut z = 0.0;
    let mut weight_sum = 0.0;
    for (i, value) in basis.iter().enumerate() {
        let index = span - degree + i;
        let weight = weights
            .and_then(|weights| weights.get(index).copied())
            .unwrap_or(1.0);
        let pole = control_points.get(index)?;
        x += value * weight * pole.x;
        y += value * weight * pole.y;
        z += value * weight * pole.z;
        weight_sum += value * weight;
    }
    (weight_sum != 0.0).then(|| Point3::new(x / weight_sum, y / weight_sum, z / weight_sum))
}

/// Effective knot domain of a structurally evaluable NURBS curve.
pub fn nurbs_curve_parameter_domain(curve: &NurbsCurve) -> Option<[f64; 2]> {
    nurbs_pcurve_parameter_domain(curve.degree, &curve.knots, curve.control_points.len())
}

/// Effective knot domain shared by model-space and parameter-space NURBS
/// carriers. The full knot vector contains multiplicity and extrapolation
/// knots; only the interval between the degree-th knot and the control-pole
/// count-th knot is evaluable.
pub fn nurbs_pcurve_parameter_domain(
    degree: u32,
    knots: &[f64],
    control_point_count: usize,
) -> Option<[f64; 2]> {
    let degree = usize::try_from(degree).ok()?;
    if control_point_count <= degree
        || knots.len() < control_point_count.checked_add(degree)?.checked_add(1)?
    {
        return None;
    }
    let lower = *knots.get(degree)?;
    let upper = *knots.get(control_point_count)?;
    (lower.is_finite() && upper.is_finite() && lower < upper).then_some([lower, upper])
}

const NURBS_SEARCH_MAX_INTERVALS: usize = 512;
const MODEL_CURVE_PARAMETER_SEARCH_MAX_NEWTON_ITERATIONS: usize = 12;

#[derive(Clone, Copy)]
struct NurbsSearchWindow<'a> {
    domain: [f64; 2],
    boundaries: &'a [f64],
}

/// Find a parameter witness whose NURBS curve point lies within `tolerance` of
/// `point`, searching finite knot spans in proximity to `seed`.
///
/// Interval rejection uses a rational-curve speed bound, so skipped intervals
/// cannot contain an admissible witness. The returned parameter is always
/// forward-evaluated within `tolerance`; `None` also covers malformed input or
/// exhaustion of the bounded certified search.
pub fn nurbs_curve_parameter_near_point(
    curve: &NurbsCurve,
    point: Point3,
    tolerance: f64,
    seed: f64,
) -> Option<f64> {
    let degree = usize::try_from(curve.degree).ok()?;
    let count = curve.control_points.len();
    let domain = nurbs_curve_parameter_domain(curve)?;
    if degree == 0
        || !tolerance.is_finite()
        || tolerance < 0.0
        || !seed.is_finite()
        || !point.x.is_finite()
        || !point.y.is_finite()
        || !point.z.is_finite()
    {
        return None;
    }
    let weights = validated_nurbs_curve_weights(curve)?;
    let speed_bound = nurbs_curve_speed_bound_about(curve, weights.as_ref(), point)?;
    let distance = |parameter| {
        let position = nurbs_curve_point(
            curve.degree,
            &curve.knots,
            &curve.control_points,
            Some(weights.as_ref()),
            parameter,
        )?;
        Some(
            ((position.x - point.x).powi(2)
                + (position.y - point.y).powi(2)
                + (position.z - point.z).powi(2))
            .sqrt(),
        )
    };
    let seed = seed.clamp(domain[0], domain[1]);
    let boundaries = &curve.knots[degree..=count];
    match nearest_boundary_witness(boundaries, seed, tolerance, distance) {
        BoundaryWitness::Found(parameter) => return Some(parameter),
        BoundaryWitness::Invalid => return None,
        BoundaryWitness::NoMatch => {}
    }
    if let Some(parameter) = nurbs_curve_parameter_near_point_newton(
        curve,
        weights.as_ref(),
        point,
        tolerance,
        seed,
        NurbsSearchWindow { domain, boundaries },
    ) {
        return Some(parameter);
    }
    let mut intervals = bounded_nearest_intervals(boundaries, seed);
    let mut examined = 0usize;
    while let Some([start, end]) = intervals.pop() {
        examined += 1;
        if examined > NURBS_SEARCH_MAX_INTERVALS {
            return None;
        }
        let middle = start + (end - start) * 0.5;
        let middle_distance = distance(middle)?;
        if middle_distance <= tolerance {
            return Some(middle);
        }
        if middle_distance - speed_bound * (end - start) * 0.5 > tolerance
            || middle == start
            || middle == end
        {
            continue;
        }
        let halves = [[start, middle], [middle, end]];
        let nearer = usize::from(
            interval_distance_to_parameter(halves[1], seed)
                < interval_distance_to_parameter(halves[0], seed),
        );
        intervals.push(halves[1 - nearer]);
        intervals.push(halves[nearer]);
    }
    None
}

fn nurbs_curve_parameter_near_point_newton(
    curve: &NurbsCurve,
    weights: &[f64],
    point: Point3,
    tolerance: f64,
    seed: f64,
    search: NurbsSearchWindow<'_>,
) -> Option<f64> {
    let [lower, upper] =
        parameter_interval_containing(search.boundaries, seed).unwrap_or(search.domain);
    let mut parameter = seed.clamp(lower, upper);
    for _ in 0..MODEL_CURVE_PARAMETER_SEARCH_MAX_NEWTON_ITERATIONS {
        let position = nurbs_curve_point(
            curve.degree,
            &curve.knots,
            &curve.control_points,
            Some(weights),
            parameter,
        )?;
        let residual = Vector3::new(
            position.x - point.x,
            position.y - point.y,
            position.z - point.z,
        );
        if residual.norm() <= tolerance {
            return Some(parameter);
        }
        let tangent = nurbs_curve_tangent(
            curve.degree,
            &curve.knots,
            &curve.control_points,
            Some(weights),
            parameter,
        )?;
        let denominator = tangent.dot(tangent);
        if !denominator.is_finite() || denominator <= 0.0 {
            return None;
        }
        let next = parameter - residual.dot(tangent) / denominator;
        if !next.is_finite() {
            return None;
        }
        let next = next.clamp(lower, upper);
        if next == parameter {
            return None;
        }
        parameter = next;
    }
    None
}

/// Global model-space speed bound for a structurally valid rational NURBS
/// curve over its effective knot domain.
pub fn nurbs_curve_speed_bound(curve: &NurbsCurve) -> Option<f64> {
    let weights = validated_nurbs_curve_weights(curve)?;
    nurbs_curve_speed_bound_about(curve, weights.as_ref(), Point3::new(0.0, 0.0, 0.0))
}

fn validated_nurbs_curve_weights(curve: &NurbsCurve) -> Option<Cow<'_, [f64]>> {
    nurbs_curve_parameter_domain(curve)?;
    let count = curve.control_points.len();
    let weights = match &curve.weights {
        Some(weights) if weights.len() == count => Cow::Borrowed(weights.as_slice()),
        Some(_) => return None,
        None => Cow::Owned(alloc_filled(count, 1.0, "ir_nurbs_curve_weights").ok()?),
    };
    if curve
        .control_points
        .iter()
        .zip(weights.as_ref())
        .any(|(control, weight)| {
            !control.x.is_finite()
                || !control.y.is_finite()
                || !control.z.is_finite()
                || !weight.is_finite()
                || *weight <= 0.0
        })
        || curve.knots.iter().any(|knot| !knot.is_finite())
        || !knots_nondecreasing(&curve.knots)
    {
        return None;
    }
    Some(weights)
}

fn nurbs_curve_speed_bound_about(
    curve: &NurbsCurve,
    weights: &[f64],
    origin: Point3,
) -> Option<f64> {
    let degree = usize::try_from(curve.degree).ok()?;
    let count = curve.control_points.len();
    let minimum_weight = weights.iter().copied().fold(f64::INFINITY, f64::min);
    let radius = |control: &Point3| {
        ((control.x - origin.x).powi(2)
            + (control.y - origin.y).powi(2)
            + (control.z - origin.z).powi(2))
        .sqrt()
    };
    let maximum_weighted_radius = curve
        .control_points
        .iter()
        .zip(weights)
        .map(|(control, weight)| weight * radius(control))
        .fold(0.0_f64, f64::max);
    let mut maximum_numerator_speed = 0.0_f64;
    let mut maximum_weight_speed = 0.0_f64;
    for index in 0..count - 1 {
        let denominator = curve.knots[index + degree + 1] - curve.knots[index + 1];
        if denominator == 0.0 {
            continue;
        }
        let factor = f64::from(curve.degree) / denominator;
        let first = curve.control_points[index];
        let second = curve.control_points[index + 1];
        let numerator_delta = Vector3::new(
            weights[index + 1] * (second.x - origin.x) - weights[index] * (first.x - origin.x),
            weights[index + 1] * (second.y - origin.y) - weights[index] * (first.y - origin.y),
            weights[index + 1] * (second.z - origin.z) - weights[index] * (first.z - origin.z),
        );
        maximum_numerator_speed = maximum_numerator_speed.max(factor * numerator_delta.norm());
        maximum_weight_speed =
            maximum_weight_speed.max(factor * (weights[index + 1] - weights[index]).abs());
    }
    let speed_bound = maximum_numerator_speed / minimum_weight
        + maximum_weighted_radius * maximum_weight_speed / minimum_weight.powi(2);
    speed_bound.is_finite().then_some(speed_bound)
}

fn interval_distance_to_parameter(interval: [f64; 2], parameter: f64) -> f64 {
    if parameter < interval[0] {
        interval[0] - parameter
    } else if parameter > interval[1] {
        parameter - interval[1]
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug)]
struct SearchInterval {
    bounds: [f64; 2],
    distance: f64,
}

impl PartialEq for SearchInterval {
    fn eq(&self, other: &Self) -> bool {
        self.distance.total_cmp(&other.distance) == Ordering::Equal
            && self.bounds[0].total_cmp(&other.bounds[0]) == Ordering::Equal
            && self.bounds[1].total_cmp(&other.bounds[1]) == Ordering::Equal
    }
}

impl Eq for SearchInterval {}

impl PartialOrd for SearchInterval {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SearchInterval {
    fn cmp(&self, other: &Self) -> Ordering {
        self.distance
            .total_cmp(&other.distance)
            .then_with(|| self.bounds[0].total_cmp(&other.bounds[0]))
            .then_with(|| self.bounds[1].total_cmp(&other.bounds[1]))
    }
}

/// Retain only the nearest knot intervals that the bounded search can visit.
fn bounded_nearest_intervals(boundaries: &[f64], seed: f64) -> Vec<[f64; 2]> {
    let mut nearest = BinaryHeap::with_capacity(NURBS_SEARCH_MAX_INTERVALS + 1);
    for pair in boundaries.windows(2) {
        if pair[0] >= pair[1] {
            continue;
        }
        let candidate = SearchInterval {
            bounds: [pair[0], pair[1]],
            distance: interval_distance_to_parameter([pair[0], pair[1]], seed),
        };
        if nearest.len() < NURBS_SEARCH_MAX_INTERVALS {
            nearest.push(candidate);
        } else if nearest.peek().is_some_and(|farthest| candidate < *farthest) {
            nearest.pop();
            nearest.push(candidate);
        }
    }
    let mut intervals = nearest.into_vec();
    intervals.sort_unstable_by(|first, second| second.cmp(first));
    intervals
        .into_iter()
        .map(|interval| interval.bounds)
        .collect()
}

/// Retain the final valid knot intervals without materializing the full partition.
fn bounded_tail_intervals(boundaries: &[f64]) -> (Vec<[f64; 2]>, bool) {
    let mut valid = boundaries
        .windows(2)
        .rev()
        .filter_map(|pair| (pair[0] < pair[1]).then_some([pair[0], pair[1]]));
    let mut intervals = valid
        .by_ref()
        .take(NURBS_SEARCH_MAX_INTERVALS)
        .collect::<Vec<_>>();
    let truncated = valid.next().is_some();
    intervals.reverse();
    (intervals, truncated)
}

#[derive(Debug, PartialEq)]
enum BoundaryWitness {
    Invalid,
    NoMatch,
    Found(f64),
}

/// Find the nearest admissible distinct boundary without cloning and sorting knots.
fn nearest_boundary_witness<F>(
    boundaries: &[f64],
    seed: f64,
    tolerance: f64,
    mut distance: F,
) -> BoundaryWitness
where
    F: FnMut(f64) -> Option<f64>,
{
    let mut previous_boundary = None;
    let mut nearest = None;
    let mut nearest_seed_distance = f64::INFINITY;
    for &parameter in boundaries {
        if previous_boundary == Some(parameter) {
            continue;
        }
        previous_boundary = Some(parameter);
        let seed_distance = (parameter - seed).abs();
        if seed_distance >= nearest_seed_distance {
            continue;
        }
        let Some(candidate_distance) = distance(parameter) else {
            return BoundaryWitness::Invalid;
        };
        if candidate_distance <= tolerance {
            nearest = Some(parameter);
            nearest_seed_distance = seed_distance;
        }
    }
    nearest.map_or(BoundaryWitness::NoMatch, BoundaryWitness::Found)
}

fn parameter_interval_containing(boundaries: &[f64], parameter: f64) -> Option<[f64; 2]> {
    boundaries.windows(2).find_map(|pair| {
        (pair[0] < pair[1] && parameter >= pair[0] && parameter <= pair[1])
            .then_some([pair[0], pair[1]])
    })
}

/// Map a NURBS parameter onto its evaluable knot branch.
///
/// Periodic parameters retain their serialized phase outside this operation
/// and are interpreted modulo the positive knot-domain period.
pub fn map_nurbs_curve_parameter(curve: &NurbsCurve, parameter: f64) -> Option<f64> {
    let [lower, upper] = nurbs_curve_parameter_domain(curve)?;
    if !parameter.is_finite() {
        return None;
    }
    if curve.periodic {
        let period = upper - lower;
        Some(lower + (parameter - lower).rem_euclid(period))
    } else {
        (lower..=upper).contains(&parameter).then_some(parameter)
    }
}

/// Evaluate a possibly-rational B-spline curve over 2D `(u, v)` poles.
pub fn nurbs_pcurve_uv(
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<Point2> {
    nurbs_pcurve_differential(degree, knots, control_points, weights, t)
        .map(|differential| differential.point)
}

/// Return the signed endpoint-frame offset between two fitted sketch NURBS.
///
/// Both curves must be nonperiodic and clamped. Their corresponding endpoint
/// tangents must be parallel, and both result endpoints must have the same
/// normal displacement from the source. The result curve can have the opposite
/// stored traversal and can use a different degree or knot vector. This checks
/// the boundary-frame invariant of a fitted offset relation; it does not assert
/// pointwise equality between independently fitted interior parameterizations.
pub fn fitted_nurbs_offset_frame_distance(
    source: &crate::sketches::SketchGeometry,
    result: &crate::sketches::SketchGeometry,
    linear_tolerance: f64,
) -> Option<f64> {
    use crate::sketches::SketchGeometry;

    if !linear_tolerance.is_finite() || linear_tolerance < 0.0 {
        return None;
    }
    let (
        SketchGeometry::Nurbs {
            degree: source_degree,
            knots: source_knots,
            control_points: source_points,
            weights: source_weights,
            periodic: false,
        },
        SketchGeometry::Nurbs {
            degree: result_degree,
            knots: result_knots,
            control_points: result_points,
            weights: result_weights,
            periodic: false,
        },
    ) = (source, result)
    else {
        return None;
    };
    let source_frames = clamped_nurbs_pcurve_endpoint_frames(
        *source_degree,
        source_knots,
        source_points,
        source_weights.as_deref(),
    )?;
    let result_frames = clamped_nurbs_pcurve_endpoint_frames(
        *result_degree,
        result_knots,
        result_points,
        result_weights.as_deref(),
    )?;
    let same = fitted_nurbs_offset_candidate(source_frames, result_frames, linear_tolerance);
    let reversed = fitted_nurbs_offset_candidate(
        source_frames,
        [
            (
                result_frames[1].0,
                Point2::new(-result_frames[1].1.u, -result_frames[1].1.v),
            ),
            (
                result_frames[0].0,
                Point2::new(-result_frames[0].1.u, -result_frames[0].1.v),
            ),
        ],
        linear_tolerance,
    );
    match (same, reversed) {
        (Some(distance), None) | (None, Some(distance)) => Some(distance),
        _ => None,
    }
}

fn clamped_nurbs_pcurve_endpoint_frames(
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
) -> Option<[(Point2, Point2); 2]> {
    let [lower, upper] = nurbs_pcurve_parameter_domain(degree, knots, control_points.len())?;
    let degree = usize::try_from(degree).ok()?;
    if degree == 0
        || control_points.len() < 2
        || knots.iter().take(degree + 1).any(|knot| *knot != lower)
        || knots
            .iter()
            .skip(control_points.len())
            .take(degree + 1)
            .any(|knot| *knot != upper)
        || weights.is_some_and(|weights| {
            weights.len() != control_points.len()
                || weights
                    .iter()
                    .any(|weight| !weight.is_finite() || *weight <= 0.0)
        })
    {
        return None;
    }
    let start = control_points[0];
    let end = *control_points.last()?;
    let start_tangent = control_points
        .iter()
        .skip(1)
        .map(|point| Point2::new(point.u - start.u, point.v - start.v))
        .find(|tangent| {
            tangent.u.hypot(tangent.v) > EPS_EVAL_CLAMPED_NURBS_PCURVE_ENDPOINT_FRAMES_E12
        })?;
    let end_tangent = control_points
        .iter()
        .rev()
        .skip(1)
        .map(|point| Point2::new(end.u - point.u, end.v - point.v))
        .find(|tangent| {
            tangent.u.hypot(tangent.v) > EPS_EVAL_CLAMPED_NURBS_PCURVE_ENDPOINT_FRAMES_E12
        })?;
    Some([(start, start_tangent), (end, end_tangent)])
}

fn fitted_nurbs_offset_candidate(
    source: [(Point2, Point2); 2],
    result: [(Point2, Point2); 2],
    linear_tolerance: f64,
) -> Option<f64> {
    let mut distances = [0.0; 2];
    for ordinal in 0..2 {
        let (source_point, source_tangent) = source[ordinal];
        let (result_point, result_tangent) = result[ordinal];
        let source_length = source_tangent.u.hypot(source_tangent.v);
        let result_length = result_tangent.u.hypot(result_tangent.v);
        if source_length <= EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E12
            || result_length <= EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E12
        {
            return None;
        }
        let parallel_error =
            (source_tangent.u * result_tangent.v - source_tangent.v * result_tangent.u).abs()
                / (source_length * result_length);
        if parallel_error > EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9 {
            return None;
        }
        let offset = Point2::new(
            result_point.u - source_point.u,
            result_point.v - source_point.v,
        );
        let tangential =
            (offset.u * source_tangent.u + offset.v * source_tangent.v) / source_length;
        let coordinate_scale = 1.0
            + source_point
                .u
                .abs()
                .max(source_point.v.abs())
                .max(result_point.u.abs())
                .max(result_point.v.abs());
        if tangential.abs()
            > linear_tolerance.max(EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9 * coordinate_scale)
        {
            return None;
        }
        distances[ordinal] =
            (-source_tangent.v * offset.u + source_tangent.u * offset.v) / source_length;
    }
    let scale = 1.0 + distances[0].abs().max(distances[1].abs());
    ((distances[0] - distances[1]).abs()
        <= linear_tolerance.max(EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9 * scale)
        && distances[0].abs() > linear_tolerance.max(EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9))
    .then_some((distances[0] + distances[1]) * 0.5)
}

struct PcurveDifferential {
    point: Point2,
    tangent: Option<Point2>,
    acceleration: Option<Point2>,
}

fn nurbs_pcurve_differential(
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<PcurveDifferential> {
    let degree = usize::try_from(degree).ok()?;
    let span = bspline_span(knots, degree, control_points.len(), t)?;
    let basis = bspline_basis(knots, degree, span, t)?;
    let derivative = bspline_basis_derivative(knots, degree, span, t)?;
    let second_derivative = bspline_basis_second_derivative(knots, degree, span, t)?;
    let mut u = 0.0;
    let mut v = 0.0;
    let mut weight_sum = 0.0;
    let mut du = 0.0;
    let mut dv = 0.0;
    let mut weight_derivative = 0.0;
    let mut ddu = 0.0;
    let mut ddv = 0.0;
    let mut weight_second_derivative = 0.0;
    for i in 0..=degree {
        let index = span - degree + i;
        let weight = weights
            .and_then(|weights| weights.get(index).copied())
            .unwrap_or(1.0);
        let pole = control_points.get(index)?;
        u += basis[i] * weight * pole.u;
        v += basis[i] * weight * pole.v;
        weight_sum += basis[i] * weight;
        du += derivative[i] * weight * pole.u;
        dv += derivative[i] * weight * pole.v;
        weight_derivative += derivative[i] * weight;
        ddu += second_derivative[i] * weight * pole.u;
        ddv += second_derivative[i] * weight * pole.v;
        weight_second_derivative += second_derivative[i] * weight;
    }
    if weight_sum == 0.0 {
        return None;
    }
    let point = Point2::new(u / weight_sum, v / weight_sum);
    let tangent = Point2::new(
        (du - point.u * weight_derivative) / weight_sum,
        (dv - point.v * weight_derivative) / weight_sum,
    );
    let acceleration = Point2::new(
        (ddu - point.u * weight_second_derivative - 2.0 * weight_derivative * tangent.u)
            / weight_sum,
        (ddv - point.v * weight_second_derivative - 2.0 * weight_derivative * tangent.v)
            / weight_sum,
    );
    if !point.u.is_finite() || !point.v.is_finite() {
        return None;
    }
    Some(PcurveDifferential {
        point,
        tangent: (tangent.u.is_finite() && tangent.v.is_finite()).then_some(tangent),
        acceleration: (acceleration.u.is_finite() && acceleration.v.is_finite())
            .then_some(acceleration),
    })
}

/// Return whether a point lies within `tolerance` of a nonperiodic NURBS
/// pcurve, using evaluated witnesses and Lipschitz-bounded interval rejection.
///
/// Positive rational weights make both the homogeneous curve and its
/// derivative convex combinations of their control polygons. Their norms
/// therefore bound Euclidean curve speed after the quotient rule. The search
/// accepts only an evaluated curve point within tolerance; intervals whose
/// midpoint distance minus the maximum possible travel exceeds tolerance are
/// discarded. `None` denotes invalid input or exhaustion of the bounded search.
pub fn nurbs_pcurve_contains_point(
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    point: Point2,
    tolerance: f64,
) -> Option<bool> {
    let degree_usize = usize::try_from(degree).ok()?;
    let count = control_points.len();
    if degree_usize == 0
        || count <= degree_usize
        || knots.len() < count.checked_add(degree_usize)?.checked_add(1)?
        || !tolerance.is_finite()
        || tolerance < 0.0
        || !point.u.is_finite()
        || !point.v.is_finite()
    {
        return None;
    }
    let owned_weights;
    let weights = match weights {
        Some(weights) if weights.len() == count => weights,
        Some(_) => return None,
        None => {
            owned_weights = alloc_filled(count, 1.0, "ir_nurbs_pcurve_weights").ok()?;
            &owned_weights
        }
    };
    if control_points.iter().zip(weights).any(|(control, weight)| {
        !control.u.is_finite() || !control.v.is_finite() || !weight.is_finite() || *weight <= 0.0
    }) || knots.iter().any(|knot| !knot.is_finite())
        || !knots_nondecreasing(knots)
    {
        return None;
    }

    let minimum_weight = weights.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum_weighted_radius = control_points
        .iter()
        .zip(weights)
        .map(|(control, weight)| weight * (control.u - point.u).hypot(control.v - point.v))
        .fold(0.0_f64, f64::max);
    let mut maximum_numerator_speed = 0.0_f64;
    let mut maximum_weight_speed = 0.0_f64;
    for index in 0..count - 1 {
        let denominator = knots[index + degree_usize + 1] - knots[index + 1];
        if denominator == 0.0 {
            continue;
        }
        let factor = f64::from(degree) / denominator;
        let first_u = weights[index] * (control_points[index].u - point.u);
        let first_v = weights[index] * (control_points[index].v - point.v);
        let second_u = weights[index + 1] * (control_points[index + 1].u - point.u);
        let second_v = weights[index + 1] * (control_points[index + 1].v - point.v);
        maximum_numerator_speed =
            maximum_numerator_speed.max(factor * (second_u - first_u).hypot(second_v - first_v));
        maximum_weight_speed =
            maximum_weight_speed.max(factor * (weights[index + 1] - weights[index]).abs());
    }
    let speed_bound = maximum_numerator_speed / minimum_weight
        + maximum_weighted_radius * maximum_weight_speed / minimum_weight.powi(2);
    if !speed_bound.is_finite() {
        return None;
    }

    let domain = [knots[degree_usize], knots[count]];
    if domain[0] > domain[1] {
        return None;
    }
    let (mut intervals, truncated) = bounded_tail_intervals(&knots[degree_usize..=count]);
    if intervals.is_empty() {
        intervals.push(domain);
    }
    let mut examined = 0usize;
    while let Some([start, end]) = intervals.pop() {
        examined += 1;
        if examined > NURBS_SEARCH_MAX_INTERVALS {
            return None;
        }
        let middle = start + (end - start) * 0.5;
        let curve_point = nurbs_pcurve_uv(degree, knots, control_points, Some(weights), middle)?;
        let distance = (curve_point.u - point.u).hypot(curve_point.v - point.v);
        if distance <= tolerance {
            return Some(true);
        }
        let travel_bound = speed_bound * (end - start) * 0.5;
        if distance - travel_bound > tolerance {
            continue;
        }
        if middle == start || middle == end {
            continue;
        }
        intervals.push([start, middle]);
        intervals.push([middle, end]);
    }
    (!truncated).then_some(false)
}

/// Evaluate a tensor-product NURBS surface at `(u, v)`.
pub fn nurbs_surface_point(surface: &NurbsSurface, u_at: f64, v_at: f64) -> Option<Point3> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    if surface.control_points.len() != u_count.checked_mul(v_count)? {
        return None;
    }
    let u_at = periodic_parameter(
        &surface.u_knots,
        u_degree,
        u_count,
        surface.u_periodic,
        u_at,
    )?;
    let v_at = periodic_parameter(
        &surface.v_knots,
        v_degree,
        v_count,
        surface.v_periodic,
        v_at,
    )?;
    let u_span = bspline_span(&surface.u_knots, u_degree, u_count, u_at)?;
    let v_span = bspline_span(&surface.v_knots, v_degree, v_count, v_at)?;
    let u_basis = bspline_basis(&surface.u_knots, u_degree, u_span, u_at)?;
    let v_basis = bspline_basis(&surface.v_knots, v_degree, v_span, v_at)?;
    let mut x = 0.0;
    let mut y = 0.0;
    let mut z = 0.0;
    let mut weight_sum = 0.0;
    for (i, u_value) in u_basis.iter().enumerate() {
        for (j, v_value) in v_basis.iter().enumerate() {
            let index = (u_span - u_degree + i) * v_count + (v_span - v_degree + j);
            let weight = surface
                .weights
                .as_ref()
                .and_then(|weights| weights.get(index).copied())
                .unwrap_or(1.0);
            let factor = u_value * v_value * weight;
            let pole = surface.control_points.get(index)?;
            x += factor * pole.x;
            y += factor * pole.y;
            z += factor * pole.z;
            weight_sum += factor;
        }
    }
    (weight_sum != 0.0).then(|| Point3::new(x / weight_sum, y / weight_sum, z / weight_sum))
}

/// The parametric direction a surface isoline holds fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolineDirection {
    /// `u` is fixed; the curve runs along `v` in the surface's `v` parameter.
    ConstantU,
    /// `v` is fixed; the curve runs along `u` in the surface's `u` parameter.
    ConstantV,
}

/// The isoline of `surface` at `at` in `direction`, as an exact NURBS curve.
///
/// A tensor-product surface restricted to a constant parameter in one direction
/// is a NURBS curve of the free direction's degree over the free direction's
/// knot vector, whose poles are the fixed direction's pole rows blended by the
/// basis at `at`. The result is exact, not a fit; its parameter is the
/// surface's own parameter in the free direction.
pub fn nurbs_surface_isoline(
    surface: &NurbsSurface,
    direction: IsolineDirection,
    at: f64,
) -> Option<NurbsCurve> {
    let fixed_axis = match direction {
        IsolineDirection::ConstantU => SurfaceParameterAxis::U,
        IsolineDirection::ConstantV => SurfaceParameterAxis::V,
    };
    nurbs_surface_isocurve(surface, fixed_axis, at)
}

/// Extract the exact rational NURBS curve obtained by fixing one parameter of
/// a tensor-product NURBS surface.
pub fn nurbs_surface_isocurve(
    surface: &NurbsSurface,
    fixed_axis: SurfaceParameterAxis,
    fixed_parameter: f64,
) -> Option<NurbsCurve> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    if surface.control_points.len() != u_count.checked_mul(v_count)?
        || surface
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != surface.control_points.len())
    {
        return None;
    }
    let (fixed_degree, fixed_count, fixed_knots, fixed_periodic) = match fixed_axis {
        SurfaceParameterAxis::U => (u_degree, u_count, &surface.u_knots, surface.u_periodic),
        SurfaceParameterAxis::V => (v_degree, v_count, &surface.v_knots, surface.v_periodic),
    };
    let fixed_parameter = periodic_parameter(
        fixed_knots,
        fixed_degree,
        fixed_count,
        fixed_periodic,
        fixed_parameter,
    )?;
    let fixed_span = bspline_span(fixed_knots, fixed_degree, fixed_count, fixed_parameter)?;
    let fixed_basis = bspline_basis(fixed_knots, fixed_degree, fixed_span, fixed_parameter)?;
    let varying_count = match fixed_axis {
        SurfaceParameterAxis::U => v_count,
        SurfaceParameterAxis::V => u_count,
    };
    let mut control_points = Vec::with_capacity(varying_count);
    let mut derived_weights = Vec::with_capacity(varying_count);
    for varying in 0..varying_count {
        let mut weighted = [0.0; 3];
        let mut weight_sum = 0.0;
        for (local, basis) in fixed_basis.iter().copied().enumerate() {
            let fixed = fixed_span - fixed_degree + local;
            let index = match fixed_axis {
                SurfaceParameterAxis::U => fixed * v_count + varying,
                SurfaceParameterAxis::V => varying * v_count + fixed,
            };
            let weight = surface
                .weights
                .as_ref()
                .and_then(|weights| weights.get(index).copied())
                .unwrap_or(1.0);
            let factor = basis * weight;
            let point = surface.control_points.get(index)?;
            weighted[0] += factor * point.x;
            weighted[1] += factor * point.y;
            weighted[2] += factor * point.z;
            weight_sum += factor;
        }
        if !weight_sum.is_finite() || weight_sum <= 0.0 {
            return None;
        }
        control_points.push(Point3::new(
            weighted[0] / weight_sum,
            weighted[1] / weight_sum,
            weighted[2] / weight_sum,
        ));
        derived_weights.push(weight_sum);
    }
    let (degree, knots, periodic) = match fixed_axis {
        SurfaceParameterAxis::U => (
            surface.v_degree,
            surface.v_knots.clone(),
            surface.v_periodic,
        ),
        SurfaceParameterAxis::V => (
            surface.u_degree,
            surface.u_knots.clone(),
            surface.u_periodic,
        ),
    };
    Some(NurbsCurve {
        degree,
        knots,
        control_points,
        weights: surface.weights.as_ref().map(|_| derived_weights),
        periodic,
    })
}

/// Point and first partial derivatives of a NURBS surface in its stored
/// parameterization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfacePartials {
    /// Surface point at `(u, v)`.
    pub point: Point3,
    /// First partial derivative with respect to `u`.
    pub du: Vector3,
    /// First partial derivative with respect to `v`.
    pub dv: Vector3,
}

/// Point, first partials, and second partials of a surface in its stored
/// parameterization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceSecondPartials {
    /// Surface point at `(u, v)`.
    pub point: Point3,
    /// First partial derivative with respect to `u`.
    pub du: Vector3,
    /// First partial derivative with respect to `v`.
    pub dv: Vector3,
    /// Second partial derivative with respect to `u`.
    pub duu: Vector3,
    /// Mixed partial derivative.
    pub duv: Vector3,
    /// Second partial derivative with respect to `v`.
    pub dvv: Vector3,
}

/// Evaluate a tensor-product NURBS surface and its exact rational first
/// partials at `(u, v)`.
pub fn nurbs_surface_partials(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Option<SurfacePartials> {
    nurbs_surface_second_partials(surface, u_at, v_at).map(|partials| SurfacePartials {
        point: partials.point,
        du: partials.du,
        dv: partials.dv,
    })
}

/// Evaluate a tensor-product NURBS surface and its exact rational first and
/// second partials at `(u, v)`.
pub fn nurbs_surface_second_partials(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Option<SurfaceSecondPartials> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    if surface.control_points.len() != u_count.checked_mul(v_count)?
        || surface
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != surface.control_points.len())
    {
        return None;
    }
    let u_at = periodic_parameter(
        &surface.u_knots,
        u_degree,
        u_count,
        surface.u_periodic,
        u_at,
    )?;
    let v_at = periodic_parameter(
        &surface.v_knots,
        v_degree,
        v_count,
        surface.v_periodic,
        v_at,
    )?;
    let u_span = bspline_span(&surface.u_knots, u_degree, u_count, u_at)?;
    let v_span = bspline_span(&surface.v_knots, v_degree, v_count, v_at)?;
    let u_basis = bspline_basis(&surface.u_knots, u_degree, u_span, u_at)?;
    let v_basis = bspline_basis(&surface.v_knots, v_degree, v_span, v_at)?;
    let u_derivative = bspline_basis_derivative(&surface.u_knots, u_degree, u_span, u_at)?;
    let v_derivative = bspline_basis_derivative(&surface.v_knots, v_degree, v_span, v_at)?;
    let u_second = bspline_basis_second_derivative(&surface.u_knots, u_degree, u_span, u_at)?;
    let v_second = bspline_basis_second_derivative(&surface.v_knots, v_degree, v_span, v_at)?;
    let mut weighted = [0.0; 3];
    let mut weighted_u = [0.0; 3];
    let mut weighted_v = [0.0; 3];
    let mut weighted_uu = [0.0; 3];
    let mut weighted_uv = [0.0; 3];
    let mut weighted_vv = [0.0; 3];
    let mut weight = 0.0;
    let mut weight_u = 0.0;
    let mut weight_v = 0.0;
    let mut weight_uu = 0.0;
    let mut weight_uv = 0.0;
    let mut weight_vv = 0.0;
    for i in 0..=u_degree {
        for j in 0..=v_degree {
            let index = (u_span - u_degree + i) * v_count + (v_span - v_degree + j);
            let pole = surface.control_points.get(index)?;
            let pole_weight = surface
                .weights
                .as_ref()
                .map_or(1.0, |weights| weights[index]);
            let basis = u_basis[i] * v_basis[j] * pole_weight;
            let basis_u = u_derivative[i] * v_basis[j] * pole_weight;
            let basis_v = u_basis[i] * v_derivative[j] * pole_weight;
            let basis_uu = u_second[i] * v_basis[j] * pole_weight;
            let basis_uv = u_derivative[i] * v_derivative[j] * pole_weight;
            let basis_vv = u_basis[i] * v_second[j] * pole_weight;
            for (axis, coordinate) in [pole.x, pole.y, pole.z].into_iter().enumerate() {
                weighted[axis] += basis * coordinate;
                weighted_u[axis] += basis_u * coordinate;
                weighted_v[axis] += basis_v * coordinate;
                weighted_uu[axis] += basis_uu * coordinate;
                weighted_uv[axis] += basis_uv * coordinate;
                weighted_vv[axis] += basis_vv * coordinate;
            }
            weight += basis;
            weight_u += basis_u;
            weight_v += basis_v;
            weight_uu += basis_uu;
            weight_uv += basis_uv;
            weight_vv += basis_vv;
        }
    }
    if weight == 0.0 {
        return None;
    }
    let point = Point3::new(
        weighted[0] / weight,
        weighted[1] / weight,
        weighted[2] / weight,
    );
    let derivative = |weighted_derivative: [f64; 3], weight_derivative: f64| {
        Vector3::new(
            (weighted_derivative[0] - point.x * weight_derivative) / weight,
            (weighted_derivative[1] - point.y * weight_derivative) / weight,
            (weighted_derivative[2] - point.z * weight_derivative) / weight,
        )
    };
    let du = derivative(weighted_u, weight_u);
    let dv = derivative(weighted_v, weight_v);
    let second_derivative = |weighted_derivative: [f64; 3],
                             weight_derivative: f64,
                             first_weight: f64,
                             first: Vector3| {
        Vector3::new(
            (weighted_derivative[0] - point.x * weight_derivative - 2.0 * first_weight * first.x)
                / weight,
            (weighted_derivative[1] - point.y * weight_derivative - 2.0 * first_weight * first.y)
                / weight,
            (weighted_derivative[2] - point.z * weight_derivative - 2.0 * first_weight * first.z)
                / weight,
        )
    };
    let mixed_derivative = Vector3::new(
        (weighted_uv[0] - point.x * weight_uv - weight_u * dv.x - weight_v * du.x) / weight,
        (weighted_uv[1] - point.y * weight_uv - weight_u * dv.y - weight_v * du.y) / weight,
        (weighted_uv[2] - point.z * weight_uv - weight_u * dv.z - weight_v * du.z) / weight,
    );
    Some(SurfaceSecondPartials {
        point,
        du,
        dv,
        duu: second_derivative(weighted_uu, weight_uu, weight_u, du),
        duv: mixed_derivative,
        dvv: second_derivative(weighted_vv, weight_vv, weight_v, dv),
    })
}

fn periodic_parameter(
    knots: &[f64],
    degree: usize,
    count: usize,
    periodic: bool,
    parameter: f64,
) -> Option<f64> {
    parameter.is_finite().then_some(())?;
    let start = *knots.get(degree)?;
    let end = *knots.get(count)?;
    if !periodic || (start..=end).contains(&parameter) {
        return Some(parameter);
    }
    let period = end - start;
    (period.is_finite() && period > 0.0).then(|| start + (parameter - start).rem_euclid(period))
}

/// Evaluate a 3D curve carrier at parameter `t` on its own parameterization.
pub fn curve_point(geometry: &CurveGeometry, t: f64) -> Option<Point3> {
    curve_point_inner(geometry, t, 0)
}

/// Evaluate the exact first derivative of a directly stored curve.
pub fn curve_tangent(geometry: &CurveGeometry, t: f64) -> Option<Vector3> {
    if !t.is_finite() {
        return None;
    }
    curve_tangent_inner(geometry, t, 0)
        .filter(|tangent| tangent.x.is_finite() && tangent.y.is_finite() && tangent.z.is_finite())
}

/// Evaluate the exact second derivative of a directly stored curve.
pub fn curve_second_derivative(geometry: &CurveGeometry, t: f64) -> Option<Vector3> {
    if !t.is_finite() {
        return None;
    }
    curve_second_derivative_inner(geometry, t, 0).filter(|derivative| {
        derivative.x.is_finite() && derivative.y.is_finite() && derivative.z.is_finite()
    })
}

fn curve_tangent_inner(geometry: &CurveGeometry, t: f64, depth: usize) -> Option<Vector3> {
    if depth > 256 {
        return None;
    }
    match geometry {
        CurveGeometry::Line { direction, .. } => Some(*direction),
        CurveGeometry::Circle {
            axis,
            ref_direction,
            radius,
            ..
        } => Some(vector_sum(&[
            (-radius * t.sin(), *ref_direction),
            (radius * t.cos(), axis.cross(*ref_direction)),
        ])),
        CurveGeometry::Ellipse {
            axis,
            major_direction,
            major_radius,
            minor_radius,
            ..
        } => Some(vector_sum(&[
            (-major_radius * t.sin(), *major_direction),
            (minor_radius * t.cos(), axis.cross(*major_direction)),
        ])),
        CurveGeometry::Parabola {
            axis,
            major_direction,
            focal_distance,
            ..
        } => Some(vector_sum(&[
            (2.0 * focal_distance * t, *major_direction),
            (2.0 * focal_distance, axis.cross(*major_direction)),
        ])),
        CurveGeometry::Hyperbola {
            axis,
            major_direction,
            major_radius,
            minor_radius,
            ..
        } => Some(vector_sum(&[
            (major_radius * t.sinh(), *major_direction),
            (minor_radius * t.cosh(), axis.cross(*major_direction)),
        ])),
        CurveGeometry::Nurbs(nurbs) => {
            let parameter = map_nurbs_curve_parameter(nurbs, t)?;
            nurbs_curve_tangent(
                nurbs.degree,
                &nurbs.knots,
                &nurbs.control_points,
                nurbs.weights.as_deref(),
                parameter,
            )
        }
        CurveGeometry::Polyline {
            points, parameters, ..
        } => polyline_tangent(points, parameters.as_deref(), t),
        CurveGeometry::Transformed { basis, transform } => curve_tangent_inner(basis, t, depth + 1)
            .map(|tangent| affine_vector(*transform, tangent)),
        CurveGeometry::Degenerate { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Unknown { .. } => None,
    }
}

fn curve_second_derivative_inner(
    geometry: &CurveGeometry,
    t: f64,
    depth: usize,
) -> Option<Vector3> {
    if depth > 256 {
        return None;
    }
    let zero = Vector3::new(0.0, 0.0, 0.0);
    match geometry {
        CurveGeometry::Line { .. } => Some(zero),
        CurveGeometry::Circle {
            axis,
            ref_direction,
            radius,
            ..
        } => Some(vector_sum(&[
            (-radius * t.cos(), *ref_direction),
            (-radius * t.sin(), axis.cross(*ref_direction)),
        ])),
        CurveGeometry::Ellipse {
            axis,
            major_direction,
            major_radius,
            minor_radius,
            ..
        } => Some(vector_sum(&[
            (-major_radius * t.cos(), *major_direction),
            (-minor_radius * t.sin(), axis.cross(*major_direction)),
        ])),
        CurveGeometry::Parabola {
            major_direction,
            focal_distance,
            ..
        } => Some(vector_sum(&[(2.0 * focal_distance, *major_direction)])),
        CurveGeometry::Hyperbola {
            axis,
            major_direction,
            major_radius,
            minor_radius,
            ..
        } => Some(vector_sum(&[
            (major_radius * t.cosh(), *major_direction),
            (minor_radius * t.sinh(), axis.cross(*major_direction)),
        ])),
        CurveGeometry::Nurbs(nurbs) => {
            let parameter = map_nurbs_curve_parameter(nurbs, t)?;
            nurbs_curve_second_derivative(
                nurbs.degree,
                &nurbs.knots,
                &nurbs.control_points,
                nurbs.weights.as_deref(),
                parameter,
            )
        }
        CurveGeometry::Polyline {
            points, parameters, ..
        } => polyline_tangent(points, parameters.as_deref(), t).map(|_| zero),
        CurveGeometry::Transformed { basis, transform } => {
            curve_second_derivative_inner(basis, t, depth + 1)
                .map(|derivative| affine_vector(*transform, derivative))
        }
        CurveGeometry::Degenerate { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Unknown { .. } => None,
    }
}

fn nurbs_curve_tangent(
    degree: u32,
    knots: &[f64],
    control_points: &[Point3],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<Vector3> {
    let degree = usize::try_from(degree).ok()?;
    let span = bspline_span(knots, degree, control_points.len(), t)?;
    let basis = bspline_basis(knots, degree, span, t)?;
    let derivatives = bspline_basis_derivative(knots, degree, span, t)?;
    let mut weighted = Vector3::new(0.0, 0.0, 0.0);
    let mut weighted_derivative = Vector3::new(0.0, 0.0, 0.0);
    let mut weight = 0.0;
    let mut weight_derivative = 0.0;
    for (local, (basis, derivative)) in basis.iter().zip(&derivatives).enumerate() {
        let index = span - degree + local;
        let control = control_points.get(index)?;
        let control_weight = weights
            .and_then(|weights| weights.get(index).copied())
            .unwrap_or(1.0);
        weighted.x += basis * control_weight * control.x;
        weighted.y += basis * control_weight * control.y;
        weighted.z += basis * control_weight * control.z;
        weighted_derivative.x += derivative * control_weight * control.x;
        weighted_derivative.y += derivative * control_weight * control.y;
        weighted_derivative.z += derivative * control_weight * control.z;
        weight += basis * control_weight;
        weight_derivative += derivative * control_weight;
    }
    if weight == 0.0 {
        return None;
    }
    let tangent = Vector3::new(
        (weighted_derivative.x * weight - weighted.x * weight_derivative) / (weight * weight),
        (weighted_derivative.y * weight - weighted.y * weight_derivative) / (weight * weight),
        (weighted_derivative.z * weight - weighted.z * weight_derivative) / (weight * weight),
    );
    (tangent.x.is_finite() && tangent.y.is_finite() && tangent.z.is_finite()).then_some(tangent)
}

fn nurbs_curve_second_derivative(
    degree: u32,
    knots: &[f64],
    control_points: &[Point3],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<Vector3> {
    let degree = usize::try_from(degree).ok()?;
    let span = bspline_span(knots, degree, control_points.len(), t)?;
    let basis = bspline_basis(knots, degree, span, t)?;
    let first_basis = bspline_basis_derivative(knots, degree, span, t)?;
    let second_basis = bspline_basis_second_derivative(knots, degree, span, t)?;
    let mut weighted = Vector3::new(0.0, 0.0, 0.0);
    let mut weighted_first = Vector3::new(0.0, 0.0, 0.0);
    let mut weighted_second = Vector3::new(0.0, 0.0, 0.0);
    let mut weight = 0.0;
    let mut weight_first = 0.0;
    let mut weight_second = 0.0;
    for local in 0..=degree {
        let index = span - degree + local;
        let control = control_points.get(index)?;
        let control_weight = weights
            .and_then(|weights| weights.get(index).copied())
            .unwrap_or(1.0);
        let accumulate = |target: &mut Vector3, factor: f64| {
            target.x += factor * control.x;
            target.y += factor * control.y;
            target.z += factor * control.z;
        };
        let basis = basis[local] * control_weight;
        let first = first_basis[local] * control_weight;
        let second = second_basis[local] * control_weight;
        accumulate(&mut weighted, basis);
        accumulate(&mut weighted_first, first);
        accumulate(&mut weighted_second, second);
        weight += basis;
        weight_first += first;
        weight_second += second;
    }
    if weight == 0.0 {
        return None;
    }
    let point = Vector3::new(
        weighted.x / weight,
        weighted.y / weight,
        weighted.z / weight,
    );
    let first = Vector3::new(
        (weighted_first.x - point.x * weight_first) / weight,
        (weighted_first.y - point.y * weight_first) / weight,
        (weighted_first.z - point.z * weight_first) / weight,
    );
    Some(Vector3::new(
        (weighted_second.x - point.x * weight_second - 2.0 * weight_first * first.x) / weight,
        (weighted_second.y - point.y * weight_second - 2.0 * weight_first * first.y) / weight,
        (weighted_second.z - point.z * weight_second - 2.0 * weight_first * first.z) / weight,
    ))
}

/// Evaluate a curve carrier selected by arena id, including supported
/// procedural constructions.
pub fn model_curve_point_by_id(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
) -> Option<Point3> {
    model_curve_point_by_id_inner(index, curve_id, parameter, 0)
}

#[derive(Clone, Copy)]
struct ModelCurveDifferential {
    point: Point3,
    tangent: Vector3,
    acceleration: Vector3,
}

/// Evaluate the native helix path and its exact angle derivatives.
///
/// The stored angular interval is the domain of the path parameter. The
/// pitch and apex terms advance by the fraction of one full revolution from
/// the interval's lower bound, while the major and minor vectors define the
/// radial frame at the stored angle.
fn helix_differential(
    definition: &ProceduralCurveDefinition,
    parameter: f64,
) -> Option<ModelCurveDifferential> {
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
    let angle_range = *angle_range;
    let center = *center;
    let major = *major;
    let minor = *minor;
    let pitch = *pitch;
    let apex_factor = *apex_factor;
    let axis = *axis;
    let [start, end] = angle_range;
    if ![start, end, apex_factor, parameter]
        .into_iter()
        .all(f64::is_finite)
        || start > end
        || parameter < start
        || parameter > end
        || ![
            center.x, center.y, center.z, major.x, major.y, major.z, minor.x, minor.y, minor.z,
            pitch.x, pitch.y, pitch.z,
        ]
        .into_iter()
        .all(f64::is_finite)
        || unit_axis(axis).is_none()
    {
        return None;
    }

    let inverse_revolution = 1.0 / std::f64::consts::TAU;
    let revolution_fraction = (parameter - start) * inverse_revolution;
    let radial_scale = 1.0 + apex_factor * revolution_fraction;
    let radial = vector_sum(&[(parameter.cos(), major), (parameter.sin(), minor)]);
    let radial_first = vector_sum(&[(-parameter.sin(), major), (parameter.cos(), minor)]);
    let point = offset(
        center,
        &[(radial_scale, radial), (revolution_fraction, pitch)],
    );
    let scale_first = apex_factor * inverse_revolution;
    let tangent = vector_sum(&[
        (radial_scale, radial_first),
        (scale_first, radial),
        (inverse_revolution, pitch),
    ]);
    let acceleration = vector_sum(&[(-radial_scale, radial), (2.0 * scale_first, radial_first)]);
    let finite_vector = |vector: Vector3| {
        [vector.x, vector.y, vector.z]
            .into_iter()
            .all(f64::is_finite)
    };
    (point.x.is_finite()
        && point.y.is_finite()
        && point.z.is_finite()
        && finite_vector(tangent)
        && finite_vector(acceleration))
    .then_some(ModelCurveDifferential {
        point,
        tangent,
        acceleration,
    })
}

fn model_curve_differential_by_id(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
) -> Option<ModelCurveDifferential> {
    model_curve_differential_by_id_inner(index, curve_id, parameter, 0)
}

fn model_curve_differential_by_id_inner(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
    depth: usize,
) -> Option<ModelCurveDifferential> {
    if depth > 256 || !parameter.is_finite() {
        return None;
    }
    let curve = index.curves(&curve_id.0)?;
    if let Some(procedural) = index
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|procedural| procedural.curve == *curve_id)
    {
        match &procedural.definition {
            ProceduralCurveDefinition::Replica { source, transform } => {
                let differential =
                    model_curve_differential_by_id_inner(index, source, parameter, depth + 1)?;
                return Some(ModelCurveDifferential {
                    point: affine_point(*transform, differential.point),
                    tangent: affine_vector(*transform, differential.tangent),
                    acceleration: affine_vector(*transform, differential.acceleration),
                });
            }
            ProceduralCurveDefinition::Subset {
                source,
                parameter_range: [start, end],
                sense,
            } => {
                let span = (end - start).abs();
                if !span.is_finite() || span == 0.0 || parameter < 0.0 || parameter > span {
                    return None;
                }
                let source_parameter = if *sense {
                    start + parameter
                } else {
                    end - parameter
                };
                let differential = model_curve_differential_by_id_inner(
                    index,
                    source,
                    source_parameter,
                    depth + 1,
                )?;
                let parameter_scale = if *sense { 1.0 } else { -1.0 };
                return Some(ModelCurveDifferential {
                    point: differential.point,
                    tangent: scale_vector(differential.tangent, parameter_scale),
                    acceleration: differential.acceleration,
                });
            }
            ProceduralCurveDefinition::Helix { .. } => {
                return helix_differential(&procedural.definition, parameter);
            }
            _ => {}
        }
    }
    if matches!(&curve.geometry, CurveGeometry::Procedural { .. }) {
        return None;
    }
    Some(ModelCurveDifferential {
        point: curve_point(&curve.geometry, parameter)?,
        tangent: curve_tangent(&curve.geometry, parameter)?,
        acceleration: curve_second_derivative(&curve.geometry, parameter)?,
    })
}

fn unit_axis(direction: Vector3) -> Option<Vector3> {
    let length = direction.norm();
    (length.is_finite() && length > f64::EPSILON).then(|| scale_vector(direction, 1.0 / length))
}

fn rotate_vector_about_axis(vector: Vector3, axis: Vector3, angle: f64) -> Vector3 {
    let cosine = angle.cos();
    let sine = angle.sin();
    vector_sum(&[
        (cosine, vector),
        (sine, axis.cross(vector)),
        (axis.dot(vector) * (1.0 - cosine), axis),
    ])
}

fn model_axis_revolution_point(
    index: &crate::index::ModelIndex<'_>,
    directrix: &crate::ids::CurveId,
    axis_origin: Point3,
    axis_direction: Vector3,
    angle: f64,
    parameter: f64,
) -> Option<Point3> {
    if !angle.is_finite() {
        return None;
    }
    let axis = unit_axis(axis_direction)?;
    let point = model_curve_point_by_id(index, directrix, parameter)?;
    let relative = Vector3::new(
        point.x - axis_origin.x,
        point.y - axis_origin.y,
        point.z - axis_origin.z,
    );
    Some(offset(
        axis_origin,
        &[(1.0, rotate_vector_about_axis(relative, axis, angle))],
    ))
}

fn model_axis_revolution_partials(
    index: &crate::index::ModelIndex<'_>,
    directrix: &crate::ids::CurveId,
    axis_origin: Point3,
    axis_direction: Vector3,
    angle: f64,
    parameter: f64,
) -> Option<SurfaceSecondPartials> {
    if !angle.is_finite() {
        return None;
    }
    let axis = unit_axis(axis_direction)?;
    let differential = model_curve_differential_by_id(index, directrix, parameter)?;
    let relative = Vector3::new(
        differential.point.x - axis_origin.x,
        differential.point.y - axis_origin.y,
        differential.point.z - axis_origin.z,
    );
    let rotated = rotate_vector_about_axis(relative, axis, angle);
    let rotated_tangent = rotate_vector_about_axis(differential.tangent, axis, angle);
    let rotated_acceleration = rotate_vector_about_axis(differential.acceleration, axis, angle);
    let du = axis.cross(rotated);
    Some(SurfaceSecondPartials {
        point: offset(axis_origin, &[(1.0, rotated)]),
        du,
        dv: rotated_tangent,
        duu: axis.cross(du),
        duv: axis.cross(rotated_tangent),
        dvv: rotated_acceleration,
    })
}

/// Map a construction-space directrix parameter to the carrier curve and
/// return the carrier derivative with respect to the construction parameter.
///
/// IGES line entities use a normalized surface interval while the neutral
/// line carrier uses signed distance. Other curve carriers retain their native
/// parameterization. A line used by more than one edge is only unambiguous
/// when every retained edge range agrees, unless the construction stores its
/// neutral carrier interval explicitly.
fn record_u_interval(record_bounds: Option<[Option<f64>; 4]>) -> Option<[f64; 2]> {
    let [Some(start), Some(end), _, _] = record_bounds? else {
        return None;
    };
    Some([start, end])
}

fn is_line_geometry(geometry: &CurveGeometry, depth: usize) -> bool {
    if depth > 256 {
        return false;
    }
    match geometry {
        CurveGeometry::Line { .. } => true,
        CurveGeometry::Transformed { basis, .. } => is_line_geometry(basis, depth + 1),
        _ => false,
    }
}

fn construction_curve_parameter(
    index: &crate::index::ModelIndex<'_>,
    directrix: &crate::ids::CurveId,
    parameter: f64,
    surface_interval: Option<[f64; 2]>,
    carrier_interval: Option<[f64; 2]>,
    reversed: bool,
) -> Option<(f64, f64)> {
    if !parameter.is_finite() {
        return None;
    }
    let (parameter, surface_derivative) = match (surface_interval, carrier_interval) {
        (Some([surface_start, surface_end]), Some([carrier_start, carrier_end])) => {
            let surface_width = surface_end - surface_start;
            let carrier_width = carrier_end - carrier_start;
            if !surface_start.is_finite()
                || !surface_end.is_finite()
                || !carrier_start.is_finite()
                || !carrier_end.is_finite()
                || surface_width <= 0.0
                || carrier_width <= 0.0
                || parameter < carrier_start
                || parameter > carrier_end
            {
                return None;
            }
            let derivative = surface_width / carrier_width;
            let source_parameter = (parameter - carrier_start).mul_add(derivative, surface_start);
            (source_parameter, derivative)
        }
        (Some([surface_start, surface_end]), None) => {
            let surface_width = surface_end - surface_start;
            if !surface_start.is_finite()
                || !surface_end.is_finite()
                || surface_width <= 0.0
                || parameter < surface_start
                || parameter > surface_end
            {
                return None;
            }
            (parameter, 1.0)
        }
        (None, Some([carrier_start, carrier_end])) => {
            if !carrier_start.is_finite()
                || !carrier_end.is_finite()
                || carrier_start >= carrier_end
                || parameter < carrier_start
                || parameter > carrier_end
            {
                return None;
            }
            (parameter, 1.0)
        }
        (None, None) => (parameter, 1.0),
    };
    let curve = index.curves(&directrix.0)?;
    let Some([surface_start, surface_end]) = surface_interval else {
        return if reversed {
            Some((-parameter, -surface_derivative))
        } else {
            Some((parameter, surface_derivative))
        };
    };
    let surface_width = surface_end - surface_start;
    if !is_line_geometry(&curve.geometry, 0) {
        return if reversed {
            Some((-parameter, -surface_derivative))
        } else {
            Some((parameter, surface_derivative))
        };
    }
    let line_interval = if let Some(carrier_interval) = carrier_interval {
        carrier_interval
    } else {
        let mut ranges = index
            .ir()
            .model
            .edges
            .iter()
            .filter(|edge| edge.curve.as_ref() == Some(directrix))
            .filter_map(|edge| edge.param_range);
        let interval = ranges.next()?;
        if ranges.any(|range| range != interval) {
            return None;
        }
        interval
    };
    let [curve_start, curve_end] = line_interval;
    if !curve_start.is_finite() || !curve_end.is_finite() {
        return None;
    }
    let curve_width = curve_end - curve_start;
    if !curve_width.is_finite() || curve_width <= 0.0 || surface_width <= 0.0 {
        return None;
    }
    let derivative = if reversed {
        -curve_width / surface_width * surface_derivative
    } else {
        curve_width / surface_width * surface_derivative
    };
    let fraction = if reversed {
        (surface_end - parameter) / surface_width
    } else {
        (parameter - surface_start) / surface_width
    };
    Some((curve_start + fraction * curve_width, derivative))
}

// Both serialized parameter domains and the revision reversal affect the derivative mapping.
#[allow(clippy::too_many_arguments)]
fn model_native_extrusion_partials(
    index: &crate::index::ModelIndex<'_>,
    directrix: &crate::ids::CurveId,
    direction: Vector3,
    parameter_interval: Option<[f64; 2]>,
    carrier_interval: Option<[f64; 2]>,
    directrix_reversed: bool,
    u: f64,
    v: f64,
) -> Option<SurfaceSecondPartials> {
    if !v.is_finite() {
        return None;
    }
    let (parameter, derivative) = construction_curve_parameter(
        index,
        directrix,
        u,
        parameter_interval,
        carrier_interval,
        directrix_reversed,
    )?;
    let differential = model_curve_differential_by_id(index, directrix, parameter)?;
    let zero = Vector3::new(0.0, 0.0, 0.0);
    Some(SurfaceSecondPartials {
        point: offset(differential.point, &[(v, direction)]),
        du: scale_vector(differential.tangent, derivative),
        dv: direction,
        duu: scale_vector(differential.acceleration, derivative * derivative),
        duv: zero,
        dvv: zero,
    })
}

fn extrusion_directrix_reversed(
    revision_form: Option<&crate::geometry::RevisionSurfaceForm>,
) -> bool {
    revision_form
        .and_then(|form| form.flags.first())
        .copied()
        .unwrap_or(false)
}

#[allow(clippy::too_many_arguments)]
fn model_native_revolution_partials(
    index: &crate::index::ModelIndex<'_>,
    directrix: &crate::ids::CurveId,
    axis_origin: Point3,
    axis_direction: Vector3,
    angular_interval: [f64; 2],
    angular_parameter_interval: Option<[f64; 2]>,
    parameter_interval: Option<[f64; 2]>,
    carrier_interval: Option<[f64; 2]>,
    transposed: bool,
    u: f64,
    v: f64,
) -> Option<SurfaceSecondPartials> {
    if !angular_interval.iter().all(|value| value.is_finite()) {
        return None;
    }
    let (directrix_parameter, angular_parameter) = if transposed { (v, u) } else { (u, v) };
    let (directrix_parameter, derivative) = construction_curve_parameter(
        index,
        directrix,
        directrix_parameter,
        parameter_interval,
        carrier_interval,
        false,
    )?;
    let (angle, angular_derivative) = angular_parameter_interval.map_or_else(
        || Some((angular_parameter, 1.0)),
        |parameter_interval| {
            let parameter_span = parameter_interval[1] - parameter_interval[0];
            let angular_span = angular_interval[1] - angular_interval[0];
            if !parameter_interval.iter().all(|value| value.is_finite())
                || parameter_span == 0.0
                || !angular_span.is_finite()
            {
                return None;
            }
            let angular_derivative = angular_span / parameter_span;
            Some((
                (angular_parameter - parameter_interval[0])
                    .mul_add(angular_derivative, angular_interval[0]),
                angular_derivative,
            ))
        },
    )?;
    let partials = model_axis_revolution_partials(
        index,
        directrix,
        axis_origin,
        axis_direction,
        angle,
        directrix_parameter,
    )?;

    if transposed {
        Some(SurfaceSecondPartials {
            point: partials.point,
            du: scale_vector(partials.du, angular_derivative),
            dv: scale_vector(partials.dv, derivative),
            duu: scale_vector(partials.duu, angular_derivative * angular_derivative),
            duv: scale_vector(partials.duv, derivative * angular_derivative),
            dvv: scale_vector(partials.dvv, derivative * derivative),
        })
    } else {
        Some(SurfaceSecondPartials {
            point: partials.point,
            du: scale_vector(partials.dv, derivative),
            dv: scale_vector(partials.du, angular_derivative),
            duu: scale_vector(partials.dvv, derivative * derivative),
            duv: scale_vector(partials.duv, derivative * angular_derivative),
            dvv: scale_vector(partials.duu, angular_derivative * angular_derivative),
        })
    }
}

fn model_curve_point_by_id_inner(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
    depth: usize,
) -> Option<Point3> {
    if depth > 256 {
        return None;
    }
    let curve = index.curves(&curve_id.0)?;
    let Some(procedural) = index
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|procedural| procedural.curve == *curve_id)
    else {
        return curve_point(&curve.geometry, parameter);
    };
    if procedural.curve != *curve_id {
        return None;
    }
    match &procedural.definition {
        ProceduralCurveDefinition::Replica { source, transform } => {
            model_curve_point_by_id_inner(index, source, parameter, depth + 1)
                .map(|point| affine_point(*transform, point))
        }
        ProceduralCurveDefinition::Subset {
            source,
            parameter_range: [start, end],
            sense,
        } => {
            let span = (end - start).abs();
            if !parameter.is_finite()
                || !span.is_finite()
                || span == 0.0
                || parameter < 0.0
                || parameter > span
            {
                return None;
            }
            let source_parameter = if *sense {
                start + parameter
            } else {
                end - parameter
            };
            model_curve_point_by_id_inner(index, source, source_parameter, depth + 1)
        }
        ProceduralCurveDefinition::Helix { .. } => {
            helix_differential(&procedural.definition, parameter)
                .map(|differential| differential.point)
        }
        ProceduralCurveDefinition::TolerantIntersection {
            supports,
            tolerance,
            parameterization: Some(parameterization),
            ..
        } => {
            let parameter_range = parameterization.parameter_range;
            if !parameter.is_finite()
                || parameter < parameter_range[0]
                || parameter > parameter_range[1]
            {
                return None;
            }
            let points = std::array::from_fn(|side| {
                let uv = pcurve_uv(&parameterization.pcurves[side], parameter)?;
                model_surface_point_by_id(index, &supports[side], uv.u, uv.v)
            });
            let [Some(first), Some(second)] = points else {
                return None;
            };
            let separation = ((first.x - second.x).powi(2)
                + (first.y - second.y).powi(2)
                + (first.z - second.z).powi(2))
            .sqrt();
            (separation.is_finite() && separation <= *tolerance).then_some(first)
        }
        _ => {
            if matches!(&curve.geometry, CurveGeometry::Procedural { .. }) {
                None
            } else {
                curve_point(&curve.geometry, parameter)
            }
        }
    }
}

/// Invert a model curve near a caller-selected branch parameter.
///
/// Direct analytic and NURBS carriers preserve their native parameterization.
/// Charted tolerant intersections invert a support chart. The seed selects
/// between repeated model-space points. The returned parameter is
/// forward-validated against the direct carrier or complete two-support
/// construction.
pub fn model_curve_parameter_near_point(
    ir: &CadIr,
    curve_id: &crate::ids::CurveId,
    point: Point3,
    seed: f64,
) -> Option<f64> {
    let index = crate::index::ModelIndex::new(ir);
    model_curve_parameter_near_point_in_index(&index, curve_id, point, seed)
}

/// Invert a model curve using a caller-owned lookup index.
///
/// Batch callers must reuse one index so carrier inversion remains linear in
/// the document population rather than rebuilding the index for every edge.
pub fn model_curve_parameter_near_point_in_index(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    point: Point3,
    seed: f64,
) -> Option<f64> {
    model_curve_parameter_near_point_in_index_with_tolerance(
        index,
        curve_id,
        point,
        seed,
        index.ir().tolerances.linear,
    )
}

/// Invert a model curve using a caller-owned lookup index and tolerance.
///
/// The tolerance controls the forward validation of every candidate returned
/// by the inversion. Callers that admit an evaluated geometric residual above
/// the document default must pass that same admission bound here.
pub fn model_curve_parameter_near_point_in_index_with_tolerance(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    point: Point3,
    seed: f64,
    tolerance: f64,
) -> Option<f64> {
    if !tolerance.is_finite() || tolerance < 0.0 {
        return None;
    }
    model_curve_parameter_near_point_with_tolerance(index, curve_id, point, seed, tolerance, 0)
}

fn model_curve_parameter_near_point_with_tolerance(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    point: Point3,
    seed: f64,
    tolerance: f64,
    depth: usize,
) -> Option<f64> {
    if depth > 256 {
        return None;
    }
    let curve = index.curves(&curve_id.0)?;
    if let Some(procedural) = index
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|procedural| procedural.curve == *curve_id)
    {
        match &procedural.definition {
            ProceduralCurveDefinition::Replica { source, transform } => {
                let (basis_point, tolerance_scale) = inverse_affine_point(*transform, point)?;
                let basis_tolerance = tolerance * tolerance_scale;
                if !basis_tolerance.is_finite() {
                    return None;
                }
                return model_curve_parameter_near_point_with_tolerance(
                    index,
                    source,
                    basis_point,
                    seed,
                    basis_tolerance,
                    depth + 1,
                );
            }
            ProceduralCurveDefinition::Subset {
                source,
                parameter_range: [start, end],
                sense,
            } => {
                let span = (end - start).abs();
                if !seed.is_finite()
                    || !tolerance.is_finite()
                    || tolerance < 0.0
                    || !span.is_finite()
                    || span == 0.0
                    || seed < 0.0
                    || seed > span
                {
                    return None;
                }
                let source_seed = if *sense { start + seed } else { end - seed };
                let source_parameter = model_curve_parameter_near_point_with_tolerance(
                    index,
                    source,
                    point,
                    source_seed,
                    tolerance,
                    depth + 1,
                )?;
                let parameter = if *sense {
                    source_parameter - start
                } else {
                    end - source_parameter
                };
                return (parameter.is_finite()
                    && parameter >= 0.0
                    && parameter <= span
                    && model_curve_point_by_id(index, curve_id, parameter)
                        .is_some_and(|evaluated| evaluated.distance(point) <= tolerance))
                .then_some(parameter);
            }
            ProceduralCurveDefinition::Helix { .. } => {
                return helix_parameter_near_point(
                    index,
                    curve_id,
                    point,
                    seed,
                    tolerance,
                    &procedural.definition,
                );
            }
            _ => {}
        }
    }
    if !matches!(&curve.geometry, CurveGeometry::Procedural { .. }) {
        return curve_parameter_near_point(&curve.geometry, point, seed, tolerance);
    }
    let CurveGeometry::Procedural { construction } = &curve.geometry else {
        unreachable!("direct carriers return before procedural inversion");
    };
    let procedural = index.procedural_curves(&construction.0)?;
    if procedural.curve != *curve_id {
        return None;
    }
    let crate::geometry::ProceduralCurveDefinition::TolerantIntersection {
        supports,
        tolerance,
        parameterization: Some(parameterization),
        ..
    } = &procedural.definition
    else {
        return None;
    };
    let range = parameterization.parameter_range;
    if !seed.is_finite() || seed < range[0] || seed > range[1] {
        return None;
    }
    let mut candidates = Vec::new();
    for (support_id, pcurve) in supports.iter().zip(&parameterization.pcurves) {
        let Some(surface) = index.surfaces(&support_id.0) else {
            continue;
        };
        let PcurveGeometry::Line { origin, direction } = pcurve else {
            continue;
        };
        let parameter = match &surface.geometry {
            SurfaceGeometry::Plane { .. } => {
                let Some(base) = model_surface_point_by_id(index, support_id, origin.u, origin.v)
                else {
                    continue;
                };
                let Some(next) = model_surface_point_by_id(
                    index,
                    support_id,
                    origin.u + direction.u,
                    origin.v + direction.v,
                ) else {
                    continue;
                };
                let tangent = Vector3::new(next.x - base.x, next.y - base.y, next.z - base.z);
                let offset = Vector3::new(point.x - base.x, point.y - base.y, point.z - base.z);
                let denominator = tangent.dot(tangent);
                (denominator.is_finite() && denominator > 0.0)
                    .then(|| offset.dot(tangent) / denominator)
            }
            SurfaceGeometry::Cylinder { .. }
            | SurfaceGeometry::Cone { .. }
            | SurfaceGeometry::Sphere { .. }
            | SurfaceGeometry::Torus { .. } => {
                analytic_surface_parameters(&surface.geometry, point).and_then(|mut uv| {
                    if direction.v == 0.0 && direction.u != 0.0 {
                        let expected = origin.u + direction.u * seed;
                        uv.u += ((expected - uv.u) / std::f64::consts::TAU).round()
                            * std::f64::consts::TAU;
                        Some((uv.u - origin.u) / direction.u)
                    } else if direction.u == 0.0
                        && direction.v != 0.0
                        && matches!(&surface.geometry, SurfaceGeometry::Torus { .. })
                    {
                        let expected = origin.v + direction.v * seed;
                        uv.v += ((expected - uv.v) / std::f64::consts::TAU).round()
                            * std::f64::consts::TAU;
                        Some((uv.v - origin.v) / direction.v)
                    } else if direction.u == 0.0 && direction.v != 0.0 {
                        Some((uv.v - origin.v) / direction.v)
                    } else {
                        None
                    }
                })
            }
            SurfaceGeometry::Nurbs(surface) => {
                let (fixed_axis, fixed_parameter, varying_origin, varying_scale) =
                    if direction.u == 0.0 && direction.v != 0.0 {
                        (SurfaceParameterAxis::U, origin.u, origin.v, direction.v)
                    } else if direction.v == 0.0 && direction.u != 0.0 {
                        (SurfaceParameterAxis::V, origin.v, origin.u, direction.u)
                    } else {
                        continue;
                    };
                let Some(isocurve) = nurbs_surface_isocurve(surface, fixed_axis, fixed_parameter)
                else {
                    continue;
                };
                let isocurve_seed = varying_origin + varying_scale * seed;
                nurbs_curve_parameter_near_point(&isocurve, point, *tolerance, isocurve_seed)
                    .map(|parameter| (parameter - varying_origin) / varying_scale)
            }
            _ => continue,
        };
        let Some(mut parameter) = parameter else {
            continue;
        };
        let endpoint_tolerance = EPS_EVAL_MODEL_CURVE_PARAMETER_NEAR_POINT_WITH_TOLERANCE_E12
            * (1.0 + range[0].abs().max(range[1].abs()));
        if parameter < range[0] && range[0] - parameter <= endpoint_tolerance {
            parameter = range[0];
        } else if parameter > range[1] && parameter - range[1] <= endpoint_tolerance {
            parameter = range[1];
        } else if parameter < range[0] || parameter > range[1] {
            continue;
        }
        let Some(evaluated) = model_curve_point_by_id(index, curve_id, parameter) else {
            continue;
        };
        let distance = ((evaluated.x - point.x).powi(2)
            + (evaluated.y - point.y).powi(2)
            + (evaluated.z - point.z).powi(2))
        .sqrt();
        if distance.is_finite() && distance <= *tolerance {
            candidates.push(parameter);
        }
    }
    candidates
        .into_iter()
        .min_by(|first, second| (first - seed).abs().total_cmp(&(second - seed).abs()))
}

/// Find a helix parameter near a caller-selected seed by bounded Newton
/// refinement of the squared model-space distance.
fn helix_parameter_near_point(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    target: Point3,
    seed: f64,
    tolerance: f64,
    definition: &ProceduralCurveDefinition,
) -> Option<f64> {
    let ProceduralCurveDefinition::Helix { angle_range, .. } = definition else {
        return None;
    };
    let [start, end] = *angle_range;
    if ![start, end, seed, tolerance]
        .into_iter()
        .all(f64::is_finite)
        || start > end
        || seed < start
        || seed > end
        || tolerance < 0.0
        || ![target.x, target.y, target.z]
            .into_iter()
            .all(f64::is_finite)
    {
        return None;
    }

    let mut parameter = seed;
    for _ in 0..MODEL_CURVE_PARAMETER_SEARCH_MAX_NEWTON_ITERATIONS {
        let differential = model_curve_differential_by_id(index, curve_id, parameter)?;
        let residual = Vector3::new(
            differential.point.x - target.x,
            differential.point.y - target.y,
            differential.point.z - target.z,
        );
        let distance = residual.norm();
        if distance.is_finite() && distance <= tolerance {
            return Some(parameter);
        }
        let denominator = differential.tangent.dot(differential.tangent);
        if !denominator.is_finite() || denominator <= 0.0 {
            break;
        }
        let next = parameter - residual.dot(differential.tangent) / denominator;
        if !next.is_finite() {
            break;
        }
        let next = next.clamp(start, end);
        if next == parameter {
            break;
        }
        parameter = next;
    }

    let differential = model_curve_differential_by_id(index, curve_id, parameter)?;
    let distance = Vector3::new(
        differential.point.x - target.x,
        differential.point.y - target.y,
        differential.point.z - target.z,
    )
    .norm();
    (distance.is_finite() && distance <= tolerance).then_some(parameter)
}

/// Invert a direct curve carrier near a caller-selected parameter seed.
pub(crate) fn curve_parameter_near_point(
    geometry: &CurveGeometry,
    point: Point3,
    seed: f64,
    tolerance: f64,
) -> Option<f64> {
    direct_curve_parameter_near_point(geometry, point, seed, tolerance)
}

fn direct_curve_parameter_near_point(
    geometry: &CurveGeometry,
    point: Point3,
    seed: f64,
    tolerance: f64,
) -> Option<f64> {
    if !seed.is_finite() || !tolerance.is_finite() || tolerance < 0.0 {
        return None;
    }
    let components = |origin: Point3, axis: Vector3, reference: Vector3| -> (f64, f64, f64) {
        let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
        let transverse = axis.cross(reference);
        (delta.dot(reference), delta.dot(transverse), delta.dot(axis))
    };
    let parameter = match geometry {
        CurveGeometry::Line { origin, direction } => {
            let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
            let denominator = direction.dot(*direction);
            (denominator.is_finite() && denominator > 0.0)
                .then(|| delta.dot(*direction) / denominator)?
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            if *radius == 0.0 {
                return None;
            }
            let (x, y, _) = components(*center, *axis, *ref_direction);
            let canonical = (y / radius).atan2(x / radius);
            canonical + ((seed - canonical) / std::f64::consts::TAU).round() * std::f64::consts::TAU
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            if *major_radius == 0.0 || *minor_radius == 0.0 {
                return None;
            }
            let (x, y, _) = components(*center, *axis, *major_direction);
            let canonical = (y / minor_radius).atan2(x / major_radius);
            canonical + ((seed - canonical) / std::f64::consts::TAU).round() * std::f64::consts::TAU
        }
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => {
            if *focal_distance == 0.0 {
                return None;
            }
            let (_, transverse, _) = components(*vertex, *axis, *major_direction);
            transverse / (2.0 * focal_distance)
        }
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            minor_radius,
            ..
        } => {
            if *minor_radius == 0.0 {
                return None;
            }
            let (_, transverse, _) = components(*center, *axis, *major_direction);
            (transverse / minor_radius).asinh()
        }
        CurveGeometry::Nurbs(curve) => {
            nurbs_curve_parameter_near_point(curve, point, tolerance, seed)?
        }
        CurveGeometry::Polyline {
            points, parameters, ..
        } => polyline_parameter_near_point(points, parameters.as_deref(), point, tolerance, seed)?,
        CurveGeometry::Transformed { basis, transform } => {
            let (basis_point, tolerance_scale) = inverse_affine_point(*transform, point)?;
            let basis_tolerance = tolerance * tolerance_scale;
            if !basis_tolerance.is_finite() {
                return None;
            }
            direct_curve_parameter_near_point(basis, basis_point, seed, basis_tolerance)?
        }
        CurveGeometry::Degenerate { point: stored } => {
            let error = (stored.x - point.x)
                .hypot(stored.y - point.y)
                .hypot(stored.z - point.z);
            (error.is_finite() && error <= tolerance).then_some(seed)?
        }
        CurveGeometry::Procedural { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Unknown { .. } => return None,
    };
    let evaluated = curve_point(geometry, parameter)?;
    let error = ((evaluated.x - point.x).powi(2)
        + (evaluated.y - point.y).powi(2)
        + (evaluated.z - point.z).powi(2))
    .sqrt();
    (parameter.is_finite() && error.is_finite() && error <= tolerance).then_some(parameter)
}

fn inverse_affine_point(transform: Transform, point: Point3) -> Option<(Point3, f64)> {
    let [first, second, third, bottom] = transform.rows;
    let [matrix_00, matrix_01, matrix_02, translate_x] = first;
    let [matrix_10, matrix_11, matrix_12, translate_y] = second;
    let [matrix_20, matrix_21, matrix_22, translate_z] = third;
    if bottom != [0.0, 0.0, 0.0, 1.0] {
        return None;
    }
    let cofactors = [
        [
            matrix_11 * matrix_22 - matrix_12 * matrix_21,
            matrix_02 * matrix_21 - matrix_01 * matrix_22,
            matrix_01 * matrix_12 - matrix_02 * matrix_11,
        ],
        [
            matrix_12 * matrix_20 - matrix_10 * matrix_22,
            matrix_00 * matrix_22 - matrix_02 * matrix_20,
            matrix_02 * matrix_10 - matrix_00 * matrix_12,
        ],
        [
            matrix_10 * matrix_21 - matrix_11 * matrix_20,
            matrix_01 * matrix_20 - matrix_00 * matrix_21,
            matrix_00 * matrix_11 - matrix_01 * matrix_10,
        ],
    ];
    let determinant =
        matrix_00 * cofactors[0][0] + matrix_01 * cofactors[1][0] + matrix_02 * cofactors[2][0];
    if !determinant.is_finite() || determinant == 0.0 {
        return None;
    }
    let inverse = cofactors.map(|row| row.map(|value| value / determinant));
    if inverse.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }
    let relative = [
        point.x - translate_x,
        point.y - translate_y,
        point.z - translate_z,
    ];
    let coordinates = inverse.map(|row| {
        row.into_iter()
            .zip(relative)
            .map(|(coefficient, coordinate)| coefficient * coordinate)
            .sum::<f64>()
    });
    let tolerance_scale = inverse
        .iter()
        .flatten()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    (coordinates
        .into_iter()
        .chain([tolerance_scale])
        .all(f64::is_finite))
    .then_some((
        Point3::new(coordinates[0], coordinates[1], coordinates[2]),
        tolerance_scale,
    ))
}

fn polyline_parameter_near_point(
    points: &[Point3],
    parameters: Option<&[f64]>,
    point: Point3,
    tolerance: f64,
    seed: f64,
) -> Option<f64> {
    if points.len() < 2 {
        return None;
    }
    let implicit;
    let parameters = if let Some(parameters) = parameters {
        (parameters.len() == points.len()).then_some(parameters)?
    } else {
        implicit = (0..points.len())
            .map(|index| index as f64)
            .collect::<Vec<_>>();
        &implicit
    };
    let mut candidates = Vec::new();
    for (segment, parameter_range) in parameters.windows(2).enumerate() {
        let [parameter_start, parameter_end] = [parameter_range[0], parameter_range[1]];
        let parameter_width = parameter_end - parameter_start;
        if !parameter_start.is_finite() || !parameter_end.is_finite() || parameter_width == 0.0 {
            continue;
        }
        let start = points[segment];
        let end = points[segment + 1];
        let direction = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
        let offset = Vector3::new(point.x - start.x, point.y - start.y, point.z - start.z);
        let length = direction.x.hypot(direction.y).hypot(direction.z);
        if !length.is_finite() {
            continue;
        }
        let fraction = if length == 0.0 {
            if offset.x.hypot(offset.y).hypot(offset.z) > tolerance {
                continue;
            }
            ((seed - parameter_start) / parameter_width).clamp(0.0, 1.0)
        } else {
            let unit = Vector3::new(
                direction.x / length,
                direction.y / length,
                direction.z / length,
            );
            (offset.dot(unit) / length).clamp(0.0, 1.0)
        };
        let candidate = parameter_start + fraction * parameter_width;
        let mapped = Point3::new(
            start.x + fraction * direction.x,
            start.y + fraction * direction.y,
            start.z + fraction * direction.z,
        );
        let error = (mapped.x - point.x)
            .hypot(mapped.y - point.y)
            .hypot(mapped.z - point.z);
        if candidate.is_finite() && error.is_finite() && error <= tolerance {
            candidates.push(candidate);
        }
    }
    candidates
        .into_iter()
        .min_by(|first, second| (first - seed).abs().total_cmp(&(second - seed).abs()))
}

fn curve_point_inner(geometry: &CurveGeometry, t: f64, depth: usize) -> Option<Point3> {
    if depth > 256 {
        return None;
    }
    match geometry {
        CurveGeometry::Line { origin, direction } => Some(offset(*origin, &[(t, *direction)])),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => Some(offset(
            *center,
            &[
                (radius * t.cos(), *ref_direction),
                (radius * t.sin(), axis.cross(*ref_direction)),
            ],
        )),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => Some(offset(
            *center,
            &[
                (major_radius * t.cos(), *major_direction),
                (minor_radius * t.sin(), axis.cross(*major_direction)),
            ],
        )),
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => Some(offset(
            *vertex,
            &[
                (focal_distance * t * t, *major_direction),
                (2.0 * focal_distance * t, axis.cross(*major_direction)),
            ],
        )),
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => Some(offset(
            *center,
            &[
                (major_radius * t.cosh(), *major_direction),
                (minor_radius * t.sinh(), axis.cross(*major_direction)),
            ],
        )),
        CurveGeometry::Degenerate { point } => Some(*point),
        CurveGeometry::Nurbs(nurbs) => {
            let parameter = map_nurbs_curve_parameter(nurbs, t)?;
            nurbs_curve_point(
                nurbs.degree,
                &nurbs.knots,
                &nurbs.control_points,
                nurbs.weights.as_deref(),
                parameter,
            )
        }
        CurveGeometry::Polyline {
            points, parameters, ..
        } => polyline_point(points, parameters.as_deref(), t),
        CurveGeometry::Transformed { basis, transform } => {
            curve_point_inner(basis, t, depth + 1).map(|point| affine_point(*transform, point))
        }
        CurveGeometry::Procedural { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Unknown { .. } => None,
    }
}

/// Evaluate a surface carrier at `(u, v)` on its own parameterization: `u` is
/// the azimuth angle and `v` the axial distance / polar angle on analytic
/// quadrics, and both are knot-domain parameters on NURBS surfaces.
pub fn surface_point(geometry: &SurfaceGeometry, u: f64, v: f64) -> Option<Point3> {
    surface_second_partials_inner(geometry, u, v, 0).map(|partials| partials.point)
}

/// Evaluate a directly stored surface and its exact first partial derivatives.
pub fn surface_partials(geometry: &SurfaceGeometry, u: f64, v: f64) -> Option<SurfacePartials> {
    surface_second_partials_inner(geometry, u, v, 0).map(|partials| SurfacePartials {
        point: partials.point,
        du: partials.du,
        dv: partials.dv,
    })
}

/// Evaluate a directly stored surface and its exact first and second partial
/// derivatives.
pub fn surface_second_partials(
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Option<SurfaceSecondPartials> {
    surface_second_partials_inner(geometry, u, v, 0)
}

fn surface_second_partials_inner(
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
    depth: usize,
) -> Option<SurfaceSecondPartials> {
    if depth > 256 {
        return None;
    }
    let zero = Vector3::new(0.0, 0.0, 0.0);
    match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let v_axis = normal.cross(*u_axis);
            Some(SurfaceSecondPartials {
                point: offset(*origin, &[(u, *u_axis), (v, v_axis)]),
                du: *u_axis,
                dv: v_axis,
                duu: zero,
                duv: zero,
                dvv: zero,
            })
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            let transverse = axis.cross(*ref_direction);
            let cosine = u.cos();
            let sine = u.sin();
            Some(SurfaceSecondPartials {
                point: offset(
                    *origin,
                    &[
                        (radius * cosine, *ref_direction),
                        (radius * sine, transverse),
                        (v, *axis),
                    ],
                ),
                du: vector_sum(&[
                    (-radius * sine, *ref_direction),
                    (radius * cosine, transverse),
                ]),
                dv: *axis,
                duu: vector_sum(&[
                    (-radius * cosine, *ref_direction),
                    (-radius * sine, transverse),
                ]),
                duv: zero,
                dvv: zero,
            })
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => {
            let transverse = axis.cross(*ref_direction);
            let cosine = u.cos();
            let sine = u.sin();
            let radial_slope = half_angle.tan();
            let local_radius = radius + v * radial_slope;
            Some(SurfaceSecondPartials {
                point: offset(
                    *origin,
                    &[
                        (local_radius * cosine, *ref_direction),
                        (local_radius * ratio * sine, transverse),
                        (v, *axis),
                    ],
                ),
                du: vector_sum(&[
                    (-local_radius * sine, *ref_direction),
                    (local_radius * ratio * cosine, transverse),
                ]),
                dv: vector_sum(&[
                    (radial_slope * cosine, *ref_direction),
                    (radial_slope * ratio * sine, transverse),
                    (1.0, *axis),
                ]),
                duu: vector_sum(&[
                    (-local_radius * cosine, *ref_direction),
                    (-local_radius * ratio * sine, transverse),
                ]),
                duv: vector_sum(&[
                    (-radial_slope * sine, *ref_direction),
                    (radial_slope * ratio * cosine, transverse),
                ]),
                dvv: zero,
            })
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let transverse = axis.cross(*ref_direction);
            let u_cosine = u.cos();
            let u_sine = u.sin();
            let v_cosine = v.cos();
            let v_sine = v.sin();
            Some(SurfaceSecondPartials {
                point: offset(
                    *center,
                    &[
                        (radius * v_cosine * u_cosine, *ref_direction),
                        (radius * v_cosine * u_sine, transverse),
                        (radius * v_sine, *axis),
                    ],
                ),
                du: vector_sum(&[
                    (-radius * v_cosine * u_sine, *ref_direction),
                    (radius * v_cosine * u_cosine, transverse),
                ]),
                dv: vector_sum(&[
                    (-radius * v_sine * u_cosine, *ref_direction),
                    (-radius * v_sine * u_sine, transverse),
                    (radius * v_cosine, *axis),
                ]),
                duu: vector_sum(&[
                    (-radius * v_cosine * u_cosine, *ref_direction),
                    (-radius * v_cosine * u_sine, transverse),
                ]),
                duv: vector_sum(&[
                    (radius * v_sine * u_sine, *ref_direction),
                    (-radius * v_sine * u_cosine, transverse),
                ]),
                dvv: vector_sum(&[
                    (-radius * v_cosine * u_cosine, *ref_direction),
                    (-radius * v_cosine * u_sine, transverse),
                    (-radius * v_sine, *axis),
                ]),
            })
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            let transverse = axis.cross(*ref_direction);
            let u_cosine = u.cos();
            let u_sine = u.sin();
            let v_cosine = v.cos();
            let v_sine = v.sin();
            let ring = major_radius + minor_radius * v_cosine;
            Some(SurfaceSecondPartials {
                point: offset(
                    *center,
                    &[
                        (ring * u_cosine, *ref_direction),
                        (ring * u_sine, transverse),
                        (minor_radius * v_sine, *axis),
                    ],
                ),
                du: vector_sum(&[
                    (-ring * u_sine, *ref_direction),
                    (ring * u_cosine, transverse),
                ]),
                dv: vector_sum(&[
                    (-minor_radius * v_sine * u_cosine, *ref_direction),
                    (-minor_radius * v_sine * u_sine, transverse),
                    (minor_radius * v_cosine, *axis),
                ]),
                duu: vector_sum(&[
                    (-ring * u_cosine, *ref_direction),
                    (-ring * u_sine, transverse),
                ]),
                duv: vector_sum(&[
                    (minor_radius * v_sine * u_sine, *ref_direction),
                    (-minor_radius * v_sine * u_cosine, transverse),
                ]),
                dvv: vector_sum(&[
                    (-minor_radius * v_cosine * u_cosine, *ref_direction),
                    (-minor_radius * v_cosine * u_sine, transverse),
                    (-minor_radius * v_sine, *axis),
                ]),
            })
        }
        SurfaceGeometry::Nurbs(nurbs) => nurbs_surface_second_partials(nurbs, u, v),
        SurfaceGeometry::Transformed { basis, transform } => {
            surface_second_partials_inner(basis, u, v, depth + 1).map(|partials| {
                SurfaceSecondPartials {
                    point: affine_point(*transform, partials.point),
                    du: affine_vector(*transform, partials.du),
                    dv: affine_vector(*transform, partials.dv),
                    duu: affine_vector(*transform, partials.duu),
                    duv: affine_vector(*transform, partials.duv),
                    dvv: affine_vector(*transform, partials.dvv),
                }
            })
        }
        SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

/// Evaluate a surface carrier with access to construction and child-carrier
/// arenas in `ir`.
pub fn model_surface_point(
    ir: &CadIr,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Option<Point3> {
    let SurfaceGeometry::Procedural { construction } = geometry else {
        return surface_point(geometry, u, v);
    };
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.id == *construction)?;
    let carrier_interval = record_u_interval(procedural.record_bounds);
    let index = crate::index::ModelIndex::new(ir);
    match &procedural.definition {
        ProceduralSurfaceDefinition::Extrusion {
            directrix,
            direction,
            parameter_interval,
            revision_form,
            ..
        } => model_native_extrusion_partials(
            &index,
            directrix,
            *direction,
            *parameter_interval,
            carrier_interval,
            extrusion_directrix_reversed(revision_form.as_ref()),
            u,
            v,
        )
        .map(|partials| partials.point),
        ProceduralSurfaceDefinition::LinearSweep {
            directrix,
            direction,
        } => model_curve_point_by_id(&index, directrix, u)
            .map(|point| offset(point, &[(v, *direction)])),
        ProceduralSurfaceDefinition::Revolution {
            directrix,
            axis_origin,
            axis_direction,
            angular_interval,
            angular_parameter_interval,
            parameter_interval,
            transposed,
            ..
        } => model_native_revolution_partials(
            &index,
            directrix,
            *axis_origin,
            *axis_direction,
            *angular_interval,
            *angular_parameter_interval,
            *parameter_interval,
            carrier_interval,
            *transposed,
            u,
            v,
        )
        .map(|partials| partials.point),
        ProceduralSurfaceDefinition::AxisRevolution {
            directrix,
            axis_origin,
            axis_direction,
        } => model_axis_revolution_point(&index, directrix, *axis_origin, *axis_direction, u, v),
        ProceduralSurfaceDefinition::Ruled { first, second } => {
            model_ruled_surface_partials(&index, first, second, u, v).map(|partials| partials.point)
        }
        ProceduralSurfaceDefinition::Sum {
            first,
            second,
            basepoint,
            ..
        } => model_sum_surface_partials(&index, first, second, *basepoint, u, v)
            .map(|partials| partials.point),
        ProceduralSurfaceDefinition::Sweep {
            profile,
            spine,
            native: Some(construction),
        } => cacheless_law_sweep_point(&index, profile, spine, construction, u, v),
        ProceduralSurfaceDefinition::VariableBlend { construction } => {
            cacheless_variable_blend_point(&index, construction, u, v)
        }
        ProceduralSurfaceDefinition::Blend {
            supports,
            radius,
            cross_section,
            native: Some(native),
            ..
        } => cacheless_constant_rolling_ball_point(
            &index,
            supports,
            radius,
            cross_section,
            native,
            u,
            v,
        ),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct ScalarSweepDifferential {
    value: f64,
    derivative: f64,
}

fn finite_sweep_differential(value: f64, derivative: f64) -> Option<ScalarSweepDifferential> {
    (value.is_finite() && derivative.is_finite())
        .then_some(ScalarSweepDifferential { value, derivative })
}

fn scalar_sweep_law_differential(
    expression: &LawExpression,
    parameter: f64,
) -> Option<ScalarSweepDifferential> {
    if !parameter.is_finite() {
        return None;
    }
    match expression {
        LawExpression::Null => finite_sweep_differential(0.0, 0.0),
        LawExpression::Integer { value } => finite_sweep_differential(*value as f64, 0.0),
        LawExpression::Double { value } => finite_sweep_differential(*value, 0.0),
        LawExpression::Text { value } => {
            let value = value.trim();
            if value == "X" {
                return finite_sweep_differential(parameter, 1.0);
            }
            if let Ok(constant) = value.parse::<f64>() {
                return finite_sweep_differential(constant, 0.0);
            }
            let (left, right) = value.split_once('*')?;
            if right.trim() == "X" {
                let coefficient = left.trim().parse::<f64>().ok()?;
                return finite_sweep_differential(coefficient * parameter, coefficient);
            }
            if left.trim() == "X" {
                let coefficient = right.trim().parse::<f64>().ok()?;
                return finite_sweep_differential(coefficient * parameter, coefficient);
            }
            None
        }
        LawExpression::Algebraic { operator, operands } => {
            if let [operand] = operands.as_slice() {
                let operand = scalar_sweep_law_differential(operand, parameter)?;
                return scalar_unary_sweep_law_differential(operator, operand);
            }
            if operator == "O" {
                let [outer, inner] = operands.as_slice() else {
                    return None;
                };
                let inner = scalar_sweep_law_differential(inner, parameter)?;
                let outer = scalar_sweep_law_differential(outer, inner.value)?;
                return finite_sweep_differential(outer.value, outer.derivative * inner.derivative);
            }
            let [left, right] = operands.as_slice() else {
                return None;
            };
            let left = scalar_sweep_law_differential(left, parameter)?;
            let right = scalar_sweep_law_differential(right, parameter)?;
            match operator.as_str() {
                "ADD" => finite_sweep_differential(
                    left.value + right.value,
                    left.derivative + right.derivative,
                ),
                "SUB" => finite_sweep_differential(
                    left.value - right.value,
                    left.derivative - right.derivative,
                ),
                "MUL" => finite_sweep_differential(
                    left.value * right.value,
                    left.derivative * right.value + left.value * right.derivative,
                ),
                "DIV" if right.value != 0.0 => {
                    let denominator = right.value * right.value;
                    finite_sweep_differential(
                        left.value / right.value,
                        (left.derivative * right.value - left.value * right.derivative)
                            / denominator,
                    )
                }
                _ => None,
            }
        }
        LawExpression::Point { .. }
        | LawExpression::Vector { .. }
        | LawExpression::Transform { .. }
        | LawExpression::TransformVec { .. }
        | LawExpression::Edge { .. }
        | LawExpression::Spline { .. } => None,
    }
}

fn scalar_unary_sweep_law_differential(
    operator: &str,
    operand: ScalarSweepDifferential,
) -> Option<ScalarSweepDifferential> {
    let x = operand.value;
    let derivative = match operator {
        "SIN" => x.cos(),
        "COS" => -x.sin(),
        "TAN" => {
            let cosine = x.cos();
            (cosine != 0.0).then_some(1.0 / (cosine * cosine))?
        }
        "COT" => {
            let sine = x.sin();
            (sine != 0.0).then_some(-1.0 / (sine * sine))?
        }
        "SEC" => {
            let cosine = x.cos();
            (cosine != 0.0).then_some(1.0 / cosine * x.tan())?
        }
        "CSC" => {
            let sine = x.sin();
            (sine != 0.0).then_some(-(1.0 / sine) * (x.cos() / sine))?
        }
        "COSH" => x.sinh(),
        "SINH" => x.cosh(),
        "TANH" => 1.0 - x.tanh() * x.tanh(),
        "COTH" => {
            let sinh = x.sinh();
            (sinh != 0.0).then_some(-1.0 / (sinh * sinh))?
        }
        "SECH" => {
            let value = 1.0 / x.cosh();
            -value * x.tanh()
        }
        "CSCH" => {
            let sinh = x.sinh();
            (sinh != 0.0).then_some(-(1.0 / sinh) * (x.cosh() / sinh))?
        }
        "ARCCOS" => {
            let denominator = (1.0 - x * x).sqrt();
            (denominator > 0.0).then_some(-1.0 / denominator)?
        }
        "ARCSIN" => {
            let denominator = (1.0 - x * x).sqrt();
            (denominator > 0.0).then_some(1.0 / denominator)?
        }
        "ARCTAN" => 1.0 / (1.0 + x * x),
        "ARCOT" => -1.0 / (1.0 + x * x),
        "ARCSEC" => {
            let denominator = (x * x - 1.0).sqrt();
            (x.abs() > 1.0 && denominator > 0.0).then_some(1.0 / (x.abs() * denominator))?
        }
        "ARCCSC" => {
            let denominator = (x * x - 1.0).sqrt();
            (x.abs() > 1.0 && denominator > 0.0).then_some(-1.0 / (x.abs() * denominator))?
        }
        "ARCCOSH" => {
            let denominator = (x * x - 1.0).sqrt();
            (x > 1.0 && denominator > 0.0).then_some(1.0 / denominator)?
        }
        "ARCSINH" => 1.0 / (1.0 + x * x).sqrt(),
        "ARCTANH" => (x.abs() < 1.0).then_some(1.0 / (1.0 - x * x))?,
        "ARCOTH" => (x.abs() > 1.0).then_some(1.0 / (1.0 - x * x))?,
        "ARCSECH" => {
            let denominator = (1.0 - x * x).sqrt();
            (x > 0.0 && x < 1.0 && denominator > 0.0).then_some(-1.0 / (x * denominator))?
        }
        "ARCCSCH" => (x != 0.0).then_some(-1.0 / (x.abs() * (1.0 + x * x).sqrt()))?,
        "ABS" => {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                return None;
            }
        }
        "EXP" => x.exp(),
        "LN" => (x > 0.0).then_some(1.0 / x)?,
        "SIGN" => (x != 0.0).then_some(0.0)?,
        "SQRT" => (x > 0.0).then_some(0.5 / x.sqrt())?,
        _ => return None,
    };
    finite_sweep_differential(
        match operator {
            "SIN" => x.sin(),
            "COS" => x.cos(),
            "TAN" => x.tan(),
            "COT" => 1.0 / x.tan(),
            "SEC" => 1.0 / x.cos(),
            "CSC" => 1.0 / x.sin(),
            "COSH" => x.cosh(),
            "SINH" => x.sinh(),
            "TANH" => x.tanh(),
            "COTH" => 1.0 / x.tanh(),
            "SECH" => 1.0 / x.cosh(),
            "CSCH" => 1.0 / x.sinh(),
            "ARCCOS" => x.acos(),
            "ARCSIN" => x.asin(),
            "ARCTAN" => x.atan(),
            "ARCOT" => std::f64::consts::FRAC_PI_2 - x.atan(),
            "ARCSEC" => (1.0 / x).acos(),
            "ARCCSC" => (1.0 / x).asin(),
            "ARCCOSH" => x.acosh(),
            "ARCSINH" => x.asinh(),
            "ARCTANH" => x.atanh(),
            "ARCOTH" => 0.5 * ((x + 1.0) / (x - 1.0)).ln(),
            "ARCSECH" => (1.0 / x).acosh(),
            "ARCCSCH" => (1.0 / x).asinh(),
            "ABS" => x.abs(),
            "EXP" => x.exp(),
            "LN" => x.ln(),
            "SIGN" => x.signum(),
            "SQRT" => x.sqrt(),
            _ => return None,
        },
        derivative * operand.derivative,
    )
}

fn sweep_scale(expression: &LawExpression) -> Option<Vector3> {
    match expression {
        LawExpression::Null => Some(Vector3::new(1.0, 1.0, 1.0)),
        LawExpression::Text { value } => {
            let value = value
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>();
            let values = value
                .strip_prefix("VEC(")
                .and_then(|value| value.strip_suffix(')'))?
                .split(',')
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            let [x, y, z] = values.as_slice() else {
                return None;
            };
            Some(Vector3::new(*x, *y, *z))
        }
        LawExpression::Vector { value } => Some(*value),
        _ => None,
    }
}

fn scale_sweep_profile(
    profile: ModelCurveDifferential,
    frame_point: Point3,
    scale: Vector3,
) -> Option<ModelCurveDifferential> {
    let displacement = point_displacement(profile.point, frame_point);
    let point = offset(
        frame_point,
        &[(
            1.0,
            Vector3::new(
                displacement.x * scale.x,
                displacement.y * scale.y,
                displacement.z * scale.z,
            ),
        )],
    );
    let tangent = Vector3::new(
        profile.tangent.x * scale.x,
        profile.tangent.y * scale.y,
        profile.tangent.z * scale.z,
    );
    let acceleration = Vector3::new(
        profile.acceleration.x * scale.x,
        profile.acceleration.y * scale.y,
        profile.acceleration.z * scale.z,
    );
    (point.x.is_finite()
        && point.y.is_finite()
        && point.z.is_finite()
        && tangent.x.is_finite()
        && tangent.y.is_finite()
        && tangent.z.is_finite()
        && acceleration.x.is_finite()
        && acceleration.y.is_finite()
        && acceleration.z.is_finite())
    .then_some(ModelCurveDifferential {
        point,
        tangent,
        acceleration,
    })
}

fn unit_domain_sweep_formula(name: &str) -> bool {
    let Some(bounds) = name
        .strip_prefix("DOMAIN(VEC(1,0,0),")
        .and_then(|name| name.strip_suffix(')'))
    else {
        return false;
    };
    let mut bounds = bounds.split(',');
    let Some(lower) = bounds.next().and_then(|value| value.parse::<f64>().ok()) else {
        return false;
    };
    let Some(upper) = bounds.next().and_then(|value| value.parse::<f64>().ok()) else {
        return false;
    };
    bounds.next().is_none() && lower.is_finite() && upper.is_finite() && lower < upper
}

fn sweep_rail_basis(formula: &LawFormula) -> Option<[Vector3; 3]> {
    if formula.variables.is_empty() {
        let name = formula
            .name
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        if name == "null_law" || unit_domain_sweep_formula(&name) {
            return Some([
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            ]);
        }
        return None;
    }
    let name = formula
        .name
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    let inner = name
        .strip_prefix("ROTATE(")
        .and_then(|name| name.strip_suffix(",TRANS1)"))?;
    if !unit_domain_sweep_formula(inner) {
        return None;
    }
    let [LawExpression::TransformVec {
        vectors,
        scale,
        flags,
    }] = formula.variables.as_slice()
    else {
        return None;
    };
    if *scale != 1.0 || *flags != [true, false, false] || vectors[3] != Vector3::new(0.0, 0.0, 0.0)
    {
        return None;
    }
    let transform = Transform {
        rows: [
            [vectors[0].x, vectors[1].x, vectors[2].x, 0.0],
            [vectors[0].y, vectors[1].y, vectors[2].y, 0.0],
            [vectors[0].z, vectors[1].z, vectors[2].z, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    transform
        .is_proper_rigid()
        .then_some([vectors[0], vectors[1], vectors[2]])
}

fn linear_sweep_rail_point(basis: [Vector3; 3], point: Point3) -> Point3 {
    Point3::new(
        point.x * basis[0].x + point.y * basis[1].x + point.z * basis[2].x,
        point.x * basis[0].y + point.y * basis[1].y + point.z * basis[2].y,
        point.x * basis[0].z + point.y * basis[1].z + point.z * basis[2].z,
    )
}

fn linear_sweep_rail_vector(basis: [Vector3; 3], vector: Vector3) -> Vector3 {
    Vector3::new(
        vector.x * basis[0].x + vector.y * basis[1].x + vector.z * basis[2].x,
        vector.x * basis[0].y + vector.y * basis[1].y + vector.z * basis[2].y,
        vector.x * basis[0].z + vector.y * basis[1].z + vector.z * basis[2].z,
    )
}

fn straight_sweep_path_origin(
    index: &crate::index::ModelIndex<'_>,
    spine: &crate::ids::CurveId,
) -> Option<Point3> {
    let curve = index.curves(&spine.0)?;
    match &curve.geometry {
        CurveGeometry::Line { origin, .. } => Some(*origin),
        CurveGeometry::Nurbs(nurbs)
            if nurbs.degree == 1 && nurbs.control_points.len() == 2 && !nurbs.periodic =>
        {
            let [start, _] = nurbs_curve_parameter_domain(nurbs)?;
            curve_point(&curve.geometry, start)
        }
        _ => None,
    }
}

fn point_displacement(point: Point3, origin: Point3) -> Vector3 {
    Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z)
}

fn sweep_tail_interval_contains(interval: [Option<f64>; 2], parameter: f64) -> bool {
    parameter.is_finite()
        && interval[0].is_none_or(|lower| parameter >= lower)
        && interval[1].is_none_or(|upper| parameter <= upper)
}

fn unit_vector_with_derivative(vector: Vector3, derivative: Vector3) -> Option<(Vector3, Vector3)> {
    let length = vector.norm();
    if !length.is_finite() || length <= f64::EPSILON {
        return None;
    }
    let unit = scale_vector(vector, 1.0 / length);
    let normal_component = unit.dot(derivative);
    let unit_derivative = scale_vector(
        vector_sum(&[(1.0, derivative), (-normal_component, unit)]),
        1.0 / length,
    );
    (unit_derivative.x.is_finite()
        && unit_derivative.y.is_finite()
        && unit_derivative.z.is_finite())
    .then_some((unit, unit_derivative))
}

fn sweep_profile_reversed(
    profile_frame: Option<(Point3, Vector3)>,
    spine_tangent: Vector3,
) -> Option<bool> {
    let Some((_, frame_vector)) = profile_frame else {
        return Some(false);
    };
    let frame_vector = unit_axis(frame_vector)?;
    let spine_tangent = unit_axis(spine_tangent)?;
    let alignment = frame_vector.dot(spine_tangent);
    ((alignment.abs() - 1.0).abs() <= EPS_EVAL_SWEEP_PROFILE_FRAME_ALIGNMENT_E9)
        .then_some(alignment < 0.0)
}

fn sweep_profile_differential(
    index: &crate::index::ModelIndex<'_>,
    profile: &crate::ids::CurveId,
    profile_range: [f64; 2],
    reversed: bool,
    parameter: f64,
) -> Option<ModelCurveDifferential> {
    if !sweep_tail_interval_contains([Some(profile_range[0]), Some(profile_range[1])], parameter) {
        return None;
    }
    let profile_span = profile_range[1] - profile_range[0];
    if !profile_span.is_finite() || profile_span <= 0.0 {
        return None;
    }
    let curve = index.curves(&profile.0)?;
    let (native_parameter, parameter_scale) = match &curve.geometry {
        CurveGeometry::Nurbs(nurbs) => {
            let [native_start, native_end] = nurbs_curve_parameter_domain(nurbs)?;
            let native_span = native_end - native_start;
            let fraction = (parameter - profile_range[0]) / profile_span;
            let fraction = if reversed { 1.0 - fraction } else { fraction };
            let native_parameter = native_start + fraction * native_span;
            let parameter_scale = native_span / profile_span * if reversed { -1.0 } else { 1.0 };
            (native_parameter, parameter_scale)
        }
        _ if !reversed => (parameter, 1.0),
        _ => return None,
    };
    let mut differential = model_curve_differential_by_id(index, profile, native_parameter)?;
    differential.tangent = scale_vector(differential.tangent, parameter_scale);
    differential.acceleration =
        scale_vector(differential.acceleration, parameter_scale * parameter_scale);
    Some(differential)
}

fn cacheless_law_sweep_differentials(
    index: &crate::index::ModelIndex<'_>,
    profile: &crate::ids::CurveId,
    spine: &crate::ids::CurveId,
    construction: &crate::geometry::SweepSurfaceConstruction,
    u: f64,
    v: f64,
) -> Option<(
    ModelCurveDifferential,
    ModelCurveDifferential,
    ScalarSweepDifferential,
    Point3,
)> {
    let form = construction.revision_form.as_ref()?;
    if form.tail_enum != 2 {
        return None;
    }
    let path_origin = straight_sweep_path_origin(index, spine)?;
    let SweepSurfaceLayout::LawDriven {
        profile_range,
        profile_frame,
        origin,
        first_law,
        first_range,
        path_mode,
        second_law,
        formula,
        formula_mode,
        trailing_flag,
        ..
    } = &construction.layout
    else {
        return None;
    };
    let parameterization = form.tail_parameterization.as_ref()?;
    let rail_basis = sweep_rail_basis(formula)?;
    let scale = sweep_scale(second_law)?;
    if *path_mode != 1
        || *formula_mode != 0
        || *trailing_flag
        || !sweep_tail_interval_contains(parameterization.u_interval, u)
        || !sweep_tail_interval_contains(parameterization.v_interval, v)
        || !sweep_tail_interval_contains([Some(first_range[0]), Some(first_range[1])], v)
    {
        return None;
    }
    let spine = model_curve_differential_by_id(index, spine, v)?;
    let reversed = sweep_profile_reversed(*profile_frame, spine.tangent)?;
    let profile = sweep_profile_differential(index, profile, *profile_range, reversed, u)?;
    let frame_point = profile_frame.map_or(*origin, |(point, _)| point);
    let mut profile = scale_sweep_profile(profile, frame_point, scale)?;
    profile.point = linear_sweep_rail_point(rail_basis, profile.point);
    profile.tangent = linear_sweep_rail_vector(rail_basis, profile.tangent);
    profile.acceleration = linear_sweep_rail_vector(rail_basis, profile.acceleration);
    let law = scalar_sweep_law_differential(first_law, v)?;
    Some((profile, spine, law, path_origin))
}

fn cacheless_law_sweep_point(
    index: &crate::index::ModelIndex<'_>,
    profile: &crate::ids::CurveId,
    spine: &crate::ids::CurveId,
    construction: &crate::geometry::SweepSurfaceConstruction,
    u: f64,
    v: f64,
) -> Option<Point3> {
    let (profile, spine, law, path_origin) =
        cacheless_law_sweep_differentials(index, profile, spine, construction, u, v)?;
    let (profile_tangent, _) = unit_vector_with_derivative(profile.tangent, profile.acceleration)?;
    let (spine_tangent, _) = unit_vector_with_derivative(spine.tangent, spine.acceleration)?;
    let normal = profile_tangent.cross(spine_tangent);
    Some(offset(
        profile.point,
        &[
            (1.0, point_displacement(spine.point, path_origin)),
            (law.value, normal),
        ],
    ))
}

fn cacheless_law_sweep_partials(
    index: &crate::index::ModelIndex<'_>,
    profile: &crate::ids::CurveId,
    spine: &crate::ids::CurveId,
    construction: &crate::geometry::SweepSurfaceConstruction,
    u: f64,
    v: f64,
) -> Option<SurfacePartials> {
    let (profile, spine, law, path_origin) =
        cacheless_law_sweep_differentials(index, profile, spine, construction, u, v)?;
    let (profile_tangent, profile_tangent_derivative) =
        unit_vector_with_derivative(profile.tangent, profile.acceleration)?;
    let (spine_tangent, spine_tangent_derivative) =
        unit_vector_with_derivative(spine.tangent, spine.acceleration)?;
    let normal = profile_tangent.cross(spine_tangent);
    let normal_u = profile_tangent_derivative.cross(spine_tangent)
        + profile_tangent.cross(spine_tangent_derivative);
    Some(SurfacePartials {
        point: offset(
            profile.point,
            &[
                (1.0, point_displacement(spine.point, path_origin)),
                (law.value, normal),
            ],
        ),
        du: profile.tangent + scale_vector(normal_u, law.value),
        dv: spine.tangent + scale_vector(normal, law.derivative),
    })
}

#[derive(Clone, Copy)]
struct ContactTrackDifferential {
    point: Point3,
    tangent: Vector3,
    normal: Vector3,
    normal_derivative: Option<Vector3>,
}

fn variable_blend_contact_track_differential(
    index: &crate::index::ModelIndex<'_>,
    side: &crate::geometry::RollingBallSide,
    parameter: f64,
) -> Option<ContactTrackDifferential> {
    let surface = side.surface.as_ref()?;
    let pcurve = side.pcurve.as_ref()?;
    let uv = pcurve_uv(pcurve, parameter)?;
    let uv_tangent = pcurve_tangent(pcurve, parameter)?;
    let support = model_surface_partials_by_id(index, surface, uv.u, uv.v)?;
    let normal_derivative = model_surface_second_partials_by_id(index, surface, uv.u, uv.v)
        .and_then(|support| {
            let du = vector_sum(&[(uv_tangent.u, support.duu), (uv_tangent.v, support.duv)]);
            let dv = vector_sum(&[(uv_tangent.u, support.duv), (uv_tangent.v, support.dvv)]);
            let normal = support.du.cross(support.dv);
            let normal_derivative = du.cross(support.dv) + support.du.cross(dv);
            unit_vector_with_derivative(normal, normal_derivative).map(|(_, derivative)| derivative)
        });
    Some(ContactTrackDifferential {
        point: support.point,
        tangent: vector_sum(&[(uv_tangent.u, support.du), (uv_tangent.v, support.dv)]),
        normal: support.du.cross(support.dv).unit()?,
        normal_derivative,
    })
}

fn cacheless_variable_blend_domain_contains(
    construction: &crate::geometry::VariableBlendConstruction,
    u: f64,
    v: f64,
) -> bool {
    let exact_construction = construction.tail_enum == 2 || construction.shape_prefix == 0;
    exact_construction
        && (0.0..=1.0).contains(&u)
        && sweep_tail_interval_contains(construction.slice_range, v)
        && construction
            .tail_parameterization
            .as_ref()
            .is_none_or(|tail| {
                sweep_tail_interval_contains(tail.u_interval, u)
                    && sweep_tail_interval_contains(tail.v_interval, v)
            })
}

fn variable_blend_has_current_cache(
    construction: &crate::geometry::VariableBlendConstruction,
) -> bool {
    construction.shape_prefix > 0 && revision_surface_tail_has_current_cache(construction.tail_enum)
}

fn sweep_has_current_cache(construction: &crate::geometry::SweepSurfaceConstruction) -> bool {
    construction
        .revision_form
        .as_ref()
        .is_some_and(|form| revision_surface_tail_has_current_cache(form.tail_enum))
}

fn revision_surface_tail_has_current_cache(tail_enum: i64) -> bool {
    tail_enum == 0
}

fn surface_cache_evaluation(
    geometry: &crate::geometry::SurfaceGeometry,
    u: f64,
    v: f64,
) -> Option<(Point3, Option<Vector3>)> {
    let partials = surface_partials(geometry, u, v)?;
    Some((partials.point, partials.du.cross(partials.dv).unit()))
}

fn variable_blend_is_zero_radius(value: &crate::geometry::VariableBlendValue) -> bool {
    match &value.payload {
        crate::geometry::VariableBlendValuePayload::TwoEnds {
            parameters: [first_parameter, second_parameter],
            radii: [first_radius, second_radius],
        } => {
            first_parameter.is_finite()
                && second_parameter.is_finite()
                && first_parameter != second_parameter
                && *first_radius == 0.0
                && *second_radius == 0.0
        }
        crate::geometry::VariableBlendValuePayload::Constant { radius, nested, .. } => {
            *radius == 0.0 && variable_blend_is_zero_radius(nested)
        }
        _ => false,
    }
}

fn cacheless_ruled_variable_blend_partials(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction,
    u: f64,
    v: f64,
) -> Option<SurfacePartials> {
    let Some(crate::geometry::VariableBlendCrossSection::RoundedChamfer { radius }) =
        construction.cross_section.as_ref()
    else {
        return None;
    };
    if !cacheless_variable_blend_domain_contains(construction, u, v)
        || radius
            .as_deref()
            .is_some_and(|radius| !variable_blend_is_zero_radius(radius))
    {
        return None;
    }
    let first = variable_blend_contact_track_differential(index, &construction.sides[0], v)?;
    let second = variable_blend_contact_track_differential(index, &construction.sides[1], v)?;
    let chord = Vector3::new(
        second.point.x - first.point.x,
        second.point.y - first.point.y,
        second.point.z - first.point.z,
    );
    Some(SurfacePartials {
        point: offset(first.point, &[(u, chord)]),
        du: chord,
        dv: vector_sum(&[(1.0 - u, first.tangent), (u, second.tangent)]),
    })
}

fn variable_blend_radius(
    value: &crate::geometry::VariableBlendValue,
    parameter: f64,
) -> Option<f64> {
    match &value.payload {
        crate::geometry::VariableBlendValuePayload::TwoEnds {
            parameters: [first_parameter, second_parameter],
            radii: [first_radius, second_radius],
        } => {
            let width = second_parameter - first_parameter;
            if width == 0.0 {
                return None;
            }
            let fraction = (parameter - first_parameter) / width;
            let radius = first_radius + fraction * (second_radius - first_radius);
            radius.is_finite().then_some(radius)
        }
        crate::geometry::VariableBlendValuePayload::Constant { nested, .. } => {
            variable_blend_radius(nested, parameter)
        }
        crate::geometry::VariableBlendValuePayload::Functional { function, .. }
        | crate::geometry::VariableBlendValuePayload::Interpolated { function, .. } => {
            let radius = pcurve_uv(function, parameter)?.u;
            radius.is_finite().then_some(radius)
        }
        _ => None,
    }
}

fn variable_blend_radius_differential(
    value: &crate::geometry::VariableBlendValue,
    parameter: f64,
) -> Option<ScalarSweepDifferential> {
    match &value.payload {
        crate::geometry::VariableBlendValuePayload::TwoEnds {
            parameters: [first_parameter, second_parameter],
            radii: [first_radius, second_radius],
        } => {
            let width = second_parameter - first_parameter;
            if width == 0.0 {
                return None;
            }
            let fraction = (parameter - first_parameter) / width;
            finite_sweep_differential(
                first_radius + fraction * (second_radius - first_radius),
                (second_radius - first_radius) / width,
            )
        }
        crate::geometry::VariableBlendValuePayload::Constant { nested, .. } => {
            variable_blend_radius_differential(nested, parameter)
        }
        crate::geometry::VariableBlendValuePayload::Functional { function, .. }
        | crate::geometry::VariableBlendValuePayload::Interpolated { function, .. } => {
            let radius = pcurve_uv(function, parameter)?.u;
            let derivative = pcurve_tangent(function, parameter)?.u;
            finite_sweep_differential(radius, derivative)
        }
        _ => None,
    }
}

fn minor_circular_arc_point(
    center: Point3,
    first: Point3,
    second: Point3,
    radius: f64,
    u: f64,
) -> Option<Point3> {
    if u == 0.0 {
        return Some(first);
    }
    if u == 1.0 {
        return Some(second);
    }
    let first_radius =
        Vector3::new(first.x - center.x, first.y - center.y, first.z - center.z).unit()?;
    let second_radius = Vector3::new(
        second.x - center.x,
        second.y - center.y,
        second.z - center.z,
    )
    .unit()?;
    let axis = first_radius.cross(second_radius).unit()?;
    let angle = first_radius.dot(second_radius).clamp(-1.0, 1.0).acos();
    let section_angle = u * angle;
    let radial = vector_sum(&[
        (section_angle.cos(), first_radius),
        (section_angle.sin(), axis.cross(first_radius)),
    ]);
    Some(offset(center, &[(radius, radial)]))
}

fn cacheless_circular_variable_blend_point(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction,
    u: f64,
    v: f64,
) -> Option<Point3> {
    if !cacheless_variable_blend_domain_contains(construction, u, v)
        || construction.radius_kind != crate::geometry::VariableBlendRadiusKind::SingleRadius
        || !matches!(
            construction.cross_section,
            None | Some(crate::geometry::VariableBlendCrossSection::Circular)
        )
    {
        return None;
    }
    let first = variable_blend_contact_track_differential(index, &construction.sides[0], v)?;
    let second = variable_blend_contact_track_differential(index, &construction.sides[1], v)?;
    if u == 0.0 {
        return Some(first.point);
    }
    if u == 1.0 {
        return Some(second.point);
    }
    let section = cacheless_circular_variable_blend_section(index, construction, u, v)?;
    minor_circular_arc_point(
        section.center,
        section.first.point,
        section.second.point,
        section.radius,
        u,
    )
}

struct CircularVariableBlendSection {
    center: Point3,
    signs: [f64; 2],
    first: ContactTrackDifferential,
    second: ContactTrackDifferential,
    radius: f64,
    radius_derivative: Option<f64>,
    tolerance: f64,
}

fn cacheless_circular_variable_blend_section(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction,
    u: f64,
    v: f64,
) -> Option<CircularVariableBlendSection> {
    if !cacheless_variable_blend_domain_contains(construction, u, v)
        || construction.radius_kind != crate::geometry::VariableBlendRadiusKind::SingleRadius
        || !matches!(
            construction.cross_section,
            None | Some(crate::geometry::VariableBlendCrossSection::Circular)
        )
    {
        return None;
    }
    let signed_radius = variable_blend_radius(&construction.first_value, v)?;
    let radius = signed_radius.abs();
    if !radius.is_finite() || radius <= f64::EPSILON {
        return None;
    }
    let radius_derivative = variable_blend_radius_differential(&construction.first_value, v)
        .filter(|differential| differential.value.signum() == signed_radius.signum())
        .map(|differential| differential.derivative * signed_radius.signum());
    let first = variable_blend_contact_track_differential(index, &construction.sides[0], v)?;
    let second = variable_blend_contact_track_differential(index, &construction.sides[1], v)?;
    let scale = radius
        .max(first.point.x.abs())
        .max(first.point.y.abs())
        .max(first.point.z.abs())
        .max(second.point.x.abs())
        .max(second.point.y.abs())
        .max(second.point.z.abs());
    let tolerance = index
        .ir()
        .tolerances
        .linear
        .max(256.0 * f64::EPSILON * scale.max(1.0));

    let mut best = None;
    let mut second_best_residual = f64::INFINITY;
    for first_sign in [-1.0, 1.0] {
        for second_sign in [-1.0, 1.0] {
            let first_center = offset(first.point, &[(first_sign * radius, first.normal)]);
            let second_center = offset(second.point, &[(second_sign * radius, second.normal)]);
            let residual = point_displacement(second_center, first_center).norm();
            if best
                .as_ref()
                .is_none_or(|(_, _, best_residual)| residual < *best_residual)
            {
                if let Some((_, _, best_residual)) = best {
                    second_best_residual = best_residual;
                }
                best = Some((
                    Point3::new(
                        (first_center.x + second_center.x) * 0.5,
                        (first_center.y + second_center.y) * 0.5,
                        (first_center.z + second_center.z) * 0.5,
                    ),
                    [first_sign, second_sign],
                    residual,
                ));
            } else if residual < second_best_residual {
                second_best_residual = residual;
            }
        }
    }
    let (center, signs, residual) = best?;
    if residual > tolerance || second_best_residual <= tolerance {
        return None;
    }
    Some(CircularVariableBlendSection {
        center,
        signs,
        first,
        second,
        radius,
        radius_derivative,
        tolerance,
    })
}

fn cacheless_constant_rolling_ball_point(
    index: &crate::index::ModelIndex<'_>,
    supports: &[Option<crate::geometry::BlendSupport>; 2],
    radius: &crate::geometry::BlendRadiusLaw,
    cross_section: &crate::geometry::BlendCrossSection,
    native: &crate::geometry::RollingBallConstruction,
    u: f64,
    v: f64,
) -> Option<Point3> {
    let section = cacheless_constant_rolling_ball_section(
        index,
        supports,
        radius,
        cross_section,
        native,
        u,
        v,
    )?;
    minor_circular_arc_point(
        section.center,
        section.first.point,
        section.second.point,
        section.radius,
        u,
    )
}

struct ConstantRollingBallSection {
    center: Point3,
    center_tangent: Option<Vector3>,
    first: ContactTrackDifferential,
    second: ContactTrackDifferential,
    radius: f64,
}

fn cacheless_constant_rolling_ball_section(
    index: &crate::index::ModelIndex<'_>,
    supports: &[Option<crate::geometry::BlendSupport>; 2],
    radius: &crate::geometry::BlendRadiusLaw,
    cross_section: &crate::geometry::BlendCrossSection,
    native: &crate::geometry::RollingBallConstruction,
    u: f64,
    v: f64,
) -> Option<ConstantRollingBallSection> {
    let crate::geometry::BlendRadiusLaw::Constant { signed_radius } = radius else {
        return None;
    };
    if native.tail_enum != 2
        || native.third.is_some()
        || *cross_section != crate::geometry::BlendCrossSection::Circular
        || !(0.0..=1.0).contains(&u)
        || !sweep_tail_interval_contains(native.slice_range, v)
        || !sweep_tail_interval_contains(native.u_range, u)
        || !sweep_tail_interval_contains(native.v_range, v)
        || !native.tail_parameterization.as_ref().is_some_and(|tail| {
            sweep_tail_interval_contains(tail.u_interval, u)
                && sweep_tail_interval_contains(tail.v_interval, v)
        })
    {
        return None;
    }
    let radius = signed_radius.abs();
    if !radius.is_finite() || radius <= f64::EPSILON {
        return None;
    }
    for (support, side) in supports.iter().zip(native.sides.iter()) {
        if support.as_ref().is_some_and(|support| {
            side.surface
                .as_ref()
                .is_some_and(|surface| *surface != support.surface)
        }) {
            return None;
        }
    }
    let first = variable_blend_contact_track_differential(index, &native.sides[0], v)?;
    let second = variable_blend_contact_track_differential(index, &native.sides[1], v)?;
    let center = model_curve_point_by_id(index, &native.slice, v)?;
    let center_tangent = model_curve_differential_by_id(index, &native.slice, v)
        .map(|differential| differential.tangent);
    let tolerance = index.ir().tolerances.linear.max(
        256.0
            * f64::EPSILON
            * radius
                .max(first.point.x.abs())
                .max(first.point.y.abs())
                .max(first.point.z.abs())
                .max(second.point.x.abs())
                .max(second.point.y.abs())
                .max(second.point.z.abs())
                .max(1.0),
    );
    let radius_error = |point: Point3| {
        (Vector3::new(point.x - center.x, point.y - center.y, point.z - center.z).norm() - radius)
            .abs()
    };
    if native
        .offsets
        .iter()
        .any(|offset| !offset.is_finite() || (*offset - signed_radius).abs() > tolerance)
        || radius_error(first.point) > tolerance
        || radius_error(second.point) > tolerance
    {
        return None;
    }
    Some(ConstantRollingBallSection {
        center,
        center_tangent,
        first,
        second,
        radius,
    })
}

fn cacheless_circular_variable_blend_partials(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction,
    u: f64,
    v: f64,
) -> Option<SurfacePartials> {
    let section = cacheless_circular_variable_blend_section(index, construction, u, v)?;
    let radius_derivative = section.radius_derivative?;
    let first_normal_derivative = section.first.normal_derivative?;
    let second_normal_derivative = section.second.normal_derivative?;
    let first_center_tangent = vector_sum(&[
        (1.0, section.first.tangent),
        (section.signs[0] * radius_derivative, section.first.normal),
        (section.signs[0] * section.radius, first_normal_derivative),
    ]);
    let second_center_tangent = vector_sum(&[
        (1.0, section.second.tangent),
        (section.signs[1] * radius_derivative, section.second.normal),
        (section.signs[1] * section.radius, second_normal_derivative),
    ]);
    if vector_sum(&[(1.0, second_center_tangent), (-1.0, first_center_tangent)]).norm()
        > section.tolerance
    {
        return None;
    }
    let center_tangent = scale_vector(
        vector_sum(&[(1.0, first_center_tangent), (1.0, second_center_tangent)]),
        0.5,
    );
    circular_arc_partials(
        section.center,
        center_tangent,
        &section.first,
        &section.second,
        section.radius,
        radius_derivative,
        u,
    )
}

fn cacheless_constant_rolling_ball_partials(
    index: &crate::index::ModelIndex<'_>,
    supports: &[Option<crate::geometry::BlendSupport>; 2],
    radius: &crate::geometry::BlendRadiusLaw,
    cross_section: &crate::geometry::BlendCrossSection,
    native: &crate::geometry::RollingBallConstruction,
    u: f64,
    v: f64,
) -> Option<SurfacePartials> {
    let section = cacheless_constant_rolling_ball_section(
        index,
        supports,
        radius,
        cross_section,
        native,
        u,
        v,
    )?;
    constant_rolling_ball_partials(&section, u)
}

fn constant_rolling_ball_partials(
    section: &ConstantRollingBallSection,
    u: f64,
) -> Option<SurfacePartials> {
    let center_tangent = section.center_tangent?;
    circular_arc_partials(
        section.center,
        center_tangent,
        &section.first,
        &section.second,
        section.radius,
        0.0,
        u,
    )
}

fn circular_arc_partials(
    center: Point3,
    center_tangent: Vector3,
    first: &ContactTrackDifferential,
    second: &ContactTrackDifferential,
    radius: f64,
    radius_derivative: f64,
    u: f64,
) -> Option<SurfacePartials> {
    let first_delta = point_displacement(first.point, center);
    let second_delta = point_displacement(second.point, center);
    let first_delta_v = vector_sum(&[(1.0, first.tangent), (-1.0, center_tangent)]);
    let second_delta_v = vector_sum(&[(1.0, second.tangent), (-1.0, center_tangent)]);
    let (first_radius, first_radius_v) = unit_vector_with_derivative(first_delta, first_delta_v)?;
    let (second_radius, second_radius_v) =
        unit_vector_with_derivative(second_delta, second_delta_v)?;
    let cosine = first_radius.dot(second_radius).clamp(-1.0, 1.0);
    let sine = (1.0 - cosine * cosine).max(0.0).sqrt();
    if sine <= f64::EPSILON {
        return None;
    }
    let angle = cosine.acos();
    let cosine_v = first_radius_v.dot(second_radius) + first_radius.dot(second_radius_v);
    let angle_v = -cosine_v / sine;
    let first_angle = (1.0 - u) * angle;
    let second_angle = u * angle;
    let first_sine = first_angle.sin();
    let second_sine = second_angle.sin();
    let first_weight = first_sine / sine;
    let second_weight = second_sine / sine;
    let radial = vector_sum(&[(first_weight, first_radius), (second_weight, second_radius)]);
    let radial_u = vector_sum(&[
        (-angle * first_angle.cos() / sine, first_radius),
        (angle * second_angle.cos() / sine, second_radius),
    ]);
    let sine_squared = sine * sine;
    let first_weight_angle =
        ((1.0 - u) * first_angle.cos() * sine - first_sine * cosine) / sine_squared;
    let second_weight_angle = (u * second_angle.cos() * sine - second_sine * cosine) / sine_squared;
    let radial_v = vector_sum(&[
        (first_weight, first_radius_v),
        (second_weight, second_radius_v),
        (
            angle_v,
            vector_sum(&[
                (first_weight_angle, first_radius),
                (second_weight_angle, second_radius),
            ]),
        ),
    ]);
    Some(SurfacePartials {
        point: offset(center, &[(radius, radial)]),
        du: scale_vector(radial_u, radius),
        dv: vector_sum(&[
            (1.0, center_tangent),
            (radius_derivative, radial),
            (radius, radial_v),
        ]),
    })
}

fn cacheless_variable_blend_point(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction,
    u: f64,
    v: f64,
) -> Option<Point3> {
    cacheless_ruled_variable_blend_partials(index, construction, u, v)
        .map(|partials| partials.point)
        .or_else(|| cacheless_circular_variable_blend_point(index, construction, u, v))
}

fn model_ruled_surface_partials(
    index: &crate::index::ModelIndex<'_>,
    first: &crate::ids::CurveId,
    second: &crate::ids::CurveId,
    u: f64,
    v: f64,
) -> Option<SurfaceSecondPartials> {
    if !v.is_finite() {
        return None;
    }
    let first = model_curve_differential_by_id(index, first, u)?;
    let second = model_curve_differential_by_id(index, second, u)?;
    let point = offset(
        first.point,
        &[(v, point_displacement(second.point, first.point))],
    );
    let blend = |first: Vector3, second: Vector3| vector_sum(&[(1.0 - v, first), (v, second)]);
    let partials = SurfaceSecondPartials {
        point,
        du: blend(first.tangent, second.tangent),
        dv: point_displacement(second.point, first.point),
        duu: blend(first.acceleration, second.acceleration),
        duv: vector_sum(&[(-1.0, first.tangent), (1.0, second.tangent)]),
        dvv: Vector3::new(0.0, 0.0, 0.0),
    };
    surface_second_partials_are_finite(partials).then_some(partials)
}

fn model_sum_surface_partials(
    index: &crate::index::ModelIndex<'_>,
    first: &crate::ids::CurveId,
    second: &crate::ids::CurveId,
    basepoint: Vector3,
    u: f64,
    v: f64,
) -> Option<SurfaceSecondPartials> {
    if ![basepoint.x, basepoint.y, basepoint.z]
        .into_iter()
        .all(f64::is_finite)
    {
        return None;
    }
    let first = model_curve_differential_by_id(index, first, u)?;
    let second = model_curve_differential_by_id(index, second, v)?;
    let point = Point3::new(
        first.point.x + second.point.x - basepoint.x,
        first.point.y + second.point.y - basepoint.y,
        first.point.z + second.point.z - basepoint.z,
    );
    let partials = SurfaceSecondPartials {
        point,
        du: first.tangent,
        dv: second.tangent,
        duu: first.acceleration,
        duv: Vector3::new(0.0, 0.0, 0.0),
        dvv: second.acceleration,
    };
    surface_second_partials_are_finite(partials).then_some(partials)
}

fn surface_second_partials_are_finite(partials: SurfaceSecondPartials) -> bool {
    let finite_point = |point: Point3| [point.x, point.y, point.z].into_iter().all(f64::is_finite);
    let finite_vector = |vector: Vector3| {
        [vector.x, vector.y, vector.z]
            .into_iter()
            .all(f64::is_finite)
    };
    finite_point(partials.point)
        && finite_vector(partials.du)
        && finite_vector(partials.dv)
        && finite_vector(partials.duu)
        && finite_vector(partials.duv)
        && finite_vector(partials.dvv)
}

/// Evaluate a surface carrier selected by arena id.
pub fn model_surface_point_by_id(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Option<Point3> {
    struct SurfaceEvaluation {
        point: Point3,
        oriented_normal: Option<Vector3>,
    }

    fn evaluate(
        index: &crate::index::ModelIndex<'_>,
        surface_id: &crate::ids::SurfaceId,
        u: f64,
        v: f64,
        visiting: &mut Vec<crate::ids::SurfaceId>,
    ) -> Option<SurfaceEvaluation> {
        if visiting.contains(surface_id) {
            return None;
        }
        visiting.push(surface_id.clone());
        let surface = index.surfaces(&surface_id.0)?;
        let procedural = index.procedural_surface_for_surface(&surface_id.0);
        let carrier_interval =
            procedural.and_then(|procedural| record_u_interval(procedural.record_bounds));
        let result = match procedural.map(|procedural| &procedural.definition) {
            Some(ProceduralSurfaceDefinition::AxisRevolution {
                directrix,
                axis_origin,
                axis_direction,
            }) => {
                model_axis_revolution_point(index, directrix, *axis_origin, *axis_direction, u, v)
                    .map(|point| SurfaceEvaluation {
                        point,
                        oriented_normal: None,
                    })
            }
            Some(ProceduralSurfaceDefinition::Extrusion {
                directrix,
                direction,
                parameter_interval,
                revision_form,
                ..
            }) => model_native_extrusion_partials(
                index,
                directrix,
                *direction,
                *parameter_interval,
                carrier_interval,
                extrusion_directrix_reversed(revision_form.as_ref()),
                u,
                v,
            )
            .map(|partials| SurfaceEvaluation {
                point: partials.point,
                oriented_normal: None,
            }),
            Some(ProceduralSurfaceDefinition::LinearSweep {
                directrix,
                direction,
            }) => model_curve_point_by_id(index, directrix, u).map(|point| SurfaceEvaluation {
                point: offset(point, &[(v, *direction)]),
                oriented_normal: None,
            }),
            Some(ProceduralSurfaceDefinition::Revolution {
                directrix,
                axis_origin,
                axis_direction,
                angular_interval,
                angular_parameter_interval,
                parameter_interval,
                transposed,
                ..
            }) => model_native_revolution_partials(
                index,
                directrix,
                *axis_origin,
                *axis_direction,
                *angular_interval,
                *angular_parameter_interval,
                *parameter_interval,
                carrier_interval,
                *transposed,
                u,
                v,
            )
            .map(|partials| SurfaceEvaluation {
                point: partials.point,
                oriented_normal: None,
            }),
            Some(ProceduralSurfaceDefinition::Ruled { first, second }) => {
                model_ruled_surface_partials(index, first, second, u, v).map(|partials| {
                    SurfaceEvaluation {
                        point: partials.point,
                        oriented_normal: None,
                    }
                })
            }
            Some(ProceduralSurfaceDefinition::Sum {
                first,
                second,
                basepoint,
                ..
            }) => {
                model_sum_surface_partials(index, first, second, *basepoint, u, v).map(|partials| {
                    SurfaceEvaluation {
                        point: partials.point,
                        oriented_normal: None,
                    }
                })
            }
            Some(ProceduralSurfaceDefinition::Sweep {
                profile,
                spine,
                native: Some(construction),
            }) => cacheless_law_sweep_point(index, profile, spine, construction, u, v)
                .map(|point| SurfaceEvaluation {
                    point,
                    oriented_normal: None,
                })
                .or_else(|| {
                    if !sweep_has_current_cache(construction) {
                        return None;
                    }
                    let (point, oriented_normal) =
                        surface_cache_evaluation(&surface.geometry, u, v)?;
                    Some(SurfaceEvaluation {
                        point,
                        oriented_normal,
                    })
                }),
            Some(ProceduralSurfaceDefinition::VariableBlend { construction }) => {
                cacheless_variable_blend_point(index, construction, u, v)
                    .map(|point| SurfaceEvaluation {
                        point,
                        oriented_normal: None,
                    })
                    .or_else(|| {
                        if !variable_blend_has_current_cache(construction) {
                            return None;
                        }
                        let (point, oriented_normal) =
                            surface_cache_evaluation(&surface.geometry, u, v)?;
                        Some(SurfaceEvaluation {
                            point,
                            oriented_normal,
                        })
                    })
            }
            Some(ProceduralSurfaceDefinition::Blend {
                supports,
                radius,
                cross_section,
                native: Some(native),
                ..
            }) => {
                if let Some(point) = cacheless_constant_rolling_ball_point(
                    index,
                    supports,
                    radius,
                    cross_section,
                    native,
                    u,
                    v,
                ) {
                    let oriented_normal = cacheless_constant_rolling_ball_partials(
                        index,
                        supports,
                        radius,
                        cross_section,
                        native,
                        u,
                        v,
                    )
                    .and_then(|partials| partials.du.cross(partials.dv).unit());
                    Some(SurfaceEvaluation {
                        point,
                        oriented_normal,
                    })
                } else if revision_surface_tail_has_current_cache(native.tail_enum) {
                    let (point, oriented_normal) =
                        surface_cache_evaluation(&surface.geometry, u, v)?;
                    Some(SurfaceEvaluation {
                        point,
                        oriented_normal,
                    })
                } else {
                    None
                }
            }
            Some(ProceduralSurfaceDefinition::CurveBounded { support, .. }) => {
                evaluate(index, support, u, v, visiting)
            }
            Some(ProceduralSurfaceDefinition::Replica { source, transform }) => {
                let mut evaluation = evaluate(index, source, u, v, visiting)?;
                let partials = model_surface_partials_by_id(index, source, u, v)?;
                let du = affine_vector(*transform, partials.du);
                let dv = affine_vector(*transform, partials.dv);
                let normal = du.cross(dv);
                let magnitude = normal.norm();
                evaluation.point = affine_point(*transform, evaluation.point);
                evaluation.oriented_normal =
                    (magnitude.is_finite() && magnitude > 0.0).then(|| {
                        Vector3::new(
                            normal.x / magnitude,
                            normal.y / magnitude,
                            normal.z / magnitude,
                        )
                    });
                Some(evaluation)
            }
            Some(ProceduralSurfaceDefinition::Subset {
                support,
                parameter_ranges,
                u_sense,
                v_sense,
            }) => {
                let (support_u, support_v, u_derivative, v_derivative) =
                    subset_support_parameters_with_derivatives(
                        u,
                        v,
                        *parameter_ranges,
                        *u_sense,
                        *v_sense,
                    )?;
                let mut evaluation = evaluate(index, support, support_u, support_v, visiting)?;
                if u_derivative * v_derivative < 0.0 {
                    evaluation.oriented_normal = evaluation
                        .oriented_normal
                        .map(|normal| scale_vector(normal, -1.0));
                }
                Some(evaluation)
            }
            Some(ProceduralSurfaceDefinition::ParallelOffset {
                support, distance, ..
            }) => {
                let support = evaluate(index, support, u, v, visiting)?;
                let normal = support.oriented_normal?;
                Some(SurfaceEvaluation {
                    point: offset(support.point, &[(*distance, normal)]),
                    oriented_normal: Some(normal),
                })
            }
            Some(ProceduralSurfaceDefinition::Offset {
                support, distance, ..
            }) => {
                let support = evaluate(index, support, u, v, visiting)?;
                let normal = support.oriented_normal?;
                Some(SurfaceEvaluation {
                    point: offset(support.point, &[(*distance, normal)]),
                    oriented_normal: Some(normal),
                })
            }
            _ if procedural.is_some() => model_surface_point(index.ir(), &surface.geometry, u, v)
                .map(|point| SurfaceEvaluation {
                    point,
                    oriented_normal: None,
                }),
            _ => surface_partials(&surface.geometry, u, v).map(|partials| {
                let normal = partials.du.cross(partials.dv);
                let magnitude = normal.norm();
                let oriented_normal = (magnitude.is_finite() && magnitude > 0.0).then(|| {
                    Vector3::new(
                        normal.x / magnitude,
                        normal.y / magnitude,
                        normal.z / magnitude,
                    )
                });
                SurfaceEvaluation {
                    point: partials.point,
                    oriented_normal,
                }
            }),
        };
        visiting.pop();
        result
    }

    evaluate(index, surface, u, v, &mut Vec::new()).map(|evaluation| evaluation.point)
}

/// Evaluate an arena-selected direct, trimmed, or uniform-offset surface and
/// its exact first partial derivatives.
///
/// Subsets map the support parameterization through a linear local domain;
/// offsets follow the support's oriented normal. The recursive carrier walk
/// preserves both contracts before evaluating the final point and partials.
pub fn model_surface_partials_by_id(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Option<SurfacePartials> {
    if let Some(ProceduralSurfaceDefinition::Blend {
        supports,
        radius,
        cross_section,
        native: Some(native),
        ..
    }) = index
        .procedural_surface_for_surface(&surface.0)
        .map(|procedural| &procedural.definition)
    {
        if let Some(partials) = cacheless_constant_rolling_ball_partials(
            index,
            supports,
            radius,
            cross_section,
            native,
            u,
            v,
        ) {
            return Some(partials);
        }
        if !revision_surface_tail_has_current_cache(native.tail_enum) {
            return None;
        }
    }
    if let Some(ProceduralSurfaceDefinition::VariableBlend { construction }) = index
        .procedural_surface_for_surface(&surface.0)
        .map(|procedural| &procedural.definition)
    {
        if let Some(partials) = cacheless_ruled_variable_blend_partials(index, construction, u, v) {
            return Some(partials);
        }
        if let Some(partials) =
            cacheless_circular_variable_blend_partials(index, construction, u, v)
        {
            return Some(partials);
        }
        if !variable_blend_has_current_cache(construction) {
            return None;
        }
    }
    if let Some(ProceduralSurfaceDefinition::Sweep {
        profile,
        spine,
        native: Some(construction),
    }) = index
        .procedural_surface_for_surface(&surface.0)
        .map(|procedural| &procedural.definition)
    {
        if let Some(partials) =
            cacheless_law_sweep_partials(index, profile, spine, construction, u, v)
        {
            return Some(partials);
        }
        if !sweep_has_current_cache(construction) {
            return None;
        }
    }
    model_surface_second_partials_by_id(index, surface, u, v).map(|partials| SurfacePartials {
        point: partials.point,
        du: partials.du,
        dv: partials.dv,
    })
}

fn model_surface_second_partials_by_id(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Option<SurfaceSecondPartials> {
    let mapping = model_surface_mapping(index, surface, u, v, &mut Vec::new())?;
    let mut partials = if mapping.offset_distance == 0.0 {
        mapping.base
    } else {
        offset_surface_second_partials(mapping.base, mapping.offset_distance)?
    };
    partials.du = scale_vector(partials.du, mapping.u_scale);
    partials.dv = scale_vector(partials.dv, mapping.v_scale);
    partials.duu = scale_vector(partials.duu, mapping.u_scale * mapping.u_scale);
    partials.duv = scale_vector(partials.duv, mapping.u_scale * mapping.v_scale);
    partials.dvv = scale_vector(partials.dvv, mapping.v_scale * mapping.v_scale);
    Some(partials)
}

#[derive(Debug, Clone, Copy)]
struct SurfaceMapping {
    /// Direct support derivatives at the mapped support coordinates.
    base: SurfaceSecondPartials,
    /// Signed distance from `base` to the evaluated surface.
    offset_distance: f64,
    /// Derivative of support U/V with respect to the evaluated U/V.
    u_scale: f64,
    v_scale: f64,
    /// Support normal orientation relative to the direct base normal.
    orientation: f64,
}

fn model_surface_mapping(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
    visiting: &mut Vec<crate::ids::SurfaceId>,
) -> Option<SurfaceMapping> {
    if visiting.contains(surface) {
        return None;
    }
    visiting.push(surface.clone());
    let carrier = index.surfaces(&surface.0)?;
    let procedural = index.procedural_surface_for_surface(&surface.0);
    let carrier_interval =
        procedural.and_then(|procedural| record_u_interval(procedural.record_bounds));
    let result = match procedural.map(|procedural| &procedural.definition) {
        Some(ProceduralSurfaceDefinition::AxisRevolution {
            directrix,
            axis_origin,
            axis_direction,
        }) => Some(SurfaceMapping {
            base: model_axis_revolution_partials(
                index,
                directrix,
                *axis_origin,
                *axis_direction,
                u,
                v,
            )?,
            offset_distance: 0.0,
            u_scale: 1.0,
            v_scale: 1.0,
            orientation: 1.0,
        }),
        Some(ProceduralSurfaceDefinition::Extrusion {
            directrix,
            direction,
            parameter_interval,
            revision_form,
            ..
        }) => Some(SurfaceMapping {
            base: model_native_extrusion_partials(
                index,
                directrix,
                *direction,
                *parameter_interval,
                carrier_interval,
                extrusion_directrix_reversed(revision_form.as_ref()),
                u,
                v,
            )?,
            offset_distance: 0.0,
            u_scale: 1.0,
            v_scale: 1.0,
            orientation: 1.0,
        }),
        Some(ProceduralSurfaceDefinition::LinearSweep {
            directrix,
            direction,
        }) => {
            let differential = model_curve_differential_by_id(index, directrix, u)?;
            let zero = Vector3::new(0.0, 0.0, 0.0);
            Some(SurfaceMapping {
                base: SurfaceSecondPartials {
                    point: offset(differential.point, &[(v, *direction)]),
                    du: differential.tangent,
                    dv: *direction,
                    duu: differential.acceleration,
                    duv: zero,
                    dvv: zero,
                },
                offset_distance: 0.0,
                u_scale: 1.0,
                v_scale: 1.0,
                orientation: 1.0,
            })
        }
        Some(ProceduralSurfaceDefinition::Revolution {
            directrix,
            axis_origin,
            axis_direction,
            angular_interval,
            angular_parameter_interval,
            parameter_interval,
            transposed,
            ..
        }) => Some(SurfaceMapping {
            base: model_native_revolution_partials(
                index,
                directrix,
                *axis_origin,
                *axis_direction,
                *angular_interval,
                *angular_parameter_interval,
                *parameter_interval,
                carrier_interval,
                *transposed,
                u,
                v,
            )?,
            offset_distance: 0.0,
            u_scale: 1.0,
            v_scale: 1.0,
            orientation: 1.0,
        }),
        Some(ProceduralSurfaceDefinition::Ruled { first, second }) => Some(SurfaceMapping {
            base: model_ruled_surface_partials(index, first, second, u, v)?,
            offset_distance: 0.0,
            u_scale: 1.0,
            v_scale: 1.0,
            orientation: 1.0,
        }),
        Some(ProceduralSurfaceDefinition::Sum {
            first,
            second,
            basepoint,
            ..
        }) => Some(SurfaceMapping {
            base: model_sum_surface_partials(index, first, second, *basepoint, u, v)?,
            offset_distance: 0.0,
            u_scale: 1.0,
            v_scale: 1.0,
            orientation: 1.0,
        }),
        Some(ProceduralSurfaceDefinition::CurveBounded { support, .. }) => {
            model_surface_mapping(index, support, u, v, visiting)
        }
        Some(ProceduralSurfaceDefinition::Replica { source, transform }) => {
            let source = model_surface_mapping(index, source, u, v, visiting)?;
            let base = if source.offset_distance == 0.0 {
                source.base
            } else {
                offset_surface_second_partials(source.base, source.offset_distance)?
            };
            Some(SurfaceMapping {
                base: transform_surface_second_partials(base, *transform),
                offset_distance: 0.0,
                u_scale: source.u_scale,
                v_scale: source.v_scale,
                orientation: source.orientation * affine_orientation(*transform),
            })
        }
        Some(ProceduralSurfaceDefinition::Subset {
            support,
            parameter_ranges,
            u_sense,
            v_sense,
        }) => {
            let (support_u, support_v, u_derivative, v_derivative) =
                subset_support_parameters_with_derivatives(
                    u,
                    v,
                    *parameter_ranges,
                    *u_sense,
                    *v_sense,
                )?;
            let support = model_surface_mapping(index, support, support_u, support_v, visiting)?;
            Some(SurfaceMapping {
                base: support.base,
                offset_distance: support.offset_distance,
                u_scale: support.u_scale * u_derivative,
                v_scale: support.v_scale * v_derivative,
                orientation: support.orientation * u_derivative * v_derivative,
            })
        }
        Some(
            ProceduralSurfaceDefinition::ParallelOffset {
                support, distance, ..
            }
            | ProceduralSurfaceDefinition::Offset {
                support, distance, ..
            },
        ) => {
            let support = model_surface_mapping(index, support, u, v, visiting)?;
            Some(SurfaceMapping {
                offset_distance: support.offset_distance + *distance * support.orientation,
                ..support
            })
        }
        _ => {
            let base = surface_second_partials(&carrier.geometry, u, v)?;
            Some(SurfaceMapping {
                base,
                offset_distance: 0.0,
                u_scale: 1.0,
                v_scale: 1.0,
                orientation: 1.0,
            })
        }
    };
    visiting.pop();
    result
}

fn subset_support_parameters_with_derivatives(
    u: f64,
    v: f64,
    parameter_ranges: [[f64; 2]; 2],
    u_sense: Option<bool>,
    v_sense: Option<bool>,
) -> Option<(f64, f64, f64, f64)> {
    let (support_u, u_derivative) = subset_parameter(parameter_ranges[0], u, u_sense)?;
    let (support_v, v_derivative) = subset_parameter(parameter_ranges[1], v, v_sense)?;
    Some((support_u, support_v, u_derivative, v_derivative))
}

fn subset_parameter(range: [f64; 2], parameter: f64, sense: Option<bool>) -> Option<(f64, f64)> {
    let span = (range[1] - range[0]).abs();
    if !range[0].is_finite()
        || !range[1].is_finite()
        || !parameter.is_finite()
        || span == 0.0
        || parameter < 0.0
        || parameter > span
    {
        return None;
    }
    let agrees = sense.unwrap_or(range[1] >= range[0]);
    let derivative = if agrees { 1.0 } else { -1.0 };
    Some((range[0] + derivative * parameter, derivative))
}

fn offset_surface_second_partials(
    base: SurfaceSecondPartials,
    distance: f64,
) -> Option<SurfaceSecondPartials> {
    let normal_vector = base.du.cross(base.dv);
    let normal_magnitude = normal_vector.norm();
    if !normal_magnitude.is_finite() || normal_magnitude == 0.0 || !distance.is_finite() {
        return None;
    }
    let normal = Vector3::new(
        normal_vector.x / normal_magnitude,
        normal_vector.y / normal_magnitude,
        normal_vector.z / normal_magnitude,
    );
    let normal_u_numerator = vector_sum(&[
        (1.0, base.duu.cross(base.dv)),
        (1.0, base.du.cross(base.duv)),
    ]);
    let normal_v_numerator = vector_sum(&[
        (1.0, base.duv.cross(base.dv)),
        (1.0, base.du.cross(base.dvv)),
    ]);
    let unit_normal_derivative = |derivative: Vector3| {
        let normal_component =
            normal.x * derivative.x + normal.y * derivative.y + normal.z * derivative.z;
        Vector3::new(
            (derivative.x - normal_component * normal.x) / normal_magnitude,
            (derivative.y - normal_component * normal.y) / normal_magnitude,
            (derivative.z - normal_component * normal.z) / normal_magnitude,
        )
    };
    let normal_u = unit_normal_derivative(normal_u_numerator);
    let normal_v = unit_normal_derivative(normal_v_numerator);
    Some(SurfaceSecondPartials {
        point: Point3::new(
            base.point.x + distance * normal.x,
            base.point.y + distance * normal.y,
            base.point.z + distance * normal.z,
        ),
        du: Vector3::new(
            base.du.x + distance * normal_u.x,
            base.du.y + distance * normal_u.y,
            base.du.z + distance * normal_u.z,
        ),
        dv: Vector3::new(
            base.dv.x + distance * normal_v.x,
            base.dv.y + distance * normal_v.y,
            base.dv.z + distance * normal_v.z,
        ),
        duu: base.duu,
        duv: base.duv,
        dvv: base.dvv,
    })
}

fn polyline_point(points: &[Point3], parameters: Option<&[f64]>, t: f64) -> Option<Point3> {
    if points.len() < 2 || !t.is_finite() {
        return None;
    }
    let implicit;
    let parameters = if let Some(parameters) = parameters {
        if parameters.len() != points.len() {
            return None;
        }
        parameters
    } else {
        implicit = (0..points.len())
            .map(|index| index as f64)
            .collect::<Vec<_>>();
        &implicit
    };
    let segment = parameters.windows(2).position(|window| {
        (t >= window[0] && t <= window[1]) || (t <= window[0] && t >= window[1])
    })?;
    let width = parameters[segment + 1] - parameters[segment];
    if width == 0.0 || !width.is_finite() {
        return None;
    }
    let fraction = (t - parameters[segment]) / width;
    let start = points[segment];
    let end = points[segment + 1];
    Some(Point3::new(
        start.x + fraction * (end.x - start.x),
        start.y + fraction * (end.y - start.y),
        start.z + fraction * (end.z - start.z),
    ))
}

fn polyline_tangent(points: &[Point3], parameters: Option<&[f64]>, t: f64) -> Option<Vector3> {
    if points.len() < 2 || !t.is_finite() {
        return None;
    }
    let implicit;
    let parameters = if let Some(parameters) = parameters {
        if parameters.len() != points.len() {
            return None;
        }
        parameters
    } else {
        implicit = (0..points.len())
            .map(|index| index as f64)
            .collect::<Vec<_>>();
        &implicit
    };
    let mut tangent = None;
    for (segment, window) in parameters.windows(2).enumerate() {
        if !((t >= window[0] && t <= window[1]) || (t <= window[0] && t >= window[1])) {
            continue;
        }
        let width = window[1] - window[0];
        if width == 0.0 || !width.is_finite() {
            return None;
        }
        let start = points[segment];
        let end = points[segment + 1];
        let candidate = Vector3::new(
            (end.x - start.x) / width,
            (end.y - start.y) / width,
            (end.z - start.z) / width,
        );
        if tangent.is_some_and(|tangent| tangent != candidate) {
            return None;
        }
        tangent = Some(candidate);
    }
    tangent
}

fn transform_surface_second_partials(
    partials: SurfaceSecondPartials,
    transform: Transform,
) -> SurfaceSecondPartials {
    SurfaceSecondPartials {
        point: affine_point(transform, partials.point),
        du: affine_vector(transform, partials.du),
        dv: affine_vector(transform, partials.dv),
        duu: affine_vector(transform, partials.duu),
        duv: affine_vector(transform, partials.duv),
        dvv: affine_vector(transform, partials.dvv),
    }
}

fn affine_orientation(transform: Transform) -> f64 {
    let [first, second, third, _] = transform.rows;
    let determinant = first[0] * (second[1] * third[2] - second[2] * third[1])
        - first[1] * (second[0] * third[2] - second[2] * third[0])
        + first[2] * (second[0] * third[1] - second[1] * third[0]);
    if determinant.is_finite() && determinant < 0.0 {
        -1.0
    } else {
        1.0
    }
}

fn affine_point(transform: Transform, point: Point3) -> Point3 {
    Point3::new(
        transform.rows[0][0] * point.x
            + transform.rows[0][1] * point.y
            + transform.rows[0][2] * point.z
            + transform.rows[0][3],
        transform.rows[1][0] * point.x
            + transform.rows[1][1] * point.y
            + transform.rows[1][2] * point.z
            + transform.rows[1][3],
        transform.rows[2][0] * point.x
            + transform.rows[2][1] * point.y
            + transform.rows[2][2] * point.z
            + transform.rows[2][3],
    )
}

fn affine_vector(transform: Transform, vector: Vector3) -> Vector3 {
    Vector3::new(
        transform.rows[0][0] * vector.x
            + transform.rows[0][1] * vector.y
            + transform.rows[0][2] * vector.z,
        transform.rows[1][0] * vector.x
            + transform.rows[1][1] * vector.y
            + transform.rows[1][2] * vector.z,
        transform.rows[2][0] * vector.x
            + transform.rows[2][1] * vector.y
            + transform.rows[2][2] * vector.z,
    )
}

fn scale_vector(vector: Vector3, factor: f64) -> Vector3 {
    Vector3::new(vector.x * factor, vector.y * factor, vector.z * factor)
}

fn vector_sum(terms: &[(f64, Vector3)]) -> Vector3 {
    terms
        .iter()
        .fold(Vector3::new(0.0, 0.0, 0.0), |mut vector, (factor, term)| {
            vector.x += factor * term.x;
            vector.y += factor * term.y;
            vector.z += factor * term.z;
            vector
        })
}

/// Evaluate a pcurve carrier at parameter `t`, yielding a surface `(u, v)`.
pub fn pcurve_uv(geometry: &PcurveGeometry, t: f64) -> Option<Point2> {
    pcurve_uv_inner(geometry, t, 0)
}

/// Evaluate the exact first derivative of a directly stored pcurve.
pub fn pcurve_tangent(geometry: &PcurveGeometry, t: f64) -> Option<Point2> {
    pcurve_uv_differential_inner(geometry, t, 0)?.tangent
}

fn pcurve_uv_inner(geometry: &PcurveGeometry, t: f64, depth: usize) -> Option<Point2> {
    pcurve_uv_differential_inner(geometry, t, depth).map(|differential| differential.point)
}

fn pcurve_uv_differential_inner(
    geometry: &PcurveGeometry,
    t: f64,
    depth: usize,
) -> Option<PcurveDifferential> {
    if depth > 256 {
        return None;
    }
    if let PcurveGeometry::Offset { distance, basis } = geometry {
        let basis = pcurve_uv_differential_inner(basis, t, depth + 1)?;
        let tangent = basis.tangent?;
        let speed = tangent.u.hypot(tangent.v);
        if !speed.is_finite() || speed == 0.0 {
            return None;
        }
        let unit = Point2::new(tangent.u / speed, tangent.v / speed);
        let point = Point2::new(
            basis.point.u - distance * unit.v,
            basis.point.v + distance * unit.u,
        );
        let tangent = basis.acceleration.map(|acceleration| {
            let tangential_acceleration = unit.u * acceleration.u + unit.v * acceleration.v;
            let unit_derivative = Point2::new(
                (acceleration.u - tangential_acceleration * unit.u) / speed,
                (acceleration.v - tangential_acceleration * unit.v) / speed,
            );
            Point2::new(
                tangent.u - distance * unit_derivative.v,
                tangent.v + distance * unit_derivative.u,
            )
        });
        return Some(PcurveDifferential {
            point,
            tangent: tangent.filter(|tangent| tangent.u.is_finite() && tangent.v.is_finite()),
            acceleration: None,
        });
    }
    let pair = match geometry {
        PcurveGeometry::Line { origin, direction } => (
            Point2::new(origin.u + t * direction.u, origin.v + t * direction.v),
            *direction,
            Point2::new(0.0, 0.0),
        ),
        PcurveGeometry::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => {
            let cosine = t.cos();
            let sine = t.sin();
            (
                offset2(
                    *center,
                    &[(radius * cosine, *x_axis), (radius * sine, *y_axis)],
                ),
                Point2::new(
                    radius * (-sine * x_axis.u + cosine * y_axis.u),
                    radius * (-sine * x_axis.v + cosine * y_axis.v),
                ),
                Point2::new(
                    -radius * (cosine * x_axis.u + sine * y_axis.u),
                    -radius * (cosine * x_axis.v + sine * y_axis.v),
                ),
            )
        }
        PcurveGeometry::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => {
            let cosine = t.cos();
            let sine = t.sin();
            (
                offset2(
                    *center,
                    &[
                        (major_radius * cosine, *x_axis),
                        (minor_radius * sine, *y_axis),
                    ],
                ),
                Point2::new(
                    -major_radius * sine * x_axis.u + minor_radius * cosine * y_axis.u,
                    -major_radius * sine * x_axis.v + minor_radius * cosine * y_axis.v,
                ),
                Point2::new(
                    -major_radius * cosine * x_axis.u - minor_radius * sine * y_axis.u,
                    -major_radius * cosine * x_axis.v - minor_radius * sine * y_axis.v,
                ),
            )
        }
        PcurveGeometry::Harmonic {
            center,
            cosine,
            sine,
        } => {
            let cosine_parameter = t.cos();
            let sine_parameter = t.sin();
            (
                offset2(
                    *center,
                    &[(cosine_parameter, *cosine), (sine_parameter, *sine)],
                ),
                Point2::new(
                    -sine_parameter * cosine.u + cosine_parameter * sine.u,
                    -sine_parameter * cosine.v + cosine_parameter * sine.v,
                ),
                Point2::new(
                    -cosine_parameter * cosine.u - sine_parameter * sine.u,
                    -cosine_parameter * cosine.v - sine_parameter * sine.v,
                ),
            )
        }
        PcurveGeometry::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } if *focal_distance != 0.0 => (
            offset2(
                *vertex,
                &[(t * t / (4.0 * focal_distance), *x_axis), (t, *y_axis)],
            ),
            Point2::new(
                t / (2.0 * focal_distance) * x_axis.u + y_axis.u,
                t / (2.0 * focal_distance) * x_axis.v + y_axis.v,
            ),
            Point2::new(
                x_axis.u / (2.0 * focal_distance),
                x_axis.v / (2.0 * focal_distance),
            ),
        ),
        PcurveGeometry::Parabola { .. } => return None,
        PcurveGeometry::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => {
            let cosine = t.cosh();
            let sine = t.sinh();
            (
                offset2(
                    *center,
                    &[
                        (major_radius * cosine, *x_axis),
                        (minor_radius * sine, *y_axis),
                    ],
                ),
                Point2::new(
                    major_radius * sine * x_axis.u + minor_radius * cosine * y_axis.u,
                    major_radius * sine * x_axis.v + minor_radius * cosine * y_axis.v,
                ),
                Point2::new(
                    major_radius * cosine * x_axis.u + minor_radius * sine * y_axis.u,
                    major_radius * cosine * x_axis.v + minor_radius * sine * y_axis.v,
                ),
            )
        }
        PcurveGeometry::Hyperbolic {
            center,
            cosine,
            sine,
        } => {
            let cosine_parameter = t.cosh();
            let sine_parameter = t.sinh();
            (
                offset2(
                    *center,
                    &[(cosine_parameter, *cosine), (sine_parameter, *sine)],
                ),
                Point2::new(
                    sine_parameter * cosine.u + cosine_parameter * sine.u,
                    sine_parameter * cosine.v + cosine_parameter * sine.v,
                ),
                Point2::new(
                    cosine_parameter * cosine.u + sine_parameter * sine.u,
                    cosine_parameter * cosine.v + sine_parameter * sine.v,
                ),
            )
        }
        PcurveGeometry::PolarHarmonic {
            radial_center,
            radial_cos,
            radial_sin,
            axial_origin,
            axial_cos,
            axial_sin,
        } => {
            let cosine = t.cos();
            let sine = t.sin();
            let x = radial_center.u + radial_cos.u * cosine + radial_sin.u * sine;
            let y = radial_center.v + radial_cos.v * cosine + radial_sin.v * sine;
            let dx = -radial_cos.u * sine + radial_sin.u * cosine;
            let dy = -radial_cos.v * sine + radial_sin.v * cosine;
            let ddx = -radial_cos.u * cosine - radial_sin.u * sine;
            let ddy = -radial_cos.v * cosine - radial_sin.v * sine;
            let radius_squared = x * x + y * y;
            if radius_squared == 0.0 {
                return None;
            }
            (
                Point2::new(
                    y.atan2(x),
                    axial_origin + axial_cos * cosine + axial_sin * sine,
                ),
                Point2::new(
                    (x * dy - y * dx) / radius_squared,
                    -axial_cos * sine + axial_sin * cosine,
                ),
                Point2::new(
                    ((x * ddy - y * ddx) * radius_squared
                        - (x * dy - y * dx) * 2.0 * (x * dx + y * dy))
                        / (radius_squared * radius_squared),
                    -axial_cos * cosine - axial_sin * sine,
                ),
            )
        }
        PcurveGeometry::PolarNurbs {
            degree,
            knots,
            radial_control_points,
            axial_control_points,
            weights,
            ..
        } => {
            if radial_control_points.len() != axial_control_points.len() {
                return None;
            }
            let radial = nurbs_pcurve_differential(
                *degree,
                knots,
                radial_control_points,
                weights.as_deref(),
                t,
            )?;
            let axial_points = axial_control_points
                .iter()
                .map(|value| Point2::new(*value, 0.0))
                .collect::<Vec<_>>();
            let axial =
                nurbs_pcurve_differential(*degree, knots, &axial_points, weights.as_deref(), t)?;
            let radius_squared = radial.point.u * radial.point.u + radial.point.v * radial.point.v;
            if radius_squared == 0.0 {
                return None;
            }
            let point = Point2::new(radial.point.v.atan2(radial.point.u), axial.point.u);
            let tangent = radial
                .tangent
                .zip(axial.tangent)
                .map(|(radial_tangent, axial_tangent)| {
                    Point2::new(
                        (radial.point.u * radial_tangent.v - radial.point.v * radial_tangent.u)
                            / radius_squared,
                        axial_tangent.u,
                    )
                })
                .filter(|tangent| tangent.u.is_finite() && tangent.v.is_finite());
            let acceleration = radial
                .tangent
                .zip(radial.acceleration)
                .zip(axial.acceleration)
                .map(
                    |((radial_tangent, radial_acceleration), axial_acceleration)| {
                        let numerator =
                            radial.point.u * radial_tangent.v - radial.point.v * radial_tangent.u;
                        let numerator_derivative = radial.point.u * radial_acceleration.v
                            - radial.point.v * radial_acceleration.u;
                        let denominator_derivative = 2.0
                            * (radial.point.u * radial_tangent.u
                                + radial.point.v * radial_tangent.v);
                        Point2::new(
                            (numerator_derivative * radius_squared
                                - numerator * denominator_derivative)
                                / (radius_squared * radius_squared),
                            axial_acceleration.u,
                        )
                    },
                )
                .filter(|acceleration| acceleration.u.is_finite() && acceleration.v.is_finite());
            return Some(PcurveDifferential {
                point,
                tangent,
                acceleration,
            });
        }
        PcurveGeometry::SphericalGreatCircle {
            azimuth_origin,
            azimuth_rate,
            plane_phase,
            plane_slope,
        } => {
            let azimuth = azimuth_origin + azimuth_rate * t;
            let phase = azimuth - plane_phase;
            let cosine = phase.cos();
            let sine = phase.sin();
            let latitude = (plane_slope * cosine).atan();
            let denominator = 1.0 + plane_slope * plane_slope * cosine * cosine;
            let numerator = -plane_slope * azimuth_rate * sine;
            let denominator_derivative =
                -2.0 * plane_slope * plane_slope * azimuth_rate * cosine * sine;
            let numerator_derivative = -plane_slope * azimuth_rate * azimuth_rate * cosine;
            let point = Point2::new(azimuth, latitude);
            let tangent = Point2::new(*azimuth_rate, numerator / denominator);
            let acceleration = Point2::new(
                0.0,
                (numerator_derivative * denominator - numerator * denominator_derivative)
                    / (denominator * denominator),
            );
            return (point.u.is_finite() && point.v.is_finite()).then_some(PcurveDifferential {
                point,
                tangent: (tangent.u.is_finite() && tangent.v.is_finite()).then_some(tangent),
                acceleration: (acceleration.u.is_finite() && acceleration.v.is_finite())
                    .then_some(acceleration),
            });
        }
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            ..
        } => {
            return nurbs_pcurve_differential(
                *degree,
                knots,
                control_points,
                weights.as_deref(),
                t,
            );
        }
        PcurveGeometry::Transformed { basis, transform } => {
            let basis = pcurve_uv_differential_inner(basis, t, depth + 1)?;
            let point = transform.apply_point(basis.point);
            let tangent = basis.tangent.map(|tangent| transform.apply_vector(tangent));
            let acceleration = basis
                .acceleration
                .map(|acceleration| transform.apply_vector(acceleration));
            return (point.u.is_finite()
                && point.v.is_finite()
                && tangent.is_none_or(|tangent| tangent.u.is_finite() && tangent.v.is_finite())
                && acceleration.is_none_or(|acceleration| {
                    acceleration.u.is_finite() && acceleration.v.is_finite()
                }))
            .then_some(PcurveDifferential {
                point,
                tangent,
                acceleration,
            });
        }
        PcurveGeometry::Trimmed { basis, .. } => {
            return pcurve_uv_differential_inner(basis, t, depth + 1);
        }
        PcurveGeometry::Offset { .. } => return None,
    };
    if !pair.0.u.is_finite() || !pair.0.v.is_finite() {
        return None;
    }
    Some(PcurveDifferential {
        point: pair.0,
        tangent: (pair.1.u.is_finite() && pair.1.v.is_finite()).then_some(pair.1),
        acceleration: (pair.2.u.is_finite() && pair.2.v.is_finite()).then_some(pair.2),
    })
}

fn offset2(base: Point2, terms: &[(f64, Point2)]) -> Point2 {
    terms.iter().fold(base, |mut point, (factor, direction)| {
        point.u += factor * direction.u;
        point.v += factor * direction.v;
        point
    })
}

#[cfg(test)]
mod tests;
