// SPDX-License-Identifier: Apache-2.0
//! B-spline basis, interpolation, extruded NURBS helpers, and tabulated-cylinder directrices.

use super::super::holes::placement::ExtrusionSpan;
use super::super::sketch::intersect::{section_point_in_model, section_xyz_in_model};
use crate::decode::analytic::edges::nurbs_intrinsic_parameter_range;
use crate::decode::analytic::planes::valid_positive_nurbs_curve;
use crate::vecmath::cross;
use crate::vecmath::normalize;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    pcurve::PcurveGeometry,
    SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};

const INTERPOLATION_DEGREE: usize = 3;
const EPS_TABULATED_ENDPOINT_ROUNDING: f64 = 1e-4;
const EPS_TABULATED_FRAME_EXACT: f64 = 1.0e-9;
const EPS_PLANAR_COORDINATE: f64 = 1.0e-12;

/// Names one saved-section spline entity by its stored entity identifier and
/// the byte offset of its entity label. A saved section states the entity
/// identifier only for entities the solver kept, so the byte offset is the
/// identity for the rest.
fn saved_spline_record(
    spline: &crate::feature::definitions::FeatureSavedSpline,
) -> impl std::fmt::Display + '_ {
    struct Record<'a>(&'a crate::feature::definitions::FeatureSavedSpline);
    impl std::fmt::Display for Record<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self.0.entity_id {
                Some(entity_id) => write!(
                    f,
                    "creo saved-spline entity {entity_id} at offset {}",
                    self.0.offset
                ),
                None => write!(f, "creo saved-spline entity at offset {}", self.0.offset),
            }
        }
    }
    Record(spline)
}

pub(in super::super) fn extruded_geometry_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
) -> Option<SurfaceGeometry> {
    match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            let start = section_point_in_model(transform, [start.u, start.v]);
            let end = section_point_in_model(transform, [end.u, end.v]);
            let line = normalize(std::array::from_fn(|axis| end[axis] - start[axis]))?;
            let normal = normalize(cross(line, transform.normal()))?;
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::from(start),
                    Vector3::from(normal),
                    Vector3::from(line),
                )
                .ok()?,
            )))
        }
        SketchGeometryDefinition::Arc { center, radius, .. }
        | SketchGeometryDefinition::Circle { center, radius } => {
            let center = section_point_in_model(transform, [center.u, center.v]);
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::from(center),
                    transform.normal_vector(),
                    transform.u_axis_vector(),
                    radius.get(),
                )
                .ok()?,
            )))
        }
        _ => None,
    }
}

pub(in super::super) fn bspline_basis(
    index: usize,
    degree: usize,
    parameter: f64,
    knots: &[f64],
    count: usize,
) -> Option<f64> {
    if degree > INTERPOLATION_DEGREE {
        return None;
    }
    let end = index.checked_add(degree)?.checked_add(1)?;
    let window = knots.get(index..=end)?;
    let first = *window.first()?;
    let second = *window.get(1)?;
    let last = *window.last()?;
    let last_knot = *knots.last()?;
    if parameter == last_knot {
        return Some(f64::from(index.checked_add(1) == Some(count)));
    }
    if degree == 0 {
        return Some(f64::from(first <= parameter && parameter < second));
    }
    let left_denominator = window[degree] - first;
    let right_denominator = last - second;
    let left = if left_denominator > 0.0 {
        let basis = bspline_basis(index, degree - 1, parameter, knots, count)?;
        (parameter - first) / left_denominator * basis
    } else {
        0.0
    };
    let right = if right_denominator > 0.0 {
        let basis = bspline_basis(index + 1, degree - 1, parameter, knots, count)?;
        (last - parameter) / right_denominator * basis
    } else {
        0.0
    };
    Some(left + right)
}

pub(in super::super) fn bspline_basis_derivative(
    index: usize,
    degree: usize,
    parameter: f64,
    knots: &[f64],
    count: usize,
) -> Option<f64> {
    if degree > INTERPOLATION_DEGREE {
        return None;
    }
    let end = index.checked_add(degree)?.checked_add(1)?;
    let window = knots.get(index..=end)?;
    if degree == 0 {
        return Some(0.0);
    }
    let degree_value = f64::from(u32::try_from(degree).ok()?);
    let left_denominator = window[degree] - window[0];
    let right_denominator = window[degree + 1] - window[1];
    let left = if left_denominator > 0.0 {
        let basis = bspline_basis(index, degree - 1, parameter, knots, count)?;
        degree_value / left_denominator * basis
    } else {
        0.0
    };
    let right = if right_denominator > 0.0 {
        let basis = bspline_basis(index + 1, degree - 1, parameter, knots, count)?;
        degree_value / right_denominator * basis
    } else {
        0.0
    };
    Some(left - right)
}

fn solve_vector_system(
    ctx: &DecodeContext<'_>,
    mut matrix: Vec<Vec<f64>>,
    mut values: Vec<[f64; 3]>,
) -> Result<Option<Vec<[f64; 3]>>, CodecError> {
    const EPS_INTERPOLATION_PIVOT: f64 = 1e-14;

    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let count = matrix.len();
    if values.len() != count {
        return Ok(None);
    }
    let mut rows = matrix.iter();
    while !rows.as_slice().is_empty() {
        let Some(row) = ctx.next_charged(&mut rows, "creo interpolation matrix shape")? else {
            break;
        };
        if row.len() != count {
            return Ok(None);
        }
    }
    let mut columns = 0..count;
    while columns.start < columns.end {
        let Some(column) = ctx.next_charged(&mut columns, "creo interpolation column scan")? else {
            break;
        };
        let Some(pivot) = ctx
            .admit_iter(column..count, "creo interpolation pivot work")?
            .max_by(|left, right| {
                matrix[*left][column]
                    .abs()
                    .total_cmp(&matrix[*right][column].abs())
            })
        else {
            return Ok(None);
        };
        if matrix[pivot][column].abs() <= EPS_INTERPOLATION_PIVOT || matrix[pivot][column].is_nan()
        {
            return Ok(None);
        }
        matrix.swap(column, pivot);
        values.swap(column, pivot);
        let scale = matrix[column][column];
        for value in ctx.admit_iter(
            &mut matrix[column][column..],
            "creo interpolation normalization work",
        )? {
            *value /= scale;
        }
        values[column] = values[column].map(|value| value / scale);
        let pivot_value = values[column];
        let (before, pivot_and_after) = matrix.split_at_mut(column);
        let Some((pivot_row, after)) = pivot_and_after.split_first_mut() else {
            return Ok(None);
        };
        let (before_values, pivot_and_after_values) = values.split_at_mut(column);
        let Some((_, after_values)) = pivot_and_after_values.split_first_mut() else {
            return Ok(None);
        };
        for (row, values) in ctx
            .admit_iter(before, "creo interpolation elimination row scan")?
            .chain(ctx.admit_iter(after, "creo interpolation elimination row scan")?)
            .zip(before_values.iter_mut().chain(after_values.iter_mut()))
        {
            let factor = row[column];
            if factor == 0.0 {
                continue;
            }
            for (entry, pivot_entry) in ctx
                .admit_iter(&mut row[column..], "creo interpolation elimination work")?
                .zip(&pivot_row[column..])
            {
                *entry -= factor * pivot_entry;
            }
            for (value, pivot) in values.iter_mut().zip(pivot_value) {
                *value -= factor * pivot;
            }
        }
    }
    Ok(Some(values))
}

fn interpolation_knots(
    ctx: &DecodeContext<'_>,
    parameters: &[f64],
) -> Result<Option<Vec<f64>>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let point_count = parameters.len();
    if point_count < 2 {
        return Ok(None);
    }
    let mut pairs = parameters.windows(2);
    while pairs.len() > 0 {
        let Some(pair) = ctx.next_charged(&mut pairs, "creo interpolation parameter order")? else {
            break;
        };
        if !(pair[0].is_finite() && pair[0] < pair[1]) {
            return Ok(None);
        }
    }
    if !parameters.last().is_some_and(|value| value.is_finite()) {
        return Ok(None);
    }
    let Some(knot_count) = point_count.checked_add(2 * INTERPOLATION_DEGREE) else {
        return Ok(None);
    };
    let mut knots = ctx.collection_vec(knot_count, "creo interpolation curve knots")?;
    knots.extend([parameters[0]; INTERPOLATION_DEGREE + 1]);
    knots.extend(
        ctx.admit_iter(
            &parameters[1..point_count - 1],
            "creo interpolation knot projection",
        )?
        .copied(),
    );
    knots.extend(std::iter::repeat_n(
        parameters[point_count - 1],
        INTERPOLATION_DEGREE + 1,
    ));
    Ok(Some(knots))
}

fn interpolation_controls(
    ctx: &DecodeContext<'_>,
    points: &[[f64; 3]],
    parameters: &[f64],
    knots: &[f64],
    endpoint_derivatives: [[f64; 3]; 2],
) -> Result<Option<Vec<[f64; 3]>>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if points.len() != parameters.len() || points.len() < 2 {
        return Ok(None);
    }
    let Some(control_count) = points.len().checked_add(2) else {
        return Ok(None);
    };
    let matrix_owned_storage = ctx.temporary_vec(0, "creo interpolation matrix rows")?;
    let mut matrix_storage = matrix_owned_storage.1;
    let mut matrix = matrix_owned_storage.0;
    ctx.reserve_scoped_vec(
        &mut matrix_storage,
        &mut matrix,
        control_count,
        "creo interpolation matrix rows",
    )?;
    let mut parameters_rows = parameters.iter();
    while !parameters_rows.as_slice().is_empty() {
        let Some(parameter) =
            ctx.next_charged(&mut parameters_rows, "creo interpolation parameter scan")?
        else {
            break;
        };
        let mut row = Vec::new();
        ctx.reserve_scoped_vec(
            &mut matrix_storage,
            &mut row,
            control_count,
            "creo interpolation matrix values",
        )?;
        let mut coefficients = 0..control_count;
        while coefficients.start < coefficients.end {
            let Some(index) =
                ctx.next_charged(&mut coefficients, "creo interpolation coefficient scan")?
            else {
                break;
            };
            let Some(basis) = bspline_basis(
                index,
                INTERPOLATION_DEGREE,
                *parameter,
                knots,
                control_count,
            ) else {
                return Ok(None);
            };
            row.push(basis);
        }
        matrix.push(row);
    }
    for parameter in [parameters[0], parameters[parameters.len() - 1]] {
        let mut row = Vec::new();
        ctx.reserve_scoped_vec(
            &mut matrix_storage,
            &mut row,
            control_count,
            "creo interpolation matrix values",
        )?;
        let mut coefficients = 0..control_count;
        while coefficients.start < coefficients.end {
            let Some(index) =
                ctx.next_charged(&mut coefficients, "creo interpolation coefficient scan")?
            else {
                break;
            };
            let Some(basis) = bspline_basis_derivative(
                index,
                INTERPOLATION_DEGREE,
                parameter,
                knots,
                control_count,
            ) else {
                return Ok(None);
            };
            row.push(basis);
        }
        matrix.push(row);
    }
    let mut values = Vec::new();
    ctx.reserve_vec(
        &mut values,
        control_count,
        "creo interpolation input values",
    )?;
    values.extend(
        ctx.admit_iter(points, "creo interpolation input projection")?
            .copied(),
    );
    values.extend(endpoint_derivatives);
    solve_vector_system(ctx, matrix, values)
}

pub(in super::super) fn saved_spline_nurbs(
    ctx: &DecodeContext<'_>,
    spline: &crate::feature::definitions::FeatureSavedSpline,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<NurbsCurve>, CodecError> {
    let Some(curve) = saved_spline_curve(ctx, spline)? else {
        return Ok(None);
    };
    match curve {
        Ok(curve) => Ok(Some(curve)),
        Err(error) => {
            refusal.note_checked(
                ctx,
                format_args!("{} NURBS record", saved_spline_record(spline)),
                &error,
            );
            Ok(None)
        }
    }
}

fn saved_spline_curve(
    ctx: &DecodeContext<'_>,
    spline: &crate::feature::definitions::FeatureSavedSpline,
) -> Result<Option<Result<NurbsCurve, cadmpeg_ir::geometry::nurbs::NurbsError>>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if spline
        .declared_point_count
        .and_then(|count| usize::try_from(count).ok())
        .is_none_or(|count| count != spline.interpolation_points.len())
    {
        return Ok(None);
    }
    let Some(parameters) = spline
        .parameters
        .as_ref()
        .map(|parameters| &parameters.value)
    else {
        return Ok(None);
    };
    let Some(tangents) = spline
        .endpoint_tangents
        .as_ref()
        .map(|tangents| tangents.value)
    else {
        return Ok(None);
    };
    if parameters.len() != spline.interpolation_points.len() {
        return Ok(None);
    }
    let Some(knots) = interpolation_knots(ctx, parameters)? else {
        return Ok(None);
    };
    let (control_points, _control_storage) =
        ctx.with_scoped_storage("creo saved spline interpolation scratch", || {
            interpolation_controls(
                ctx,
                &spline.interpolation_points,
                parameters,
                &knots,
                tangents,
            )
        })?;
    let Some(control_points) = control_points else {
        return Ok(None);
    };
    let converted_controls_owned_storage = ctx.temporary_vec(0, "creo saved spline controls")?;
    let mut converted_storage = converted_controls_owned_storage.1;
    let mut converted_controls = converted_controls_owned_storage.0;
    ctx.reserve_scoped_vec(
        &mut converted_storage,
        &mut converted_controls,
        control_points.len(),
        "creo saved spline controls",
    )?;
    converted_controls.extend(
        ctx.admit_iter(control_points, "creo saved spline control projection")?
            .map(Point3::from),
    );
    NurbsCurve::from_lanes(ctx, 3, knots, converted_controls, None, false).map(Some)
}

/// The first input of `spline` that states a `z` off the sketch plane, named as
/// the record names it, with the `z` the record states.
///
/// The record's own inputs are the subject, not the fitted control points. The
/// fit is a per-coordinate linear solve over the interpolation points and the
/// two endpoint tangents, so a `z` column that is all zeros answers a control
/// polygon whose `z` is all zeros: inputs on the sketch plane fit control
/// points on it. A control point off the plane therefore states an input off
/// the plane, and the input is the number the record's bytes carry.
fn saved_spline_off_plane_input(
    ctx: &DecodeContext<'_>,
    spline: &crate::feature::definitions::FeatureSavedSpline,
) -> Result<Option<(OffPlaneInput, f64)>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut points = spline.interpolation_points.iter().enumerate();
    while points.len() > 0 {
        let Some((index, point)) =
            ctx.next_charged(&mut points, "creo saved spline off-plane input scan")?
        else {
            break;
        };
        if point[2].abs() > EPS_PLANAR_COORDINATE {
            return Ok(Some((OffPlaneInput::Point(index), point[2])));
        }
    }
    Ok(spline.endpoint_tangents.as_ref().and_then(|tangents| {
        ["start", "end"]
            .into_iter()
            .zip(tangents.value)
            .find(|(_, tangent)| tangent[2].abs() > EPS_PLANAR_COORDINATE)
            .map(|(end, tangent)| (OffPlaneInput::Tangent(end), tangent[2]))
    }))
}

enum OffPlaneInput {
    Point(usize),
    Tangent(&'static str),
}

impl std::fmt::Display for OffPlaneInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Point(index) => write!(f, "interpolation point {index}"),
            Self::Tangent(end) => write!(f, "endpoint tangent {end}"),
        }
    }
}

pub(in super::super) fn saved_spline_sketch_geometry(
    ctx: &DecodeContext<'_>,
    spline: &crate::feature::definitions::FeatureSavedSpline,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<SketchGeometry>, CodecError> {
    if let Some((subject, z)) = saved_spline_off_plane_input(ctx, spline)? {
        refusal.note_checked(
            ctx,
            format_args!("{} sketch geometry record", saved_spline_record(spline)),
            &format_args!("{subject} is not on the sketch plane: z states {z}"),
        );
        return Ok(None);
    }
    let (curve, _curve_storage) = ctx
        .with_scoped_storage("creo saved spline sketch source curve", || {
            saved_spline_curve(ctx, spline)
        })?;
    let Some(curve) = curve else {
        return Ok(None);
    };
    let nurbs = match curve {
        Ok(curve) => curve,
        Err(error) => {
            refusal.note_checked(
                ctx,
                format_args!("{} NURBS record", saved_spline_record(spline)),
                &error,
            );
            return Ok(None);
        }
    };
    let knots = nurbs
        .knots()
        .try_clone_for_decode(ctx, "creo saved spline sketch knots")?;
    let poles = match nurbs.pole_rows() {
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
            let mut controls = Vec::new();
            ctx.reserve_vec(
                &mut controls,
                points.len(),
                "creo saved spline sketch controls",
            )?;
            for point in ctx.admit_iter(points, "creo NURBS point projection")? {
                let [x, y, _] = point.coordinates();
                controls.push(cadmpeg_ir::units::FinitePoint2::from_coordinates(x, y));
            }
            cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles::Polynomial { points: controls }
        }
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
            let mut paired = Vec::new();
            ctx.reserve_vec(
                &mut paired,
                points.len(),
                "creo saved spline sketch paired poles",
            )?;
            for pole in ctx.admit_iter(points, "creo NURBS pole projection")? {
                let [x, y, _] = pole.point.coordinates();
                paired.push(cadmpeg_ir::geometry::pcurve::WeightedPole2 {
                    point: cadmpeg_ir::units::FinitePoint2::from_coordinates(x, y),
                    weight: pole.weight,
                });
            }
            cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles::Rational { points: paired }
        }
    };
    match cadmpeg_ir::geometry::pcurve::PcurveNurbs::new(
        ctx,
        nurbs.degree(),
        knots,
        poles,
        nurbs.periodic(),
    )? {
        Ok(pcurve) => Ok(Some(SketchGeometry::nurbs(pcurve))),
        Err(error) => {
            refusal.note_checked(
                ctx,
                format_args!("{} sketch geometry record", saved_spline_record(spline)),
                &error,
            );
            Ok(None)
        }
    }
}

pub(in super::super) fn interpolation_spline_surface(
    ctx: &DecodeContext<'_>,
    grid: &crate::interpolation_grid::InterpolationGrid,
    record: &dyn std::fmt::Display,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<NurbsSurface>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let points = grid.points();
    let u_parameters = grid.u_parameters();
    let v_parameters = grid.v_parameters();
    let end_u_derivatives = grid.u_derivatives();
    let end_v_derivatives = grid.v_derivatives();
    let corner_mixed_derivatives = grid.mixed_derivatives();
    let u_sample_count = u_parameters.len();
    let v_sample_count = v_parameters.len();
    let Some(u_control_count) = u_sample_count.checked_add(2) else {
        return Ok(None);
    };
    let Some(v_control_count) = v_sample_count.checked_add(2) else {
        return Ok(None);
    };
    let Some(u_knots) = interpolation_knots(ctx, u_parameters)? else {
        return Ok(None);
    };
    let Some(v_knots) = interpolation_knots(ctx, v_parameters)? else {
        return Ok(None);
    };
    let mut interpolation_storage = ctx.reserve_scoped(0, "creo interpolation surface scratch")?;
    let mut position_controls = interpolation_storage.with_storage(|| {
        ctx.collect_indexed_vec(
            u_control_count,
            "creo interpolation surface position controls",
            |_| Ok(Vec::<[f64; 3]>::new()),
        )
    })?;
    for row in ctx.admit_iter(
        &mut position_controls,
        "creo interpolation position row scan",
    )? {
        interpolation_storage.with_storage(|| {
            ctx.reserve_vec(
                row,
                v_sample_count,
                "creo interpolation surface position row",
            )
        })?;
    }
    let mut v_samples = 0..v_sample_count;
    while v_samples.start < v_samples.end {
        let Some(v) = ctx.next_charged(&mut v_samples, "creo interpolation v sample scan")? else {
            break;
        };
        let samples_owned_storage =
            ctx.temporary_vec(0, "creo interpolation surface position samples")?;
        let mut sample_storage = samples_owned_storage.1;
        let mut samples = samples_owned_storage.0;
        ctx.reserve_scoped_vec(
            &mut sample_storage,
            &mut samples,
            u_sample_count,
            "creo interpolation surface position samples",
        )?;
        samples.extend(
            ctx.admit_iter(0..u_sample_count, "creo interpolation sample projection")?
                .map(|u| points[u * v_sample_count + v]),
        );
        let (controls, _control_storage) =
            ctx.with_scoped_storage("creo interpolation fitted control scratch", || {
                interpolation_controls(
                    ctx,
                    &samples,
                    u_parameters,
                    &u_knots,
                    [end_u_derivatives[v], end_u_derivatives[v_sample_count + v]],
                )
            })?;
        let Some(controls) = controls else {
            return Ok(None);
        };
        for (u, control) in ctx
            .admit_iter(controls, "creo interpolation position control scan")?
            .enumerate()
        {
            position_controls[u].push(control);
        }
    }
    let mut v_derivative_controls = [Vec::new(), Vec::new()];
    for (v_boundary, derivative_controls) in v_derivative_controls.iter_mut().enumerate() {
        let samples_owned_storage =
            ctx.temporary_vec(0, "creo interpolation surface derivative samples")?;
        let mut sample_storage = samples_owned_storage.1;
        let mut samples = samples_owned_storage.0;
        ctx.reserve_scoped_vec(
            &mut sample_storage,
            &mut samples,
            u_sample_count,
            "creo interpolation surface derivative samples",
        )?;
        samples.extend(
            ctx.admit_iter(
                0..u_sample_count,
                "creo interpolation derivative sample projection",
            )?
            .map(|u| end_v_derivatives[v_boundary * u_sample_count + u]),
        );
        let controls = interpolation_storage.with_storage(|| {
            interpolation_controls(
                ctx,
                &samples,
                u_parameters,
                &u_knots,
                [
                    corner_mixed_derivatives[v_boundary * 2],
                    corner_mixed_derivatives[v_boundary * 2 + 1],
                ],
            )
        })?;
        let Some(controls) = controls else {
            return Ok(None);
        };
        *derivative_controls = controls;
    }
    let pole_rows_owned_storage =
        ctx.temporary_vec(0, "creo interpolation surface NURBS pole rows")?;
    let mut pole_storage = pole_rows_owned_storage.1;
    let mut pole_rows = pole_rows_owned_storage.0;
    ctx.reserve_scoped_vec(
        &mut pole_storage,
        &mut pole_rows,
        u_control_count,
        "creo interpolation surface NURBS pole rows",
    )?;
    let mut u_controls = 0..u_control_count;
    while u_controls.start < u_controls.end {
        let Some(u) = ctx.next_charged(&mut u_controls, "creo interpolation u control scan")?
        else {
            break;
        };
        let (controls, _control_storage) =
            ctx.with_scoped_storage("creo interpolation fitted control scratch", || {
                interpolation_controls(
                    ctx,
                    &position_controls[u],
                    v_parameters,
                    &v_knots,
                    [v_derivative_controls[0][u], v_derivative_controls[1][u]],
                )
            })?;
        let Some(controls) = controls else {
            return Ok(None);
        };
        let mut row = Vec::new();
        pole_storage.with_storage(|| {
            ctx.reserve_vec(
                &mut row,
                controls.len(),
                "creo interpolation surface NURBS pole values",
            )
        })?;
        row.extend(
            ctx.admit_iter(controls, "creo interpolation pole projection")?
                .map(Point3::from),
        );
        pole_rows.push(row);
    }
    if u32::try_from(v_control_count).is_err() {
        return Ok(None);
    }
    match NurbsSurface::from_lanes(
        ctx,
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(3, u_knots, false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(3, v_knots, false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(pole_rows, None),
        false,
    )? {
        Ok(surface) => Ok(Some(surface)),
        Err(error) => {
            refusal.note_checked(
                ctx,
                format_args!("creo interpolation-spline surface record for {record}"),
                &error,
            );
            Ok(None)
        }
    }
}

pub(in super::super) fn placed_section_nurbs(
    ctx: &DecodeContext<'_>,
    transform: &crate::placement::FeatureSectionTransform,
    nurbs: &NurbsCurve,
) -> Result<Option<NurbsCurve>, CodecError> {
    nurbs.map_control_points(ctx, "creo placed section NURBS curve", |point| {
        Point3::from(section_xyz_in_model(transform, [point.x, point.y, point.z]))
    })
}

pub(super) fn translated_nurbs_curve(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    translation: [f64; 3],
) -> Result<Option<NurbsCurve>, CodecError> {
    curve.map_control_points(ctx, "creo translated NURBS curve", |point| {
        Point3::new(
            point.x + translation[0],
            point.y + translation[1],
            point.z + translation[2],
        )
    })
}

pub(in super::super) fn extruded_nurbs_surface(
    ctx: &DecodeContext<'_>,
    directrix: &NurbsCurve,
    sweep: [f64; 3],
    record: &dyn std::fmt::Display,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<NurbsSurface>, CodecError> {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurfaceAxis, WeightedPole3};
    use cadmpeg_ir::scalar::NonZeroReal;

    let count = directrix.pole_count();
    let rational = matches!(
        directrix.pole_rows(),
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { .. }
    );
    let mut polynomial_rows = Vec::new();
    let mut rational_rows = Vec::new();
    if rational {
        ctx.reserve_vec(&mut rational_rows, count, "creo extruded NURBS pole rows")?;
    } else {
        ctx.reserve_vec(&mut polynomial_rows, count, "creo extruded NURBS pole rows")?;
    }
    let mut source_poles = 0..count;
    while source_poles.start < source_poles.end {
        let Some(index) =
            ctx.next_charged(&mut source_poles, "creo extruded NURBS source pole scan")?
        else {
            break;
        };
        let Some(point) = directrix.pole_rows().point_at(index) else {
            return Ok(None);
        };
        let source = point.get();
        let translated = Point3::new(
            source.x + sweep[0],
            source.y + sweep[1],
            source.z + sweep[2],
        );
        let Some(translated) = FinitePoint3::new(translated) else {
            refusal.note_checked(
                ctx,
                format_args!("creo extruded NURBS surface record for {record}"),
                &cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                    "control_points contains a non-finite point".into(),
                ),
            );
            return Ok(None);
        };
        if rational {
            let Some(weight) = directrix
                .pole_rows()
                .weight_at(index)
                .and_then(NonZeroReal::new)
            else {
                return Ok(None);
            };
            let mut row = Vec::new();
            ctx.reserve_vec(&mut row, 2, "creo extruded NURBS pole values")?;
            row.extend([
                WeightedPole3 { point, weight },
                WeightedPole3 {
                    point: translated,
                    weight,
                },
            ]);
            rational_rows.push(row);
        } else {
            let mut row = Vec::new();
            ctx.reserve_vec(&mut row, 2, "creo extruded NURBS pole values")?;
            row.extend([point, translated]);
            polynomial_rows.push(row);
        }
    }
    let poles = if rational {
        NurbsPoleGrid::Rational {
            rows: rational_rows,
        }
    } else {
        NurbsPoleGrid::Polynomial {
            rows: polynomial_rows,
        }
    };
    let u_knots = directrix
        .knots()
        .try_clone_for_decode(ctx, "creo extruded NURBS U knots")?;
    let mut v_knots = Vec::new();
    ctx.reserve_vec(&mut v_knots, 4, "creo extruded NURBS V knots")?;
    v_knots.extend([0.0, 0.0, 1.0, 1.0]);
    let v_knots = match cadmpeg_ir::geometry::nurbs::KnotVector::new(ctx, v_knots)? {
        Ok(knots) => knots,
        Err(error) => {
            refusal.note_checked(
                ctx,
                format_args!("creo extruded NURBS surface record for {record}"),
                &error,
            );
            return Ok(None);
        }
    };
    match NurbsSurface::new(
        ctx,
        NurbsSurfaceAxis::new(directrix.degree(), u_knots, directrix.periodic()),
        NurbsSurfaceAxis::new(1, v_knots, false),
        poles,
        false,
    )? {
        Ok(surface) => Ok(Some(surface)),
        Err(error) => {
            refusal.note_checked(
                ctx,
                format_args!("creo extruded NURBS surface record for {record}"),
                &error,
            );
            Ok(None)
        }
    }
}

pub(super) fn sketch_nurbs_curve(
    ctx: &DecodeContext<'_>,
    geometry: &SketchGeometry,
) -> Result<Option<NurbsCurve>, CodecError> {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::nurbs::{NurbsPoles3, WeightedPole3};
    use cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles;
    use cadmpeg_ir::scalar::FiniteReal;

    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }

    let SketchGeometryDefinition::Nurbs { curve } = geometry.definition() else {
        return Ok(None);
    };
    let knots = curve
        .knots()
        .try_clone_for_decode(ctx, "creo sketch NURBS lift knots")?;
    let poles = match curve.pole_rows() {
        PcurveNurbsPoles::Polynomial { points } => {
            let mut lifted = Vec::new();
            ctx.reserve_vec(&mut lifted, points.len(), "creo sketch NURBS lift poles")?;
            for point in ctx.admit_iter(points, "creo NURBS point projection")? {
                let [x, y] = point.coordinates();
                lifted.push(FinitePoint3::from_coordinates(x, y, FiniteReal::ZERO));
            }
            NurbsPoles3::Polynomial { points: lifted }
        }
        PcurveNurbsPoles::Rational { points } => {
            let mut lifted = Vec::new();
            ctx.reserve_vec(&mut lifted, points.len(), "creo sketch NURBS lift poles")?;
            for pole in ctx.admit_iter(points, "creo NURBS pole projection")? {
                let [x, y] = pole.point.coordinates();
                lifted.push(WeightedPole3 {
                    point: FinitePoint3::from_coordinates(x, y, FiniteReal::ZERO),
                    weight: pole.weight,
                });
            }
            NurbsPoles3::Rational { points: lifted }
        }
    };
    let Some(nurbs) = NurbsCurve::new(ctx, curve.degree(), knots, poles, curve.periodic())?.ok()
    else {
        return Ok(None);
    };
    Ok(valid_positive_nurbs_curve(ctx, &nurbs)?.map(|()| nurbs))
}

pub(super) fn oriented_sketch_nurbs_curve(
    ctx: &DecodeContext<'_>,
    geometry: &SketchGeometry,
    reversed: bool,
) -> Result<Option<NurbsCurve>, CodecError> {
    let Some(mut nurbs) = sketch_nurbs_curve(ctx, geometry)? else {
        return Ok(None);
    };
    if !reversed {
        return Ok(Some(nurbs));
    }
    let Some([lower, upper]) = nurbs_intrinsic_parameter_range(&nurbs) else {
        return Ok(None);
    };
    Ok(nurbs
        .reverse_parameterization_in_range(ctx, lower, upper)?
        .map(|()| nurbs))
}

pub(super) fn sketch_nurbs_pcurve(
    ctx: &DecodeContext<'_>,
    geometry: &SketchGeometry,
    reversed: bool,
    record: &dyn std::fmt::Display,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<PcurveGeometry>, CodecError> {
    use cadmpeg_ir::geometry::nurbs::NurbsPoles3;
    use cadmpeg_ir::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles, WeightedPole2};

    let (nurbs, _curve_storage) = ctx
        .with_scoped_storage("creo sketch pcurve lift scratch", || {
            oriented_sketch_nurbs_curve(ctx, geometry, reversed)
        })?;
    let Some(nurbs) = nurbs else {
        return Ok(None);
    };
    let knots = nurbs
        .knots()
        .try_clone_for_decode(ctx, "creo sketch NURBS pcurve knots")?;
    let poles = match nurbs.pole_rows() {
        NurbsPoles3::Polynomial { points } => {
            let mut projected = Vec::new();
            ctx.reserve_vec(
                &mut projected,
                points.len(),
                "creo sketch NURBS pcurve poles",
            )?;
            for point in ctx.admit_iter(points, "creo NURBS point projection")? {
                let [x, y, _] = point.coordinates();
                projected.push(cadmpeg_ir::units::FinitePoint2::from_coordinates(x, y));
            }
            PcurveNurbsPoles::Polynomial { points: projected }
        }
        NurbsPoles3::Rational { points } => {
            let mut projected = Vec::new();
            ctx.reserve_vec(
                &mut projected,
                points.len(),
                "creo sketch NURBS pcurve poles",
            )?;
            for pole in ctx.admit_iter(points, "creo NURBS pole projection")? {
                let [x, y, _] = pole.point.coordinates();
                projected.push(WeightedPole2 {
                    point: cadmpeg_ir::units::FinitePoint2::from_coordinates(x, y),
                    weight: pole.weight,
                });
            }
            PcurveNurbsPoles::Rational { points: projected }
        }
    };
    match PcurveNurbs::new(ctx, nurbs.degree(), knots, poles, nurbs.periodic())? {
        Ok(nurbs) => Ok(Some(PcurveGeometry::Nurbs { nurbs })),
        Err(error) => {
            refusal.note_checked(
                ctx,
                format_args!("creo sketch NURBS pcurve record for {record}"),
                &error,
            );
            Ok(None)
        }
    }
}

pub(in super::super) fn extrusion_brep_side_surface(
    ctx: &DecodeContext<'_>,
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
    reversed: bool,
    endpoints: [[f64; 2]; 2],
    span: ExtrusionSpan,
    diagnostics: &mut crate::lane_refusal::LaneRefusalContext<'_, '_>,
) -> Result<Option<SurfaceGeometry>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let [start, end] = endpoints;

    if matches!(
        geometry.definition(),
        SketchGeometryDefinition::Nurbs { .. }
    ) {
        let (directrix, _directrix_storage) = ctx
            .with_scoped_storage("creo extrusion directrix scratch", || {
                oriented_sketch_nurbs_curve(ctx, geometry, reversed)
            })?;
        let Some(directrix) = directrix else {
            return Ok(None);
        };
        let lower_translation = transform.normal().map(|value| value * span.lower());
        let sweep = transform
            .normal()
            .map(|value| value * (span.upper() - span.lower()));
        let (placed, _placed_storage) = ctx
            .with_scoped_storage("creo extrusion directrix scratch", || {
                placed_section_nurbs(ctx, transform, &directrix)
            })?;
        let Some(placed) = placed else {
            return Ok(None);
        };
        let (translated, _translated_storage) = ctx
            .with_scoped_storage("creo extrusion directrix scratch", || {
                translated_nurbs_curve(ctx, &placed, lower_translation)
            })?;
        let Some(translated) = translated else {
            return Ok(None);
        };
        let Some(surface) = extruded_nurbs_surface(
            ctx,
            &translated,
            sweep,
            diagnostics.record,
            diagnostics.refusals,
        )?
        else {
            return Ok(None);
        };
        return Ok(Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            surface,
        ))));
    }
    let surface = match geometry.definition() {
        SketchGeometryDefinition::Line { .. } => {
            let Ok(line) = SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start[0], start[1]),
                end: Point2::new(end[0], end[1]),
            }) else {
                return Ok(None);
            };
            extruded_geometry_surface(transform, &line)
        }
        _ => extruded_geometry_surface(transform, geometry),
    };
    Ok(surface)
}

pub(in super::super) fn signed_unit_chart(
    local: [f64; 2],
    frame: [f64; 2],
    offset: f64,
) -> Option<(f64, f64)> {
    let endpoint_close = |left: f64, right: f64| {
        (left - right).abs()
            <= EPS_TABULATED_ENDPOINT_ROUNDING * left.abs().max(right.abs()).max(1.0)
    };
    let frame_close = |left: f64, right: f64| {
        (left - right).abs() <= EPS_TABULATED_FRAME_EXACT * left.abs().max(right.abs()).max(1.0)
    };
    let mut mapping = None;
    for first_sign in [-1.0, 1.0] {
        for second_sign in [-1.0, 1.0] {
            let frame = [first_sign * frame[0], second_sign * frame[1]];
            for reversed in [false, true] {
                let target = if reversed {
                    [frame[1], frame[0]]
                } else {
                    frame
                };
                let slope = if reversed { -1.0 } else { 1.0 };
                let chart_intercept = target[0] - slope * local[0];
                if endpoint_close(target[1], slope * local[1] + chart_intercept)
                    && frame_close(chart_intercept.abs(), offset)
                {
                    let candidate = (slope, chart_intercept);
                    if mapping.is_some_and(|existing| existing != candidate) {
                        return None;
                    }
                    mapping = Some(candidate);
                }
            }
        }
    }
    mapping
}

fn tabulated_cylinder_placement(
    replay: &crate::surface::TabulatedCylinderCurveReplay,
    parameters: &crate::surface::SurfaceParameterRecord,
    chart_origin: Option<[f64; 3]>,
) -> Option<([Point3; 4], [f64; 3])> {
    #[derive(Clone, Copy)]
    enum FrameLayout {
        LegacyReflected,
        PrototypeOffsetPlanar,
        ZeroOffsetPlanar,
        SelectedPlanar,
    }
    if parameters.boundary != crate::surface::SurfaceBodyBoundary::CompoundClose {
        return None;
    }
    let [Some(p0), Some(p1), Some(p2), Some(p3)] = replay.control_points else {
        return None;
    };
    let points = [p0, p1, p2, p3];
    let (values, layout) = parameters
        .tabulated_cylinder_frame()
        .map(|frame| {
            let values = frame.values().get();
            let heads = frame.prefixes();
            let offset_planar_layout = matches!(heads.as_slice(), [_, 0x46, _, _, 0x46, _]);
            let zero_offset_layout = matches!(heads.as_slice(), [_, 0x42, _, _, 0x18, _]);
            if offset_planar_layout {
                (values, FrameLayout::PrototypeOffsetPlanar)
            } else if zero_offset_layout {
                (values, FrameLayout::ZeroOffsetPlanar)
            } else {
                (values, FrameLayout::SelectedPlanar)
            }
        })
        .or_else(|| {
            let [_, frame] = parameters.scalar_frames.as_slice() else {
                return None;
            };
            let [a0, a1, a2, b0, b1, b2] = frame.slots.as_slice() else {
                return None;
            };
            let values = [
                a0.value?, a1.value?, a2.value?, b0.value?, b1.value?, b2.value?,
            ];
            Some((values, FrameLayout::LegacyReflected))
        })?;
    let [a0, a1, a2, b0, b1, b2] = &values;
    let first = [*a0, *a1, *a2];
    let second = [*b0, *b1, *b2];
    let local_start = points.first()?;
    let local_end = points.last()?;
    let local_span = [
        (local_end[0] - local_start[0]).abs(),
        (local_end[1] - local_start[1]).abs(),
    ];
    if local_span
        .iter()
        .any(|span| !span.is_finite() || *span <= 0.0)
    {
        return None;
    }
    let close = |left: f64, right: f64| {
        (left - right).abs() <= EPS_TABULATED_FRAME_EXACT * left.abs().max(right.abs()).max(1.0)
    };
    let axis_matches = |axis: usize, coordinate: usize| match layout {
        FrameLayout::LegacyReflected => {
            close((second[axis] - first[axis]).abs(), local_span[coordinate])
        }
        FrameLayout::PrototypeOffsetPlanar => chart_origin.is_some_and(|origin| {
            signed_unit_chart(
                [local_start[coordinate], local_end[coordinate]],
                [first[axis], second[axis]],
                if coordinate == 0 {
                    origin[axis].abs()
                } else {
                    0.0
                },
            )
            .is_some()
        }),
        FrameLayout::ZeroOffsetPlanar => signed_unit_chart(
            [local_start[coordinate], local_end[coordinate]],
            [first[axis], second[axis]],
            0.0,
        )
        .is_some(),
        FrameLayout::SelectedPlanar => {
            let zero_offset = signed_unit_chart(
                [local_start[coordinate], local_end[coordinate]],
                [first[axis], second[axis]],
                0.0,
            )
            .is_some();
            let prototype_offset = (coordinate == 0)
                .then(|| chart_origin.map(|origin| origin[axis].abs()))
                .flatten()
                .filter(|offset| offset.is_finite() && !close(*offset, 0.0))
                .is_some_and(|offset| {
                    signed_unit_chart(
                        [local_start[coordinate], local_end[coordinate]],
                        [first[axis], second[axis]],
                        offset,
                    )
                    .is_some()
                });
            zero_offset || prototype_offset
        }
    };
    let mut assignment = None;
    for candidate in (0..3).flat_map(|first_axis| {
        (0..3)
            .filter(move |&second_axis| {
                first_axis != second_axis
                    && axis_matches(first_axis, 0)
                    && axis_matches(second_axis, 1)
            })
            .map(move |second_axis| (first_axis, second_axis, 3 - first_axis - second_axis))
    }) {
        if assignment.replace(candidate).is_some() {
            return None;
        }
    }
    let (first_axis, second_axis, sweep_axis) = assignment?;
    let (signed_chart, reflect_sweep) = match layout {
        FrameLayout::LegacyReflected => (None, false),
        FrameLayout::PrototypeOffsetPlanar => (
            Some((
                signed_unit_chart(
                    [local_start[0], local_end[0]],
                    [first[first_axis], second[first_axis]],
                    chart_origin?[first_axis].abs(),
                )?,
                signed_unit_chart(
                    [local_start[1], local_end[1]],
                    [first[second_axis], second[second_axis]],
                    0.0,
                )?,
            )),
            false,
        ),
        FrameLayout::ZeroOffsetPlanar => (
            Some((
                signed_unit_chart(
                    [local_start[0], local_end[0]],
                    [first[first_axis], second[first_axis]],
                    0.0,
                )?,
                signed_unit_chart(
                    [local_start[1], local_end[1]],
                    [first[second_axis], second[second_axis]],
                    0.0,
                )?,
            )),
            false,
        ),
        FrameLayout::SelectedPlanar => {
            let first_intercepts = [
                Some((0.0, false)),
                chart_origin.and_then(|origin| {
                    let intercept = origin[first_axis].abs();
                    (intercept.is_finite() && !close(intercept, 0.0)).then_some((intercept, true))
                }),
            ];
            let mut selected = None;
            for candidate in first_intercepts.into_iter().flatten().filter_map(
                |(first_offset, reflect_sweep)| {
                    Some((
                        (
                            signed_unit_chart(
                                [local_start[0], local_end[0]],
                                [first[first_axis], second[first_axis]],
                                first_offset,
                            )?,
                            signed_unit_chart(
                                [local_start[1], local_end[1]],
                                [first[second_axis], second[second_axis]],
                                0.0,
                            )?,
                        ),
                        reflect_sweep,
                    ))
                },
            ) {
                if selected.replace(candidate).is_some() {
                    return None;
                }
            }
            let (chart, reflect_sweep) = selected?;
            (Some(chart), reflect_sweep)
        }
    };
    let control_points = points.map(|point| {
        let mut placed = [0.0; 3];
        match signed_chart {
            Some(((first_slope, first_intercept), (second_slope, second_intercept))) => {
                placed[first_axis] = first_slope * point[0] + first_intercept;
                placed[second_axis] = second_slope * point[1] + second_intercept;
                placed[sweep_axis] = if reflect_sweep {
                    -first[sweep_axis]
                } else {
                    first[sweep_axis]
                };
            }
            None => {
                let chart_first =
                    first[first_axis].max(second[first_axis]) - (point[0] - local_start[0]);
                let chart_second =
                    first[second_axis].min(second[second_axis]) + (point[1] - local_start[1]);
                placed[first_axis] = if first_axis < 2 {
                    -chart_first
                } else {
                    chart_first
                };
                placed[second_axis] = if second_axis < 2 {
                    -chart_second
                } else {
                    chart_second
                };
                placed[sweep_axis] = first[sweep_axis];
            }
        }
        Point3::from(placed)
    });
    let mut sweep = [0.0; 3];
    sweep[sweep_axis] = if reflect_sweep {
        first[sweep_axis] - second[sweep_axis]
    } else {
        second[sweep_axis] - first[sweep_axis]
    };
    if !sweep[sweep_axis].is_finite() || sweep[sweep_axis] == 0.0 {
        return None;
    }
    Some((control_points, sweep))
}

pub(in super::super) fn placed_tabulated_cylinder_directrix(
    ctx: &DecodeContext<'_>,
    replay: &crate::surface::TabulatedCylinderCurveReplay,
    parameters: &crate::surface::SurfaceParameterRecord,
    chart_origin: Option<[f64; 3]>,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<(NurbsCurve, [f64; 3])>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some((control_points, sweep)) =
        tabulated_cylinder_placement(replay, parameters, chart_origin)
    else {
        return Ok(None);
    };
    let controls_owned_storage =
        ctx.temporary_vec(0, "creo tabulated-cylinder directrix controls")?;
    let mut control_storage = controls_owned_storage.1;
    let mut controls = controls_owned_storage.0;
    ctx.reserve_scoped_vec(
        &mut control_storage,
        &mut controls,
        control_points.len(),
        "creo tabulated-cylinder directrix controls",
    )?;
    controls.extend(control_points);
    let mut knots = Vec::new();
    ctx.reserve_vec(&mut knots, 8, "creo tabulated-cylinder directrix knots")?;
    knots.extend([0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
    match NurbsCurve::from_lanes(ctx, 3, knots, controls, None, false)? {
        Ok(curve) => Ok(Some((curve, sweep))),
        Err(error) => {
            refusal.note_checked(
                ctx,
                format_args!(
                    "creo placed tabulated-cylinder directrix record for surface {} at offset {}",
                    parameters.surface_id, parameters.offset
                ),
                &error,
            );
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extruded_nurbs_surface, oriented_sketch_nurbs_curve, placed_section_nurbs,
        signed_unit_chart, translated_nurbs_curve,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
    use cadmpeg_ir::math::{Point2, Point3};
    use cadmpeg_ir::sketches::SketchGeometry;

    fn with_collection_limit<T>(limit: u64, run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
        run(&ctx)
    }

    fn sketch_line_nurbs() -> SketchGeometry {
        SketchGeometry::nurbs(
            PcurveNurbs::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                None,
                false,
            )
            .expect("fixture pcurve construction admission")
            .expect("linear sketch NURBS"),
        )
    }

    #[test]
    fn sketch_nurbs_lift_knots_refuse_collection_limit() {
        let geometry = sketch_line_nurbs();
        let run =
            |limit| with_collection_limit(limit, |ctx| super::sketch_nurbs_curve(ctx, &geometry));
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo sketch NURBS lift knots"),
            run,
        );
        let error = run(limit).expect_err("named collection boundary");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo sketch NURBS lift knots")
        );
        assert!(
            with_collection_limit(6, |ctx| super::sketch_nurbs_curve(ctx, &geometry))
                .expect("six collection items")
                .is_some()
        );
    }

    #[test]
    fn sketch_nurbs_lift_poles_refuse_collection_limit() {
        let geometry = sketch_line_nurbs();
        let run =
            |limit| with_collection_limit(limit, |ctx| super::sketch_nurbs_curve(ctx, &geometry));
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo sketch NURBS lift poles"),
            run,
        );
        let error = run(limit).expect_err("named collection boundary");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo sketch NURBS lift poles")
        );
        assert!(
            with_collection_limit(6, |ctx| super::sketch_nurbs_curve(ctx, &geometry))
                .expect("six collection items")
                .is_some()
        );
    }

    #[test]
    fn sketch_nurbs_pcurve_knots_refuse_collection_limit() {
        let geometry = sketch_line_nurbs();
        let run = |limit| {
            with_collection_limit(limit, |ctx| {
                super::sketch_nurbs_pcurve(
                    ctx,
                    &geometry,
                    false,
                    &"linear sketch",
                    &mut crate::lane_refusal::LaneRefusals::new(),
                )
            })
        };
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo sketch NURBS pcurve knots"),
            run,
        );
        let error = run(limit).expect_err("named collection boundary");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo sketch NURBS pcurve knots")
        );
        assert!(with_collection_limit(12, |ctx| {
            super::sketch_nurbs_pcurve(
                ctx,
                &geometry,
                false,
                &"linear sketch",
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
        .expect("twelve collection items")
        .is_some());
    }

    #[test]
    fn sketch_nurbs_pcurve_poles_refuse_collection_limit() {
        let geometry = sketch_line_nurbs();
        let run = |limit| {
            with_collection_limit(limit, |ctx| {
                super::sketch_nurbs_pcurve(
                    ctx,
                    &geometry,
                    false,
                    &"linear sketch",
                    &mut crate::lane_refusal::LaneRefusals::new(),
                )
            })
        };
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo sketch NURBS pcurve poles"),
            run,
        );
        let error = run(limit).expect_err("named collection boundary");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo sketch NURBS pcurve poles")
        );
        assert!(with_collection_limit(12, |ctx| {
            super::sketch_nurbs_pcurve(
                ctx,
                &geometry,
                false,
                &"linear sketch",
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
        .expect("twelve collection items")
        .is_some());
    }

    fn interpolation_grid() -> crate::interpolation_grid::InterpolationGrid {
        crate::decode::with_test_decode_ctx(|ctx| {
            crate::interpolation_grid::InterpolationGrid::try_new(
                ctx,
                vec![
                    [0.0, 0.0, 0.0],
                    [0.0, 1.0, 2.0],
                    [1.0, 0.0, 1.0],
                    [1.0, 1.0, 3.0],
                ],
                vec![0.0, 1.0],
                vec![0.0, 1.0],
                vec![[1.0, 0.0, 1.0]; 4],
                vec![[0.0, 1.0, 2.0]; 4],
                [[0.0; 3]; 4],
            )
        })
        .expect("grid work admission")
        .expect("complete interpolation grid")
    }

    fn interpolation_surface_refusal(operation: &'static str) -> cadmpeg_core::CodecError {
        let grid = interpolation_grid();
        let run = |limit| {
            with_collection_limit(limit, |ctx| {
                super::interpolation_spline_surface(
                    ctx,
                    &grid,
                    &"interpolation grid fixture",
                    &mut crate::lane_refusal::LaneRefusals::new(),
                )
            })
        };
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some(operation),
            run,
        );
        run(limit).expect_err("interpolation allocation exceeds the collection limit")
    }

    #[test]
    fn saved_spline_constructor_refuses_typed_pole_storage() {
        let run = |limit| {
            with_collection_limit(limit, |ctx| {
                super::saved_spline_nurbs(
                    ctx,
                    &planar_or_offset_spline(0.0),
                    &mut crate::lane_refusal::LaneRefusals::new(),
                )
            })
        };
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("IR NURBS admitted poles"),
            run,
        );
        let error = run(limit).expect_err("named collection boundary");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == "IR NURBS admitted poles")
        );
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| super::saved_spline_nurbs(
                ctx,
                &planar_or_offset_spline(0.0),
                &mut crate::lane_refusal::LaneRefusals::new()
            ))
            .expect("service constructor")
            .is_some()
        );
    }

    #[test]
    fn interpolation_constructor_refuses_typed_grid_storage() {
        assert!(
            matches!(interpolation_surface_refusal("IR NURBS admitted grid rows"), cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == "IR NURBS admitted grid rows")
        );
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| super::interpolation_spline_surface(
                ctx,
                &interpolation_grid(),
                &"interpolation grid fixture",
                &mut crate::lane_refusal::LaneRefusals::new()
            ))
            .expect("service constructor")
            .is_some()
        );
    }

    #[test]
    fn interpolation_dense_solver_refuses_each_work_boundary() {
        crate::test_support::assert_work_boundaries(
            &[
                "creo interpolation pivot work",
                "creo interpolation normalization work",
                "creo interpolation elimination work",
            ],
            |ctx| {
                super::solve_vector_system(
                    ctx,
                    vec![vec![1.0, 0.0], vec![1.0, 1.0]],
                    vec![[1.0; 3], [2.0; 3]],
                )
            },
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| super::solve_vector_system(
                ctx,
                vec![vec![1.0, 0.0], vec![1.0, 1.0]],
                vec![[1.0; 3], [2.0; 3]]
            ))
            .expect("service solver"),
            Some(vec![[1.0; 3]; 2])
        );
    }

    #[test]
    fn interpolation_curve_knots_refuse_collection_limit() {
        let run = |limit| {
            with_collection_limit(limit, |ctx| {
                let points = [[0.0; 3], [1.0; 3]];
                let parameters = [0.0, 1.0];
                let derivatives = [[0.0; 3]; 2];
                let Some(knots) = super::interpolation_knots(ctx, &parameters)? else {
                    return Ok(None);
                };
                super::interpolation_controls(ctx, &points, &parameters, &knots, derivatives)
            })
        };
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo interpolation curve knots"),
            run,
        );
        let error = run(limit).expect_err("named collection boundary");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo interpolation curve knots"
        ));
    }

    #[test]
    fn interpolation_position_controls_refuse_collection_limit() {
        assert!(matches!(
            interpolation_surface_refusal("creo interpolation surface position controls"),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo interpolation surface position controls"
        ));
    }

    #[test]
    fn interpolation_position_row_refuses_collection_limit() {
        assert!(matches!(
            interpolation_surface_refusal("creo interpolation surface position row"),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo interpolation surface position row"
        ));
    }

    #[test]
    fn interpolation_pole_rows_refuse_at_collection_limit() {
        let grid = interpolation_grid();
        crate::decode::with_test_decode_ctx(|ctx| {
            assert!(super::interpolation_spline_surface(
                ctx,
                &grid,
                &"interpolation grid fixture",
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
            .expect("service profile admits interpolation grid")
            .is_some());
        });
        assert!(matches!(
            interpolation_surface_refusal("creo interpolation surface NURBS pole rows"),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo interpolation surface NURBS pole rows"
        ));
    }

    #[test]
    fn interpolation_pole_values_refuse_at_collection_limit() {
        assert!(matches!(
            interpolation_surface_refusal("creo interpolation surface NURBS pole values"),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo interpolation surface NURBS pole values"
        ));
    }

    #[test]
    fn reversed_sketch_nurbs_keeps_finite_knots_when_endpoint_sum_overflows() {
        let lower = 9.0e307;
        let upper = f64::MAX;
        let geometry = SketchGeometry::nurbs(
            PcurveNurbs::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![lower, lower, upper, upper],
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                None,
                false,
            )
            .expect("fixture pcurve construction admission")
            .expect("wide finite sketch NURBS"),
        );
        let reversed = crate::decode::with_test_decode_ctx(|ctx| {
            oriented_sketch_nurbs_curve(ctx, &geometry, true)
        })
        .expect("service resources")
        .expect("finite reversed sketch NURBS");
        assert_eq!(reversed.knots().as_slice(), [lower, lower, upper, upper]);
        assert_eq!(reversed.control_points()[0], Point3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn signed_unit_chart_accepts_bounded_endpoint_rounding_only() {
        assert_eq!(
            signed_unit_chart([1.0, 4.0], [1.0, 4.000_03], 0.0),
            Some((1.0, 0.0))
        );
        assert!(signed_unit_chart([1.0, 4.0], [1.0, 4.001], 0.0).is_none());
    }

    #[test]
    fn translating_nurbs_rejects_nonfinite_poles() {
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(f64::MAX, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("finite NURBS fixture");

        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            translated_nurbs_curve(ctx, &curve, [f64::MAX, 0.0, 0.0])
        })
        .expect("translation resources")
        .is_none());
        assert_eq!(curve.control_points()[0], Point3::new(f64::MAX, 0.0, 0.0));
    }

    #[test]
    fn placed_section_nurbs_refuses_knot_and_pole_limits() {
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(1.0, 2.0, 0.0), Point3::new(3.0, 4.0, 0.0)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("finite curve");
        let transform = crate::placement::FeatureSectionTransform::new(
            1,
            Some(1),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0,
        )
        .expect("section frame");
        crate::test_support::assert_refusal_order(
            ResourceDimension::CollectionItems,
            &[
                "creo placed section NURBS curve",
                "creo placed section NURBS curve",
            ],
            |limit| {
                with_collection_limit(limit, |ctx| placed_section_nurbs(ctx, &transform, &curve))
            },
        );
        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            placed_section_nurbs(ctx, &transform, &curve)
        })
        .expect("service resources")
        .is_some());
    }

    #[test]
    fn translated_nurbs_refuses_knot_and_pole_limits() {
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(1.0, 2.0, 0.0), Point3::new(3.0, 4.0, 0.0)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("finite curve");
        crate::test_support::assert_refusal_order(
            ResourceDimension::CollectionItems,
            &["creo translated NURBS curve", "creo translated NURBS curve"],
            |limit| {
                with_collection_limit(limit, |ctx| {
                    translated_nurbs_curve(ctx, &curve, [0.0, 0.0, 2.0])
                })
            },
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| {
                translated_nurbs_curve(ctx, &curve, [0.0, 0.0, 2.0])
            })
            .expect("service resources")
            .expect("finite translation")
            .control_points(),
            vec![Point3::new(1.0, 2.0, 2.0), Point3::new(3.0, 4.0, 2.0)]
        );
    }

    fn extruded_nurbs_refusal_at_limit(
        limit: u64,
    ) -> Result<Option<cadmpeg_ir::geometry::nurbs::NurbsSurface>, cadmpeg_core::CodecError> {
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(1.0, 2.0, 0.0), Point3::new(3.0, 4.0, 0.0)],
            Some(vec![1.0, 0.5]),
            false,
        )
        .expect("fixture constructor admission")
        .expect("rational directrix");
        with_collection_limit(limit, |ctx| {
            extruded_nurbs_surface(
                ctx,
                &curve,
                [0.0, 0.0, 2.0],
                &"rational directrix",
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
    }

    #[test]
    fn extruded_nurbs_pole_rows_refuse_collection_limit() {
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo extruded NURBS pole rows"),
            extruded_nurbs_refusal_at_limit,
        );
        let result =
            extruded_nurbs_refusal_at_limit(limit).expect_err("named surface collection boundary");
        assert!(matches!(result,
            cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo extruded NURBS pole rows"));
    }

    #[test]
    fn extruded_nurbs_pole_values_refuse_each_row_limit() {
        crate::test_support::assert_refusal_order(
            ResourceDimension::CollectionItems,
            &[
                "creo extruded NURBS pole values",
                "creo extruded NURBS pole values",
            ],
            extruded_nurbs_refusal_at_limit,
        );
    }

    #[test]
    fn extruded_nurbs_u_knots_refuse_collection_limit() {
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo extruded NURBS U knots"),
            extruded_nurbs_refusal_at_limit,
        );
        let result =
            extruded_nurbs_refusal_at_limit(limit).expect_err("named surface collection boundary");
        assert!(matches!(result,
            cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo extruded NURBS U knots"));
    }
    #[test]
    fn extruded_nurbs_v_knots_refuse_collection_limit() {
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo extruded NURBS V knots"),
            extruded_nurbs_refusal_at_limit,
        );
        let result =
            extruded_nurbs_refusal_at_limit(limit).expect_err("named surface collection boundary");
        assert!(matches!(result,
            cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo extruded NURBS V knots"));
    }

    fn planar_or_offset_spline(z: f64) -> crate::feature::definitions::FeatureSavedSpline {
        crate::feature::definitions::FeatureSavedSpline {
            entity_id: Some(11),
            declared_point_count: Some(2),
            interpolation_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, z]],
            interpolation_points_body: Vec::new(),
            endpoint_tangents: Some(crate::feature::definitions::DecodedField {
                value: [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                body: Vec::new(),
            }),
            parameters: Some(crate::feature::definitions::DecodedField {
                value: vec![0.0, 1.0],
                body: Vec::new(),
            }),
            offset: 64,
        }
    }

    #[test]
    fn a_planar_saved_spline_is_sketch_geometry_and_states_no_refusal() {
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| super::saved_spline_sketch_geometry(
                ctx,
                &planar_or_offset_spline(0.0),
                &mut refusal
            ))
            .expect("test spline allocation")
            .is_some()
        );
        assert!(refusal.take_records().is_empty());
    }

    #[test]
    fn saved_spline_sketch_knots_refuse_at_collection_limit() {
        let spline = planar_or_offset_spline(0.0);
        let run = |limit| {
            with_collection_limit(limit, |ctx| {
                super::saved_spline_sketch_geometry(
                    ctx,
                    &spline,
                    &mut crate::lane_refusal::LaneRefusals::new(),
                )
            })
        };
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo saved spline sketch knots"),
            run,
        );
        let error = run(limit).expect_err("named collection boundary");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo saved spline sketch knots"
        ));
    }

    #[test]
    fn saved_spline_sketch_controls_refuse_at_collection_limit() {
        let spline = planar_or_offset_spline(0.0);
        let run = |limit| {
            with_collection_limit(limit, |ctx| {
                super::saved_spline_sketch_geometry(
                    ctx,
                    &spline,
                    &mut crate::lane_refusal::LaneRefusals::new(),
                )
            })
        };
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo saved spline sketch controls"),
            run,
        );
        let error = run(limit).expect_err("named collection boundary");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo saved spline sketch controls"
        ));
    }

    #[test]
    fn a_non_planar_saved_spline_states_the_interpolation_point_that_left_the_sketch_plane() {
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| super::saved_spline_sketch_geometry(
                ctx,
                &planar_or_offset_spline(2.0),
                &mut refusal
            ))
            .expect("test spline allocation")
            .is_none()
        );
        let records = refusal.take_records();
        assert_eq!(records.len(), 1);
        // The fixture states `z = 2` at interpolation point 1, and the note
        // names that point and that number: both are bytes of the record.
        assert_eq!(
            records[0],
            "creo saved-spline entity 11 at offset 64 sketch geometry record: interpolation \
             point 1 is not on the sketch plane: z states 2"
        );
    }

    #[test]
    fn non_planar_saved_spline_refusal_text_obeys_retained_byte_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            ResourceDimension::RetainedBytes,
            Some("creo lane refusal text"),
            |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                    .expect("test decode context");
                let mut refusal = crate::lane_refusal::LaneRefusals::new();
                assert!(super::saved_spline_sketch_geometry(
                    &ctx,
                    &planar_or_offset_spline(2.0),
                    &mut refusal
                )
                .expect("candidate route")
                .is_none());
                refusal.take_records_checked()
            },
        );
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        assert!(super::saved_spline_sketch_geometry(
            &ctx,
            &planar_or_offset_spline(2.0),
            &mut refusal
        )
        .expect("candidate route")
        .is_none());
        let error = refusal
            .take_records_checked()
            .expect_err("refusal text exceeds limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "creo lane refusal text"
        ));
    }

    #[test]
    fn a_non_planar_saved_spline_tangent_states_the_tangent_that_left_the_sketch_plane() {
        let mut spline = planar_or_offset_spline(0.0);
        spline
            .endpoint_tangents
            .as_mut()
            .expect("endpoint tangents")
            .value[1] = [1.0, 0.0, 3.0];

        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| super::saved_spline_sketch_geometry(
                ctx,
                &spline,
                &mut refusal
            ))
            .expect("test spline allocation")
            .is_none()
        );
        let records = refusal.take_records();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0],
            "creo saved-spline entity 11 at offset 64 sketch geometry record: endpoint tangent \
             end is not on the sketch plane: z states 3"
        );
    }

    #[test]
    fn saved_spline_records_name_the_entity_and_its_offset() {
        let mut spline = crate::feature::definitions::FeatureSavedSpline {
            entity_id: Some(7),
            declared_point_count: Some(2),
            interpolation_points: Vec::new(),
            interpolation_points_body: Vec::new(),
            endpoint_tangents: None,
            parameters: None,
            offset: 2048,
        };
        assert_eq!(
            super::saved_spline_record(&spline).to_string(),
            "creo saved-spline entity 7 at offset 2048"
        );
        spline.entity_id = None;
        assert_eq!(
            super::saved_spline_record(&spline).to_string(),
            "creo saved-spline entity at offset 2048"
        );
    }

    #[test]
    fn malformed_basis_knots_are_rejected() {
        for knots in [&[][..], &[0.0][..]] {
            assert_eq!(super::bspline_basis(0, 3, 0.5, knots, 4), None);
            assert_eq!(super::bspline_basis_derivative(0, 3, 0.5, knots, 4), None);
        }
    }

    #[test]
    fn interpolation_basis_rejects_degree_above_cubic() {
        let knots = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        assert_eq!(super::bspline_basis(0, 4, 0.5, &knots, 5), None);
        assert_eq!(super::bspline_basis_derivative(0, 4, 0.5, &knots, 5), None);
    }

    #[test]
    fn two_refused_extruded_carriers_state_two_records_each_naming_its_carrier() {
        let directrix = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                Point3::new(f64::MAX, 0.0, 0.0),
                Point3::new(f64::MAX, 1.0, 0.0),
            ],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid directrix");
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let (first, second) = with_collection_limit(u64::MAX, |ctx| {
            let first = extruded_nurbs_surface(
                ctx,
                &directrix,
                [f64::MAX, 0.0, 0.0],
                &"surface 11 at offset 64",
                &mut refusal,
            )
            .expect("first carrier resources");
            let second = extruded_nurbs_surface(
                ctx,
                &directrix,
                [f64::MAX, 0.0, 0.0],
                &"surface 12 at offset 128",
                &mut refusal,
            )
            .expect("second carrier resources");
            (first, second)
        });
        assert!(first.is_none(), "the refused ruling states no surface");
        assert!(second.is_none(), "the refused ruling states no surface");
        let records = refusal.take_records();
        assert_eq!(
            records.len(),
            2,
            "one record per refused ruling: {records:?}"
        );
        assert!(
            records[0]
                .starts_with("creo extruded NURBS surface record for surface 11 at offset 64: "),
            "the record names the instance that stated the lanes: {}",
            records[0]
        );
        assert!(
            records[1]
                .starts_with("creo extruded NURBS surface record for surface 12 at offset 128: "),
            "the record names the instance that stated the lanes: {}",
            records[1]
        );
        assert!(
            refusal.take_records().is_empty(),
            "the sink is empty once its records are taken"
        );
    }

    mod admission_visits;
}
