// SPDX-License-Identifier: Apache-2.0
//! Piecewise parametric spline projection.

use super::geometry::{
    entity_loss, resolve_transform, source_object, DeclaredInterval, WireProjectionOutcome,
};
use crate::directory::DirectoryEntry;
use crate::global::{ProjectedGlobal, RealPrecision};
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{alloc_filled, refuse_local_limit, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, SurfaceId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

const MAX_SPLINE_SEGMENTS: usize = 100_000;
const MAX_SPLINE_SURFACE_POLES: usize = 1_000_000;

fn declared_interval(
    record: &ParameterRecord,
    index: usize,
    value: f64,
    precision: RealPrecision,
) -> DeclaredInterval {
    DeclaredInterval::around(value, record.number_uncertainty(index, value, precision))
}

fn terminal_intervals(
    record: &ParameterRecord,
    coefficient_start: usize,
    coefficients: &[f64],
    width: DeclaredInterval,
    precision: RealPrecision,
) -> [DeclaredInterval; 4] {
    let coefficient = |offset: usize| {
        declared_interval(
            record,
            coefficient_start + offset,
            coefficients[offset],
            precision,
        )
    };
    let [a, b, c, d] = [
        coefficient(0),
        coefficient(1),
        coefficient(2),
        coefficient(3),
    ];
    let width_squared = width.multiply(width);
    let width_cubed = width_squared.multiply(width);
    [
        a.add(b.multiply(width))
            .add(c.multiply(width_squared))
            .add(d.multiply(width_cubed)),
        b.add(c.multiply(width).scale(2.0))
            .add(d.multiply(width_squared).scale(3.0)),
        c.add(d.multiply(width).scale(3.0)),
        d,
    ]
}

type IntervalVector = [DeclaredInterval; 3];

fn interval_dot(left: IntervalVector, right: IntervalVector) -> DeclaredInterval {
    left.into_iter()
        .zip(right)
        .fold(DeclaredInterval::around(0.0, 0.0), |sum, (left, right)| {
            sum.add(left.multiply(right))
        })
}

fn interval_unit_tangent(vector: IntervalVector) -> Option<IntervalVector> {
    let speed_squared = interval_dot(vector, vector);
    let inverse_speed = speed_squared.sqrt()?.reciprocal()?;
    Some(vector.map(|component| component.multiply(inverse_speed)))
}

fn interval_curvature(
    derivative: IntervalVector,
    second_derivative: IntervalVector,
) -> Option<IntervalVector> {
    let speed_squared = interval_dot(derivative, derivative);
    let inverse_speed_fourth = speed_squared.multiply(speed_squared).reciprocal()?;
    let derivative_second_dot = interval_dot(derivative, second_derivative);
    Some(std::array::from_fn(|index| {
        second_derivative[index]
            .multiply(speed_squared)
            .subtract(derivative[index].multiply(derivative_second_dot))
            .multiply(inverse_speed_fourth)
    }))
}

fn intervals_overlap(left: IntervalVector, right: IntervalVector) -> bool {
    left.into_iter()
        .zip(right)
        .all(|(left, right)| left.overlaps(right))
}

fn points_within_resolution(left: Point3, right: Point3, resolution: f64) -> bool {
    let distance = left.distance(right);
    if resolution == 0.0 {
        distance == 0.0
    } else {
        distance < resolution
    }
}

fn surface_point_close(left: Point3, right: Point3) -> bool {
    let close = |left: f64, right: f64| {
        (left - right).abs() <= left.abs().max(right.abs()).max(1.0) * 1.0e-10
    };
    close(left.x, right.x) && close(left.y, right.y) && close(left.z, right.z)
}

fn power_to_bezier(coefficients: [f64; 4], width: f64) -> [f64; 4] {
    let [a, b, c, d] = coefficients;
    [
        a,
        a + b * width / 3.0,
        a + 2.0 * b * width / 3.0 + c * width * width / 3.0,
        a + b * width + c * width * width + d * width * width * width,
    ]
}

fn patch_bezier(coefficients: &[f64], u_width: f64, v_width: f64) -> [[f64; 4]; 4] {
    let mut u_converted = [[0.0; 4]; 4];
    for v_power in 0..4 {
        let converted = power_to_bezier(
            [
                coefficients[v_power * 4],
                coefficients[v_power * 4 + 1],
                coefficients[v_power * 4 + 2],
                coefficients[v_power * 4 + 3],
            ],
            u_width,
        );
        for u_control in 0..4 {
            u_converted[u_control][v_power] = converted[u_control];
        }
    }
    let mut result = [[0.0; 4]; 4];
    for u_control in 0..4 {
        result[u_control] = power_to_bezier(u_converted[u_control], v_width);
    }
    result
}

fn add_edge(
    ir: &mut CadIr,
    entry: &DirectoryEntry,
    nurbs: NurbsCurve,
    parameter_range: [f64; 2],
) -> Option<EdgeId> {
    let start = cadmpeg_ir::eval::nurbs_curve_point(
        nurbs.degree,
        &nurbs.knots,
        &nurbs.control_points,
        None,
        parameter_range[0],
    )?;
    let end = cadmpeg_ir::eval::nurbs_curve_point(
        nurbs.degree,
        &nurbs.knots,
        &nurbs.control_points,
        None,
        parameter_range[1],
    )?;
    let stem = format!("D{}", entry.sequence);
    let start_point = PointId(format!("iges:model:point#{stem}-start"));
    let end_point = PointId(format!("iges:model:point#{stem}-end"));
    let start_vertex = VertexId(format!("iges:model:vertex#{stem}-start"));
    let end_vertex = VertexId(format!("iges:model:vertex#{stem}-end"));
    let curve = CurveId(format!("iges:model:curve#{stem}"));
    let edge = EdgeId(format!("iges:model:edge#{stem}"));
    ir.model.points.extend([
        Point {
            source_object: None,
            id: start_point.clone(),
            position: start,
        },
        Point {
            source_object: None,
            id: end_point.clone(),
            position: end,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: start_vertex.clone(),
            point: start_point,
            tolerance: None,
        },
        Vertex {
            id: end_vertex.clone(),
            point: end_point,
            tolerance: None,
        },
    ]);
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Nurbs(nurbs),
        source_object: Some(source_object(entry)),
    });
    ir.model.edges.push(Edge {
        id: edge.clone(),
        curve: Some(curve),
        start: start_vertex,
        end: end_vertex,
        param_range: Some(parameter_range),
        tolerance: None,
    });
    Some(edge)
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<WireProjectionOutcome, CodecError> {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut wire_edges = Vec::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 112 && entry.form == 0)
    {
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let (Some(curve_type), Some(continuity), Some(dimensions)) =
            (record.integer(1), record.integer(2), record.integer(3))
        else {
            losses.push(entity_loss(entry, "spline header fields are not integers"));
            continue;
        };
        if !(1..=6).contains(&curve_type)
            || !(0..=2).contains(&continuity)
            || !matches!(dimensions, 2 | 3)
        {
            losses.push(entity_loss(entry, "spline header enum is out of range"));
            continue;
        }
        let Some(raw_segment_count) = record.integer(4) else {
            losses.push(entity_loss(entry, "spline segment count is invalid"));
            continue;
        };
        if raw_segment_count > MAX_SPLINE_SEGMENTS as i64 {
            return Err(refuse_local_limit(
                "iges_spline_segments",
                MAX_SPLINE_SEGMENTS as u64,
                u64::try_from(raw_segment_count).unwrap_or(u64::MAX),
                None,
            ));
        }
        let Some(segment_count) = usize::try_from(raw_segment_count)
            .ok()
            .filter(|count| *count > 0)
        else {
            losses.push(entity_loss(
                entry,
                format!("segment count is outside 1..={MAX_SPLINE_SEGMENTS}"),
            ));
            continue;
        };
        let Some(breakpoint_count) = segment_count.checked_add(1) else {
            losses.push(entity_loss(entry, "breakpoint count overflows"));
            continue;
        };
        let Some(breakpoints) = (5..5 + breakpoint_count)
            .map(|index| record.number(index).filter(|value| value.is_finite()))
            .collect::<Option<Vec<_>>>()
        else {
            losses.push(entity_loss(
                entry,
                "breakpoint array is truncated or non-finite",
            ));
            continue;
        };
        if breakpoints.windows(2).any(|pair| pair[0] >= pair[1]) {
            losses.push(entity_loss(
                entry,
                "breakpoints are not strictly increasing",
            ));
            continue;
        }
        let coefficient_start = 6 + segment_count;
        let Some(coefficient_count) = segment_count.checked_mul(12) else {
            losses.push(entity_loss(entry, "coefficient count overflows"));
            continue;
        };
        let Some(coefficients) = (coefficient_start..coefficient_start + coefficient_count)
            .map(|index| record.number(index).filter(|value| value.is_finite()))
            .collect::<Option<Vec<_>>>()
        else {
            losses.push(entity_loss(
                entry,
                "coefficient array is truncated or non-finite",
            ));
            continue;
        };
        let transform = match resolve_transform(
            entry.transform,
            &entries,
            &records,
            factor,
            global.real_precision(),
            &mut BTreeSet::new(),
            ctx,
        ) {
            Ok(transform) => transform,
            Err(message) => {
                losses.push(entity_loss(entry, message));
                continue;
            }
        };
        let mut control_points = Vec::with_capacity(segment_count * 3 + 1);
        let mut continuous = true;
        let precision = global.real_precision();
        let resolution = global.minimum_resolution_mm();
        let mut previous_terminal_point = None;
        let mut previous_terminal_tangent = None;
        let mut previous_terminal_curvature = None;
        for (segment, values) in coefficients.chunks_exact(12).enumerate() {
            let width = breakpoints[segment + 1] - breakpoints[segment];
            let width_interval =
                declared_interval(record, 5 + segment + 1, breakpoints[segment + 1], precision)
                    .subtract(declared_interval(
                        record,
                        5 + segment,
                        breakpoints[segment],
                        precision,
                    ));
            let segment_start = coefficient_start + segment * 12;
            let intervals = [
                terminal_intervals(
                    record,
                    segment_start,
                    &values[0..4],
                    width_interval,
                    precision,
                ),
                terminal_intervals(
                    record,
                    segment_start + 4,
                    &values[4..8],
                    width_interval,
                    precision,
                ),
                terminal_intervals(
                    record,
                    segment_start + 8,
                    &values[8..12],
                    width_interval,
                    precision,
                ),
            ];
            if !values
                .chunks_exact(4)
                .any(|component| component[1] != 0.0 || component[2] != 0.0 || component[3] != 0.0)
            {
                continuous = false;
                break;
            }
            if dimensions == 2
                && (1..4).any(|power| {
                    !declared_interval(
                        record,
                        segment_start + 8 + power,
                        values[8 + power],
                        precision,
                    )
                    .contains(0.0)
                })
            {
                continuous = false;
                break;
            }
            let coordinate = |offset: usize| {
                let a = values[offset];
                let b = values[offset + 1];
                let c = values[offset + 2];
                let d = values[offset + 3];
                [
                    a,
                    a + b * width / 3.0,
                    a + 2.0 * b * width / 3.0 + c * width * width / 3.0,
                    a + b * width + c * width * width + d * width * width * width,
                ]
            };
            let x = coordinate(0);
            let y = coordinate(4);
            let z = coordinate(8);
            let start_point =
                transform.point(Point3::new(x[0] * factor, y[0] * factor, z[0] * factor));
            let end_point =
                transform.point(Point3::new(x[3] * factor, y[3] * factor, z[3] * factor));
            // GE-03: IGES §2.2.4.3.19 supplies the positional comparison only.
            if previous_terminal_point.is_some_and(|previous| {
                !points_within_resolution(previous, start_point, resolution)
            }) {
                continuous = false;
                break;
            }
            let starts = [0, 4, 8].map(|offset| {
                [0, 1, 2, 3].map(|order| {
                    declared_interval(
                        record,
                        segment_start + offset + order,
                        values[offset + order],
                        precision,
                    )
                })
            });
            if continuity >= 1 && segment > 0 {
                // GE-03: IGES §4.14 defines slope and curvature continuity but
                // gives no numeric receiver tolerance. Interval overlap is the
                // CADIR admission decision for the declared real values.
                let start_derivative = starts.map(|component| component[1]);
                let Some(start_tangent) = interval_unit_tangent(start_derivative) else {
                    continuous = false;
                    break;
                };
                let Some(previous_tangent) = previous_terminal_tangent else {
                    continuous = false;
                    break;
                };
                if !intervals_overlap(previous_tangent, start_tangent) {
                    continuous = false;
                    break;
                }
                if continuity >= 2 {
                    let start_second_derivative = starts.map(|component| component[2].scale(2.0));
                    let Some(start_curvature) =
                        interval_curvature(start_derivative, start_second_derivative)
                    else {
                        continuous = false;
                        break;
                    };
                    let Some(previous_curvature) = previous_terminal_curvature else {
                        continuous = false;
                        break;
                    };
                    if !intervals_overlap(previous_curvature, start_curvature) {
                        continuous = false;
                        break;
                    }
                }
            }
            if segment + 1 < segment_count && continuity >= 1 {
                let terminal_derivative = intervals.map(|component| component[1]);
                previous_terminal_tangent = interval_unit_tangent(terminal_derivative);
                if previous_terminal_tangent.is_none() {
                    continuous = false;
                    break;
                }
                if continuity >= 2 {
                    let terminal_second_derivative =
                        intervals.map(|component| component[2].scale(2.0));
                    previous_terminal_curvature =
                        interval_curvature(terminal_derivative, terminal_second_derivative);
                    if previous_terminal_curvature.is_none() {
                        continuous = false;
                        break;
                    }
                }
            }
            let bezier = (0..4)
                .map(|index| {
                    transform.point(Point3::new(
                        x[index] * factor,
                        y[index] * factor,
                        z[index] * factor,
                    ))
                })
                .collect::<Vec<_>>();
            if control_points.is_empty() {
                control_points.extend(bezier);
            } else {
                control_points.extend_from_slice(&bezier[1..]);
            }
            previous_terminal_point = Some(end_point);
        }
        if !continuous {
            losses.push(entity_loss(
                entry,
                "spline segments violate declared continuity, planarity, or non-degeneracy",
            ));
            continue;
        }
        let tail_start = coefficient_start + coefficient_count;
        let Some(tail) = (tail_start..tail_start + 12)
            .map(|index| record.number(index).filter(|value| value.is_finite()))
            .collect::<Option<Vec<_>>>()
        else {
            losses.push(entity_loss(entry, "terminal derivative block is missing"));
            continue;
        };
        // GE-03: §4.14 calls this block redundant. CADIR keeps the
        // coefficient-defined carrier when a present block disagrees.
        let last_values = &coefficients[coefficients.len() - 12..];
        let last_segment_start = coefficient_start + (segment_count - 1) * 12;
        let last_width = declared_interval(
            record,
            5 + segment_count,
            breakpoints[segment_count],
            precision,
        )
        .subtract(declared_interval(
            record,
            5 + segment_count - 1,
            breakpoints[segment_count - 1],
            precision,
        ));
        let expected_tail = [0, 4, 8].map(|offset| {
            terminal_intervals(
                record,
                last_segment_start + offset,
                &last_values[offset..offset + 4],
                last_width,
                precision,
            )
        });
        if tail.iter().enumerate().any(|(offset, actual)| {
            !declared_interval(record, tail_start + offset, *actual, precision)
                .overlaps(expected_tail[offset / 4][offset % 4])
        }) {
            losses.push(entity_loss(
                entry,
                "terminal derivative block disagrees with the last polynomial",
            ));
        }
        let mut knots = vec![breakpoints[0]; 4];
        for breakpoint in &breakpoints[1..segment_count] {
            knots.extend([*breakpoint; 3]);
        }
        knots.extend([breakpoints[segment_count]; 4]);
        let nurbs = NurbsCurve {
            degree: 3,
            knots,
            control_points,
            weights: None,
            periodic: false,
        };
        let Some(edge) = add_edge(
            ir,
            entry,
            nurbs,
            [breakpoints[0], breakpoints[segment_count]],
        ) else {
            losses.push(entity_loss(
                entry,
                "converted spline endpoints cannot be evaluated",
            ));
            continue;
        };
        wire_edges.push(edge);
        losses.push(
            IgesLossCode::SplineHeaderNotTransferred
                .note(
                    "Type 112 curve type, continuity, and dimensionality are retained only in native parameters",
                )
                .with_provenance(entry.loss_provenance()),
        );
        decoded.insert(entry.sequence);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 114 && entry.form == 0)
    {
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let (Some(curve_type), Some(patch_type)) = (record.integer(1), record.integer(2)) else {
            losses.push(entity_loss(
                entry,
                "spline-surface type fields are not integers",
            ));
            continue;
        };
        if !(1..=6).contains(&curve_type) || !matches!(patch_type, 0 | 1) {
            losses.push(entity_loss(
                entry,
                "spline-surface type enum is out of range",
            ));
            continue;
        }
        let dimensions = [record.integer(3), record.integer(4)];
        let [Some(raw_u_segments), Some(raw_v_segments)] = dimensions else {
            losses.push(entity_loss(entry, "spline-surface dimensions are invalid"));
            continue;
        };
        if raw_u_segments > 0 && raw_v_segments > 0 {
            let requested = u64::try_from(raw_u_segments)
                .ok()
                .and_then(|value| value.checked_mul(3))
                .and_then(|value| value.checked_add(1))
                .and_then(|u_count| {
                    u64::try_from(raw_v_segments)
                        .ok()
                        .and_then(|value| value.checked_mul(3))
                        .and_then(|value| value.checked_add(1))
                        .and_then(|v_count| u_count.checked_mul(v_count))
                });
            match requested {
                None => {
                    return Err(refuse_local_limit(
                        "iges_spline_surface_poles",
                        MAX_SPLINE_SURFACE_POLES as u64,
                        u64::MAX,
                        None,
                    ));
                }
                Some(requested) if requested > MAX_SPLINE_SURFACE_POLES as u64 => {
                    return Err(refuse_local_limit(
                        "iges_spline_surface_poles",
                        MAX_SPLINE_SURFACE_POLES as u64,
                        requested,
                        None,
                    ));
                }
                Some(_) => {}
            }
        }
        let [Some(u_segments), Some(v_segments)] = [raw_u_segments, raw_v_segments]
            .map(|value| usize::try_from(value).ok().filter(|count| *count > 0))
        else {
            losses.push(entity_loss(entry, "spline-surface dimensions are invalid"));
            continue;
        };
        let (Some(u_count), Some(v_count)) = (
            u_segments
                .checked_mul(3)
                .and_then(|value| value.checked_add(1)),
            v_segments
                .checked_mul(3)
                .and_then(|value| value.checked_add(1)),
        ) else {
            losses.push(entity_loss(
                entry,
                "spline-surface pole dimensions overflow",
            ));
            continue;
        };
        let Some(pole_count) = u_count.checked_mul(v_count) else {
            return Err(refuse_local_limit(
                "iges_spline_surface_poles",
                MAX_SPLINE_SURFACE_POLES as u64,
                u64::MAX,
                None,
            ));
        };
        if pole_count > MAX_SPLINE_SURFACE_POLES {
            return Err(refuse_local_limit(
                "iges_spline_surface_poles",
                MAX_SPLINE_SURFACE_POLES as u64,
                pole_count as u64,
                None,
            ));
        }
        let Some(u_breakpoint_count) = u_segments.checked_add(1) else {
            losses.push(entity_loss(entry, "u-breakpoint count overflows"));
            continue;
        };
        let Some(v_breakpoint_count) = v_segments.checked_add(1) else {
            losses.push(entity_loss(entry, "v-breakpoint count overflows"));
            continue;
        };
        let Some(u_breakpoints) = (5..5 + u_breakpoint_count)
            .map(|index| record.number(index).filter(|value| value.is_finite()))
            .collect::<Option<Vec<_>>>()
        else {
            losses.push(entity_loss(
                entry,
                "u-breakpoints are truncated or non-finite",
            ));
            continue;
        };
        let v_breakpoint_start = 5 + u_breakpoint_count;
        let Some(v_breakpoints) = (v_breakpoint_start..v_breakpoint_start + v_breakpoint_count)
            .map(|index| record.number(index).filter(|value| value.is_finite()))
            .collect::<Option<Vec<_>>>()
        else {
            losses.push(entity_loss(
                entry,
                "v-breakpoints are truncated or non-finite",
            ));
            continue;
        };
        if u_breakpoints.windows(2).any(|pair| pair[0] >= pair[1])
            || v_breakpoints.windows(2).any(|pair| pair[0] >= pair[1])
        {
            losses.push(entity_loss(
                entry,
                "spline-surface breakpoints are not increasing",
            ));
            continue;
        }
        let transform = match resolve_transform(
            entry.transform,
            &entries,
            &records,
            factor,
            global.real_precision(),
            &mut BTreeSet::new(),
            ctx,
        ) {
            Ok(transform) => transform,
            Err(message) => {
                losses.push(entity_loss(entry, message));
                continue;
            }
        };
        let coefficient_start = v_breakpoint_start + v_breakpoint_count;
        let Some(block_columns) = v_segments.checked_add(1) else {
            losses.push(entity_loss(entry, "spline-surface block stride overflows"));
            continue;
        };
        let Some(total_block_count) = u_segments
            .checked_add(1)
            .and_then(|rows| rows.checked_mul(block_columns))
        else {
            losses.push(entity_loss(
                entry,
                "spline-surface placeholder grid overflows",
            ));
            continue;
        };
        let Some(required_parameter_count) = total_block_count
            .checked_mul(48)
            .and_then(|count| coefficient_start.checked_add(count))
        else {
            losses.push(entity_loss(
                entry,
                "spline-surface parameter count overflows",
            ));
            continue;
        };
        if record.parameter_end() < required_parameter_count {
            losses.push(entity_loss(
                entry,
                "spline-surface placeholder grid is truncated",
            ));
            continue;
        }
        let mut grid = ctx.map_or_else(
            || alloc_filled(pole_count, None, "iges spline surface control grid"),
            |ctx| ctx.alloc_filled(pole_count, None, "iges spline surface control grid"),
        )?;
        let mut valid = true;
        'patches: for u_patch in 0..u_segments {
            for v_patch in 0..v_segments {
                let Some(block_index) = u_patch
                    .checked_mul(block_columns)
                    .and_then(|value| value.checked_add(v_patch))
                else {
                    valid = false;
                    break 'patches;
                };
                let Some(block_start) = block_index
                    .checked_mul(48)
                    .and_then(|value| coefficient_start.checked_add(value))
                else {
                    valid = false;
                    break 'patches;
                };
                let Some(values) = (block_start..block_start + 48)
                    .map(|index| record.number(index).filter(|value| value.is_finite()))
                    .collect::<Option<Vec<_>>>()
                else {
                    valid = false;
                    break 'patches;
                };
                let u_width = u_breakpoints[u_patch + 1] - u_breakpoints[u_patch];
                let v_width = v_breakpoints[v_patch + 1] - v_breakpoints[v_patch];
                let coordinates = [
                    patch_bezier(&values[0..16], u_width, v_width),
                    patch_bezier(&values[16..32], u_width, v_width),
                    patch_bezier(&values[32..48], u_width, v_width),
                ];
                for (u_local, x_row) in coordinates[0].iter().enumerate() {
                    for (v_local, x) in x_row.iter().enumerate() {
                        let point = transform.point(Point3::new(
                            *x * factor,
                            coordinates[1][u_local][v_local] * factor,
                            coordinates[2][u_local][v_local] * factor,
                        ));
                        let u_index = u_patch * 3 + u_local;
                        let v_index = v_patch * 3 + v_local;
                        let index = u_index * v_count + v_index;
                        if grid[index].is_some_and(|existing| !surface_point_close(existing, point))
                        {
                            valid = false;
                            break 'patches;
                        }
                        grid[index] = Some(point);
                    }
                }
            }
        }
        if !valid {
            losses.push(entity_loss(
                entry,
                "spline-surface patch indexing or continuity is invalid",
            ));
            continue;
        }
        let Some(control_points) = grid.into_iter().collect::<Option<Vec<_>>>() else {
            losses.push(entity_loss(
                entry,
                "spline-surface patch grid is incomplete",
            ));
            continue;
        };
        let mut u_knots = vec![u_breakpoints[0]; 4];
        for breakpoint in &u_breakpoints[1..u_segments] {
            u_knots.extend([*breakpoint; 3]);
        }
        u_knots.extend([u_breakpoints[u_segments]; 4]);
        let mut v_knots = vec![v_breakpoints[0]; 4];
        for breakpoint in &v_breakpoints[1..v_segments] {
            v_knots.extend([*breakpoint; 3]);
        }
        v_knots.extend([v_breakpoints[v_segments]; 4]);
        let (Ok(u_count), Ok(v_count)) = (u32::try_from(u_count), u32::try_from(v_count)) else {
            losses.push(entity_loss(
                entry,
                "spline-surface pole dimensions exceed u32",
            ));
            continue;
        };
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("iges:model:surface#D{}", entry.sequence)),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree: 3,
                v_degree: 3,
                u_knots,
                v_knots,
                u_count,
                v_count,
                control_points,
                weights: None,
                u_periodic: false,
                v_periodic: false,
            }),
            source_object: Some(source_object(entry)),
        });
        losses.push(
            IgesLossCode::SplineHeaderNotTransferred
                .note("Type 114 curve and patch types are retained only in native parameters")
                .with_provenance(entry.loss_provenance()),
        );
        decoded.insert(entry.sequence);
    }

    Ok(WireProjectionOutcome {
        decoded,
        losses,
        wire_edges,
    })
}

#[cfg(test)]
mod tests;
