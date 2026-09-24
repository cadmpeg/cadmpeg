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
//! [`SolvedSurfaceGeometry::Unknown`], parabolas, and hyperbolas) evaluate to `None`.
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
    sampled::PolylineCurve,
    CurveGeometry, LawExpression, LawFormula, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    SweepSurfaceLayout,
};
use crate::math::solve::least_squares_step;
use crate::math::sum::{scaled_ratio_products, ExactSignedSum};
use crate::math::{product_quotient, scaled_sinh_cosh};
use crate::math::{Point2, Point3, Vector3};
use crate::scalar::{
    ExtendedReal, FiniteReal, Length, NonNegativeLength, NonNegativeReal, NonZeroLength,
    NonZeroReal, PositiveReal,
};
use crate::topology::{IncreasingParameterInterval, ParameterInterval};
use crate::transform::Transform;
use crate::units::{FinitePoint2, FiniteVector, UnitVector3};
use crate::CadIr;
use cadmpeg_core::decode::{alloc_filled, WorkBudget};

mod rational;
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
        Some(
            difference_quotient(value, lower, upper, lower)
                .ok()?
                .get()
                .clamp(0.0, 1.0),
        )
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
            nurbs_surface_point_with_budget(surface, parameters.u, parameters.v, budget)?;
        let residual = Vector3::new(
            position.x - point.x,
            position.y - point.y,
            position.z - point.z,
        );
        let partials =
            nurbs_surface_partials_with_budget(surface, parameters.u, parameters.v, budget)?;
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
                nurbs_surface_point_with_budget(surface, candidate.u, candidate.v, budget)?;
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
            nurbs_surface_point_with_budget(surface, parameters.u, parameters.v, budget)?;
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
            nurbs_surface_point_with_budget(surface, parameters.u, parameters.v, budget)?;
        let distance = (position.x - point.x)
            .hypot(position.y - point.y)
            .hypot(position.z - point.z);
        if distance.is_finite() && distance <= tolerance {
            return Some((parameters, distance));
        }
        if let Some(refined) =
            refine_nurbs_surface_parameters(surface, point, parameters, u_domain, v_domain, budget)
        {
            let position = nurbs_surface_point_with_budget(surface, refined.u, refined.v, budget)?;
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
        let Some(position) =
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
                    let candidate = nurbs_surface_point(surface, u.get(), v.get())?;
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
        let partials = nurbs_surface_partials(surface, parameters.u, parameters.v)?;
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
            let candidate_point = nurbs_surface_point(surface, candidate.u, candidate.v)?;
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

/// Evaluate a possibly-rational B-spline curve over 3D poles. The point is
/// absent when a coordinate is not finite.
pub fn nurbs_curve_point(
    degree: u32,
    knots: &[f64],
    control_points: &[Point3],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<FinitePoint3> {
    nurbs_curve_point_with(
        degree,
        knots,
        control_points.len(),
        |index| FinitePoint3::new(*control_points.get(index)?),
        weights,
        t,
    )
}

/// Evaluate a NURBS curve at knot-domain parameter `t` over its admitted
/// poles. The point is absent when `t` is outside the knot domain or a
/// homogeneous sum is not finite.
pub fn nurbs_curve_point_at(curve: &NurbsCurve, t: f64) -> Option<FinitePoint3> {
    let poles = curve.control_points();
    nurbs_curve_point_with(
        curve.degree(),
        curve.knots(),
        poles.len(),
        |index| poles.get(index).copied(),
        curve.pole_rows().weights().as_deref(),
        t,
    )
}

/// [`nurbs_curve_point`] over `count` poles that `pole` hands out admitted.
/// The projected coordinates are finite, so the point needs no check.
fn nurbs_curve_point_with(
    degree: u32,
    knots: &[f64],
    count: usize,
    pole: impl Fn(usize) -> Option<FinitePoint3>,
    weights: Option<&[f64]>,
    t: f64,
) -> Option<FinitePoint3> {
    let degree = usize::try_from(degree).ok()?;
    let span = bspline_span(knots, degree, count, t)?;
    let basis = bspline_basis(knots, degree, span, t)?;
    let poles = local_poles(span, degree, pole)?;
    let base = homogeneous_curve_sum(&basis, &poles, weights, span - degree)?;
    let [x, y, z] = finite_lanes(base.project(base, &[])?).ok()?;
    Some(FinitePoint3::from_coordinates(x, y, z))
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
    weights: Option<&[f64]>,
    first: usize,
) -> Option<Homogeneous> {
    Homogeneous::sum(values.iter().copied().enumerate().map(|(local, basis)| {
        Some((
            [basis, 1.0],
            weights
                .and_then(|weights| weights.get(first + local).copied())
                .unwrap_or(1.0),
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
        let position = nurbs_curve_point_with(
            curve.degree(),
            curve.knots(),
            poles.len(),
            |index| poles.get(index).copied(),
            Some(weights.as_ref()),
            parameter.get(),
        )?;
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
        let position = nurbs_curve_point_with(
            curve.degree(),
            curve.knots(),
            poles.len(),
            |index| poles.get(index).copied(),
            Some(weights),
            parameter.get(),
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
            curve.degree(),
            curve.knots(),
            &poles,
            Some(weights),
            parameter.get(),
        )?
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
pub fn map_nurbs_curve_parameter(curve: &NurbsCurve, parameter: f64) -> Option<FiniteReal> {
    let domain = nurbs_curve_parameter_domain(curve)?;
    let [lower, upper] = domain.endpoints();
    if !parameter.is_finite() {
        return None;
    }
    if !curve.periodic() && !(lower..=upper).contains(&parameter) {
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

/// Evaluate a possibly-rational B-spline curve over 2D `(u, v)` poles. The
/// point is absent when a coordinate is not finite.
pub fn nurbs_pcurve_uv(
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<FinitePoint2> {
    nurbs_pcurve_differential(degree, knots, control_points, weights, t)
        .ok()
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
    let sum = |values: &[f64]| homogeneous_curve_sum(values, &poles, weights, span - degree);
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
        let curve_uv = Point2::from(nurbs_pcurve_uv(
            degree,
            knots,
            control_points,
            Some(weights),
            middle,
        )?);
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

/// Evaluate a tensor-product NURBS surface at `(u, v)`. The point is absent
/// when a coordinate is not finite.
pub fn nurbs_surface_point(surface: &NurbsSurface, u_at: f64, v_at: f64) -> Option<FinitePoint3> {
    nurbs_surface_point_evaluation(surface, u_at, v_at).ok()
}

/// Evaluate a tensor-product NURBS surface at `(u, v)`, or report why it has
/// no finite point there.
///
/// A basis that leaves the finite range reaches no coordinate, and each
/// coordinate reads NaN; a projection that overflows carries each coordinate
/// it reached. A parameter that is not finite, a knot vector or pole net that
/// states no span at the parameter, and a zero weight sum have no value.
fn nurbs_surface_point_evaluation(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
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
        u_at,
    )
    .ok_or(no_value)?
    .get();
    let v_at = periodic_parameter(
        surface.v_knots(),
        v_degree,
        v_count,
        surface.v_periodic(),
        v_at,
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
    let base = Homogeneous::sum(
        u_basis
            .iter()
            .copied()
            .enumerate()
            .flat_map(|(i, u_value)| {
                v_basis
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
    .ok_or(no_value)?;
    let [x, y, z] = finite_lanes(base.project(base, &[]).ok_or(no_value)?)
        .map_err(|[x, y, z]| EvaluationFailure::NonFinite(Point3::new(x, y, z)))?;
    Ok(FinitePoint3::from_coordinates(x, y, z))
}

/// Evaluate a tensor-product NURBS surface at `(u, v)` within a caller-owned
/// work slice.
pub fn nurbs_surface_point_with_budget(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
    budget: &WorkBudget<'_>,
) -> Option<FinitePoint3> {
    nurbs_surface_point_with_budget_evaluation(surface, u_at, v_at, budget).ok()
}

/// [`nurbs_surface_point_evaluation`] within a caller-owned work slice. A
/// refused charge leaves no value.
fn nurbs_surface_point_with_budget_evaluation(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
    budget: &WorkBudget<'_>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let cost = nurbs_surface_evaluation_cost(surface).ok_or(EvaluationFailure::NoValue)?;
    if !budget.charge_by(cost) {
        return Err(EvaluationFailure::NoValue);
    }
    nurbs_surface_point_evaluation(surface, u_at, v_at)
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
        fixed_parameter,
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

/// Why a surface or pcurve evaluator has no finite value at its input.
///
/// [`NoValue`](Self::NoValue) states that there is no value: the input is
/// not a finite parameter, the carrier states no value there, or a step of
/// the evaluation is undefined, such as the angle of a polar chart at its
/// origin. An arm that reads a model curve through the curve evaluators,
/// which state no reason, also reports every failure of that curve this way.
///
/// [`NonFinite`](Self::NonFinite) states that the evaluation left the finite
/// range, and carries the value it reached. A coordinate that no step
/// reached is NaN. Where an evaluator forms derivatives together with its
/// value, a derivative that leaves the finite range leaves the evaluation
/// there, and the value it carries can be finite.
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
}

/// Admit an evaluated surface point, or report the non-finite point.
fn admit_surface_point(point: Point3) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
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

/// Evaluate a tensor-product NURBS surface and its exact rational first
/// partials at `(u, v)`.
pub fn nurbs_surface_partials(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Option<SurfacePartials<FinitePoint3, FiniteVector3>> {
    nurbs_surface_second_partials(surface, u_at, v_at).map(|partials| SurfacePartials {
        point: partials.point,
        du: partials.du,
        dv: partials.dv,
    })
}

/// Evaluate a tensor-product NURBS surface and its exact first partials within
/// a caller-owned work slice.
pub fn nurbs_surface_partials_with_budget(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
    budget: &WorkBudget<'_>,
) -> Option<SurfacePartials<FinitePoint3, FiniteVector3>> {
    budget
        .charge_by(nurbs_surface_partials_evaluation_cost(surface)?)
        .then_some(())?;
    nurbs_surface_partials(surface, u_at, v_at)
}

/// Evaluate a tensor-product NURBS surface and its exact rational first and
/// second partials at `(u, v)`.
pub fn nurbs_surface_second_partials(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Option<SurfaceSecondPartials<FinitePoint3, FiniteVector3>> {
    let u_degree = usize::try_from(surface.u_degree()).ok()?;
    let v_degree = usize::try_from(surface.v_degree()).ok()?;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let u_at = periodic_parameter(
        surface.u_knots(),
        u_degree,
        u_count,
        surface.u_periodic(),
        u_at,
    )?
    .get();
    let v_at = periodic_parameter(
        surface.v_knots(),
        v_degree,
        v_count,
        surface.v_periodic(),
        v_at,
    )?
    .get();
    let u_span = bspline_span(surface.u_knots(), u_degree, u_count, u_at)?;
    let v_span = bspline_span(surface.v_knots(), v_degree, v_count, v_at)?;
    let u_basis = bspline_basis(surface.u_knots(), u_degree, u_span, u_at)?;
    let v_basis = bspline_basis(surface.v_knots(), v_degree, v_span, v_at)?;
    let u_derivative = bspline_basis_derivative(surface.u_knots(), u_degree, u_span, u_at)?;
    let v_derivative = bspline_basis_derivative(surface.v_knots(), v_degree, v_span, v_at)?;
    let u_second = bspline_basis_second_derivative(surface.u_knots(), u_degree, u_span, u_at)?;
    let v_second = bspline_basis_second_derivative(surface.v_knots(), v_degree, v_span, v_at)?;
    let sum = |u_values: &[f64], v_values: &[f64]| {
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
    };
    let base = sum(&u_basis, &v_basis)?;
    let u = sum(&u_derivative, &v_basis)?;
    let v = sum(&u_basis, &v_derivative)?;
    let uu = sum(&u_second, &v_basis)?;
    let uv = sum(&u_derivative, &v_derivative)?;
    let vv = sum(&u_basis, &v_second)?;
    let point = finite_lanes(base.project(base, &[])?).ok()?;
    let du = finite_lanes(u.project(base, &[(u, point)])?).ok()?;
    let dv = finite_lanes(v.project(base, &[(v, point)])?).ok()?;
    let vector = |[x, y, z]: [FiniteReal; 3]| FiniteVector3::from_components(x, y, z);
    Some(SurfaceSecondPartials {
        point: FinitePoint3::from_coordinates(point[0], point[1], point[2]),
        du: vector(du),
        dv: vector(dv),
        duu: vector(finite_lanes(uu.project(base, &[(uu, point), (u, du), (u, du)])?).ok()?),
        duv: vector(finite_lanes(uv.project(base, &[(uv, point), (u, dv), (v, du)])?).ok()?),
        dvv: vector(finite_lanes(vv.project(base, &[(vv, point), (v, dv), (v, dv)])?).ok()?),
    })
}

/// Evaluate a tensor-product NURBS surface and its exact first and second
/// partials within a caller-owned work slice.
pub fn nurbs_surface_second_partials_with_budget(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
    budget: &WorkBudget<'_>,
) -> Option<SurfaceSecondPartials<FinitePoint3, FiniteVector3>> {
    budget
        .charge_by(nurbs_surface_partials_evaluation_cost(surface)?)
        .then_some(())?;
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
    parameter: f64,
) -> Option<FiniteReal> {
    let admitted = FiniteReal::new(parameter)?;
    let start = *knots.get(degree)?;
    let end = *knots.get(count)?;
    if !periodic || (start..=end).contains(&parameter) {
        return Some(admitted);
    }
    crate::math::wrap_parameter(parameter, start, end)
}

/// Evaluate the exact first derivative of a directly stored curve. The
/// derivative is absent when a component is not finite.
pub fn curve_tangent_solved(geometry: &SolvedCurveGeometry, t: f64) -> Option<FiniteVector3> {
    if !t.is_finite() {
        return None;
    }
    curve_tangent_inner(geometry, t)
}

/// Evaluate the exact second derivative of a directly stored curve. The
/// derivative is absent when a component is not finite.
pub fn curve_second_derivative_solved(
    geometry: &SolvedCurveGeometry,
    t: f64,
) -> Option<FiniteVector3> {
    if !t.is_finite() {
        return None;
    }
    curve_second_derivative_inner(geometry, t)
}

/// Evaluate a directly stored curve at `t` within a caller-owned work slice.
/// Analytic curves are constant-cost; transformed, polyline, and NURBS curves
/// charge the work performed by their representation.
pub fn curve_point_with_budget_solved(
    geometry: &SolvedCurveGeometry,
    t: f64,
    budget: &WorkBudget<'_>,
) -> Option<FinitePoint3> {
    /// The descent is bounded by [`PlacedCurve`](crate::geometry::PlacedCurve)
    /// construction; no arm follows an arena id.
    fn evaluate(
        geometry: &SolvedCurveGeometry,
        t: f64,
        budget: &WorkBudget<'_>,
    ) -> Option<FinitePoint3> {
        match geometry {
            SolvedCurveGeometry::Nurbs(nurbs) => {
                budget
                    .charge_by(nurbs_curve_evaluation_cost(nurbs)?)
                    .then_some(())?;
                curve_point_solved(geometry, t)
            }
            SolvedCurveGeometry::Polyline(polyline) => {
                budget.charge_by(polyline.point_count()).then_some(())?;
                curve_point_solved(geometry, t)
            }
            SolvedCurveGeometry::Transformed(placed) => {
                budget.charge().then_some(())?;
                evaluate(placed.basis(), t, budget)
                    .and_then(|point| placed.transform().apply_point(point.get()))
            }
            _ => curve_point_solved(geometry, t),
        }
    }

    evaluate(geometry, t, budget)
}

/// Evaluate the exact first derivative of a directly stored curve within a
/// caller-owned work slice.
pub fn curve_tangent_with_budget_solved(
    geometry: &SolvedCurveGeometry,
    t: f64,
    budget: &WorkBudget<'_>,
) -> Option<FiniteVector3> {
    /// The descent is bounded by [`PlacedCurve`](crate::geometry::PlacedCurve)
    /// construction; no arm follows an arena id.
    fn evaluate(
        geometry: &SolvedCurveGeometry,
        t: f64,
        budget: &WorkBudget<'_>,
    ) -> Option<FiniteVector3> {
        match geometry {
            SolvedCurveGeometry::Nurbs(nurbs) => {
                budget
                    .charge_by(nurbs_curve_derivative_evaluation_cost(nurbs, 2)?)
                    .then_some(())?;
                curve_tangent_solved(geometry, t)
            }
            SolvedCurveGeometry::Polyline(polyline) => {
                budget.charge_by(polyline.point_count()).then_some(())?;
                curve_tangent_solved(geometry, t)
            }
            SolvedCurveGeometry::Transformed(placed) => {
                budget.charge().then_some(())?;
                evaluate(placed.basis(), t, budget)
                    .and_then(|tangent| placed.transform().apply_vector(tangent.get()))
            }
            _ => curve_tangent_solved(geometry, t),
        }
    }

    evaluate(geometry, t, budget)
}

/// Evaluate the exact second derivative of a directly stored curve within a
/// caller-owned work slice.
pub fn curve_second_derivative_with_budget_solved(
    geometry: &SolvedCurveGeometry,
    t: f64,
    budget: &WorkBudget<'_>,
) -> Option<FiniteVector3> {
    /// The descent is bounded by [`PlacedCurve`](crate::geometry::PlacedCurve)
    /// construction; no arm follows an arena id.
    fn evaluate(
        geometry: &SolvedCurveGeometry,
        t: f64,
        budget: &WorkBudget<'_>,
    ) -> Option<FiniteVector3> {
        match geometry {
            SolvedCurveGeometry::Nurbs(nurbs) => {
                budget
                    .charge_by(nurbs_curve_derivative_evaluation_cost(nurbs, 3)?)
                    .then_some(())?;
                curve_second_derivative_solved(geometry, t)
            }
            SolvedCurveGeometry::Polyline(polyline) => {
                budget.charge_by(polyline.point_count()).then_some(())?;
                curve_second_derivative_solved(geometry, t)
            }
            SolvedCurveGeometry::Transformed(placed) => {
                budget.charge().then_some(())?;
                evaluate(placed.basis(), t, budget)
                    .and_then(|derivative| placed.transform().apply_vector(derivative.get()))
            }
            _ => curve_second_derivative_solved(geometry, t),
        }
    }

    evaluate(geometry, t, budget)
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

/// The descent is bounded by [`PlacedCurve`](crate::geometry::PlacedCurve)
/// construction; no arm follows an arena id.
fn curve_tangent_inner(geometry: &SolvedCurveGeometry, t: f64) -> Option<FiniteVector3> {
    match geometry {
        SolvedCurveGeometry::Line(line_curve) => Some(FiniteVector3::from(line_curve.direction())),
        SolvedCurveGeometry::Circle(circle_curve) => {
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius().get();
            FiniteVector3::new(vector_sum(&[
                (-radius * t.sin(), *ref_direction),
                (radius * t.cos(), axis.cross(*ref_direction)),
            ]))
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            FiniteVector3::new(vector_sum(&[
                (-major_radius * t.sin(), *major_direction),
                (minor_radius * t.cos(), axis.cross(*major_direction)),
            ]))
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let axis = parabola_curve.frame().axis().as_raw();
            let major_direction = parabola_curve.frame().reference().as_raw();
            let focal_distance = parabola_curve.focal_distance().get();
            FiniteVector3::new(vector_sum(&[
                (
                    product_quotient([2.0, focal_distance, t], [])?.get(),
                    *major_direction,
                ),
                (
                    product_quotient([2.0, focal_distance], [])?.get(),
                    axis.cross(*major_direction),
                ),
            ]))
        }
        SolvedCurveGeometry::Hyperbola(hyperbola_curve) => {
            let axis = hyperbola_curve.frame().axis().as_raw();
            let major_direction = hyperbola_curve.frame().reference().as_raw();
            let major_radius = hyperbola_curve.major_radius().magnitude();
            let minor_radius = Length::from(hyperbola_curve.minor_radius()).magnitude();
            let parameter = FiniteReal::new(t)?;
            FiniteVector3::new(vector_sum(&[
                (
                    scaled_sinh_cosh(major_radius, parameter).ok()?.0.get(),
                    *major_direction,
                ),
                (
                    scaled_sinh_cosh(minor_radius, parameter).ok()?.1.get(),
                    axis.cross(*major_direction),
                ),
            ]))
        }
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let parameter = map_nurbs_curve_parameter(nurbs, t)?.get();
            nurbs_curve_tangent(
                nurbs.degree(),
                nurbs.knots(),
                &nurbs.control_points(),
                nurbs.pole_rows().weights().as_deref(),
                parameter,
            )
        }
        SolvedCurveGeometry::Polyline(polyline) => {
            let (points, parameters) = polyline_samples(polyline);
            polyline_tangent(&points, &parameters, t)
        }
        SolvedCurveGeometry::Transformed(placed) => curve_tangent_inner(placed.basis(), t)
            .and_then(|tangent| placed.transform().apply_vector(tangent.get())),
        SolvedCurveGeometry::Degenerate(_) => None,
        SolvedCurveGeometry::Composite { .. } => None,
        SolvedCurveGeometry::Unknown { .. } => None,
    }
}

/// The descent is bounded by [`PlacedCurve`](crate::geometry::PlacedCurve)
/// construction; no arm follows an arena id.
fn curve_second_derivative_inner(geometry: &SolvedCurveGeometry, t: f64) -> Option<FiniteVector3> {
    match geometry {
        SolvedCurveGeometry::Line(_) => Some(FiniteVector3::ZERO),
        SolvedCurveGeometry::Circle(circle_curve) => {
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius().get();
            FiniteVector3::new(vector_sum(&[
                (-radius * t.cos(), *ref_direction),
                (-radius * t.sin(), axis.cross(*ref_direction)),
            ]))
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            FiniteVector3::new(vector_sum(&[
                (-major_radius * t.cos(), *major_direction),
                (-minor_radius * t.sin(), axis.cross(*major_direction)),
            ]))
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let major_direction = parabola_curve.frame().reference().as_raw();
            let focal_distance = parabola_curve.focal_distance().get();
            FiniteVector3::new(vector_sum(&[(
                product_quotient([2.0, focal_distance], [])?.get(),
                *major_direction,
            )]))
        }
        SolvedCurveGeometry::Hyperbola(hyperbola_curve) => {
            let axis = hyperbola_curve.frame().axis().as_raw();
            let major_direction = hyperbola_curve.frame().reference().as_raw();
            let major_radius = hyperbola_curve.major_radius().magnitude();
            let minor_radius = Length::from(hyperbola_curve.minor_radius()).magnitude();
            let parameter = FiniteReal::new(t)?;
            FiniteVector3::new(vector_sum(&[
                (
                    scaled_sinh_cosh(major_radius, parameter).ok()?.1.get(),
                    *major_direction,
                ),
                (
                    scaled_sinh_cosh(minor_radius, parameter).ok()?.0.get(),
                    axis.cross(*major_direction),
                ),
            ]))
        }
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let parameter = map_nurbs_curve_parameter(nurbs, t)?.get();
            nurbs_curve_second_derivative(
                nurbs.degree(),
                nurbs.knots(),
                &nurbs.control_points(),
                nurbs.pole_rows().weights().as_deref(),
                parameter,
            )
        }
        SolvedCurveGeometry::Polyline(polyline) => {
            let (points, parameters) = polyline_samples(polyline);
            polyline_tangent(&points, &parameters, t).map(|_| FiniteVector3::ZERO)
        }
        SolvedCurveGeometry::Transformed(placed) => {
            curve_second_derivative_inner(placed.basis(), t)
                .and_then(|derivative| placed.transform().apply_vector(derivative.get()))
        }
        SolvedCurveGeometry::Degenerate(_) => None,
        SolvedCurveGeometry::Composite { .. } => None,
        SolvedCurveGeometry::Unknown { .. } => None,
    }
}

fn nurbs_curve_tangent(
    degree: u32,
    knots: &[f64],
    control_points: &[FinitePoint3],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<FiniteVector3> {
    let degree = usize::try_from(degree).ok()?;
    let span = bspline_span(knots, degree, control_points.len(), t)?;
    let basis = bspline_basis(knots, degree, span, t)?;
    let mut derivatives = bspline_basis_derivative(knots, degree, span, t)?;
    let scale = if derivatives.iter().all(|value| value.is_finite()) {
        PositiveReal::ONE
    } else {
        let scale = PositiveReal::new(knots[span + 1] - knots[span])?;
        if degree == 1 {
            return finite_lanes(linear_nurbs_derivative(
                &basis,
                control_points,
                weights,
                span,
                scale.get(),
                false,
            )?)
            .ok()
            .map(|[x, y, z]| FiniteVector3::from_components(x, y, z));
        }
        derivatives = bspline_basis_scaled_derivatives(knots, degree, span, t, scale)?.0;
        scale
    };
    let poles = local_poles(span, degree, |index| control_points.get(index).copied())?;
    let sum = |values: &[f64]| homogeneous_curve_sum(values, &poles, weights, span - degree);
    let base = sum(&basis)?;
    let derivative = sum(&derivatives)?;
    let point = finite_lanes(base.project(base, &[])?).ok()?;
    let [x, y, z] = finite_lanes(derivative.project(base, &[(derivative, point)])?).ok()?;
    let unscale = |value: FiniteReal| {
        if scale.get() == 1.0 {
            Some(value)
        } else {
            difference_quotient(value, FiniteReal::ZERO, scale.into(), FiniteReal::ZERO).ok()
        }
    };
    Some(FiniteVector3::from_components(
        unscale(x)?,
        unscale(y)?,
        unscale(z)?,
    ))
}

fn nurbs_curve_second_derivative(
    degree: u32,
    knots: &[f64],
    control_points: &[FinitePoint3],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<FiniteVector3> {
    let degree = usize::try_from(degree).ok()?;
    let span = bspline_span(knots, degree, control_points.len(), t)?;
    let basis = bspline_basis(knots, degree, span, t)?;
    let mut first_basis = bspline_basis_derivative(knots, degree, span, t)?;
    let mut second_basis = bspline_basis_second_derivative(knots, degree, span, t)?;
    let scale = if first_basis
        .iter()
        .chain(&second_basis)
        .all(|value| value.is_finite())
    {
        PositiveReal::ONE
    } else {
        let scale = PositiveReal::new(knots[span + 1] - knots[span])?;
        if degree == 1 {
            return finite_lanes(linear_nurbs_derivative(
                &basis,
                control_points,
                weights,
                span,
                scale.get(),
                true,
            )?)
            .ok()
            .map(|[x, y, z]| FiniteVector3::from_components(x, y, z));
        }
        (first_basis, second_basis) =
            bspline_basis_scaled_derivatives(knots, degree, span, t, scale)?;
        scale
    };
    let poles = local_poles(span, degree, |index| control_points.get(index).copied())?;
    let sum = |values: &[f64]| homogeneous_curve_sum(values, &poles, weights, span - degree);
    let base = sum(&basis)?;
    let first_sum = sum(&first_basis)?;
    let second_sum = sum(&second_basis)?;
    let point = finite_lanes(base.project(base, &[])?).ok()?;
    let first = finite_lanes(first_sum.project(base, &[(first_sum, point)])?).ok()?;
    let [x, y, z] = finite_lanes(second_sum.project(
        base,
        &[(second_sum, point), (first_sum, first), (first_sum, first)],
    )?)
    .ok()?;
    let unscale = |value: FiniteReal| {
        if scale.get() == 1.0 {
            Some(value)
        } else {
            difference_quotient(
                difference_quotient(value, FiniteReal::ZERO, scale.into(), FiniteReal::ZERO)
                    .ok()?,
                FiniteReal::ZERO,
                scale.into(),
                FiniteReal::ZERO,
            )
            .ok()
        }
    };
    Some(FiniteVector3::from_components(
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
/// procedural constructions.
pub fn model_curve_point_by_id(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
) -> Option<FinitePoint3> {
    model_curve_point_by_id_inner(index, curve_id, parameter, 0, None)
}

/// Evaluate a model curve carrier within a caller-owned work slice. Carrier
/// recursion and direct NURBS or polyline work consume the supplied budget.
pub fn model_curve_point_by_id_with_budget(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
    budget: &WorkBudget<'_>,
) -> Option<FinitePoint3> {
    let _guard = budget.recursion_guard()?;
    model_curve_point_by_id_inner(index, curve_id, parameter, 0, Some(budget))
}

#[derive(Clone, Copy)]
struct ModelCurveDifferential {
    point: FinitePoint3,
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
    let ProceduralCurveDefinition::Helix(helix_payload) = definition else {
        return None;
    };
    let [start, end] = helix_payload.angle_range().finite_components();
    let center = helix_payload.center().get();
    let major = helix_payload.major().get();
    let minor = helix_payload.minor().get();
    let pitch = helix_payload.pitch().get();
    let apex_factor = helix_payload.apex_factor().get();
    let axis = helix_payload.axis().get();
    let finite_parameter = FiniteReal::new(parameter)?;
    if finite_parameter < start || finite_parameter > end || unit_axis(axis).is_none() {
        return None;
    }

    let inverse_revolution = 1.0 / std::f64::consts::TAU;
    let revolution_fraction =
        difference_quotient(finite_parameter, start, FiniteReal::TAU, FiniteReal::ZERO)
            .ok()?
            .get();
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
    let point = FinitePoint3::new(point)?;
    (tangent.is_finite() && acceleration.is_finite()).then_some(ModelCurveDifferential {
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
    model_curve_differential_by_id_inner(index, curve_id, parameter, 0, None)
}

fn model_curve_differential_by_id_with_budget(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
    budget: &WorkBudget<'_>,
) -> Option<ModelCurveDifferential> {
    model_curve_differential_by_id_inner(index, curve_id, parameter, 0, Some(budget))
}

fn model_curve_differential_by_id_inner(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
    depth: usize,
    budget: Option<&WorkBudget<'_>>,
) -> Option<ModelCurveDifferential> {
    if depth > 256 || !parameter.is_finite() {
        return None;
    }
    let curve = index.curves(curve_id.as_str())?;
    if let Some(budget) = budget {
        budget.charge().then_some(())?;
    }
    if let Some(procedural) = index
        .procedural_curves_for_curve(curve_id.as_str())
        .and_then(|procedurals| procedurals.first().copied())
    {
        match procedural.definition() {
            ProceduralCurveDefinition::Replica { source, transform } => {
                let differential = model_curve_differential_by_id_inner(
                    index,
                    source,
                    parameter,
                    depth + 1,
                    budget,
                )?;
                return Some(ModelCurveDifferential {
                    point: transform.apply_point(differential.point.get())?,
                    tangent: transform.apply_vector(differential.tangent)?.get(),
                    acceleration: transform.apply_vector(differential.acceleration)?.get(),
                });
            }
            ProceduralCurveDefinition::Subset(definition_payload) => {
                let source = definition_payload.source();
                let [start, end] = definition_payload.parameter_range().endpoints();
                let sense = definition_payload.sense();
                {
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
                        budget,
                    )?;
                    let parameter_scale = if *sense { 1.0 } else { -1.0 };
                    return Some(ModelCurveDifferential {
                        point: differential.point,
                        tangent: scale_vector(differential.tangent, parameter_scale),
                        acceleration: differential.acceleration,
                    });
                }
            }
            ProceduralCurveDefinition::Helix(_) => {
                return helix_differential(procedural.definition(), parameter);
            }
            _ => {}
        }
    }
    if let Some(cache) = curve.geometry.solved_cache() {
        return Some(ModelCurveDifferential {
            point: budget.map_or_else(
                || curve_point_solved(cache, parameter),
                |budget| curve_point_with_budget_solved(cache, parameter, budget),
            )?,
            tangent: curve_tangent_solved(cache, parameter)?.get(),
            acceleration: curve_second_derivative_solved(cache, parameter)?.get(),
        });
    }
    if matches!(&curve.geometry, CurveGeometry::Procedural { .. }) {
        return None;
    }
    if let Some(budget) = budget {
        return Some(ModelCurveDifferential {
            point: curve_point_with_budget(&curve.geometry, parameter, budget)?,
            tangent: curve_tangent_with_budget(&curve.geometry, parameter, budget)?.get(),
            acceleration: curve_second_derivative_with_budget(&curve.geometry, parameter, budget)?
                .get(),
        });
    }
    Some(ModelCurveDifferential {
        point: curve_point(&curve.geometry, parameter)?,
        tangent: curve_tangent(&curve.geometry, parameter)?.get(),
        acceleration: curve_second_derivative(&curve.geometry, parameter)?.get(),
    })
}

/// The admitted revolution axis scaled by the reciprocal of its length. The
/// admission holds the length within `1e-9` of one without rescaling, and the
/// rotation needs unit length to rounding.
fn unit_length_axis(direction: UnitVector3) -> Vector3 {
    let direction = *direction.as_raw();
    scale_vector(direction, 1.0 / direction.norm())
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
    axis_direction: UnitVector3,
    angle: f64,
    parameter: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Option<Point3> {
    if !angle.is_finite() {
        return None;
    }
    let axis = unit_length_axis(axis_direction);
    let point = budget.map_or_else(
        || model_curve_point_by_id(index, directrix, parameter),
        |budget| model_curve_point_by_id_with_budget(index, directrix, parameter, budget),
    )?;
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
    axis_direction: UnitVector3,
    angle: f64,
    parameter: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Option<SurfaceSecondPartials> {
    if !angle.is_finite() {
        return None;
    }
    let axis = unit_length_axis(axis_direction);
    let differential = budget.map_or_else(
        || model_curve_differential_by_id(index, directrix, parameter),
        |budget| model_curve_differential_by_id_with_budget(index, directrix, parameter, budget),
    )?;
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
fn record_u_interval(record_bounds: Option<crate::geometry::RecordBounds>) -> Option<[f64; 2]> {
    let [Some(start), Some(end), _, _] = record_bounds?.get() else {
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

fn construction_curve_parameter(
    index: &crate::index::ModelIndex<'_>,
    directrix: &crate::ids::CurveId,
    parameter: f64,
    surface_interval: Option<[FiniteReal; 2]>,
    carrier_interval: Option<[f64; 2]>,
    reversed: bool,
) -> Option<(FiniteReal, FiniteReal)> {
    let parameter = FiniteReal::new(parameter)?;
    // The carrier interval is a record pair, so its endpoints are admitted
    // here.
    let carrier_interval = match carrier_interval {
        Some(interval) => Some(FiniteReal::array(interval)?),
        None => None,
    };
    // The surface width is admitted positive where it is formed, and only
    // the arms that hold a surface interval form it.
    let positive_width = |start: FiniteReal, end: FiniteReal| {
        FiniteReal::new(end.get() - start.get()).filter(|width| width.get() > 0.0)
    };
    let (parameter, surface_derivative, surface_width) = match (surface_interval, carrier_interval)
    {
        (Some([surface_start, surface_end]), Some([carrier_start, carrier_end])) => {
            let surface_width = positive_width(surface_start, surface_end)?;
            let carrier_width = carrier_end.get() - carrier_start.get();
            if !carrier_width.is_finite()
                || carrier_width <= 0.0
                || parameter.get() < carrier_start.get()
                || parameter.get() > carrier_end.get()
            {
                return None;
            }
            let derivative = FiniteReal::new(surface_width.get() / carrier_width)?;
            let source_parameter = FiniteReal::new(
                (parameter.get() - carrier_start.get())
                    .mul_add(derivative.get(), surface_start.get()),
            )?;
            (source_parameter, derivative, Some(surface_width))
        }
        (Some([surface_start, surface_end]), None) => {
            let surface_width = positive_width(surface_start, surface_end)?;
            if parameter.get() < surface_start.get() || parameter.get() > surface_end.get() {
                return None;
            }
            (parameter, FiniteReal::ONE, Some(surface_width))
        }
        (None, Some([carrier_start, carrier_end])) => {
            if carrier_start.get() >= carrier_end.get()
                || parameter.get() < carrier_start.get()
                || parameter.get() > carrier_end.get()
            {
                return None;
            }
            (parameter, FiniteReal::ONE, None)
        }
        (None, None) => (parameter, FiniteReal::ONE, None),
    };
    let curve = index.curves(directrix.as_str())?;
    let (Some([surface_start, surface_end]), Some(surface_width)) =
        (surface_interval, surface_width)
    else {
        return if reversed {
            Some((parameter.negated(), surface_derivative.negated()))
        } else {
            Some((parameter, surface_derivative))
        };
    };
    if !curve.geometry.solved().is_some_and(is_line_geometry) {
        return if reversed {
            Some((parameter.negated(), surface_derivative.negated()))
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
            .filter(|edge| edge.curve() == Some(directrix))
            .filter_map(crate::topology::Edge::param_range);
        let interval = ranges.next()?;
        if ranges.any(|range| range != interval) {
            return None;
        }
        interval.finite_components()
    };
    let [curve_start, curve_end] = line_interval;
    let curve_width = positive_width(curve_start, curve_end)?;
    let [derivative] = scaled_ratio_products(curve_width, surface_width, [surface_derivative])?;
    let derivative = if reversed {
        derivative.negated()
    } else {
        derivative
    };
    let fraction = if reversed {
        (surface_end.get() - parameter.get()) / surface_width.get()
    } else {
        (parameter.get() - surface_start.get()) / surface_width.get()
    };
    let parameter = FiniteReal::new(curve_start.get() + fraction * curve_width.get())?;
    Some((parameter, derivative))
}

// The native and carrier parameter intervals are independent serialized semantics;
// the revision reversal affects the derivative mapping, and the optional budget
// must remain explicit across recursive evaluation.
fn model_native_extrusion_partials(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::ExtrusionSurfaceConstruction,
    carrier_interval: Option<[f64; 2]>,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    if !v.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let directrix = construction.directrix();
    let direction = *construction.direction();
    let (parameter, derivative) = construction_curve_parameter(
        index,
        directrix,
        u,
        construction
            .parameter_interval()
            .map(FiniteVector::finite_components),
        carrier_interval,
        extrusion_directrix_reversed(construction.revision_form()),
    )
    .ok_or(EvaluationFailure::NoValue)?;
    let differential = budget
        .map_or_else(
            || model_curve_differential_by_id(index, directrix, parameter.get()),
            |budget| {
                model_curve_differential_by_id_with_budget(
                    index,
                    directrix,
                    parameter.get(),
                    budget,
                )
            },
        )
        .ok_or(EvaluationFailure::NoValue)?;
    // A product outside the finite range reaches its signed infinity; a
    // component that is not finite reaches no product.
    let squared_derivative_component =
        |component| match crate::math::sum::product_sum(std::iter::once(Some([
            derivative.get(),
            derivative.get(),
            component,
        ]))) {
            crate::math::sum::ProductSum::Zero => 0.0,
            crate::math::sum::ProductSum::Value(value) => value
                .finite()
                .map_or_else(|reached| reached, FiniteReal::get),
            crate::math::sum::ProductSum::Undefined => f64::NAN,
        };
    let zero = Vector3::new(0.0, 0.0, 0.0);
    admitted_partials(SurfaceSecondPartials {
        point: offset(differential.point.get(), &[(v, direction.get())]),
        du: scale_vector(differential.tangent, derivative.get()),
        dv: direction.get(),
        duu: Vector3::new(
            squared_derivative_component(differential.acceleration.x),
            squared_derivative_component(differential.acceleration.y),
            squared_derivative_component(differential.acceleration.z),
        ),
        duv: zero,
        dvv: zero,
    })
}

/// Partials an arm forms raw, admitted together: finite when every lane is.
/// Otherwise the evaluation left the finite range, and it carries the point
/// it reached, which is finite where only a derivative left the range.
fn admitted_partials(
    partials: SurfaceSecondPartials,
) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    let admit = || {
        Some(SurfaceSecondPartials {
            point: FinitePoint3::new(partials.point)?,
            du: FiniteVector3::new(partials.du)?,
            dv: FiniteVector3::new(partials.dv)?,
            duu: FiniteVector3::new(partials.duu)?,
            duv: FiniteVector3::new(partials.duv)?,
            dvv: FiniteVector3::new(partials.dvv)?,
        })
    };
    admit().ok_or(EvaluationFailure::NonFinite(partials.point))
}

fn extrusion_directrix_reversed(
    revision_form: Option<&crate::geometry::RevisionSurfaceForm<Vec<bool>, FiniteReal>>,
) -> bool {
    revision_form
        .and_then(|form| form.flags.first())
        .copied()
        .unwrap_or(false)
}

fn model_native_revolution_partials(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::RevolutionSurfaceConstruction,
    carrier_interval: Option<[f64; 2]>,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Option<SurfaceSecondPartials> {
    let directrix = construction.directrix();
    let angular_interval = construction.angular_interval().endpoints();
    let transposed = *construction.transposed();
    let (directrix_parameter, angular_parameter) = if transposed { (v, u) } else { (u, v) };
    let (directrix_parameter, derivative) = construction_curve_parameter(
        index,
        directrix,
        directrix_parameter,
        construction
            .parameter_interval()
            .map(crate::topology::IncreasingParameterInterval::finite_endpoints),
        carrier_interval,
        false,
    )?;
    let (directrix_parameter, derivative) = (directrix_parameter.get(), derivative.get());
    let (angle, angular_derivative) = construction.angular_parameter_interval().map_or_else(
        || Some((angular_parameter, 1.0)),
        |parameter_interval| {
            // The admitted interval is finite and strictly increasing, so
            // its span is positive.
            let parameter_interval = parameter_interval.endpoints();
            let parameter_span = parameter_interval[1] - parameter_interval[0];
            let angular_span = angular_interval[1] - angular_interval[0];
            if !angular_span.is_finite() {
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
        construction.axis_origin().get(),
        construction.axis_direction(),
        angle,
        directrix_parameter,
        budget,
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
    budget: Option<&WorkBudget<'_>>,
) -> Option<FinitePoint3> {
    if depth > 256 {
        return None;
    }
    let curve = index.curves(curve_id.as_str())?;
    if let Some(budget) = budget {
        budget.charge().then_some(())?;
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
        ProceduralCurveDefinition::Replica { source, transform } => {
            model_curve_point_by_id_inner(index, source, parameter, depth + 1, budget)
                .and_then(|point| transform.apply_point(point.get()))
        }
        ProceduralCurveDefinition::Subset(definition_payload) => {
            let source = definition_payload.source();
            let [start, end] = definition_payload.parameter_range().endpoints();
            let sense = definition_payload.sense();
            {
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
                model_curve_point_by_id_inner(index, source, source_parameter, depth + 1, budget)
            }
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
                return None;
            }
            let points = std::array::from_fn(|side| {
                // A non-finite offset-pcurve point is evaluated on its support
                // as a finite one is.
                let uv = match pcurve_uv(&parameterization.pcurves[side], parameter) {
                    Ok(uv) => uv.get(),
                    Err(failure) => failure.non_finite()?,
                };
                budget
                    .map_or_else(
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
                    .ok()
            });
            let [Some(first), Some(second)] = points else {
                return None;
            };
            let separation = first.distance(second.get());
            (separation.is_finite() && separation <= tolerance).then_some(first)
        }
        _ => {
            if let Some(cache) = curve.geometry.solved_cache() {
                budget.map_or_else(
                    || curve_point_solved(cache, parameter),
                    |budget| curve_point_with_budget_solved(cache, parameter, budget),
                )
            } else if matches!(&curve.geometry, CurveGeometry::Procedural { .. }) {
                None
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
        0,
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
        0,
    )
}

/// Invert a model curve with an admitted tolerance.
fn model_curve_parameter_near_point_with_tolerance(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    point: Point3,
    seed: f64,
    tolerance: NonNegativeLength,
    depth: usize,
) -> Option<FiniteReal> {
    if depth > 256 {
        return None;
    }
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
                    depth + 1,
                );
            }
            ProceduralCurveDefinition::Subset(definition_payload) => {
                let source = definition_payload.source();
                let [start, end] = definition_payload.parameter_range().endpoints();
                let sense = definition_payload.sense();
                {
                    let span = (end - start).abs();
                    if !seed.is_finite()
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
                    )?
                    .get();
                    let parameter = FiniteReal::new(if *sense {
                        source_parameter - start
                    } else {
                        end - source_parameter
                    })?;
                    return (parameter.get() >= 0.0
                        && parameter.get() <= span
                        && model_curve_point_by_id(index, curve_id, parameter.get()).is_some_and(
                            |evaluated| evaluated.distance(point) <= tolerance.get(),
                        ))
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
        let Some(evaluated) = model_curve_point_by_id(index, curve_id, parameter.get()) else {
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
        let differential = model_curve_differential_by_id(index, curve_id, parameter.get())?;
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
        let Some(next) =
            FiniteReal::new(parameter.get() - residual.dot(differential.tangent) / denominator)
        else {
            break;
        };
        let next = angles.project(ExtendedReal::from_finite(next));
        if next == parameter {
            break;
        }
        parameter = next;
    }

    let differential = model_curve_differential_by_id(index, curve_id, parameter.get())?;
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
    let evaluated = curve_point_solved(geometry, parameter.get())?;
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

/// Evaluate a 3D curve carrier at parameter `t` on its own parameterization.
/// The point is absent when a coordinate is not finite.
///
/// The descent is bounded by [`PlacedCurve`](crate::geometry::PlacedCurve)
/// construction; no arm follows an arena id.
pub fn curve_point_solved(geometry: &SolvedCurveGeometry, t: f64) -> Option<FinitePoint3> {
    match geometry {
        SolvedCurveGeometry::Line(line_curve) => {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            FinitePoint3::new(offset(origin, &[(t, direction)]))
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius().get();
            FinitePoint3::new(offset(
                center,
                &[
                    (radius * t.cos(), *ref_direction),
                    (radius * t.sin(), axis.cross(*ref_direction)),
                ],
            ))
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            FinitePoint3::new(offset(
                center,
                &[
                    (major_radius * t.cos(), *major_direction),
                    (minor_radius * t.sin(), axis.cross(*major_direction)),
                ],
            ))
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let vertex = parabola_curve.vertex().get();
            let axis = parabola_curve.frame().axis().as_raw();
            let major_direction = parabola_curve.frame().reference().as_raw();
            let focal_distance = parabola_curve.focal_distance().get();
            FinitePoint3::new(offset(
                vertex,
                &[
                    (
                        product_quotient([focal_distance, t, t], [])?.get(),
                        *major_direction,
                    ),
                    (
                        product_quotient([2.0, focal_distance, t], [])?.get(),
                        axis.cross(*major_direction),
                    ),
                ],
            ))
        }
        SolvedCurveGeometry::Hyperbola(hyperbola_curve) => {
            let center = hyperbola_curve.center().get();
            let axis = hyperbola_curve.frame().axis().as_raw();
            let major_direction = hyperbola_curve.frame().reference().as_raw();
            let major_radius = hyperbola_curve.major_radius().magnitude();
            let minor_radius = Length::from(hyperbola_curve.minor_radius()).magnitude();
            let parameter = FiniteReal::new(t)?;
            FinitePoint3::new(offset(
                center,
                &[
                    (
                        scaled_sinh_cosh(major_radius, parameter).ok()?.1.get(),
                        *major_direction,
                    ),
                    (
                        scaled_sinh_cosh(minor_radius, parameter).ok()?.0.get(),
                        axis.cross(*major_direction),
                    ),
                ],
            ))
        }
        SolvedCurveGeometry::Degenerate(degenerate_curve) => Some(degenerate_curve.point()),
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let parameter = map_nurbs_curve_parameter(nurbs, t)?.get();
            let poles = nurbs.control_points();
            nurbs_curve_point_with(
                nurbs.degree(),
                nurbs.knots(),
                poles.len(),
                |index| poles.get(index).copied(),
                nurbs.pole_rows().weights().as_deref(),
                parameter,
            )
        }
        SolvedCurveGeometry::Polyline(polyline) => {
            let (points, parameters) = polyline_samples(polyline);
            polyline_point(&points, &parameters, t)
        }
        SolvedCurveGeometry::Transformed(placed) => curve_point_solved(placed.basis(), t)
            .and_then(|point| placed.transform().apply_point(point.get())),
        SolvedCurveGeometry::Composite { .. } | SolvedCurveGeometry::Unknown { .. } => None,
    }
}

/// Evaluate a surface carrier at `(u, v)` on its own parameterization: `u` is
/// the azimuth angle and `v` the axial distance / polar angle on analytic
/// quadrics, and both are knot-domain parameters on NURBS surfaces.
pub fn surface_point_solved(
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    // A NURBS or placed carrier evaluates its partials with its point, so a
    // partial outside the finite range leaves the evaluation there too.
    match geometry {
        SolvedSurfaceGeometry::Nurbs(nurbs) => nurbs_surface_second_partials(nurbs, u, v)
            .map(|partials| partials.point)
            .ok_or_else(|| failure_at(nurbs_surface_point_evaluation(nurbs, u, v))),
        SolvedSurfaceGeometry::Transformed(placed) => {
            transformed_surface_second_partials(placed, u, v)
                .map(|partials| partials.point)
                .ok_or_else(|| {
                    failure_at(placed_point(
                        *placed.transform(),
                        surface_point_solved(placed.basis(), u, v),
                    ))
                })
        }
        _ => {
            let partials =
                surface_second_partials_solved(geometry, u, v).ok_or(EvaluationFailure::NoValue)?;
            admit_surface_point(partials.point)
        }
    }
}

/// The failure of an evaluation whose partials failed, from its point alone:
/// a point that is finite states that a partial left the finite range.
fn failure_at(point: Result<FinitePoint3, EvaluationFailure<Point3>>) -> EvaluationFailure<Point3> {
    match point {
        Ok(point) => EvaluationFailure::NonFinite(point.get()),
        Err(failure) => failure,
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

/// The second partials of a placed carrier: its basis partials under the
/// placement. A non-finite basis lane, and a lane the placement leaves
/// non-finite, leave the carrier without partials.
fn transformed_surface_second_partials(
    placed: &crate::geometry::PlacedSurface,
    u: f64,
    v: f64,
) -> Option<SurfaceSecondPartials<FinitePoint3, FiniteVector3>> {
    transform_surface_second_partials(
        surface_second_partials_solved(placed.basis(), u, v)?,
        *placed.transform(),
    )
}

/// Evaluate a directly stored surface at `(u, v)` within a caller-owned work
/// slice. Analytic surfaces are constant-cost; transformed carriers charge
/// each transform layer and NURBS carriers charge their local basis work.
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
        SolvedSurfaceGeometry::Plane(plane_surface) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            let v_axis = normal.cross(*u_axis);
            admit_surface_point(offset(origin, &[(u, *u_axis), (v, v_axis)]))
        }
        SolvedSurfaceGeometry::Cylinder(cylinder_surface) => {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let ref_direction = cylinder_surface.frame().reference().as_raw();
            let radius = cylinder_surface.radius().get();
            let transverse = axis.cross(*ref_direction);
            let cosine = u.cos();
            let sine = u.sin();
            admit_surface_point(offset(
                origin,
                &[
                    (radius * cosine, *ref_direction),
                    (radius * sine, transverse),
                    (v, *axis),
                ],
            ))
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
            admit_surface_point(offset(
                origin,
                &[
                    (local_radius * cosine, *ref_direction),
                    (local_radius * ratio * sine, transverse),
                    (v, *axis),
                ],
            ))
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
            admit_surface_point(offset(
                center,
                &[
                    (radius * v_cosine * u_cosine, *ref_direction),
                    (radius * v_cosine * u_sine, transverse),
                    (radius * v_sine, *axis),
                ],
            ))
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
            admit_surface_point(offset(
                center,
                &[
                    (ring * u_cosine, *ref_direction),
                    (ring * u_sine, transverse),
                    (minor_radius * v_sine, *axis),
                ],
            ))
        }
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            nurbs_surface_point_with_budget_evaluation(nurbs, u, v, budget)
        }
        SolvedSurfaceGeometry::Transformed(placed) => {
            if !budget.charge() {
                return Err(EvaluationFailure::NoValue);
            }
            placed_point(
                *placed.transform(),
                surface_point_with_budget_solved(placed.basis(), u, v, budget),
            )
        }
        SolvedSurfaceGeometry::Polygonal(_) | SolvedSurfaceGeometry::Unknown { .. } => {
            Err(EvaluationFailure::NoValue)
        }
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
pub fn rolling_ball_jet_point(
    definition: &ProceduralSurfaceDefinition,
    t: f64,
    s: f64,
) -> Option<FinitePoint3> {
    let ProceduralSurfaceDefinition::RollingBallJet(jet) = definition else {
        return None;
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
        return None;
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
        return None;
    }
    let span = stations
        .windows(2)
        .position(|pair| t >= pair[0].knot.get() && t <= pair[1].knot.get())?;
    let span_width = stations[span + 1].knot.get() - stations[span].knot.get();
    if !span_width.is_finite() {
        return None;
    }
    let fraction = ((t - stations[span].knot.get()) / span_width).clamp(0.0, 1.0);
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
        span_width,
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
        span_width,
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
        span_width,
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
        span_width,
    );
    if !angle.is_finite() {
        return None;
    }
    let first_radius = first_limit.vector_from(center);
    let first_direction = first_radius.scale(1.0 / radius);
    let first_direction_squared = first_direction.dot(first_direction);
    if !first_direction_squared.is_finite() || first_direction_squared <= f64::EPSILON {
        return None;
    }
    let second_radius = second_limit.vector_from(center);
    let second_direction = FiniteVector3::new(
        second_radius
            - first_direction.scale(second_radius.dot(first_direction) / first_direction_squared),
    )?
    .unit_nonzero()?;
    let radial =
        first_direction.scale((s * angle).cos()) + second_direction.scale((s * angle).sin());
    FinitePoint3::new(center.translated(radial, radius))
}

fn rolling_ball_jet_interpolate_point(
    values: [Point3; 2],
    first_derivatives: [Vector3; 2],
    second_derivatives: [Vector3; 2],
    fraction: f64,
    span_width: f64,
) -> Point3 {
    Point3::new(
        rolling_ball_jet_interpolate_scalar(
            [values[0].x, values[1].x],
            [first_derivatives[0].x, first_derivatives[1].x],
            [second_derivatives[0].x, second_derivatives[1].x],
            fraction,
            span_width,
        ),
        rolling_ball_jet_interpolate_scalar(
            [values[0].y, values[1].y],
            [first_derivatives[0].y, first_derivatives[1].y],
            [second_derivatives[0].y, second_derivatives[1].y],
            fraction,
            span_width,
        ),
        rolling_ball_jet_interpolate_scalar(
            [values[0].z, values[1].z],
            [first_derivatives[0].z, first_derivatives[1].z],
            [second_derivatives[0].z, second_derivatives[1].z],
            fraction,
            span_width,
        ),
    )
}

fn rolling_ball_jet_interpolate_scalar(
    values: [f64; 2],
    first_derivatives: [f64; 2],
    second_derivatives: [f64; 2],
    fraction: f64,
    span_width: f64,
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
            Some([first_derivatives[0], span_width, h10, 1.0]),
            Some([second_derivatives[0], span_width, span_width, h20]),
            Some([values[1], h01, 1.0, 1.0]),
            Some([first_derivatives[1], span_width, h11, 1.0]),
            Some([second_derivatives[1], span_width, span_width, h21]),
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

/// Evaluate a directly stored surface and its exact first partial derivatives.
pub fn surface_partials_solved(
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
) -> Option<SurfacePartials> {
    surface_second_partials_solved(geometry, u, v).map(|partials| SurfacePartials {
        point: partials.point,
        du: partials.du,
        dv: partials.dv,
    })
}

/// Evaluate a directly stored surface and its exact first and second partial
/// derivatives.
///
/// The descent is bounded by [`PlacedSurface`](crate::geometry::PlacedSurface)
/// construction; no arm follows an arena id.
pub fn surface_second_partials_solved(
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
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            nurbs_surface_second_partials(nurbs, u, v).map(SurfaceSecondPartials::into_raw)
        }
        SolvedSurfaceGeometry::Transformed(placed) => {
            transformed_surface_second_partials(placed, u, v).map(SurfaceSecondPartials::into_raw)
        }
        SolvedSurfaceGeometry::Polygonal(_) | SolvedSurfaceGeometry::Unknown { .. } => None,
    }
}

/// Evaluate a surface carrier with access to construction and child-carrier
/// arenas in `ir`.
pub fn model_surface_point(
    ir: &CadIr,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
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
    let point = match procedural.definition() {
        ProceduralSurfaceDefinition::Extrusion(definition_payload) => {
            return model_native_extrusion_partials(
                &index,
                definition_payload,
                carrier_interval,
                u,
                v,
                None,
            )
            .map(|partials| partials.point);
        }
        ProceduralSurfaceDefinition::LinearSweep(definition_payload) => {
            model_curve_point_by_id(&index, definition_payload.directrix(), u)
                .map(|point| offset(point.get(), &[(v, definition_payload.direction().get())]))
        }
        ProceduralSurfaceDefinition::Revolution(definition_payload) => {
            model_native_revolution_partials(
                &index,
                definition_payload,
                carrier_interval,
                u,
                v,
                None,
            )
            .map(|partials| partials.point)
        }
        ProceduralSurfaceDefinition::AxisRevolution(definition_payload) => {
            model_axis_revolution_point(
                &index,
                definition_payload.directrix(),
                definition_payload.axis_origin().get(),
                definition_payload.axis_direction(),
                u,
                v,
                None,
            )
        }
        ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
            return model_ruled_surface_partials(&index, first, second, u, v)
                .map(|partials| partials.point);
        }
        ProceduralSurfaceDefinition::Sum(definition_payload) => {
            return model_sum_surface_partials(&index, definition_payload, u, v)
                .map(|partials| partials.point);
        }
        ProceduralSurfaceDefinition::Sweep(definition_payload) => {
            if let Some(construction) = definition_payload.native() {
                let profile = definition_payload.profile();
                let spine = definition_payload.spine();

                cacheless_law_sweep_point(&index, profile, spine, construction, u, v)
            } else {
                None
            }
        }
        ProceduralSurfaceDefinition::VariableBlend(definition_payload) => {
            let construction = definition_payload.construction();

            cacheless_variable_blend_point(&index, construction, u, v)
        }
        ProceduralSurfaceDefinition::Blend(definition_payload) => {
            if let Some(native) = definition_payload.native() {
                let supports = definition_payload.supports();
                let radius = definition_payload.radius();
                let cross_section = definition_payload.cross_section();

                cacheless_constant_rolling_ball_point(
                    &index,
                    supports,
                    radius,
                    cross_section,
                    native,
                    u,
                    v,
                )
            } else {
                None
            }
        }
        ProceduralSurfaceDefinition::RollingBallJet(_) => {
            return rolling_ball_jet_point(procedural.definition(), u, v)
                .ok_or(EvaluationFailure::NoValue);
        }
        _ => None,
    };
    point.map_or(Err(EvaluationFailure::NoValue), admit_surface_point)
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

/// [`finite_sweep_differential`] for an admitted derivative, which leaves the
/// value as the only condition to test.
fn sweep_differential_of(value: f64, derivative: FiniteReal) -> Option<ScalarSweepDifferential> {
    value.is_finite().then_some(ScalarSweepDifferential {
        value,
        derivative: derivative.get(),
    })
}

fn scalar_sweep_law_differential(
    expression: &LawExpression<FiniteReal, FiniteVector3, FinitePoint3>,
    parameter: f64,
) -> Option<ScalarSweepDifferential> {
    if !parameter.is_finite() {
        return None;
    }
    match expression {
        LawExpression::Null {} => finite_sweep_differential(0.0, 0.0),
        LawExpression::Integer { value } => finite_sweep_differential(*value as f64, 0.0),
        LawExpression::Double { value } => finite_sweep_differential(value.get(), 0.0),
        LawExpression::Text { value } => {
            let value = value.as_str().trim();
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
                    let mut numerator = ExactSignedSum::default();
                    numerator.add_product(left.derivative, right.value);
                    numerator.add_product(-left.value, right.derivative);
                    let mut denominator = ExactSignedSum::default();
                    denominator.add_product(right.value, right.value);
                    let derivative = match numerator.finish() {
                        Some(value) => value.quotient(denominator.finish()?).ok()?,
                        None => FiniteReal::ZERO,
                    };
                    sweep_differential_of(left.value / right.value, derivative)
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
    match operator {
        "LN" => {
            return (x > 0.0)
                .then(|| finite_sweep_differential(x.ln(), operand.derivative / x))
                .flatten()
        }
        "EXP" => {
            let value = x.exp();
            if !value.is_finite() {
                return None;
            }
            let half = (0.5 * x).exp();
            let mut product = ExactSignedSum::default();
            product.add_factors([half, half, operand.derivative]);
            let derivative = product
                .finish()
                .map_or(Some(FiniteReal::ZERO), |value| value.finite().ok())?;
            return Some(ScalarSweepDifferential {
                value,
                derivative: derivative.get(),
            });
        }
        "COT" | "CSC" | "TAN" | "SEC" | "ARCSECH" => {
            let mut denominator = ExactSignedSum::default();
            let (value, factor) = match operator {
                "COT" | "CSC" => {
                    let sine = x.sin();
                    if sine == 0.0 {
                        return None;
                    }
                    denominator.add_product(sine, sine);
                    if operator == "COT" {
                        (1.0 / x.tan(), -1.0)
                    } else {
                        (1.0 / sine, -x.cos())
                    }
                }
                "TAN" | "SEC" => {
                    let cosine = x.cos();
                    if cosine == 0.0 {
                        return None;
                    }
                    denominator.add_product(cosine, cosine);
                    if operator == "TAN" {
                        (x.tan(), 1.0)
                    } else {
                        (1.0 / cosine, x.sin())
                    }
                }
                _ => {
                    if x <= 0.0 || x >= 1.0 {
                        return None;
                    }
                    let root = ((1.0 - x) * (1.0 + x)).sqrt();
                    denominator.add_product(x, root);
                    (((1.0 + root).ln() - x.ln()), -1.0)
                }
            };
            let mut numerator = ExactSignedSum::default();
            numerator.add_product(factor, operand.derivative);
            let denominator = denominator.finish()?;
            return sweep_differential_of(
                value,
                numerator.finish().map_or(Some(FiniteReal::ZERO), |value| {
                    value.quotient(denominator).ok()
                })?,
            );
        }
        "ARCTAN" | "ARCOT" | "ARCSEC" | "ARCCSC" | "ARCCSCH" => {
            let mut denominator = ExactSignedSum::default();
            let (value, sign) = match operator {
                "ARCTAN" | "ARCOT" => {
                    denominator.add_product(x, x);
                    denominator.add_product(1.0, 1.0);
                    if operator == "ARCTAN" {
                        (x.atan(), 1.0)
                    } else {
                        (std::f64::consts::FRAC_PI_2 - x.atan(), -1.0)
                    }
                }
                "ARCSEC" | "ARCCSC" => {
                    if x.abs() <= 1.0 {
                        return None;
                    }
                    let factor = (((x.abs() - 1.0) / x.abs()) * (1.0 + 1.0 / x.abs())).sqrt();
                    denominator.add_factors([x.abs(), x.abs(), factor]);
                    if operator == "ARCSEC" {
                        ((1.0 / x).acos(), 1.0)
                    } else {
                        ((1.0 / x).asin(), -1.0)
                    }
                }
                _ => {
                    if x == 0.0 {
                        return None;
                    }
                    denominator.add_product(x.abs(), x.hypot(1.0));
                    let inverse = 1.0 / x;
                    let value = if inverse.is_finite() {
                        inverse.asinh()
                    } else {
                        (std::f64::consts::LN_2 - x.abs().ln()).copysign(x)
                    };
                    (value, -1.0)
                }
            };
            let derivative = match crate::math::sum::scaled_finite(operand.derivative) {
                Some(numerator) => {
                    let quotient = numerator.quotient(denominator.finish()?).ok()?;
                    if sign < 0.0 {
                        quotient.negated()
                    } else {
                        quotient
                    }
                }
                None if operand.derivative == 0.0 => FiniteReal::ZERO,
                None => return None,
            };
            return sweep_differential_of(value, derivative);
        }
        "COTH" | "SECH" | "CSCH" => {
            if x == 0.0 && operator != "SECH" {
                return None;
            }
            let tail = (-x.abs()).exp();
            let sinh_denominator = -(-2.0 * x.abs()).exp_m1();
            let mut numerator = ExactSignedSum::default();
            let mut denominator = ExactSignedSum::default();
            let value = match operator {
                "COTH" => {
                    numerator.add_factors([-4.0, tail, tail, operand.derivative]);
                    denominator.add_product(sinh_denominator, sinh_denominator);
                    1.0 / x.tanh()
                }
                "SECH" => {
                    let half_tail = (-0.5 * x.abs()).exp();
                    numerator.add_factors([
                        -2.0 * x.tanh(),
                        half_tail,
                        half_tail,
                        operand.derivative,
                    ]);
                    denominator.add_product(1.0 + tail * tail, 1.0);
                    2.0 * tail / (1.0 + tail * tail)
                }
                _ => {
                    let half_tail = (-0.5 * x.abs()).exp();
                    numerator.add_factors([
                        -2.0 * (1.0 + tail * tail),
                        half_tail,
                        half_tail,
                        operand.derivative,
                    ]);
                    denominator.add_product(sinh_denominator, sinh_denominator);
                    (2.0 * tail / sinh_denominator).copysign(x)
                }
            };
            let derivative = match numerator.finish() {
                Some(value) => value.quotient(denominator.finish()?).ok()?,
                None => FiniteReal::ZERO,
            };
            return sweep_differential_of(value, derivative);
        }
        "TANH" => {
            let exponential = (-x.abs()).exp();
            let mut numerator = ExactSignedSum::default();
            numerator.add_factors([4.0, exponential, exponential, operand.derivative]);
            let denominator =
                crate::math::sum::scaled_finite((1.0 + exponential * exponential).powi(2))?;
            let derivative = match numerator.finish() {
                Some(value) => value.quotient(denominator).ok()?,
                None => FiniteReal::ZERO,
            };
            return sweep_differential_of(x.tanh(), derivative);
        }
        "ARCSINH" => {
            return finite_sweep_differential(x.asinh(), operand.derivative / x.hypot(1.0))
        }
        "ARCCOSH" => {
            if x <= 1.0 {
                return None;
            }
            let denominator = if x < 2.0 {
                ((x - 1.0) * (x + 1.0)).sqrt()
            } else {
                x * (1.0 - (1.0 / x).powi(2)).sqrt()
            };
            return finite_sweep_differential(x.acosh(), operand.derivative / denominator);
        }
        "ARCOTH" => {
            if x.abs() <= 1.0 {
                return None;
            }
            let inverse = 1.0 / x;
            let denominator = ((x - 1.0) / x) * ((x + 1.0) / x);
            let [left, right, denominator] =
                FiniteReal::array([operand.derivative / x, -inverse, denominator])?;
            let derivative = crate::math::multiply_divide(left, right, denominator)?;
            return sweep_differential_of(inverse.atanh(), derivative);
        }
        _ => {}
    }

    let derivative = match operator {
        "SIN" => x.cos(),
        "COS" => -x.sin(),
        "COSH" => x.sinh(),
        "SINH" => x.cosh(),
        "ARCCOS" => {
            let denominator = (1.0 - x * x).sqrt();
            (denominator > 0.0).then_some(-1.0 / denominator)?
        }
        "ARCSIN" => {
            let denominator = (1.0 - x * x).sqrt();
            (denominator > 0.0).then_some(1.0 / denominator)?
        }
        "ARCTANH" => (x.abs() < 1.0).then_some(1.0 / (1.0 - x * x))?,
        "ABS" => {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                return None;
            }
        }
        "SIGN" => (x != 0.0).then_some(0.0)?,
        "SQRT" => (x > 0.0).then_some(0.5 / x.sqrt())?,
        _ => return None,
    };
    finite_sweep_differential(
        match operator {
            "SIN" => x.sin(),
            "COS" => x.cos(),
            "COSH" => x.cosh(),
            "SINH" => x.sinh(),
            "ARCCOS" => x.acos(),
            "ARCSIN" => x.asin(),
            "ARCTANH" => x.atanh(),
            "ABS" => x.abs(),
            "SIGN" => x.signum(),
            "SQRT" => x.sqrt(),
            _ => return None,
        },
        derivative * operand.derivative,
    )
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

fn scale_sweep_profile(
    profile: ModelCurveDifferential,
    frame_point: Point3,
    scale: Vector3,
) -> Option<ModelCurveDifferential> {
    let displacement = point_displacement(profile.point.get(), frame_point);
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
    let point = FinitePoint3::new(point)?;
    (tangent.is_finite() && acceleration.is_finite()).then_some(ModelCurveDifferential {
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
    let transform = Transform::affine([
        [vectors[0].x, vectors[1].x, vectors[2].x, 0.0],
        [vectors[0].y, vectors[1].y, vectors[2].y, 0.0],
        [vectors[0].z, vectors[1].z, vectors[2].z, 0.0],
    ])?;
    transform.is_proper_rigid().then_some(transform)
}

fn straight_sweep_path_origin(
    index: &crate::index::ModelIndex<'_>,
    spine: &crate::ids::CurveId,
) -> Option<Point3> {
    let curve = index.curves(spine.as_str())?;
    match &curve.geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            Some(line_curve.origin().get())
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
            if nurbs.degree() == 1 && nurbs.control_points().len() == 2 && !nurbs.periodic() =>
        {
            let [start, _] = nurbs_curve_parameter_domain(nurbs)?.endpoints();
            curve_point(&curve.geometry, start).map(FinitePoint3::get)
        }
        _ => None,
    }
}

fn point_displacement(point: Point3, origin: Point3) -> Vector3 {
    Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z)
}

fn sweep_tail_interval_contains(interval: [Option<FiniteReal>; 2], parameter: f64) -> bool {
    parameter.is_finite()
        && interval[0].is_none_or(|lower| parameter >= lower.get())
        && interval[1].is_none_or(|upper| parameter <= upper.get())
}

fn unit_vector_with_derivative(vector: Vector3, derivative: Vector3) -> Option<(Vector3, Vector3)> {
    use crate::math::sum::ExactSignedSum;

    let unit = FiniteVector3::new(vector)?.unit_nonzero()?;
    if !derivative.is_finite() {
        return None;
    }
    let components = [vector.x, vector.y, vector.z];
    let derivatives = [derivative.x, derivative.y, derivative.z];
    let scale = components
        .iter()
        .fold(0.0_f64, |scale, value| scale.max(value.abs()));
    let length = (vector.x / scale)
        .hypot(vector.y / scale)
        .hypot(vector.z / scale);
    let mut denominator = ExactSignedSum::default();
    denominator.add_factors([scale, scale, scale, length * length * length]);
    let denominator = denominator.finish()?;
    // (|v|² d - v(v·d)) / |v|³. Exact products keep parallel derivatives
    // zero and postpone range checks until the normalized result is formed.
    let component = |index: usize| {
        let mut numerator = ExactSignedSum::default();
        for other in 0..3 {
            if other != index {
                numerator.add_factors([components[other], components[other], derivatives[index]]);
                numerator.add_factors([-components[index], components[other], derivatives[other]]);
            }
        }
        numerator.finish().map_or(Some(0.0), |value| {
            value.quotient(denominator).ok().map(FiniteReal::get)
        })
    };
    Some((
        unit,
        Vector3::new(component(0)?, component(1)?, component(2)?),
    ))
}

fn sweep_profile_reversed(
    profile_frame: Option<(FinitePoint3, FiniteVector3)>,
    spine_tangent: Vector3,
) -> Option<bool> {
    let Some((_, frame_vector)) = profile_frame else {
        return Some(false);
    };
    let frame_vector = unit_axis(frame_vector.get())?;
    let spine_tangent = unit_axis(spine_tangent)?;
    let alignment = frame_vector.dot(spine_tangent);
    ((alignment.abs() - 1.0).abs() <= EPS_EVAL_SWEEP_PROFILE_FRAME_ALIGNMENT_E9)
        .then_some(alignment < 0.0)
}

fn sweep_profile_differential(
    index: &crate::index::ModelIndex<'_>,
    profile: &crate::ids::CurveId,
    profile_range: [FiniteReal; 2],
    reversed: bool,
    parameter: f64,
) -> Option<ModelCurveDifferential> {
    if !sweep_tail_interval_contains([Some(profile_range[0]), Some(profile_range[1])], parameter) {
        return None;
    }
    let [profile_start, profile_end] = FiniteReal::raw_array(profile_range);
    let profile_span = profile_end - profile_start;
    if !profile_span.is_finite() || profile_span <= 0.0 {
        return None;
    }
    let curve = index.curves(profile.as_str())?;
    let (native_parameter, parameter_scale) = match &curve.geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            let [native_start, native_end] = nurbs_curve_parameter_domain(nurbs)?.endpoints();
            let native_span = native_end - native_start;
            let fraction = (parameter - profile_start) / profile_span;
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
    construction: &crate::geometry::SweepSurfaceConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Option<(
    ModelCurveDifferential,
    ModelCurveDifferential,
    ScalarSweepDifferential,
    Point3,
)> {
    let form = construction.cache.form()?;
    let parameterization = form.cache.parameterization()?;
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
    let rail_transform = sweep_rail_transform(formula)?;
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
    let frame_point = profile_frame.map_or(*origin, |(point, _)| point).get();
    let mut profile = scale_sweep_profile(profile, frame_point, scale)?;
    profile.point = rail_transform.apply_point(profile.point.get())?;
    profile.tangent = rail_transform.apply_vector(profile.tangent)?.get();
    profile.acceleration = rail_transform.apply_vector(profile.acceleration)?.get();
    let law = scalar_sweep_law_differential(first_law, v)?;
    Some((profile, spine, law, path_origin))
}

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
) -> Option<Point3> {
    let (profile, spine, law, path_origin) =
        cacheless_law_sweep_differentials(index, profile, spine, construction, u, v)?;
    let (profile_tangent, _) = unit_vector_with_derivative(profile.tangent, profile.acceleration)?;
    let (spine_tangent, _) = unit_vector_with_derivative(spine.tangent, spine.acceleration)?;
    let normal = profile_tangent.cross(spine_tangent);
    Some(offset(
        profile.point.get(),
        &[
            (1.0, point_displacement(spine.point.get(), path_origin)),
            (law.value, normal),
        ],
    ))
}

fn cacheless_law_sweep_partials(
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
            profile.point.get(),
            &[
                (1.0, point_displacement(spine.point.get(), path_origin)),
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
    side: &crate::geometry::RollingBallSide<
        crate::ids::SurfaceId,
        crate::ids::CurveId,
        PcurveGeometry,
        FiniteReal,
        FinitePoint3,
    >,
    parameter: f64,
) -> Option<ContactTrackDifferential> {
    let surface = &side.surface.as_ref()?.surface;
    let pcurve = side.pcurve.as_ref()?;
    // A non-finite offset-pcurve point is evaluated on its support as a
    // finite one is.
    let uv = match pcurve_uv(pcurve, parameter) {
        Ok(uv) => uv.get(),
        Err(failure) => failure.non_finite()?,
    };
    let uv_tangent = pcurve_tangent(pcurve, parameter).ok()?;
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
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> bool {
    let exact_construction = matches!(
        construction.cache,
        crate::geometry::VariableBlendCache::Parameterization { .. }
            | crate::geometry::VariableBlendCache::Stale {}
    );
    exact_construction
        && (0.0..=1.0).contains(&u)
        && sweep_tail_interval_contains(construction.slice_range, v)
        && construction.cache.parameterization().is_none_or(|tail| {
            sweep_tail_interval_contains(tail.u_interval, u)
                && sweep_tail_interval_contains(tail.v_interval, v)
        })
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

fn surface_cache_evaluation(
    geometry: &crate::geometry::SurfaceGeometry,
    u: f64,
    v: f64,
) -> Option<(Point3, Option<Vector3>)> {
    let partials = surface_partials(geometry, u, v)?;
    Some((partials.point, partials.du.cross(partials.dv).unit()))
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

fn cacheless_ruled_variable_blend_partials(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
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
    value: &crate::geometry::VariableBlendValue<FiniteReal, FiniteVector3, FinitePoint3>,
    parameter: f64,
) -> Option<FiniteReal> {
    match &value.payload {
        crate::geometry::VariableBlendValuePayload::TwoEnds {
            parameters, radii, ..
        } => {
            let [first_parameter, second_parameter] = FiniteReal::raw_array(*parameters);
            let [first_radius, second_radius] = FiniteReal::raw_array(*radii);
            let width = second_parameter - first_parameter;
            if width == 0.0 {
                return None;
            }
            let fraction = (parameter - first_parameter) / width;
            FiniteReal::new(first_radius + fraction * (second_radius - first_radius))
        }
        crate::geometry::VariableBlendValuePayload::Constant { nested, .. } => {
            variable_blend_radius(nested, parameter)
        }
        crate::geometry::VariableBlendValuePayload::Functional { function, .. }
        | crate::geometry::VariableBlendValuePayload::Interpolated { function, .. } => {
            let [radius, _] = pcurve_uv(function, parameter).ok()?.coordinates();
            Some(radius)
        }
        _ => None,
    }
}

fn variable_blend_radius_differential(
    value: &crate::geometry::VariableBlendValue<FiniteReal, FiniteVector3, FinitePoint3>,
    parameter: f64,
) -> Option<ScalarSweepDifferential> {
    match &value.payload {
        crate::geometry::VariableBlendValuePayload::TwoEnds {
            parameters, radii, ..
        } => {
            let [first_parameter, second_parameter] = FiniteReal::raw_array(*parameters);
            let [first_radius, second_radius] = FiniteReal::raw_array(*radii);
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
            let [radius, _] = pcurve_uv(function, parameter).ok()?.coordinates();
            let [derivative, _] = pcurve_tangent(function, parameter).ok()?.coordinates();
            Some(ScalarSweepDifferential {
                value: radius.get(),
                derivative: derivative.get(),
            })
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
    let angle = first_radius
        .cross(second_radius)
        .norm()
        .atan2(first_radius.dot(second_radius));
    let section_angle = u * angle;
    let radial = vector_sum(&[
        (section_angle.cos(), first_radius),
        (section_angle.sin(), axis.cross(first_radius)),
    ]);
    Some(offset(center, &[(radius, radial)]))
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
) -> Option<Point3> {
    if !cacheless_variable_blend_domain_contains(construction, u, v)
        || !construction.radii.is_single()
        || !matches!(
            construction.cross_section,
            None | Some(crate::geometry::VariableBlendCrossSection::Circular {})
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
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Option<CircularVariableBlendSection> {
    if !cacheless_variable_blend_domain_contains(construction, u, v)
        || !construction.radii.is_single()
        || !matches!(
            construction.cross_section,
            None | Some(crate::geometry::VariableBlendCrossSection::Circular {})
        )
    {
        return None;
    }
    let signed_radius = variable_blend_radius(construction.radii.first(), v)?.get();
    let radius = signed_radius.abs();
    if radius <= f64::EPSILON {
        return None;
    }
    let radius_derivative = variable_blend_radius_differential(construction.radii.first(), v)
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
        .get()
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
    native: &crate::geometry::RollingBallConstruction<FiniteReal, FiniteVector3, FinitePoint3>,
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
    native: &crate::geometry::RollingBallConstruction<FiniteReal, FiniteVector3, FinitePoint3>,
    u: f64,
    v: f64,
) -> Option<ConstantRollingBallSection> {
    let crate::geometry::BlendRadiusLaw::Constant { signed_radius } = radius else {
        return None;
    };
    let signed_radius = signed_radius.get();
    if !matches!(
        native.cache,
        crate::geometry::RevisionCacheForm::Parameterization(_)
    ) || native.third.is_some()
        || *cross_section != crate::geometry::BlendCrossSection::Circular
        || !(0.0..=1.0).contains(&u)
        || !sweep_tail_interval_contains(native.slice_range, v)
        || !sweep_tail_interval_contains(native.u_range, u)
        || !sweep_tail_interval_contains(native.v_range, v)
        || !native.cache.parameterization().is_some_and(|tail| {
            sweep_tail_interval_contains(tail.u_interval, u)
                && sweep_tail_interval_contains(tail.v_interval, v)
        })
    {
        return None;
    }
    let radius = signed_radius.abs();
    if radius <= f64::EPSILON {
        return None;
    }
    for (support, side) in supports.iter().zip(native.sides.iter()) {
        if support.as_ref().is_some_and(|support| {
            side.surface
                .as_ref()
                .is_some_and(|surface| surface.surface != support.surface)
        }) {
            return None;
        }
    }
    let first = variable_blend_contact_track_differential(index, &native.sides[0], v)?;
    let second = variable_blend_contact_track_differential(index, &native.sides[1], v)?;
    let center = model_curve_point_by_id(index, &native.slice, v)?;
    let center_tangent = model_curve_differential_by_id(index, &native.slice, v)
        .map(|differential| differential.tangent);
    let tolerance = index.ir().tolerances.linear.get().max(
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
        .any(|offset| (offset.get() - signed_radius).abs() > tolerance)
        || radius_error(first.point) > tolerance
        || radius_error(second.point) > tolerance
    {
        return None;
    }
    Some(ConstantRollingBallSection {
        center: center.get(),
        center_tangent,
        first,
        second,
        radius,
    })
}

fn cacheless_circular_variable_blend_partials(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
    u: f64,
    v: f64,
) -> Option<SurfacePartials<FinitePoint3, FiniteVector3>> {
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
    native: &crate::geometry::RollingBallConstruction<FiniteReal, FiniteVector3, FinitePoint3>,
    u: f64,
    v: f64,
) -> Option<SurfacePartials<FinitePoint3, FiniteVector3>> {
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
) -> Option<SurfacePartials<FinitePoint3, FiniteVector3>> {
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
) -> Option<SurfacePartials<FinitePoint3, FiniteVector3>> {
    let first_delta = point_displacement(first.point, center);
    let second_delta = point_displacement(second.point, center);
    let first_delta_v = vector_sum(&[(1.0, first.tangent), (-1.0, center_tangent)]);
    let second_delta_v = vector_sum(&[(1.0, second.tangent), (-1.0, center_tangent)]);
    let (first_radius, first_radius_v) = unit_vector_with_derivative(first_delta, first_delta_v)?;
    let (second_radius, second_radius_v) =
        unit_vector_with_derivative(second_delta, second_delta_v)?;
    let cosine = first_radius.dot(second_radius).clamp(-1.0, 1.0);
    let cross = first_radius.cross(second_radius);
    let sine = cross.norm();
    let axis = FiniteVector3::new(cross)?.unit_nonzero()?;
    let angle = sine.atan2(cosine);
    let cosine_v = first_radius_v.dot(second_radius) + first_radius.dot(second_radius_v);
    let cross_v = vector_sum(&[
        (1.0, first_radius_v.cross(second_radius)),
        (1.0, first_radius.cross(second_radius_v)),
    ]);
    let sine_v = axis.dot(cross_v);
    let angle_v = cosine * sine_v - sine * cosine_v;
    let axis_v = vector_sum(&[(1.0, cross_v), (-sine_v, axis)]).scale(1.0 / sine);
    let transverse = axis.cross(first_radius);
    let transverse_v = vector_sum(&[
        (1.0, axis_v.cross(first_radius)),
        (1.0, axis.cross(first_radius_v)),
    ]);
    let (section_sine, section_cosine) = (u * angle).sin_cos();
    let radial = vector_sum(&[(section_cosine, first_radius), (section_sine, transverse)]);
    let angular_direction =
        vector_sum(&[(-section_sine, first_radius), (section_cosine, transverse)]);
    let radial_v = vector_sum(&[
        (section_cosine, first_radius_v),
        (section_sine, transverse_v),
        (u * angle_v, angular_direction),
    ]);
    Some(SurfacePartials {
        point: FinitePoint3::new(offset(center, &[(radius, radial)]))?,
        du: FiniteVector3::new(angular_direction.scale(radius * angle))?,
        dv: FiniteVector3::new(vector_sum(&[
            (1.0, center_tangent),
            (radius_derivative, radial),
            (radius, radial_v),
        ]))?,
    })
}

fn cacheless_variable_blend_point(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::VariableBlendConstruction<
        FiniteReal,
        FiniteVector3,
        FinitePoint3,
    >,
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
) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    if !v.is_finite() {
        return Err(EvaluationFailure::NoValue);
    }
    let first =
        model_curve_differential_by_id(index, first, u).ok_or(EvaluationFailure::NoValue)?;
    let second =
        model_curve_differential_by_id(index, second, u).ok_or(EvaluationFailure::NoValue)?;
    let point = offset(
        first.point.get(),
        &[(v, point_displacement(second.point.get(), first.point.get()))],
    );
    let blend = |first: Vector3, second: Vector3| vector_sum(&[(1.0 - v, first), (v, second)]);
    admitted_partials(SurfaceSecondPartials {
        point,
        du: blend(first.tangent, second.tangent),
        dv: point_displacement(second.point.get(), first.point.get()),
        duu: blend(first.acceleration, second.acceleration),
        duv: vector_sum(&[(-1.0, first.tangent), (1.0, second.tangent)]),
        dvv: Vector3::new(0.0, 0.0, 0.0),
    })
}

fn model_sum_surface_partials(
    index: &crate::index::ModelIndex<'_>,
    construction: &crate::geometry::surface_payloads::SumSurfaceConstruction,
    u: f64,
    v: f64,
) -> Result<SurfaceSecondPartials<FinitePoint3, FiniteVector3>, EvaluationFailure<Point3>> {
    let basepoint = *construction.basepoint();
    let first = model_curve_differential_by_id(index, construction.first(), u)
        .ok_or(EvaluationFailure::NoValue)?;
    let second = model_curve_differential_by_id(index, construction.second(), v)
        .ok_or(EvaluationFailure::NoValue)?;
    admitted_partials(SurfaceSecondPartials {
        point: Point3::new(
            first.point.x + second.point.x - basepoint.x,
            first.point.y + second.point.y - basepoint.y,
            first.point.z + second.point.z - basepoint.z,
        ),
        du: first.tangent,
        dv: second.tangent,
        duu: first.acceleration,
        duv: Vector3::new(0.0, 0.0, 0.0),
        dvv: second.acceleration,
    })
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
            nurbs_surface_point_with_budget_evaluation(nurbs, u, v, budget)
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

fn surface_partials_with_budget_solved(
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Option<SurfacePartials> {
    /// The descent is bounded by
    /// [`PlacedSurface`](crate::geometry::PlacedSurface) construction; no arm
    /// follows an arena id.
    fn evaluate(
        geometry: &SolvedSurfaceGeometry,
        u: f64,
        v: f64,
        budget: &WorkBudget<'_>,
    ) -> Option<SurfacePartials> {
        match geometry {
            SolvedSurfaceGeometry::Nurbs(nurbs) => {
                nurbs_surface_partials_with_budget(nurbs, u, v, budget)
                    .map(SurfacePartials::into_raw)
            }
            SolvedSurfaceGeometry::Transformed(placed) => {
                budget.charge().then_some(())?;
                evaluate(placed.basis(), u, v, budget).and_then(|partials| {
                    let transform = placed.transform();
                    Some(SurfacePartials {
                        point: transform.apply_point(partials.point)?.get(),
                        du: transform.apply_vector(partials.du)?.get(),
                        dv: transform.apply_vector(partials.dv)?.get(),
                    })
                })
            }
            _ => surface_partials_solved(geometry, u, v),
        }
    }

    match (geometry, budget) {
        (_, Some(budget)) => evaluate(geometry, u, v, budget),
        _ => surface_partials_solved(geometry, u, v),
    }
}

fn surface_second_partials_with_budget_solved(
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Option<SurfaceSecondPartials> {
    /// The descent is bounded by
    /// [`PlacedSurface`](crate::geometry::PlacedSurface) construction; no arm
    /// follows an arena id.
    fn evaluate(
        geometry: &SolvedSurfaceGeometry,
        u: f64,
        v: f64,
        budget: &WorkBudget<'_>,
    ) -> Option<SurfaceSecondPartials> {
        match geometry {
            SolvedSurfaceGeometry::Nurbs(nurbs) => {
                nurbs_surface_second_partials_with_budget(nurbs, u, v, budget)
                    .map(SurfaceSecondPartials::into_raw)
            }
            SolvedSurfaceGeometry::Transformed(placed) => {
                budget.charge().then_some(())?;
                evaluate(placed.basis(), u, v, budget)
                    .and_then(|partials| {
                        transform_surface_second_partials(partials, *placed.transform())
                    })
                    .map(SurfaceSecondPartials::into_raw)
            }
            _ => surface_second_partials_solved(geometry, u, v),
        }
    }

    match (geometry, budget) {
        (_, Some(budget)) => evaluate(geometry, u, v, budget),
        _ => surface_second_partials_solved(geometry, u, v),
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
    model_surface_point_by_id_inner(index, surface, u, v, Some(budget))
}

fn model_surface_point_by_id_inner(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    /// An arm's point, admitted where the arm computes it raw, and the
    /// support's oriented normal.
    struct SurfaceEvaluation {
        /// The point, or the non-finite point the arm reached.
        point: Result<FinitePoint3, Point3>,
        oriented_normal: Option<Vector3>,
    }

    /// Admit a point an arm computes raw, keeping the non-finite point.
    fn evaluated(point: Point3) -> Result<FinitePoint3, Point3> {
        FinitePoint3::new(point).ok_or(point)
    }

    /// The point of an evaluation, finite or not.
    fn reached(point: Result<FinitePoint3, Point3>) -> Point3 {
        point.map_or_else(|point| point, FinitePoint3::get)
    }

    /// The evaluation of an arm whose partials are admitted together.
    fn partials_evaluation(
        partials: Result<
            SurfaceSecondPartials<FinitePoint3, FiniteVector3>,
            EvaluationFailure<Point3>,
        >,
    ) -> Option<SurfaceEvaluation> {
        let point = match partials {
            Ok(partials) => Ok(partials.point),
            Err(failure) => Err(failure.non_finite()?),
        };
        Some(SurfaceEvaluation {
            point,
            oriented_normal: None,
        })
    }

    /// A stored carrier whose partials are absent: the point its evaluation
    /// reached where the evaluation left the finite range. An exhausted
    /// budget, and a carrier with no point, leave no value.
    fn partials_left_finite_range(
        geometry: &SurfaceGeometry,
        u: f64,
        v: f64,
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<SurfaceEvaluation> {
        if budget.is_some_and(WorkBudget::exhausted) {
            return None;
        }
        Some(SurfaceEvaluation {
            point: Err(failure_at(surface_point(geometry, u, v)).non_finite()?),
            oriented_normal: None,
        })
    }

    /// The offset of a support whose normal is absent: a support point
    /// outside the finite range reaches no normal, so the offset reaches no
    /// coordinate; a finite support point without a normal has no offset.
    fn offset_without_normal(support: &SurfaceEvaluation) -> Option<SurfaceEvaluation> {
        support.point.is_err().then_some(SurfaceEvaluation {
            point: Err(Point3::new(f64::NAN, f64::NAN, f64::NAN)),
            oriented_normal: None,
        })
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
        let boundary_u = u.clamp(u_domain[0], u_domain[1]);
        let boundary_v = v.clamp(v_domain[0], v_domain[1]);
        if boundary_u == u && boundary_v == v {
            return None;
        }
        let partials =
            surface_partials_with_budget(&support.geometry, boundary_u, boundary_v, budget)?;
        let normal = partials.du.cross(partials.dv);
        let normal = if nurbs.normal_reversed() {
            scale_vector(normal, -1.0)
        } else {
            normal
        };
        let magnitude = normal.norm();
        let oriented_normal = (magnitude.is_finite() && magnitude > 0.0).then(|| {
            Vector3::new(
                normal.x / magnitude,
                normal.y / magnitude,
                normal.z / magnitude,
            )
        })?;
        let du = u - boundary_u;
        let dv = v - boundary_v;
        Some(SurfaceEvaluation {
            point: evaluated(Point3::new(
                partials.point.x + du * partials.du.x + dv * partials.dv.x,
                partials.point.y + du * partials.du.y + dv * partials.dv.y,
                partials.point.z + du * partials.du.z + dv * partials.dv.z,
            )),
            oriented_normal: Some(oriented_normal),
        })
    }

    fn evaluate(
        index: &crate::index::ModelIndex<'_>,
        surface_id: &crate::ids::SurfaceId,
        u: f64,
        v: f64,
        visiting: &mut Vec<crate::ids::SurfaceId>,
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<SurfaceEvaluation> {
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
                model_axis_revolution_point(
                    index,
                    definition_payload.directrix(),
                    definition_payload.axis_origin().get(),
                    definition_payload.axis_direction(),
                    u,
                    v,
                    budget,
                )
                .map(|point| SurfaceEvaluation {
                    point: evaluated(point),
                    oriented_normal: None,
                })
            }
            Some(ProceduralSurfaceDefinition::Extrusion(definition_payload)) => {
                partials_evaluation(model_native_extrusion_partials(
                    index,
                    definition_payload,
                    carrier_interval,
                    u,
                    v,
                    budget,
                ))
            }
            Some(ProceduralSurfaceDefinition::LinearSweep(definition_payload)) => {
                let directrix = definition_payload.directrix();
                budget
                    .map_or_else(
                        || model_curve_point_by_id(index, directrix, u),
                        |budget| model_curve_point_by_id_with_budget(index, directrix, u, budget),
                    )
                    .map(|point| SurfaceEvaluation {
                        point: evaluated(offset(
                            point.get(),
                            &[(v, definition_payload.direction().get())],
                        )),
                        oriented_normal: None,
                    })
            }
            Some(ProceduralSurfaceDefinition::Revolution(definition_payload)) => {
                model_native_revolution_partials(
                    index,
                    definition_payload,
                    carrier_interval,
                    u,
                    v,
                    budget,
                )
                .map(|partials| SurfaceEvaluation {
                    point: evaluated(partials.point),
                    oriented_normal: None,
                })
            }
            Some(ProceduralSurfaceDefinition::Ruled { first, second, .. }) => {
                partials_evaluation(model_ruled_surface_partials(index, first, second, u, v))
            }
            Some(ProceduralSurfaceDefinition::Sum(definition_payload)) => {
                partials_evaluation(model_sum_surface_partials(index, definition_payload, u, v))
            }
            Some(ProceduralSurfaceDefinition::Sweep(definition_payload)) => {
                if let Some(construction) = definition_payload.native() {
                    let profile = definition_payload.profile();
                    let spine = definition_payload.spine();

                    cacheless_law_sweep_point(index, profile, spine, construction, u, v)
                        .map(|point| SurfaceEvaluation {
                            point: evaluated(point),
                            oriented_normal: None,
                        })
                        .or_else(|| {
                            if !sweep_has_current_cache(construction) {
                                return None;
                            }
                            let (point, oriented_normal) =
                                surface_cache_evaluation(&surface.geometry, u, v)?;
                            Some(SurfaceEvaluation {
                                point: evaluated(point),
                                oriented_normal,
                            })
                        })
                } else {
                    surface_partials_with_budget(&surface.geometry, u, v, budget)
                        .map(|partials| {
                            let normal = partials.du.cross(partials.dv);
                            let normal =
                                match &surface.geometry {
                                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                                        nurbs,
                                    )) if nurbs.normal_reversed() => scale_vector(normal, -1.0),
                                    _ => normal,
                                };
                            let magnitude = normal.norm();
                            let oriented_normal =
                                (magnitude.is_finite() && magnitude > 0.0).then(|| {
                                    Vector3::new(
                                        normal.x / magnitude,
                                        normal.y / magnitude,
                                        normal.z / magnitude,
                                    )
                                });
                            SurfaceEvaluation {
                                point: evaluated(partials.point),
                                oriented_normal,
                            }
                        })
                        .or_else(|| partials_left_finite_range(&surface.geometry, u, v, budget))
                }
            }
            Some(ProceduralSurfaceDefinition::VariableBlend(definition_payload)) => {
                let construction = definition_payload.construction();

                cacheless_variable_blend_point(index, construction, u, v)
                    .map(|point| SurfaceEvaluation {
                        point: evaluated(point),
                        oriented_normal: None,
                    })
                    .or_else(|| {
                        if !variable_blend_has_current_cache(construction) {
                            return None;
                        }
                        let (point, oriented_normal) =
                            surface_cache_evaluation(&surface.geometry, u, v)?;
                        Some(SurfaceEvaluation {
                            point: evaluated(point),
                            oriented_normal,
                        })
                    })
            }
            Some(ProceduralSurfaceDefinition::Blend(definition_payload)) => {
                if let Some(native) = definition_payload.native() {
                    let supports = definition_payload.supports();
                    let radius = definition_payload.radius();
                    let cross_section = definition_payload.cross_section();

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
                        .and_then(|partials| partials.du.cross(partials.dv.get()).unit());
                        Some(SurfaceEvaluation {
                            point: evaluated(point),
                            oriented_normal,
                        })
                    } else if revision_surface_tail_has_current_cache(&native.cache) {
                        let (point, oriented_normal) =
                            surface_cache_evaluation(&surface.geometry, u, v)?;
                        Some(SurfaceEvaluation {
                            point: evaluated(point),
                            oriented_normal,
                        })
                    } else {
                        None
                    }
                } else {
                    surface_partials_with_budget(&surface.geometry, u, v, budget)
                        .map(|partials| {
                            let normal = partials.du.cross(partials.dv);
                            let normal =
                                match &surface.geometry {
                                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                                        nurbs,
                                    )) if nurbs.normal_reversed() => scale_vector(normal, -1.0),
                                    _ => normal,
                                };
                            let magnitude = normal.norm();
                            let oriented_normal =
                                (magnitude.is_finite() && magnitude > 0.0).then(|| {
                                    Vector3::new(
                                        normal.x / magnitude,
                                        normal.y / magnitude,
                                        normal.z / magnitude,
                                    )
                                });
                            SurfaceEvaluation {
                                point: evaluated(partials.point),
                                oriented_normal,
                            }
                        })
                        .or_else(|| partials_left_finite_range(&surface.geometry, u, v, budget))
                }
            }
            Some(ProceduralSurfaceDefinition::RollingBallJet(_)) => procedural
                .and_then(|procedural| rolling_ball_jet_point(procedural.definition(), u, v))
                .map(|point| SurfaceEvaluation {
                    point: Ok(point),
                    oriented_normal: None,
                }),
            Some(ProceduralSurfaceDefinition::CurveBounded { support, .. }) => {
                evaluate(index, support, u, v, visiting, budget)
            }
            Some(ProceduralSurfaceDefinition::Replica { source, transform }) => {
                let mut evaluation = evaluate(index, source, u, v, visiting, budget)?;
                let transformed_normal =
                    model_surface_second_partials_by_id_inner(index, source, u, v, budget)
                        .and_then(|partials| {
                            let normal = transform
                                .apply_vector(partials.du)?
                                .get()
                                .cross(transform.apply_vector(partials.dv)?.get());
                            normal.unit()
                        })
                        .or_else(|| {
                            evaluation
                                .oriented_normal
                                .and_then(|normal| transform.apply_normal(normal))
                                .and_then(|normal| {
                                    Some(scale_vector(*normal.as_raw(), transform.orientation()?))
                                })
                        });
                evaluation.point = match evaluation.point {
                    Ok(point) => transform.apply_point_reaching(point.get()),
                    // The placement of a point outside the finite range stays
                    // outside it, whatever point the placement reaches.
                    Err(point) => Err(transform
                        .apply_point_reaching(point)
                        .map_or_else(|point| point, FinitePoint3::get)),
                };
                evaluation.oriented_normal = transformed_normal;
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
                    let mut evaluation =
                        evaluate(index, support, support_u, support_v, visiting, budget)?;
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
                    let support = evaluate(index, support, u, v, visiting, budget)?;
                    match support.oriented_normal {
                        Some(normal) => Some(SurfaceEvaluation {
                            point: evaluated(offset(
                                reached(support.point),
                                &[(distance.get(), normal)],
                            )),
                            oriented_normal: Some(normal),
                        }),
                        None => offset_without_normal(&support),
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
                        .or_else(|| evaluate(index, support, u, v, visiting, budget))?;
                    match support.oriented_normal {
                        Some(normal) => Some(SurfaceEvaluation {
                            point: evaluated(offset(
                                reached(support.point),
                                &[(distance.get(), normal)],
                            )),
                            oriented_normal: Some(normal),
                        }),
                        None => offset_without_normal(&support),
                    }
                }
            }
            _ if procedural.is_some() => {
                // A non-finite point enters the evaluation as a finite one
                // does.
                match model_surface_point_with_budget(index.ir(), &surface.geometry, u, v, budget) {
                    Ok(point) => Some(Ok(point)),
                    Err(failure) => failure.non_finite().map(Err),
                }
                .map(|point| SurfaceEvaluation {
                    point,
                    oriented_normal: None,
                })
            }
            _ => surface_partials_with_budget(&surface.geometry, u, v, budget)
                .map(|partials| {
                    let normal = partials.du.cross(partials.dv);
                    let normal = match &surface.geometry {
                        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs))
                            if nurbs.normal_reversed() =>
                        {
                            scale_vector(normal, -1.0)
                        }
                        _ => normal,
                    };
                    let magnitude = normal.norm();
                    let oriented_normal = (magnitude.is_finite() && magnitude > 0.0).then(|| {
                        Vector3::new(
                            normal.x / magnitude,
                            normal.y / magnitude,
                            normal.z / magnitude,
                        )
                    });
                    SurfaceEvaluation {
                        point: evaluated(partials.point),
                        oriented_normal,
                    }
                })
                .or_else(|| partials_left_finite_range(&surface.geometry, u, v, budget)),
        };
        visiting.pop();
        result
    }

    if let Some(budget) = budget {
        if index
            .procedural_surface_for_surface(surface.as_str())
            .is_none()
        {
            budget
                .charge()
                .then_some(())
                .ok_or(EvaluationFailure::NoValue)?;
            let surface = index
                .surfaces(surface.as_str())
                .ok_or(EvaluationFailure::NoValue)?;
            return surface_point_with_budget(&surface.geometry, u, v, budget);
        }
    }
    evaluate(index, surface, u, v, &mut Vec::new(), budget)
        .ok_or(EvaluationFailure::NoValue)?
        .point
        .map_err(EvaluationFailure::NonFinite)
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
    if let Some(ProceduralSurfaceDefinition::Blend(definition_payload)) = index
        .procedural_surface_for_surface(surface.as_str())
        .map(crate::geometry::ProceduralSurface::definition)
    {
        if let Some(native) = definition_payload.native() {
            let supports = definition_payload.supports();
            let radius = definition_payload.radius();
            let cross_section = definition_payload.cross_section();

            if let Some(partials) = cacheless_constant_rolling_ball_partials(
                index,
                supports,
                radius,
                cross_section,
                native,
                u,
                v,
            ) {
                return Some(partials.into_raw());
            }
            if !revision_surface_tail_has_current_cache(&native.cache) {
                return None;
            }
        }
    }
    if let Some(ProceduralSurfaceDefinition::VariableBlend(definition_payload)) = index
        .procedural_surface_for_surface(surface.as_str())
        .map(crate::geometry::ProceduralSurface::definition)
    {
        let construction = definition_payload.construction();

        if let Some(partials) = cacheless_ruled_variable_blend_partials(index, construction, u, v) {
            return Some(partials);
        }
        if let Some(partials) =
            cacheless_circular_variable_blend_partials(index, construction, u, v)
        {
            return Some(partials.into_raw());
        }
        if !variable_blend_has_current_cache(construction) {
            return None;
        }
    }
    if let Some(ProceduralSurfaceDefinition::Sweep(definition_payload)) = index
        .procedural_surface_for_surface(surface.as_str())
        .map(crate::geometry::ProceduralSurface::definition)
    {
        if let Some(construction) = definition_payload.native() {
            let profile = definition_payload.profile();
            let spine = definition_payload.spine();

            if let Some(partials) =
                cacheless_law_sweep_partials(index, profile, spine, construction, u, v)
            {
                return Some(partials);
            }
            if !sweep_has_current_cache(construction) {
                return None;
            }
        }
    }
    model_surface_second_partials_by_id_inner(index, surface, u, v, None).map(|partials| {
        SurfacePartials {
            point: partials.point,
            du: partials.du,
            dv: partials.dv,
        }
    })
}

/// Evaluate an arena-selected surface and its exact first partial derivatives
/// within a caller-owned work slice.
pub fn model_surface_partials_by_id_with_budget(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
    budget: &WorkBudget<'_>,
) -> Option<SurfacePartials> {
    let _guard = budget.recursion_guard()?;
    model_surface_second_partials_by_id_inner(index, surface, u, v, Some(budget)).map(|partials| {
        SurfacePartials {
            point: partials.point,
            du: partials.du,
            dv: partials.dv,
        }
    })
}

fn model_surface_second_partials_by_id(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Option<SurfaceSecondPartials> {
    model_surface_second_partials_by_id_inner(index, surface, u, v, None)
}

fn model_surface_second_partials_by_id_inner(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Option<SurfaceSecondPartials> {
    let mapping = model_surface_mapping(index, surface, u, v, &mut Vec::new(), budget)?;
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
    budget: Option<&WorkBudget<'_>>,
) -> Option<SurfaceMapping> {
    if let Some(budget) = budget {
        budget.charge().then_some(())?;
    }
    if visiting.contains(surface) {
        return None;
    }
    visiting.push(surface.clone());
    let carrier = index.surfaces(surface.as_str())?;
    let procedural = index.procedural_surface_for_surface(surface.as_str());
    let carrier_interval =
        procedural.and_then(|procedural| record_u_interval(procedural.record_bounds()));
    let result = match procedural.map(crate::geometry::ProceduralSurface::definition) {
        Some(ProceduralSurfaceDefinition::AxisRevolution(definition_payload)) => {
            Some(SurfaceMapping {
                base: model_axis_revolution_partials(
                    index,
                    definition_payload.directrix(),
                    definition_payload.axis_origin().get(),
                    definition_payload.axis_direction(),
                    u,
                    v,
                    budget,
                )?,
                offset_distance: 0.0,
                u_scale: 1.0,
                v_scale: 1.0,
                orientation: 1.0,
            })
        }
        Some(ProceduralSurfaceDefinition::Extrusion(definition_payload)) => Some(SurfaceMapping {
            base: model_native_extrusion_partials(
                index,
                definition_payload,
                carrier_interval,
                u,
                v,
                budget,
            )
            .ok()?
            .into_raw(),
            offset_distance: 0.0,
            u_scale: 1.0,
            v_scale: 1.0,
            orientation: 1.0,
        }),
        Some(ProceduralSurfaceDefinition::LinearSweep(definition_payload)) => {
            let directrix = definition_payload.directrix();
            let direction = definition_payload.direction();
            let differential = budget.map_or_else(
                || model_curve_differential_by_id(index, directrix, u),
                |budget| model_curve_differential_by_id_with_budget(index, directrix, u, budget),
            )?;
            let zero = Vector3::new(0.0, 0.0, 0.0);
            Some(SurfaceMapping {
                base: SurfaceSecondPartials {
                    point: offset(differential.point.get(), &[(v, direction.get())]),
                    du: differential.tangent,
                    dv: direction.get(),
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
        Some(ProceduralSurfaceDefinition::Revolution(definition_payload)) => Some(SurfaceMapping {
            base: model_native_revolution_partials(
                index,
                definition_payload,
                carrier_interval,
                u,
                v,
                budget,
            )?,
            offset_distance: 0.0,
            u_scale: 1.0,
            v_scale: 1.0,
            orientation: 1.0,
        }),
        Some(ProceduralSurfaceDefinition::Ruled { first, second, .. }) => Some(SurfaceMapping {
            base: model_ruled_surface_partials(index, first, second, u, v)
                .ok()?
                .into_raw(),
            offset_distance: 0.0,
            u_scale: 1.0,
            v_scale: 1.0,
            orientation: 1.0,
        }),
        Some(ProceduralSurfaceDefinition::Sum(definition_payload)) => Some(SurfaceMapping {
            base: model_sum_surface_partials(index, definition_payload, u, v)
                .ok()?
                .into_raw(),
            offset_distance: 0.0,
            u_scale: 1.0,
            v_scale: 1.0,
            orientation: 1.0,
        }),
        Some(ProceduralSurfaceDefinition::CurveBounded { support, .. }) => {
            model_surface_mapping(index, support, u, v, visiting, budget)
        }
        Some(ProceduralSurfaceDefinition::Replica { source, transform }) => {
            let source = model_surface_mapping(index, source, u, v, visiting, budget)?;
            let base = if source.offset_distance == 0.0 {
                source.base
            } else {
                offset_surface_second_partials(source.base, source.offset_distance)?
            };
            Some(SurfaceMapping {
                base: transform_surface_second_partials(base, *transform)?.into_raw(),
                offset_distance: 0.0,
                u_scale: source.u_scale,
                v_scale: source.v_scale,
                orientation: source.orientation * transform.orientation()?,
            })
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
                let support =
                    model_surface_mapping(index, support, support_u, support_v, visiting, budget)?;
                Some(SurfaceMapping {
                    base: support.base,
                    offset_distance: support.offset_distance,
                    u_scale: support.u_scale * u_derivative,
                    v_scale: support.v_scale * v_derivative,
                    orientation: support.orientation * u_derivative * v_derivative,
                })
            }
        }
        Some(ProceduralSurfaceDefinition::ParallelOffset(payload)) => {
            let support = model_surface_mapping(index, payload.support(), u, v, visiting, budget)?;
            Some(SurfaceMapping {
                offset_distance: support.offset_distance
                    + payload.distance().get() * support.orientation,
                ..support
            })
        }
        Some(ProceduralSurfaceDefinition::Offset(payload)) => {
            let support = model_surface_mapping(index, payload.support(), u, v, visiting, budget)?;
            Some(SurfaceMapping {
                offset_distance: support.offset_distance
                    + payload.distance().get() * support.orientation,
                ..support
            })
        }
        _ => {
            let base = surface_second_partials_with_budget(&carrier.geometry, u, v, budget)?;
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

/// The polyline's samples with the parameterization it evaluates on.
///
/// A sample row carries its own parameter, so the two lists this returns agree
/// by construction. An unparameterized polyline evaluates on its sample index.
fn polyline_samples(polyline: &PolylineCurve) -> (Vec<FinitePoint3>, Vec<FiniteReal>) {
    let points: Vec<FinitePoint3> = polyline.points().collect();
    let parameters = polyline.parameters().map_or_else(
        || (0..points.len()).map(FiniteReal::from_index).collect(),
        Iterator::collect,
    );
    (points, parameters)
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

fn polyline_point(
    points: &[FinitePoint3],
    parameters: &[FiniteReal],
    t: f64,
) -> Option<FinitePoint3> {
    if points.len() < 2 || points.len() != parameters.len() {
        return None;
    }
    let t = FiniteReal::new(t)?;
    let segment = parameters.windows(2).position(|window| {
        (t >= window[0] && t <= window[1]) || (t <= window[0] && t >= window[1])
    })?;
    let fraction = difference_quotient(
        t,
        parameters[segment],
        parameters[segment + 1],
        parameters[segment],
    )
    .ok()?
    .get();
    let start = points[segment].get();
    let end = points[segment + 1].get();
    let lerp =
        |start, end| crate::math::sum::finite_dot([1.0 - fraction, fraction], [start, end]).ok();
    Some(FinitePoint3::from_coordinates(
        lerp(start.x, end.x)?,
        lerp(start.y, end.y)?,
        lerp(start.z, end.z)?,
    ))
}

fn polyline_tangent(
    points: &[FinitePoint3],
    parameters: &[FiniteReal],
    t: f64,
) -> Option<FiniteVector3> {
    if points.len() < 2 || points.len() != parameters.len() {
        return None;
    }
    let t = FiniteReal::new(t)?;
    let mut tangent = None;
    for (segment, window) in parameters.windows(2).enumerate() {
        if !((t >= window[0] && t <= window[1]) || (t <= window[0] && t >= window[1])) {
            continue;
        }
        let [start_x, start_y, start_z] = points[segment].coordinates();
        let [end_x, end_y, end_z] = points[segment + 1].coordinates();
        let candidate = FiniteVector3::from_components(
            difference_quotient(end_x, start_x, window[1], window[0]).ok()?,
            difference_quotient(end_y, start_y, window[1], window[0]).ok()?,
            difference_quotient(end_z, start_z, window[1], window[0]).ok()?,
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
) -> Option<SurfaceSecondPartials<FinitePoint3, FiniteVector3>> {
    Some(SurfaceSecondPartials {
        point: transform.apply_point(partials.point)?,
        du: transform.apply_vector(partials.du)?,
        dv: transform.apply_vector(partials.dv)?,
        duu: transform.apply_vector(partials.duu)?,
        duv: transform.apply_vector(partials.duv)?,
        dvv: transform.apply_vector(partials.dvv)?,
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
            let major = scaled_sinh_cosh(hyperbola.major_radius().into(), parameter);
            let minor = scaled_sinh_cosh(hyperbola.minor_radius().into(), parameter);
            let reached = |pair: Result<(FiniteReal, FiniteReal), (f64, f64)>| {
                pair.map_or_else(|plain| plain, |(sinh, cosh)| (sinh.get(), cosh.get()))
            };
            let (major_sinh, major_cosh) = reached(major);
            let (minor_sinh, minor_cosh) = reached(minor);
            let zero = Point2::new(0.0, 0.0);
            let point = offset2(
                hyperbola.center().get(),
                &[(major_cosh, x.get()), (minor_sinh, y.get())],
            );
            let tangent = offset2(zero, &[(major_sinh, x.get()), (minor_cosh, y.get())]);
            if major.is_err() || minor.is_err() {
                return Some(PcurveEvaluation::left_finite_range(point, tangent));
            }
            (
                point,
                tangent,
                offset2(zero, &[(major_cosh, x.get()), (minor_sinh, y.get())]),
            )
        }
        PcurveGeometry::Hyperbolic(hyperbolic) => {
            let [cosine_u, cosine_v] = hyperbolic.cosine().coordinates();
            let [sine_u, sine_v] = hyperbolic.sine().coordinates();
            let [center_u, center_v] = hyperbolic.center().coordinates();
            // Each coordinate and its two derivatives, from the four scaled
            // hyperbolic products; `false` states that a product left the
            // finite range.
            let coordinate = |center: FiniteReal, cosine: FiniteReal, sine: FiniteReal| {
                let cosine = scaled_sinh_cosh(cosine, parameter);
                let sine = scaled_sinh_cosh(sine, parameter);
                let finite = cosine.is_ok() && sine.is_ok();
                let reached = |pair: Result<(FiniteReal, FiniteReal), (f64, f64)>| {
                    pair.map_or_else(|plain| plain, |(sinh, cosh)| (sinh.get(), cosh.get()))
                };
                let (cosine_sinh, cosine_cosh) = reached(cosine);
                let (sine_sinh, sine_cosh) = reached(sine);
                (
                    finite,
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
            let (Ok(point_u), Ok(point_v), true, true) = (u.1, v.1, u.0, v.0) else {
                return Some(PcurveEvaluation::left_finite_range(
                    Point2::new(reached(u.1), reached(v.1)),
                    Point2::new(reached(u.2), reached(v.2)),
                ));
            };
            return Some(PcurveEvaluation {
                point: Ok(FinitePoint2::from_coordinates(point_u, point_v)),
                tangent: planar_value(lane(u.2), lane(v.2)),
                acceleration: u
                    .3
                    .ok()
                    .zip(v.3.ok())
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
            return Some(PcurveEvaluation {
                point: FinitePoint2::new(point).ok_or(point),
                tangent: tangent.map_or(Err(EvaluationFailure::NoValue), admit_parameter_point),
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
pub fn curve_point(geometry: &CurveGeometry, t: f64) -> Option<FinitePoint3> {
    curve_point_solved(geometry.solved()?, t)
}

/// Evaluate the exact first derivative of a stored curve carrier.
pub fn curve_tangent(geometry: &CurveGeometry, t: f64) -> Option<FiniteVector3> {
    curve_tangent_solved(geometry.solved()?, t)
}

/// Evaluate the exact second derivative of a stored curve carrier.
pub fn curve_second_derivative(geometry: &CurveGeometry, t: f64) -> Option<FiniteVector3> {
    curve_second_derivative_solved(geometry.solved()?, t)
}

/// Evaluate a stored curve carrier at `t` within a caller-owned work slice.
pub fn curve_point_with_budget(
    geometry: &CurveGeometry,
    t: f64,
    budget: &WorkBudget<'_>,
) -> Option<FinitePoint3> {
    curve_point_with_budget_solved(geometry.solved()?, t, budget)
}

/// Evaluate the first derivative of a stored curve within a work slice.
pub fn curve_tangent_with_budget(
    geometry: &CurveGeometry,
    t: f64,
    budget: &WorkBudget<'_>,
) -> Option<FiniteVector3> {
    curve_tangent_with_budget_solved(geometry.solved()?, t, budget)
}

/// Evaluate the second derivative of a stored curve within a work slice.
pub fn curve_second_derivative_with_budget(
    geometry: &CurveGeometry,
    t: f64,
    budget: &WorkBudget<'_>,
) -> Option<FiniteVector3> {
    curve_second_derivative_with_budget_solved(geometry.solved()?, t, budget)
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

/// Evaluate the first partial derivatives of a surface carrier.
pub fn surface_partials(geometry: &SurfaceGeometry, u: f64, v: f64) -> Option<SurfacePartials> {
    surface_partials_solved(geometry.solved()?, u, v)
}

/// Evaluate the second partial derivatives of a surface carrier.
pub fn surface_second_partials(
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Option<SurfaceSecondPartials> {
    surface_second_partials_solved(geometry.solved()?, u, v)
}

/// Analytic surface parameters of the point on a surface carrier.
pub fn analytic_surface_parameters(
    geometry: &SurfaceGeometry,
    point: Point3,
) -> Option<FinitePoint2> {
    analytic_surface_parameters_solved(geometry.solved()?, point)
}

fn surface_partials_with_budget(
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Option<SurfacePartials> {
    surface_partials_with_budget_solved(geometry.solved()?, u, v, budget)
}

fn surface_second_partials_with_budget(
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Option<SurfaceSecondPartials> {
    surface_second_partials_with_budget_solved(geometry.solved()?, u, v, budget)
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
        None => model_surface_point(ir, geometry, u, v),
    }
}

#[cfg(test)]
mod numerical_range_tests;
