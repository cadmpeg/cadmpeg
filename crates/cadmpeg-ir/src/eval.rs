// SPDX-License-Identifier: Apache-2.0
//! Point evaluation of geometry carriers.
//!
//! Evaluators map carrier parameters to model-space (or parameter-space)
//! points using the carriers' own parameterizations: conic parameters are
//! angles from the reference/major direction, line parameters are signed
//! distances along the unit direction, and B-splines evaluate by Cox–de Boor
//! over their stored knot vectors. [`model_surface_point`] resolves construction-
//! backed carriers that require other model entities. Carriers without a typed
//! parameterization ([`SolvedCurveGeometry::Unknown`], [`SolvedCurveGeometry::Composite`],
//! [`SolvedSurfaceGeometry::Unknown`]) have no value.
//! [`model_curve_point_by_id`] resolves construction-backed curves whose
//! parameterization is established by model entities.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::nurbs::bezier::{homogeneous_spans, positive_controls};
use crate::geometry::nurbs::bounds::speed_bound;
use crate::geometry::{
    nurbs::{knots_nondecreasing, NurbsCurve, NurbsSurface, SurfaceParameterAxis},
    pcurve::{PcurveGeometry, PcurveNurbs},
    CurveGeometry, LawExpression, LawFormula, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    SweepSurfaceLayout,
};
use crate::math::solve::least_squares_step;
use crate::math::sum::{scaled_ratio_products, ExactSignedSum, ScaledValue};
use crate::math::{product_quotient, scaled_sinh_cosh};
use crate::math::{Point2, Point3, Vector3};
use crate::scalar::{
    ExtendedReal, FiniteReal, Length, NonNegativeLength, NonNegativeReal, NonZeroLength,
    NonZeroReal, PositiveReal, SegmentPosition, UnitCosine,
};
use crate::topology::{IncreasingParameterInterval, ParameterInterval};
use crate::transform::Transform;
use crate::units::{FinitePoint2, FiniteVector, UnitVector3};
use crate::CadIr;
use cadmpeg_core::decode::{alloc_filled, WorkBudget};

mod depth;
mod polyline;
mod rational;
use depth::ModelEvaluationDepthGuard;
use polyline::{polyline_point, polyline_samples, polyline_tangent};
use rational::{finite_lanes, Homogeneous};

const DEFAULT_NURBS_SURFACE_INVERSION_WORK: usize = 1_000_000;

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
        first.x.midpoint(second.x),
        first.y.midpoint(second.y),
        first.z.midpoint(second.z),
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
    if !scale.is_finite() {
        return false;
    }
    let Some(axis) = FiniteVector3::new(axis).and_then(FiniteVector3::unit_nonzero) else {
        return false;
    };
    let scaled =
        |vector: Vector3| Vector3::new(vector.x / scale, vector.y / scale, vector.z / scale);
    axis.cross(scaled(from_axis)).norm() <= EPS_EVAL_SPATIAL_POINTS_ARE_REFLECTIONS_E9
        && axis.dot(scaled(separation)).abs() <= EPS_EVAL_SPATIAL_POINTS_ARE_REFLECTIONS_E9
}

/// Recover native parameters for an analytic surface point. The parameters
/// are absent when a projection of the point is not finite, and on a cone at
/// its apex and where its angle is undefined.
pub fn analytic_surface_parameters_solved(
    geometry: &SolvedSurfaceGeometry,
    point: Point3,
) -> Option<FinitePoint2> {
    let components = |origin: Point3, axis: Vector3, reference: Vector3| {
        let project = |direction: Vector3| {
            crate::math::sum::finite_dot(
                [point.x, point.y, point.z, -origin.x, -origin.y, -origin.z],
                [
                    direction.x,
                    direction.y,
                    direction.z,
                    direction.x,
                    direction.y,
                    direction.z,
                ],
            )
            .ok()
        };
        let transverse = axis.cross(reference);
        (project(reference), project(transverse), project(axis))
    };
    let finite = ExtendedReal::from_finite;
    match geometry {
        SolvedSurfaceGeometry::Plane(plane_surface) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            let (u, v, _) = components(origin, *normal, *u_axis);
            Some(FinitePoint2::from_coordinates(u?, v?))
        }
        SolvedSurfaceGeometry::Cylinder(cylinder_surface) => {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let ref_direction = cylinder_surface.frame().reference().as_raw();
            let radius = NonZeroLength::from(cylinder_surface.radius());
            let (x, y, v) = components(origin, *axis, *ref_direction);
            let angle = finite(y?).over(radius).atan2(finite(x?).over(radius));
            Some(FinitePoint2::from_coordinates(angle, v?))
        }
        SolvedSurfaceGeometry::Cone(cone_surface) => {
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let ref_direction = cone_surface.frame().reference().as_raw();
            let radius = cone_surface.radius().get();
            let ratio = cone_surface.ratio().get();
            let half_angle = cone_surface.half_angle().get();
            let (x, y, v) = components(origin, *axis, *ref_direction);
            let (x, y, v) = (x?.get(), y?.get(), v?);
            let local_radius = radius + v.get() * half_angle.tan();
            if local_radius == 0.0 {
                return None;
            }
            // A section radius whose product with the ratio underflows to
            // zero leaves 0 / 0 at y = 0.
            let angle = FiniteReal::new((y / (local_radius * ratio)).atan2(x / local_radius))?;
            Some(FinitePoint2::from_coordinates(angle, v))
        }
        SolvedSurfaceGeometry::Sphere(sphere_surface) => {
            let center = sphere_surface.center().get();
            let axis = sphere_surface.frame().axis().as_raw();
            let ref_direction = sphere_surface.frame().reference().as_raw();
            let (x, y, z) = components(center, *axis, *ref_direction);
            let (x, y, z) = (x?, y?, z?);
            Some(FinitePoint2::from_coordinates(
                y.atan2(x),
                finite(z).atan2(ExtendedReal::hypot(x, y)),
            ))
        }
        SolvedSurfaceGeometry::Torus(torus_surface) => {
            let center = torus_surface.center().get();
            let axis = torus_surface.frame().axis().as_raw();
            let ref_direction = torus_surface.frame().reference().as_raw();
            let major_radius = torus_surface.major_radius();
            let minor_radius = torus_surface.minor_radius();
            let (x, y, z) = components(center, *axis, *ref_direction);
            let (x, y, z) = (x?, y?, z?);
            Some(FinitePoint2::from_coordinates(
                y.atan2(x),
                finite(z).over(minor_radius).atan2(
                    ExtendedReal::hypot(x, y)
                        .minus(major_radius.into())
                        .over(minor_radius),
                ),
            ))
        }
        _ => None,
    }
}

#[derive(Clone)]
struct RationalBezierSurfacePatch {
    u_domain: IncreasingParameterInterval,
    v_domain: IncreasingParameterInterval,
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

struct HomogeneousBezierSplit {
    left: Vec<[f64; 4]>,
    point: [f64; 4],
    right_reversed: Vec<[f64; 4]>,
}

impl HomogeneousBezierSplit {
    fn into_polygons(mut self) -> (Vec<[f64; 4]>, Vec<[f64; 4]>) {
        self.left.push(self.point);
        self.right_reversed.push(self.point);
        self.right_reversed.reverse();
        (self.left, self.right_reversed)
    }
}

fn rational_surface_patches(surface: &NurbsSurface) -> Option<Vec<RationalBezierSurfacePatch>> {
    let budget = WorkBudget::new(DEFAULT_NURBS_SURFACE_INVERSION_WORK);
    rational_surface_patches_with_budget(surface, &budget)
}

fn rational_surface_patches_with_budget(
    surface: &NurbsSurface,
    budget: &WorkBudget<'_>,
) -> Option<Vec<RationalBezierSurfacePatch>> {
    let u_degree = usize::try_from(surface.u_degree()).ok()?;
    let v_degree = usize::try_from(surface.v_degree()).ok()?;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let control_count = u_count.checked_mul(v_count)?;
    let patch_control_count = (u_degree + 1).checked_mul(v_degree + 1)?;
    budget
        .charge_by(
            control_count
                .checked_add(surface.u_knots().len())?
                .checked_add(surface.v_knots().len())?,
        )
        .then_some(())?;
    if u_degree >= u_count || v_degree >= v_count {
        return None;
    }
    let weights = match surface.pole_weights() {
        Some(values) => {
            if !values.iter().all(|weight| weight.get() > 0.0) {
                return None;
            }
            values.into_iter().map(NonZeroReal::get).collect()
        }
        None => alloc_filled(control_count, 1.0, "ir_nurbs_surface_weights").ok()?,
    };
    let homogeneous_controls = positive_controls(&surface.poles(), &weights)?;
    // The Bezier spans of every row and column share the knots, so their
    // domains are the intervals between consecutive distinct active knots.
    let u_domains = surface.u_knots().active_spans(u_degree, u_count)?;
    let v_domains = surface.v_knots().active_spans(v_degree, v_count)?;
    let u_spans_by_v = (0..v_count)
        .map(|v| {
            homogeneous_spans(
                u_degree,
                surface.u_knots(),
                (0..u_count)
                    .map(|u| homogeneous_controls[u * v_count + v])
                    .collect(),
            )
        })
        .collect::<Option<Vec<_>>>()?;
    if u_spans_by_v
        .iter()
        .any(|spans| spans.len() != u_domains.len())
    {
        return None;
    }
    let mut patches = Vec::new();
    for (u_span, &u_domain) in u_domains.iter().enumerate() {
        let v_spans_by_u = (0..=u_degree)
            .map(|u_control| {
                homogeneous_spans(
                    v_degree,
                    surface.v_knots(),
                    (0..v_count)
                        .map(|v| u_spans_by_v[v][u_span].controls[u_control])
                        .collect(),
                )
            })
            .collect::<Option<Vec<_>>>()?;
        if v_spans_by_u
            .iter()
            .any(|spans| spans.len() != v_domains.len())
        {
            return None;
        }
        for (v_span, &v_domain) in v_domains.iter().enumerate() {
            budget.charge_by(patch_control_count).then_some(())?;
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
    budget: &WorkBudget<'_>,
) -> Option<Vec<RationalBezierSurfacePatch>> {
    if !point.is_finite() {
        return None;
    }
    let mut patches = rational_surface_patches_with_budget(surface, budget)?;
    let residual_work = patches
        .iter()
        .try_fold(0usize, |work, patch| work.checked_add(patch.controls.len()))?;
    budget.charge_by(residual_work).then_some(())?;
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
    if !parameter.is_finite() || !(0.0..=1.0).contains(&parameter) {
        return None;
    }
    split_homogeneous_bezier_with(controls, |left, right| {
        std::array::from_fn(|axis| (1.0 - parameter) * left[axis] + parameter * right[axis])
    })
}

fn split_homogeneous_bezier_midpoint(controls: &[[f64; 4]]) -> Option<HomogeneousBezierSplit> {
    split_homogeneous_bezier_with(controls, |left, right| {
        std::array::from_fn(|axis| 0.5 * (left[axis] + right[axis]))
    })
}

fn split_homogeneous_bezier_with(
    controls: &[[f64; 4]],
    blend: impl Fn([f64; 4], [f64; 4]) -> [f64; 4],
) -> Option<HomogeneousBezierSplit> {
    let (&first, rest) = controls.split_first()?;
    let mut first = first;
    let mut rest = rest.to_vec();
    let mut left = Vec::with_capacity(controls.len());
    let mut right = Vec::with_capacity(controls.len());
    while let Some((&second, tail)) = rest.split_first() {
        left.push(first);
        right.push(tail.last().copied().unwrap_or(second));
        first = blend(first, second);
        rest = rest
            .iter()
            .zip(tail)
            .map(|(&first, &second)| blend(first, second))
            .collect();
    }
    Some(HomogeneousBezierSplit {
        left,
        point: first,
        right_reversed: right,
    })
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
        let point = split_homogeneous_bezier(controls, start)?.point;
        return alloc_filled(controls.len(), point, "ir_bezier_collapsed_controls").ok();
    }
    let left = split_homogeneous_bezier(controls, end)?.into_polygons().0;
    if start == 0.0 {
        return Some(left);
    }
    let relative_start = start / end;
    split_homogeneous_bezier(&left, relative_start).map(|split| split.into_polygons().1)
}

fn binomial_coefficient(degree: usize, index: usize) -> f64 {
    let index = index.min(degree - index);
    (1..=index).fold(1.0, |value, factor| {
        value * (degree - index + factor) as f64 / factor as f64
    })
}

fn rational_patch_parameter_segment(
    patch: &RationalBezierSurfacePatch,
    start: FinitePoint2,
    end: FinitePoint2,
) -> Option<Vec<[f64; 4]>> {
    let normalize = |value: FiniteReal, domain: IncreasingParameterInterval| {
        let [lower, upper] = domain.finite_endpoints();
        match value.segment_position(lower, upper) {
            SegmentPosition::Within(fraction) => Some(fraction.get()),
            SegmentPosition::Outside | SegmentPosition::Degenerate => None,
        }
    };
    let [start_u, start_v] = start.coordinates();
    let [end_u, end_v] = end.coordinates();
    let u_range = [
        normalize(start_u, patch.u_domain)?,
        normalize(end_u, patch.u_domain)?,
    ];
    let v_range = [
        normalize(start_v, patch.v_domain)?,
        normalize(end_v, patch.v_domain)?,
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
        let mut residual_norm = 0.0_f64;
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
            residual_norm = residual_norm.hypot(residual);
            coordinate_scale = coordinate_scale.max((coordinate / weight).abs());
        }
        bound = bound.max(residual_norm);
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
    let [Some(first), Some(last)] = parameters.map(FinitePoint2::new) else {
        return None;
    };
    if chord.iter().any(|point| !point.is_finite()) {
        return None;
    }
    let patches = rational_surface_patches(surface)?;
    let [first_u, first_v] = first.coordinates();
    let [last_u, last_v] = last.coordinates();
    let mut splits = vec![0.0, 1.0];
    for patch in &patches {
        let [u_lower, u_upper] = patch.u_domain.finite_endpoints();
        let [v_lower, v_upper] = patch.v_domain.finite_endpoints();
        for (boundary, start, end) in [
            (u_lower, first_u, last_u),
            (u_upper, first_u, last_u),
            (v_lower, first_v, last_v),
            (v_upper, first_v, last_v),
        ] {
            if start.get().min(end.get()) < boundary.get()
                && boundary.get() < start.get().max(end.get())
            {
                let parameter = difference_quotient(boundary, start, end, start).ok()?.get();
                if 0.0 < parameter && parameter < 1.0 {
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
            Some(FinitePoint2::from_coordinates(
                crate::math::interpolate(first.u, last.u, parameter)?,
                crate::math::interpolate(first.v, last.v, parameter)?,
            ))
        };
        let midpoint = parameter_point(middle)?;
        let patch = patches.iter().find(|patch| {
            patch.u_domain.lower() <= midpoint.u
                && midpoint.u <= patch.u_domain.upper()
                && patch.v_domain.lower() <= midpoint.v
                && midpoint.v <= patch.v_domain.upper()
        })?;
        let controls = rational_patch_parameter_segment(
            patch,
            parameter_point(range[0])?,
            parameter_point(range[1])?,
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

fn rational_patch_distance_bounds_with_budget(
    patch: &RationalBezierSurfacePatch,
    budget: &WorkBudget<'_>,
) -> Option<(f64, f64)> {
    budget.charge_by(patch.controls.len()).then_some(())?;
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
                minimum[axis]
            } else if maximum[axis] < 0.0 {
                -maximum[axis]
            } else {
                0.0
            }
        })
        .fold(0.0_f64, f64::hypot);
    let diameter = (0..3)
        .map(|axis| maximum[axis] - minimum[axis])
        .fold(0.0_f64, f64::hypot);
    (lower.is_finite() && diameter.is_finite()).then_some((lower, diameter))
}

fn split_rational_surface_patch(
    patch: &RationalBezierSurfacePatch,
    split_u: bool,
    budget: &WorkBudget<'_>,
) -> Option<[RationalBezierSurfacePatch; 2]> {
    let (degree, line_count) = if split_u {
        (patch.u_degree, patch.v_degree + 1)
    } else {
        (patch.v_degree, patch.u_degree + 1)
    };
    budget
        .charge_by(patch.controls.len().checked_mul(degree + 1)?)
        .then_some(())?;
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
        let (first, second) = split_homogeneous_bezier_midpoint(&controls)?.into_polygons();
        first_lines.push(first);
        second_lines.push(second);
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
    let (first_u, second_u, first_v, second_v) = if split_u {
        let (first_u, second_u) = patch.u_domain.split_at_midpoint()?;
        (first_u, second_u, patch.v_domain, patch.v_domain)
    } else {
        let (first_v, second_v) = patch.v_domain.split_at_midpoint()?;
        (patch.u_domain, patch.u_domain, first_v, second_v)
    };
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
    start: FinitePoint2,
    u_domain: ParameterInterval,
    v_domain: ParameterInterval,
    budget: &WorkBudget<'_>,
) -> Option<FinitePoint2> {
    let distance = |position: Point3| position.distance(point);
    let [start_u, start_v] = start.coordinates();
    let mut parameters = FinitePoint2::from_coordinates(
        u_domain.project(ExtendedReal::from_finite(start_u)),
        v_domain.project(ExtendedReal::from_finite(start_v)),
    );
    for _ in 0..32 {
        let position =
            nurbs_surface_point_with_budget(surface, parameters.u, parameters.v, budget).ok()?;
        let residual = Vector3::new(
            position.x - point.x,
            position.y - point.y,
            position.z - point.z,
        );
        let partials =
            nurbs_surface_partials_with_budget(surface, parameters.u, parameters.v, budget).ok()?;
        let Some((step_u, step_v)) = least_squares_step(partials.du, partials.dv, residual) else {
            break;
        };
        let current_distance = distance(position.get());
        let [u, v] = parameters.coordinates();
        let mut scale = FiniteReal::ONE;
        let mut accepted = None;
        for _ in 0..16 {
            let candidate = FinitePoint2::from_coordinates(
                u_domain.project(ExtendedReal::stepped(u, scale, step_u)),
                v_domain.project(ExtendedReal::stepped(v, scale, step_v)),
            );
            let candidate_position =
                nurbs_surface_point_with_budget(surface, candidate.u, candidate.v, budget).ok()?;
            if distance(candidate_position.get()) <= current_distance {
                accepted = Some(candidate);
                break;
            }
            scale = scale.halved();
        }
        let Some(candidate) = accepted else {
            break;
        };
        parameters = candidate;
        if scale.get() * step_u.get().abs()
            <= EPS_EVAL_REFINE_NURBS_SURFACE_PARAMETERS_E12 * (1.0 + parameters.u.abs())
            && scale.get() * step_v.get().abs()
                <= EPS_EVAL_REFINE_NURBS_SURFACE_PARAMETERS_E12 * (1.0 + parameters.v.abs())
        {
            break;
        }
    }
    Some(parameters)
}

fn nurbs_surface_evaluation_cost(surface: &NurbsSurface) -> Option<usize> {
    let (u_support, v_support) = nurbs_surface_support_sizes(surface)?;
    let control_work = u_support.checked_mul(v_support)?;
    control_work
        .checked_add(u_support.checked_mul(u_support)?)?
        .checked_add(v_support.checked_mul(v_support)?)
}

fn complete_nurbs_surface_starts(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<FinitePoint2>,
    fit_tolerance: Option<f64>,
    budget: &WorkBudget<'_>,
) -> Option<Vec<FinitePoint2>> {
    const MAX_PATCHES: usize = 1_000_000;

    let patches = rational_surface_residual_patches(surface, point, budget)?;
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
    let distance_at = |parameters: FinitePoint2| {
        let position =
            nurbs_surface_point_with_budget(surface, parameters.u, parameters.v, budget).ok()?;
        let distance = (position.x - point.x)
            .hypot(position.y - point.y)
            .hypot(position.z - point.z);
        distance.is_finite().then_some(distance)
    };
    let center = |patch: &RationalBezierSurfacePatch| {
        let [u_start, u_end] = patch.u_domain.finite_endpoints();
        let [v_start, v_end] = patch.v_domain.finite_endpoints();
        FinitePoint2::from_coordinates(u_start.midpoint(u_end), v_start.midpoint(v_end))
    };
    let surface_u_domain = surface
        .u_knots()
        .span(usize::try_from(surface.u_degree()).ok()?, surface.u_count())?;
    let surface_v_domain = surface
        .v_knots()
        .span(usize::try_from(surface.v_degree()).ok()?, surface.v_count())?;
    let refined_upper = |start, u_domain, v_domain| {
        let parameters =
            refine_nurbs_surface_parameters(surface, point, start, u_domain, v_domain, budget)
                .unwrap_or(start);
        Some((parameters, distance_at(parameters)?))
    };
    let mut best_distance = f64::INFINITY;
    let mut best_upper_parameters = Vec::new();
    {
        let mut consider_upper = |(parameters, distance): (FinitePoint2, f64)| {
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
                    .max(distance_tolerance);
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
                patch.u_domain.into(),
                patch.v_domain.into(),
            )?);
        }
    }
    best_distance.is_finite().then_some(())?;
    // A tolerance-bounded inverse needs a constructive fitting parameter, not
    // a proof of the global minimum. Every upper candidate is surface-evaluated.
    if fit_tolerance.is_some() && best_distance <= distance_tolerance {
        return (!best_upper_parameters.is_empty()).then_some(best_upper_parameters);
    }
    let mut queue = BinaryHeap::new();
    let mut sequence = 0usize;
    for patch in patches {
        let (lower_bound, diameter) = rational_patch_distance_bounds_with_budget(&patch, budget)?;
        queue.push(SurfacePatchQueueEntry {
            lower_bound,
            diameter,
            sequence,
            patch,
        });
        sequence += 1;
    }
    let mut terminal = Vec::<(FinitePoint2, f64)>::new();
    let mut examined = 0usize;
    while let Some(entry) = queue.pop() {
        examined += 1;
        if examined > MAX_PATCHES || !budget.charge() {
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
                .max(distance_tolerance);
        if lower_bound > best_distance + comparison_tolerance {
            break;
        }
        let parameters = center(&patch);
        let (upper_parameters, center_distance) =
            refined_upper(parameters, patch.u_domain.into(), patch.v_domain.into())?;
        if fit_tolerance.is_some() && center_distance <= distance_tolerance {
            return Some(vec![upper_parameters]);
        }
        let upper_tolerance = 128.0
            * f64::EPSILON
            * center_distance
                .abs()
                .max(best_distance.abs())
                .max(distance_tolerance);
        if center_distance < best_distance && best_distance - center_distance > upper_tolerance {
            best_distance = center_distance;
            best_upper_parameters.clear();
        }
        if (center_distance - best_distance).abs() <= upper_tolerance {
            best_upper_parameters.push(upper_parameters);
        }
        let indivisible = parameters.u == patch.u_domain.lower()
            || parameters.u == patch.u_domain.upper()
            || parameters.v == patch.v_domain.lower()
            || parameters.v == patch.v_domain.upper();
        if diameter <= distance_tolerance
            || center_distance - lower_bound <= distance_tolerance
            || indivisible
        {
            terminal.push((upper_parameters, lower_bound));
            continue;
        }
        budget.charge_by(patch.controls.len()).then_some(())?;
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
                    .map(|axis| second[axis] - first[axis])
                    .fold(0.0_f64, f64::hypot)
            })
            .fold(0.0_f64, f64::max);
        let v_variation = (0..=patch.u_degree)
            .flat_map(|u| (0..patch.v_degree).map(move |v| (u, v)))
            .map(|(u, v)| {
                let first = control(u, v);
                let second = control(u, v + 1);
                (0..3)
                    .map(|axis| second[axis] - first[axis])
                    .fold(0.0_f64, f64::hypot)
            })
            .fold(0.0_f64, f64::max);
        let children = split_rational_surface_patch(&patch, u_variation >= v_variation, budget)?;
        for patch in children {
            let (lower_bound, diameter) =
                rational_patch_distance_bounds_with_budget(&patch, budget)?;
            queue.push(SurfacePatchQueueEntry {
                lower_bound,
                diameter,
                sequence,
                patch,
            });
            sequence += 1;
        }
    }
    let final_tolerance = 128.0 * f64::EPSILON * best_distance.abs().max(distance_tolerance);
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
    budget: &WorkBudget<'_>,
) -> Option<(FinitePoint2, f64)> {
    let seed = seed.and_then(FinitePoint2::new);
    let u_degree = usize::try_from(surface.u_degree()).ok()?;
    let v_degree = usize::try_from(surface.v_degree()).ok()?;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let u_domain = surface.u_knots().span(u_degree, u_count)?;
    let v_domain = surface.v_knots().span(v_degree, v_count)?;
    if u_domain.endpoints()[0] >= u_domain.endpoints()[1]
        || v_domain.endpoints()[0] >= v_domain.endpoints()[1]
    {
        return None;
    }
    if let (Some(seed), Some(tolerance)) = (seed, fit_tolerance) {
        let [seed_u, seed_v] = seed.coordinates();
        let parameters = FinitePoint2::from_coordinates(
            u_domain.project(ExtendedReal::from_finite(seed_u)),
            v_domain.project(ExtendedReal::from_finite(seed_v)),
        );
        let position =
            nurbs_surface_point_with_budget(surface, parameters.u, parameters.v, budget).ok()?;
        let distance = (position.x - point.x)
            .hypot(position.y - point.y)
            .hypot(position.z - point.z);
        if distance.is_finite() && distance <= tolerance {
            return Some((parameters, distance));
        }
        if let Some(refined) =
            refine_nurbs_surface_parameters(surface, point, parameters, u_domain, v_domain, budget)
        {
            let position =
                nurbs_surface_point_with_budget(surface, refined.u, refined.v, budget).ok()?;
            let distance = (position.x - point.x)
                .hypot(position.y - point.y)
                .hypot(position.z - point.z);
            if distance.is_finite() && distance <= tolerance {
                return Some((refined, distance));
            }
        }
    }
    let starts = complete_nurbs_surface_starts(surface, point, seed, fit_tolerance, budget)?;
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    let mut best_seed_distance = f64::INFINITY;
    for start in starts {
        let Some(parameters) =
            refine_nurbs_surface_parameters(surface, point, start, u_domain, v_domain, budget)
        else {
            continue;
        };
        let Ok(position) =
            nurbs_surface_point_with_budget(surface, parameters.u, parameters.v, budget)
        else {
            continue;
        };
        let distance = (position.x - point.x)
            .hypot(position.y - point.y)
            .hypot(position.z - point.z);
        if !distance.is_finite() {
            continue;
        }
        // Quartering before subtraction keeps the tie metric finite even when
        // parameters span the full finite range. Its common scale preserves order.
        let seed_distance = seed.map_or(
            parameters.u.abs() * 0.25 + parameters.v.abs() * 0.25,
            |seed| (parameters.u * 0.25 - seed.u * 0.25).hypot(parameters.v * 0.25 - seed.v * 0.25),
        );
        let same_point = (distance - best_distance).abs()
            <= f64::EPSILON * 64.0 * distance.abs().max(best_distance.abs()).max(1.0);
        if best.is_none()
            || distance < best_distance && !same_point
            || same_point && seed_distance < best_seed_distance
        {
            best = Some(parameters);
            best_distance = distance;
            best_seed_distance = seed_distance;
        }
    }
    best.map(|parameters| (parameters, best_distance))
}

/// Find a globally closest parameter pair on a finite NURBS surface within a
/// caller-owned work slice.
pub fn nurbs_surface_closest_parameter_with_budget(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    budget: &WorkBudget<'_>,
) -> Option<FinitePoint2> {
    solve_nurbs_surface_parameter(surface, point, seed, None, budget)
        .map(|(parameters, _)| parameters)
}

/// Find a bounded local parameter candidate on a finite NURBS surface.
///
/// A supplied seed selects the local Newton branch. Without a seed, a fixed
/// parameter grid supplies the initial branch. The result is a constructive
/// candidate only; callers must forward-evaluate it and apply their own
/// residual bound. This operation does not prove that the candidate is
/// globally closest.
pub fn nurbs_surface_parameter_near_point(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
) -> Option<FinitePoint2> {
    const COARSE_GRID: usize = 8;
    const MAX_ITERATIONS: usize = 24;
    const MAX_LINE_SEARCH_STEPS: usize = 12;

    if !point.is_finite() {
        return None;
    }
    let u_degree = usize::try_from(surface.u_degree()).ok()?;
    let v_degree = usize::try_from(surface.v_degree()).ok()?;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let u_domain = surface.u_knots().span(u_degree, u_count)?;
    let v_domain = surface.v_knots().span(v_degree, v_count)?;
    let [u_start, u_end] = u_domain.endpoints();
    let [v_start, v_end] = v_domain.endpoints();
    if u_start >= u_end || v_start >= v_end {
        return None;
    }
    let mut parameters = match seed.and_then(FinitePoint2::new) {
        Some(seed) => {
            let [seed_u, seed_v] = seed.coordinates();
            FinitePoint2::from_coordinates(
                u_domain.project(ExtendedReal::from_finite(seed_u)),
                v_domain.project(ExtendedReal::from_finite(seed_v)),
            )
        }
        None => {
            let mut best = None;
            for u_index in 0..=COARSE_GRID {
                let u =
                    crate::math::interpolate(u_start, u_end, u_index as f64 / COARSE_GRID as f64)?;
                for v_index in 0..=COARSE_GRID {
                    let v = crate::math::interpolate(
                        v_start,
                        v_end,
                        v_index as f64 / COARSE_GRID as f64,
                    )?;
                    let candidate = nurbs_surface_point(surface, u.get(), v.get()).ok()?;
                    let distance = candidate.distance(point);
                    if best.is_none_or(|(_, best_distance)| distance < best_distance) {
                        best = Some((FinitePoint2::from_coordinates(u, v), distance));
                    }
                }
            }
            best?.0
        }
    };
    let distance = |left: Point3| left.distance(point);
    for _ in 0..MAX_ITERATIONS {
        let partials = nurbs_surface_partials(surface, parameters.u, parameters.v).ok()?;
        let current_distance = distance(partials.point.get());
        if current_distance == 0.0 {
            return Some(parameters);
        }
        let residual = Vector3::new(
            partials.point.x - point.x,
            partials.point.y - point.y,
            partials.point.z - point.z,
        );
        let Some((step_u, step_v)) = least_squares_step(partials.du, partials.dv, residual) else {
            break;
        };
        let [u, v] = parameters.coordinates();
        let mut scale = FiniteReal::ONE;
        let mut accepted = false;
        for _ in 0..MAX_LINE_SEARCH_STEPS {
            let candidate = FinitePoint2::from_coordinates(
                u_domain.project(ExtendedReal::stepped(u, scale, step_u)),
                v_domain.project(ExtendedReal::stepped(v, scale, step_v)),
            );
            let candidate_point = nurbs_surface_point(surface, candidate.u, candidate.v).ok()?;
            if distance(candidate_point.get()) < current_distance {
                parameters = candidate;
                accepted = true;
                break;
            }
            scale = scale.halved();
        }
        if !accepted {
            break;
        }
    }
    Some(parameters)
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
) -> Option<FinitePoint2> {
    let budget = WorkBudget::new(DEFAULT_NURBS_SURFACE_INVERSION_WORK);
    nurbs_surface_parameter_within_tolerance_with_budget(surface, point, seed, tolerance, &budget)
}

/// Find a NURBS surface parameter pair within `tolerance` using a
/// caller-owned work slice. A negative or non-finite tolerance finds nothing.
pub fn nurbs_surface_parameter_within_tolerance_with_budget(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    tolerance: f64,
    budget: &WorkBudget<'_>,
) -> Option<FinitePoint2> {
    nurbs_surface_parameter_within_nonnegative_tolerance_with_budget(
        surface,
        point,
        seed,
        NonNegativeReal::new(tolerance)?,
        budget,
    )
}

/// Find a NURBS surface parameter pair within an admitted `tolerance` using
/// a caller-owned work slice.
pub fn nurbs_surface_parameter_within_nonnegative_tolerance_with_budget(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    tolerance: NonNegativeReal,
    budget: &WorkBudget<'_>,
) -> Option<FinitePoint2> {
    let tolerance = tolerance.get();
    let (parameters, distance) =
        solve_nurbs_surface_parameter(surface, point, seed, Some(tolerance), budget)?;
    (distance.is_finite() && distance <= tolerance).then_some(parameters)
}

/// `base + Σ factorᵢ · directionᵢ` in model space.
fn offset(base: Point3, terms: &[(f64, Vector3)]) -> Point3 {
    let coordinate = |base: f64, component: fn(&Vector3) -> f64| match crate::math::sum::product_sum(
        std::iter::once(Some([1.0, base])).chain(
            terms
                .iter()
                .map(|(factor, vector)| Some([*factor, component(vector)])),
        ),
    ) {
        crate::math::sum::ProductSum::Value(value) => {
            value.finite().map_or(f64::NAN, FiniteReal::get)
        }
        crate::math::sum::ProductSum::Zero => 0.0,
        crate::math::sum::ProductSum::Undefined => f64::NAN,
    };
    Point3::new(
        coordinate(base.x, |v| v.x),
        coordinate(base.y, |v| v.y),
        coordinate(base.z, |v| v.z),
    )
}

/// Knot span index of `t` for a clamped B-spline basis, or `None` when the
/// knot vector cannot support `count` poles of the given degree.
fn bspline_span(knots: &[f64], degree: usize, count: usize, t: f64) -> Option<usize> {
    if count <= degree || knots.len() < count.checked_add(degree)?.checked_add(1)? {
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
    let finite_t = FiniteReal::new(t);
    let mut values = vec![1.0];
    let mut left = alloc_filled(degree.checked_add(1)?, None, "IR B-spline basis left").ok()?;
    let mut right = alloc_filled(degree.checked_add(1)?, None, "IR B-spline basis right").ok()?;
    for j in 1..=degree {
        // Each knot distance is admitted where it is formed.
        left[j] = FiniteReal::new(t - knots[span + 1 - j]);
        right[j] = FiniteReal::new(knots[span + j] - t);
        let mut saved = 0.0;
        let mut next = alloc_filled(j.checked_add(1)?, 0.0, "IR B-spline basis level").ok()?;
        for (r, &value) in values.iter().enumerate().take(j) {
            // Two finite distances with a finite sum form the scaled ratio. A
            // distance or a sum outside the finite range takes the exact knot
            // differences instead.
            let ratio_terms = right[r + 1].zip(left[j - r]).and_then(|(right, left)| {
                Some((right, left, FiniteReal::new(right.get() + left.get())?))
            });
            let [right_term, left_term] = if let Some((right, left, denominator)) = ratio_terms {
                scaled_ratio_products(FiniteReal::new(value)?, denominator, [right, left])?
                    .map(FiniteReal::get)
            } else {
                let t = finite_t?;
                let [right_knot, left_knot] =
                    FiniteReal::array([knots[span + r + 1], knots[span + 1 - j + r]])?;
                [
                    value
                        * difference_quotient(right_knot, t, right_knot, left_knot)
                            .ok()?
                            .get(),
                    value
                        * difference_quotient(t, left_knot, right_knot, left_knot)
                            .ok()?
                            .get(),
                ]
            };
            next[r] = saved + right_term;
            saved = left_term;
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
            } else if left_denominator.is_finite() {
                degree as f64 * lower_at(index) / left_denominator
            } else {
                FiniteReal::array([
                    degree as f64 * lower_at(index),
                    knots[index + degree],
                    knots[index],
                ])
                .and_then(|[value, end, start]| {
                    difference_quotient(value, FiniteReal::ZERO, end, start).ok()
                })
                .map_or(f64::NAN, FiniteReal::get)
            };
            let right = if right_denominator == 0.0 {
                0.0
            } else if right_denominator.is_finite() {
                degree as f64 * lower_at(index + 1) / right_denominator
            } else {
                FiniteReal::array([
                    degree as f64 * lower_at(index + 1),
                    knots[index + degree + 1],
                    knots[index + 1],
                ])
                .and_then(|[value, end, start]| {
                    difference_quotient(value, FiniteReal::ZERO, end, start).ok()
                })
                .map_or(f64::NAN, FiniteReal::get)
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
            } else if left_denominator.is_finite() {
                degree as f64 * lower_at(index) / left_denominator
            } else {
                FiniteReal::array([
                    degree as f64 * lower_at(index),
                    knots[index + degree],
                    knots[index],
                ])
                .and_then(|[value, end, start]| {
                    difference_quotient(value, FiniteReal::ZERO, end, start).ok()
                })
                .map_or(f64::NAN, FiniteReal::get)
            };
            let right = if right_denominator == 0.0 {
                0.0
            } else if right_denominator.is_finite() {
                degree as f64 * lower_at(index + 1) / right_denominator
            } else {
                FiniteReal::array([
                    degree as f64 * lower_at(index + 1),
                    knots[index + degree + 1],
                    knots[index + 1],
                ])
                .and_then(|[value, end, start]| {
                    difference_quotient(value, FiniteReal::ZERO, end, start).ok()
                })
                .map_or(f64::NAN, FiniteReal::get)
            };
            left - right
        })
        .collect::<Vec<_>>()
        .into()
}

/// Basis derivatives with respect to a local coordinate whose unit is the
/// active knot span. This keeps the coefficients finite when derivatives in
/// the original parameter would exceed binary64 range.
fn bspline_basis_scaled_derivatives(
    knots: &[f64],
    degree: usize,
    span: usize,
    t: f64,
    scale: PositiveReal,
) -> Option<(Vec<f64>, Vec<f64>)> {
    if degree == 0 {
        return Some((vec![0.0], vec![0.0]));
    }
    let lower = bspline_basis(knots, degree - 1, span, t)?;
    let first = bspline_basis_scaled_derivative_level(knots, degree, span, scale, &lower)?;
    let second = if degree == 1 {
        vec![0.0, 0.0]
    } else {
        let lower_lower = bspline_basis(knots, degree - 2, span, t)?;
        let lower_first =
            bspline_basis_scaled_derivative_level(knots, degree - 1, span, scale, &lower_lower)?;
        bspline_basis_scaled_derivative_level(knots, degree, span, scale, &lower_first)?
    };
    Some((first, second))
}

fn bspline_basis_scaled_derivative_level(
    knots: &[f64],
    degree: usize,
    span: usize,
    scale: PositiveReal,
    lower: &[f64],
) -> Option<Vec<f64>> {
    let lower_start = span - (degree - 1);
    let mut derivative = alloc_filled(
        degree.checked_add(1)?,
        0.0,
        "IR scaled B-spline derivative basis",
    )
    .ok()?;
    for (local, derivative_value) in derivative.iter_mut().enumerate() {
        let index = span - degree + local;
        let lower_at = |values: &[f64], global: usize| {
            global
                .checked_sub(lower_start)
                .and_then(|at| values.get(at))
                .copied()
                .unwrap_or(0.0)
        };
        let ratio = |hi: usize, lo: usize| {
            if knots[hi] == knots[lo] {
                Some(0.0)
            } else {
                let [hi_knot, lo_knot] = FiniteReal::array([knots[hi], knots[lo]])?;
                difference_quotient(scale.into(), FiniteReal::ZERO, hi_knot, lo_knot)
                    .ok()
                    .map(FiniteReal::get)
            }
        };
        let left = ratio(index + degree, index)?;
        let right = ratio(index + degree + 1, index + 1)?;
        *derivative_value =
            degree as f64 * (left * lower_at(lower, index) - right * lower_at(lower, index + 1));
        if !derivative_value.is_finite() {
            return None;
        }
    }
    Some(derivative)
}

/// Evaluate a NURBS curve at knot-domain parameter `t` over its admitted
/// poles, or report why it has no finite point there.
///
/// A parameter that is not finite, a knot vector that states no span at `t`,
/// and a zero weight sum have no value. A basis that leaves the finite range
/// reaches no coordinate, and each reads NaN; a projection that overflows
/// carries each coordinate it reached.
pub fn nurbs_curve_point_at(
    curve: &NurbsCurve,
    t: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let poles = curve.pole_rows();
    nurbs_curve_point_evaluation(
        curve.degree(),
        curve.knots(),
        poles.count(),
        |index| poles.point_at(index),
        |index| poles.weight_at(index),
        FiniteReal::new(t).ok_or(EvaluationFailure::NoValue)?,
    )
}

/// The point at `t` of a possibly-rational B-spline over `count` poles that
/// `pole` hands out admitted, or why it has no finite point there.
///
/// A basis that leaves the finite range reaches no coordinate, and each
/// reads NaN; a projection that overflows carries each coordinate it
/// reached. A knot vector or pole list that states no span at `t`, and a zero
/// weight sum, have no value.
fn nurbs_curve_point_evaluation(
    degree: u32,
    knots: &[f64],
    count: usize,
    pole: impl Fn(usize) -> Option<FinitePoint3>,
    weight: impl Fn(usize) -> Option<f64>,
    t: FiniteReal,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let t = t.get();
    let no_value = EvaluationFailure::NoValue;
    let unreached = EvaluationFailure::NonFinite(UNREACHED_POINT);
    let degree = usize::try_from(degree).map_err(|_| no_value)?;
    let span = bspline_span(knots, degree, count, t).ok_or(no_value)?;
    // At a finite parameter over finite knots, the basis is absent or not
    // finite only where one of its terms left the finite range.
    let basis = bspline_basis(knots, degree, span, t).ok_or(unreached)?;
    if !basis.iter().all(|value| value.is_finite()) {
        return Err(unreached);
    }
    let poles = local_poles(span, degree, pole).ok_or(no_value)?;
    let base = homogeneous_curve_sum(&basis, &poles, weight, span - degree).ok_or(no_value)?;
    let [x, y, z] = finite_lanes(base.project(base, &[]).ok_or(no_value)?)
        .map_err(|[x, y, z]| EvaluationFailure::NonFinite(Point3::new(x, y, z)))?;
    Ok(FinitePoint3::from_coordinates(x, y, z))
}

/// The `degree + 1` poles that support span `span`, in parameter order.
fn local_poles(
    span: usize,
    degree: usize,
    pole: impl Fn(usize) -> Option<FinitePoint3>,
) -> Option<Vec<FinitePoint3>> {
    (span - degree..=span).map(pole).collect()
}

/// The homogeneous sum of `poles`, the first of which is global pole `first`,
/// blended by `values`.
fn homogeneous_curve_sum(
    values: &[f64],
    poles: &[FinitePoint3],
    weight: impl Fn(usize) -> Option<f64>,
    first: usize,
) -> Option<Homogeneous> {
    Homogeneous::sum(values.iter().copied().enumerate().map(|(local, basis)| {
        Some((
            [basis, 1.0],
            weight(first + local).unwrap_or(1.0),
            *poles.get(local)?,
        ))
    }))
}

/// Effective knot domain of a structurally evaluable NURBS curve.
pub fn nurbs_curve_parameter_domain(
    curve: &NurbsCurve,
) -> Option<crate::topology::IncreasingParameterInterval> {
    nurbs_pcurve_parameter_domain(curve.degree(), curve.knots(), curve.pole_count())
}

/// Effective knot domain shared by model-space and parameter-space NURBS
/// carriers. The full knot vector contains multiplicity and extrapolation
/// knots; only the interval between the degree-th knot and the control-pole
/// count-th knot is evaluable.
pub fn nurbs_pcurve_parameter_domain(
    degree: u32,
    knots: &[f64],
    control_point_count: usize,
) -> Option<crate::topology::IncreasingParameterInterval> {
    let degree = usize::try_from(degree).ok()?;
    if control_point_count <= degree
        || knots.len() < control_point_count.checked_add(degree)?.checked_add(1)?
    {
        return None;
    }
    let lower = *knots.get(degree)?;
    let upper = *knots.get(control_point_count)?;
    crate::topology::IncreasingParameterInterval::new([lower, upper])
}

const NURBS_SEARCH_MAX_INTERVALS: usize = 512;
const MODEL_CURVE_PARAMETER_SEARCH_MAX_NEWTON_ITERATIONS: usize = 12;

#[derive(Clone, Copy)]
struct NurbsSearchWindow<'a> {
    domain: ParameterInterval,
    boundaries: &'a [FiniteReal],
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
) -> Option<FiniteReal> {
    nurbs_curve_parameter_near_point_with_nonnegative_tolerance(
        curve,
        point,
        NonNegativeLength::new(tolerance)?,
        FiniteReal::new(seed)?,
    )
}

/// [`nurbs_curve_parameter_near_point`] with an admitted tolerance and seed.
fn nurbs_curve_parameter_near_point_with_nonnegative_tolerance(
    curve: &NurbsCurve,
    point: Point3,
    tolerance: NonNegativeLength,
    seed: FiniteReal,
) -> Option<FiniteReal> {
    let tolerance = tolerance.get();
    let degree = usize::try_from(curve.degree()).ok()?;
    let count = curve.control_points().len();
    let domain = ParameterInterval::from(nurbs_curve_parameter_domain(curve)?);
    if degree == 0 || !point.is_finite() {
        return None;
    }
    let weights = validated_nurbs_curve_weights(curve)?;
    let speed_bound = nurbs_curve_speed_bound_about(curve, weights.as_ref(), point)?.get();
    let poles = curve.control_points();
    let distance = |parameter: FiniteReal| {
        let position = nurbs_curve_point_evaluation(
            curve.degree(),
            curve.knots(),
            poles.len(),
            |index| poles.get(index).copied(),
            |index| weights.get(index).copied(),
            parameter,
        )
        .ok()?;
        Some(position.distance(point))
    };
    let seed = domain.project(ExtendedReal::from_finite(seed));
    let boundaries = (degree..=count)
        .map(|index| curve.knots().finite_knot(index))
        .collect::<Option<Vec<_>>>()?;
    match nearest_boundary_witness(&boundaries, seed, tolerance, distance) {
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
        NurbsSearchWindow {
            domain,
            boundaries: &boundaries,
        },
    ) {
        return Some(parameter);
    }
    let mut intervals = bounded_nearest_intervals(&boundaries, seed);
    let mut examined = 0usize;
    while let Some([start, end]) = intervals.pop() {
        examined += 1;
        if examined > NURBS_SEARCH_MAX_INTERVALS {
            return None;
        }
        let middle = start.midpoint(end);
        let middle_distance = distance(middle)?;
        if middle_distance <= tolerance {
            return Some(middle);
        }
        let (start_value, middle_value, end_value) = (start.get(), middle.get(), end.get());
        if middle_distance
            - speed_bound * (end_value - middle_value).max(middle_value - start_value)
            > tolerance
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
    seed: FiniteReal,
    search: NurbsSearchWindow<'_>,
) -> Option<FiniteReal> {
    let window = parameter_interval_containing(search.boundaries, seed).unwrap_or(search.domain);
    let mut parameter = window.project(ExtendedReal::from_finite(seed));
    let poles = curve.control_points();
    for _ in 0..MODEL_CURVE_PARAMETER_SEARCH_MAX_NEWTON_ITERATIONS {
        let position = nurbs_curve_point_evaluation(
            curve.degree(),
            curve.knots(),
            poles.len(),
            |index| poles.get(index).copied(),
            |index| weights.get(index).copied(),
            parameter,
        )
        .ok()?;
        let residual = Vector3::new(
            position.x - point.x,
            position.y - point.y,
            position.z - point.z,
        );
        if residual.norm() <= tolerance {
            return Some(parameter);
        }
        let tangent = nurbs_curve_derivative(
            curve.degree(),
            curve.knots(),
            &poles,
            Some(weights),
            parameter,
            CurveDerivative::First,
        )
        .ok()?
        .get();
        let denominator = tangent.dot(tangent);
        if !denominator.is_finite() || denominator <= 0.0 {
            return None;
        }
        let next = FiniteReal::new(parameter.get() - residual.dot(tangent) / denominator)?;
        let next = window.project(ExtendedReal::from_finite(next));
        if next == parameter {
            return None;
        }
        parameter = next;
    }
    None
}

/// Global model-space speed bound for a structurally valid rational NURBS
/// curve over its effective knot domain.
pub fn nurbs_curve_speed_bound(curve: &NurbsCurve) -> Option<FiniteReal> {
    let weights = validated_nurbs_curve_weights(curve)?;
    nurbs_curve_speed_bound_about(curve, weights.as_ref(), Point3::new(0.0, 0.0, 0.0))
}

fn validated_nurbs_curve_weights(curve: &NurbsCurve) -> Option<Cow<'static, [f64]>> {
    nurbs_curve_parameter_domain(curve)?;
    let count = curve.pole_count();
    let weights: Cow<'static, [f64]> = match curve.weights() {
        Some(weights) => {
            if weights.iter().any(|weight| weight.get() <= 0.0) {
                return None;
            }
            Cow::Owned(weights.into_iter().map(NonZeroReal::get).collect())
        }
        None => Cow::Owned(alloc_filled(count, 1.0, "ir_nurbs_curve_weights").ok()?),
    };
    Some(weights)
}

fn nurbs_curve_speed_bound_about(
    curve: &NurbsCurve,
    weights: &[f64],
    origin: Point3,
) -> Option<FiniteReal> {
    let points = curve
        .pole_rows()
        .raw_points()
        .into_iter()
        .map(<[f64; 3]>::from)
        .collect::<Vec<_>>();
    speed_bound(
        curve.degree(),
        curve.knots(),
        &points,
        weights,
        origin.into(),
    )
}

fn interval_distance_to_parameter(interval: [FiniteReal; 2], parameter: FiniteReal) -> f64 {
    let [lower, upper] = interval.map(FiniteReal::get);
    let parameter = parameter.get();
    if parameter < lower {
        lower - parameter
    } else if parameter > upper {
        parameter - upper
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug)]
struct SearchInterval {
    bounds: [FiniteReal; 2],
    distance: f64,
}

impl PartialEq for SearchInterval {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
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
            .then_with(|| self.bounds[0].get().total_cmp(&other.bounds[0].get()))
            .then_with(|| self.bounds[1].get().total_cmp(&other.bounds[1].get()))
    }
}

/// Retain only the nearest knot intervals that the bounded search can visit.
fn bounded_nearest_intervals(boundaries: &[FiniteReal], seed: FiniteReal) -> Vec<[FiniteReal; 2]> {
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
    Found(FiniteReal),
}

/// Find the nearest admissible distinct boundary without cloning and sorting knots.
fn nearest_boundary_witness<F>(
    boundaries: &[FiniteReal],
    seed: FiniteReal,
    tolerance: f64,
    mut distance: F,
) -> BoundaryWitness
where
    F: FnMut(FiniteReal) -> Option<f64>,
{
    let mut previous_boundary = None;
    let mut nearest = None;
    let mut nearest_seed_distance = f64::INFINITY;
    for &parameter in boundaries {
        if previous_boundary == Some(parameter) {
            continue;
        }
        previous_boundary = Some(parameter);
        let seed_distance = (parameter.get() - seed.get()).abs();
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

fn parameter_interval_containing(
    boundaries: &[FiniteReal],
    parameter: FiniteReal,
) -> Option<ParameterInterval> {
    boundaries.windows(2).find_map(|pair| {
        IncreasingParameterInterval::between(pair[0], pair[1])
            .filter(|_| parameter >= pair[0] && parameter <= pair[1])
            .map(ParameterInterval::from)
    })
}

/// Map a NURBS parameter onto its evaluable knot branch.
///
/// Periodic parameters retain their serialized phase outside this operation
/// and are interpreted modulo the positive knot-domain period.
pub fn map_nurbs_curve_parameter(curve: &NurbsCurve, parameter: FiniteReal) -> Option<FiniteReal> {
    let domain = nurbs_curve_parameter_domain(curve)?;
    let [lower, upper] = domain.endpoints();
    if !curve.periodic() && !(lower..=upper).contains(&parameter.get()) {
        return None;
    }
    let mapped = periodic_parameter(
        curve.knots(),
        usize::try_from(curve.degree()).ok()?,
        curve.pole_count(),
        curve.periodic(),
        parameter,
    )?;
    Some(if curve.periodic() && mapped.get() == upper {
        domain.finite_endpoints()[0]
    } else {
        mapped
    })
}

/// Evaluate a possibly-rational B-spline curve over 2D `(u, v)` poles, or
/// report why it has no finite point there.
///
/// A parameter that is not finite, a pole that is not finite, a knot vector
/// or pole list that states no span at `t`, and a zero weight sum have no
/// value. A basis that leaves the finite range reaches no coordinate, and
/// each reads NaN; a projection that overflows carries each coordinate it
/// reached.
pub fn nurbs_pcurve_uv(
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    t: f64,
) -> Result<FinitePoint2, EvaluationFailure<Point2>> {
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
) -> Option<FiniteReal> {
    use crate::sketches::SketchGeometryDefinition;

    if !linear_tolerance.is_finite() || linear_tolerance < 0.0 {
        return None;
    }
    let (
        SketchGeometryDefinition::Nurbs { curve: source },
        SketchGeometryDefinition::Nurbs { curve: result },
    ) = (source.definition(), result.definition())
    else {
        return None;
    };
    if source.periodic() || result.periodic() {
        return None;
    }
    let source_frames = clamped_nurbs_pcurve_endpoint_frames(source)?;
    let result_frames = clamped_nurbs_pcurve_endpoint_frames(result)?;
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

fn clamped_nurbs_pcurve_endpoint_frames(curve: &PcurveNurbs) -> Option<[(Point2, Point2); 2]> {
    let knots = curve.knots();
    let control_points = curve.pole_rows().raw_points();
    let [lower, upper] =
        nurbs_pcurve_parameter_domain(curve.degree(), knots, control_points.len())?.endpoints();
    let degree = curve.degree() as usize;
    if knots.iter().take(degree + 1).any(|knot| *knot != lower)
        || knots
            .iter()
            .skip(control_points.len())
            .take(degree + 1)
            .any(|knot| *knot != upper)
        || curve
            .weights()
            .is_some_and(|weights| weights.iter().any(|weight| weight.get() <= 0.0))
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
) -> Option<FiniteReal> {
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
        let source_unit =
            FiniteVector3::new(Vector3::new(source_tangent.u, source_tangent.v, 0.0))?
                .unit_nonzero()?;
        let result_unit =
            FiniteVector3::new(Vector3::new(result_tangent.u, result_tangent.v, 0.0))?
                .unit_nonzero()?;
        if source_unit.cross(result_unit).norm() > EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9 {
            return None;
        }
        let offset_projection = |x, y| {
            crate::math::sum::finite_dot(
                [
                    result_point.u,
                    -source_point.u,
                    result_point.v,
                    -source_point.v,
                ],
                [x, x, y, y],
            )
            .ok()
        };
        let tangential = offset_projection(source_unit.x, source_unit.y)?.get();
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
        distances[ordinal] = offset_projection(-source_unit.y, source_unit.x)?.get();
    }
    let scale = 1.0 + distances[0].abs().max(distances[1].abs());
    // Both thresholds are comparison thresholds of this predicate, not the
    // document tolerance: the constant is the double-precision noise floor of
    // the distance arithmetic above, it is never read from the source, and no
    // stated tolerance is stored or reported at the floored magnitude.
    ((distances[0] - distances[1]).abs()
        <= linear_tolerance.max(EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9 * scale)
        && distances[0].abs() > linear_tolerance.max(EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9))
    .then(|| crate::math::interpolate(distances[0], distances[1], 0.5))
    .flatten()
}

struct PcurveDifferential {
    point: FinitePoint2,
    /// The first derivative, or why it has no finite value.
    tangent: Result<FinitePoint2, EvaluationFailure<Point2>>,
    acceleration: Option<FinitePoint2>,
}

/// The parameter-plane pole `(u, v)` as the model-space pole `(u, v, 0)`.
fn planar_pole(pole: FinitePoint2) -> FinitePoint3 {
    let [u, v] = pole.coordinates();
    FinitePoint3::from_coordinates(u, v, FiniteReal::ZERO)
}

/// A parameter-plane value from its two coordinate evaluations: finite when
/// both are, absent when either is, and otherwise non-finite with the value
/// each coordinate reached.
fn planar_value(
    u: Result<FiniteReal, EvaluationFailure<f64>>,
    v: Result<FiniteReal, EvaluationFailure<f64>>,
) -> Result<FinitePoint2, EvaluationFailure<Point2>> {
    let reached = |coordinate: Result<FiniteReal, EvaluationFailure<f64>>| match coordinate {
        Ok(value) => Some(value.get()),
        Err(failure) => failure.non_finite(),
    };
    match (u, v) {
        (Ok(u), Ok(v)) => Ok(FinitePoint2::from_coordinates(u, v)),
        (u, v) => match (reached(u), reached(v)) {
            (Some(u), Some(v)) => Err(EvaluationFailure::NonFinite(Point2::new(u, v))),
            _ => Err(EvaluationFailure::NoValue),
        },
    }
}

/// [`nurbs_pcurve_differential_with`] over raw `(u, v)` poles, each admitted
/// as the span that supports `t` reads it.
fn nurbs_pcurve_differential(
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    t: f64,
) -> Result<PcurveDifferential, EvaluationFailure<Point2>> {
    nurbs_pcurve_differential_with(
        degree,
        knots,
        control_points.len(),
        |index| FinitePoint2::new(*control_points.get(index)?).map(planar_pole),
        weights,
        FiniteReal::new(t).ok_or(EvaluationFailure::NoValue)?,
    )
}

/// The point and first two derivatives at `t` of a possibly-rational
/// B-spline over `count` planar poles that `pole` hands out admitted and
/// placed in the model-space plane `z = 0`.
///
/// A point that overflows is non-finite and carries the value each
/// coordinate reached; a basis that leaves the finite range reaches no
/// coordinate, and each reads NaN. The rational quotient rule forms the
/// derivatives from the finite point, so a curve without one has no
/// derivatives here.
fn nurbs_pcurve_differential_with(
    degree: u32,
    knots: &[f64],
    count: usize,
    pole: impl Fn(usize) -> Option<FinitePoint3>,
    weights: Option<&[f64]>,
    t: FiniteReal,
) -> Result<PcurveDifferential, EvaluationFailure<Point2>> {
    let t = t.get();
    let unreached = EvaluationFailure::NonFinite(Point2::new(f64::NAN, f64::NAN));
    let degree = usize::try_from(degree).map_err(|_| EvaluationFailure::NoValue)?;
    let span = bspline_span(knots, degree, count, t).ok_or(EvaluationFailure::NoValue)?;
    // At a finite parameter over finite knots, the basis is absent or not
    // finite only where one of its terms left the finite range.
    let basis = bspline_basis(knots, degree, span, t).ok_or(unreached)?;
    if !basis.iter().all(|value| value.is_finite()) {
        return Err(unreached);
    }
    let poles = local_poles(span, degree, pole).ok_or(EvaluationFailure::NoValue)?;
    let sum = |values: &[f64]| {
        homogeneous_curve_sum(
            values,
            &poles,
            |index| weights.and_then(|weights| weights.get(index).copied()),
            span - degree,
        )
    };
    let base = sum(&basis).ok_or(EvaluationFailure::NoValue)?;
    let point = finite_lanes(base.project(base, &[]).ok_or(EvaluationFailure::NoValue)?)
        .map_err(|[u, v, _]| EvaluationFailure::NonFinite(Point2::new(u, v)))?;
    let uv = |p: [FiniteReal; 3]| FinitePoint2::from_coordinates(p[0], p[1]);
    let point_only = |tangent| PcurveDifferential {
        point: uv(point),
        tangent: Err(tangent),
        acceleration: None,
    };
    // The derivative basis is absent only where a term of the lower basis
    // left the finite range.
    let Some(mut first_basis) = bspline_basis_derivative(knots, degree, span, t) else {
        return Ok(point_only(unreached));
    };
    let mut second_basis = bspline_basis_second_derivative(knots, degree, span, t);
    let scale = if first_basis.iter().all(|value| value.is_finite())
        && second_basis
            .as_ref()
            .is_none_or(|values| values.iter().all(|value| value.is_finite()))
    {
        PositiveReal::ONE
    } else {
        let width = knots[span + 1] - knots[span];
        let Some(scale) = PositiveReal::new(width) else {
            // A span of zero width has no derivative; the derivative over a
            // width outside the finite range is not reached.
            return Ok(point_only(if width.is_finite() {
                EvaluationFailure::NoValue
            } else {
                unreached
            }));
        };
        if degree == 1 {
            let local_weights = weights.map(|weights| {
                [
                    weights.get(span - 1).copied().unwrap_or(1.0),
                    weights.get(span).copied().unwrap_or(1.0),
                ]
            });
            let derivative = |second| {
                linear_nurbs_derivative(
                    &basis,
                    &poles,
                    local_weights.as_ref().map(<[f64; 2]>::as_slice),
                    1,
                    scale.get(),
                    second,
                )
            };
            // The quotient form divides by the span itself, so each lane is
            // the derivative's coordinate or the signed infinity it reached.
            let lane = |lane: Result<FiniteReal, f64>| lane.map_err(EvaluationFailure::NonFinite);
            return Ok(PcurveDifferential {
                point: uv(point),
                tangent: derivative(false).map_or(Err(EvaluationFailure::NoValue), |[x, y, _]| {
                    planar_value(lane(x), lane(y))
                }),
                acceleration: derivative(true)
                    .and_then(|lanes| finite_lanes(lanes).ok())
                    .map(uv),
            });
        }
        let Some(scaled) = bspline_basis_scaled_derivatives(knots, degree, span, t, scale) else {
            return Ok(point_only(unreached));
        };
        first_basis = scaled.0;
        second_basis = Some(scaled.1);
        scale
    };
    let first_sum = sum(&first_basis);
    let first_lanes = first_sum.and_then(|sum| sum.project(base, &[(sum, point)]));
    let first = first_lanes.and_then(|lanes| finite_lanes(lanes).ok());
    let second = first_sum.zip(first).and_then(|(first_sum, first)| {
        let second_sum = sum(second_basis.as_ref()?)?;
        finite_lanes(second_sum.project(
            base,
            &[(second_sum, point), (first_sum, first), (first_sum, first)],
        )?)
        .ok()
    });
    let unscale_twice = |value: FiniteReal| {
        if scale.get() == 1.0 {
            Some(value)
        } else {
            let value =
                difference_quotient(value, FiniteReal::ZERO, scale.into(), FiniteReal::ZERO)
                    .ok()?;
            difference_quotient(value, FiniteReal::ZERO, scale.into(), FiniteReal::ZERO).ok()
        }
    };
    // A derivative lane over the span is the coordinate over the scale, or
    // the signed infinity its quotient reached; the positive scale leaves an
    // infinity unchanged.
    let tangent_lane = |lane: Result<FiniteReal, f64>| match lane {
        Ok(value) if scale.get() == 1.0 => Ok(value),
        Ok(value) => difference_quotient(value, FiniteReal::ZERO, scale.into(), FiniteReal::ZERO),
        Err(reached) => Err(EvaluationFailure::NonFinite(reached)),
    };
    Ok(PcurveDifferential {
        point: uv(point),
        // A derivative sum or projection is absent only where the derivative
        // basis left the finite range.
        tangent: first_lanes.map_or(Err(unreached), |[x, y, _]| {
            planar_value(tangent_lane(x), tangent_lane(y))
        }),
        acceleration: second.and_then(|value| {
            Some(FinitePoint2::from_coordinates(
                unscale_twice(value[0])?,
                unscale_twice(value[1])?,
            ))
        }),
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
        || !point.is_finite()
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
    if control_points
        .iter()
        .zip(weights)
        .any(|(control, weight)| !control.is_finite() || !weight.is_finite() || *weight <= 0.0)
        || knots.iter().any(|knot| !knot.is_finite())
        || !knots_nondecreasing(knots)
    {
        return None;
    }

    let points = control_points
        .iter()
        .map(|p| [p.u, p.v])
        .collect::<Vec<_>>();
    let speed_bound = speed_bound(degree, knots, &points, weights, [point.u, point.v])?.get();

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
        let middle = start.midpoint(end);
        let curve_uv = Point2::from(
            nurbs_pcurve_uv(degree, knots, control_points, Some(weights), middle).ok()?,
        );
        let distance = (curve_uv.u - point.u).hypot(curve_uv.v - point.v);
        if distance <= tolerance {
            return Some(true);
        }
        let travel_bound = speed_bound * (end - middle).max(middle - start);
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

/// Evaluate a tensor-product NURBS surface at `(u, v)`, or report why it has
/// no finite point there.
///
/// A parameter that is not finite, a knot vector or pole net that states no
/// span at the parameter, and a zero weight sum have no value. A basis that
/// leaves the finite range reaches no coordinate, and each reads NaN; a
/// projection that overflows carries each coordinate it reached.
pub fn nurbs_surface_point(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let [x, y, z] = nurbs_surface_local(surface, u_at, v_at)?.point;
    Ok(FinitePoint3::from_coordinates(x, y, z))
}

/// A tensor-product NURBS surface at a parameter: its spans, its bases and
/// its homogeneous base sum, with its finite point.
struct NurbsSurfaceLocal<'a> {
    surface: &'a NurbsSurface,
    degrees: [usize; 2],
    spans: [usize; 2],
    parameters: [f64; 2],
    bases: [Vec<f64>; 2],
    base: Homogeneous,
    point: [FiniteReal; 3],
}

/// The first partials of a NURBS surface at a parameter: the derivative
/// bases, the homogeneous derivative sums and the finite lanes.
struct NurbsSurfaceFirstPartials {
    bases: [Vec<f64>; 2],
    sums: [Homogeneous; 2],
    lanes: [[FiniteReal; 3]; 2],
}

/// The homogeneous sum of a NURBS surface's poles local to `spans`, blended
/// by `u_values` along `u` and `v_values` along `v`. A missing pole and a
/// value that is not finite leave no sum.
fn nurbs_local_sum(
    surface: &NurbsSurface,
    [u_degree, v_degree]: [usize; 2],
    [u_span, v_span]: [usize; 2],
    u_values: &[f64],
    v_values: &[f64],
) -> Option<Homogeneous> {
    Homogeneous::sum(
        u_values
            .iter()
            .copied()
            .enumerate()
            .flat_map(|(i, u_value)| {
                v_values
                    .iter()
                    .copied()
                    .enumerate()
                    .map(move |(j, v_value)| {
                        let (pole_u, pole_v) = (u_span - u_degree + i, v_span - v_degree + j);
                        Some((
                            [u_value, v_value],
                            surface.weight(pole_u, pole_v).map_or(1.0, NonZeroReal::get),
                            surface.pole(pole_u, pole_v)?,
                        ))
                    })
            }),
    )
}

impl NurbsSurfaceLocal<'_> {
    /// The homogeneous sum of the local poles blended by `u_values` along
    /// `u` and `v_values` along `v`.
    fn sum(&self, u_values: &[f64], v_values: &[f64]) -> Option<Homogeneous> {
        nurbs_local_sum(self.surface, self.degrees, self.spans, u_values, v_values)
    }

    /// The first partials, or why they have none. At the finite point over
    /// finite knots, a derivative basis that is absent, a sum that is absent
    /// and a projection that is absent or overflows each left the finite
    /// range.
    fn first(&self) -> Result<NurbsSurfaceFirstPartials, EvaluationFailure<()>> {
        let non_finite = EvaluationFailure::NonFinite(());
        let knots = [self.surface.u_knots(), self.surface.v_knots()];
        let derivative = |axis: usize| {
            bspline_basis_derivative(
                knots[axis],
                self.degrees[axis],
                self.spans[axis],
                self.parameters[axis],
            )
            .ok_or(non_finite)
        };
        let bases = [derivative(0)?, derivative(1)?];
        let u = self.sum(&bases[0], &self.bases[1]).ok_or(non_finite)?;
        let v = self.sum(&self.bases[0], &bases[1]).ok_or(non_finite)?;
        let lane = |sum: Homogeneous| {
            finite_lanes(
                sum.project(self.base, &[(sum, self.point)])
                    .ok_or(non_finite)?,
            )
            .map_err(|_| non_finite)
        };
        let lanes = [lane(u)?, lane(v)?];
        Ok(NurbsSurfaceFirstPartials {
            bases,
            sums: [u, v],
            lanes,
        })
    }

    /// The second partials over `first`, or why they have none, in the terms
    /// of [`Self::first`].
    fn second(
        &self,
        first: &NurbsSurfaceFirstPartials,
    ) -> Result<[[FiniteReal; 3]; 3], EvaluationFailure<()>> {
        let non_finite = EvaluationFailure::NonFinite(());
        let knots = [self.surface.u_knots(), self.surface.v_knots()];
        let second = |axis: usize| {
            bspline_basis_second_derivative(
                knots[axis],
                self.degrees[axis],
                self.spans[axis],
                self.parameters[axis],
            )
            .ok_or(non_finite)
        };
        let [u_second, v_second] = [second(0)?, second(1)?];
        let [u, v] = first.sums;
        let [du, dv] = first.lanes;
        let uu = self.sum(&u_second, &self.bases[1]).ok_or(non_finite)?;
        let uv = self
            .sum(&first.bases[0], &first.bases[1])
            .ok_or(non_finite)?;
        let vv = self.sum(&self.bases[0], &v_second).ok_or(non_finite)?;
        let lane = |sum: Homogeneous, corrections: &[(Homogeneous, [FiniteReal; 3])]| {
            finite_lanes(sum.project(self.base, corrections).ok_or(non_finite)?)
                .map_err(|_| non_finite)
        };
        Ok([
            lane(uu, &[(uu, self.point), (u, du), (u, du)])?,
            lane(uv, &[(uv, self.point), (u, dv), (v, du)])?,
            lane(vv, &[(vv, self.point), (v, dv), (v, dv)])?,
        ])
    }
}

/// A tensor-product NURBS surface at `(u, v)`, or why it has no finite
/// point there.
///
/// A basis that leaves the finite range reaches no coordinate, and each
/// coordinate reads NaN; a projection that overflows carries each coordinate
/// it reached. A parameter that is not finite, a knot vector or pole net that
/// states no span at the parameter, and a zero weight sum have no value.
fn nurbs_surface_local(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<NurbsSurfaceLocal<'_>, EvaluationFailure<Point3>> {
    let no_value = EvaluationFailure::NoValue;
    let unreached = EvaluationFailure::NonFinite(Point3::new(f64::NAN, f64::NAN, f64::NAN));
    let u_degree = usize::try_from(surface.u_degree()).map_err(|_| no_value)?;
    let v_degree = usize::try_from(surface.v_degree()).map_err(|_| no_value)?;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let u_at = periodic_parameter(
        surface.u_knots(),
        u_degree,
        u_count,
        surface.u_periodic(),
        FiniteReal::new(u_at).ok_or(no_value)?,
    )
    .ok_or(no_value)?
    .get();
    let v_at = periodic_parameter(
        surface.v_knots(),
        v_degree,
        v_count,
        surface.v_periodic(),
        FiniteReal::new(v_at).ok_or(no_value)?,
    )
    .ok_or(no_value)?
    .get();
    let u_span = bspline_span(surface.u_knots(), u_degree, u_count, u_at).ok_or(no_value)?;
    let v_span = bspline_span(surface.v_knots(), v_degree, v_count, v_at).ok_or(no_value)?;
    // At a finite parameter over finite knots, the basis is absent or not
    // finite only where one of its terms left the finite range.
    let u_basis = bspline_basis(surface.u_knots(), u_degree, u_span, u_at).ok_or(unreached)?;
    let v_basis = bspline_basis(surface.v_knots(), v_degree, v_span, v_at).ok_or(unreached)?;
    if !u_basis
        .iter()
        .chain(&v_basis)
        .all(|value| value.is_finite())
    {
        return Err(unreached);
    }
    let degrees = [u_degree, v_degree];
    let spans = [u_span, v_span];
    let base = nurbs_local_sum(surface, degrees, spans, &u_basis, &v_basis).ok_or(no_value)?;
    let point = finite_lanes(base.project(base, &[]).ok_or(no_value)?)
        .map_err(|[x, y, z]| EvaluationFailure::NonFinite(Point3::new(x, y, z)))?;
    Ok(NurbsSurfaceLocal {
        surface,
        degrees,
        spans,
        parameters: [u_at, v_at],
        bases: [u_basis, v_basis],
        base,
        point,
    })
}

/// A NURBS surface's point and first partials at `(u, v)`, the partials with
/// their own outcome, or why the point has none.
fn nurbs_surface_first_order(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let local = nurbs_surface_local(surface, u_at, v_at)?;
    let [x, y, z] = local.point;
    Ok(SurfaceFirstOrder {
        point: FinitePoint3::from_coordinates(x, y, z),
        first: local.first().map(|first| first.lanes.map(finite_vector)),
    })
}

/// A NURBS surface's point with its first and second partials at `(u, v)`,
/// each order with its own outcome, or why the point has none.
fn nurbs_surface_jet(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    let local = nurbs_surface_local(surface, u_at, v_at)?;
    let [x, y, z] = local.point;
    let first = local.first();
    let second = first
        .as_ref()
        .map_err(|failure| *failure)
        .and_then(|first| local.second(first));
    Ok(SurfaceJet {
        point: FinitePoint3::from_coordinates(x, y, z),
        first: first.map(|first| first.lanes.map(finite_vector)),
        second: second.map(|lanes| lanes.map(finite_vector)),
    })
}

/// The vector of three finite lanes.
fn finite_vector([x, y, z]: [FiniteReal; 3]) -> FiniteVector3 {
    FiniteVector3::from_components(x, y, z)
}

/// [`nurbs_surface_point`] within a caller-owned work slice. A refused
/// charge leaves no value.
pub fn nurbs_surface_point_with_budget(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
    budget: &WorkBudget<'_>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let cost = nurbs_surface_evaluation_cost(surface).ok_or(EvaluationFailure::NoValue)?;
    if !budget.charge_by(cost) {
        return Err(EvaluationFailure::NoValue);
    }
    nurbs_surface_point(surface, u_at, v_at)
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
    let u_degree = usize::try_from(surface.u_degree()).ok()?;
    let v_degree = usize::try_from(surface.v_degree()).ok()?;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let (fixed_degree, fixed_count, fixed_knots, fixed_periodic) = match fixed_axis {
        SurfaceParameterAxis::U => (u_degree, u_count, surface.u_knots(), surface.u_periodic()),
        SurfaceParameterAxis::V => (v_degree, v_count, surface.v_knots(), surface.v_periodic()),
    };
    let fixed_parameter = periodic_parameter(
        fixed_knots,
        fixed_degree,
        fixed_count,
        fixed_periodic,
        FiniteReal::new(fixed_parameter)?,
    )?
    .get();
    let fixed_span = bspline_span(fixed_knots, fixed_degree, fixed_count, fixed_parameter)?;
    let fixed_basis = bspline_basis(fixed_knots, fixed_degree, fixed_span, fixed_parameter)?;
    let varying_count = match fixed_axis {
        SurfaceParameterAxis::U => v_count,
        SurfaceParameterAxis::V => u_count,
    };
    let rational = surface.weights().is_some();
    let mut control_points = Vec::with_capacity(varying_count);
    let mut sums = Vec::with_capacity(varying_count);
    for varying in 0..varying_count {
        let sum = Homogeneous::sum(fixed_basis.iter().copied().enumerate().map(
            |(local, basis)| {
                let fixed = fixed_span - fixed_degree + local;
                let (pole_u, pole_v) = match fixed_axis {
                    SurfaceParameterAxis::U => (fixed, varying),
                    SurfaceParameterAxis::V => (varying, fixed),
                };
                Some((
                    [basis, 1.0],
                    surface.weight(pole_u, pole_v).map_or(1.0, NonZeroReal::get),
                    surface.pole(pole_u, pole_v)?,
                ))
            },
        ))?;
        let [x, y, z] = finite_lanes(sum.project(sum, &[])?).ok()?;
        control_points.push(FinitePoint3::from_coordinates(x, y, z));
        sums.push(sum);
    }
    let (degree, knots, periodic) = match fixed_axis {
        SurfaceParameterAxis::U => (
            surface.v_degree(),
            surface.v_knots().to_vec(),
            surface.v_periodic(),
        ),
        SurfaceParameterAxis::V => (
            surface.u_degree(),
            surface.u_knots().to_vec(),
            surface.u_periodic(),
        ),
    };
    let weights = if rational {
        Some(Homogeneous::weights(&sums)?)
    } else {
        None
    };
    NurbsCurve::from_lanes(degree, knots, control_points, weights, periodic).ok()
}

/// Why an evaluator has no finite value at its input.
///
/// [`NoValue`](Self::NoValue) states that there is no value: the input is
/// not a finite parameter, the carrier states no value there, or a step the
/// value needs is undefined, such as the angle of a polar chart at its
/// origin.
///
/// [`NonFinite`](Self::NonFinite) states that the evaluation left the finite
/// range, and carries the value it reached. A coordinate that no step
/// reached is NaN. Where the value an evaluator returns includes
/// derivatives, as a surface's partials do, a derivative that leaves the
/// finite range leaves the evaluation there, and the point it carries can be
/// finite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EvaluationFailure<R> {
    /// There is no value at the input.
    NoValue,
    /// The evaluation left the finite range and reached this value.
    NonFinite(R),
}

impl<R> EvaluationFailure<R> {
    /// The value a non-finite evaluation reached, absent for an input with no
    /// value.
    pub fn non_finite(self) -> Option<R> {
        match self {
            Self::NoValue => None,
            Self::NonFinite(value) => Some(value),
        }
    }

    /// The same failure with the reached value mapped by `reach`.
    pub fn map<S>(self, reach: impl FnOnce(R) -> S) -> EvaluationFailure<S> {
        match self {
            Self::NoValue => EvaluationFailure::NoValue,
            Self::NonFinite(value) => EvaluationFailure::NonFinite(reach(value)),
        }
    }
}

/// The point of an evaluation that left the finite range before it reached
/// any coordinate.
const UNREACHED_POINT: Point3 = Point3 {
    x: f64::NAN,
    y: f64::NAN,
    z: f64::NAN,
};

/// Admit an evaluated model-space point, or report the non-finite point.
fn admit_point(point: Point3) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    FinitePoint3::new(point).ok_or(EvaluationFailure::NonFinite(point))
}

/// Admit an evaluated parameter-space value, or report the non-finite value.
fn admit_parameter_point(point: Point2) -> Result<FinitePoint2, EvaluationFailure<Point2>> {
    FinitePoint2::new(point).ok_or(EvaluationFailure::NonFinite(point))
}

/// Point and first partial derivatives of a surface in its stored
/// parameterization. An evaluator that admits every lane returns the
/// `SurfacePartials<FinitePoint3, FiniteVector3>` instantiation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfacePartials<P = Point3, V = Vector3> {
    /// Surface point at `(u, v)`.
    pub point: P,
    /// First partial derivative with respect to `u`.
    pub du: V,
    /// First partial derivative with respect to `v`.
    pub dv: V,
}

impl SurfacePartials<FinitePoint3, FiniteVector3> {
    /// The partials with raw lanes, for a reader that computes with them.
    #[must_use]
    pub fn into_raw(self) -> SurfacePartials {
        SurfacePartials {
            point: self.point.get(),
            du: self.du.get(),
            dv: self.dv.get(),
        }
    }
}

/// Point, first partials, and second partials of a surface in its stored
/// parameterization. An evaluator that admits every lane returns the
/// `SurfaceSecondPartials<FinitePoint3, FiniteVector3>` instantiation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceSecondPartials<P = Point3, V = Vector3> {
    /// Surface point at `(u, v)`.
    pub point: P,
    /// First partial derivative with respect to `u`.
    pub du: V,
    /// First partial derivative with respect to `v`.
    pub dv: V,
    /// Second partial derivative with respect to `u`.
    pub duu: V,
    /// Mixed partial derivative.
    pub duv: V,
    /// Second partial derivative with respect to `v`.
    pub dvv: V,
}

impl SurfaceSecondPartials<FinitePoint3, FiniteVector3> {
    /// The partials with raw lanes, for a reader that computes with them.
    #[must_use]
    pub fn into_raw(self) -> SurfaceSecondPartials {
        SurfaceSecondPartials {
            point: self.point.get(),
            du: self.du.get(),
            dv: self.dv.get(),
            duu: self.duu.get(),
            duv: self.duv.get(),
            dvv: self.dvv.get(),
        }
    }
}

/// A surface's finite point with its first and second partials, each order
/// with its own outcome: an order that has no value there, or that left the
/// finite range, leaves the point and the other order as they are. A reader
/// fails on the orders it reads.
#[derive(Clone, Copy)]
struct SurfaceJet {
    point: FinitePoint3,
    first: Result<[FiniteVector3; 2], EvaluationFailure<()>>,
    second: Result<[FiniteVector3; 3], EvaluationFailure<()>>,
}

impl SurfaceJet {
    /// The jet an arm forms at a finite point from raw lanes: an order with a
    /// lane outside the finite range left it.
    fn formed(
        point: FinitePoint3,
        first: Result<[Vector3; 2], EvaluationFailure<()>>,
        second: Result<[Vector3; 3], EvaluationFailure<()>>,
    ) -> Self {
        Self {
            point,
            first: first.and_then(admit_lanes),
            second: second.and_then(admit_lanes),
        }
    }

    /// An order's failure carrying the finite point.
    fn at_point(self, failure: EvaluationFailure<()>) -> EvaluationFailure<Point3> {
        failure.map(|()| self.point.get())
    }

    /// The point with the first partials.
    fn first_order(self) -> SurfaceFirstOrder {
        SurfaceFirstOrder {
            point: self.point,
            first: self.first,
        }
    }

    /// The point with both orders, or the failure of the first order that has
    /// none, carrying the finite point.
    fn second_partials(
        self,
    ) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
        let [du, dv] = self.first.map_err(|failure| self.at_point(failure))?;
        let [duu, duv, dvv] = self.second.map_err(|failure| self.at_point(failure))?;
        Ok(SurfaceSecondPartials {
            point: self.point,
            du,
            dv,
            duu,
            duv,
            dvv,
        })
    }
}

/// A surface's finite point with its first partials, which state their own
/// outcome.
#[derive(Clone, Copy)]
struct SurfaceFirstOrder {
    point: FinitePoint3,
    first: Result<[FiniteVector3; 2], EvaluationFailure<()>>,
}

impl SurfaceFirstOrder {
    /// The point and first partials, or the first partials' failure carrying
    /// the finite point.
    fn partials(
        self,
    ) -> Result<SurfacePartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
        let [du, dv] = self
            .first
            .map_err(|failure| failure.map(|()| self.point.get()))?;
        Ok(SurfacePartials {
            point: self.point,
            du,
            dv,
        })
    }
}

/// Admit the lanes of one partial order together: a lane outside the finite
/// range leaves the order there.
fn admit_lanes<const N: usize>(
    lanes: [Vector3; N],
) -> Result<[FiniteVector3; N], EvaluationFailure<()>> {
    FiniteVector3::array(lanes).ok_or(EvaluationFailure::NonFinite(()))
}

/// Evaluate a tensor-product NURBS surface and its exact rational first
/// partials at `(u, v)`, or report why they have no finite value there.
///
/// The point fails as [`nurbs_surface_point`] states. At a finite point,
/// first partials outside the finite range leave the evaluation there,
/// carrying the point.
pub fn nurbs_surface_partials(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<SurfacePartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    nurbs_surface_first_order(surface, u_at, v_at)?.partials()
}

/// [`nurbs_surface_partials`] within a caller-owned work slice. A refused
/// charge leaves no value.
pub fn nurbs_surface_partials_with_budget(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
    budget: &WorkBudget<'_>,
) -> Result<SurfacePartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    charge_nurbs_surface_partials(surface, budget)?;
    nurbs_surface_partials(surface, u_at, v_at)
}

/// Charge the work of a NURBS surface's partials to `budget`: a cost that
/// does not fit and a refused charge leave no value.
fn charge_nurbs_surface_partials(
    surface: &NurbsSurface,
    budget: &WorkBudget<'_>,
) -> Result<(), EvaluationFailure<Point3>> {
    nurbs_surface_partials_evaluation_cost(surface)
        .is_some_and(|cost| budget.charge_by(cost))
        .then_some(())
        .ok_or(EvaluationFailure::NoValue)
}

/// Evaluate a tensor-product NURBS surface and its exact rational first and
/// second partials at `(u, v)`, or report why they have no finite value
/// there.
///
/// The point fails as [`nurbs_surface_point`] states. At a finite point,
/// partials outside the finite range leave the evaluation there, carrying
/// the point.
pub fn nurbs_surface_second_partials(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    nurbs_surface_jet(surface, u_at, v_at)?.second_partials()
}

/// [`nurbs_surface_second_partials`] within a caller-owned work slice. A
/// refused charge leaves no value.
pub fn nurbs_surface_second_partials_with_budget(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
    budget: &WorkBudget<'_>,
) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    charge_nurbs_surface_partials(surface, budget)?;
    nurbs_surface_second_partials(surface, u_at, v_at)
}

fn nurbs_surface_partials_evaluation_cost(surface: &NurbsSurface) -> Option<usize> {
    let (u_support, v_support) = nurbs_surface_support_sizes(surface)?;
    let control_work = u_support.checked_mul(v_support)?;
    let u_basis_work = u_support.checked_mul(u_support)?.checked_mul(3)?;
    let v_basis_work = v_support.checked_mul(v_support)?.checked_mul(3)?;
    control_work
        .checked_add(u_basis_work)?
        .checked_add(v_basis_work)
}

fn nurbs_surface_support_sizes(surface: &NurbsSurface) -> Option<(usize, usize)> {
    Some((
        usize::try_from(surface.u_degree()).ok()?.checked_add(1)?,
        usize::try_from(surface.v_degree()).ok()?.checked_add(1)?,
    ))
}

fn periodic_parameter(
    knots: &[f64],
    degree: usize,
    count: usize,
    periodic: bool,
    parameter: FiniteReal,
) -> Option<FiniteReal> {
    let start = *knots.get(degree)?;
    let end = *knots.get(count)?;
    if !periodic || (start..=end).contains(&parameter.get()) {
        return Some(parameter);
    }
    crate::math::wrap_parameter(parameter.get(), start, end)
}

/// Evaluate the exact first derivative of a directly stored curve, or report
/// why it has no finite value.
///
/// A parameter that is not finite, a degenerate carrier, a polyline
/// parameter outside its segments or at a vertex whose segments disagree,
/// and a NURBS span of zero width have no derivative. A derivative that
/// leaves the finite range is non-finite; it carries no value.
pub fn curve_tangent_solved(
    geometry: &SolvedCurveGeometry,
    t: f64,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    curve_derivative_evaluation(geometry, t, CurveDerivative::First)
}

/// Evaluate the exact second derivative of a directly stored curve, or report
/// why it has no finite value, as [`curve_tangent_solved`] states.
pub fn curve_second_derivative_solved(
    geometry: &SolvedCurveGeometry,
    t: f64,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    curve_derivative_evaluation(geometry, t, CurveDerivative::Second)
}

/// Evaluate a directly stored curve at `t` within a caller-owned work slice.
/// Analytic curves are constant-cost; transformed, polyline, and NURBS curves
/// charge the work performed by their representation. A refused charge
/// leaves no value; otherwise the failure is [`curve_point_solved`]'s.
pub fn curve_point_with_budget_solved(
    geometry: &SolvedCurveGeometry,
    t: f64,
    budget: &WorkBudget<'_>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    /// The descent is bounded by [`PlacedCurve`](crate::geometry::PlacedCurve)
    /// construction; no arm follows an arena id.
    fn evaluate(
        geometry: &SolvedCurveGeometry,
        t: f64,
        budget: &WorkBudget<'_>,
    ) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
        let charged = |cost: Option<usize>| {
            cost.is_some_and(|cost| budget.charge_by(cost))
                .then_some(())
                .ok_or(EvaluationFailure::NoValue)
        };
        match geometry {
            SolvedCurveGeometry::Nurbs(nurbs) => {
                charged(nurbs_curve_evaluation_cost(nurbs))?;
                curve_point_solved(geometry, t)
            }
            SolvedCurveGeometry::Polyline(polyline) => {
                charged(Some(polyline.point_count()))?;
                curve_point_solved(geometry, t)
            }
            SolvedCurveGeometry::Transformed(placed) => {
                if !budget.charge() {
                    return Err(EvaluationFailure::NoValue);
                }
                placed_point(*placed.transform(), evaluate(placed.basis(), t, budget))
            }
            _ => curve_point_solved(geometry, t),
        }
    }

    evaluate(geometry, t, budget)
}

/// [`curve_tangent_solved`] within a caller-owned work slice. A refused
/// charge leaves no value.
pub fn curve_tangent_with_budget_solved(
    geometry: &SolvedCurveGeometry,
    t: f64,
    budget: &WorkBudget<'_>,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    curve_derivative_with_budget_evaluation(geometry, t, CurveDerivative::First, budget)
}

/// [`curve_second_derivative_solved`] within a caller-owned work slice. A
/// refused charge leaves no value.
pub fn curve_second_derivative_with_budget_solved(
    geometry: &SolvedCurveGeometry,
    t: f64,
    budget: &WorkBudget<'_>,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    curve_derivative_with_budget_evaluation(geometry, t, CurveDerivative::Second, budget)
}

fn nurbs_curve_evaluation_cost(curve: &NurbsCurve) -> Option<usize> {
    let support = usize::try_from(curve.degree()).ok()?.checked_add(1)?;
    support.checked_mul(support).filter(|cost| *cost > 0)
}

fn nurbs_curve_derivative_evaluation_cost(
    curve: &NurbsCurve,
    basis_levels: usize,
) -> Option<usize> {
    nurbs_curve_evaluation_cost(curve)?.checked_mul(basis_levels)
}

/// The order of a curve derivative.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CurveDerivative {
    First,
    Second,
}

/// [`curve_derivative_evaluation`] within a caller-owned work slice. A
/// refused charge leaves no value.
///
/// The descent is bounded by [`PlacedCurve`](crate::geometry::PlacedCurve)
/// construction; no arm follows an arena id.
fn curve_derivative_with_budget_evaluation(
    geometry: &SolvedCurveGeometry,
    t: f64,
    order: CurveDerivative,
    budget: &WorkBudget<'_>,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    let charged = |cost: Option<usize>| {
        cost.is_some_and(|cost| budget.charge_by(cost))
            .then_some(())
            .ok_or(EvaluationFailure::NoValue)
    };
    match geometry {
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let basis_levels = match order {
                CurveDerivative::First => 2,
                CurveDerivative::Second => 3,
            };
            charged(nurbs_curve_derivative_evaluation_cost(nurbs, basis_levels))?;
            curve_derivative_evaluation(geometry, t, order)
        }
        SolvedCurveGeometry::Polyline(polyline) => {
            charged(Some(polyline.point_count()))?;
            curve_derivative_evaluation(geometry, t, order)
        }
        SolvedCurveGeometry::Transformed(placed) => {
            if !budget.charge() {
                return Err(EvaluationFailure::NoValue);
            }
            placed_derivative(
                *placed.transform(),
                curve_derivative_with_budget_evaluation(placed.basis(), t, order, budget),
            )
        }
        _ => curve_derivative_evaluation(geometry, t, order),
    }
}

/// A placed carrier's derivative from its basis derivative. A derivative the
/// placement leaves outside the finite range is non-finite.
fn placed_derivative(
    transform: Transform,
    basis: Result<FiniteVector3, EvaluationFailure<()>>,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    transform
        .apply_vector(basis?.get())
        .ok_or(EvaluationFailure::NonFinite(()))
}

/// Admit a derivative an arm computes raw: a component outside the finite
/// range leaves the derivative there.
fn admit_derivative(derivative: Vector3) -> Result<FiniteVector3, EvaluationFailure<()>> {
    FiniteVector3::new(derivative).ok_or(EvaluationFailure::NonFinite(()))
}

/// The exact derivative of order `order` of a directly stored curve at `t`,
/// or why it has none there.
///
/// A parameter that is not finite, a degenerate carrier, a polyline
/// parameter outside its segments or at a vertex whose segments disagree,
/// and a NURBS span of zero width have no derivative. A derivative that
/// leaves the finite range is non-finite; it carries no value, because its
/// readers carry the point instead.
///
/// The descent is bounded by [`PlacedCurve`](crate::geometry::PlacedCurve)
/// construction; no arm follows an arena id.
fn curve_derivative_evaluation(
    geometry: &SolvedCurveGeometry,
    t: f64,
    order: CurveDerivative,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    let parameter = FiniteReal::new(t).ok_or(EvaluationFailure::NoValue)?;
    let t = parameter.get();
    let second = order == CurveDerivative::Second;
    match geometry {
        SolvedCurveGeometry::Line(line_curve) => Ok(if second {
            FiniteVector3::ZERO
        } else {
            FiniteVector3::from(line_curve.direction())
        }),
        SolvedCurveGeometry::Circle(circle_curve) => {
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius().get();
            admit_derivative(vector_sum(&if second {
                [
                    (-radius * t.cos(), *ref_direction),
                    (-radius * t.sin(), axis.cross(*ref_direction)),
                ]
            } else {
                [
                    (-radius * t.sin(), *ref_direction),
                    (radius * t.cos(), axis.cross(*ref_direction)),
                ]
            }))
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            admit_derivative(vector_sum(&if second {
                [
                    (-major_radius * t.cos(), *major_direction),
                    (-minor_radius * t.sin(), axis.cross(*major_direction)),
                ]
            } else {
                [
                    (-major_radius * t.sin(), *major_direction),
                    (minor_radius * t.cos(), axis.cross(*major_direction)),
                ]
            }))
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let axis = parabola_curve.frame().axis().as_raw();
            let major_direction = parabola_curve.frame().reference().as_raw();
            let focal_distance = parabola_curve.focal_distance().get();
            // The products of finite factors are absent only where they
            // overflow.
            let product = |value: Option<FiniteReal>| {
                value
                    .map(FiniteReal::get)
                    .ok_or(EvaluationFailure::NonFinite(()))
            };
            let curvature = product(product_quotient([2.0, focal_distance], []))?;
            admit_derivative(if second {
                vector_sum(&[(curvature, *major_direction)])
            } else {
                vector_sum(&[
                    (
                        product(product_quotient([2.0, focal_distance, t], []))?,
                        *major_direction,
                    ),
                    (curvature, axis.cross(*major_direction)),
                ])
            })
        }
        SolvedCurveGeometry::Hyperbola(hyperbola_curve) => {
            let axis = hyperbola_curve.frame().axis().as_raw();
            let major_direction = hyperbola_curve.frame().reference().as_raw();
            let major_radius = hyperbola_curve.major_radius().magnitude();
            let minor_radius = Length::from(hyperbola_curve.minor_radius()).magnitude();
            // A product the derivative reads outside the finite range leaves
            // the derivative there.
            let [major_sinh, major_cosh] =
                sinh_cosh_lanes(scaled_sinh_cosh(major_radius, parameter));
            let [minor_sinh, minor_cosh] =
                sinh_cosh_lanes(scaled_sinh_cosh(minor_radius, parameter));
            let (major, minor) = if second {
                (major_cosh, minor_sinh)
            } else {
                (major_sinh, minor_cosh)
            };
            let (Ok(major), Ok(minor)) = (major, minor) else {
                return Err(EvaluationFailure::NonFinite(()));
            };
            admit_derivative(vector_sum(&[
                (major.get(), *major_direction),
                (minor.get(), axis.cross(*major_direction)),
            ]))
        }
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let parameter =
                map_nurbs_curve_parameter(nurbs, parameter).ok_or(EvaluationFailure::NoValue)?;
            nurbs_curve_derivative(
                nurbs.degree(),
                nurbs.knots(),
                &nurbs.control_points(),
                nurbs.pole_rows().weights().as_deref(),
                parameter,
                order,
            )
        }
        SolvedCurveGeometry::Polyline(polyline) => {
            let (points, parameters) = polyline_samples(polyline);
            let tangent = polyline_tangent(&points, &parameters, t)?;
            Ok(if second { FiniteVector3::ZERO } else { tangent })
        }
        SolvedCurveGeometry::Transformed(placed) => placed_derivative(
            *placed.transform(),
            curve_derivative_evaluation(placed.basis(), t, order),
        ),
        SolvedCurveGeometry::Degenerate(_)
        | SolvedCurveGeometry::Composite { .. }
        | SolvedCurveGeometry::Unknown { .. } => Err(EvaluationFailure::NoValue),
    }
}

/// The products `scale * sinh(parameter)` and `scale * cosh(parameter)` of a
/// [`scaled_sinh_cosh`] pair, each finite or the plain product it reached. A
/// pair outside the finite range carries both plain products, so one of
/// them can be finite.
fn sinh_cosh_lanes(
    pair: Result<(FiniteReal, FiniteReal), (f64, f64)>,
) -> [Result<FiniteReal, f64>; 2] {
    match pair {
        Ok((sinh, cosh)) => [Ok(sinh), Ok(cosh)],
        Err((sinh, cosh)) => [
            FiniteReal::new(sinh).ok_or(sinh),
            FiniteReal::new(cosh).ok_or(cosh),
        ],
    }
}

/// The derivative of order `order` at `t` of a possibly-rational B-spline
/// curve, or why it has none there.
///
/// At a finite parameter over finite knots, a basis that is absent or not
/// finite, and a derivative basis the span's scale cannot bring into range,
/// left the finite range, as did a point or derivative projection that
/// overflows. A knot vector or pole list that states no span at `t`, a zero
/// weight sum, and a span of zero width have no value.
fn nurbs_curve_derivative(
    degree: u32,
    knots: &[f64],
    control_points: &[FinitePoint3],
    weights: Option<&[f64]>,
    t: FiniteReal,
    order: CurveDerivative,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    let t = t.get();
    let no_value = EvaluationFailure::NoValue;
    let non_finite = EvaluationFailure::NonFinite(());
    let second = order == CurveDerivative::Second;
    let degree = usize::try_from(degree).map_err(|_| no_value)?;
    let span = bspline_span(knots, degree, control_points.len(), t).ok_or(no_value)?;
    let basis = bspline_basis(knots, degree, span, t).ok_or(non_finite)?;
    if !basis.iter().all(|value| value.is_finite()) {
        return Err(non_finite);
    }
    let mut first_basis = bspline_basis_derivative(knots, degree, span, t).ok_or(non_finite)?;
    let mut second_basis = if second {
        bspline_basis_second_derivative(knots, degree, span, t).ok_or(non_finite)?
    } else {
        Vec::new()
    };
    let scale = if first_basis
        .iter()
        .chain(&second_basis)
        .all(|value| value.is_finite())
    {
        PositiveReal::ONE
    } else {
        let width = knots[span + 1] - knots[span];
        let scale = PositiveReal::new(width).ok_or(if width.is_finite() {
            no_value
        } else {
            non_finite
        })?;
        if degree == 1 {
            let [x, y, z] = finite_lanes(
                linear_nurbs_derivative(&basis, control_points, weights, span, scale.get(), second)
                    .ok_or(no_value)?,
            )
            .map_err(|_| non_finite)?;
            return Ok(FiniteVector3::from_components(x, y, z));
        }
        let scaled =
            bspline_basis_scaled_derivatives(knots, degree, span, t, scale).ok_or(non_finite)?;
        first_basis = scaled.0;
        if second {
            second_basis = scaled.1;
        }
        scale
    };
    let poles =
        local_poles(span, degree, |index| control_points.get(index).copied()).ok_or(no_value)?;
    // A sum is absent only where one of its derivative basis terms is not
    // finite.
    let sum = |values: &[f64]| {
        homogeneous_curve_sum(
            values,
            &poles,
            |index| weights.and_then(|weights| weights.get(index).copied()),
            span - degree,
        )
        .ok_or(non_finite)
    };
    let base = sum(&basis)?;
    let first_sum = sum(&first_basis)?;
    let point = finite_lanes(base.project(base, &[]).ok_or(no_value)?).map_err(|_| non_finite)?;
    let first = finite_lanes(
        first_sum
            .project(base, &[(first_sum, point)])
            .ok_or(no_value)?,
    )
    .map_err(|_| non_finite)?;
    let (lanes, divisions) = if second {
        let second_sum = sum(&second_basis)?;
        let lanes = finite_lanes(
            second_sum
                .project(
                    base,
                    &[(second_sum, point), (first_sum, first), (first_sum, first)],
                )
                .ok_or(no_value)?,
        )
        .map_err(|_| non_finite)?;
        (lanes, 2)
    } else {
        (first, 1)
    };
    // Each derivative lane over the span divides by the positive scale once
    // per order; a quotient that overflows leaves the derivative there.
    let unscale = |value: FiniteReal| {
        if scale.get() == 1.0 {
            return Ok(value);
        }
        (0..divisions).try_fold(value, |value, _| {
            difference_quotient(value, FiniteReal::ZERO, scale.into(), FiniteReal::ZERO)
                .map_err(|failure| failure.map(|_| ()))
        })
    };
    let [x, y, z] = lanes;
    Ok(FiniteVector3::from_components(
        unscale(x)?,
        unscale(y)?,
        unscale(z)?,
    ))
}

/// The degree-one rational derivative has a two-pole quotient form. Form its
/// numerator as exact products before dividing by the span and homogeneous
/// weight; neither derivative basis coefficient needs to fit in `f64`.
///
/// Each lane is the derivative's coordinate, or the signed infinity of a
/// coordinate whose quotient overflows.
fn linear_nurbs_derivative(
    basis: &[f64],
    control_points: &[FinitePoint3],
    weights: Option<&[f64]>,
    span: usize,
    width: f64,
    second: bool,
) -> Option<[Result<FiniteReal, f64>; 3]> {
    use crate::math::sum::{product_sum, scaled_finite, ProductSum};
    let start = span.checked_sub(1)?;
    let first = *control_points.get(start)?;
    let last = *control_points.get(span)?;
    let weight0 = weights
        .and_then(|weights| weights.get(start))
        .copied()
        .unwrap_or(1.0);
    let weight1 = weights
        .and_then(|weights| weights.get(span))
        .copied()
        .unwrap_or(1.0);
    let ProductSum::Value(base_weight) =
        product_sum([Some([basis[0], weight0]), Some([basis[1], weight1])].into_iter())
    else {
        return None;
    };
    let width = scaled_finite(width)?;
    let coordinate = |left: f64, right: f64| {
        let mut sum = ExactSignedSum::default();
        if second {
            // -w0*w1*(w1-w0)*(right-left); the factor 2 is an exponent shift.
            sum.add_factors([-weight0, weight1, weight1, right]);
            sum.add_factors([weight0, weight1, weight1, left]);
            sum.add_factors([weight0, weight1, weight0, right]);
            sum.add_factors([-weight0, weight1, weight0, left]);
            sum.finish().map_or(Ok(FiniteReal::ZERO), |numerator| {
                numerator.quotient_by_factors(
                    [base_weight, base_weight, base_weight, width, width],
                    true,
                )
            })
        } else {
            sum.add_factors([weight0, weight1, right]);
            sum.add_factors([-weight0, weight1, left]);
            sum.finish().map_or(Ok(FiniteReal::ZERO), |numerator| {
                numerator.quotient_by_factors([base_weight, base_weight, width], false)
            })
        }
    };
    Some([
        coordinate(first.x, last.x),
        coordinate(first.y, last.y),
        coordinate(first.z, last.z),
    ])
}

/// Evaluate a curve carrier selected by arena id, including supported
/// procedural constructions, or report why it has no finite point there.
pub fn model_curve_point_by_id(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    model_curve_point_by_id_inner(index, curve_id, parameter, None)
}

/// Evaluate a model curve carrier within a caller-owned work slice. Carrier
/// recursion and direct NURBS or polyline work consume the supplied budget;
/// a refused charge leaves no value.
pub fn model_curve_point_by_id_with_budget(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
    budget: &WorkBudget<'_>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let _guard = budget.recursion_guard().ok_or(EvaluationFailure::NoValue)?;
    let point = model_curve_point_by_id_inner(index, curve_id, parameter, Some(budget));
    ModelEvaluationDepthGuard::finish_budgeted(budget, point)
}

/// A model curve's finite point with its tangent and acceleration, each
/// derivative with its own outcome: a derivative that has no value there, or
/// that left the finite range, leaves the point and the other derivative as
/// they are. A reader fails on the derivatives it reads.
#[derive(Clone, Copy)]
struct ModelCurveDifferential {
    point: FinitePoint3,
    tangent: Result<FiniteVector3, EvaluationFailure<()>>,
    acceleration: Result<FiniteVector3, EvaluationFailure<()>>,
}

impl ModelCurveDifferential {
    /// The tangent, or its failure carrying the finite point, for a reader
    /// that fails on it.
    fn tangent(&self) -> Result<FiniteVector3, EvaluationFailure<Point3>> {
        self.tangent
            .map_err(|failure| failure.map(|()| self.point.get()))
    }
}

/// The differential of a curve whose point is `point` and whose derivatives
/// are evaluated separately, each once the point is finite.
fn differential_at(
    point: Result<FinitePoint3, EvaluationFailure<Point3>>,
    tangent: impl FnOnce() -> Result<FiniteVector3, EvaluationFailure<()>>,
    acceleration: impl FnOnce() -> Result<FiniteVector3, EvaluationFailure<()>>,
) -> Result<ModelCurveDifferential, EvaluationFailure<Point3>> {
    Ok(ModelCurveDifferential {
        point: point?,
        tangent: tangent(),
        acceleration: acceleration(),
    })
}

/// Evaluate the native helix path and its exact angle derivatives.
///
/// The stored angular interval is the domain of the path parameter. The
/// pitch and apex terms advance by the fraction of one full revolution from
/// the interval's lower bound, while the major and minor vectors define the
/// radial frame at the stored angle.
///
/// A parameter outside the interval and a zero axis have no value. The point
/// reads no derivative, so a tangent or acceleration outside the finite
/// range is that derivative's own failure.
fn helix_differential(
    definition: &ProceduralCurveDefinition,
    parameter: f64,
) -> Result<ModelCurveDifferential, EvaluationFailure<Point3>> {
    let ProceduralCurveDefinition::Helix(helix_payload) = definition else {
        return Err(EvaluationFailure::NoValue);
    };
    let [start, end] = helix_payload.angle_range().finite_components();
    let center = helix_payload.center().get();
    let major = helix_payload.major().get();
    let minor = helix_payload.minor().get();
    let pitch = helix_payload.pitch().get();
    let apex_factor = helix_payload.apex_factor().get();
    let finite_parameter = FiniteReal::new(parameter).ok_or(EvaluationFailure::NoValue)?;
    if finite_parameter < start || finite_parameter > end {
        return Err(EvaluationFailure::NoValue);
    }
    let inverse_revolution = 1.0 / std::f64::consts::TAU;
    let revolution_fraction = finite_parameter.turns_from(start).get();
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
    Ok(ModelCurveDifferential {
        point: admit_point(point)?,
        tangent: admit_derivative(tangent),
        acceleration: admit_derivative(acceleration),
    })
}

fn model_curve_differential_by_id(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
) -> Result<ModelCurveDifferential, EvaluationFailure<Point3>> {
    model_curve_differential_by_id_inner(index, curve_id, parameter, None)
}

/// The point, tangent and acceleration of a model curve at `parameter`, or
/// why its point has no finite value there. At a finite point each
/// derivative states its own outcome: no value where the curve has no
/// derivative there, non-finite where the derivative left the finite range.
fn model_curve_differential_by_id_inner(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<ModelCurveDifferential, EvaluationFailure<Point3>> {
    let _depth = ModelEvaluationDepthGuard::enter(budget).ok_or(EvaluationFailure::NoValue)?;
    if !parameter.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let curve = index
        .curves(curve_id.as_str())
        .ok_or(EvaluationFailure::NoValue)?;
    if budget.is_some_and(|budget| !budget.charge()) {
        return Err(EvaluationFailure::NoValue);
    }
    if let Some(procedural) = index
        .procedural_curves_for_curve(curve_id.as_str())
        .and_then(|procedurals| procedurals.first().copied())
    {
        match procedural.definition() {
            ProceduralCurveDefinition::Replica { source, transform } => {
                let differential =
                    model_curve_differential_by_id_inner(index, source, parameter, budget)
                        .map_err(|failure| {
                            failure.map(|point| {
                                transform
                                    .apply_point_reaching(point)
                                    .map_or_else(|point| point, FinitePoint3::get)
                            })
                        })?;
                let point = transform
                    .apply_point_reaching(differential.point.get())
                    .map_err(EvaluationFailure::NonFinite)?;
                return Ok(ModelCurveDifferential {
                    point,
                    tangent: placed_derivative(*transform, differential.tangent),
                    acceleration: placed_derivative(*transform, differential.acceleration),
                });
            }
            ProceduralCurveDefinition::Subset(definition_payload) => {
                let source = definition_payload.source();
                let sense = definition_payload.sense();
                let source_parameter = subset_source_parameter(
                    *definition_payload.parameter_range(),
                    *sense,
                    parameter,
                )?;
                let differential =
                    model_curve_differential_by_id_inner(index, source, source_parameter, budget)?;
                return Ok(ModelCurveDifferential {
                    point: differential.point,
                    tangent: differential.tangent.map(|tangent| {
                        if *sense {
                            tangent
                        } else {
                            tangent.negated()
                        }
                    }),
                    acceleration: differential.acceleration,
                });
            }
            ProceduralCurveDefinition::Helix(_) => {
                return helix_differential(procedural.definition(), parameter);
            }
            _ => {}
        }
    }
    if let Some(cache) = curve.geometry.solved_cache() {
        return differential_at(
            budget.map_or_else(
                || curve_point_solved(cache, parameter),
                |budget| curve_point_with_budget_solved(cache, parameter, budget),
            ),
            || curve_derivative_evaluation(cache, parameter, CurveDerivative::First),
            || curve_derivative_evaluation(cache, parameter, CurveDerivative::Second),
        );
    }
    let solved = match &curve.geometry {
        CurveGeometry::Solved(solved) => solved,
        CurveGeometry::Procedural { .. } => return Err(EvaluationFailure::NoValue),
    };
    match budget {
        Some(budget) => differential_at(
            curve_point_with_budget_solved(solved, parameter, budget),
            || {
                curve_derivative_with_budget_evaluation(
                    solved,
                    parameter,
                    CurveDerivative::First,
                    budget,
                )
            },
            || {
                curve_derivative_with_budget_evaluation(
                    solved,
                    parameter,
                    CurveDerivative::Second,
                    budget,
                )
            },
        ),
        None => differential_at(
            curve_point_solved(solved, parameter),
            || curve_derivative_evaluation(solved, parameter, CurveDerivative::First),
            || curve_derivative_evaluation(solved, parameter, CurveDerivative::Second),
        ),
    }
}

/// Map a nonnegative local subset parameter into its ordered source range.
fn subset_source_parameter(
    interval: ParameterInterval,
    sense: bool,
    parameter: f64,
) -> Result<f64, EvaluationFailure<Point3>> {
    let interval =
        IncreasingParameterInterval::new(interval.endpoints()).ok_or(EvaluationFailure::NoValue)?;
    if !parameter.is_finite() || parameter < 0.0 {
        return Err(EvaluationFailure::NoValue);
    }
    if interval
        .scaled_span()
        .finite()
        .is_ok_and(|span| parameter > span.get())
    {
        return Err(EvaluationFailure::NoValue);
    }
    let [start, end] = interval.endpoints();
    Ok(if sense {
        start + parameter
    } else {
        end - parameter
    })
}

/// The admitted revolution axis scaled by the reciprocal of its length. The
/// admission holds the length within `1e-9` of one without rescaling, and the
/// rotation needs unit length to rounding.
fn unit_length_axis(direction: UnitVector3) -> Vector3 {
    let direction = *direction.as_raw();
    scale_vector(direction, 1.0 / direction.norm())
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

/// `point` rotated by `angle` about the unit `axis` through `axis_origin`.
fn revolved_point(point: Point3, axis_origin: Point3, axis: Vector3, angle: f64) -> Point3 {
    offset(
        axis_origin,
        &[(
            1.0,
            rotate_vector_about_axis(point_displacement(point, axis_origin), axis, angle),
        )],
    )
}

/// The point of a directrix revolved about an axis, or why it has none. A
/// directrix point outside the finite range leaves the surface there, at the
/// point its revolution reaches.
fn model_axis_revolution_point(
    index: &crate::index::ModelIndex<'_>,
    directrix: &crate::ids::CurveId,
    axis_origin: Point3,
    axis_direction: UnitVector3,
    angle: f64,
    parameter: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    if !angle.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let axis = unit_length_axis(axis_direction);
    let point = budget
        .map_or_else(
            || model_curve_point_by_id(index, directrix, parameter),
            |budget| model_curve_point_by_id_with_budget(index, directrix, parameter, budget),
        )
        .map_err(|failure| failure.map(|point| revolved_point(point, axis_origin, axis, angle)))?;
    admit_point(revolved_point(point.get(), axis_origin, axis, angle))
}

/// The jet of a directrix revolved about an axis, or why its point has none.
/// A directrix point outside the finite range leaves the surface there, at
/// the point its revolution reaches. The first partials read the directrix
/// tangent; the second read its acceleration as well.
fn model_axis_revolution_jet(
    index: &crate::index::ModelIndex<'_>,
    directrix: &crate::ids::CurveId,
    axis_origin: Point3,
    axis_direction: UnitVector3,
    angle: f64,
    parameter: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    if !angle.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let axis = unit_length_axis(axis_direction);
    let differential = model_curve_differential_by_id_inner(index, directrix, parameter, budget)
        .map_err(|failure| failure.map(|point| revolved_point(point, axis_origin, axis, angle)))?;
    let rotated = rotate_vector_about_axis(
        point_displacement(differential.point.get(), axis_origin),
        axis,
        angle,
    );
    let point = admit_point(offset(axis_origin, &[(1.0, rotated)]))?;
    let du = axis.cross(rotated);
    let rotated_tangent = differential
        .tangent
        .map(|tangent| rotate_vector_about_axis(tangent.get(), axis, angle));
    let rotated_acceleration = differential
        .acceleration
        .map(|acceleration| rotate_vector_about_axis(acceleration.get(), axis, angle));
    Ok(SurfaceJet::formed(
        point,
        rotated_tangent.map(|tangent| [du, tangent]),
        rotated_tangent
            .and_then(|tangent| Ok([axis.cross(du), axis.cross(tangent), rotated_acceleration?])),
    ))
}

/// Map a construction-space directrix parameter to the carrier curve and
/// return the carrier derivative with respect to the construction parameter.
///
/// IGES line entities use a normalized surface interval while the neutral
/// line carrier uses signed distance. Other curve carriers retain their native
/// parameterization. A line used by more than one edge is only unambiguous
/// when every retained edge range agrees, unless the construction stores its
/// neutral carrier interval explicitly.
fn record_u_interval(
    record_bounds: Option<crate::geometry::RecordBounds>,
) -> Option<[FiniteReal; 2]> {
    let [Some(start), Some(end), _, _] = record_bounds?.finite_values() else {
        return None;
    };
    Some([start, end])
}

/// The descent is bounded by [`PlacedCurve`](crate::geometry::PlacedCurve)
/// construction; no arm follows an arena id.
fn is_line_geometry(geometry: &SolvedCurveGeometry) -> bool {
    match geometry {
        SolvedCurveGeometry::Line(_) => true,
        SolvedCurveGeometry::Transformed(placed) => is_line_geometry(placed.basis()),
        _ => false,
    }
}

/// A construction parameter mapped onto its carrier curve.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ConstructionParameter {
    /// The carrier parameter.
    parameter: FiniteReal,
    /// The carrier parameter's derivative with respect to the construction
    /// parameter, or why it has no finite value. A line carrier forms it
    /// apart from the parameter, which does not need it.
    derivative: Result<FiniteReal, EvaluationFailure<()>>,
}

/// The carrier parameter of a construction parameter and its derivative, or
/// why there is no carrier parameter. A parameter that is not finite or lies
/// outside its interval, an interval that is empty or reversed, and a line
/// carrier whose edge ranges disagree have no value. A width or mapped
/// parameter that overflows, and a derivative the mapping reads, leave the
/// finite range; the evaluation reaches no coordinate there.
fn construction_curve_parameter(
    index: &crate::index::ModelIndex<'_>,
    directrix: &crate::ids::CurveId,
    parameter: f64,
    surface_interval: Option<[FiniteReal; 2]>,
    carrier_interval: Option<[FiniteReal; 2]>,
    reversed: bool,
) -> Result<ConstructionParameter, EvaluationFailure<()>> {
    let no_value = EvaluationFailure::NoValue;
    let non_finite = EvaluationFailure::NonFinite(());
    let overflow = |value: f64| FiniteReal::new(value).ok_or(non_finite);
    let parameter = FiniteReal::new(parameter).ok_or(no_value)?;
    // The surface width is admitted positive where it is formed, and only
    // the arms that hold a surface interval form it.
    let positive_width = |start: FiniteReal, end: FiniteReal| {
        let width = overflow(end.get() - start.get())?;
        if width.get() > 0.0 {
            Ok(width)
        } else {
            Err(no_value)
        }
    };
    let (parameter, surface_derivative, surface_width) = match (surface_interval, carrier_interval)
    {
        (Some([surface_start, surface_end]), Some([carrier_start, carrier_end])) => {
            let surface_width = positive_width(surface_start, surface_end)?;
            let carrier_width = carrier_end.get() - carrier_start.get();
            if carrier_width <= 0.0
                || parameter.get() < carrier_start.get()
                || parameter.get() > carrier_end.get()
            {
                return Err(no_value);
            }
            let carrier_width = overflow(carrier_width)?;
            let derivative = overflow(surface_width.get() / carrier_width.get())?;
            let source_parameter = overflow(
                (parameter.get() - carrier_start.get())
                    .mul_add(derivative.get(), surface_start.get()),
            )?;
            (source_parameter, derivative, Some(surface_width))
        }
        (Some([surface_start, surface_end]), None) => {
            let surface_width = positive_width(surface_start, surface_end)?;
            if parameter.get() < surface_start.get() || parameter.get() > surface_end.get() {
                return Err(no_value);
            }
            (parameter, FiniteReal::ONE, Some(surface_width))
        }
        (None, Some([carrier_start, carrier_end])) => {
            if carrier_start.get() >= carrier_end.get()
                || parameter.get() < carrier_start.get()
                || parameter.get() > carrier_end.get()
            {
                return Err(no_value);
            }
            (parameter, FiniteReal::ONE, None)
        }
        (None, None) => (parameter, FiniteReal::ONE, None),
    };
    let curve = index.curves(directrix.as_str()).ok_or(no_value)?;
    let directed = |parameter: FiniteReal, derivative: FiniteReal| {
        Ok(if reversed {
            ConstructionParameter {
                parameter: parameter.negated(),
                derivative: Ok(derivative.negated()),
            }
        } else {
            ConstructionParameter {
                parameter,
                derivative: Ok(derivative),
            }
        })
    };
    let (Some([surface_start, surface_end]), Some(surface_width)) =
        (surface_interval, surface_width)
    else {
        return directed(parameter, surface_derivative);
    };
    if !curve.geometry.solved().is_some_and(is_line_geometry) {
        return directed(parameter, surface_derivative);
    }
    let line_interval = if let Some(carrier_interval) = carrier_interval {
        carrier_interval
    } else {
        let mut ranges = index
            .ir()
            .model
            .edges
            .iter()
            .filter(|edge| edge.curve() == Some(directrix))
            .filter_map(crate::topology::Edge::param_range);
        let interval = ranges.next().ok_or(no_value)?;
        if ranges.any(|range| range != interval) {
            return Err(no_value);
        }
        interval.finite_components()
    };
    let [curve_start, curve_end] = line_interval;
    let curve_width = positive_width(curve_start, curve_end)?;
    // An absent product is one that no finite value holds.
    let derivative = scaled_ratio_products(curve_width, surface_width, [surface_derivative])
        .map(|[derivative]| {
            if reversed {
                derivative.negated()
            } else {
                derivative
            }
        })
        .ok_or(non_finite);
    let fraction = if reversed {
        (surface_end.get() - parameter.get()) / surface_width.get()
    } else {
        (parameter.get() - surface_start.get()) / surface_width.get()
    };
    let parameter = overflow(curve_start.get() + fraction * curve_width.get())?;
    Ok(ConstructionParameter {
        parameter,
        derivative,
    })
}

/// The point of a native extrusion: the directrix point at the carrier
/// parameter, displaced by `v` along the extrusion direction, or why it has
/// none. The point fails on no derivative. A `v` that is not finite has no
/// value; a directrix point outside the finite range leaves the surface
/// there, at the point its displacement reaches.
fn model_native_extrusion_point(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::ExtrusionSurfaceConstruction,
    carrier_interval: Option<[FiniteReal; 2]>,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    if !v.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let directrix = construction.directrix();
    let direction = construction.direction().get();
    let carrier = construction_curve_parameter(
        index,
        directrix,
        u,
        construction
            .parameter_interval()
            .map(FiniteVector::finite_components),
        carrier_interval,
        extrusion_directrix_reversed(construction.revision_form()),
    )
    .map_err(|failure| failure.map(|()| UNREACHED_POINT))?;
    let point =
        model_curve_differential_by_id_inner(index, directrix, carrier.parameter.get(), budget)
            .map_err(|failure| failure.map(|point| offset(point, &[(v, direction)])))?
            .point;
    admit_point(offset(point.get(), &[(v, direction)]))
}

// The native and carrier parameter intervals are independent serialized semantics;
// the revision reversal affects the derivative mapping, and the optional budget
// must remain explicit across recursive evaluation.
fn model_native_extrusion_jet(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::ExtrusionSurfaceConstruction,
    carrier_interval: Option<[FiniteReal; 2]>,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    if !v.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let directrix = construction.directrix();
    let direction = construction.direction().get();
    let carrier = construction_curve_parameter(
        index,
        directrix,
        u,
        construction
            .parameter_interval()
            .map(FiniteVector::finite_components),
        carrier_interval,
        extrusion_directrix_reversed(construction.revision_form()),
    )
    .map_err(|failure| failure.map(|()| UNREACHED_POINT))?;
    // A directrix point that leaves the finite range leaves the surface
    // there, at the point its extrusion reaches.
    let differential =
        model_curve_differential_by_id_inner(index, directrix, carrier.parameter.get(), budget)
            .map_err(|failure| failure.map(|point| offset(point, &[(v, direction)])))?;
    let point = admit_point(offset(differential.point.get(), &[(v, direction)]))?;
    let derivative = carrier.derivative;
    // A product outside the finite range reaches its signed infinity; a
    // component that is not finite reaches no product.
    let squared_derivative_component =
        |derivative: FiniteReal, component| match crate::math::sum::product_sum(std::iter::once(
            Some([derivative.get(), derivative.get(), component]),
        )) {
            crate::math::sum::ProductSum::Zero => 0.0,
            crate::math::sum::ProductSum::Value(value) => value
                .finite()
                .map_or_else(|reached| reached, FiniteReal::get),
            crate::math::sum::ProductSum::Undefined => f64::NAN,
        };
    Ok(SurfaceJet {
        point,
        first: derivative.and_then(|derivative| {
            Ok([
                admit_derivative(scale_vector(differential.tangent?.get(), derivative.get()))?,
                *construction.direction(),
            ])
        }),
        second: derivative.and_then(|derivative| {
            let acceleration = differential.acceleration?.get();
            Ok([
                admit_derivative(Vector3::new(
                    squared_derivative_component(derivative, acceleration.x),
                    squared_derivative_component(derivative, acceleration.y),
                    squared_derivative_component(derivative, acceleration.z),
                ))?,
                FiniteVector3::ZERO,
                FiniteVector3::ZERO,
            ])
        }),
    })
}

fn extrusion_directrix_reversed(
    revision_form: Option<&crate::geometry::RevisionSurfaceForm<Vec<bool>, FiniteReal>>,
) -> bool {
    revision_form
        .and_then(|form| form.flags.first())
        .copied()
        .unwrap_or(false)
}

/// The angle of a native revolution at its angular parameter, and the
/// angle's derivative, or why there is none. An angle that overflows reaches
/// no coordinate.
fn native_revolution_angle(
    construction: &crate::geometry::surface_payloads::RevolutionSurfaceConstruction,
    angular_parameter: FiniteReal,
) -> Result<(f64, f64), EvaluationFailure<Point3>> {
    let unreached = EvaluationFailure::NonFinite(UNREACHED_POINT);
    let (angle, angular_derivative) = match construction.angular_parameter_interval() {
        None => (angular_parameter.get(), 1.0),
        Some(parameter_interval) => {
            let angular_interval = construction.angular_interval();
            let angle = angular_interval
                .map_from(parameter_interval, angular_parameter, false)
                .map_err(|_| unreached)?
                .get();
            let angular_derivative = angular_interval
                .scaled_span()
                .quotient(parameter_interval.scaled_span())
                .map_or_else(|overflow| overflow, FiniteReal::get);
            (angle, angular_derivative)
        }
    };
    Ok((angle, angular_derivative))
}

/// The directrix and angular parameters of a native revolution at `(u, v)`.
/// An angular parameter that is not finite has no value.
fn native_revolution_parameters(
    construction: &crate::geometry::surface_payloads::RevolutionSurfaceConstruction,
    u: f64,
    v: f64,
) -> Result<(f64, FiniteReal), EvaluationFailure<Point3>> {
    let (directrix_parameter, angular_parameter) = if *construction.transposed() {
        (v, u)
    } else {
        (u, v)
    };
    Ok((
        directrix_parameter,
        FiniteReal::new(angular_parameter).ok_or(EvaluationFailure::NoValue)?,
    ))
}

/// The directrix parameter of a native revolution on its carrier curve, or
/// why there is none.
fn native_revolution_carrier(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::RevolutionSurfaceConstruction,
    carrier_interval: Option<[FiniteReal; 2]>,
    directrix_parameter: f64,
) -> Result<ConstructionParameter, EvaluationFailure<Point3>> {
    construction_curve_parameter(
        index,
        construction.directrix(),
        directrix_parameter,
        construction
            .parameter_interval()
            .map(crate::topology::IncreasingParameterInterval::finite_endpoints),
        carrier_interval,
        false,
    )
    .map_err(|failure| failure.map(|()| UNREACHED_POINT))
}

/// The point of a native revolution, or why it has none: the directrix point
/// at the carrier parameter revolved by the mapped angle. The point fails on
/// no derivative.
fn model_native_revolution_point(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::RevolutionSurfaceConstruction,
    carrier_interval: Option<[FiniteReal; 2]>,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let (directrix_parameter, angular_parameter) =
        native_revolution_parameters(construction, u, v)?;
    let carrier =
        native_revolution_carrier(index, construction, carrier_interval, directrix_parameter)?;
    let (angle, _) = native_revolution_angle(construction, angular_parameter)?;
    let axis_origin = construction.axis_origin().get();
    let axis = unit_length_axis(construction.axis_direction());
    let point = model_curve_differential_by_id_inner(
        index,
        construction.directrix(),
        carrier.parameter.get(),
        budget,
    )
    .map_err(|failure| failure.map(|point| revolved_point(point, axis_origin, axis, angle)))?
    .point;
    admit_point(revolved_point(point.get(), axis_origin, axis, angle))
}

/// The jet of a native revolution, or why its point has none. Each partial
/// order reads the carrier parameter's derivative besides the axis
/// revolution's order.
fn model_native_revolution_jet(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::RevolutionSurfaceConstruction,
    carrier_interval: Option<[FiniteReal; 2]>,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    let (directrix_parameter, angular_parameter) =
        native_revolution_parameters(construction, u, v)?;
    let carrier =
        native_revolution_carrier(index, construction, carrier_interval, directrix_parameter)?;
    let (angle, angular_derivative) = native_revolution_angle(construction, angular_parameter)?;
    let jet = model_axis_revolution_jet(
        index,
        construction.directrix(),
        construction.axis_origin().get(),
        construction.axis_direction(),
        angle,
        carrier.parameter.get(),
        budget,
    )?;
    let derivative = carrier.derivative.map(FiniteReal::get);
    let transposed = *construction.transposed();
    let first = derivative.and_then(|derivative| {
        let [du, dv] = FiniteVector3::raw_array(jet.first?);
        Ok(if transposed {
            [
                scale_vector(du, angular_derivative),
                scale_vector(dv, derivative),
            ]
        } else {
            [
                scale_vector(dv, derivative),
                scale_vector(du, angular_derivative),
            ]
        })
    });
    let second = derivative.and_then(|derivative| {
        let [duu, duv, dvv] = FiniteVector3::raw_array(jet.second?);
        Ok(if transposed {
            [
                scale_vector(duu, angular_derivative * angular_derivative),
                scale_vector(duv, derivative * angular_derivative),
                scale_vector(dvv, derivative * derivative),
            ]
        } else {
            [
                scale_vector(dvv, derivative * derivative),
                scale_vector(duv, derivative * angular_derivative),
                scale_vector(duu, angular_derivative * angular_derivative),
            ]
        })
    });
    Ok(SurfaceJet::formed(jet.point, first, second))
}

fn model_curve_point_by_id_inner(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let _depth = ModelEvaluationDepthGuard::enter(budget).ok_or(EvaluationFailure::NoValue)?;
    let curve = index
        .curves(curve_id.as_str())
        .ok_or(EvaluationFailure::NoValue)?;
    if budget.is_some_and(|budget| !budget.charge()) {
        return Err(EvaluationFailure::NoValue);
    }
    let Some(procedural) = index
        .procedural_curves_for_curve(curve_id.as_str())
        .and_then(|procedurals| procedurals.first().copied())
    else {
        return budget.map_or_else(
            || curve_point(&curve.geometry, parameter),
            |budget| curve_point_with_budget(&curve.geometry, parameter, budget),
        );
    };
    match procedural.definition() {
        ProceduralCurveDefinition::Replica { source, transform } => placed_point(
            *transform,
            model_curve_point_by_id_inner(index, source, parameter, budget),
        ),
        ProceduralCurveDefinition::Subset(definition_payload) => {
            let source = definition_payload.source();
            let source_parameter = subset_source_parameter(
                *definition_payload.parameter_range(),
                *definition_payload.sense(),
                parameter,
            )?;
            model_curve_point_by_id_inner(index, source, source_parameter, budget)
        }
        ProceduralCurveDefinition::Helix(_) => {
            helix_differential(procedural.definition(), parameter)
                .map(|differential| differential.point)
        }
        ProceduralCurveDefinition::TolerantIntersection {
            construction: intersection,
            parameterization: Some(parameterization),
            ..
        } => {
            let supports = intersection.supports();
            let tolerance = intersection.tolerance().get();

            let parameter_range = parameterization.parameter_range().endpoints();
            if !parameter.is_finite()
                || parameter < parameter_range[0]
                || parameter > parameter_range[1]
            {
                return Err(EvaluationFailure::NoValue);
            }
            let points = std::array::from_fn(|side| {
                // A non-finite offset-pcurve point is evaluated on its support
                // as a finite one is.
                let uv = match pcurve_uv(&parameterization.pcurves[side], parameter) {
                    Ok(uv) => uv.get(),
                    Err(EvaluationFailure::NonFinite(uv)) => uv,
                    Err(EvaluationFailure::NoValue) => return Err(EvaluationFailure::NoValue),
                };
                budget.map_or_else(
                    || model_surface_point_by_id(index, &supports[side], uv.u, uv.v),
                    |budget| {
                        model_surface_point_by_id_with_budget(
                            index,
                            &supports[side],
                            uv.u,
                            uv.v,
                            budget,
                        )
                    },
                )
            });
            // A side with no value leaves no point. A side outside the finite
            // range leaves the evaluation there, carrying the first side's
            // point as far as it was reached. Two finite points farther apart
            // than the tolerance, including a separation that overflows, have
            // no intersection point.
            let reached = |side: Result<FinitePoint3, EvaluationFailure<Point3>>| match side {
                Ok(point) => Some(point.get()),
                Err(failure) => failure.non_finite(),
            };
            match points {
                [Ok(first), Ok(second)] => {
                    let separation = first.distance(second.get());
                    (separation.is_finite() && separation <= tolerance)
                        .then_some(first)
                        .ok_or(EvaluationFailure::NoValue)
                }
                [first, second] => match (reached(first), reached(second)) {
                    (Some(first), Some(_)) => Err(EvaluationFailure::NonFinite(first)),
                    _ => Err(EvaluationFailure::NoValue),
                },
            }
        }
        _ => {
            if let Some(cache) = curve.geometry.solved_cache() {
                budget.map_or_else(
                    || curve_point_solved(cache, parameter),
                    |budget| curve_point_with_budget_solved(cache, parameter, budget),
                )
            } else if matches!(&curve.geometry, CurveGeometry::Procedural { .. }) {
                Err(EvaluationFailure::NoValue)
            } else if let Some(budget) = budget {
                curve_point_with_budget(&curve.geometry, parameter, budget)
            } else {
                curve_point(&curve.geometry, parameter)
            }
        }
    }
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
) -> Option<FiniteReal> {
    model_curve_parameter_near_point_with_tolerance(
        index,
        curve_id,
        point,
        seed,
        NonNegativeLength::from(index.ir().tolerances.linear),
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
) -> Option<FiniteReal> {
    model_curve_parameter_near_point_with_tolerance(
        index,
        curve_id,
        point,
        seed,
        NonNegativeLength::new(tolerance)?,
    )
}

/// Invert a model curve with an admitted tolerance.
fn model_curve_parameter_near_point_with_tolerance(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    point: Point3,
    seed: f64,
    tolerance: NonNegativeLength,
) -> Option<FiniteReal> {
    let _depth = ModelEvaluationDepthGuard::enter(None)?;
    let curve = index.curves(curve_id.as_str())?;
    if let Some(procedural) = index
        .procedural_curves_for_curve(curve_id.as_str())
        .and_then(|procedurals| procedurals.first().copied())
    {
        match procedural.definition() {
            ProceduralCurveDefinition::Replica { source, transform } => {
                let (basis_point, tolerance_scale) = inverse_affine_point(*transform, point)?;
                // The scale is a finite norm, so the admission refuses only an
                // overflowed product.
                let basis_tolerance = NonNegativeLength::new(tolerance.get() * tolerance_scale)?;
                return model_curve_parameter_near_point_with_tolerance(
                    index,
                    source,
                    basis_point,
                    seed,
                    basis_tolerance,
                );
            }
            ProceduralCurveDefinition::Subset(definition_payload) => {
                let source = definition_payload.source();
                let [start, end] = definition_payload.parameter_range().endpoints();
                let sense = definition_payload.sense();
                {
                    let interval = IncreasingParameterInterval::new([start, end])?;
                    let span = interval.scaled_span().finite();
                    if !seed.is_finite() || seed < 0.0 || span.is_ok_and(|span| seed > span.get()) {
                        return None;
                    }
                    let source_seed = if *sense { start + seed } else { end - seed };
                    let source_parameter = model_curve_parameter_near_point_with_tolerance(
                        index,
                        source,
                        point,
                        source_seed,
                        tolerance,
                    )?
                    .get();
                    let parameter = FiniteReal::new(if *sense {
                        source_parameter - start
                    } else {
                        end - source_parameter
                    })?;
                    return (parameter.get() >= 0.0
                        && span.map_or(true, |span| parameter <= span)
                        && model_curve_point_by_id(index, curve_id, parameter.get())
                            .is_ok_and(|evaluated| evaluated.distance(point) <= tolerance.get()))
                    .then_some(parameter);
                }
            }
            ProceduralCurveDefinition::Helix(_) => {
                return helix_parameter_near_point(
                    index,
                    curve_id,
                    point,
                    seed,
                    tolerance,
                    procedural.definition(),
                );
            }
            _ => {}
        }
    }
    if let Some(cache) = curve.geometry.solved_cache() {
        return direct_curve_parameter_near_point(cache, point, FiniteReal::new(seed)?, tolerance);
    }
    if !matches!(&curve.geometry, CurveGeometry::Procedural { .. }) {
        return curve.geometry.solved().and_then(|geometry| {
            direct_curve_parameter_near_point(geometry, point, FiniteReal::new(seed)?, tolerance)
        });
    }
    let construction = curve.geometry.procedural_construction()?;
    let procedural = index.procedural_curves(construction.as_str())?;
    let crate::geometry::ProceduralCurveDefinition::TolerantIntersection {
        construction: intersection,
        parameterization: Some(parameterization),
        ..
    } = procedural.definition()
    else {
        return None;
    };
    let supports = intersection.supports();
    // The intersection tolerance bounds model-space distances, so it is a
    // length.
    let admitted_tolerance = NonNegativeLength::from_assigned_real(intersection.tolerance());
    let tolerance = admitted_tolerance.get();

    let range = parameterization.parameter_range().endpoints();
    let finite_range = parameterization.parameter_range().finite_endpoints();
    if !seed.is_finite() || seed < range[0] || seed > range[1] {
        return None;
    }
    let mut candidates = Vec::new();
    for (support_id, pcurve) in supports.iter().zip(&parameterization.pcurves) {
        let Some(surface) = index.surfaces(support_id.as_str()) else {
            continue;
        };
        let PcurveGeometry::Line(line_pcurve) = pcurve else {
            continue;
        };
        let origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
        let parameter = match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => {
                let Some(base) =
                    model_surface_point_by_id(index, support_id, origin.u, origin.v).ok()
                else {
                    continue;
                };
                let Some(next) = model_surface_point_by_id(
                    index,
                    support_id,
                    origin.u + direction.u,
                    origin.v + direction.v,
                )
                .ok() else {
                    continue;
                };
                let tangent = Vector3::new(next.x - base.x, next.y - base.y, next.z - base.z);
                let offset = Vector3::new(point.x - base.x, point.y - base.y, point.z - base.z);
                let denominator = tangent.dot(tangent);
                (denominator.is_finite() && denominator > 0.0)
                    .then(|| offset.dot(tangent) / denominator)
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => {
                analytic_surface_parameters(&surface.geometry, point)
                    .map(Point2::from)
                    .and_then(|mut uv| {
                        if direction.v == 0.0 && direction.u != 0.0 {
                            let expected = origin.u + direction.u * seed;
                            uv.u += ((expected - uv.u) / std::f64::consts::TAU).round()
                                * std::f64::consts::TAU;
                            Some((uv.u - origin.u) / direction.u)
                        } else if direction.u == 0.0
                            && direction.v != 0.0
                            && matches!(
                                surface.geometry.solved(),
                                Some(SolvedSurfaceGeometry::Torus(_))
                            )
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
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => {
                analytic_surface_parameters(&surface.geometry, point)
                    .map(Point2::from)
                    .and_then(|mut uv| {
                        if direction.v == 0.0 && direction.u != 0.0 {
                            let expected = origin.u + direction.u * seed;
                            uv.u += ((expected - uv.u) / std::f64::consts::TAU).round()
                                * std::f64::consts::TAU;
                            Some((uv.u - origin.u) / direction.u)
                        } else if direction.u == 0.0
                            && direction.v != 0.0
                            && matches!(
                                surface.geometry.solved(),
                                Some(SolvedSurfaceGeometry::Torus(_))
                            )
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
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)) => {
                analytic_surface_parameters(&surface.geometry, point)
                    .map(Point2::from)
                    .and_then(|mut uv| {
                        if direction.v == 0.0 && direction.u != 0.0 {
                            let expected = origin.u + direction.u * seed;
                            uv.u += ((expected - uv.u) / std::f64::consts::TAU).round()
                                * std::f64::consts::TAU;
                            Some((uv.u - origin.u) / direction.u)
                        } else if direction.u == 0.0
                            && direction.v != 0.0
                            && matches!(
                                surface.geometry.solved(),
                                Some(SolvedSurfaceGeometry::Torus(_))
                            )
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
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_)) => {
                analytic_surface_parameters(&surface.geometry, point)
                    .map(Point2::from)
                    .and_then(|mut uv| {
                        if direction.v == 0.0 && direction.u != 0.0 {
                            let expected = origin.u + direction.u * seed;
                            uv.u += ((expected - uv.u) / std::f64::consts::TAU).round()
                                * std::f64::consts::TAU;
                            Some((uv.u - origin.u) / direction.u)
                        } else if direction.u == 0.0
                            && direction.v != 0.0
                            && matches!(
                                surface.geometry.solved(),
                                Some(SolvedSurfaceGeometry::Torus(_))
                            )
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
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) => {
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
                FiniteReal::new(varying_origin + varying_scale * seed)
                    .and_then(|isocurve_seed| {
                        nurbs_curve_parameter_near_point_with_nonnegative_tolerance(
                            &isocurve,
                            point,
                            admitted_tolerance,
                            isocurve_seed,
                        )
                    })
                    .map(|parameter| (parameter.get() - varying_origin) / varying_scale)
            }
            _ => continue,
        };
        // A parameter that is not finite lies outside the finite range, so
        // it has no candidate.
        let Some(mut parameter) = parameter.and_then(FiniteReal::new) else {
            continue;
        };
        let endpoint_tolerance = EPS_EVAL_MODEL_CURVE_PARAMETER_NEAR_POINT_WITH_TOLERANCE_E12
            * (1.0 + range[0].abs().max(range[1].abs()));
        let value = parameter.get();
        if value < range[0] && range[0] - value <= endpoint_tolerance {
            parameter = finite_range[0];
        } else if value > range[1] && value - range[1] <= endpoint_tolerance {
            parameter = finite_range[1];
        } else if value < range[0] || value > range[1] {
            continue;
        }
        let Ok(evaluated) = model_curve_point_by_id(index, curve_id, parameter.get()) else {
            continue;
        };
        let distance = evaluated.distance(point);
        if distance.is_finite() && distance <= tolerance {
            candidates.push(parameter);
        }
    }
    candidates.into_iter().min_by(|first, second| {
        (first.get() - seed)
            .abs()
            .total_cmp(&(second.get() - seed).abs())
    })
}

/// Find a helix parameter near a caller-selected seed by bounded Newton
/// refinement of the squared model-space distance.
fn helix_parameter_near_point(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    target: Point3,
    seed: f64,
    tolerance: NonNegativeLength,
    definition: &ProceduralCurveDefinition,
) -> Option<FiniteReal> {
    let ProceduralCurveDefinition::Helix(helix_payload) = definition else {
        return None;
    };
    let [start, end] = helix_payload.angle_range().finite_components();
    let tolerance = tolerance.get();

    // A reversed angle range holds no seed.
    let angles = ParameterInterval::ordered(start, end)?;
    let seed = FiniteReal::new(seed)?;
    if seed < start || seed > end || !target.is_finite() {
        return None;
    }

    let mut parameter = seed;
    for _ in 0..MODEL_CURVE_PARAMETER_SEARCH_MAX_NEWTON_ITERATIONS {
        let differential = model_curve_differential_by_id(index, curve_id, parameter.get()).ok()?;
        let residual = Vector3::new(
            differential.point.x - target.x,
            differential.point.y - target.y,
            differential.point.z - target.z,
        );
        let distance = residual.norm();
        if distance.is_finite() && distance <= tolerance {
            return Some(parameter);
        }
        let Ok(tangent) = differential.tangent else {
            return None;
        };
        let tangent = tangent.get();
        let denominator = tangent.dot(tangent);
        if !denominator.is_finite() || denominator <= 0.0 {
            break;
        }
        let Some(next) = FiniteReal::new(parameter.get() - residual.dot(tangent) / denominator)
        else {
            break;
        };
        let next = angles.project(ExtendedReal::from_finite(next));
        if next == parameter {
            break;
        }
        parameter = next;
    }

    let differential = model_curve_differential_by_id(index, curve_id, parameter.get()).ok()?;
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
) -> Option<FiniteReal> {
    direct_curve_parameter_near_point(
        geometry.solved()?,
        point,
        FiniteReal::new(seed)?,
        NonNegativeLength::new(tolerance)?,
    )
}

fn direct_curve_parameter_near_point(
    geometry: &SolvedCurveGeometry,
    point: Point3,
    seed: FiniteReal,
    admitted_tolerance: NonNegativeLength,
) -> Option<FiniteReal> {
    let tolerance = admitted_tolerance.get();
    let components = |origin: Point3, axis: Vector3, reference: Vector3| -> (f64, f64, f64) {
        let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
        let transverse = axis.cross(reference);
        (delta.dot(reference), delta.dot(transverse), delta.dot(axis))
    };
    let parameter = match geometry {
        SolvedCurveGeometry::Line(line_curve) => {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
            FiniteReal::new(delta.dot(direction) / direction.dot(direction))?
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius().get();
            let (x, y, _) = components(center, *axis, *ref_direction);
            let canonical = (y / radius).atan2(x / radius);
            FiniteReal::new(
                canonical
                    + ((seed.get() - canonical) / std::f64::consts::TAU).round()
                        * std::f64::consts::TAU,
            )?
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            let (x, y, _) = components(center, *axis, *major_direction);
            let canonical = (y / minor_radius).atan2(x / major_radius);
            FiniteReal::new(
                canonical
                    + ((seed.get() - canonical) / std::f64::consts::TAU).round()
                        * std::f64::consts::TAU,
            )?
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let vertex = parabola_curve.vertex().get();
            let axis = parabola_curve.frame().axis().as_raw();
            let major_direction = parabola_curve.frame().reference().as_raw();
            let focal_distance = parabola_curve.focal_distance().magnitude();
            let (_, transverse, _) = components(vertex, *axis, *major_direction);
            crate::math::multiply_divide(
                FiniteReal::new(transverse)?,
                FiniteReal::HALF,
                focal_distance,
            )?
        }
        SolvedCurveGeometry::Hyperbola(hyperbola_curve) => {
            let center = hyperbola_curve.center().get();
            let axis = hyperbola_curve.frame().axis().as_raw();
            let major_direction = hyperbola_curve.frame().reference().as_raw();
            let minor_radius = hyperbola_curve.minor_radius().get();
            let (_, transverse, _) = components(center, *axis, *major_direction);
            FiniteReal::new((transverse / minor_radius).asinh())?
        }
        SolvedCurveGeometry::Nurbs(curve) => {
            nurbs_curve_parameter_near_point_with_nonnegative_tolerance(
                curve,
                point,
                admitted_tolerance,
                seed,
            )?
        }
        SolvedCurveGeometry::Polyline(polyline) => {
            let (points, parameters) = polyline_samples(polyline);
            polyline_parameter_near_point(&points, &parameters, point, tolerance, seed)?
        }
        SolvedCurveGeometry::Transformed(placed) => {
            let (basis_point, tolerance_scale) = inverse_affine_point(*placed.transform(), point)?;
            // The scale is a finite norm, so the admission refuses only an
            // overflowed product.
            let basis_tolerance = NonNegativeLength::new(tolerance * tolerance_scale)?;
            direct_curve_parameter_near_point(placed.basis(), basis_point, seed, basis_tolerance)?
        }
        SolvedCurveGeometry::Degenerate(degenerate_curve) => {
            let stored = degenerate_curve.point().get();
            let error = (stored.x - point.x)
                .hypot(stored.y - point.y)
                .hypot(stored.z - point.z);
            (error.is_finite() && error <= tolerance).then_some(seed)?
        }
        SolvedCurveGeometry::Composite { .. } | SolvedCurveGeometry::Unknown { .. } => return None,
    };
    let evaluated = curve_point_solved(geometry, parameter.get()).ok()?;
    let error = evaluated.distance(point);
    (error.is_finite() && error <= tolerance).then_some(parameter)
}

fn inverse_affine_point(transform: Transform, point: Point3) -> Option<(Point3, f64)> {
    let inverse = transform.try_inverse_affine().ok()?;
    let coordinates = inverse.apply_point(point)?;
    let tolerance_scale = inverse
        .affine_rows()
        .iter()
        .flat_map(|row| &row[..3])
        .fold(0.0_f64, |length, &value| length.hypot(value));
    tolerance_scale
        .is_finite()
        .then_some((coordinates.get(), tolerance_scale))
}

fn polyline_parameter_near_point(
    points: &[FinitePoint3],
    parameters: &[FiniteReal],
    point: Point3,
    tolerance: f64,
    seed: FiniteReal,
) -> Option<FiniteReal> {
    if points.len() < 2 {
        return None;
    }
    let mut candidates = Vec::new();
    for (segment, parameter_range) in parameters.windows(2).enumerate() {
        let [parameter_start, parameter_end] = [parameter_range[0].get(), parameter_range[1].get()];
        let parameter_width = parameter_end - parameter_start;
        if parameter_width == 0.0 {
            continue;
        }
        let start = points[segment].get();
        let end = points[segment + 1].get();
        let direction = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
        let offset = Vector3::new(point.x - start.x, point.y - start.y, point.z - start.z);
        let length = direction.x.hypot(direction.y).hypot(direction.z);
        if !length.is_finite() {
            continue;
        }
        // A width or length that overflows leaves a NaN fraction, which has no
        // candidate.
        let fraction = if length == 0.0 {
            if offset.x.hypot(offset.y).hypot(offset.z) > tolerance {
                continue;
            }
            ExtendedReal::new((seed.get() - parameter_start) / parameter_width)
        } else {
            let unit = Vector3::new(
                direction.x / length,
                direction.y / length,
                direction.z / length,
            );
            ExtendedReal::new(offset.dot(unit) / length)
        };
        let Some(fraction) = fraction else {
            continue;
        };
        let fraction = ParameterInterval::UNIT.project(fraction).get();
        let candidate = parameter_start + fraction * parameter_width;
        let mapped = Point3::new(
            start.x + fraction * direction.x,
            start.y + fraction * direction.y,
            start.z + fraction * direction.z,
        );
        let error = (mapped.x - point.x)
            .hypot(mapped.y - point.y)
            .hypot(mapped.z - point.z);
        if let Some(candidate) = FiniteReal::new(candidate) {
            if error.is_finite() && error <= tolerance {
                candidates.push(candidate);
            }
        }
    }
    candidates.into_iter().min_by(|first, second| {
        (first.get() - seed.get())
            .abs()
            .total_cmp(&(second.get() - seed.get()).abs())
    })
}

/// Evaluate a 3D curve carrier at parameter `t` on its own parameterization,
/// or report why it has no finite point there.
///
/// A parameter that is not finite has no value on every carrier that reads
/// it; a degenerate carrier is its point at every parameter. A point outside
/// the finite range is non-finite and carries the point the arm reached; a
/// coefficient outside the finite range reaches no coordinate, and each reads
/// NaN.
///
/// The descent is bounded by [`PlacedCurve`](crate::geometry::PlacedCurve)
/// construction; no arm follows an arena id.
pub fn curve_point_solved(
    geometry: &SolvedCurveGeometry,
    t: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let parameter = || FiniteReal::new(t).ok_or(EvaluationFailure::NoValue);
    match geometry {
        SolvedCurveGeometry::Line(line_curve) => {
            let t = parameter()?.get();
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            admit_point(offset(origin, &[(t, direction)]))
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let t = parameter()?.get();
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius().get();
            admit_point(offset(
                center,
                &[
                    (radius * t.cos(), *ref_direction),
                    (radius * t.sin(), axis.cross(*ref_direction)),
                ],
            ))
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let t = parameter()?.get();
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            admit_point(offset(
                center,
                &[
                    (major_radius * t.cos(), *major_direction),
                    (minor_radius * t.sin(), axis.cross(*major_direction)),
                ],
            ))
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let t = parameter()?.get();
            let vertex = parabola_curve.vertex().get();
            let axis = parabola_curve.frame().axis().as_raw();
            let major_direction = parabola_curve.frame().reference().as_raw();
            let focal_distance = parabola_curve.focal_distance().get();
            // The products of finite factors are absent only where they
            // overflow.
            let (Some(axial), Some(transverse)) = (
                product_quotient([focal_distance, t, t], []),
                product_quotient([2.0, focal_distance, t], []),
            ) else {
                return Err(EvaluationFailure::NonFinite(UNREACHED_POINT));
            };
            admit_point(offset(
                vertex,
                &[
                    (axial.get(), *major_direction),
                    (transverse.get(), axis.cross(*major_direction)),
                ],
            ))
        }
        SolvedCurveGeometry::Hyperbola(hyperbola_curve) => {
            let parameter = parameter()?;
            let center = hyperbola_curve.center().get();
            let axis = hyperbola_curve.frame().axis().as_raw();
            let major_direction = hyperbola_curve.frame().reference().as_raw();
            let major_radius = hyperbola_curve.major_radius().magnitude();
            let minor_radius = Length::from(hyperbola_curve.minor_radius()).magnitude();
            // The point reads the major cosine and the minor sine products
            // only; one outside the finite range carries its plain value.
            let [_, major_cosh] = sinh_cosh_lanes(scaled_sinh_cosh(major_radius, parameter));
            let [minor_sinh, _] = sinh_cosh_lanes(scaled_sinh_cosh(minor_radius, parameter));
            match (major_cosh, minor_sinh) {
                (Ok(major_cosh), Ok(minor_sinh)) => admit_point(offset(
                    center,
                    &[
                        (major_cosh.get(), *major_direction),
                        (minor_sinh.get(), axis.cross(*major_direction)),
                    ],
                )),
                (major_cosh, minor_sinh) => {
                    let plain = |lane: Result<FiniteReal, f64>| {
                        lane.map_or_else(|plain| plain, FiniteReal::get)
                    };
                    Err(EvaluationFailure::NonFinite(offset(
                        center,
                        &[
                            (plain(major_cosh), *major_direction),
                            (plain(minor_sinh), axis.cross(*major_direction)),
                        ],
                    )))
                }
            }
        }
        SolvedCurveGeometry::Degenerate(degenerate_curve) => Ok(degenerate_curve.point()),
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let parameter =
                map_nurbs_curve_parameter(nurbs, parameter()?).ok_or(EvaluationFailure::NoValue)?;
            let poles = nurbs.pole_rows();
            nurbs_curve_point_evaluation(
                nurbs.degree(),
                nurbs.knots(),
                poles.count(),
                |index| poles.point_at(index),
                |index| poles.weight_at(index),
                parameter,
            )
        }
        SolvedCurveGeometry::Polyline(polyline) => polyline_point(
            polyline.point_count(),
            |index| polyline.point_at(index),
            |index| polyline.parameter_at(index),
            t,
        ),
        SolvedCurveGeometry::Transformed(placed) => {
            placed_point(*placed.transform(), curve_point_solved(placed.basis(), t))
        }
        SolvedCurveGeometry::Composite { .. } | SolvedCurveGeometry::Unknown { .. } => {
            Err(EvaluationFailure::NoValue)
        }
    }
}

/// Evaluate a surface carrier at `(u, v)` on its own parameterization: `u` is
/// the azimuth angle and `v` the axial distance / polar angle on analytic
/// quadrics, and both are knot-domain parameters on NURBS surfaces.
///
/// The evaluation fails only on the point: every carrier evaluates its point
/// alone, so partials outside the finite range at a finite point leave the
/// point finite.
pub fn surface_point_solved(
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    match geometry {
        SolvedSurfaceGeometry::Nurbs(nurbs) => nurbs_surface_point(nurbs, u, v),
        SolvedSurfaceGeometry::Transformed(placed) => placed_point(
            *placed.transform(),
            surface_point_solved(placed.basis(), u, v),
        ),
        _ => analytic_surface_second_partials(geometry, u, v)
            .map_or(Err(EvaluationFailure::NoValue), |partials| {
                admit_point(partials.point)
            }),
    }
}

/// A placed carrier's point from its basis point. A basis point outside the
/// finite range carries the point the placement reaches from it, finite or
/// not.
fn placed_point(
    transform: Transform,
    basis: Result<FinitePoint3, EvaluationFailure<Point3>>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    match basis {
        Ok(point) => transform
            .apply_point_reaching(point.get())
            .map_err(EvaluationFailure::NonFinite),
        Err(EvaluationFailure::NonFinite(point)) => Err(EvaluationFailure::NonFinite(
            transform
                .apply_point_reaching(point)
                .map_or_else(|point| point, FinitePoint3::get),
        )),
        Err(EvaluationFailure::NoValue) => Err(EvaluationFailure::NoValue),
    }
}

/// Evaluate a directly stored surface at `(u, v)` within a caller-owned work
/// slice. Analytic surfaces are constant-cost; transformed carriers charge
/// each transform layer and NURBS carriers charge their local basis work. A
/// refused charge leaves no value; otherwise the failure is
/// [`surface_point_solved`]'s.
///
/// The descent is bounded by [`PlacedSurface`](crate::geometry::PlacedSurface)
/// construction; no arm follows an arena id.
pub fn surface_point_with_budget_solved(
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
    budget: &WorkBudget<'_>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    match geometry {
        SolvedSurfaceGeometry::Nurbs(nurbs) => nurbs_surface_point_with_budget(nurbs, u, v, budget),
        SolvedSurfaceGeometry::Transformed(placed) => {
            if !budget.charge() {
                return Err(EvaluationFailure::NoValue);
            }
            placed_point(
                *placed.transform(),
                surface_point_with_budget_solved(placed.basis(), u, v, budget),
            )
        }
        _ => surface_point_solved(geometry, u, v),
    }
}

const ROLLING_BALL_JET_RADIUS_TOLERANCE: f64 = 1e-8;

/// Evaluate a quintic rolling-ball jet at spine parameter `t` and arc
/// fraction `s`.
///
/// The jet stores value, first-derivative, and second-derivative rows for the
/// two limiting points, centre, and opening angle. Each scalar channel is
/// interpolated with the unique quintic Hermite polynomial on its knot span.
/// The interpolated limiting points then define the circular section whose
/// fixed radius is taken from the first stored station.
///
/// A definition that is not a quintic jet with triple interior knots, a
/// parameter that is not finite or lies outside the jet, stations whose
/// radii disagree, and a section whose first limiting direction vanishes or
/// whose second is parallel to it have no point. A knot span, interpolated
/// channel or section direction that overflows reaches no coordinate; a
/// point outside the finite range carries the point reached.
pub fn rolling_ball_jet_point(
    definition: &ProceduralSurfaceDefinition,
    t: f64,
    s: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let no_value = EvaluationFailure::NoValue;
    let unreached = EvaluationFailure::NonFinite(UNREACHED_POINT);
    let ProceduralSurfaceDefinition::RollingBallJet(jet) = definition else {
        return Err(no_value);
    };
    let degree = jet.degree();
    let stations = jet.stations();
    if degree != 5
        || stations
            .iter()
            .skip(1)
            .take(stations.len() - 2)
            .any(|station| station.multiplicity != 3)
        || !t.is_finite()
        || !s.is_finite()
        || !(0.0..=1.0).contains(&s)
    {
        return Err(no_value);
    }
    let radius = stations[0]
        .site
        .first_limit
        .distance(stations[0].site.center.get());
    if stations.iter().any(|station| {
        let site = &station.site;
        let first_radius = site.first_limit.distance(site.center.get());
        let second_radius = site.second_limit.distance(site.center.get());
        second_radius <= 0.0
            || (first_radius - radius).abs()
                > ROLLING_BALL_JET_RADIUS_TOLERANCE * first_radius.abs().max(radius.abs()).max(1.0)
    }) {
        return Err(no_value);
    }
    let span = stations
        .windows(2)
        .position(|pair| t >= pair[0].knot.get() && t <= pair[1].knot.get())
        .ok_or(no_value)?;
    let interval =
        IncreasingParameterInterval::between(stations[span].knot, stations[span + 1].knot)
            .ok_or(no_value)?;
    let (span_factor, doubled) = interval.span_factors();
    let span_factors = [span_factor, if doubled { 2.0 } else { 1.0 }];
    let fraction = match FiniteReal::new(t)
        .ok_or(no_value)?
        .segment_position(stations[span].knot, stations[span + 1].knot)
    {
        SegmentPosition::Within(fraction) => fraction.get(),
        SegmentPosition::Outside | SegmentPosition::Degenerate => return Err(no_value),
    };
    let first = &stations[span].site;
    let second = &stations[span + 1].site;
    let first_limit = rolling_ball_jet_interpolate_point(
        [first.first_limit.get(), second.first_limit.get()],
        [
            first.first_derivative.first_limit.get(),
            second.first_derivative.first_limit.get(),
        ],
        [
            first.second_derivative.first_limit.get(),
            second.second_derivative.first_limit.get(),
        ],
        fraction,
        span_factors,
    );
    let second_limit = rolling_ball_jet_interpolate_point(
        [first.second_limit.get(), second.second_limit.get()],
        [
            first.first_derivative.second_limit.get(),
            second.first_derivative.second_limit.get(),
        ],
        [
            first.second_derivative.second_limit.get(),
            second.second_derivative.second_limit.get(),
        ],
        fraction,
        span_factors,
    );
    let center = rolling_ball_jet_interpolate_point(
        [first.center.get(), second.center.get()],
        [
            first.first_derivative.center.get(),
            second.first_derivative.center.get(),
        ],
        [
            first.second_derivative.center.get(),
            second.second_derivative.center.get(),
        ],
        fraction,
        span_factors,
    );
    let angle = rolling_ball_jet_interpolate_scalar(
        [first.angle.get(), second.angle.get()],
        [
            first.first_derivative.angle.get(),
            second.first_derivative.angle.get(),
        ],
        [
            first.second_derivative.angle.get(),
            second.second_derivative.angle.get(),
        ],
        fraction,
        span_factors,
    );
    // An interpolated channel reads NaN only where its value overflowed.
    if !angle.is_finite() {
        return Err(unreached);
    }
    let first_radius = first_limit.vector_from(center);
    let first_direction = first_radius.scale(1.0 / radius);
    let first_direction_squared = first_direction.dot(first_direction);
    if !first_direction_squared.is_finite() {
        return Err(unreached);
    }
    if first_direction_squared <= f64::EPSILON {
        return Err(no_value);
    }
    let second_radius = second_limit.vector_from(center);
    let second_direction = FiniteVector3::new(
        second_radius
            - first_direction.scale(second_radius.dot(first_direction) / first_direction_squared),
    )
    .ok_or(unreached)?
    .unit_nonzero()
    .ok_or(no_value)?;
    let radial =
        first_direction.scale((s * angle).cos()) + second_direction.scale((s * angle).sin());
    admit_point(center.translated(radial, radius))
}

fn rolling_ball_jet_interpolate_point(
    values: [Point3; 2],
    first_derivatives: [Vector3; 2],
    second_derivatives: [Vector3; 2],
    fraction: f64,
    span_factors: [f64; 2],
) -> Point3 {
    Point3::new(
        rolling_ball_jet_interpolate_scalar(
            [values[0].x, values[1].x],
            [first_derivatives[0].x, first_derivatives[1].x],
            [second_derivatives[0].x, second_derivatives[1].x],
            fraction,
            span_factors,
        ),
        rolling_ball_jet_interpolate_scalar(
            [values[0].y, values[1].y],
            [first_derivatives[0].y, first_derivatives[1].y],
            [second_derivatives[0].y, second_derivatives[1].y],
            fraction,
            span_factors,
        ),
        rolling_ball_jet_interpolate_scalar(
            [values[0].z, values[1].z],
            [first_derivatives[0].z, first_derivatives[1].z],
            [second_derivatives[0].z, second_derivatives[1].z],
            fraction,
            span_factors,
        ),
    )
}

fn rolling_ball_jet_interpolate_scalar(
    values: [f64; 2],
    first_derivatives: [f64; 2],
    second_derivatives: [f64; 2],
    fraction: f64,
    [span_factor, multiplier]: [f64; 2],
) -> f64 {
    let s2 = fraction * fraction;
    let s3 = s2 * fraction;
    let s4 = s3 * fraction;
    let s5 = s4 * fraction;
    let h00 = 1.0 - 10.0 * s3 + 15.0 * s4 - 6.0 * s5;
    let h10 = fraction - 6.0 * s3 + 8.0 * s4 - 3.0 * s5;
    let h20 = 0.5 * s2 - 1.5 * s3 + 1.5 * s4 - 0.5 * s5;
    let h01 = 10.0 * s3 - 15.0 * s4 + 6.0 * s5;
    let h11 = -4.0 * s3 + 7.0 * s4 - 3.0 * s5;
    let h21 = 0.5 * s3 - s4 + 0.5 * s5;
    match crate::math::sum::product_sum(
        [
            Some([values[0], h00, 1.0, 1.0]),
            Some([first_derivatives[0], span_factor, h10, multiplier]),
            Some([
                second_derivatives[0],
                span_factor,
                span_factor,
                h20 * multiplier * multiplier,
            ]),
            Some([values[1], h01, 1.0, 1.0]),
            Some([first_derivatives[1], span_factor, h11, multiplier]),
            Some([
                second_derivatives[1],
                span_factor,
                span_factor,
                h21 * multiplier * multiplier,
            ]),
        ]
        .into_iter(),
    ) {
        crate::math::sum::ProductSum::Zero => 0.0,
        crate::math::sum::ProductSum::Value(value) => {
            value.finite().map_or(f64::NAN, FiniteReal::get)
        }
        crate::math::sum::ProductSum::Undefined => f64::NAN,
    }
}

/// Evaluate a directly stored surface and its exact first partial
/// derivatives, or report why they have no finite value.
///
/// The point fails as [`surface_point_solved`] states. At a finite point,
/// first partials outside the finite range leave the evaluation there,
/// carrying the point.
pub fn surface_partials_solved(
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<SurfacePartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    surface_first_order_solved(geometry, u, v, None)?.partials()
}

/// The raw point and exact first and second partials of an analytic surface
/// at `(u, v)`, or none for a carrier that is not analytic. The lanes are
/// formed from finite frames and can leave the finite range.
fn analytic_surface_second_partials(
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
) -> Option<SurfaceSecondPartials> {
    let zero = Vector3::new(0.0, 0.0, 0.0);
    match geometry {
        SolvedSurfaceGeometry::Plane(plane_surface) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            let v_axis = normal.cross(*u_axis);
            Some(SurfaceSecondPartials {
                point: offset(origin, &[(u, *u_axis), (v, v_axis)]),
                du: *u_axis,
                dv: v_axis,
                duu: zero,
                duv: zero,
                dvv: zero,
            })
        }
        SolvedSurfaceGeometry::Cylinder(cylinder_surface) => {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let ref_direction = cylinder_surface.frame().reference().as_raw();
            let radius = cylinder_surface.radius().get();
            let transverse = axis.cross(*ref_direction);
            let cosine = u.cos();
            let sine = u.sin();
            Some(SurfaceSecondPartials {
                point: offset(
                    origin,
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
        SolvedSurfaceGeometry::Cone(cone_surface) => {
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let ref_direction = cone_surface.frame().reference().as_raw();
            let radius = cone_surface.radius().get();
            let ratio = cone_surface.ratio().get();
            let half_angle = cone_surface.half_angle().get();
            let transverse = axis.cross(*ref_direction);
            let cosine = u.cos();
            let sine = u.sin();
            let radial_slope = half_angle.tan();
            let local_radius = radius + v * radial_slope;
            Some(SurfaceSecondPartials {
                point: offset(
                    origin,
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
        SolvedSurfaceGeometry::Sphere(sphere_surface) => {
            let center = sphere_surface.center().get();
            let axis = sphere_surface.frame().axis().as_raw();
            let ref_direction = sphere_surface.frame().reference().as_raw();
            let radius = sphere_surface.radius().get();
            let transverse = axis.cross(*ref_direction);
            let u_cosine = u.cos();
            let u_sine = u.sin();
            let v_cosine = v.cos();
            let v_sine = v.sin();
            Some(SurfaceSecondPartials {
                point: offset(
                    center,
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
        SolvedSurfaceGeometry::Torus(torus_surface) => {
            let center = torus_surface.center().get();
            let axis = torus_surface.frame().axis().as_raw();
            let ref_direction = torus_surface.frame().reference().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            let transverse = axis.cross(*ref_direction);
            let u_cosine = u.cos();
            let u_sine = u.sin();
            let v_cosine = v.cos();
            let v_sine = v.sin();
            let ring = major_radius + minor_radius * v_cosine;
            Some(SurfaceSecondPartials {
                point: offset(
                    center,
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
        SolvedSurfaceGeometry::Nurbs(_)
        | SolvedSurfaceGeometry::Transformed(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Unknown { .. } => None,
    }
}

/// The point with the first and second partials of a directly stored
/// surface at `(u, v)`, each partial order with its own outcome, or why the
/// point has none. Within a work slice, NURBS carriers charge their partials
/// work and placed carriers each placement; a refused charge leaves no
/// value.
///
/// The descent is bounded by [`PlacedSurface`](crate::geometry::PlacedSurface)
/// construction; no arm follows an arena id.
fn surface_jet_solved(
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    match geometry {
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            if let Some(budget) = budget {
                charge_nurbs_surface_partials(nurbs, budget)?;
            }
            nurbs_surface_jet(nurbs, u, v)
        }
        SolvedSurfaceGeometry::Transformed(placed) => {
            if budget.is_some_and(|budget| !budget.charge()) {
                return Err(EvaluationFailure::NoValue);
            }
            placed_jet(
                *placed.transform(),
                surface_jet_solved(placed.basis(), u, v, budget),
            )
        }
        _ => {
            let partials = analytic_surface_second_partials(geometry, u, v)
                .ok_or(EvaluationFailure::NoValue)?;
            Ok(SurfaceJet::formed(
                admit_point(partials.point)?,
                Ok([partials.du, partials.dv]),
                Ok([partials.duu, partials.duv, partials.dvv]),
            ))
        }
    }
}

/// The point with the first partials of a directly stored surface at
/// `(u, v)`, the partials with their own outcome, or why the point has none,
/// in the terms of [`surface_jet_solved`].
///
/// The descent is bounded by [`PlacedSurface`](crate::geometry::PlacedSurface)
/// construction; no arm follows an arena id.
fn surface_first_order_solved(
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    match geometry {
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            if let Some(budget) = budget {
                charge_nurbs_surface_partials(nurbs, budget)?;
            }
            nurbs_surface_first_order(nurbs, u, v)
        }
        SolvedSurfaceGeometry::Transformed(placed) => {
            if budget.is_some_and(|budget| !budget.charge()) {
                return Err(EvaluationFailure::NoValue);
            }
            let transform = *placed.transform();
            let basis = surface_first_order_solved(placed.basis(), u, v, budget)
                .map_err(|failure| failure.map(|point| placed_reach(transform, point)))?;
            Ok(SurfaceFirstOrder {
                point: transform
                    .apply_point_reaching(basis.point.get())
                    .map_err(EvaluationFailure::NonFinite)?,
                first: basis
                    .first
                    .and_then(|vectors| placed_vectors(transform, vectors)),
            })
        }
        _ => {
            let partials = analytic_surface_second_partials(geometry, u, v)
                .ok_or(EvaluationFailure::NoValue)?;
            Ok(SurfaceFirstOrder {
                point: admit_point(partials.point)?,
                first: admit_lanes([partials.du, partials.dv]),
            })
        }
    }
}

/// The point a placement reaches from a basis point outside the finite
/// range, finite or not.
fn placed_reach(transform: Transform, point: Point3) -> Point3 {
    transform
        .apply_point_reaching(point)
        .map_or_else(|point| point, FinitePoint3::get)
}

/// Vectors under a placement's linear part: a vector the placement leaves
/// outside the finite range leaves them there.
fn placed_vectors<const N: usize>(
    transform: Transform,
    vectors: [FiniteVector3; N],
) -> Result<[FiniteVector3; N], EvaluationFailure<()>> {
    let mut placed = vectors;
    for vector in &mut placed {
        *vector = transform
            .apply_vector(vector.get())
            .ok_or(EvaluationFailure::NonFinite(()))?;
    }
    Ok(placed)
}

/// A placed carrier's jet from its basis jet: the placed point, and each
/// order under the placement's linear part. A basis point outside the finite
/// range carries the point the placement reaches from it.
fn placed_jet(
    transform: Transform,
    basis: Result<SurfaceJet, EvaluationFailure<Point3>>,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    let basis = basis.map_err(|failure| failure.map(|point| placed_reach(transform, point)))?;
    Ok(SurfaceJet {
        point: transform
            .apply_point_reaching(basis.point.get())
            .map_err(EvaluationFailure::NonFinite)?,
        first: basis
            .first
            .and_then(|vectors| placed_vectors(transform, vectors)),
        second: basis
            .second
            .and_then(|vectors| placed_vectors(transform, vectors)),
    })
}

/// Evaluate a directly stored surface and its exact first and second partial
/// derivatives, or report why they have no finite value.
///
/// The point fails as [`surface_point_solved`] states. At a finite point,
/// partials outside the finite range leave the evaluation there, carrying
/// the point.
pub fn surface_second_partials_solved(
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    surface_jet_solved(geometry, u, v, None)?.second_partials()
}

/// Evaluate a surface carrier with access to construction and child-carrier
/// arenas in `ir`.
pub fn model_surface_point(
    ir: &CadIr,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    model_surface_point_inner(ir, geometry, u, v, None)
}

fn model_surface_point_inner(
    ir: &CadIr,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let _depth = ModelEvaluationDepthGuard::enter(budget).ok_or(EvaluationFailure::NoValue)?;
    if let Some(cache) = geometry.solved_cache() {
        return surface_point_solved(cache, u, v);
    }
    let Some(construction) = geometry.procedural_construction() else {
        return surface_point(geometry, u, v);
    };
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.id == *construction)
        .ok_or(EvaluationFailure::NoValue)?;
    let carrier_interval = record_u_interval(procedural.record_bounds());
    let index = crate::index::ModelIndex::new(ir);
    match procedural.definition() {
        ProceduralSurfaceDefinition::Extrusion(definition_payload) => {
            model_native_extrusion_point(&index, definition_payload, carrier_interval, u, v, budget)
        }
        ProceduralSurfaceDefinition::LinearSweep(definition_payload) => {
            model_linear_sweep_point(&index, definition_payload, u, v, budget)
        }
        ProceduralSurfaceDefinition::Revolution(definition_payload) => {
            model_native_revolution_point(
                &index,
                definition_payload,
                carrier_interval,
                u,
                v,
                budget,
            )
        }
        ProceduralSurfaceDefinition::AxisRevolution(definition_payload) => {
            model_axis_revolution_point(
                &index,
                definition_payload.directrix(),
                definition_payload.axis_origin().get(),
                definition_payload.axis_direction(),
                u,
                v,
                budget,
            )
        }
        ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
            model_ruled_surface_jet(&index, first, second, u, v).map(|jet| jet.point)
        }
        ProceduralSurfaceDefinition::Sum(definition_payload) => {
            model_sum_surface_jet(&index, definition_payload, u, v).map(|jet| jet.point)
        }
        ProceduralSurfaceDefinition::Sweep(definition_payload) => {
            let construction = definition_payload
                .native()
                .as_deref()
                .ok_or(EvaluationFailure::NoValue)?;
            cacheless_law_sweep_point(
                &index,
                definition_payload.profile(),
                definition_payload.spine(),
                construction,
                u,
                v,
            )
            .and_then(admit_point)
        }
        ProceduralSurfaceDefinition::VariableBlend(definition_payload) => {
            cacheless_variable_blend_point(&index, definition_payload.construction(), u, v)
                .and_then(admit_point)
        }
        ProceduralSurfaceDefinition::Blend(definition_payload) => {
            let (native, native_ranges) = definition_payload
                .native_with_ranges()
                .ok_or(EvaluationFailure::NoValue)?;
            cacheless_constant_rolling_ball_point(
                &index,
                definition_payload.supports(),
                definition_payload.radius(),
                definition_payload.cross_section(),
                native,
                native_ranges,
                u,
                v,
            )
            .map_err(|failure| failure.map(|()| UNREACHED_POINT))
            .and_then(admit_point)
        }
        ProceduralSurfaceDefinition::RollingBallJet(_) => {
            rolling_ball_jet_point(procedural.definition(), u, v)
        }
        _ => Err(EvaluationFailure::NoValue),
    }
}

/// The point of a linear sweep: the directrix point displaced by `v` along
/// the sweep direction. A `v` that is not finite has no value; a directrix
/// point outside the finite range leaves the surface there, at the point
/// its displacement reaches.
fn model_linear_sweep_point(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::LinearSweepSurfaceConstruction,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    if !v.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let directrix = construction.directrix();
    let direction = construction.direction().get();
    let point = budget
        .map_or_else(
            || model_curve_point_by_id(index, directrix, u),
            |budget| model_curve_point_by_id_with_budget(index, directrix, u, budget),
        )
        .map_err(|failure| failure.map(|point| offset(point, &[(v, direction)])))?;
    admit_point(offset(point.get(), &[(v, direction)]))
}

/// The jet of a linear sweep, or why its point has none. The first partials
/// read the directrix tangent, and the second its acceleration.
fn model_linear_sweep_jet(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::LinearSweepSurfaceConstruction,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    if !v.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let directrix = construction.directrix();
    let direction = *construction.direction();
    let differential = model_curve_differential_by_id_inner(index, directrix, u, budget)
        .map_err(|failure| failure.map(|point| offset(point, &[(v, direction.get())])))?;
    Ok(SurfaceJet {
        point: admit_point(offset(differential.point.get(), &[(v, direction.get())]))?,
        first: differential.tangent.map(|tangent| [tangent, direction]),
        second: differential
            .acceleration
            .map(|acceleration| [acceleration, FiniteVector3::ZERO, FiniteVector3::ZERO]),
    })
}

/// A scalar law's finite value with its derivative, which states its own
/// outcome: a value can exist where its derivative has no value, or where
/// its derivative left the finite range.
#[derive(Clone, Copy)]
struct ScalarSweepDifferential {
    value: FiniteReal,
    derivative: Result<FiniteReal, EvaluationFailure<()>>,
}

/// A law value or derivative computed from finite operands: one outside the
/// finite range leaves the law there.
fn law_real(value: f64) -> Result<FiniteReal, EvaluationFailure<()>> {
    FiniteReal::new(value).ok_or(EvaluationFailure::NonFinite(()))
}

/// A constant law whose value is finite by its type.
fn constant_sweep_differential(value: FiniteReal) -> ScalarSweepDifferential {
    ScalarSweepDifferential {
        value,
        derivative: Ok(FiniteReal::ZERO),
    }
}

/// The derivatives of two operands, or the failure of the first that has
/// none.
fn operand_derivatives(
    first: ScalarSweepDifferential,
    second: ScalarSweepDifferential,
) -> Result<(FiniteReal, FiniteReal), EvaluationFailure<()>> {
    Ok((first.derivative?, second.derivative?))
}

/// A scalar sweep law's value and derivative at `parameter`, or why it has
/// no value.
///
/// A form the evaluator does not read, a text that is not a finite number or
/// a finite multiple of `X`, an operand outside its operator's domain and a
/// division by zero state no value; a value that overflows leaves the law
/// outside the finite range. The derivative states its own outcome in the
/// same terms.
fn scalar_sweep_law_differential(
    expression: &LawExpression<FiniteReal, FiniteVector3, FinitePoint3>,
    parameter: FiniteReal,
) -> Result<ScalarSweepDifferential, EvaluationFailure<()>> {
    let no_value = EvaluationFailure::NoValue;
    match expression {
        LawExpression::Null {} => Ok(constant_sweep_differential(FiniteReal::ZERO)),
        LawExpression::Integer { value } => Ok(constant_sweep_differential(
            FiniteReal::from_integer(*value),
        )),
        LawExpression::Double { value } => Ok(constant_sweep_differential(*value)),
        LawExpression::Text { value } => {
            let value = value.as_str().trim();
            if value == "X" {
                return Ok(ScalarSweepDifferential {
                    value: parameter,
                    derivative: Ok(FiniteReal::ONE),
                });
            }
            // A text constant that is not finite states no value.
            if let Ok(constant) = value.parse::<f64>() {
                return FiniteReal::new(constant)
                    .map(constant_sweep_differential)
                    .ok_or(no_value);
            }
            let (left, right) = value.split_once('*').ok_or(no_value)?;
            let coefficient = if right.trim() == "X" {
                left.trim().parse::<f64>()
            } else if left.trim() == "X" {
                right.trim().parse::<f64>()
            } else {
                return Err(no_value);
            };
            let coefficient = coefficient.ok().and_then(FiniteReal::new).ok_or(no_value)?;
            Ok(ScalarSweepDifferential {
                value: law_real(coefficient.get() * parameter.get())?,
                derivative: Ok(coefficient),
            })
        }
        LawExpression::Algebraic { operator, operands } => {
            if let [operand] = operands.as_slice() {
                let operand = scalar_sweep_law_differential(operand, parameter)?;
                return scalar_unary_sweep_law_differential(operator, operand);
            }
            if operator == "O" {
                let [outer, inner] = operands.as_slice() else {
                    return Err(no_value);
                };
                let inner = scalar_sweep_law_differential(inner, parameter)?;
                let outer = scalar_sweep_law_differential(outer, inner.value)?;
                return Ok(ScalarSweepDifferential {
                    value: outer.value,
                    derivative: operand_derivatives(outer, inner)
                        .and_then(|(outer, inner)| law_real(outer.get() * inner.get())),
                });
            }
            let [left, right] = operands.as_slice() else {
                return Err(no_value);
            };
            let left = scalar_sweep_law_differential(left, parameter)?;
            let right = scalar_sweep_law_differential(right, parameter)?;
            let (x, y) = (left.value.get(), right.value.get());
            let derivatives = operand_derivatives(left, right);
            match operator.as_str() {
                "ADD" => Ok(ScalarSweepDifferential {
                    value: law_real(x + y)?,
                    derivative: derivatives.and_then(|(dx, dy)| law_real(dx.get() + dy.get())),
                }),
                "SUB" => Ok(ScalarSweepDifferential {
                    value: law_real(x - y)?,
                    derivative: derivatives.and_then(|(dx, dy)| law_real(dx.get() - dy.get())),
                }),
                "MUL" => Ok(ScalarSweepDifferential {
                    value: law_real(x * y)?,
                    derivative: derivatives
                        .and_then(|(dx, dy)| law_real(dx.get() * y + x * dy.get())),
                }),
                "DIV" => {
                    let divisor = NonZeroReal::new(y).ok_or(no_value)?;
                    Ok(ScalarSweepDifferential {
                        value: law_real(x / y)?,
                        derivative: derivatives.and_then(|(dx, dy)| {
                            let mut numerator = ExactSignedSum::default();
                            numerator.add_product(dx.get(), y);
                            numerator.add_product(-x, dy.get());
                            numerator.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                                sweep_quotient(
                                    value,
                                    crate::math::sum::ScaledValue::product_of_nonzero([
                                        divisor, divisor,
                                    ]),
                                )
                            })
                        }),
                    })
                }
                _ => Err(no_value),
            }
        }
        LawExpression::Point { .. }
        | LawExpression::Vector { .. }
        | LawExpression::Transform { .. }
        | LawExpression::TransformVec { .. }
        | LawExpression::Edge { .. }
        | LawExpression::Spline { .. } => Err(no_value),
    }
}

/// `numerator / denominator` for a law derivative. A quotient that overflows
/// leaves the derivative outside the finite range.
fn sweep_quotient(
    numerator: crate::math::sum::ScaledValue,
    denominator: crate::math::sum::ScaledValue,
) -> Result<FiniteReal, EvaluationFailure<()>> {
    numerator
        .quotient(denominator)
        .map_err(|_| EvaluationFailure::NonFinite(()))
}

/// The derivative `factor * derivative` of a unary law whose operand has
/// derivative `derivative`, or the first failure among the two.
fn chain_derivative(
    factor: Result<f64, EvaluationFailure<()>>,
    derivative: Result<FiniteReal, EvaluationFailure<()>>,
) -> Result<FiniteReal, EvaluationFailure<()>> {
    let derivative = derivative?;
    law_real(factor? * derivative.get())
}

/// A unary law operator applied to its operand's value and derivative. An
/// operand outside the domain of the operator's value, and an operator the
/// evaluator does not read, state no value; a value that overflows leaves
/// the law outside the finite range. The derivative states its own outcome:
/// an operand outside the domain of the operator's derivative states no
/// derivative, and a derivative that overflows left the finite range.
fn scalar_unary_sweep_law_differential(
    operator: &str,
    operand: ScalarSweepDifferential,
) -> Result<ScalarSweepDifferential, EvaluationFailure<()>> {
    let no_value = EvaluationFailure::NoValue;
    let non_finite = EvaluationFailure::NonFinite(());
    let x = operand.value.get();
    let law = |value: f64,
               derivative: Result<FiniteReal, EvaluationFailure<()>>|
     -> Result<ScalarSweepDifferential, EvaluationFailure<()>> {
        Ok(ScalarSweepDifferential {
            value: law_real(value)?,
            derivative,
        })
    };
    match operator {
        "LN" => {
            if x <= 0.0 {
                return Err(no_value);
            }
            return law(
                x.ln(),
                operand
                    .derivative
                    .and_then(|derivative| law_real(derivative.get() / x)),
            );
        }
        "EXP" => {
            let value = x.exp();
            let half = (0.5 * x).exp();
            return law(
                value,
                operand.derivative.and_then(|derivative| {
                    let mut product = ExactSignedSum::default();
                    product.add_factors([half, half, derivative.get()]);
                    product.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                        value.finite().map_err(|_| non_finite)
                    })
                }),
            );
        }
        "COT" | "CSC" | "TAN" | "SEC" | "ARCSECH" => {
            let (value, factor, denominator) = match operator {
                "COT" | "CSC" => {
                    let sine = NonZeroReal::new(x.sin()).ok_or(no_value)?;
                    let denominator =
                        crate::math::sum::ScaledValue::product_of_nonzero([sine, sine]);
                    if operator == "COT" {
                        (1.0 / x.tan(), -1.0, Ok(denominator))
                    } else {
                        (1.0 / sine.get(), -x.cos(), Ok(denominator))
                    }
                }
                "TAN" | "SEC" => {
                    let cosine = NonZeroReal::new(x.cos()).ok_or(no_value)?;
                    let denominator =
                        crate::math::sum::ScaledValue::product_of_nonzero([cosine, cosine]);
                    if operator == "TAN" {
                        (x.tan(), 1.0, Ok(denominator))
                    } else {
                        (1.0 / cosine.get(), x.sin(), Ok(denominator))
                    }
                }
                _ => {
                    // The value is defined on (0, 1]; at 1 the root is zero
                    // and the derivative has no value.
                    let positive = PositiveReal::new(x).ok_or(no_value)?;
                    if x > 1.0 {
                        return Err(no_value);
                    }
                    let root = positive.unit_complement_root();
                    (
                        (1.0 + root.map_or(0.0, PositiveReal::get)).ln() - x.ln(),
                        -1.0,
                        root.map(|root| {
                            crate::math::sum::ScaledValue::product_of_nonzero([
                                positive.into(),
                                root.into(),
                            ])
                        })
                        .ok_or(no_value),
                    )
                }
            };
            return law(
                value,
                operand.derivative.and_then(|derivative| {
                    let denominator = denominator?;
                    let mut numerator = ExactSignedSum::default();
                    numerator.add_product(factor, derivative.get());
                    numerator.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                        sweep_quotient(value, denominator)
                    })
                }),
            );
        }
        "ARCTAN" | "ARCOT" | "ARCSEC" | "ARCCSC" | "ARCCSCH" => {
            let (value, sign, denominator) = match operator {
                "ARCTAN" | "ARCOT" => {
                    let hypotenuse = operand.value.hypot_one_nonzero();
                    let denominator =
                        Some(ScaledValue::product_of_nonzero([hypotenuse, hypotenuse]));
                    if operator == "ARCTAN" {
                        (x.atan(), 1.0, denominator)
                    } else {
                        (std::f64::consts::FRAC_PI_2 - x.atan(), -1.0, denominator)
                    }
                }
                "ARCSEC" | "ARCCSC" => {
                    if x.abs() < 1.0 {
                        return Err(no_value);
                    }
                    let denominator = operand.value.beyond_unit().map(|beyond| {
                        let magnitude = beyond.magnitude();
                        let factor = beyond.arcsec_factor();
                        ScaledValue::product_of_nonzero([magnitude, magnitude, factor])
                    });
                    if operator == "ARCSEC" {
                        ((1.0 / x).acos(), 1.0, denominator)
                    } else {
                        ((1.0 / x).asin(), -1.0, denominator)
                    }
                }
                _ => {
                    let magnitude = NonZeroReal::new(x).ok_or(no_value)?.magnitude();
                    let hypotenuse = operand.value.hypot_one_nonzero();
                    let denominator =
                        Some(ScaledValue::product_of_nonzero([magnitude, hypotenuse]));
                    let inverse = 1.0 / x;
                    let value = if inverse.is_finite() {
                        inverse.asinh()
                    } else {
                        (std::f64::consts::LN_2 - x.abs().ln()).copysign(x)
                    };
                    (value, -1.0, denominator)
                }
            };
            return law(
                value,
                operand.derivative.and_then(|derivative| {
                    let denominator = denominator.ok_or(no_value)?;
                    // The operand derivative is finite, so it has a scaled
                    // form exactly when it is not zero.
                    match crate::math::sum::scaled_finite(derivative.get()) {
                        Some(numerator) => {
                            let quotient = sweep_quotient(numerator, denominator)?;
                            Ok(if sign < 0.0 {
                                quotient.negated()
                            } else {
                                quotient
                            })
                        }
                        None => Ok(FiniteReal::ZERO),
                    }
                }),
            );
        }
        "COTH" | "SECH" | "CSCH" => {
            let (tail, unit_sum) = operand.value.hyperbolic_tail_unit_sum();
            let (value, numerator_factors, denominator) = match operator {
                "COTH" => {
                    let sinh = NonZeroReal::new(x)
                        .ok_or(no_value)?
                        .hyperbolic_sinh_denominator();
                    (
                        1.0 / x.tanh(),
                        [-4.0, tail, tail],
                        ScaledValue::product_of_nonzero([sinh, sinh]),
                    )
                }
                "SECH" => {
                    let half_tail = (-0.5 * x.abs()).exp();
                    (
                        2.0 * tail / (1.0 + tail * tail),
                        [-2.0 * x.tanh(), half_tail, half_tail],
                        ScaledValue::of_nonzero(unit_sum),
                    )
                }
                _ => {
                    let half_tail = (-0.5 * x.abs()).exp();
                    let sinh = NonZeroReal::new(x)
                        .ok_or(no_value)?
                        .hyperbolic_sinh_denominator();
                    (
                        (2.0 * tail / sinh.get()).copysign(x),
                        [-2.0 * (1.0 + tail * tail), half_tail, half_tail],
                        ScaledValue::product_of_nonzero([sinh, sinh]),
                    )
                }
            };
            return law(
                value,
                operand.derivative.and_then(|derivative| {
                    let [first, second, third] = numerator_factors;
                    let mut numerator = ExactSignedSum::default();
                    numerator.add_factors([first, second, third, derivative.get()]);
                    match numerator.finish() {
                        Some(value) => sweep_quotient(value, denominator),
                        None => Ok(FiniteReal::ZERO),
                    }
                }),
            );
        }
        "TANH" => {
            let (tail, denominator) = operand.value.hyperbolic_tail();
            return law(
                x.tanh(),
                operand.derivative.and_then(|derivative| {
                    let mut numerator = ExactSignedSum::default();
                    numerator.add_factors([4.0, tail, tail, derivative.get()]);
                    match numerator.finish() {
                        Some(value) => sweep_quotient(
                            value,
                            crate::math::sum::ScaledValue::of_nonzero(denominator),
                        ),
                        None => Ok(FiniteReal::ZERO),
                    }
                }),
            );
        }
        "ARCSINH" => {
            return law(
                x.asinh(),
                operand
                    .derivative
                    .and_then(|derivative| law_real(derivative.get() / x.hypot(1.0))),
            )
        }
        "ARCCOSH" => {
            if x < 1.0 {
                return Err(no_value);
            }
            return law(
                x.acosh(),
                operand.derivative.and_then(|derivative| {
                    if x == 1.0 {
                        return Err(no_value);
                    }
                    let denominator = if x < 2.0 {
                        ((x - 1.0) * (x + 1.0)).sqrt()
                    } else {
                        x * (1.0 - (1.0 / x).powi(2)).sqrt()
                    };
                    law_real(derivative.get() / denominator)
                }),
            );
        }
        "ARCOTH" => {
            let beyond = operand.value.beyond_unit().ok_or(no_value)?;
            return Ok(ScalarSweepDifferential {
                value: beyond.arcoth(),
                derivative: operand.derivative.and_then(|derivative| {
                    // (derivative / x) * (-1 / x) over 1 - 1/x², formed as one
                    // exact product and one quotient.
                    let mut numerator = ExactSignedSum::default();
                    numerator.add_product(
                        beyond.quotient(derivative).get(),
                        beyond.quotient(FiniteReal::ONE).negated().get(),
                    );
                    numerator.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                        sweep_quotient(
                            value,
                            crate::math::sum::ScaledValue::of_nonzero(beyond.square_complement()),
                        )
                    })
                }),
            });
        }
        _ => {}
    }

    let (value, factor) = match operator {
        "SIN" => (x.sin(), Ok(x.cos())),
        "COS" => (x.cos(), Ok(-x.sin())),
        "COSH" => (x.cosh(), Ok(x.sinh())),
        "SINH" => (x.sinh(), Ok(x.cosh())),
        "ARCCOS" | "ARCSIN" => {
            if x.abs() > 1.0 {
                return Err(no_value);
            }
            let denominator = (1.0 - x * x).sqrt();
            let factor = (denominator > 0.0)
                .then_some(1.0 / denominator)
                .ok_or(no_value);
            if operator == "ARCCOS" {
                (x.acos(), factor.map(|factor| -factor))
            } else {
                (x.asin(), factor)
            }
        }
        "ARCTANH" => {
            if x.abs() >= 1.0 {
                return Err(no_value);
            }
            (x.atanh(), Ok(1.0 / (1.0 - x * x)))
        }
        "ABS" => (
            x.abs(),
            if x > 0.0 {
                Ok(1.0)
            } else if x < 0.0 {
                Ok(-1.0)
            } else {
                Err(no_value)
            },
        ),
        "SIGN" => {
            if x == 0.0 {
                return Err(no_value);
            }
            (x.signum(), Ok(0.0))
        }
        "SQRT" => {
            if x < 0.0 {
                return Err(no_value);
            }
            (x.sqrt(), (x > 0.0).then(|| 0.5 / x.sqrt()).ok_or(no_value))
        }
        _ => return Err(no_value),
    };
    law(value, chain_derivative(factor, operand.derivative))
}

fn sweep_scale(
    expression: &LawExpression<FiniteReal, FiniteVector3, FinitePoint3>,
) -> Option<Vector3> {
    match expression {
        LawExpression::Null {} => Some(Vector3::new(1.0, 1.0, 1.0)),
        LawExpression::Text { value } => {
            let value = value
                .as_str()
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
        LawExpression::Vector { value } => Some(value.get()),
        _ => None,
    }
}

/// The profile differential scaled about `frame_point`. A scaled point
/// outside the finite range is non-finite; each scaled derivative states its
/// own outcome.
fn scale_sweep_profile(
    profile: ModelCurveDifferential,
    frame_point: Point3,
    scale: Vector3,
) -> Result<ModelCurveDifferential, EvaluationFailure<Point3>> {
    let scaled =
        |vector: Vector3| Vector3::new(vector.x * scale.x, vector.y * scale.y, vector.z * scale.z);
    let point = offset(
        frame_point,
        &[(
            1.0,
            scaled(point_displacement(profile.point.get(), frame_point)),
        )],
    );
    Ok(ModelCurveDifferential {
        point: admit_point(point)?,
        tangent: profile
            .tangent
            .and_then(|tangent| admit_derivative(scaled(tangent.get()))),
        acceleration: profile
            .acceleration
            .and_then(|acceleration| admit_derivative(scaled(acceleration.get()))),
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

fn sweep_rail_transform(
    formula: &LawFormula<FiniteReal, FiniteVector3, FinitePoint3>,
) -> Option<Transform> {
    match formula {
        LawFormula::Null {} => {
            return Some(Transform::identity());
        }
        LawFormula::Named { name, variables } if variables.is_empty() => {
            let name = name
                .as_str()
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>();
            return unit_domain_sweep_formula(&name).then_some(Transform::identity());
        }
        LawFormula::Named { .. } => {}
    }
    let LawFormula::Named { name, variables } = formula else {
        return None;
    };
    let name = name
        .as_str()
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
    }] = variables.as_slice()
    else {
        return None;
    };
    if scale.get() != 1.0
        || *flags != [true, false, false]
        || vectors[3].get() != Vector3::new(0.0, 0.0, 0.0)
    {
        return None;
    }
    let [x, y, z] = [vectors[0], vectors[1], vectors[2]].map(FiniteVector3::components);
    let zero = FiniteReal::ZERO;
    let transform = Transform::from_finite_rows([
        [x[0], y[0], z[0], zero],
        [x[1], y[1], z[1], zero],
        [x[2], y[2], z[2], zero],
    ]);
    transform.is_proper_rigid().then_some(transform)
}

/// The origin of a straight sweep path: a line's origin, or the start of a
/// two-pole linear NURBS. Any other spine has no straight origin.
fn straight_sweep_path_origin(
    index: &crate::index::ModelIndex<'_>,
    spine: &crate::ids::CurveId,
) -> Result<Point3, EvaluationFailure<()>> {
    let curve = index
        .curves(spine.as_str())
        .ok_or(EvaluationFailure::NoValue)?;
    match &curve.geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            Ok(line_curve.origin().get())
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
            if nurbs.degree() == 1 && nurbs.control_points().len() == 2 && !nurbs.periodic() =>
        {
            let [start, _] = nurbs_curve_parameter_domain(nurbs)
                .ok_or(EvaluationFailure::NoValue)?
                .endpoints();
            curve_point(&curve.geometry, start)
                .map(FinitePoint3::get)
                .map_err(|failure| failure.map(|_| ()))
        }
        _ => Err(EvaluationFailure::NoValue),
    }
}

fn point_displacement(point: Point3, origin: Point3) -> Vector3 {
    Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z)
}

fn sweep_tail_interval_contains(interval: [Option<FiniteReal>; 2], parameter: FiniteReal) -> bool {
    interval[0].is_none_or(|lower| parameter >= lower)
        && interval[1].is_none_or(|upper| parameter <= upper)
}

/// The unit direction of a nonzero `vector`, none for the zero vector, and
/// the unit direction's derivative given the vector's derivative, with its
/// own outcome: a vector derivative without a value leaves it without one,
/// and a quotient that overflows leaves it outside the finite range.
fn unit_vector_with_derivative(
    vector: FiniteVector3,
    derivative: Result<FiniteVector3, EvaluationFailure<()>>,
) -> Option<(Vector3, Result<Vector3, EvaluationFailure<()>>)> {
    let (unit, cubed_length) = vector.unit_with_cubed_length()?;
    let vector = vector.get();
    let components = [vector.x, vector.y, vector.z];
    let denominator = crate::math::sum::ScaledValue::product_of_nonzero(cubed_length);
    let unit_derivative = derivative.and_then(|derivative| {
        let derivative = derivative.get();
        let derivatives = [derivative.x, derivative.y, derivative.z];
        // (|v|² d - v(v·d)) / |v|³. Exact products keep parallel derivatives
        // zero and postpone range checks until the normalized result is
        // formed.
        let component = |index: usize| {
            let mut numerator = ExactSignedSum::default();
            for other in 0..3 {
                if other != index {
                    numerator.add_factors([
                        components[other],
                        components[other],
                        derivatives[index],
                    ]);
                    numerator.add_factors([
                        -components[index],
                        components[other],
                        derivatives[other],
                    ]);
                }
            }
            numerator.finish().map_or(Ok(0.0), |value| {
                value
                    .quotient(denominator)
                    .map(FiniteReal::get)
                    .map_err(|_| EvaluationFailure::NonFinite(()))
            })
        };
        Ok(Vector3::new(component(0)?, component(1)?, component(2)?))
    });
    Some((unit, unit_derivative))
}

/// Whether the profile frame runs against the spine tangent. A frame
/// vector or spine tangent that is zero or not aligned states no direction;
/// one whose length overflows still has a direction.
fn sweep_profile_reversed(
    profile_frame: Option<(FinitePoint3, FiniteVector3)>,
    spine_tangent: Vector3,
) -> Result<bool, EvaluationFailure<()>> {
    let Some((_, frame_vector)) = profile_frame else {
        return Ok(false);
    };
    let unit = |vector: Vector3| {
        let finite = FiniteVector3::new(vector).ok_or(EvaluationFailure::NonFinite(()))?;
        if vector.norm() <= f64::EPSILON {
            return Err(EvaluationFailure::NoValue);
        }
        finite.unit_nonzero().ok_or(EvaluationFailure::NoValue)
    };
    let frame_vector = unit(frame_vector.get())?;
    let spine_tangent = unit(spine_tangent)?;
    let alignment = frame_vector.dot(spine_tangent);
    ((alignment.abs() - 1.0).abs() <= EPS_EVAL_SWEEP_PROFILE_FRAME_ALIGNMENT_E9)
        .then_some(alignment < 0.0)
        .ok_or(EvaluationFailure::NoValue)
}

/// Evaluate a sweep profile in its native domain and scale its derivatives.
/// Out-of-range parameters and reverse mappings without a native domain have
/// no value; a mapped parameter outside finite range reaches no coordinate.
fn sweep_profile_differential(
    index: &crate::index::ModelIndex<'_>,
    profile: &crate::ids::CurveId,
    profile_range: [FiniteReal; 2],
    reversed: bool,
    parameter: FiniteReal,
) -> Result<ModelCurveDifferential, EvaluationFailure<Point3>> {
    let no_value = EvaluationFailure::NoValue;
    let unreached = EvaluationFailure::NonFinite(UNREACHED_POINT);
    if !sweep_tail_interval_contains([Some(profile_range[0]), Some(profile_range[1])], parameter) {
        return Err(no_value);
    }
    let profile_interval =
        IncreasingParameterInterval::new(FiniteReal::raw_array(profile_range)).ok_or(no_value)?;
    let curve = index.curves(profile.as_str()).ok_or(no_value)?;
    let (native_parameter, parameter_scale) = match &curve.geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            let native_interval = nurbs_curve_parameter_domain(nurbs).ok_or(no_value)?;
            let native_parameter = native_interval
                .map_from(profile_interval, parameter, reversed)
                .map_err(|_| unreached)?
                .get();
            let scale = native_interval
                .scaled_span()
                .quotient(profile_interval.scaled_span())
                .map_or_else(|overflow| overflow, FiniteReal::get);
            (native_parameter, if reversed { -scale } else { scale })
        }
        _ if !reversed => (parameter.get(), 1.0),
        _ => return Err(no_value),
    };
    let differential = model_curve_differential_by_id(index, profile, native_parameter)?;
    Ok(ModelCurveDifferential {
        point: differential.point,
        tangent: differential
            .tangent
            .and_then(|tangent| admit_derivative(scale_vector(tangent.get(), parameter_scale))),
        acceleration: differential.acceleration.and_then(|acceleration| {
            admit_derivative(scale_vector(
                acceleration.get(),
                parameter_scale * parameter_scale,
            ))
        }),
    })
}

/// The spine of a law-driven sweep at a spine parameter: its point, its
/// tangent, which the section frame reads, and its acceleration with its
/// own outcome.
#[derive(Clone, Copy)]
struct SweepSpine {
    point: FinitePoint3,
    tangent: FiniteVector3,
    acceleration: Result<FiniteVector3, EvaluationFailure<()>>,
}

/// The placed profile differential, the spine, the law value and derivative,
/// and the straight path origin of a law-driven sweep at `(u, v)`, or why
/// there are none. A sweep outside the law-driven straight form the
/// evaluator reads, and a parameter outside its intervals, have no value.
/// The section frame reads the spine tangent, so a spine tangent without a
/// value fails the sweep, carrying the spine point.
fn cacheless_law_sweep_differentials(
    index: &crate::index::ModelIndex<'_>,
    profile: &crate::ids::CurveId,
    spine: &crate::ids::CurveId,
    construction: &crate::geometry::SweepSurfaceConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Result<
    (
        ModelCurveDifferential,
        SweepSpine,
        ScalarSweepDifferential,
        Point3,
    ),
    EvaluationFailure<Point3>,
> {
    let no_value = EvaluationFailure::NoValue;
    let unreached = |failure: EvaluationFailure<()>| failure.map(|()| UNREACHED_POINT);
    let form = construction.cache.form().ok_or(no_value)?;
    let parameterization = form.cache.parameterization().ok_or(no_value)?;
    let path_origin = straight_sweep_path_origin(index, spine).map_err(unreached)?;
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
        return Err(no_value);
    };
    let rail_transform = sweep_rail_transform(formula).ok_or(no_value)?;
    let scale = sweep_scale(second_law).ok_or(no_value)?;
    let (Some(u), Some(v)) = (FiniteReal::new(u), FiniteReal::new(v)) else {
        return Err(no_value);
    };
    if *path_mode != 1
        || *formula_mode != 0
        || *trailing_flag
        || !sweep_tail_interval_contains(parameterization.u_interval, u)
        || !sweep_tail_interval_contains(parameterization.v_interval, v)
        || !sweep_tail_interval_contains([Some(first_range[0]), Some(first_range[1])], v)
    {
        return Err(no_value);
    }
    let spine = model_curve_differential_by_id(index, spine, v.get())?;
    let spine = SweepSpine {
        point: spine.point,
        tangent: spine.tangent()?,
        acceleration: spine.acceleration,
    };
    let reversed =
        sweep_profile_reversed(*profile_frame, spine.tangent.get()).map_err(unreached)?;
    let profile = sweep_profile_differential(index, profile, *profile_range, reversed, u)?;
    let frame_point = profile_frame.map_or(*origin, |(point, _)| point).get();
    let profile = scale_sweep_profile(profile, frame_point, scale)?;
    let profile = ModelCurveDifferential {
        point: rail_transform
            .apply_point_reaching(profile.point.get())
            .map_err(EvaluationFailure::NonFinite)?,
        tangent: placed_derivative(rail_transform, profile.tangent),
        acceleration: placed_derivative(rail_transform, profile.acceleration),
    };
    let law = scalar_sweep_law_differential(first_law, v).map_err(unreached)?;
    Ok((profile, spine, law, path_origin))
}

/// The raw point of a law-driven sweep, or why it has none: the profile
/// point displaced along the spine and by the law along the section normal.
/// The normal reads the profile and spine tangents; the point reads no
/// acceleration and no law derivative.
fn cacheless_law_sweep_point(
    index: &crate::index::ModelIndex<'_>,
    profile: &crate::ids::CurveId,
    spine: &crate::ids::CurveId,
    construction: &crate::geometry::SweepSurfaceConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Result<Point3, EvaluationFailure<Point3>> {
    let (profile, spine, law, path_origin) =
        cacheless_law_sweep_differentials(index, profile, spine, construction, u, v)?;
    let profile_tangent = profile
        .tangent()?
        .unit_nonzero()
        .ok_or(EvaluationFailure::NoValue)?;
    let spine_tangent = spine
        .tangent
        .unit_nonzero()
        .ok_or(EvaluationFailure::NoValue)?;
    let normal = profile_tangent.cross(spine_tangent);
    Ok(offset(
        profile.point.get(),
        &[
            (1.0, point_displacement(spine.point.get(), path_origin)),
            (law.value.get(), normal),
        ],
    ))
}

/// The point and first partials of a law-driven sweep, the partials with
/// their own outcome, or why the point has none. The partials read the
/// profile and spine accelerations and the law derivative besides what the
/// point reads.
fn cacheless_law_sweep_first_order(
    index: &crate::index::ModelIndex<'_>,
    profile: &crate::ids::CurveId,
    spine: &crate::ids::CurveId,
    construction: &crate::geometry::SweepSurfaceConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let (profile, spine, law, path_origin) =
        cacheless_law_sweep_differentials(index, profile, spine, construction, u, v)?;
    let profile_tangent = profile.tangent()?;
    let (profile_unit, profile_unit_derivative) =
        unit_vector_with_derivative(profile_tangent, profile.acceleration)
            .ok_or(EvaluationFailure::NoValue)?;
    let (spine_unit, spine_unit_derivative) =
        unit_vector_with_derivative(spine.tangent, spine.acceleration)
            .ok_or(EvaluationFailure::NoValue)?;
    let normal = profile_unit.cross(spine_unit);
    let point = admit_point(offset(
        profile.point.get(),
        &[
            (1.0, point_displacement(spine.point.get(), path_origin)),
            (law.value.get(), normal),
        ],
    ))?;
    let first = (|| {
        let normal_u =
            profile_unit_derivative?.cross(spine_unit) + profile_unit.cross(spine_unit_derivative?);
        admit_lanes([
            profile_tangent.get() + scale_vector(normal_u, law.value.get()),
            spine.tangent.get() + scale_vector(normal, law.derivative?.get()),
        ])
    })();
    Ok(SurfaceFirstOrder { point, first })
}

/// The unit direction of a vector formed from finite values: a component
/// outside the finite range leaves the evaluation there, and a vector within
/// [`f64::EPSILON`] of zero has no direction.
fn unit_direction(vector: Vector3) -> Result<Vector3, EvaluationFailure<()>> {
    if !vector.is_finite() {
        return Err(EvaluationFailure::NonFinite(()));
    }
    vector.unit().ok_or(EvaluationFailure::NoValue)
}

/// The unit cross direction of finite partials. Normalizing each partial
/// before crossing retains the direction when their raw cross overflows.
fn unit_cross_direction(
    first: FiniteVector3,
    second: FiniteVector3,
) -> Result<Vector3, EvaluationFailure<()>> {
    let first = first.unit_nonzero().ok_or(EvaluationFailure::NoValue)?;
    let second = second.unit_nonzero().ok_or(EvaluationFailure::NoValue)?;
    first.cross(second).unit().ok_or(EvaluationFailure::NoValue)
}

/// The contact track of a blend side at a parameter: the support's point
/// with its first partials, the side pcurve's tangent, and the derivative of
/// the support's unit normal along the track, each derivative with its own
/// outcome.
#[derive(Clone, Copy)]
struct ContactTrack {
    support: SurfaceFirstOrder,
    uv_tangent: Result<FinitePoint2, EvaluationFailure<()>>,
    normal_derivative: Result<Vector3, EvaluationFailure<()>>,
}

impl ContactTrack {
    /// The support point.
    fn point(&self) -> Point3 {
        self.support.point.get()
    }

    /// The support's unit normal: a degenerate normal has no direction.
    fn normal(&self) -> Result<Vector3, EvaluationFailure<()>> {
        let [du, dv] = self.support.first?;
        let cross = du.get().cross(dv.get());
        if cross.is_finite() {
            unit_direction(cross)
        } else {
            unit_cross_direction(du, dv)
        }
    }

    /// The track tangent: the pcurve tangent carried by the support's first
    /// partials.
    fn tangent(&self) -> Result<Vector3, EvaluationFailure<()>> {
        let uv_tangent = self.uv_tangent?;
        let [du, dv] = self.support.first?;
        Ok(vector_sum(&[
            (uv_tangent.u, du.get()),
            (uv_tangent.v, dv.get()),
        ]))
    }
}

/// The contact track of a blend side at `parameter`, or why its support
/// point has none. A side without a support or pcurve, and a pcurve without
/// a point there, have no value. The track's derivatives state their own
/// outcomes.
fn variable_blend_contact_track(
    index: &crate::index::ModelIndex<'_>,
    side: &crate::geometry::RollingBallSide<
        crate::ids::SurfaceId,
        crate::ids::CurveId,
        PcurveGeometry,
        FiniteReal,
        FinitePoint3,
    >,
    parameter: f64,
) -> Result<ContactTrack, EvaluationFailure<()>> {
    let no_value = EvaluationFailure::NoValue;
    let surface = &side.surface.as_ref().ok_or(no_value)?.surface;
    let pcurve = side.pcurve.as_ref().ok_or(no_value)?;
    // A non-finite offset-pcurve point is evaluated on its support as a
    // finite one is.
    let uv = match pcurve_uv(pcurve, parameter) {
        Ok(uv) => uv.get(),
        Err(EvaluationFailure::NonFinite(uv)) => uv,
        Err(EvaluationFailure::NoValue) => return Err(no_value),
    };
    let support = model_surface_first_order_by_id(index, surface, uv.u, uv.v, None)
        .map_err(|failure| failure.map(|_| ()))?;
    let uv_tangent = pcurve_tangent(pcurve, parameter).map_err(|failure| failure.map(|_| ()));
    let normal_derivative = uv_tangent.and_then(|uv_tangent| {
        let support = model_surface_second_partials_by_id(index, surface, uv.u, uv.v)
            .map_err(|failure| failure.map(|_| ()))?;
        let [du, dv, duu, duv, dvv] = FiniteVector3::raw_array([
            support.du,
            support.dv,
            support.duu,
            support.duv,
            support.dvv,
        ]);
        let du_along = vector_sum(&[(uv_tangent.u, duu), (uv_tangent.v, duv)]);
        let dv_along = vector_sum(&[(uv_tangent.u, duv), (uv_tangent.v, dvv)]);
        let normal = admit_derivative(du.cross(dv))?;
        let normal_derivative = admit_derivative(du_along.cross(dv) + du.cross(dv_along));
        let (_, derivative) = unit_vector_with_derivative(normal, normal_derivative)
            .ok_or(EvaluationFailure::NoValue)?;
        derivative
    });
    Ok(ContactTrack {
        support,
        uv_tangent,
        normal_derivative,
    })
}

fn cacheless_variable_blend_domain_contains(
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: FiniteReal,
    v: FiniteReal,
) -> bool {
    let exact_construction = matches!(
        construction.cache,
        crate::geometry::VariableBlendCache::Parameterization { .. }
            | crate::geometry::VariableBlendCache::Stale {}
    );
    exact_construction
        && (0.0..=1.0).contains(&u.get())
        && sweep_tail_interval_contains(construction.slice_range, v)
        && construction.cache.parameterization().is_none_or(|tail| {
            sweep_tail_interval_contains(tail.u_interval, u)
                && sweep_tail_interval_contains(tail.v_interval, v)
        })
}

/// The finite blend parameters `(u, v)`, or no value where either is not
/// finite.
fn blend_parameters(u: f64, v: f64) -> Result<(FiniteReal, FiniteReal), EvaluationFailure<()>> {
    match (FiniteReal::new(u), FiniteReal::new(v)) {
        (Some(u), Some(v)) => Ok((u, v)),
        _ => Err(EvaluationFailure::NoValue),
    }
}

fn variable_blend_has_current_cache(
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
) -> bool {
    construction.cache.shape_prefix() > 0
        && matches!(
            construction.cache,
            crate::geometry::VariableBlendCache::Current { .. }
        )
}

fn sweep_has_current_cache(
    construction: &crate::geometry::SweepSurfaceConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
) -> bool {
    construction
        .cache
        .form()
        .is_some_and(|form| revision_surface_tail_has_current_cache(&form.cache))
}

fn revision_surface_tail_has_current_cache<P>(
    cache: &crate::geometry::RevisionCacheForm<P>,
) -> bool {
    matches!(
        cache,
        crate::geometry::RevisionCacheForm::SolvedCache { .. }
    )
}

fn variable_blend_is_zero_radius(
    value: &crate::geometry::VariableBlendValue<FiniteReal, FiniteVector3, FinitePoint3>,
) -> bool {
    match &value.payload {
        crate::geometry::VariableBlendValuePayload::TwoEnds {
            parameters: [first_parameter, second_parameter],
            radii: [first_radius, second_radius],
            ..
        } => {
            first_parameter != second_parameter
                && first_radius.get() == 0.0
                && second_radius.get() == 0.0
        }
        crate::geometry::VariableBlendValuePayload::Constant { radius, nested, .. } => {
            radius.get() == 0.0 && variable_blend_is_zero_radius(nested)
        }
        _ => false,
    }
}

/// The two contact tracks of a zero-radius rounded chamfer at `(u, v)`, or
/// why there are none: a blend outside that form or its domain has no
/// value.
fn cacheless_ruled_variable_blend_tracks(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Result<[ContactTrack; 2], EvaluationFailure<()>> {
    let no_value = EvaluationFailure::NoValue;
    let Some(crate::geometry::VariableBlendCrossSection::RoundedChamfer { radius }) =
        construction.cross_section.as_ref()
    else {
        return Err(no_value);
    };
    let (finite_u, finite_v) = blend_parameters(u, v)?;
    if !cacheless_variable_blend_domain_contains(construction, finite_u, finite_v)
        || radius
            .as_deref()
            .is_some_and(|radius| !variable_blend_is_zero_radius(radius))
    {
        return Err(no_value);
    }
    Ok([
        variable_blend_contact_track(index, &construction.sides[0], v)?,
        variable_blend_contact_track(index, &construction.sides[1], v)?,
    ])
}

/// The point of a zero-radius rounded chamfer: its chord between the two
/// contact points at fraction `u`. It reads the contact points only.
fn cacheless_ruled_variable_blend_point(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Result<Point3, EvaluationFailure<()>> {
    let [first, second] = cacheless_ruled_variable_blend_tracks(index, construction, u, v)?;
    Ok(offset(
        first.point(),
        &[(u, point_displacement(second.point(), first.point()))],
    ))
}

/// The point and first partials of a zero-radius rounded chamfer, the first
/// partials reading the track tangents, or why its point has none.
fn cacheless_ruled_variable_blend_first_order(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let [first, second] = cacheless_ruled_variable_blend_tracks(index, construction, u, v)
        .map_err(|failure| failure.map(|()| UNREACHED_POINT))?;
    let chord = point_displacement(second.point(), first.point());
    let point = admit_point(offset(first.point(), &[(u, chord)]))?;
    let tangents = first
        .tangent()
        .and_then(|first| Ok([first, second.tangent()?]));
    Ok(SurfaceFirstOrder {
        point,
        first: tangents
            .map(|[first, second]| [chord, vector_sum(&[(1.0 - u, first), (u, second)])])
            .and_then(admit_lanes),
    })
}

/// A variable blend radius at `parameter`, or why it has none. Two equal
/// end parameters, a radius function without a value there, and a payload
/// the evaluator does not read have no value; an interpolation or radius
/// function that overflows leaves the evaluation outside the finite range.
fn variable_blend_radius(
    value: &crate::geometry::VariableBlendValue<FiniteReal, FiniteVector3, FinitePoint3>,
    parameter: f64,
) -> Result<FiniteReal, EvaluationFailure<()>> {
    match &value.payload {
        crate::geometry::VariableBlendValuePayload::TwoEnds {
            parameters, radii, ..
        } => {
            let [first_parameter, second_parameter] = FiniteReal::raw_array(*parameters);
            let [first_radius, second_radius] = FiniteReal::raw_array(*radii);
            let width = second_parameter - first_parameter;
            if width == 0.0 {
                return Err(EvaluationFailure::NoValue);
            }
            let fraction = (parameter - first_parameter) / width;
            FiniteReal::new(first_radius + fraction * (second_radius - first_radius))
                .ok_or(EvaluationFailure::NonFinite(()))
        }
        crate::geometry::VariableBlendValuePayload::Constant { nested, .. } => {
            variable_blend_radius(nested, parameter)
        }
        crate::geometry::VariableBlendValuePayload::Functional { function, .. }
        | crate::geometry::VariableBlendValuePayload::Interpolated { function, .. } => {
            let [radius, _] = pcurve_uv(function, parameter)
                .map_err(|failure| failure.map(|_| ()))?
                .coordinates();
            Ok(radius)
        }
        _ => Err(EvaluationFailure::NoValue),
    }
}

/// The derivative of a variable blend radius at `parameter`, or why it has
/// none, in the terms of [`variable_blend_radius`].
fn variable_blend_radius_derivative(
    value: &crate::geometry::VariableBlendValue<FiniteReal, FiniteVector3, FinitePoint3>,
    parameter: f64,
) -> Result<FiniteReal, EvaluationFailure<()>> {
    match &value.payload {
        crate::geometry::VariableBlendValuePayload::TwoEnds {
            parameters, radii, ..
        } => {
            let [first_parameter, second_parameter] = FiniteReal::raw_array(*parameters);
            let [first_radius, second_radius] = FiniteReal::raw_array(*radii);
            let width = second_parameter - first_parameter;
            if width == 0.0 {
                return Err(EvaluationFailure::NoValue);
            }
            law_real((second_radius - first_radius) / width)
        }
        crate::geometry::VariableBlendValuePayload::Constant { nested, .. } => {
            variable_blend_radius_derivative(nested, parameter)
        }
        crate::geometry::VariableBlendValuePayload::Functional { function, .. }
        | crate::geometry::VariableBlendValuePayload::Interpolated { function, .. } => {
            let [derivative, _] = pcurve_tangent(function, parameter)
                .map_err(|failure| failure.map(|_| ()))?
                .coordinates();
            Ok(derivative)
        }
        _ => Err(EvaluationFailure::NoValue),
    }
}

/// The point at fraction `u` along the minor arc of radius `radius` about
/// `center` from `first` to `second`. Radii that vanish or are parallel
/// state no arc; a radius outside the finite range leaves the evaluation
/// there.
fn minor_circular_arc_point(
    center: Point3,
    first: Point3,
    second: Point3,
    radius: f64,
    u: f64,
) -> Result<Point3, EvaluationFailure<()>> {
    if u == 0.0 {
        return Ok(first);
    }
    if u == 1.0 {
        return Ok(second);
    }
    let first_radius = unit_direction(Vector3::new(
        first.x - center.x,
        first.y - center.y,
        first.z - center.z,
    ))?;
    let second_radius = unit_direction(Vector3::new(
        second.x - center.x,
        second.y - center.y,
        second.z - center.z,
    ))?;
    let axis = unit_direction(first_radius.cross(second_radius))?;
    let angle = first_radius
        .cross(second_radius)
        .norm()
        .atan2(first_radius.dot(second_radius));
    let section_angle = u * angle;
    let radial = vector_sum(&[
        (section_angle.cos(), first_radius),
        (section_angle.sin(), axis.cross(first_radius)),
    ]);
    Ok(offset(center, &[(radius, radial)]))
}

/// Whether a variable blend takes the circular cross section with a single
/// radius over its domain at `(u, v)`.
fn circular_variable_blend_applies(
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: FiniteReal,
    v: FiniteReal,
) -> bool {
    cacheless_variable_blend_domain_contains(construction, u, v)
        && construction.radii.is_single()
        && matches!(
            construction.cross_section,
            None | Some(crate::geometry::VariableBlendCrossSection::Circular {})
        )
}

/// The two contact tracks of a circular variable blend at `(u, v)`, or why
/// there are none: a blend outside that form or its domain has no value.
fn circular_variable_blend_tracks(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Result<[ContactTrack; 2], EvaluationFailure<()>> {
    let (finite_u, finite_v) = blend_parameters(u, v)?;
    if !circular_variable_blend_applies(construction, finite_u, finite_v) {
        return Err(EvaluationFailure::NoValue);
    }
    Ok([
        variable_blend_contact_track(index, &construction.sides[0], v)?,
        variable_blend_contact_track(index, &construction.sides[1], v)?,
    ])
}

fn cacheless_circular_variable_blend_point(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Result<Point3, EvaluationFailure<()>> {
    let tracks = circular_variable_blend_tracks(index, construction, u, v)?;
    if u == 0.0 {
        return Ok(tracks[0].point());
    }
    if u == 1.0 {
        return Ok(tracks[1].point());
    }
    let section = cacheless_circular_variable_blend_section(index, construction, v, tracks)?;
    minor_circular_arc_point(
        section.center,
        section.first.point(),
        section.second.point(),
        section.radius,
        u,
    )
}

struct CircularVariableBlendSection {
    center: Point3,
    signs: [f64; 2],
    first: ContactTrack,
    second: ContactTrack,
    normals: [Vector3; 2],
    radius: f64,
    radius_derivative: Result<f64, EvaluationFailure<()>>,
    tolerance: f64,
}

/// The circular section of a variable blend at `v` over its contact tracks:
/// the common center of the two normal offsets by the radius, or why there
/// is none. The section reads the contact points and normals and the radius;
/// the radius derivative states its own outcome.
fn cacheless_circular_variable_blend_section(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    v: f64,
    [first, second]: [ContactTrack; 2],
) -> Result<CircularVariableBlendSection, EvaluationFailure<()>> {
    let no_value = EvaluationFailure::NoValue;
    let signed_radius = variable_blend_radius(construction.radii.first(), v)?.get();
    let radius = signed_radius.abs();
    if radius <= f64::EPSILON {
        return Err(no_value);
    }
    let radius_derivative = variable_blend_radius_derivative(construction.radii.first(), v)
        .map(|derivative| derivative.get() * signed_radius.signum());
    let normals = [first.normal()?, second.normal()?];
    let [first_point, second_point] = [first.point(), second.point()];
    let scale = radius
        .max(first_point.x.abs())
        .max(first_point.y.abs())
        .max(first_point.z.abs())
        .max(second_point.x.abs())
        .max(second_point.y.abs())
        .max(second_point.z.abs());
    let tolerance = index
        .ir()
        .tolerances
        .linear
        .get()
        .max(256.0 * f64::EPSILON * scale.max(1.0));

    let candidate = |first_sign: f64, second_sign: f64| {
        let first_center = offset(first_point, &[(first_sign * radius, normals[0])]);
        let second_center = offset(second_point, &[(second_sign * radius, normals[1])]);
        let residual = point_displacement(second_center, first_center).norm();
        residual.is_finite().then(|| {
            (
                Point3::new(
                    first_center.x.midpoint(second_center.x),
                    first_center.y.midpoint(second_center.y),
                    first_center.z.midpoint(second_center.z),
                ),
                [first_sign, second_sign],
                residual,
            )
        })
    };
    let mut best: Option<(Point3, [f64; 2], f64)> = None;
    let mut second_best_residual = f64::INFINITY;
    for (first_sign, second_sign) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
        let Some(next) = candidate(first_sign, second_sign) else {
            continue;
        };
        match best {
            Some(current) if next.2 < current.2 => {
                second_best_residual = current.2;
                best = Some(next);
            }
            Some(_) => second_best_residual = second_best_residual.min(next.2),
            None => best = Some(next),
        }
    }
    let (center, signs, residual) = best.ok_or(no_value)?;
    if residual > tolerance || second_best_residual <= tolerance {
        return Err(no_value);
    }
    Ok(CircularVariableBlendSection {
        center,
        signs,
        first,
        second,
        normals,
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
    native: &crate::geometry::RollingBallConstruction<FiniteReal, FiniteVector3, FinitePoint3>,
    native_ranges: [crate::geometry::surface_payloads::OrderedOptionalRange; 2],
    u: f64,
    v: f64,
) -> Result<Point3, EvaluationFailure<()>> {
    let section = cacheless_constant_rolling_ball_section(
        index,
        supports,
        radius,
        cross_section,
        native,
        native_ranges,
        u,
        v,
    )?;
    minor_circular_arc_point(
        section.center,
        section.first.point(),
        section.second.point(),
        section.radius,
        u,
    )
}

struct ConstantRollingBallSection {
    center: Point3,
    center_tangent: Result<Vector3, EvaluationFailure<()>>,
    first: ContactTrack,
    second: ContactTrack,
    radius: f64,
}

/// The section of a constant rolling ball at `(u, v)`, or why there is none:
/// its spine point as the center of the two contact points. The section
/// reads the contact points and the spine point; the spine tangent states
/// its own outcome.
fn cacheless_constant_rolling_ball_section(
    index: &crate::index::ModelIndex<'_>,
    supports: &[Option<crate::geometry::BlendSupport>; 2],
    radius: &crate::geometry::BlendRadiusLaw,
    cross_section: &crate::geometry::BlendCrossSection,
    native: &crate::geometry::RollingBallConstruction<FiniteReal, FiniteVector3, FinitePoint3>,
    native_ranges: [crate::geometry::surface_payloads::OrderedOptionalRange; 2],
    u: f64,
    v: f64,
) -> Result<ConstantRollingBallSection, EvaluationFailure<()>> {
    let no_value = EvaluationFailure::NoValue;
    let crate::geometry::BlendRadiusLaw::Constant { signed_radius } = radius else {
        return Err(no_value);
    };
    let signed_radius = signed_radius.get();
    let (finite_u, finite_v) = blend_parameters(u, v)?;
    if !matches!(
        native.cache,
        crate::geometry::RevisionCacheForm::Parameterization(_)
    ) || native.third.is_some()
        || *cross_section != crate::geometry::BlendCrossSection::Circular
        || !(0.0..=1.0).contains(&u)
        || !sweep_tail_interval_contains(native.slice_range, finite_v)
        || !sweep_tail_interval_contains(native_ranges[0].endpoints(), finite_u)
        || !sweep_tail_interval_contains(native_ranges[1].endpoints(), finite_v)
        || !native.cache.parameterization().is_some_and(|tail| {
            sweep_tail_interval_contains(tail.u_interval, finite_u)
                && sweep_tail_interval_contains(tail.v_interval, finite_v)
        })
    {
        return Err(no_value);
    }
    let radius = signed_radius.abs();
    if radius <= f64::EPSILON {
        return Err(no_value);
    }
    for (support, side) in supports.iter().zip(native.sides.iter()) {
        if support.as_ref().is_some_and(|support| {
            side.surface
                .as_ref()
                .is_some_and(|surface| surface.surface != support.surface)
        }) {
            return Err(no_value);
        }
    }
    let first = variable_blend_contact_track(index, &native.sides[0], v)?;
    let second = variable_blend_contact_track(index, &native.sides[1], v)?;
    let center =
        model_curve_point_by_id(index, &native.slice, v).map_err(|failure| failure.map(|_| ()))?;
    let center_tangent = model_curve_differential_by_id(index, &native.slice, v)
        .map_err(|failure| failure.map(|_| ()))
        .and_then(|differential| differential.tangent)
        .map(FiniteVector3::get);
    let [first_point, second_point] = [first.point(), second.point()];
    let tolerance = index.ir().tolerances.linear.get().max(
        256.0
            * f64::EPSILON
            * radius
                .max(first_point.x.abs())
                .max(first_point.y.abs())
                .max(first_point.z.abs())
                .max(second_point.x.abs())
                .max(second_point.y.abs())
                .max(second_point.z.abs())
                .max(1.0),
    );
    let radius_error = |point: Point3| {
        (Vector3::new(point.x - center.x, point.y - center.y, point.z - center.z).norm() - radius)
            .abs()
    };
    if native
        .offsets
        .iter()
        .any(|offset| (offset.get() - signed_radius).abs() > tolerance)
        || radius_error(first_point) > tolerance
        || radius_error(second_point) > tolerance
    {
        return Err(no_value);
    }
    Ok(ConstantRollingBallSection {
        center: center.get(),
        center_tangent,
        first,
        second,
        radius,
    })
}

/// The point and first partials of a circular variable blend, the first
/// partials reading the radius derivative, the normal derivatives and the
/// tangents of both tracks, or why its point has none.
fn cacheless_circular_variable_blend_first_order(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let unreached = |failure: EvaluationFailure<()>| failure.map(|()| UNREACHED_POINT);
    let tracks = circular_variable_blend_tracks(index, construction, u, v).map_err(unreached)?;
    let section = cacheless_circular_variable_blend_section(index, construction, v, tracks)
        .map_err(unreached)?;
    let center_tangent = (|| {
        let radius_derivative = section.radius_derivative?;
        let center_tangent = |track: &ContactTrack, sign: f64, normal: Vector3| {
            Ok(vector_sum(&[
                (1.0, track.tangent()?),
                (sign * radius_derivative, normal),
                (sign * section.radius, track.normal_derivative?),
            ]))
        };
        let first_center_tangent =
            center_tangent(&section.first, section.signs[0], section.normals[0])?;
        let second_center_tangent =
            center_tangent(&section.second, section.signs[1], section.normals[1])?;
        if vector_sum(&[(1.0, second_center_tangent), (-1.0, first_center_tangent)]).norm()
            > section.tolerance
        {
            return Err(EvaluationFailure::NoValue);
        }
        Ok(scale_vector(
            vector_sum(&[(1.0, first_center_tangent), (1.0, second_center_tangent)]),
            0.5,
        ))
    })();
    circular_arc_first_order(
        section.center,
        center_tangent,
        &section.first,
        &section.second,
        section.radius,
        section.radius_derivative,
        u,
    )
}

fn cacheless_constant_rolling_ball_first_order(
    index: &crate::index::ModelIndex<'_>,
    supports: &[Option<crate::geometry::BlendSupport>; 2],
    radius: &crate::geometry::BlendRadiusLaw,
    cross_section: &crate::geometry::BlendCrossSection,
    native: &crate::geometry::RollingBallConstruction<FiniteReal, FiniteVector3, FinitePoint3>,
    native_ranges: [crate::geometry::surface_payloads::OrderedOptionalRange; 2],
    u: f64,
    v: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let section = cacheless_constant_rolling_ball_section(
        index,
        supports,
        radius,
        cross_section,
        native,
        native_ranges,
        u,
        v,
    )
    .map_err(|failure| failure.map(|()| UNREACHED_POINT))?;
    constant_rolling_ball_first_order(&section, u)
}

fn constant_rolling_ball_first_order(
    section: &ConstantRollingBallSection,
    u: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    circular_arc_first_order(
        section.center,
        section.center_tangent,
        &section.first,
        &section.second,
        section.radius,
        Ok(0.0),
        u,
    )
}

/// The point at fraction `u` of the circular arc about `center` from the
/// first contact point to the second, with the arc's first partials, or why
/// its point has none. Contact points whose offsets from the center overflow
/// reach no coordinate; offsets that vanish or are parallel state no arc.
/// The first partials read the center tangent, the track tangents and the
/// radius derivative.
fn circular_arc_first_order(
    center: Point3,
    center_tangent: Result<Vector3, EvaluationFailure<()>>,
    first: &ContactTrack,
    second: &ContactTrack,
    radius: f64,
    radius_derivative: Result<f64, EvaluationFailure<()>>,
    u: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let unreached = |failure: EvaluationFailure<()>| failure.map(|()| UNREACHED_POINT);
    let delta_derivative = |track: &ContactTrack| {
        let center_tangent = center_tangent?;
        admit_derivative(vector_sum(&[
            (1.0, track.tangent()?),
            (-1.0, center_tangent),
        ]))
    };
    let radius_direction = |track: &ContactTrack| {
        let delta = admit_derivative(point_displacement(track.point(), center))?;
        unit_vector_with_derivative(delta, delta_derivative(track))
            .ok_or(EvaluationFailure::NoValue)
    };
    let (first_radius, first_radius_v) = radius_direction(first).map_err(unreached)?;
    let (second_radius, second_radius_v) = radius_direction(second).map_err(unreached)?;
    let first_unit = UnitVector3::new(first_radius).ok_or(EvaluationFailure::NoValue)?;
    let second_unit = UnitVector3::new(second_radius).ok_or(EvaluationFailure::NoValue)?;
    let cosine = UnitCosine::between(first_unit, second_unit).get();
    let cross = first_unit.finite_cross(second_unit);
    let sine = cross.get().norm();
    let axis = cross.unit_nonzero().ok_or(EvaluationFailure::NoValue)?;
    let angle = sine.atan2(cosine);
    let transverse = axis.cross(first_radius);
    let (section_sine, section_cosine) = (u * angle).sin_cos();
    let radial = vector_sum(&[(section_cosine, first_radius), (section_sine, transverse)]);
    let angular_direction =
        vector_sum(&[(-section_sine, first_radius), (section_cosine, transverse)]);
    let point = admit_point(offset(center, &[(radius, radial)]))?;
    let first_order = (|| {
        let first_radius_v = first_radius_v?;
        let second_radius_v = second_radius_v?;
        let center_tangent = center_tangent?;
        let radius_derivative = radius_derivative?;
        let cosine_v = first_radius_v.dot(second_radius) + first_radius.dot(second_radius_v);
        let cross_v = vector_sum(&[
            (1.0, first_radius_v.cross(second_radius)),
            (1.0, first_radius.cross(second_radius_v)),
        ]);
        let sine_v = axis.dot(cross_v);
        let angle_v = cosine * sine_v - sine * cosine_v;
        let axis_v = vector_sum(&[(1.0, cross_v), (-sine_v, axis)]).scale(1.0 / sine);
        let transverse_v = vector_sum(&[
            (1.0, axis_v.cross(first_radius)),
            (1.0, axis.cross(first_radius_v)),
        ]);
        let radial_v = vector_sum(&[
            (section_cosine, first_radius_v),
            (section_sine, transverse_v),
            (u * angle_v, angular_direction),
        ]);
        admit_lanes([
            angular_direction.scale(radius * angle),
            vector_sum(&[
                (1.0, center_tangent),
                (radius_derivative, radial),
                (radius, radial_v),
            ]),
        ])
    })();
    Ok(SurfaceFirstOrder {
        point,
        first: first_order,
    })
}

/// The point of a variable blend: the zero-radius rounded chamfer's, or the
/// circular section's.
fn cacheless_variable_blend_point(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Result<Point3, EvaluationFailure<Point3>> {
    match cacheless_ruled_variable_blend_point(index, construction, u, v) {
        Ok(point) => Ok(point),
        // The routes need different cross sections, so at most one reaches
        // past its structure: its failure outside the finite range is the
        // evaluation's.
        Err(ruled) => cacheless_circular_variable_blend_point(index, construction, u, v)
            .map_err(|circular| match ruled {
                EvaluationFailure::NonFinite(()) => ruled,
                EvaluationFailure::NoValue => circular,
            })
            .map_err(|failure| failure.map(|()| UNREACHED_POINT)),
    }
}

/// The point and first partials of a variable blend: the zero-radius rounded
/// chamfer's, or the circular section's.
fn cacheless_variable_blend_first_order(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    match cacheless_ruled_variable_blend_first_order(index, construction, u, v) {
        Ok(order) => Ok(order),
        // The routes need different cross sections, so at most one reaches
        // past its structure: its failure outside the finite range is the
        // evaluation's.
        Err(ruled) => cacheless_circular_variable_blend_first_order(index, construction, u, v)
            .map_err(|circular| match ruled {
                EvaluationFailure::NonFinite(_) => ruled,
                EvaluationFailure::NoValue => circular,
            }),
    }
}

/// The jet of a ruled surface between two model curves, or why its point
/// has none. The point reads both curve points only; the first partials read
/// both tangents, and the second both accelerations as well.
fn model_ruled_surface_jet(
    index: &crate::index::ModelIndex<'_>,
    first: &crate::ids::CurveId,
    second: &crate::ids::CurveId,
    u: f64,
    v: f64,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    if !v.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let rule =
        |first: Point3, second: Point3| offset(first, &[(v, point_displacement(second, first))]);
    let (first, second) = curve_pair(
        model_curve_differential_by_id(index, first, u),
        model_curve_differential_by_id(index, second, u),
        rule,
    )?;
    let point = admit_point(rule(first.point.get(), second.point.get()))?;
    let blend = |first: Vector3, second: Vector3| vector_sum(&[(1.0 - v, first), (v, second)]);
    let tangents = first
        .tangent
        .and_then(|first| Ok([first.get(), second.tangent?.get()]));
    Ok(SurfaceJet {
        point,
        first: tangents.and_then(|[first_tangent, second_tangent]| {
            admit_lanes([
                blend(first_tangent, second_tangent),
                point_displacement(second.point.get(), first.point.get()),
            ])
        }),
        second: tangents.and_then(|[first_tangent, second_tangent]| {
            let [acceleration, twist] = admit_lanes([
                blend(first.acceleration?.get(), second.acceleration?.get()),
                vector_sum(&[(-1.0, first_tangent), (1.0, second_tangent)]),
            ])?;
            Ok([acceleration, twist, FiniteVector3::ZERO])
        }),
    })
}

/// The jet of a sum surface of two model curves, or why its point has none.
/// The point reads both curve points only; the first partials read both
/// tangents, and the second both accelerations.
fn model_sum_surface_jet(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::SumSurfaceConstruction,
    u: f64,
    v: f64,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    let basepoint = *construction.basepoint();
    let sum = |first: Point3, second: Point3| {
        Point3::new(
            first.x + second.x - basepoint.x,
            first.y + second.y - basepoint.y,
            first.z + second.z - basepoint.z,
        )
    };
    let (first, second) = curve_pair(
        model_curve_differential_by_id(index, construction.first(), u),
        model_curve_differential_by_id(index, construction.second(), v),
        sum,
    )?;
    let point = admit_point(sum(first.point.get(), second.point.get()))?;
    Ok(SurfaceJet {
        point,
        first: first.tangent.and_then(|first| Ok([first, second.tangent?])),
        second: first
            .acceleration
            .and_then(|first| Ok([first, FiniteVector3::ZERO, second.acceleration?])),
    })
}

/// The two curve differentials a surface arm combines. A curve with no value
/// leaves the surface without one; otherwise a curve outside the finite
/// range leaves the surface there, at the point `combine` reaches from the
/// two curve points.
fn curve_pair(
    first: Result<ModelCurveDifferential, EvaluationFailure<Point3>>,
    second: Result<ModelCurveDifferential, EvaluationFailure<Point3>>,
    combine: impl Fn(Point3, Point3) -> Point3,
) -> Result<(ModelCurveDifferential, ModelCurveDifferential), EvaluationFailure<Point3>> {
    let reached = |side: Result<ModelCurveDifferential, EvaluationFailure<Point3>>| match side {
        Ok(differential) => Some(differential.point.get()),
        Err(failure) => failure.non_finite(),
    };
    match (first, second) {
        (Ok(first), Ok(second)) => Ok((first, second)),
        (first, second) => Err(match (reached(first), reached(second)) {
            (Some(first), Some(second)) => EvaluationFailure::NonFinite(combine(first, second)),
            _ => EvaluationFailure::NoValue,
        }),
    }
}

/// The descent is bounded by [`PlacedSurface`](crate::geometry::PlacedSurface)
/// construction; no arm follows an arena id.
fn model_surface_point_with_budget_solved(
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    match (geometry, budget) {
        (SolvedSurfaceGeometry::Nurbs(nurbs), Some(budget)) => {
            nurbs_surface_point_with_budget(nurbs, u, v, budget)
        }
        (SolvedSurfaceGeometry::Transformed(placed), Some(budget)) => {
            if !budget.charge() {
                return Err(EvaluationFailure::NoValue);
            }
            placed_point(
                *placed.transform(),
                model_surface_point_with_budget_solved(placed.basis(), u, v, Some(budget)),
            )
        }
        _ => surface_point_solved(geometry, u, v),
    }
}

/// Evaluate a surface carrier selected by arena id.
pub fn model_surface_point_by_id(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    model_surface_point_by_id_inner(index, surface, u, v, None)
}

/// Evaluate a surface carrier selected by arena id within a caller-owned work
/// slice. Surface-carrier recursion and NURBS evaluation both consume the
/// supplied budget.
pub fn model_surface_point_by_id_with_budget(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
    budget: &WorkBudget<'_>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let _guard = budget.recursion_guard().ok_or(EvaluationFailure::NoValue)?;
    let point = model_surface_point_by_id_inner(index, surface, u, v, Some(budget));
    ModelEvaluationDepthGuard::finish_budgeted(budget, point)
}

fn model_surface_point_by_id_inner(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    /// An arm's point, admitted where the arm computes it raw, and the
    /// support's oriented unit normal for an offset that reads it.
    struct SurfaceEvaluation {
        /// The point, or the non-finite point the arm reached.
        point: Result<FinitePoint3, Point3>,
        /// The oriented unit normal, or why it has none: an arm that forms
        /// no normal, and an evaluation whose reader reads none, have no
        /// value.
        oriented_normal: Result<Vector3, EvaluationFailure<()>>,
    }

    /// Admit a point an arm computes raw, keeping the non-finite point.
    fn evaluated(point: Point3) -> Result<FinitePoint3, Point3> {
        FinitePoint3::new(point).ok_or(point)
    }

    /// The point of an evaluation, finite or not.
    fn reached(point: Result<FinitePoint3, Point3>) -> Point3 {
        point.map_or_else(|point| point, FinitePoint3::get)
    }

    /// The evaluation of an arm that forms its point alone.
    fn point_evaluation(
        point: Result<FinitePoint3, EvaluationFailure<Point3>>,
    ) -> Option<SurfaceEvaluation> {
        let point = match point {
            Ok(point) => Ok(point),
            Err(failure) => Err(failure.non_finite()?),
        };
        Some(SurfaceEvaluation {
            point,
            oriented_normal: Err(EvaluationFailure::NoValue),
        })
    }

    /// The evaluation of a construction that falls back to its current
    /// cache: the cache's point where it has one, otherwise the construction's
    /// point outside the finite range, otherwise the cache's point outside the
    /// finite range.
    fn cache_fallback(
        construction: EvaluationFailure<Point3>,
        cached: Option<SurfaceEvaluation>,
    ) -> Option<SurfaceEvaluation> {
        match (cached, construction) {
            (Some(cached), _) if cached.point.is_ok() => Some(cached),
            (_, EvaluationFailure::NonFinite(point)) => Some(SurfaceEvaluation {
                point: Err(point),
                oriented_normal: Err(EvaluationFailure::NonFinite(())),
            }),
            (cached, EvaluationFailure::NoValue) => cached,
        }
    }

    /// The oriented unit normal of first partials: their cross product,
    /// reversed where `reversed` is set. A normal whose direction is zero
    /// has no value.
    fn oriented_normal(
        first: Result<[FiniteVector3; 2], EvaluationFailure<()>>,
        reversed: bool,
    ) -> Result<Vector3, EvaluationFailure<()>> {
        let [du, dv] = first?;
        let cross = du.get().cross(dv.get());
        let magnitude = cross.norm();
        let normal = if magnitude.is_finite() && magnitude > 0.0 {
            Vector3::new(
                cross.x / magnitude,
                cross.y / magnitude,
                cross.z / magnitude,
            )
        } else {
            unit_cross_direction(du, dv)?
        };
        Ok(if reversed {
            scale_vector(normal, -1.0)
        } else {
            normal
        })
    }

    /// A stored cache's point and unit normal. The point is evaluated alone;
    /// the normal is that of the cache's first partials.
    fn cache_evaluation(geometry: &SurfaceGeometry, u: f64, v: f64) -> Option<SurfaceEvaluation> {
        let point = match surface_point(geometry, u, v) {
            Ok(point) => Ok(point),
            Err(failure) => Err(failure.non_finite()?),
        };
        Some(SurfaceEvaluation {
            point,
            oriented_normal: surface_first_order(geometry, u, v, None)
                .map_err(|failure| failure.map(|_| ()))
                .and_then(|order| {
                    let [du, dv] = order.first?;
                    du.get()
                        .cross(dv.get())
                        .unit()
                        .ok_or(EvaluationFailure::NoValue)
                }),
        })
    }

    /// A directly stored carrier's point and, for a reader that reads it,
    /// its oriented unit normal. The point is evaluated alone where no normal
    /// is read; otherwise it is the point of the first partials, which leave
    /// the point finite where they leave the finite range.
    fn direct_evaluation(
        geometry: &SurfaceGeometry,
        u: f64,
        v: f64,
        budget: Option<&WorkBudget<'_>>,
        normal: bool,
    ) -> Option<SurfaceEvaluation> {
        if !normal {
            return point_evaluation(match budget {
                Some(budget) => surface_point_with_budget(geometry, u, v, budget),
                None => surface_point(geometry, u, v),
            });
        }
        let order = match surface_first_order(geometry, u, v, budget) {
            Ok(order) => order,
            Err(failure) => {
                return Some(SurfaceEvaluation {
                    point: Err(failure.non_finite()?),
                    oriented_normal: Err(EvaluationFailure::NonFinite(())),
                })
            }
        };
        let reversed = matches!(
            geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) if nurbs.normal_reversed()
        );
        Some(SurfaceEvaluation {
            point: Ok(order.point),
            oriented_normal: oriented_normal(order.first, reversed),
        })
    }

    /// The offset of a support whose normal has no value: a support point
    /// outside the finite range, and a normal outside it, reach no offset
    /// coordinate; a finite support point without a normal has no offset.
    fn offset_without_normal(
        support: &SurfaceEvaluation,
        failure: EvaluationFailure<()>,
    ) -> Option<SurfaceEvaluation> {
        (support.point.is_err() || failure == EvaluationFailure::NonFinite(())).then_some(
            SurfaceEvaluation {
                point: Err(Point3::new(f64::NAN, f64::NAN, f64::NAN)),
                oriented_normal: Err(EvaluationFailure::NonFinite(())),
            },
        )
    }

    fn linear_nurbs_support_extension(
        index: &crate::index::ModelIndex<'_>,
        support: &crate::ids::SurfaceId,
        u: f64,
        v: f64,
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<SurfaceEvaluation> {
        let support = index.surfaces(support.as_str())?;
        let Some(SolvedSurfaceGeometry::Nurbs(nurbs)) = support.geometry.solved() else {
            return None;
        };
        let u_degree = usize::try_from(nurbs.u_degree()).ok()?;
        let v_degree = usize::try_from(nurbs.v_degree()).ok()?;
        let u_count = nurbs.u_count();
        let v_count = nurbs.v_count();
        let u_domain = [
            *nurbs.u_knots().get(u_degree)?,
            *nurbs.u_knots().get(u_count)?,
        ];
        let v_domain = [
            *nurbs.v_knots().get(v_degree)?,
            *nurbs.v_knots().get(v_count)?,
        ];
        let boundary = |value: f64, [lower, upper]: [f64; 2]| {
            if !value.is_finite() || !lower.is_finite() || !upper.is_finite() || lower > upper {
                return None;
            }
            Some(if value < lower {
                (lower, true)
            } else if value > upper {
                (upper, true)
            } else {
                (value, false)
            })
        };
        let (boundary_u, u_extended) = boundary(u, u_domain)?;
        let (boundary_v, v_extended) = boundary(v, v_domain)?;
        if !u_extended && !v_extended {
            return None;
        }
        let partials = surface_first_order(&support.geometry, boundary_u, boundary_v, budget)
            .ok()?
            .partials()
            .ok()?;
        let oriented_normal =
            oriented_normal(Ok([partials.du, partials.dv]), nurbs.normal_reversed()).ok()?;
        let partials = partials.into_raw();
        let du = u - boundary_u;
        let dv = v - boundary_v;
        Some(SurfaceEvaluation {
            point: evaluated(Point3::new(
                partials.point.x + du * partials.du.x + dv * partials.dv.x,
                partials.point.y + du * partials.du.y + dv * partials.dv.y,
                partials.point.z + du * partials.du.z + dv * partials.dv.z,
            )),
            oriented_normal: Ok(oriented_normal),
        })
    }

    /// The evaluation of `surface_id` at `(u, v)`, with the oriented normal
    /// where `normal` is set: an offset reads its support's normal, and a
    /// placement or subset passes its reader's need on to its support.
    fn evaluate(
        index: &crate::index::ModelIndex<'_>,
        surface_id: &crate::ids::SurfaceId,
        u: f64,
        v: f64,
        visiting: &mut Vec<crate::ids::SurfaceId>,
        budget: Option<&WorkBudget<'_>>,
        normal: bool,
    ) -> Option<SurfaceEvaluation> {
        let _depth = ModelEvaluationDepthGuard::enter(budget)?;
        if let Some(budget) = budget {
            budget.charge().then_some(())?;
        }
        if visiting.contains(surface_id) {
            return None;
        }
        visiting.push(surface_id.clone());
        let surface = index.surfaces(surface_id.as_str())?;
        let procedural = index.procedural_surface_for_surface(surface_id.as_str());
        let carrier_interval =
            procedural.and_then(|procedural| record_u_interval(procedural.record_bounds()));
        let result = match procedural.map(crate::geometry::ProceduralSurface::definition) {
            Some(ProceduralSurfaceDefinition::AxisRevolution(definition_payload)) => {
                point_evaluation(model_axis_revolution_point(
                    index,
                    definition_payload.directrix(),
                    definition_payload.axis_origin().get(),
                    definition_payload.axis_direction(),
                    u,
                    v,
                    budget,
                ))
            }
            Some(ProceduralSurfaceDefinition::Extrusion(definition_payload)) => {
                point_evaluation(model_native_extrusion_point(
                    index,
                    definition_payload,
                    carrier_interval,
                    u,
                    v,
                    budget,
                ))
            }
            Some(ProceduralSurfaceDefinition::LinearSweep(definition_payload)) => point_evaluation(
                model_linear_sweep_point(index, definition_payload, u, v, budget),
            ),
            Some(ProceduralSurfaceDefinition::Revolution(definition_payload)) => {
                point_evaluation(model_native_revolution_point(
                    index,
                    definition_payload,
                    carrier_interval,
                    u,
                    v,
                    budget,
                ))
            }
            Some(ProceduralSurfaceDefinition::Ruled { first, second, .. }) => point_evaluation(
                model_ruled_surface_jet(index, first, second, u, v).map(|jet| jet.point),
            ),
            Some(ProceduralSurfaceDefinition::Sum(definition_payload)) => point_evaluation(
                model_sum_surface_jet(index, definition_payload, u, v).map(|jet| jet.point),
            ),
            Some(ProceduralSurfaceDefinition::Sweep(definition_payload)) => {
                if let Some(construction) = definition_payload.native() {
                    let profile = definition_payload.profile();
                    let spine = definition_payload.spine();

                    match cacheless_law_sweep_point(index, profile, spine, construction, u, v) {
                        Ok(point) => Some(SurfaceEvaluation {
                            point: evaluated(point),
                            oriented_normal: Err(EvaluationFailure::NoValue),
                        }),
                        Err(failure) => cache_fallback(
                            failure,
                            sweep_has_current_cache(construction)
                                .then(|| cache_evaluation(&surface.geometry, u, v))
                                .flatten(),
                        ),
                    }
                } else {
                    direct_evaluation(&surface.geometry, u, v, budget, normal)
                }
            }
            Some(ProceduralSurfaceDefinition::VariableBlend(definition_payload)) => {
                let construction = definition_payload.construction();

                match cacheless_variable_blend_point(index, construction, u, v) {
                    Ok(point) => Some(SurfaceEvaluation {
                        point: evaluated(point),
                        oriented_normal: Err(EvaluationFailure::NoValue),
                    }),
                    Err(failure) => cache_fallback(
                        failure,
                        variable_blend_has_current_cache(construction)
                            .then(|| cache_evaluation(&surface.geometry, u, v))
                            .flatten(),
                    ),
                }
            }
            Some(ProceduralSurfaceDefinition::Blend(definition_payload)) => {
                if let Some((native, native_ranges)) = definition_payload.native_with_ranges() {
                    let supports = definition_payload.supports();
                    let radius = definition_payload.radius();
                    let cross_section = definition_payload.cross_section();

                    match cacheless_constant_rolling_ball_point(
                        index,
                        supports,
                        radius,
                        cross_section,
                        native,
                        native_ranges,
                        u,
                        v,
                    ) {
                        Ok(point) => Some(SurfaceEvaluation {
                            point: evaluated(point),
                            oriented_normal: if normal {
                                cacheless_constant_rolling_ball_first_order(
                                    index,
                                    supports,
                                    radius,
                                    cross_section,
                                    native,
                                    native_ranges,
                                    u,
                                    v,
                                )
                                .map_err(|failure| failure.map(|_| ()))
                                .and_then(|order| {
                                    let [du, dv] = order.first?;
                                    du.get()
                                        .cross(dv.get())
                                        .unit()
                                        .ok_or(EvaluationFailure::NoValue)
                                })
                            } else {
                                Err(EvaluationFailure::NoValue)
                            },
                        }),
                        Err(failure) => cache_fallback(
                            failure.map(|()| UNREACHED_POINT),
                            revision_surface_tail_has_current_cache(&native.cache)
                                .then(|| cache_evaluation(&surface.geometry, u, v))
                                .flatten(),
                        ),
                    }
                } else {
                    direct_evaluation(&surface.geometry, u, v, budget, normal)
                }
            }
            Some(definition @ ProceduralSurfaceDefinition::RollingBallJet(_)) => {
                point_evaluation(rolling_ball_jet_point(definition, u, v))
            }
            Some(ProceduralSurfaceDefinition::CurveBounded { support, .. }) => {
                evaluate(index, support, u, v, visiting, budget, normal)
            }
            Some(ProceduralSurfaceDefinition::Replica { source, transform }) => {
                let mut evaluation = evaluate(index, source, u, v, visiting, budget, normal)?;
                if normal {
                    let placed_normal = model_surface_jet_by_id(index, source, u, v, budget)
                        .map_err(|failure| failure.map(|_| ()))
                        .and_then(|jet| {
                            let [du, dv] = placed_vectors(*transform, jet.first?)?;
                            du.get()
                                .cross(dv.get())
                                .unit()
                                .ok_or(EvaluationFailure::NoValue)
                        });
                    evaluation.oriented_normal = placed_normal.or_else(|placed_failure| {
                        evaluation
                            .oriented_normal
                            .and_then(|normal| {
                                transform
                                    .apply_normal(normal)
                                    .ok_or(EvaluationFailure::NonFinite(()))
                            })
                            .and_then(|normal| {
                                Ok(scale_vector(
                                    *normal.as_raw(),
                                    transform.orientation().ok_or(EvaluationFailure::NoValue)?,
                                ))
                            })
                            .map_err(|evaluation_failure| match placed_failure {
                                EvaluationFailure::NonFinite(()) => placed_failure,
                                EvaluationFailure::NoValue => evaluation_failure,
                            })
                    });
                }
                evaluation.point = match evaluation.point {
                    Ok(point) => transform.apply_point_reaching(point.get()),
                    // The placement of a point outside the finite range stays
                    // outside it, whatever point the placement reaches.
                    Err(point) => Err(placed_reach(*transform, point)),
                };
                Some(evaluation)
            }
            Some(ProceduralSurfaceDefinition::Subset(definition_payload)) => {
                let support = definition_payload.support();
                let parameter_ranges = definition_payload
                    .parameter_ranges()
                    .map(crate::geometry::DirectedParameterRange::finite_endpoints);
                let u_sense = definition_payload.u_sense();
                let v_sense = definition_payload.v_sense();
                {
                    let (support_u, support_v, u_derivative, v_derivative) =
                        subset_support_parameters_with_derivatives(
                            u,
                            v,
                            parameter_ranges,
                            *u_sense,
                            *v_sense,
                        )?;
                    let mut evaluation = evaluate(
                        index, support, support_u, support_v, visiting, budget, normal,
                    )?;
                    if u_derivative * v_derivative < 0.0 {
                        evaluation.oriented_normal = evaluation
                            .oriented_normal
                            .map(|normal| scale_vector(normal, -1.0));
                    }
                    Some(evaluation)
                }
            }
            Some(ProceduralSurfaceDefinition::ParallelOffset(definition_payload)) => {
                let support = definition_payload.support();
                let distance = definition_payload.distance();
                {
                    let support = evaluate(index, support, u, v, visiting, budget, true)?;
                    match support.oriented_normal {
                        Ok(normal) => Some(SurfaceEvaluation {
                            point: evaluated(offset(
                                reached(support.point),
                                &[(distance.get(), normal)],
                            )),
                            oriented_normal: Ok(normal),
                        }),
                        Err(failure) => offset_without_normal(&support, failure),
                    }
                }
            }
            Some(ProceduralSurfaceDefinition::Offset(definition_payload)) => {
                let support = definition_payload.support();
                let distance = definition_payload.distance();
                let linear_extension = definition_payload.linear_support_extension();
                {
                    let support = linear_extension
                        .then(|| linear_nurbs_support_extension(index, support, u, v, budget))
                        .flatten()
                        .or_else(|| evaluate(index, support, u, v, visiting, budget, true))?;
                    match support.oriented_normal {
                        Ok(normal) => Some(SurfaceEvaluation {
                            point: evaluated(offset(
                                reached(support.point),
                                &[(distance.get(), normal)],
                            )),
                            oriented_normal: Ok(normal),
                        }),
                        Err(failure) => offset_without_normal(&support, failure),
                    }
                }
            }
            _ if procedural.is_some() => {
                // A non-finite point enters the evaluation as a finite one
                // does.
                point_evaluation(model_surface_point_with_budget(
                    index.ir(),
                    &surface.geometry,
                    u,
                    v,
                    budget,
                ))
            }
            _ => direct_evaluation(&surface.geometry, u, v, budget, normal),
        };
        visiting.pop();
        result
    }

    evaluate(index, surface, u, v, &mut Vec::new(), budget, false)
        .ok_or(EvaluationFailure::NoValue)?
        .point
        .map_err(EvaluationFailure::NonFinite)
}

/// Evaluate an arena-selected direct, trimmed, or uniform-offset surface and
/// its exact first partial derivatives, or report why they have no finite
/// value.
///
/// Subsets map the support parameterization through a linear local domain;
/// offsets follow the support's oriented normal. The recursive carrier walk
/// preserves both contracts before evaluating the final point and partials.
/// The point fails as [`model_surface_point_by_id`]'s arms state; at a finite
/// point, first partials without a value, or outside the finite range, fail
/// the evaluation, carrying the point.
pub fn model_surface_partials_by_id(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Result<SurfacePartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    model_surface_first_order_by_id(index, surface, u, v, None)?.partials()
}

/// The point and first partials of an arena surface, the first partials
/// with their own outcome, or why the point has none.
///
/// A cacheless blend or sweep whose point and first partials both have
/// values is evaluated without its cache, and the budget does not reach it.
/// Otherwise one with a current cache falls back to the cache: the cache's
/// complete evaluation wins, then an evaluation with a point, the cacheless
/// one first; of two failures, the cacheless one outside the finite range
/// wins.
fn model_surface_first_order_by_id(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let _depth = ModelEvaluationDepthGuard::enter(budget).ok_or(EvaluationFailure::NoValue)?;
    let cacheless = match index
        .procedural_surface_for_surface(surface.as_str())
        .map(crate::geometry::ProceduralSurface::definition)
    {
        Some(ProceduralSurfaceDefinition::Blend(definition_payload)) => definition_payload
            .native_with_ranges()
            .map(|(native, native_ranges)| {
                (
                    cacheless_constant_rolling_ball_first_order(
                        index,
                        definition_payload.supports(),
                        definition_payload.radius(),
                        definition_payload.cross_section(),
                        native,
                        native_ranges,
                        u,
                        v,
                    ),
                    revision_surface_tail_has_current_cache(&native.cache),
                )
            }),
        Some(ProceduralSurfaceDefinition::VariableBlend(definition_payload)) => {
            let construction = definition_payload.construction();
            Some((
                cacheless_variable_blend_first_order(index, construction, u, v),
                variable_blend_has_current_cache(construction),
            ))
        }
        Some(ProceduralSurfaceDefinition::Sweep(definition_payload)) => {
            definition_payload.native().as_deref().map(|construction| {
                (
                    cacheless_law_sweep_first_order(
                        index,
                        definition_payload.profile(),
                        definition_payload.spine(),
                        construction,
                        u,
                        v,
                    ),
                    sweep_has_current_cache(construction),
                )
            })
        }
        _ => None,
    };
    let cached =
        || model_surface_jet_by_id(index, surface, u, v, budget).map(SurfaceJet::first_order);
    let complete = |order: &Result<SurfaceFirstOrder, EvaluationFailure<Point3>>| {
        order.as_ref().is_ok_and(|order| order.first.is_ok())
    };
    match cacheless {
        None => cached(),
        Some((cacheless, _)) if complete(&cacheless) => cacheless,
        Some((cacheless, false)) => cacheless,
        Some((cacheless, true)) => {
            let cached = cached();
            if complete(&cached) {
                return cached;
            }
            match (cacheless, cached) {
                (Ok(order), _) | (Err(_), Ok(order)) => Ok(order),
                (Err(cacheless), Err(cached)) => Err(match cacheless {
                    EvaluationFailure::NonFinite(_) => cacheless,
                    EvaluationFailure::NoValue => cached,
                }),
            }
        }
    }
}

/// [`model_surface_partials_by_id`] within a caller-owned work slice.
/// Surface-carrier recursion and NURBS evaluation both consume the supplied
/// budget; a refused charge leaves no value.
pub fn model_surface_partials_by_id_with_budget(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
    budget: &WorkBudget<'_>,
) -> Result<SurfacePartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    let _guard = budget.recursion_guard().ok_or(EvaluationFailure::NoValue)?;
    let partials = model_surface_first_order_by_id(index, surface, u, v, Some(budget))
        .and_then(SurfaceFirstOrder::partials);
    ModelEvaluationDepthGuard::finish_budgeted(budget, partials)
}

fn model_surface_second_partials_by_id(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    model_surface_jet_by_id(index, surface, u, v, None)?.second_partials()
}

/// The point with the first and second partials of an arena surface through
/// its carrier walk, each order with its own outcome, or why the point has
/// none. An offset's point reads its support's first partials and its first
/// partials read the support's second.
fn model_surface_jet_by_id(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    let mapping = model_surface_mapping(index, surface, u, v, &mut Vec::new(), budget)?;
    let jet = if mapping.offset_distance == 0.0 {
        mapping.base
    } else {
        offset_surface_jet(mapping.base, mapping.offset_distance)?
    };
    let reversed = |vector: FiniteVector3, reversed: bool| {
        if reversed {
            vector.negated()
        } else {
            vector
        }
    };
    let [u_reversed, v_reversed] = mapping.reversed;
    Ok(SurfaceJet {
        point: jet.point,
        first: jet
            .first
            .map(|[du, dv]| [reversed(du, u_reversed), reversed(dv, v_reversed)]),
        second: jet
            .second
            .map(|[duu, duv, dvv]| [duu, reversed(duv, u_reversed != v_reversed), dvv]),
    })
}

/// A surface's carrier walk to its direct support: the support's jet at the
/// mapped support coordinates, the offset from it, and the directions the
/// mapping reverses.
#[derive(Clone, Copy)]
struct SurfaceMapping {
    /// The direct support's jet at the mapped support coordinates.
    base: SurfaceJet,
    /// Signed distance from `base` to the evaluated surface.
    offset_distance: f64,
    /// Whether the support's `u` and `v` run against the evaluated `u` and
    /// `v`.
    reversed: [bool; 2],
    /// Support normal orientation relative to the direct base normal.
    orientation: f64,
}

/// The carrier walk of an arena surface to its direct support at `(u, v)`,
/// or why its point has none.
fn model_surface_mapping(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
    visiting: &mut Vec<crate::ids::SurfaceId>,
    budget: Option<&WorkBudget<'_>>,
) -> Result<SurfaceMapping, EvaluationFailure<Point3>> {
    let _depth = ModelEvaluationDepthGuard::enter(budget).ok_or(EvaluationFailure::NoValue)?;
    let no_value = EvaluationFailure::NoValue;
    if budget.is_some_and(|budget| !budget.charge()) {
        return Err(no_value);
    }
    if visiting.contains(surface) {
        return Err(no_value);
    }
    visiting.push(surface.clone());
    let carrier = index.surfaces(surface.as_str()).ok_or(no_value)?;
    let procedural = index.procedural_surface_for_surface(surface.as_str());
    let carrier_interval =
        procedural.and_then(|procedural| record_u_interval(procedural.record_bounds()));
    let direct = |base: SurfaceJet| SurfaceMapping {
        base,
        offset_distance: 0.0,
        reversed: [false, false],
        orientation: 1.0,
    };
    let result = match procedural.map(crate::geometry::ProceduralSurface::definition) {
        Some(ProceduralSurfaceDefinition::AxisRevolution(definition_payload)) => {
            model_axis_revolution_jet(
                index,
                definition_payload.directrix(),
                definition_payload.axis_origin().get(),
                definition_payload.axis_direction(),
                u,
                v,
                budget,
            )
            .map(direct)
        }
        Some(ProceduralSurfaceDefinition::Extrusion(definition_payload)) => {
            model_native_extrusion_jet(index, definition_payload, carrier_interval, u, v, budget)
                .map(direct)
        }
        Some(ProceduralSurfaceDefinition::LinearSweep(definition_payload)) => {
            model_linear_sweep_jet(index, definition_payload, u, v, budget).map(direct)
        }
        Some(ProceduralSurfaceDefinition::Revolution(definition_payload)) => {
            model_native_revolution_jet(index, definition_payload, carrier_interval, u, v, budget)
                .map(direct)
        }
        Some(ProceduralSurfaceDefinition::Ruled { first, second, .. }) => {
            model_ruled_surface_jet(index, first, second, u, v).map(direct)
        }
        Some(ProceduralSurfaceDefinition::Sum(definition_payload)) => {
            model_sum_surface_jet(index, definition_payload, u, v).map(direct)
        }
        Some(ProceduralSurfaceDefinition::CurveBounded { support, .. }) => {
            model_surface_mapping(index, support, u, v, visiting, budget)
        }
        Some(ProceduralSurfaceDefinition::Replica { source, transform }) => {
            model_surface_mapping(index, source, u, v, visiting, budget).and_then(|source| {
                let base = if source.offset_distance == 0.0 {
                    source.base
                } else {
                    offset_surface_jet(source.base, source.offset_distance)?
                };
                Ok(SurfaceMapping {
                    base: placed_jet(*transform, Ok(base))?,
                    offset_distance: 0.0,
                    reversed: source.reversed,
                    orientation: source.orientation * transform.orientation().ok_or(no_value)?,
                })
            })
        }
        Some(ProceduralSurfaceDefinition::Subset(definition_payload)) => {
            let support = definition_payload.support();
            let parameter_ranges = definition_payload
                .parameter_ranges()
                .map(crate::geometry::DirectedParameterRange::finite_endpoints);
            let u_sense = definition_payload.u_sense();
            let v_sense = definition_payload.v_sense();
            subset_support_parameters_with_derivatives(u, v, parameter_ranges, *u_sense, *v_sense)
                .ok_or(no_value)
                .and_then(|(support_u, support_v, u_derivative, v_derivative)| {
                    let support = model_surface_mapping(
                        index, support, support_u, support_v, visiting, budget,
                    )?;
                    let [u_reversed, v_reversed] = support.reversed;
                    Ok(SurfaceMapping {
                        base: support.base,
                        offset_distance: support.offset_distance,
                        reversed: [
                            u_reversed != (u_derivative < 0.0),
                            v_reversed != (v_derivative < 0.0),
                        ],
                        orientation: support.orientation * u_derivative * v_derivative,
                    })
                })
        }
        Some(ProceduralSurfaceDefinition::ParallelOffset(payload)) => {
            model_surface_mapping(index, payload.support(), u, v, visiting, budget).map(|support| {
                SurfaceMapping {
                    offset_distance: support.offset_distance
                        + payload.distance().get() * support.orientation,
                    ..support
                }
            })
        }
        Some(ProceduralSurfaceDefinition::Offset(payload)) => {
            model_surface_mapping(index, payload.support(), u, v, visiting, budget).map(|support| {
                SurfaceMapping {
                    offset_distance: support.offset_distance
                        + payload.distance().get() * support.orientation,
                    ..support
                }
            })
        }
        _ => surface_jet(&carrier.geometry, u, v, budget).map(direct),
    };
    visiting.pop();
    result
}

fn subset_support_parameters_with_derivatives(
    u: f64,
    v: f64,
    parameter_ranges: [[FiniteReal; 2]; 2],
    u_sense: Option<bool>,
    v_sense: Option<bool>,
) -> Option<(f64, f64, f64, f64)> {
    let (support_u, u_derivative) = subset_parameter(parameter_ranges[0], u, u_sense)?;
    let (support_v, v_derivative) = subset_parameter(parameter_ranges[1], v, v_sense)?;
    Some((support_u, support_v, u_derivative, v_derivative))
}

/// Map a subset parameter onto its support: the support parameter and the
/// derivative sign. The range endpoints are finite and distinct, so the span
/// is not zero. A sense against the range direction runs the support
/// parameter away from the range, where it can leave the finite range; the
/// support evaluation then reports that evaluation.
fn subset_parameter(
    range: [FiniteReal; 2],
    parameter: f64,
    sense: Option<bool>,
) -> Option<(f64, f64)> {
    let [start, end] = range.map(FiniteReal::get);
    let span = (end - start).abs();
    if !parameter.is_finite() || parameter < 0.0 || parameter > span {
        return None;
    }
    let agrees = sense.unwrap_or(end >= start);
    let derivative = if agrees { 1.0 } else { -1.0 };
    Some((start + derivative * parameter, derivative))
}

/// The jet of the offset by `distance` along the unit normal of `base`, or
/// why its point has none. The offset point reads the base's first partials,
/// and its first partials read the base's second. A distance or normal
/// outside the finite range reaches no coordinate; a zero normal has no
/// direction. The second partials are the base's.
fn offset_surface_jet(
    base: SurfaceJet,
    distance: f64,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    let unreached = EvaluationFailure::NonFinite(UNREACHED_POINT);
    if !distance.is_finite() {
        return Err(unreached);
    }
    let [du, dv] = FiniteVector3::raw_array(
        base.first
            .map_err(|failure| failure.map(|()| UNREACHED_POINT))?,
    );
    let normal_vector = du.cross(dv);
    let normal_magnitude = normal_vector.norm();
    if !normal_magnitude.is_finite() {
        return Err(unreached);
    }
    if normal_magnitude == 0.0 {
        return Err(EvaluationFailure::NoValue);
    }
    let normal = Vector3::new(
        normal_vector.x / normal_magnitude,
        normal_vector.y / normal_magnitude,
        normal_vector.z / normal_magnitude,
    );
    let base_point = base.point.get();
    let point = admit_point(Point3::new(
        base_point.x + distance * normal.x,
        base_point.y + distance * normal.y,
        base_point.z + distance * normal.z,
    ))?;
    let unit_normal_derivative = |derivative: Vector3| {
        let normal_component =
            normal.x * derivative.x + normal.y * derivative.y + normal.z * derivative.z;
        Vector3::new(
            (derivative.x - normal_component * normal.x) / normal_magnitude,
            (derivative.y - normal_component * normal.y) / normal_magnitude,
            (derivative.z - normal_component * normal.z) / normal_magnitude,
        )
    };
    let first = base.second.and_then(|second| {
        let [duu, duv, dvv] = FiniteVector3::raw_array(second);
        let normal_u =
            unit_normal_derivative(vector_sum(&[(1.0, duu.cross(dv)), (1.0, du.cross(duv))]));
        let normal_v =
            unit_normal_derivative(vector_sum(&[(1.0, duv.cross(dv)), (1.0, du.cross(dvv))]));
        admit_lanes([
            Vector3::new(
                du.x + distance * normal_u.x,
                du.y + distance * normal_u.y,
                du.z + distance * normal_u.z,
            ),
            Vector3::new(
                dv.x + distance * normal_v.x,
                dv.y + distance * normal_v.y,
                dv.z + distance * normal_v.z,
            ),
        ])
    });
    Ok(SurfaceJet {
        point,
        first,
        second: base.second,
    })
}

/// `(end - start) / (domain_end - domain_start)` over exact differences. An
/// empty domain has no quotient; a quotient that overflows is non-finite and
/// carries its signed infinity.
fn difference_quotient(
    end: FiniteReal,
    start: FiniteReal,
    domain_end: FiniteReal,
    domain_start: FiniteReal,
) -> Result<FiniteReal, EvaluationFailure<f64>> {
    let mut denominator = ExactSignedSum::default();
    denominator.add_product(domain_end.get(), 1.0);
    denominator.add_product(domain_start.get(), -1.0);
    let denominator = denominator.finish().ok_or(EvaluationFailure::NoValue)?;
    let mut numerator = ExactSignedSum::default();
    numerator.add_product(end.get(), 1.0);
    numerator.add_product(start.get(), -1.0);
    numerator.finish().map_or(Ok(FiniteReal::ZERO), |value| {
        value
            .quotient(denominator)
            .map_err(EvaluationFailure::NonFinite)
    })
}

fn scale_vector(vector: Vector3, factor: f64) -> Vector3 {
    Vector3::new(vector.x * factor, vector.y * factor, vector.z * factor)
}

fn vector_sum(terms: &[(f64, Vector3)]) -> Vector3 {
    let point = offset(Point3::new(0.0, 0.0, 0.0), terms);
    Vector3::new(point.x, point.y, point.z)
}

/// Evaluate a pcurve carrier at parameter `t`, yielding a surface `(u, v)`.
///
/// Evaluation is total over the carrier's stated shape and does not consult a
/// declared domain. A NURBS carrier extrapolates its end span past the knot
/// interval, a trim hands `t` to its basis outside the trim interval, and a
/// line and a conic evaluate at every finite `t`. The failure is never that
/// `t` is out of domain.
///
/// An evaluation that leaves the finite range reports
/// [`EvaluationFailure::NonFinite`] with the point it reached; a coordinate
/// that no step reached is NaN. A carrier that evaluates its derivatives
/// together with its point also reports its point this way when a derivative
/// leaves the finite range. A parameter that is not finite, a structure that
/// states no point at `t`, and an undefined step (a polar chart at its
/// origin, an offset whose basis tangent is zero) report
/// [`EvaluationFailure::NoValue`].
///
/// Callers that recover a parameter from an unreliable declared interval
/// depend on this: they seed and step outside the interval and use the
/// evaluated point as the witness. A caller that wants the domain asks
/// the carrier for it.
pub fn pcurve_uv(
    geometry: &PcurveGeometry,
    t: f64,
) -> Result<FinitePoint2, EvaluationFailure<Point2>> {
    let t = FiniteReal::new(t).ok_or(EvaluationFailure::NoValue)?;
    pcurve_uv_differential(geometry, t)
        .ok_or(EvaluationFailure::NoValue)?
        .point
        .map_err(EvaluationFailure::NonFinite)
}

/// Evaluate the exact first derivative of a directly stored pcurve.
///
/// The derivative is finite only where the carrier's evaluation is: an
/// evaluation that leaves the finite range reports
/// [`EvaluationFailure::NonFinite`] with the derivative it reached, finite
/// or not. A parameter that is not finite and a structure that states no
/// derivative report [`EvaluationFailure::NoValue`].
pub fn pcurve_tangent(
    geometry: &PcurveGeometry,
    t: f64,
) -> Result<FinitePoint2, EvaluationFailure<Point2>> {
    let t = FiniteReal::new(t).ok_or(EvaluationFailure::NoValue)?;
    pcurve_uv_differential(geometry, t)
        .ok_or(EvaluationFailure::NoValue)?
        .tangent
}

/// The angle of a polar chart point and its first two derivatives, without
/// squaring coordinates in the finite f64 range. A chart point at the origin
/// has no angle.
///
/// The first derivative is absent without a chart tangent. A chart tangent
/// that left the finite range, and a quotient that overflows, make it
/// non-finite; a derivative that no step reached is NaN.
fn polar_angle_differential(
    point: FinitePoint2,
    tangent: Result<FinitePoint2, EvaluationFailure<Point2>>,
    acceleration: Option<FinitePoint2>,
) -> Option<PolarAngle> {
    let mut radius = ExactSignedSum::default();
    radius.add_product(point.u, point.u);
    radius.add_product(point.v, point.v);
    let radius = radius.finish()?;
    let first = match tangent {
        Ok(tangent) => {
            let mut cross = ExactSignedSum::default();
            cross.add_product(point.u, tangent.v);
            cross.add_product(-point.v, tangent.u);
            cross.finish().map_or(Ok(FiniteReal::ZERO), |cross| {
                cross.quotient(radius).map_err(EvaluationFailure::NonFinite)
            })
        }
        Err(EvaluationFailure::NonFinite(_)) => Err(EvaluationFailure::NonFinite(f64::NAN)),
        Err(EvaluationFailure::NoValue) => Err(EvaluationFailure::NoValue),
    };
    let second = tangent.ok().zip(acceleration).zip(first.ok()).and_then(
        |((tangent, acceleration), first)| {
            let mut dot = ExactSignedSum::default();
            dot.add_product(point.u, tangent.u);
            dot.add_product(point.v, tangent.v);
            let mut numerator = ExactSignedSum::default();
            numerator.add_product(point.u, acceleration.v);
            numerator.add_product(-point.v, acceleration.u);
            numerator.add_scaled_product(dot.finish(), first.negated())?;
            numerator.add_scaled_product(dot.finish(), first.negated())?;
            numerator
                .finish()
                .map_or(Some(FiniteReal::ZERO), |value| value.quotient(radius).ok())
        },
    );
    let [u, v] = point.coordinates();
    Some(PolarAngle {
        angle: v.atan2(u),
        first,
        second,
    })
}

/// The angle of a polar chart point and its first two derivatives.
struct PolarAngle {
    angle: FiniteReal,
    /// The first derivative, or why it has no finite value.
    first: Result<FiniteReal, EvaluationFailure<f64>>,
    /// The second derivative, where it is finite.
    second: Option<FiniteReal>,
}

/// A pcurve carrier's point and first two derivatives at a parameter.
struct PcurveEvaluation {
    /// The point, or the point an evaluation that left the finite range
    /// reached.
    point: Result<FinitePoint2, Point2>,
    /// The first derivative, or why it has no finite value.
    tangent: Result<FinitePoint2, EvaluationFailure<Point2>>,
    /// The second derivative, where it is finite.
    acceleration: Option<FinitePoint2>,
}

impl PcurveEvaluation {
    /// A carrier whose point and derivatives are evaluated together: the
    /// point is finite only where the whole evaluation is. An evaluation that
    /// left the finite range carries the point and tangent it reached and no
    /// acceleration.
    fn evaluated(
        point: Point2,
        tangent: Result<FinitePoint2, EvaluationFailure<Point2>>,
        acceleration: Option<FinitePoint2>,
    ) -> Self {
        match FinitePoint2::new(point) {
            Some(point) => Self {
                point: Ok(point),
                tangent,
                acceleration,
            },
            None => Self::left_finite_range(point, reached_value(tangent)),
        }
    }

    /// An evaluation that left the finite range, with the point and tangent
    /// it reached. Either can be finite: the evaluation also leaves the range
    /// where the other, or a quantity formed with them, does.
    fn left_finite_range(point: Point2, tangent: Point2) -> Self {
        Self {
            point: Err(point),
            tangent: Err(EvaluationFailure::NonFinite(tangent)),
            acceleration: None,
        }
    }
}

impl From<PcurveDifferential> for PcurveEvaluation {
    fn from(differential: PcurveDifferential) -> Self {
        Self {
            point: Ok(differential.point),
            tangent: differential.tangent,
            acceleration: differential.acceleration,
        }
    }
}

/// The value an evaluation reached: the finite value, the non-finite value,
/// or NaN in each coordinate where no step reached one.
fn reached_value(value: Result<FinitePoint2, EvaluationFailure<Point2>>) -> Point2 {
    match value {
        Ok(value) => value.get(),
        Err(EvaluationFailure::NonFinite(value)) => value,
        Err(EvaluationFailure::NoValue) => Point2::new(f64::NAN, f64::NAN),
    }
}

/// Evaluate a pcurve carrier's point and its first two derivatives at `t`.
/// `None` states that the carrier has no value at `t`.
///
/// The recursion needs no depth budget. It descends only through
/// [`PlacedPcurve`](crate::geometry::pcurve::PlacedPcurve),
/// [`TrimmedPcurve`](crate::geometry::pcurve::TrimmedPcurve) and
/// [`OffsetPcurve`](crate::geometry::pcurve::OffsetPcurve), each holding one
/// inline `Box<PcurveGeometry>` behind a `try_new` that refuses a chain past
/// [`MAX_GEOMETRY_NESTING`](crate::geometry::MAX_GEOMETRY_NESTING). No arm
/// follows an arena id, so the value handed in bounds the descent.
fn pcurve_uv_differential(
    geometry: &PcurveGeometry,
    parameter: FiniteReal,
) -> Option<PcurveEvaluation> {
    let t = parameter.get();
    let pair = match geometry {
        PcurveGeometry::Line(line_pcurve) => {
            let origin = line_pcurve.origin().as_raw();
            let direction = line_pcurve.direction().as_raw();
            (
                Point2::new(origin.u + t * direction.u, origin.v + t * direction.v),
                *direction,
                Point2::new(0.0, 0.0),
            )
        }
        PcurveGeometry::Circle(circle_pcurve) => {
            let center = circle_pcurve.center();
            let x_axis = circle_pcurve.x_axis();
            let y_axis = circle_pcurve.y_axis();
            let radius = circle_pcurve.radius();
            let cosine = t.cos();
            let sine = t.sin();
            (
                offset2(
                    center.get(),
                    &[
                        (radius.get() * cosine, x_axis.get()),
                        (radius.get() * sine, y_axis.get()),
                    ],
                ),
                Point2::new(
                    radius.get() * (-sine * x_axis.u + cosine * y_axis.u),
                    radius.get() * (-sine * x_axis.v + cosine * y_axis.v),
                ),
                Point2::new(
                    -radius.get() * (cosine * x_axis.u + sine * y_axis.u),
                    -radius.get() * (cosine * x_axis.v + sine * y_axis.v),
                ),
            )
        }
        PcurveGeometry::Ellipse(ellipse_pcurve) => {
            let center = ellipse_pcurve.center();
            let x_axis = ellipse_pcurve.x_axis();
            let y_axis = ellipse_pcurve.y_axis();
            let major_radius = ellipse_pcurve.major_radius();
            let minor_radius = ellipse_pcurve.minor_radius();
            let cosine = t.cos();
            let sine = t.sin();
            (
                offset2(
                    center.get(),
                    &[
                        (major_radius.get() * cosine, x_axis.get()),
                        (minor_radius.get() * sine, y_axis.get()),
                    ],
                ),
                Point2::new(
                    -major_radius.get() * sine * x_axis.u + minor_radius.get() * cosine * y_axis.u,
                    -major_radius.get() * sine * x_axis.v + minor_radius.get() * cosine * y_axis.v,
                ),
                Point2::new(
                    -major_radius.get() * cosine * x_axis.u - minor_radius.get() * sine * y_axis.u,
                    -major_radius.get() * cosine * x_axis.v - minor_radius.get() * sine * y_axis.v,
                ),
            )
        }
        PcurveGeometry::Harmonic(harmonic_pcurve) => {
            let center = harmonic_pcurve.center();
            let cosine = harmonic_pcurve.cosine();
            let sine = harmonic_pcurve.sine();
            let cosine_parameter = t.cos();
            let sine_parameter = t.sin();
            (
                offset2(
                    center.get(),
                    &[
                        (cosine_parameter, cosine.get()),
                        (sine_parameter, sine.get()),
                    ],
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
        PcurveGeometry::Parabola(parabola) => {
            let vertex = parabola.vertex();
            let x = parabola.x_axis();
            let y = parabola.y_axis();
            let focal = parabola.focal_distance();
            // `t * t` and `4 * focal` are not negative, so an axial quotient
            // that overflows reaches positive infinity.
            let axial =
                product_quotient([t, t], [4.0, focal.get()]).map_or(f64::INFINITY, FiniteReal::get);
            let derivative = |axis| {
                product_quotient([t, axis], [2.0, focal.get()]).map_or(f64::NAN, FiniteReal::get)
            };
            let second = |axis| {
                product_quotient([axis], [2.0, focal.get()]).map_or(f64::NAN, FiniteReal::get)
            };
            (
                offset2(vertex.get(), &[(axial, x.get()), (t, y.get())]),
                Point2::new(derivative(x.u) + y.u, derivative(x.v) + y.v),
                Point2::new(second(x.u), second(x.v)),
            )
        }
        PcurveGeometry::Hyperbola(hyperbola) => {
            let x = hyperbola.x_axis();
            let y = hyperbola.y_axis();
            let reached = |pair: Result<(FiniteReal, FiniteReal), (f64, f64)>| {
                pair.map_or_else(|plain| plain, |(sinh, cosh)| (sinh.get(), cosh.get()))
            };
            let (major_sinh, major_cosh) =
                reached(scaled_sinh_cosh(hyperbola.major_radius().into(), parameter));
            let (minor_sinh, minor_cosh) =
                reached(scaled_sinh_cosh(hyperbola.minor_radius().into(), parameter));
            let zero = Point2::new(0.0, 0.0);
            let point = offset2(
                hyperbola.center().get(),
                &[(major_cosh, x.get()), (minor_sinh, y.get())],
            );
            let tangent = offset2(zero, &[(major_sinh, x.get()), (minor_cosh, y.get())]);
            let acceleration = offset2(zero, &[(major_cosh, x.get()), (minor_sinh, y.get())]);
            return Some(PcurveEvaluation::evaluated(
                point,
                FinitePoint2::new(tangent).ok_or(EvaluationFailure::NonFinite(tangent)),
                FinitePoint2::new(acceleration),
            ));
        }
        PcurveGeometry::Hyperbolic(hyperbolic) => {
            let [cosine_u, cosine_v] = hyperbolic.cosine().coordinates();
            let [sine_u, sine_v] = hyperbolic.sine().coordinates();
            let [center_u, center_v] = hyperbolic.center().coordinates();
            // Each coordinate and its two derivatives read their own lanes.
            let coordinate = |center: FiniteReal, cosine: FiniteReal, sine: FiniteReal| {
                let cosine = scaled_sinh_cosh(cosine, parameter);
                let sine = scaled_sinh_cosh(sine, parameter);
                let reached = |pair: Result<(FiniteReal, FiniteReal), (f64, f64)>| {
                    pair.map_or_else(|plain| plain, |(sinh, cosh)| (sinh.get(), cosh.get()))
                };
                let (cosine_sinh, cosine_cosh) = reached(cosine);
                let (sine_sinh, sine_cosh) = reached(sine);
                (
                    crate::math::sum::finite_dot([1.0; 3], [center.get(), cosine_cosh, sine_sinh]),
                    crate::math::sum::finite_dot([1.0; 2], [cosine_sinh, sine_cosh]),
                    crate::math::sum::finite_dot([1.0; 2], [cosine_cosh, sine_sinh]),
                )
            };
            let u = coordinate(center_u, cosine_u, sine_u);
            let v = coordinate(center_v, cosine_v, sine_v);
            let reached =
                |value: Result<FiniteReal, f64>| value.map_or_else(|plain| plain, FiniteReal::get);
            let lane = |value: Result<FiniteReal, f64>| value.map_err(EvaluationFailure::NonFinite);
            let (Ok(point_u), Ok(point_v)) = (u.0, v.0) else {
                return Some(PcurveEvaluation::left_finite_range(
                    Point2::new(reached(u.0), reached(v.0)),
                    Point2::new(reached(u.1), reached(v.1)),
                ));
            };
            return Some(PcurveEvaluation {
                point: Ok(FinitePoint2::from_coordinates(point_u, point_v)),
                tangent: planar_value(lane(u.1), lane(v.1)),
                acceleration: u
                    .2
                    .ok()
                    .zip(v.2.ok())
                    .map(|(u, v)| FinitePoint2::from_coordinates(u, v)),
            });
        }
        PcurveGeometry::PolarHarmonic(polar_harmonic_pcurve) => {
            let radial_center = polar_harmonic_pcurve.radial_center();
            let radial_cos = polar_harmonic_pcurve.radial_cos();
            let radial_sin = polar_harmonic_pcurve.radial_sin();
            let axial_origin = polar_harmonic_pcurve.axial_origin().get();
            let axial_cos = polar_harmonic_pcurve.axial_cos().get();
            let axial_sin = polar_harmonic_pcurve.axial_sin().get();
            let cosine = t.cos();
            let sine = t.sin();
            let x = radial_center.u + radial_cos.u * cosine + radial_sin.u * sine;
            let y = radial_center.v + radial_cos.v * cosine + radial_sin.v * sine;
            let dx = -radial_cos.u * sine + radial_sin.u * cosine;
            let dy = -radial_cos.v * sine + radial_sin.v * cosine;
            let ddx = -radial_cos.u * cosine - radial_sin.u * sine;
            let ddy = -radial_cos.v * cosine - radial_sin.v * sine;
            let axial = axial_origin + axial_cos * cosine + axial_sin * sine;
            let axial_derivative = -axial_cos * sine + axial_sin * cosine;
            let Some(radial) = FinitePoint2::new(Point2::new(x, y)) else {
                // The angle of a radial point outside the finite range is
                // not reached.
                return Some(PcurveEvaluation::left_finite_range(
                    Point2::new(f64::NAN, axial),
                    Point2::new(f64::NAN, axial_derivative),
                ));
            };
            let PolarAngle {
                angle,
                first,
                second,
            } = polar_angle_differential(
                radial,
                admit_parameter_point(Point2::new(dx, dy)),
                FinitePoint2::new(Point2::new(ddx, ddy)),
            )?;
            let Some(finite_axial) = FiniteReal::new(axial) else {
                let first = first.map_or_else(
                    |failure| failure.non_finite().unwrap_or(f64::NAN),
                    FiniteReal::get,
                );
                return Some(PcurveEvaluation::left_finite_range(
                    Point2::new(angle.get(), axial),
                    Point2::new(first, axial_derivative),
                ));
            };
            let axial_lane =
                |value: f64| FiniteReal::new(value).ok_or(EvaluationFailure::NonFinite(value));
            return Some(PcurveEvaluation {
                point: Ok(FinitePoint2::from_coordinates(angle, finite_axial)),
                tangent: planar_value(first, axial_lane(axial_derivative)),
                acceleration: second.and_then(|second| {
                    let axial = FiniteReal::new(-axial_cos * cosine - axial_sin * sine)?;
                    Some(FinitePoint2::from_coordinates(second, axial))
                }),
            });
        }
        PcurveGeometry::PolarNurbs { nurbs } => {
            let radial_control_points = nurbs.radial_control_points();
            let radial = nurbs_pcurve_differential_with(
                nurbs.degree(),
                nurbs.knots(),
                radial_control_points.len(),
                |index| radial_control_points.get(index).copied().map(planar_pole),
                nurbs.pole_rows().weights().as_deref(),
                parameter,
            );
            let axial_values = nurbs.axial_control_values();
            let axial = nurbs_pcurve_differential_with(
                nurbs.degree(),
                nurbs.knots(),
                axial_values.len(),
                |index| {
                    let axial = *axial_values.get(index)?;
                    Some(FinitePoint3::from_coordinates(
                        axial,
                        FiniteReal::ZERO,
                        FiniteReal::ZERO,
                    ))
                },
                nurbs.pole_rows().weights().as_deref(),
                parameter,
            );
            let (radial, axial) = match (radial, axial) {
                (Ok(radial), Ok(axial)) => (radial, axial),
                (Err(EvaluationFailure::NoValue), _) | (_, Err(EvaluationFailure::NoValue)) => {
                    return None;
                }
                // The angle of a radial point outside the finite range is
                // not reached, and the derivatives of a spline point outside
                // it are not formed.
                (radial, axial) => {
                    let axial = match axial {
                        Ok(axial) => axial.point.get().u,
                        Err(failure) => failure.non_finite().map_or(f64::NAN, |axial| axial.u),
                    };
                    // A radial point at the origin has no angle.
                    let angle = match radial {
                        Ok(radial) => polar_angle_differential(radial.point, radial.tangent, None)?
                            .angle
                            .get(),
                        Err(_) => f64::NAN,
                    };
                    return Some(PcurveEvaluation::left_finite_range(
                        Point2::new(angle, axial),
                        Point2::new(f64::NAN, f64::NAN),
                    ));
                }
            };
            let PolarAngle {
                angle,
                first,
                second,
            } = polar_angle_differential(radial.point, radial.tangent, radial.acceleration)?;
            let axial_lane = |value: Result<FinitePoint2, EvaluationFailure<Point2>>| match value {
                Ok(value) => Ok(value.coordinates()[0]),
                Err(failure) => Err(match failure {
                    EvaluationFailure::NoValue => EvaluationFailure::NoValue,
                    EvaluationFailure::NonFinite(value) => EvaluationFailure::NonFinite(value.u),
                }),
            };
            return Some(PcurveEvaluation {
                point: Ok(FinitePoint2::from_coordinates(
                    angle,
                    axial.point.coordinates()[0],
                )),
                tangent: planar_value(first, axial_lane(axial.tangent)),
                acceleration: second.zip(axial.acceleration).map(|(second, axial)| {
                    FinitePoint2::from_coordinates(second, axial.coordinates()[0])
                }),
            });
        }
        PcurveGeometry::SphericalGreatCircle(spherical_great_circle_pcurve) => {
            let azimuth_origin = spherical_great_circle_pcurve.azimuth_origin().get();
            let admitted_rate = spherical_great_circle_pcurve.azimuth_rate();
            let azimuth_rate = admitted_rate.get();
            let plane_phase = spherical_great_circle_pcurve.plane_phase().get();
            let plane_slope = spherical_great_circle_pcurve.plane_slope().get();
            let raw_azimuth = azimuth_origin + azimuth_rate * t;
            let Some(azimuth) = FiniteReal::new(raw_azimuth) else {
                // The latitude of an azimuth outside the finite range is not
                // reached.
                return Some(PcurveEvaluation::left_finite_range(
                    Point2::new(raw_azimuth, f64::NAN),
                    Point2::new(azimuth_rate, f64::NAN),
                ));
            };
            let phase = azimuth.get() - plane_phase;
            let cosine = phase.cos();
            let sine = phase.sin();
            let scale = plane_slope.abs().max(1.0);
            let slope = plane_slope / scale;
            // The chart and its derivatives are finite exactly when the phase
            // is: a phase outside the finite range reaches no latitude.
            let Some([chart_u, chart_v, tangent_v, acceleration_v]) =
                FiniteReal::array([1.0 / scale, slope * cosine, -slope * sine, -slope * cosine])
            else {
                return Some(PcurveEvaluation::left_finite_range(
                    Point2::new(azimuth.get(), f64::NAN),
                    Point2::new(azimuth_rate, f64::NAN),
                ));
            };
            let PolarAngle {
                angle: latitude,
                first,
                second,
            } = polar_angle_differential(
                FinitePoint2::from_coordinates(chart_u, chart_v),
                Ok(FinitePoint2::from_coordinates(FiniteReal::ZERO, tangent_v)),
                Some(FinitePoint2::from_coordinates(
                    FiniteReal::ZERO,
                    acceleration_v,
                )),
            )?;
            let latitude_rate = first.and_then(|first| {
                let mut sum = ExactSignedSum::default();
                sum.add_product(first.get(), azimuth_rate);
                sum.finish().map_or(Ok(FiniteReal::ZERO), |value| {
                    value.finite().map_err(EvaluationFailure::NonFinite)
                })
            });
            let acceleration = second
                .and_then(|second| {
                    let mut sum = ExactSignedSum::default();
                    sum.add_factors([second.get(), azimuth_rate, azimuth_rate]);
                    sum.finish()
                        .map_or(Some(FiniteReal::ZERO), |value| value.finite().ok())
                })
                .map(|value| FinitePoint2::from_coordinates(FiniteReal::ZERO, value));
            return Some(PcurveEvaluation {
                point: Ok(FinitePoint2::from_coordinates(azimuth, latitude)),
                tangent: planar_value(Ok(admitted_rate), latitude_rate),
                acceleration,
            });
        }
        PcurveGeometry::Nurbs { nurbs } => {
            let control_points = nurbs.control_points();
            return match nurbs_pcurve_differential_with(
                nurbs.degree(),
                nurbs.knots(),
                control_points.len(),
                |index| control_points.get(index).copied().map(planar_pole),
                nurbs.pole_rows().weights().as_deref(),
                parameter,
            ) {
                Ok(differential) => Some(PcurveEvaluation::from(differential)),
                // The quotient rule forms the derivatives from the finite
                // point, so a point outside the finite range reaches none.
                Err(EvaluationFailure::NonFinite(point)) => Some(
                    PcurveEvaluation::left_finite_range(point, Point2::new(f64::NAN, f64::NAN)),
                ),
                Err(EvaluationFailure::NoValue) => None,
            };
        }
        PcurveGeometry::Transformed(placed) => {
            let transform = placed.transform();
            let basis = pcurve_uv_differential(placed.basis(), parameter)?;
            let point =
                transform.apply_point(basis.point.map_or_else(|point| point, FinitePoint2::get));
            let tangent = match basis.tangent {
                Ok(tangent) => admit_parameter_point(transform.apply_vector(tangent.get())),
                Err(EvaluationFailure::NonFinite(tangent)) => Err(EvaluationFailure::NonFinite(
                    transform.apply_vector(tangent),
                )),
                Err(EvaluationFailure::NoValue) => Err(EvaluationFailure::NoValue),
            };
            let acceleration = basis
                .acceleration
                .map(|acceleration| transform.apply_vector(acceleration.get()));
            // The placement evaluates its derivatives with its point: a basis
            // tangent or acceleration the placement carries outside the finite
            // range leaves the evaluation there.
            let derivative_left_range = basis.tangent.is_ok() && tangent.is_err()
                || acceleration.is_some_and(|acceleration| !acceleration.is_finite());
            if basis.point.is_err() || derivative_left_range {
                return Some(PcurveEvaluation::left_finite_range(
                    point,
                    reached_value(tangent),
                ));
            }
            return Some(PcurveEvaluation::evaluated(
                point,
                tangent,
                acceleration.and_then(FinitePoint2::new),
            ));
        }
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let basis = trimmed_pcurve.basis();
            return pcurve_uv_differential(basis, parameter);
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            let distance = offset_pcurve.distance();
            let basis = offset_pcurve.basis();
            let basis = pcurve_uv_differential(basis, parameter)?;
            let tangent = match basis.tangent {
                Ok(tangent) => tangent,
                // The offset direction is the unit normal of the basis
                // tangent, which a tangent outside the finite range does not
                // reach; a basis without a tangent states no direction.
                Err(EvaluationFailure::NonFinite(_)) => {
                    return Some(PcurveEvaluation::left_finite_range(
                        Point2::new(f64::NAN, f64::NAN),
                        Point2::new(f64::NAN, f64::NAN),
                    ));
                }
                Err(EvaluationFailure::NoValue) if basis.point.is_err() => {
                    return Some(PcurveEvaluation::left_finite_range(
                        Point2::new(f64::NAN, f64::NAN),
                        Point2::new(f64::NAN, f64::NAN),
                    ));
                }
                Err(EvaluationFailure::NoValue) => return None,
            };
            let speed = tangent.u.hypot(tangent.v);
            if speed == 0.0 {
                return None;
            }
            if !speed.is_finite() {
                // The unit normal of a tangent whose length overflows is not
                // reached.
                return Some(PcurveEvaluation::left_finite_range(
                    Point2::new(f64::NAN, f64::NAN),
                    Point2::new(f64::NAN, f64::NAN),
                ));
            }
            let unit = Point2::new(tangent.u / speed, tangent.v / speed);
            // The offset of a non-finite basis point is the non-finite point
            // it reaches.
            let basis_point = basis.point.map_or_else(|point| point, FinitePoint2::get);
            let point = Point2::new(
                basis_point.u - distance.get() * unit.v,
                basis_point.v + distance.get() * unit.u,
            );
            let tangent = basis.acceleration.map(|acceleration| {
                let tangential_acceleration = unit.u * acceleration.u + unit.v * acceleration.v;
                let unit_derivative = Point2::new(
                    (acceleration.u - tangential_acceleration * unit.u) / speed,
                    (acceleration.v - tangential_acceleration * unit.v) / speed,
                );
                Point2::new(
                    tangent.u - distance.get() * unit_derivative.v,
                    tangent.v + distance.get() * unit_derivative.u,
                )
            });
            // The offset tangent reads the basis acceleration. A basis that
            // states one and has none at a finite point and tangent has an
            // acceleration outside the finite range, which the offset tangent
            // does not reach; an offset basis states none.
            let tangent = match tangent {
                Some(tangent) => admit_parameter_point(tangent),
                None if states_pcurve_acceleration(offset_pcurve.basis()) => Err(
                    EvaluationFailure::NonFinite(Point2::new(f64::NAN, f64::NAN)),
                ),
                None => Err(EvaluationFailure::NoValue),
            };
            return Some(PcurveEvaluation {
                point: FinitePoint2::new(point).ok_or(point),
                tangent,
                acceleration: None,
            });
        }
    };
    Some(PcurveEvaluation::evaluated(
        pair.0,
        admit_parameter_point(pair.1),
        FinitePoint2::new(pair.2),
    ))
}

/// Whether a pcurve's evaluation states a second derivative. Every carrier
/// does except an offset, whose evaluation forms no second derivative, and
/// the trimmed and placed carriers over one.
fn states_pcurve_acceleration(geometry: &PcurveGeometry) -> bool {
    match geometry {
        PcurveGeometry::Offset(_) => false,
        PcurveGeometry::Trimmed(trimmed) => states_pcurve_acceleration(trimmed.basis()),
        PcurveGeometry::Transformed(placed) => states_pcurve_acceleration(placed.basis()),
        _ => true,
    }
}

fn offset2(base: Point2, terms: &[(f64, Point2)]) -> Point2 {
    let coordinate = |base: f64, component: fn(&Point2) -> f64| match crate::math::sum::product_sum(
        std::iter::once(Some([1.0, base])).chain(
            terms
                .iter()
                .map(|(factor, vector)| Some([*factor, component(vector)])),
        ),
    ) {
        crate::math::sum::ProductSum::Value(value) => {
            value.finite().map_or(f64::NAN, FiniteReal::get)
        }
        crate::math::sum::ProductSum::Zero => 0.0,
        crate::math::sum::ProductSum::Undefined => f64::NAN,
    };
    Point2::new(coordinate(base.u, |p| p.u), coordinate(base.v, |p| p.v))
}

#[cfg(test)]
mod tests;

/// Evaluate a 3D curve carrier at parameter `t` on its own parameterization.
/// A procedural carrier without a solved cache has no value here.
pub fn curve_point(
    geometry: &CurveGeometry,
    t: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    curve_point_solved(geometry.solved().ok_or(EvaluationFailure::NoValue)?, t)
}

/// Evaluate the exact first derivative of a stored curve carrier, or report
/// why it has no finite value, as [`curve_tangent_solved`] states. A
/// procedural carrier without a solved cache has no value here.
pub fn curve_tangent(
    geometry: &CurveGeometry,
    t: f64,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    curve_tangent_solved(geometry.solved().ok_or(EvaluationFailure::NoValue)?, t)
}

/// Evaluate the exact second derivative of a stored curve carrier, or report
/// why it has no finite value, as [`curve_tangent_solved`] states. A
/// procedural carrier without a solved cache has no value here.
pub fn curve_second_derivative(
    geometry: &CurveGeometry,
    t: f64,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    curve_second_derivative_solved(geometry.solved().ok_or(EvaluationFailure::NoValue)?, t)
}

/// Evaluate a stored curve carrier at `t` within a caller-owned work slice.
pub fn curve_point_with_budget(
    geometry: &CurveGeometry,
    t: f64,
    budget: &WorkBudget<'_>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    curve_point_with_budget_solved(
        geometry.solved().ok_or(EvaluationFailure::NoValue)?,
        t,
        budget,
    )
}

/// [`curve_tangent`] within a caller-owned work slice. A refused charge
/// leaves no value.
pub fn curve_tangent_with_budget(
    geometry: &CurveGeometry,
    t: f64,
    budget: &WorkBudget<'_>,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    curve_tangent_with_budget_solved(
        geometry.solved().ok_or(EvaluationFailure::NoValue)?,
        t,
        budget,
    )
}

/// [`curve_second_derivative`] within a caller-owned work slice. A refused
/// charge leaves no value.
pub fn curve_second_derivative_with_budget(
    geometry: &CurveGeometry,
    t: f64,
    budget: &WorkBudget<'_>,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    curve_second_derivative_with_budget_solved(
        geometry.solved().ok_or(EvaluationFailure::NoValue)?,
        t,
        budget,
    )
}

/// Evaluate a surface carrier at `(u, v)` on its own parameterization.
pub fn surface_point(
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    surface_point_solved(geometry.solved().ok_or(EvaluationFailure::NoValue)?, u, v)
}

/// Evaluate a surface carrier at `(u, v)` within a caller-owned work slice.
pub fn surface_point_with_budget(
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
    budget: &WorkBudget<'_>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    surface_point_with_budget_solved(
        geometry.solved().ok_or(EvaluationFailure::NoValue)?,
        u,
        v,
        budget,
    )
}

/// Evaluate the first partial derivatives of a surface carrier, or report
/// why they have no finite value, as [`surface_partials_solved`] states. A
/// procedural carrier without a solved cache has no value here.
pub fn surface_partials(
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<SurfacePartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    surface_partials_solved(geometry.solved().ok_or(EvaluationFailure::NoValue)?, u, v)
}

/// Evaluate the second partial derivatives of a surface carrier, or report
/// why they have no finite value, as [`surface_second_partials_solved`]
/// states. A procedural carrier without a solved cache has no value here.
pub fn surface_second_partials(
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    surface_second_partials_solved(geometry.solved().ok_or(EvaluationFailure::NoValue)?, u, v)
}

/// Analytic surface parameters of the point on a surface carrier.
pub fn analytic_surface_parameters(
    geometry: &SurfaceGeometry,
    point: Point3,
) -> Option<FinitePoint2> {
    analytic_surface_parameters_solved(geometry.solved()?, point)
}

/// [`surface_first_order_solved`] of a surface carrier's solved geometry. A
/// procedural carrier without a solved cache has no value here.
fn surface_first_order(
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    surface_first_order_solved(
        geometry.solved().ok_or(EvaluationFailure::NoValue)?,
        u,
        v,
        budget,
    )
}

/// [`surface_jet_solved`] of a surface carrier's solved geometry. A
/// procedural carrier without a solved cache has no value here.
fn surface_jet(
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    surface_jet_solved(
        geometry.solved().ok_or(EvaluationFailure::NoValue)?,
        u,
        v,
        budget,
    )
}

fn model_surface_point_with_budget(
    ir: &CadIr,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    match geometry.solved() {
        Some(solved) => model_surface_point_with_budget_solved(solved, u, v, budget),
        None => model_surface_point_inner(ir, geometry, u, v, budget),
    }
}

#[cfg(test)]
mod numerical_range_tests;
