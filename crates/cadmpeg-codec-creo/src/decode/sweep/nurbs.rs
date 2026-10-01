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
    ctx: &DecodeContext<'_>,
    index: usize,
    degree: usize,
    parameter: f64,
    knots: &[f64],
    count: usize,
) -> Result<Option<f64>, CodecError> {
    let _depth = ctx.enter_nested("creo interpolation basis depth")?;
    ctx.charge_work(1, "creo interpolation basis work")?;
    let Some(end) = index.checked_add(degree).and_then(|end| end.checked_add(1)) else {
        return Ok(None);
    };
    let Some(window) = knots.get(index..=end) else {
        return Ok(None);
    };
    let (Some(&first), Some(&second), Some(&last), Some(&last_knot)) =
        (window.first(), window.get(1), window.last(), knots.last())
    else {
        return Ok(None);
    };
    if parameter == last_knot {
        return Ok(Some(f64::from(index.checked_add(1) == Some(count))));
    }
    if degree == 0 {
        return Ok(Some(f64::from(first <= parameter && parameter < second)));
    }
    let left_denominator = window[degree] - first;
    let right_denominator = last - second;
    let left = if left_denominator > 0.0 {
        let Some(basis) = bspline_basis(ctx, index, degree - 1, parameter, knots, count)? else {
            return Ok(None);
        };
        (parameter - first) / left_denominator * basis
    } else {
        0.0
    };
    let right = if right_denominator > 0.0 {
        let Some(basis) = bspline_basis(ctx, index + 1, degree - 1, parameter, knots, count)?
        else {
            return Ok(None);
        };
        (last - parameter) / right_denominator * basis
    } else {
        0.0
    };
    Ok(Some(left + right))
}

pub(in super::super) fn bspline_basis_derivative(
    ctx: &DecodeContext<'_>,
    index: usize,
    degree: usize,
    parameter: f64,
    knots: &[f64],
    count: usize,
) -> Result<Option<f64>, CodecError> {
    let _depth = ctx.enter_nested("creo interpolation basis depth")?;
    ctx.charge_work(1, "creo interpolation basis work")?;
    let Some(end) = index.checked_add(degree).and_then(|end| end.checked_add(1)) else {
        return Ok(None);
    };
    let Some(window) = knots.get(index..=end) else {
        return Ok(None);
    };
    if degree == 0 {
        return Ok(Some(0.0));
    }
    let Ok(degree_value) = u32::try_from(degree).map(f64::from) else {
        return Ok(None);
    };
    let left_denominator = window[degree] - window[0];
    let right_denominator = window[degree + 1] - window[1];
    let left = if left_denominator > 0.0 {
        let Some(basis) = bspline_basis(ctx, index, degree - 1, parameter, knots, count)? else {
            return Ok(None);
        };
        degree_value / left_denominator * basis
    } else {
        0.0
    };
    let right = if right_denominator > 0.0 {
        let Some(basis) = bspline_basis(ctx, index + 1, degree - 1, parameter, knots, count)?
        else {
            return Ok(None);
        };
        degree_value / right_denominator * basis
    } else {
        0.0
    };
    Ok(Some(left - right))
}

fn solve_vector_system(
    ctx: &DecodeContext<'_>,
    mut matrix: Vec<Vec<f64>>,
    mut values: Vec<[f64; 3]>,
) -> Result<Option<Vec<[f64; 3]>>, CodecError> {
    const EPS_INTERPOLATION_PIVOT: f64 = 1e-14;
    let count = matrix.len();
    if values.len() != count || matrix.iter().any(|row| row.len() != count) {
        return Ok(None);
    }
    for column in 0..count {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(count - column),
            "creo interpolation pivot work",
        )?;
        let Some(pivot) = (column..count).max_by(|left, right| {
            matrix[*left][column]
                .abs()
                .total_cmp(&matrix[*right][column].abs())
        }) else {
            return Ok(None);
        };
        if matrix[pivot][column].abs() <= EPS_INTERPOLATION_PIVOT || matrix[pivot][column].is_nan()
        {
            return Ok(None);
        }
        matrix.swap(column, pivot);
        values.swap(column, pivot);
        let scale = matrix[column][column];
        for value in &mut matrix[column][column..] {
            ctx.charge_work(1, "creo interpolation normalization work")?;
            *value /= scale;
        }
        ctx.charge_work(3, "creo interpolation normalization work")?;
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
        for (row, values) in before
            .iter_mut()
            .chain(after.iter_mut())
            .zip(before_values.iter_mut().chain(after_values.iter_mut()))
        {
            ctx.charge_work(1, "creo interpolation elimination work")?;
            let factor = row[column];
            if factor == 0.0 {
                continue;
            }
            for (entry, pivot_entry) in row[column..].iter_mut().zip(&pivot_row[column..]) {
                ctx.charge_work(1, "creo interpolation elimination work")?;
                *entry -= factor * pivot_entry;
            }
            for (value, pivot) in values.iter_mut().zip(pivot_value) {
                ctx.charge_work(1, "creo interpolation elimination work")?;
                *value -= factor * pivot;
            }
        }
    }
    Ok(Some(values))
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
    ctx.reserve_vec(
        &mut knots,
        point_count - 2 + DEGREE + 1,
        "creo interpolation curve knot tail",
    )?;
    knots.extend_from_slice(&parameters[1..point_count - 1]);
    knots.extend(std::iter::repeat_n(parameters[point_count - 1], DEGREE + 1));
    let mut matrix = Vec::new();
    ctx.reserve_vec(&mut matrix, control_count, "creo interpolation matrix rows")?;
    for parameter in parameters {
        let mut row = ctx.alloc_filled(control_count, 0.0, "creo interpolation matrix values")?;
        for (index, value) in row.iter_mut().enumerate() {
            let Some(basis) = bspline_basis(ctx, index, DEGREE, *parameter, &knots, control_count)?
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
                bspline_basis_derivative(ctx, index, DEGREE, parameter, &knots, control_count)?
            else {
                return Ok(None);
            };
            *value = basis;
        }
        matrix.push(row);
    }
    let mut values = Vec::new();
    ctx.reserve_vec(
        &mut values,
        control_count,
        "creo interpolation input values",
    )?;
    values.extend_from_slice(points);
    values.extend(endpoint_derivatives);
    Ok(solve_vector_system(ctx, matrix, values)?
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
    ctx.reserve_vec(
        &mut converted_controls,
        control_points.len(),
        "creo saved spline controls",
    )?;
    converted_controls.extend(control_points.into_iter().map(Point3::from));
    match NurbsCurve::from_lanes_for_decode(ctx, 3, knots, converted_controls, None, false)? {
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
) -> Option<(OffPlaneInput, f64)> {
    let point = spline
        .interpolation_points
        .iter()
        .enumerate()
        .find(|(_, point)| point[2].abs() > EPS_PLANAR_COORDINATE)
        .map(|(index, point)| (OffPlaneInput::Point(index), point[2]));
    let tangent = spline.endpoint_tangents.as_ref().and_then(|tangents| {
        ["start", "end"]
            .into_iter()
            .zip(tangents.value)
            .find(|(_, tangent)| tangent[2].abs() > EPS_PLANAR_COORDINATE)
            .map(|(end, tangent)| (OffPlaneInput::Tangent(end), tangent[2]))
    });
    point.or(tangent)
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
    if let Some((subject, z)) = saved_spline_off_plane_input(spline) {
        refusal.note_checked(
            ctx,
            format_args!("{} sketch geometry record", saved_spline_record(spline)),
            &format_args!("{subject} is not on the sketch plane: z states {z}"),
        );
        return Ok(None);
    }
    let Some(nurbs) = saved_spline_nurbs(ctx, spline, refusal)? else {
        return Ok(None);
    };
    let knots = nurbs
        .knots()
        .try_clone_for_decode(ctx, "creo saved spline sketch knots")?;
    let mut controls = Vec::new();
    ctx.reserve_vec(
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
            ctx.reserve_vec(
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
    let poles =
        if let Some(weights) = weights {
            let mut paired = Vec::new();
            ctx.reserve_vec(
                &mut paired,
                controls.len(),
                "creo saved spline sketch paired poles",
            )?;
            paired.extend(controls.into_iter().zip(weights).map(|(point, weight)| {
                cadmpeg_ir::geometry::pcurve::WeightedPole2 { point, weight }
            }));
            cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles::Rational { points: paired }
        } else {
            cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles::Polynomial { points: controls }
        };
    match cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_admitted_rows(
        nurbs.degree(),
        knots,
        poles,
        nurbs.periodic(),
    ) {
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
        ctx.reserve_vec(
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
        ctx.reserve_vec(
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
    ctx.reserve_vec(
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
    ctx.reserve_vec(
        &mut pole_rows,
        u_control_count,
        "creo interpolation surface NURBS pole rows",
    )?;
    for points in control_points.chunks(v_control_count) {
        let mut row = Vec::new();
        ctx.reserve_vec(
            &mut row,
            points.len(),
            "creo interpolation surface NURBS pole values",
        )?;
        row.extend_from_slice(points);
        pole_rows.push(row);
    }
    match NurbsSurface::from_lanes_for_decode(
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
    for index in 0..count {
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
    let v_knots = match cadmpeg_ir::geometry::nurbs::KnotValue::admit(v_knots) {
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
    match NurbsSurface::from_admitted_grid(
        NurbsSurfaceAxis::new(directrix.degree(), u_knots, directrix.periodic()),
        NurbsSurfaceAxis::new(1, v_knots, false),
        poles,
        false,
    ) {
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

/// Copies a pcurve NURBS with the knot lane and the pole lane each charged under its own operation.
pub(super) fn copy_pcurve_nurbs(
    ctx: &DecodeContext<'_>,
    curve: &cadmpeg_ir::geometry::pcurve::PcurveNurbs,
    knot_operation: &'static str,
    pole_operation: &'static str,
) -> Result<cadmpeg_ir::geometry::pcurve::PcurveNurbs, CodecError> {
    use cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles;

    let knots = curve.knots().try_clone_for_decode(ctx, knot_operation)?;
    let poles = match curve.pole_rows() {
        PcurveNurbsPoles::Polynomial { points } => PcurveNurbsPoles::Polynomial {
            points: ctx.copy_slice(points, pole_operation)?,
        },
        PcurveNurbsPoles::Rational { points } => PcurveNurbsPoles::Rational {
            points: ctx.copy_slice(points, pole_operation)?,
        },
    };
    cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_admitted_rows(
        curve.degree(),
        knots,
        poles,
        curve.periodic(),
    )
    .map_err(CodecError::malformed)
}

pub(super) fn sketch_nurbs_curve(
    ctx: &DecodeContext<'_>,
    geometry: &SketchGeometry,
) -> Result<Option<NurbsCurve>, CodecError> {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::nurbs::{NurbsPoles3, WeightedPole3};
    use cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles;
    use cadmpeg_ir::scalar::FiniteReal;

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
            for point in points {
                let [x, y] = point.coordinates();
                lifted.push(FinitePoint3::from_coordinates(x, y, FiniteReal::ZERO));
            }
            NurbsPoles3::Polynomial { points: lifted }
        }
        PcurveNurbsPoles::Rational { points } => {
            let mut lifted = Vec::new();
            ctx.reserve_vec(&mut lifted, points.len(), "creo sketch NURBS lift poles")?;
            for pole in points {
                let [x, y] = pole.point.coordinates();
                lifted.push(WeightedPole3 {
                    point: FinitePoint3::from_coordinates(x, y, FiniteReal::ZERO),
                    weight: pole.weight,
                });
            }
            NurbsPoles3::Rational { points: lifted }
        }
    };
    let Some(nurbs) =
        NurbsCurve::new_admitted_poles(curve.degree(), knots, poles, curve.periodic()).ok()
    else {
        return Ok(None);
    };
    Ok(valid_positive_nurbs_curve(&nurbs).map(|()| nurbs))
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
        .reverse_parameterization_in_range(lower, upper)
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

    let Some(nurbs) = oriented_sketch_nurbs_curve(ctx, geometry, reversed)? else {
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
            for point in points {
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
            for pole in points {
                let [x, y, _] = pole.point.coordinates();
                projected.push(WeightedPole2 {
                    point: cadmpeg_ir::units::FinitePoint2::from_coordinates(x, y),
                    weight: pole.weight,
                });
            }
            PcurveNurbsPoles::Rational { points: projected }
        }
    };
    match PcurveNurbs::from_admitted_rows(nurbs.degree(), knots, poles, nurbs.periodic()) {
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
    let [start, end] = endpoints;

    if matches!(
        geometry.definition(),
        SketchGeometryDefinition::Nurbs { .. }
    ) {
        let Some(directrix) = oriented_sketch_nurbs_curve(ctx, geometry, reversed)? else {
            return Ok(None);
        };
        let lower_translation = transform.normal().map(|value| value * span.lower());
        let sweep = transform
            .normal()
            .map(|value| value * (span.upper() - span.lower()));
        let Some(placed) = placed_section_nurbs(ctx, transform, &directrix)? else {
            return Ok(None);
        };
        let Some(translated) = translated_nurbs_curve(ctx, &placed, lower_translation)? else {
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
    let Some((control_points, sweep)) =
        tabulated_cylinder_placement(replay, parameters, chart_origin)
    else {
        return Ok(None);
    };
    let mut controls = Vec::new();
    ctx.reserve_vec(
        &mut controls,
        control_points.len(),
        "creo tabulated-cylinder directrix controls",
    )?;
    controls.extend(control_points);
    let mut knots = Vec::new();
    ctx.reserve_vec(&mut knots, 8, "creo tabulated-cylinder directrix knots")?;
    knots.extend([0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
    match NurbsCurve::from_lanes_for_decode(ctx, 3, knots, controls, None, false)? {
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
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                None,
                false,
            )
            .expect("linear sketch NURBS"),
        )
    }

    #[test]
    fn sketch_nurbs_lift_knots_refuse_collection_limit() {
        let geometry = sketch_line_nurbs();
        let error = with_collection_limit(0, |ctx| super::sketch_nurbs_curve(ctx, &geometry))
            .expect_err("four knots exceed zero items");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo sketch NURBS lift knots")
        );
        assert!(
            with_collection_limit(6, |ctx| super::sketch_nurbs_curve(ctx, &geometry))
                .expect("service sized collection")
                .is_some()
        );
    }

    #[test]
    fn sketch_nurbs_lift_poles_refuse_collection_limit() {
        let geometry = sketch_line_nurbs();
        let error = with_collection_limit(4, |ctx| super::sketch_nurbs_curve(ctx, &geometry))
            .expect_err("two poles exceed the four knot items");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo sketch NURBS lift poles")
        );
        assert!(
            with_collection_limit(6, |ctx| super::sketch_nurbs_curve(ctx, &geometry))
                .expect("service sized collection")
                .is_some()
        );
    }

    #[test]
    fn sketch_nurbs_pcurve_knots_refuse_collection_limit() {
        let geometry = sketch_line_nurbs();
        let error = with_collection_limit(6, |ctx| {
            super::sketch_nurbs_pcurve(
                ctx,
                &geometry,
                false,
                &"linear sketch",
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
        .expect_err("pcurve knots exceed the lifted curve items");
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
        .expect("service sized collection")
        .is_some());
    }

    #[test]
    fn sketch_nurbs_pcurve_poles_refuse_collection_limit() {
        let geometry = sketch_line_nurbs();
        let error = with_collection_limit(10, |ctx| {
            super::sketch_nurbs_pcurve(
                ctx,
                &geometry,
                false,
                &"linear sketch",
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
        .expect_err("pcurve poles exceed the ten prior items");
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
        .expect("service sized collection")
        .is_some());
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
    fn saved_spline_constructor_refuses_typed_pole_storage() {
        let error = with_collection_limit(39, |ctx| {
            super::saved_spline_nurbs(
                ctx,
                &planar_or_offset_spline(0.0),
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
        .expect_err("typed poles need forty items in total");
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
        // The earlier collections use 302 items; four rows and sixteen poles
        // use twenty more before the first typed row.
        assert!(
            matches!(interpolation_surface_refusal(322), cadmpeg_core::CodecError::ResourceLimit(resource)
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
    fn interpolation_basis_refuses_work_and_recursive_depth() {
        for (work, depth, operation) in [
            (0, 128, "creo interpolation basis work"),
            (u64::MAX, 1, "creo interpolation basis depth"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = work;
            policy.limits.max_recursion_depth = depth;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            for derivative in [false, true] {
                let result = if derivative {
                    super::bspline_basis_derivative(
                        &ctx,
                        0,
                        3,
                        0.5,
                        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                        4,
                    )
                } else {
                    super::bspline_basis(
                        &ctx,
                        0,
                        3,
                        0.5,
                        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                        4,
                    )
                };
                assert!(
                    matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(resource)) if resource.operation == operation)
                );
            }
        }
    }

    #[test]
    fn interpolation_dense_solver_refuses_each_work_boundary() {
        for (cap, operation) in [
            (1, "creo interpolation pivot work"),
            (2, "creo interpolation normalization work"),
            (7, "creo interpolation elimination work"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = super::solve_vector_system(
                &ctx,
                vec![vec![1.0, 0.0], vec![1.0, 1.0]],
                vec![[1.0; 3], [2.0; 3]],
            );
            assert!(
                matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(resource)) if resource.operation == operation)
            );
        }
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
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(f64::MAX, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
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
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(1.0, 2.0, 0.0), Point3::new(3.0, 4.0, 0.0)],
            None,
            false,
        )
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
        for limit in [0, 4] {
            let error =
                with_collection_limit(limit, |ctx| placed_section_nurbs(ctx, &transform, &curve))
                    .expect_err("knot or pole copy exceeds limit");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.operation == "creo placed section NURBS curve"
                    && resource.dimension == ResourceDimension::CollectionItems)
            );
        }
        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            placed_section_nurbs(ctx, &transform, &curve)
        })
        .expect("service resources")
        .is_some());
    }

    #[test]
    fn translated_nurbs_refuses_knot_and_pole_limits() {
        let curve = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(1.0, 2.0, 0.0), Point3::new(3.0, 4.0, 0.0)],
            None,
            false,
        )
        .expect("finite curve");
        for limit in [0, 4] {
            let error = with_collection_limit(limit, |ctx| {
                translated_nurbs_curve(ctx, &curve, [0.0, 0.0, 2.0])
            })
            .expect_err("knot or pole copy exceeds limit");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.operation == "creo translated NURBS curve"
                    && resource.dimension == ResourceDimension::CollectionItems)
            );
        }
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

    fn extruded_nurbs_refusal_at_limit(limit: u64) -> cadmpeg_core::CodecError {
        let curve = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(1.0, 2.0, 0.0), Point3::new(3.0, 4.0, 0.0)],
            Some(vec![1.0, 0.5]),
            false,
        )
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
        .expect_err("surface grid exceeds collection limit")
    }

    #[test]
    fn extruded_nurbs_pole_rows_refuse_collection_limit() {
        assert!(matches!(extruded_nurbs_refusal_at_limit(0),
            cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo extruded NURBS pole rows"));
    }

    #[test]
    fn extruded_nurbs_pole_values_refuse_each_row_limit() {
        for limit in [2, 4] {
            assert!(matches!(extruded_nurbs_refusal_at_limit(limit),
                cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == "creo extruded NURBS pole values"));
        }
    }

    #[test]
    fn extruded_nurbs_u_knots_refuse_collection_limit() {
        assert!(matches!(extruded_nurbs_refusal_at_limit(6),
            cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo extruded NURBS U knots"));
    }
    #[test]
    fn extruded_nurbs_v_knots_refuse_collection_limit() {
        assert!(matches!(extruded_nurbs_refusal_at_limit(13),
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
        let error = with_collection_limit(47, |ctx| {
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
        let error = with_collection_limit(51, |ctx| {
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
    fn non_planar_saved_spline_refusal_text_obeys_retained_byte_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
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
            assert_eq!(
                crate::decode::with_test_decode_ctx(|ctx| super::bspline_basis(
                    ctx, 0, 3, 0.5, knots, 4
                ))
                .expect("service basis"),
                None
            );
            assert_eq!(
                crate::decode::with_test_decode_ctx(|ctx| super::bspline_basis_derivative(
                    ctx, 0, 3, 0.5, knots, 4
                ))
                .expect("service basis"),
                None
            );
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
}
