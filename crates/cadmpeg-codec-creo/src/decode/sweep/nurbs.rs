// SPDX-License-Identifier: Apache-2.0
//! B-spline basis, interpolation, extruded NURBS helpers, and tabulated-cylinder directrices.

use super::super::analytic::{cross, nurbs_intrinsic_parameter_range, valid_positive_nurbs_curve};
use super::super::holes::ExtrusionSpan;
use super::super::sketch::{normalized, section_point_in_model, section_xyz_in_model};
use cadmpeg_ir::geometry::{NurbsCurve, NurbsSurface, PcurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::SketchGeometry;

const EPS_PLANAR_COORDINATE: f64 = 1.0e-12;

pub(in super::super) fn extruded_geometry_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
) -> Option<SurfaceGeometry> {
    match geometry {
        SketchGeometry::Line { start, end } => {
            let start = section_point_in_model(transform, [start.u, start.v]);
            let end = section_point_in_model(transform, [end.u, end.v]);
            let line = normalized(std::array::from_fn(|axis| end[axis] - start[axis]))?;
            let normal = normalized(cross(line, transform.normal))?;
            Some(SurfaceGeometry::Plane {
                origin: Point3::new(start[0], start[1], start[2]),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(line[0], line[1], line[2]),
            })
        }
        SketchGeometry::Arc { center, radius, .. } | SketchGeometry::Circle { center, radius } => {
            let center = section_point_in_model(transform, [center.u, center.v]);
            Some(SurfaceGeometry::Cylinder {
                origin: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(
                    transform.normal[0],
                    transform.normal[1],
                    transform.normal[2],
                ),
                ref_direction: Vector3::new(
                    transform.u_axis[0],
                    transform.u_axis[1],
                    transform.u_axis[2],
                ),
                radius: radius.0,
            })
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
) -> f64 {
    if parameter == *knots.last().expect("nonempty knots") {
        return if index + 1 == count { 1.0 } else { 0.0 };
    }
    if degree == 0 {
        return if knots[index] <= parameter && parameter < knots[index + 1] {
            1.0
        } else {
            0.0
        };
    }
    let left_denominator = knots[index + degree] - knots[index];
    let right_denominator = knots[index + degree + 1] - knots[index + 1];
    let left = if left_denominator > 0.0 {
        (parameter - knots[index]) / left_denominator
            * bspline_basis(index, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    let right = if right_denominator > 0.0 {
        (knots[index + degree + 1] - parameter) / right_denominator
            * bspline_basis(index + 1, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    left + right
}

pub(in super::super) fn bspline_basis_derivative(
    index: usize,
    degree: usize,
    parameter: f64,
    knots: &[f64],
    count: usize,
) -> f64 {
    let left_denominator = knots[index + degree] - knots[index];
    let right_denominator = knots[index + degree + 1] - knots[index + 1];
    let left = if left_denominator > 0.0 {
        degree as f64 / left_denominator * bspline_basis(index, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    let right = if right_denominator > 0.0 {
        degree as f64 / right_denominator
            * bspline_basis(index + 1, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    left - right
}

pub(in super::super) fn solve_vector_system(
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
        let pivot_row = matrix[column].clone();
        let pivot_value = values[column];
        for row in 0..count {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            if factor == 0.0 {
                continue;
            }
            for (entry, pivot_entry) in matrix[row][column..].iter_mut().zip(&pivot_row[column..]) {
                *entry -= factor * pivot_entry;
            }
            for (value, pivot) in values[row].iter_mut().zip(pivot_value) {
                *value -= factor * pivot;
            }
        }
    }
    Some(values)
}

pub(in super::super) fn interpolation_curve_data(
    points: &[[f64; 3]],
    parameters: &[f64],
    endpoint_derivatives: [[f64; 3]; 2],
) -> Option<(Vec<f64>, Vec<[f64; 3]>)> {
    const DEGREE: usize = 3;
    let point_count = points.len();
    (point_count >= 2 && parameters.len() == point_count).then_some(())?;
    parameters
        .windows(2)
        .all(|pair| pair[0].is_finite() && pair[0] < pair[1])
        .then_some(())?;
    parameters.last()?.is_finite().then_some(())?;
    let control_count = point_count + 2;
    let mut knots = vec![parameters[0]; DEGREE + 1];
    knots.extend_from_slice(&parameters[1..point_count - 1]);
    knots.extend(std::iter::repeat_n(parameters[point_count - 1], DEGREE + 1));
    let mut matrix = Vec::with_capacity(control_count);
    for parameter in parameters {
        matrix.push(
            (0..control_count)
                .map(|index| bspline_basis(index, DEGREE, *parameter, &knots, control_count))
                .collect(),
        );
    }
    for parameter in [parameters[0], parameters[point_count - 1]] {
        matrix.push(
            (0..control_count)
                .map(|index| {
                    bspline_basis_derivative(index, DEGREE, parameter, &knots, control_count)
                })
                .collect(),
        );
    }
    let mut values = points.to_vec();
    values.extend(endpoint_derivatives);
    Some((knots, solve_vector_system(matrix, values)?))
}

pub(in super::super) fn saved_spline_nurbs(
    spline: &crate::feature::FeatureSavedSpline,
) -> Option<NurbsCurve> {
    (usize::try_from(spline.declared_point_count?).ok()? == spline.interpolation_points.len())
        .then_some(())?;
    let parameters = spline.parameters.as_ref()?;
    let tangents = spline.endpoint_tangents?;
    let (knots, control_points) =
        interpolation_curve_data(&spline.interpolation_points, parameters, tangents)?;
    let control_points = control_points
        .into_iter()
        .map(|point| Point3::new(point[0], point[1], point[2]))
        .collect();
    Some(NurbsCurve {
        degree: 3,
        knots,
        control_points,
        weights: None,
        periodic: false,
    })
}

pub(in super::super) fn saved_spline_sketch_geometry(
    spline: &crate::feature::FeatureSavedSpline,
) -> Option<SketchGeometry> {
    let nurbs = saved_spline_nurbs(spline)?;
    nurbs
        .control_points
        .iter()
        .all(|point| point.z.abs() <= EPS_PLANAR_COORDINATE)
        .then(|| SketchGeometry::Nurbs {
            degree: nurbs.degree,
            knots: nurbs.knots,
            control_points: nurbs
                .control_points
                .into_iter()
                .map(|point| cadmpeg_ir::math::Point2::new(point.x, point.y))
                .collect(),
            weights: nurbs.weights,
            periodic: nurbs.periodic,
        })
}

pub(in super::super) fn interpolation_spline_surface(
    points: &[[f64; 3]],
    u_parameters: &[f64],
    v_parameters: &[f64],
    end_u_derivatives: &[[f64; 3]],
    end_v_derivatives: &[[f64; 3]],
    corner_mixed_derivatives: &[[f64; 3]],
) -> Option<NurbsSurface> {
    let u_sample_count = u_parameters.len();
    let v_sample_count = v_parameters.len();
    let point_count = u_sample_count.checked_mul(v_sample_count)?;
    let u_boundary_derivative_count = v_sample_count.checked_mul(2)?;
    let v_boundary_derivative_count = u_sample_count.checked_mul(2)?;
    (points.len() == point_count
        && end_u_derivatives.len() == u_boundary_derivative_count
        && end_v_derivatives.len() == v_boundary_derivative_count
        && corner_mixed_derivatives.len() == 4)
        .then_some(())?;

    let u_control_count = u_sample_count.checked_add(2)?;
    let v_control_count = v_sample_count.checked_add(2)?;
    let mut position_controls = vec![vec![[0.0; 3]; v_sample_count]; u_control_count];
    let mut u_knots = None;
    for v in 0..v_sample_count {
        let samples = (0..u_sample_count)
            .map(|u| points[u * v_sample_count + v])
            .collect::<Vec<_>>();
        let (knots, controls) = interpolation_curve_data(
            &samples,
            u_parameters,
            [end_u_derivatives[v], end_u_derivatives[v_sample_count + v]],
        )?;
        u_knots.get_or_insert(knots);
        for (u, control) in controls.into_iter().enumerate() {
            position_controls[u][v] = control;
        }
    }

    let mut v_derivative_controls = vec![vec![[0.0; 3]; u_control_count]; 2];
    for v_boundary in 0..2 {
        let samples = (0..u_sample_count)
            .map(|u| end_v_derivatives[v_boundary * u_sample_count + u])
            .collect::<Vec<_>>();
        let (_, controls) = interpolation_curve_data(
            &samples,
            u_parameters,
            [
                corner_mixed_derivatives[v_boundary * 2],
                corner_mixed_derivatives[v_boundary * 2 + 1],
            ],
        )?;
        v_derivative_controls[v_boundary] = controls;
    }

    let mut control_points = Vec::with_capacity(u_control_count * v_control_count);
    let mut v_knots = None;
    for u in 0..u_control_count {
        let (knots, controls) = interpolation_curve_data(
            &position_controls[u],
            v_parameters,
            [v_derivative_controls[0][u], v_derivative_controls[1][u]],
        )?;
        v_knots.get_or_insert(knots);
        control_points.extend(
            controls
                .into_iter()
                .map(|point| Point3::new(point[0], point[1], point[2])),
        );
    }

    Some(NurbsSurface {
        u_degree: 3,
        v_degree: 3,
        u_knots: u_knots?,
        v_knots: v_knots?,
        u_count: u32::try_from(u_control_count).ok()?,
        v_count: u32::try_from(v_control_count).ok()?,
        control_points,
        weights: None,
        u_periodic: false,
        v_periodic: false,
    })
}

pub(in super::super) fn placed_section_nurbs(
    transform: &crate::placement::FeatureSectionTransform,
    nurbs: &NurbsCurve,
) -> NurbsCurve {
    NurbsCurve {
        degree: nurbs.degree,
        knots: nurbs.knots.clone(),
        control_points: nurbs
            .control_points
            .iter()
            .map(|point| {
                let placed = section_xyz_in_model(transform, [point.x, point.y, point.z]);
                Point3::new(placed[0], placed[1], placed[2])
            })
            .collect(),
        weights: nurbs.weights.clone(),
        periodic: nurbs.periodic,
    }
}

pub(in super::super) fn translated_nurbs_curve(
    curve: &NurbsCurve,
    translation: [f64; 3],
) -> NurbsCurve {
    NurbsCurve {
        degree: curve.degree,
        knots: curve.knots.clone(),
        control_points: curve
            .control_points
            .iter()
            .map(|point| {
                Point3::new(
                    point.x + translation[0],
                    point.y + translation[1],
                    point.z + translation[2],
                )
            })
            .collect(),
        weights: curve.weights.clone(),
        periodic: curve.periodic,
    }
}

pub(in super::super) fn extruded_nurbs_surface(
    directrix: &NurbsCurve,
    sweep: [f64; 3],
) -> Option<NurbsSurface> {
    if directrix
        .weights
        .as_ref()
        .is_some_and(|weights| weights.len() != directrix.control_points.len())
    {
        return None;
    }
    let mut control_points = Vec::with_capacity(directrix.control_points.len() * 2);
    let mut weights = directrix
        .weights
        .as_ref()
        .map(|_| Vec::with_capacity(control_points.capacity()));
    for (index, point) in directrix.control_points.iter().enumerate() {
        control_points.push(*point);
        control_points.push(Point3::new(
            point.x + sweep[0],
            point.y + sweep[1],
            point.z + sweep[2],
        ));
        if let (Some(source), Some(target)) = (&directrix.weights, &mut weights) {
            target.extend([source[index], source[index]]);
        }
    }
    Some(NurbsSurface {
        u_degree: directrix.degree,
        v_degree: 1,
        u_knots: directrix.knots.clone(),
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: u32::try_from(directrix.control_points.len()).ok()?,
        v_count: 2,
        control_points,
        weights,
        u_periodic: directrix.periodic,
        v_periodic: false,
    })
}

pub(in super::super) fn sketch_nurbs_curve(geometry: &SketchGeometry) -> Option<NurbsCurve> {
    let SketchGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = geometry
    else {
        return None;
    };
    let nurbs = NurbsCurve {
        degree: *degree,
        knots: knots.clone(),
        control_points: control_points
            .iter()
            .map(|point| Point3::new(point.u, point.v, 0.0))
            .collect(),
        weights: weights.clone(),
        periodic: *periodic,
    };
    valid_positive_nurbs_curve(&nurbs).map(|()| nurbs)
}

pub(in super::super) fn oriented_sketch_nurbs_curve(
    geometry: &SketchGeometry,
    reversed: bool,
) -> Option<NurbsCurve> {
    let nurbs = sketch_nurbs_curve(geometry)?;
    if !reversed {
        return Some(nurbs);
    }
    let [lower, upper] = nurbs_intrinsic_parameter_range(&nurbs)?;
    Some(NurbsCurve {
        degree: nurbs.degree,
        knots: nurbs
            .knots
            .iter()
            .rev()
            .map(|knot| lower + upper - knot)
            .collect(),
        control_points: nurbs.control_points.into_iter().rev().collect(),
        weights: nurbs
            .weights
            .map(|weights| weights.into_iter().rev().collect()),
        periodic: nurbs.periodic,
    })
}

pub(in super::super) fn sketch_nurbs_pcurve(
    geometry: &SketchGeometry,
    reversed: bool,
) -> Option<PcurveGeometry> {
    let nurbs = oriented_sketch_nurbs_curve(geometry, reversed)?;
    Some(PcurveGeometry::Nurbs {
        degree: nurbs.degree,
        knots: nurbs.knots,
        control_points: nurbs
            .control_points
            .into_iter()
            .map(|point| Point2::new(point.x, point.y))
            .collect(),
        weights: nurbs.weights,
        periodic: nurbs.periodic,
    })
}

pub(in super::super) fn extrusion_brep_side_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
    reversed: bool,
    start: [f64; 2],
    end: [f64; 2],
    span: ExtrusionSpan,
) -> Option<SurfaceGeometry> {
    if matches!(geometry, SketchGeometry::Nurbs { .. }) {
        let directrix = oriented_sketch_nurbs_curve(geometry, reversed)?;
        let placed = placed_section_nurbs(transform, &directrix);
        let lower_translation = transform.normal.map(|value| value * span.lower);
        let sweep = transform
            .normal
            .map(|value| value * (span.upper - span.lower));
        return Some(SurfaceGeometry::Nurbs(extruded_nurbs_surface(
            &translated_nurbs_curve(&placed, lower_translation),
            sweep,
        )?));
    }
    let section_geometry = match geometry {
        SketchGeometry::Line { .. } => SketchGeometry::Line {
            start: Point2::new(start[0], start[1]),
            end: Point2::new(end[0], end[1]),
        },
        value => value.clone(),
    };
    extruded_geometry_surface(transform, &section_geometry)
}

pub(in super::super) fn signed_unit_chart(
    local: [f64; 2],
    frame: [f64; 2],
    offset: f64,
) -> Option<(f64, f64)> {
    let close = |left: f64, right: f64| {
        (left - right).abs() <= 1.0e-9 * left.abs().max(right.abs()).max(1.0)
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
                if close(target[1], slope * local[1] + chart_intercept)
                    && close(chart_intercept.abs(), offset)
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
        .tabulated_cylinder_frame
        .map(|frame| {
            let values = frame.values.to_vec();
            let heads = frame.prefixes;
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
        (left - right).abs() <= 1.0e-9 * left.abs().max(right.abs()).max(1.0)
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
            Point3::new(placed[0], placed[1], placed[2])
        })
        .collect();
    let mut sweep = [0.0; 3];
    sweep[*sweep_axis] = if reflect_sweep {
        first[*sweep_axis] - second[*sweep_axis]
    } else {
        second[*sweep_axis] - first[*sweep_axis]
    };
    (sweep[*sweep_axis].is_finite() && sweep[*sweep_axis] != 0.0).then_some((
        NurbsCurve {
            degree: 3,
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            control_points,
            weights: None,
            periodic: false,
        },
        sweep,
    ))
}
