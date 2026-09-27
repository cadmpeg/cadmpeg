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

const EPS_TABULATED_ENDPOINT_ROUNDING: f64 = 1e-4;
const EPS_TABULATED_FRAME_EXACT: f64 = 1.0e-9;
const EPS_PLANAR_COORDINATE: f64 = 1.0e-12;

/// Names one saved-section spline entity by its stored entity identifier and
/// the byte offset of its entity label. A saved section states the entity
/// identifier only for entities the solver kept, so the byte offset is the
/// identity for the rest.
fn saved_spline_record(spline: &crate::feature::definitions::FeatureSavedSpline) -> String {
    match spline.entity_id {
        Some(entity_id) => format!(
            "creo saved-spline entity {entity_id} at offset {}",
            spline.offset
        ),
        None => format!("creo saved-spline entity at offset {}", spline.offset),
    }
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
    let end = index.checked_add(degree)?.checked_add(1)?;
    let window = knots.get(index..=end)?;
    let first = *window.first()?;
    let second = *window.get(1)?;
    let last = *window.last()?;
    if parameter == *knots.last()? {
        return Some(if index.checked_add(1)? == count {
            1.0
        } else {
            0.0
        });
    }
    if degree == 0 {
        return Some(if first <= parameter && parameter < second {
            1.0
        } else {
            0.0
        });
    }
    let left_denominator = window[degree] - first;
    let right_denominator = last - second;
    let left = if left_denominator > 0.0 {
        (parameter - first) / left_denominator
            * bspline_basis(index, degree - 1, parameter, knots, count)?
    } else {
        0.0
    };
    let right = if right_denominator > 0.0 {
        (last - parameter) / right_denominator
            * bspline_basis(index + 1, degree - 1, parameter, knots, count)?
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
    let end = index.checked_add(degree)?.checked_add(1)?;
    let window = knots.get(index..=end)?;
    if degree == 0 {
        return Some(0.0);
    }
    let left_denominator = window[degree] - window[0];
    let right_denominator = window[degree + 1] - window[1];
    let left = if left_denominator > 0.0 {
        degree as f64 / left_denominator
            * bspline_basis(index, degree - 1, parameter, knots, count)?
    } else {
        0.0
    };
    let right = if right_denominator > 0.0 {
        degree as f64 / right_denominator
            * bspline_basis(index + 1, degree - 1, parameter, knots, count)?
    } else {
        0.0
    };
    Some(left - right)
}

fn solve_vector_system(
    mut matrix: Vec<Vec<f64>>,
    mut values: Vec<[f64; 3]>,
) -> Option<Vec<[f64; 3]>> {
    let count = matrix.len();
    (values.len() == count && matrix.iter().all(|row| row.len() == count)).then_some(())?;
    for column in 0..count {
        let pivot = (column..count).max_by(|left, right| {
            matrix[*left][column]
                .abs()
                .total_cmp(&matrix[*right][column].abs())
        })?;
        (matrix[pivot][column].abs() > 1e-14).then_some(())?;
        matrix.swap(column, pivot);
        values.swap(column, pivot);
        let scale = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= scale;
        }
        values[column] = values[column].map(|value| value / scale);
        let pivot_value = values[column];
        let (before, pivot_and_after) = matrix.split_at_mut(column);
        let (pivot_row, after) = pivot_and_after.split_first_mut()?;
        let (before_values, pivot_and_after_values) = values.split_at_mut(column);
        let (_, after_values) = pivot_and_after_values.split_first_mut()?;
        for (row, values) in before
            .iter_mut()
            .chain(after.iter_mut())
            .zip(before_values.iter_mut().chain(after_values.iter_mut()))
        {
            let factor = row[column];
            if factor == 0.0 {
                continue;
            }
            for (entry, pivot_entry) in row[column..].iter_mut().zip(&pivot_row[column..]) {
                *entry -= factor * pivot_entry;
            }
            for (value, pivot) in values.iter_mut().zip(pivot_value) {
                *value -= factor * pivot;
            }
        }
    }
    Some(values)
}

#[derive(Debug)]
struct InterpolationCurveData {
    knots: Vec<f64>,
    controls: Vec<[f64; 3]>,
}

fn interpolation_curve_data(
    ctx: &DecodeContext<'_>,
    points: &[[f64; 3]],
    parameters: &[f64],
    endpoint_derivatives: [[f64; 3]; 2],
) -> Result<Option<InterpolationCurveData>, CodecError> {
    const DEGREE: usize = 3;
    let point_count = points.len();
    if point_count < 2
        || parameters.len() != point_count
        || !parameters
            .windows(2)
            .all(|pair| pair[0].is_finite() && pair[0] < pair[1])
        || !parameters.last().is_some_and(|value| value.is_finite())
    {
        return Ok(None);
    }
    let Some(control_count) = point_count.checked_add(2) else {
        return Ok(None);
    };
    let mut knots =
        ctx.alloc_filled(DEGREE + 1, parameters[0], "creo interpolation curve knots")?;
    ctx.try_reserve_items(
        &mut knots,
        point_count - 2 + DEGREE + 1,
        "creo interpolation curve knot tail",
    )?;
    knots.extend_from_slice(&parameters[1..point_count - 1]);
    knots.extend(std::iter::repeat_n(parameters[point_count - 1], DEGREE + 1));
    let mut matrix = Vec::new();
    ctx.try_reserve_items(&mut matrix, control_count, "creo interpolation matrix rows")?;
    for parameter in parameters {
        let mut row = ctx.alloc_filled(control_count, 0.0, "creo interpolation matrix values")?;
        for (index, value) in row.iter_mut().enumerate() {
            let Some(basis) = bspline_basis(index, DEGREE, *parameter, &knots, control_count)
            else {
                return Ok(None);
            };
            *value = basis;
        }
        matrix.push(row);
    }
    for parameter in [parameters[0], parameters[point_count - 1]] {
        let mut row = ctx.alloc_filled(control_count, 0.0, "creo interpolation matrix values")?;
        for (index, value) in row.iter_mut().enumerate() {
            let Some(basis) =
                bspline_basis_derivative(index, DEGREE, parameter, &knots, control_count)
            else {
                return Ok(None);
            };
            *value = basis;
        }
        matrix.push(row);
    }
    let mut values = Vec::new();
    ctx.try_reserve_items(
        &mut values,
        control_count,
        "creo interpolation input values",
    )?;
    values.extend_from_slice(points);
    values.extend(endpoint_derivatives);
    Ok(solve_vector_system(matrix, values)
        .map(|controls| InterpolationCurveData { knots, controls }))
}

pub(in super::super) fn saved_spline_nurbs(
    ctx: &DecodeContext<'_>,
    spline: &crate::feature::definitions::FeatureSavedSpline,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<NurbsCurve>, CodecError> {
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
    let Some(InterpolationCurveData {
        knots,
        controls: control_points,
    }) = interpolation_curve_data(ctx, &spline.interpolation_points, parameters, tangents)?
    else {
        return Ok(None);
    };
    let mut converted_controls = Vec::new();
    ctx.try_reserve_items(
        &mut converted_controls,
        control_points.len(),
        "creo saved spline controls",
    )?;
    converted_controls.extend(control_points.into_iter().map(Point3::from));
    match NurbsCurve::from_lanes(3, knots, converted_controls, None, false) {
        Ok(curve) => Ok(Some(curve)),
        Err(error) => {
            refusal.note(
                format!("{} NURBS record", saved_spline_record(spline)),
                &error,
            );
            Ok(None)
        }
    }
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
    spline: &crate::feature::definitions::FeatureSavedSpline,
) -> Option<(String, f64)> {
    let point = spline
        .interpolation_points
        .iter()
        .enumerate()
        .find(|(_, point)| point[2].abs() > EPS_PLANAR_COORDINATE)
        .map(|(index, point)| (format!("interpolation point {index}"), point[2]));
    let tangent = spline.endpoint_tangents.as_ref().and_then(|tangents| {
        ["start", "end"]
            .into_iter()
            .zip(tangents.value)
            .find(|(_, tangent)| tangent[2].abs() > EPS_PLANAR_COORDINATE)
            .map(|(end, tangent)| (format!("endpoint tangent {end}"), tangent[2]))
    });
    point.or(tangent)
}

pub(in super::super) fn saved_spline_sketch_geometry(
    ctx: &DecodeContext<'_>,
    spline: &crate::feature::definitions::FeatureSavedSpline,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<SketchGeometry>, CodecError> {
    if let Some((subject, z)) = saved_spline_off_plane_input(spline) {
        refusal.note(
            format!("{} sketch geometry record", saved_spline_record(spline)),
            &format_args!("{subject} is not on the sketch plane: z states {z}"),
        );
        return Ok(None);
    }
    let Some(nurbs) = saved_spline_nurbs(ctx, spline, refusal)? else {
        return Ok(None);
    };
    let knots = ctx.try_collection(
        nurbs.knots().len(),
        "creo saved spline sketch knots",
        || nurbs.knots().try_clone(),
    )?;
    let mut controls = Vec::new();
    ctx.try_reserve_items(
        &mut controls,
        nurbs.pole_count(),
        "creo saved spline sketch controls",
    )?;
    let weights = match nurbs.pole_rows() {
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
            for point in points {
                let [x, y, _] = point.coordinates();
                controls.push(cadmpeg_ir::units::FinitePoint2::from_coordinates(x, y));
            }
            None
        }
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
            let mut weights = Vec::new();
            ctx.try_reserve_items(
                &mut weights,
                points.len(),
                "creo saved spline sketch weights",
            )?;
            for pole in points {
                let [x, y, _] = pole.point.coordinates();
                controls.push(cadmpeg_ir::units::FinitePoint2::from_coordinates(x, y));
                weights.push(pole.weight);
            }
            Some(weights)
        }
    };
    match cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_checked_lanes(
        nurbs.degree(),
        knots,
        controls,
        weights,
        nurbs.periodic(),
    ) {
        Ok(pcurve) => Ok(Some(SketchGeometry::nurbs(pcurve))),
        Err(error) => {
            refusal.note(
                format!("{} sketch geometry record", saved_spline_record(spline)),
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
    let mut position_controls = ctx.alloc_filled(
        u_control_count,
        Vec::<[f64; 3]>::new(),
        "creo interpolation surface position controls",
    )?;
    for row in &mut position_controls {
        *row = ctx.alloc_filled(
            v_sample_count,
            [0.0; 3],
            "creo interpolation surface position row",
        )?;
    }
    let mut u_knots = None;
    for v in 0..v_sample_count {
        let mut samples = Vec::new();
        ctx.try_reserve_items(
            &mut samples,
            u_sample_count,
            "creo interpolation surface position samples",
        )?;
        samples.extend((0..u_sample_count).map(|u| points[u * v_sample_count + v]));
        let Some(InterpolationCurveData { knots, controls }) = interpolation_curve_data(
            ctx,
            &samples,
            u_parameters,
            [end_u_derivatives[v], end_u_derivatives[v_sample_count + v]],
        )?
        else {
            return Ok(None);
        };
        u_knots.get_or_insert(knots);
        for (u, control) in controls.into_iter().enumerate() {
            position_controls[u][v] = control;
        }
    }

    let mut v_derivative_controls = ctx.alloc_filled(
        2,
        Vec::<[f64; 3]>::new(),
        "creo interpolation surface derivative controls",
    )?;
    for row in &mut v_derivative_controls {
        *row = ctx.alloc_filled(
            u_control_count,
            [0.0; 3],
            "creo interpolation surface derivative row",
        )?;
    }
    for v_boundary in 0..2 {
        let mut samples = Vec::new();
        ctx.try_reserve_items(
            &mut samples,
            u_sample_count,
            "creo interpolation surface derivative samples",
        )?;
        samples.extend(
            (0..u_sample_count).map(|u| end_v_derivatives[v_boundary * u_sample_count + u]),
        );
        let Some(InterpolationCurveData { controls, .. }) = interpolation_curve_data(
            ctx,
            &samples,
            u_parameters,
            [
                corner_mixed_derivatives[v_boundary * 2],
                corner_mixed_derivatives[v_boundary * 2 + 1],
            ],
        )?
        else {
            return Ok(None);
        };
        v_derivative_controls[v_boundary] = controls;
    }

    let Some(control_count) = u_control_count.checked_mul(v_control_count) else {
        return Ok(None);
    };
    let mut control_points = Vec::new();
    ctx.try_reserve_items(
        &mut control_points,
        control_count,
        "creo interpolation surface controls",
    )?;
    let mut v_knots = None;
    for u in 0..u_control_count {
        let Some(InterpolationCurveData { knots, controls }) = interpolation_curve_data(
            ctx,
            &position_controls[u],
            v_parameters,
            [v_derivative_controls[0][u], v_derivative_controls[1][u]],
        )?
        else {
            return Ok(None);
        };
        v_knots.get_or_insert(knots);
        control_points.extend(controls.into_iter().map(Point3::from));
    }

    let (Some(u_knots), Some(v_knots)) = (u_knots, v_knots) else {
        return Ok(None);
    };
    if u32::try_from(v_control_count).is_err() {
        return Ok(None);
    }
    let mut pole_rows = Vec::new();
    ctx.try_reserve_items(
        &mut pole_rows,
        u_control_count,
        "creo interpolation surface NURBS pole rows",
    )?;
    for points in control_points.chunks(v_control_count) {
        let mut row = Vec::new();
        ctx.try_reserve_items(
            &mut row,
            points.len(),
            "creo interpolation surface NURBS pole values",
        )?;
        row.extend_from_slice(points);
        pole_rows.push(row);
    }
    match NurbsSurface::from_lanes(
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(3, u_knots, false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(3, v_knots, false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(pole_rows, None),
        false,
    ) {
        Ok(surface) => Ok(Some(surface)),
        Err(error) => {
            refusal.note(
                format!("creo interpolation-spline surface record for {record}"),
                &error,
            );
            Ok(None)
        }
    }
}

pub(in super::super) fn placed_section_nurbs(
    transform: &crate::placement::FeatureSectionTransform,
    nurbs: &NurbsCurve,
) -> Option<NurbsCurve> {
    let mut placed = nurbs.clone();
    placed
        .edit_control_points(|point| {
            let model = section_xyz_in_model(transform, [point.x, point.y, point.z]);
            *point = Point3::from(model);
            Ok(())
        })
        .ok()?;
    Some(placed)
}

pub(super) fn translated_nurbs_curve(
    curve: &NurbsCurve,
    translation: [f64; 3],
) -> Option<NurbsCurve> {
    let mut translated = curve.clone();
    translated
        .edit_control_points(|point| {
            *point = Point3::new(
                point.x + translation[0],
                point.y + translation[1],
                point.z + translation[2],
            );
            Ok(())
        })
        .ok()?;
    Some(translated)
}

pub(in super::super) fn extruded_nurbs_surface(
    directrix: &NurbsCurve,
    sweep: [f64; 3],
    record: &dyn std::fmt::Display,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<NurbsSurface> {
    let directrix_points = directrix.pole_rows().raw_points();
    let directrix_weights = directrix.weights();
    let mut control_points = Vec::with_capacity(directrix_points.len() * 2);
    let mut weights = directrix_weights
        .as_ref()
        .map(|_| Vec::with_capacity(control_points.capacity()));
    for (index, point) in directrix_points.iter().enumerate() {
        control_points.push(*point);
        control_points.push(Point3::new(
            point.x + sweep[0],
            point.y + sweep[1],
            point.z + sweep[2],
        ));
        if let (Some(source), Some(target)) = (&directrix_weights, &mut weights) {
            target.extend([source[index], source[index]]);
        }
    }
    match cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::from_checked_lanes(
        control_points.chunks(2_usize).map(<[_]>::to_vec).collect(),
        weights.map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
    )
    .and_then(|poles| {
        NurbsSurface::new(
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                directrix.degree(),
                directrix.knots().clone(),
                directrix.periodic(),
            ),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            poles,
            false,
        )
    }) {
        Ok(surface) => Some(surface),
        Err(error) => {
            refusal.note(
                format!("creo extruded NURBS surface record for {record}"),
                &error,
            );
            None
        }
    }
}

pub(super) fn sketch_nurbs_curve(geometry: &SketchGeometry) -> Option<NurbsCurve> {
    let SketchGeometryDefinition::Nurbs { curve } = geometry.definition() else {
        return None;
    };
    let nurbs = curve
        .lift(|point| Point3::new(point.u, point.v, 0.0))
        .ok()?;
    valid_positive_nurbs_curve(&nurbs).map(|()| nurbs)
}

pub(super) fn oriented_sketch_nurbs_curve(
    geometry: &SketchGeometry,
    reversed: bool,
) -> Option<NurbsCurve> {
    let nurbs = sketch_nurbs_curve(geometry)?;
    if !reversed {
        return Some(nurbs);
    }
    let [lower, upper] =
        cadmpeg_ir::scalar::FiniteReal::raw_array(nurbs_intrinsic_parameter_range(&nurbs)?);
    let mut reversed = nurbs;
    let knots = reversed
        .knots()
        .iter()
        .rev()
        .map(|knot| lower + upper - knot)
        .collect::<Vec<_>>();
    let knots = if knots.iter().all(|knot| knot.is_finite()) {
        knots
    } else {
        let interval = cadmpeg_ir::topology::IncreasingParameterInterval::new([lower, upper])?;
        reversed
            .knots()
            .iter()
            .rev()
            .map(|knot| {
                interval
                    .map_from(interval, cadmpeg_ir::scalar::FiniteReal::new(*knot)?, true)
                    .ok()
                    .map(cadmpeg_ir::scalar::FiniteReal::get)
            })
            .collect::<Option<Vec<_>>>()?
    };
    reversed.reverse_parameterization();
    reversed
        .edit_knots(|target| target.copy_from_slice(&knots))
        .ok()?;
    Some(reversed)
}

pub(super) fn sketch_nurbs_pcurve(
    geometry: &SketchGeometry,
    reversed: bool,
    record: &dyn std::fmt::Display,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<PcurveGeometry> {
    let nurbs = oriented_sketch_nurbs_curve(geometry, reversed)?;
    match cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_checked_lanes(
        nurbs.degree(),
        nurbs.knots().clone(),
        nurbs
            .control_points()
            .iter()
            .map(|point| {
                let [x, y, _] = point.coordinates();
                cadmpeg_ir::units::FinitePoint2::from_coordinates(x, y)
            })
            .collect(),
        nurbs.weights(),
        nurbs.periodic(),
    ) {
        Ok(nurbs) => Some(PcurveGeometry::Nurbs { nurbs }),
        Err(error) => {
            refusal.note(
                format!("creo sketch NURBS pcurve record for {record}"),
                &error,
            );
            None
        }
    }
}

pub(in super::super) fn extrusion_brep_side_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
    reversed: bool,
    start: [f64; 2],
    end: [f64; 2],
    span: ExtrusionSpan,
    diagnostics: &mut crate::lane_refusal::LaneRefusalContext<'_, '_>,
) -> Option<SurfaceGeometry> {
    if matches!(
        geometry.definition(),
        SketchGeometryDefinition::Nurbs { .. }
    ) {
        let directrix = oriented_sketch_nurbs_curve(geometry, reversed)?;
        let lower_translation = transform.normal().map(|value| value * span.lower());
        let sweep = transform
            .normal()
            .map(|value| value * (span.upper() - span.lower()));
        let placed = placed_section_nurbs(transform, &directrix)?;
        let translated = translated_nurbs_curve(&placed, lower_translation)?;
        return Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            extruded_nurbs_surface(&translated, sweep, diagnostics.record, diagnostics.refusals)?,
        )));
    }
    let section_geometry = match geometry.definition() {
        SketchGeometryDefinition::Line { .. } => {
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start[0], start[1]),
                end: Point2::new(end[0], end[1]),
            })
            .ok()?
        }
        _ => geometry.clone(),
    };
    extruded_geometry_surface(transform, &section_geometry)
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
    let mut matches = Vec::new();
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
                    && !matches.contains(&(slope, chart_intercept))
                {
                    matches.push((slope, chart_intercept));
                }
            }
        }
    }
    let [mapping] = matches.as_slice() else {
        return None;
    };
    Some(*mapping)
}

pub(in super::super) fn placed_tabulated_cylinder_directrix(
    replay: &crate::surface::TabulatedCylinderCurveReplay,
    parameters: &crate::surface::SurfaceParameterRecord,
    chart_origin: Option<[f64; 3]>,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<(NurbsCurve, [f64; 3])> {
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
    let points = replay
        .control_points
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()?;
    let (values, layout) = parameters
        .tabulated_cylinder_frame()
        .map(|frame| {
            let values = frame.values().to_vec();
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
            let values = frame
                .slots
                .iter()
                .map(|slot| slot.value)
                .collect::<Option<Vec<_>>>()?;
            Some((values, FrameLayout::LegacyReflected))
        })?;
    let [a0, a1, a2, b0, b1, b2] = values.as_slice() else {
        return None;
    };
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
    let assignments = (0..3)
        .flat_map(|first_axis| {
            (0..3)
                .filter(move |&second_axis| {
                    first_axis != second_axis
                        && axis_matches(first_axis, 0)
                        && axis_matches(second_axis, 1)
                })
                .map(move |second_axis| (first_axis, second_axis, 3 - first_axis - second_axis))
        })
        .collect::<Vec<_>>();
    let [(first_axis, second_axis, sweep_axis)] = assignments.as_slice() else {
        return None;
    };
    let (signed_chart, reflect_sweep) = match layout {
        FrameLayout::LegacyReflected => (None, false),
        FrameLayout::PrototypeOffsetPlanar => (
            Some((
                signed_unit_chart(
                    [local_start[0], local_end[0]],
                    [first[*first_axis], second[*first_axis]],
                    chart_origin?[*first_axis].abs(),
                )?,
                signed_unit_chart(
                    [local_start[1], local_end[1]],
                    [first[*second_axis], second[*second_axis]],
                    0.0,
                )?,
            )),
            false,
        ),
        FrameLayout::ZeroOffsetPlanar => (
            Some((
                signed_unit_chart(
                    [local_start[0], local_end[0]],
                    [first[*first_axis], second[*first_axis]],
                    0.0,
                )?,
                signed_unit_chart(
                    [local_start[1], local_end[1]],
                    [first[*second_axis], second[*second_axis]],
                    0.0,
                )?,
            )),
            false,
        ),
        FrameLayout::SelectedPlanar => {
            let mut first_intercepts = vec![(0.0, false)];
            if let Some(origin) = chart_origin {
                let intercept = origin[*first_axis].abs();
                if intercept.is_finite() && !close(intercept, 0.0) {
                    first_intercepts.push((intercept, true));
                }
            }
            let candidates = first_intercepts
                .into_iter()
                .filter_map(|(first_offset, reflect_sweep)| {
                    Some((
                        (
                            signed_unit_chart(
                                [local_start[0], local_end[0]],
                                [first[*first_axis], second[*first_axis]],
                                first_offset,
                            )?,
                            signed_unit_chart(
                                [local_start[1], local_end[1]],
                                [first[*second_axis], second[*second_axis]],
                                0.0,
                            )?,
                        ),
                        reflect_sweep,
                    ))
                })
                .collect::<Vec<_>>();
            let [(chart, reflect_sweep)] = candidates.as_slice() else {
                return None;
            };
            (Some(*chart), *reflect_sweep)
        }
    };
    let control_points = points
        .iter()
        .map(|point| {
            let mut placed = [0.0; 3];
            match signed_chart {
                Some(((first_slope, first_intercept), (second_slope, second_intercept))) => {
                    placed[*first_axis] = first_slope * point[0] + first_intercept;
                    placed[*second_axis] = second_slope * point[1] + second_intercept;
                    placed[*sweep_axis] = if reflect_sweep {
                        -first[*sweep_axis]
                    } else {
                        first[*sweep_axis]
                    };
                }
                None => {
                    let chart_first =
                        first[*first_axis].max(second[*first_axis]) - (point[0] - local_start[0]);
                    let chart_second =
                        first[*second_axis].min(second[*second_axis]) + (point[1] - local_start[1]);
                    placed[*first_axis] = if *first_axis < 2 {
                        -chart_first
                    } else {
                        chart_first
                    };
                    placed[*second_axis] = if *second_axis < 2 {
                        -chart_second
                    } else {
                        chart_second
                    };
                    placed[*sweep_axis] = first[*sweep_axis];
                }
            }
            Point3::from(placed)
        })
        .collect();
    let mut sweep = [0.0; 3];
    sweep[*sweep_axis] = if reflect_sweep {
        first[*sweep_axis] - second[*sweep_axis]
    } else {
        second[*sweep_axis] - first[*sweep_axis]
    };
    if !sweep[*sweep_axis].is_finite() || sweep[*sweep_axis] == 0.0 {
        return None;
    }
    match NurbsCurve::from_lanes(
        3,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        control_points,
        None,
        false,
    ) {
        Ok(curve) => Some((curve, sweep)),
        Err(error) => {
            refusal.note(
                format!(
                    "creo placed tabulated-cylinder directrix record for surface {} at offset {}",
                    parameters.surface_id, parameters.offset
                ),
                &error,
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extruded_nurbs_surface, oriented_sketch_nurbs_curve, signed_unit_chart,
        translated_nurbs_curve,
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

    fn interpolation_grid() -> crate::interpolation_grid::InterpolationGrid {
        crate::interpolation_grid::InterpolationGrid::try_new(
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
        .expect("complete interpolation grid")
    }

    fn interpolation_surface_refusal(limit: u64) -> cadmpeg_core::CodecError {
        let grid = interpolation_grid();
        with_collection_limit(limit, |ctx| {
            super::interpolation_spline_surface(
                ctx,
                &grid,
                &"interpolation grid fixture",
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
        .expect_err("interpolation allocation exceeds the collection limit")
    }

    #[test]
    fn interpolation_curve_knots_refuse_collection_limit() {
        let error = with_collection_limit(3, |ctx| {
            super::interpolation_curve_data(ctx, &[[0.0; 3], [1.0; 3]], &[0.0, 1.0], [[0.0; 3]; 2])
        })
        .expect_err("four curve knots exceed the collection limit");
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
            interpolation_surface_refusal(3),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo interpolation surface position controls"
        ));
    }

    #[test]
    fn interpolation_position_row_refuses_collection_limit() {
        assert!(matches!(
            interpolation_surface_refusal(5),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo interpolation surface position row"
        ));
    }

    #[test]
    fn interpolation_derivative_controls_refuse_collection_limit() {
        assert!(matches!(
            interpolation_surface_refusal(81),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo interpolation surface derivative controls"
        ));
    }

    #[test]
    fn interpolation_derivative_row_refuses_collection_limit() {
        assert!(matches!(
            interpolation_surface_refusal(85),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo interpolation surface derivative row"
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
            interpolation_surface_refusal(305),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo interpolation surface NURBS pole rows"
        ));
    }

    #[test]
    fn interpolation_pole_values_refuse_at_collection_limit() {
        assert!(matches!(
            interpolation_surface_refusal(309),
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
                1,
                vec![lower, lower, upper, upper],
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                None,
                false,
            )
            .expect("wide finite sketch NURBS"),
        );
        let reversed =
            oriented_sketch_nurbs_curve(&geometry, true).expect("finite reversed sketch NURBS");
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
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(f64::MAX, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("finite NURBS fixture");

        assert!(translated_nurbs_curve(&curve, [f64::MAX, 0.0, 0.0]).is_none());
        assert_eq!(curve.control_points()[0], Point3::new(f64::MAX, 0.0, 0.0));
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
        let error = with_collection_limit(43, |ctx| {
            super::saved_spline_sketch_geometry(
                ctx,
                &spline,
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
        .expect_err("thirty-six source items leave fewer than eight knot slots");
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
        let error = with_collection_limit(47, |ctx| {
            super::saved_spline_sketch_geometry(
                ctx,
                &spline,
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
        .expect_err("forty-four admitted items leave fewer than four controls");
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
            super::saved_spline_record(&spline),
            "creo saved-spline entity 7 at offset 2048"
        );
        spline.entity_id = None;
        assert_eq!(
            super::saved_spline_record(&spline),
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
    fn two_refused_extruded_carriers_state_two_records_each_naming_its_carrier() {
        let directrix = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                Point3::new(f64::MAX, 0.0, 0.0),
                Point3::new(f64::MAX, 1.0, 0.0),
            ],
            None,
            false,
        )
        .expect("valid directrix");
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let first = extruded_nurbs_surface(
            &directrix,
            [f64::MAX, 0.0, 0.0],
            &"surface 11 at offset 64",
            &mut refusal,
        );
        let second = extruded_nurbs_surface(
            &directrix,
            [f64::MAX, 0.0, 0.0],
            &"surface 12 at offset 128",
            &mut refusal,
        );
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
}
