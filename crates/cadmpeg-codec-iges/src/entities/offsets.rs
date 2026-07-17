// SPDX-License-Identifier: Apache-2.0
//! Offset curve entity projection.

use super::geometry::{entity_loss, source_object};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::ParameterRecord;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, CurveOffsetDistanceLaw, CurveOffsetLawBasis, NurbsCurve, ProceduralCurve,
    ProceduralCurveDefinition,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, ProceduralCurveId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

fn cross(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(
        left.y * right.z - left.z * right.y,
        left.z * right.x - left.x * right.z,
        left.x * right.y - left.y * right.x,
    )
}

fn dot(left: Vector3, right: Vector3) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}

fn normalized(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm.is_finite() && norm > 0.0)
        .then(|| Vector3::new(vector.x / norm, vector.y / norm, vector.z / norm))
}

fn add(point: Point3, vector: Vector3, scale: f64) -> Point3 {
    Point3::new(
        point.x + vector.x * scale,
        point.y + vector.y * scale,
        point.z + vector.z * scale,
    )
}

fn coordinate(point: Point3, index: u8) -> Option<f64> {
    match index {
        1 => Some(point.x),
        2 => Some(point.y),
        3 => Some(point.z),
        _ => None,
    }
}

fn greville(knots: &[f64], degree: usize, control: usize) -> Option<f64> {
    let values = knots.get(control + 1..=control + degree)?;
    Some(values.iter().sum::<f64>() / degree as f64)
}

pub(super) struct OffsetProjection {
    pub(super) handled: BTreeSet<u32>,
    pub(super) decoded: BTreeSet<u32>,
    pub(super) losses: Vec<LossNote>,
    pub(super) wire_edges: Vec<EdgeId>,
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &Global,
) -> OffsetProjection {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let mut handled = BTreeSet::new();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut wire_edges = Vec::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 130 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(factor) = global.length_factor_mm() else {
            losses.push(entity_loss(entry, "units or model scale are unsupported"));
            continue;
        };
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(source_sequence) = record
            .integer(1)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(entry, "offset source pointer is invalid"));
            continue;
        };
        let Some(flag) = record.integer(2).filter(|flag| matches!(flag, 1..=3)) else {
            losses.push(entity_loss(entry, "offset distance flag is not 1, 2, or 3"));
            continue;
        };
        let components = [record.number(10), record.number(11), record.number(12)];
        let [Some(x), Some(y), Some(z)] = components else {
            losses.push(entity_loss(entry, "offset plane normal is not numeric"));
            continue;
        };
        let Some(normal) = normalized(Vector3::new(x, y, z)) else {
            losses.push(entity_loss(
                entry,
                "offset plane normal is zero or non-finite",
            ));
            continue;
        };
        if (Vector3::new(x, y, z).norm() - 1.0).abs() > 1.0e-10 {
            losses.push(entity_loss(
                entry,
                "offset plane normal is not a unit vector",
            ));
            continue;
        }
        let native_bounds = [record.number(13), record.number(14)];
        let [Some(native_start), Some(native_end)] = native_bounds else {
            losses.push(entity_loss(
                entry,
                "offset parameter interval is not numeric",
            ));
            continue;
        };
        if !native_start.is_finite() || !native_end.is_finite() || native_start >= native_end {
            losses.push(entity_loss(
                entry,
                "offset parameter interval is not increasing",
            ));
            continue;
        }
        if entry.transform != 0 {
            losses.push(entity_loss(
                entry,
                "placed offset curves require composed source projection",
            ));
            continue;
        }
        let source_id = CurveId(format!("iges:model:curve#D{source_sequence}"));
        let Some(source) = ir.model.curves.iter().find(|curve| curve.id == source_id) else {
            losses.push(entity_loss(entry, "offset source curve is missing"));
            continue;
        };
        let source_range = ir.model.edges.iter().find_map(|edge| {
            (edge.curve.as_ref() == Some(&source_id))
                .then_some(edge.param_range)
                .flatten()
        });
        let (parameter_origin, parameter_factor) =
            if matches!(source.geometry, CurveGeometry::Line { .. }) {
                let Some(range) = source_range else {
                    losses.push(entity_loss(
                        entry,
                        "offset line source has no bounded parameter domain",
                    ));
                    continue;
                };
                (range[0], range[1] - range[0])
            } else {
                (0.0, 1.0)
            };
        let start = parameter_origin + native_start * parameter_factor;
        let end = parameter_origin + native_end * parameter_factor;
        let within_source_domain = ir.model.edges.iter().any(|edge| {
            edge.curve.as_ref() == Some(&source_id)
                && edge
                    .param_range
                    .is_some_and(|range| start >= range[0] && end <= range[1])
        });
        if !within_source_domain {
            losses.push(entity_loss(
                entry,
                "offset parameter interval lies outside the source curve domain",
            ));
            continue;
        }
        let (distance, distance_law, geometry) = match flag {
            1 => {
                if record.integer(3) != Some(0)
                    || record.integer(4) != Some(0)
                    || record.integer(5) != Some(0)
                    || record.number(7) != Some(0.0)
                    || record.number(8) != Some(0.0)
                    || record.number(9) != Some(0.0)
                {
                    losses.push(entity_loss(
                        entry,
                        "uniform offset has a nonzero unused field",
                    ));
                    continue;
                }
                let Some(distance) = record.number(6).filter(|value| value.is_finite()) else {
                    losses.push(entity_loss(entry, "uniform offset distance is not finite"));
                    continue;
                };
                let distance = distance * factor;
                let geometry = match &source.geometry {
                    CurveGeometry::Line { origin, direction }
                        if dot(normal, *direction).abs() <= 1.0e-10 =>
                    {
                        CurveGeometry::Line {
                            origin: add(*origin, cross(normal, *direction), distance),
                            direction: *direction,
                        }
                    }
                    CurveGeometry::Circle {
                        center,
                        axis,
                        ref_direction,
                        radius,
                    } if dot(normal, *axis).abs() >= 1.0 - 1.0e-10 => {
                        let offset_radius = radius - distance * dot(normal, *axis).signum();
                        if offset_radius <= 0.0 {
                            losses.push(entity_loss(
                                entry,
                                "offset collapses or reverses the circle",
                            ));
                            continue;
                        }
                        CurveGeometry::Circle {
                            center: *center,
                            axis: *axis,
                            ref_direction: *ref_direction,
                            radius: offset_radius,
                        }
                    }
                    _ => {
                        losses.push(entity_loss(
                            entry,
                            "source curve has no exact uniform offset carrier",
                        ));
                        continue;
                    }
                };
                (distance, None, geometry)
            }
            2 => {
                if record.integer(3) != Some(0) || record.integer(4) != Some(0) {
                    losses.push(entity_loss(
                        entry,
                        "linear offset has a nonzero function field",
                    ));
                    continue;
                }
                let basis = match record.integer(5) {
                    Some(1) => CurveOffsetLawBasis::ArcLength,
                    Some(2) => CurveOffsetLawBasis::Parameter,
                    _ => {
                        losses.push(entity_loss(entry, "linear offset basis is not 1 or 2"));
                        continue;
                    }
                };
                let values = [
                    record.number(6),
                    record.number(7),
                    record.number(8),
                    record.number(9),
                ];
                let [Some(d1), Some(td1), Some(d2), Some(td2)] = values else {
                    losses.push(entity_loss(entry, "linear offset controls are not numeric"));
                    continue;
                };
                if [d1, td1, d2, td2].iter().any(|value| !value.is_finite()) || td1 >= td2 {
                    losses.push(entity_loss(
                        entry,
                        "linear offset control range is not increasing and finite",
                    ));
                    continue;
                }
                let distances = [d1 * factor, d2 * factor];
                let control_factor = match basis {
                    CurveOffsetLawBasis::ArcLength => factor,
                    CurveOffsetLawBasis::Parameter => parameter_factor,
                };
                let control_origin = match basis {
                    CurveOffsetLawBasis::ArcLength => 0.0,
                    CurveOffsetLawBasis::Parameter => parameter_origin,
                };
                let control_range = [
                    control_origin + td1 * control_factor,
                    control_origin + td2 * control_factor,
                ];
                let CurveGeometry::Line { direction, .. } = &source.geometry else {
                    losses.push(entity_loss(
                        entry,
                        "linear offset source has no exact neutral carrier",
                    ));
                    continue;
                };
                if dot(normal, *direction).abs() > 1.0e-10 {
                    losses.push(entity_loss(
                        entry,
                        "offset normal is not perpendicular to the line",
                    ));
                    continue;
                }
                let law_parameter = |parameter: f64| match basis {
                    CurveOffsetLawBasis::Parameter => parameter,
                    CurveOffsetLawBasis::ArcLength => parameter - start,
                };
                let evaluate_distance = |parameter: f64| {
                    let alpha = (law_parameter(parameter) - control_range[0])
                        / (control_range[1] - control_range[0]);
                    distances[0] + alpha * (distances[1] - distances[0])
                };
                let offset_direction = cross(normal, *direction);
                let Some(source_start) = cadmpeg_ir::eval::curve_point(&source.geometry, start)
                else {
                    losses.push(entity_loss(
                        entry,
                        "linear offset source start cannot be evaluated",
                    ));
                    continue;
                };
                let Some(source_end) = cadmpeg_ir::eval::curve_point(&source.geometry, end) else {
                    losses.push(entity_loss(
                        entry,
                        "linear offset source end cannot be evaluated",
                    ));
                    continue;
                };
                let controls = vec![
                    add(source_start, offset_direction, evaluate_distance(start)),
                    add(source_end, offset_direction, evaluate_distance(end)),
                ];
                let law = CurveOffsetDistanceLaw::Linear {
                    basis,
                    distances,
                    control_range,
                };
                (
                    distances[0],
                    Some(law),
                    CurveGeometry::Nurbs(NurbsCurve {
                        degree: 1,
                        knots: vec![start, start, end, end],
                        control_points: controls,
                        weights: None,
                        periodic: false,
                    }),
                )
            }
            3 => {
                let Some(function_sequence) = record
                    .integer(3)
                    .and_then(|value| u32::try_from(value).ok())
                else {
                    losses.push(entity_loss(entry, "offset function pointer is invalid"));
                    continue;
                };
                let Some(coordinate_index) = record
                    .integer(4)
                    .and_then(|value| u8::try_from(value).ok())
                    .filter(|value| matches!(value, 1..=3))
                else {
                    losses.push(entity_loss(
                        entry,
                        "offset function coordinate is not 1, 2, or 3",
                    ));
                    continue;
                };
                let basis = match record.integer(5) {
                    Some(1) => CurveOffsetLawBasis::ArcLength,
                    Some(2) => CurveOffsetLawBasis::Parameter,
                    _ => {
                        losses.push(entity_loss(entry, "function offset basis is not 1 or 2"));
                        continue;
                    }
                };
                if (6..=9).any(|index| record.number(index) != Some(0.0)) {
                    losses.push(entity_loss(
                        entry,
                        "function offset has a nonzero unused field",
                    ));
                    continue;
                }
                let function_id = CurveId(format!("iges:model:curve#D{function_sequence}"));
                let Some(function) = ir.model.curves.iter().find(|curve| curve.id == function_id)
                else {
                    losses.push(entity_loss(entry, "offset function curve is missing"));
                    continue;
                };
                let CurveGeometry::Nurbs(function_nurbs) = &function.geometry else {
                    losses.push(entity_loss(
                        entry,
                        "offset function has no polynomial NURBS carrier",
                    ));
                    continue;
                };
                if function_nurbs.weights.is_some() || function_nurbs.degree == 0 {
                    losses.push(entity_loss(
                        entry,
                        "offset function is rational or degree zero",
                    ));
                    continue;
                }
                let CurveGeometry::Line { direction, .. } = &source.geometry else {
                    losses.push(entity_loss(
                        entry,
                        "function offset source has no exact neutral carrier",
                    ));
                    continue;
                };
                if dot(normal, *direction).abs() > 1.0e-10 {
                    losses.push(entity_loss(
                        entry,
                        "offset normal is not perpendicular to the line",
                    ));
                    continue;
                }
                let (function_parameter_offset, function_parameter_scale) = match basis {
                    CurveOffsetLawBasis::ArcLength => (0.0, 1.0 / factor),
                    CurveOffsetLawBasis::Parameter => {
                        (-parameter_origin / parameter_factor, 1.0 / parameter_factor)
                    }
                };
                let independent_range = match basis {
                    CurveOffsetLawBasis::ArcLength => [0.0, end - start],
                    CurveOffsetLawBasis::Parameter => [start, end],
                };
                let function_range = independent_range
                    .map(|value| function_parameter_offset + function_parameter_scale * value);
                let degree = function_nurbs.degree as usize;
                let Some(domain_start) = function_nurbs.knots.get(degree).copied() else {
                    losses.push(entity_loss(entry, "offset function knot domain is missing"));
                    continue;
                };
                let Some(domain_end) = function_nurbs
                    .knots
                    .get(function_nurbs.knots.len().saturating_sub(degree + 1))
                    .copied()
                else {
                    losses.push(entity_loss(entry, "offset function knot domain is missing"));
                    continue;
                };
                if function_range[0] < domain_start || function_range[1] > domain_end {
                    losses.push(entity_loss(
                        entry,
                        "offset function domain does not cover the source interval",
                    ));
                    continue;
                }
                let inverse_parameter =
                    |value: f64| (value - function_parameter_offset) / function_parameter_scale;
                let source_parameter = |independent: f64| match basis {
                    CurveOffsetLawBasis::ArcLength => start + independent,
                    CurveOffsetLawBasis::Parameter => independent,
                };
                let offset_direction = cross(normal, *direction);
                let mut controls = Vec::with_capacity(function_nurbs.control_points.len());
                for (index, function_control) in
                    function_nurbs.control_points.iter().copied().enumerate()
                {
                    let Some(function_parameter) = greville(&function_nurbs.knots, degree, index)
                    else {
                        losses.push(entity_loss(
                            entry,
                            "offset function Greville parameter is missing",
                        ));
                        controls.clear();
                        break;
                    };
                    let independent = inverse_parameter(function_parameter);
                    let Some(base) = cadmpeg_ir::eval::curve_point(
                        &source.geometry,
                        source_parameter(independent),
                    ) else {
                        controls.clear();
                        break;
                    };
                    let Some(distance) = coordinate(function_control, coordinate_index) else {
                        controls.clear();
                        break;
                    };
                    controls.push(add(base, offset_direction, distance));
                }
                if controls.len() != function_nurbs.control_points.len() {
                    losses.push(entity_loss(
                        entry,
                        "offset function controls cannot be composed",
                    ));
                    continue;
                }
                let knots = function_nurbs
                    .knots
                    .iter()
                    .map(|value| source_parameter(inverse_parameter(*value)))
                    .collect();
                let Some(function_start) =
                    cadmpeg_ir::eval::curve_point(&function.geometry, function_range[0])
                else {
                    losses.push(entity_loss(
                        entry,
                        "offset function start cannot be evaluated",
                    ));
                    continue;
                };
                let Some(distance) = coordinate(function_start, coordinate_index) else {
                    losses.push(entity_loss(entry, "offset function coordinate is invalid"));
                    continue;
                };
                let law = CurveOffsetDistanceLaw::Coordinate {
                    function: function_id,
                    coordinate: coordinate_index,
                    basis,
                    function_parameter_offset,
                    function_parameter_scale,
                };
                (
                    distance,
                    Some(law),
                    CurveGeometry::Nurbs(NurbsCurve {
                        degree: function_nurbs.degree,
                        knots,
                        control_points: controls,
                        weights: None,
                        periodic: false,
                    }),
                )
            }
            _ => {
                losses.push(entity_loss(entry, "offset curve form is unsupported"));
                continue;
            }
        };
        let Some(start_position) = cadmpeg_ir::eval::curve_point(&geometry, start) else {
            losses.push(entity_loss(
                entry,
                "offset start parameter cannot be evaluated",
            ));
            continue;
        };
        let Some(end_position) = cadmpeg_ir::eval::curve_point(&geometry, end) else {
            losses.push(entity_loss(
                entry,
                "offset end parameter cannot be evaluated",
            ));
            continue;
        };
        let curve_id = CurveId(format!("iges:model:curve#D{}", entry.sequence));
        let start_point = PointId(format!("iges:model:point#D{}:start", entry.sequence));
        let end_point = PointId(format!("iges:model:point#D{}:end", entry.sequence));
        let start_vertex = VertexId(format!("iges:model:vertex#D{}:start", entry.sequence));
        let end_vertex = VertexId(format!("iges:model:vertex#D{}:end", entry.sequence));
        let edge_id = EdgeId(format!("iges:model:edge#D{}", entry.sequence));
        ir.model.points.extend([
            Point {
                source_object: None,
                id: start_point.clone(),
                position: start_position,
            },
            Point {
                source_object: None,
                id: end_point.clone(),
                position: end_position,
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
            id: curve_id.clone(),
            geometry,
            source_object: Some(source_object(entry)),
        });
        ir.model.edges.push(Edge {
            id: edge_id.clone(),
            curve: Some(curve_id.clone()),
            start: start_vertex,
            end: end_vertex,
            param_range: Some([start, end]),
            tolerance: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: ProceduralCurveId(format!("iges:model:procedural-curve#D{}", entry.sequence)),
            curve: curve_id,
            definition: ProceduralCurveDefinition::Offset {
                source: source_id,
                distance,
                support: None,
                direction: None,
                normal: Some(normal),
                parameter_range: Some([start, end]),
                distance_law,
            },
            cache_fit_tolerance: None,
        });
        wire_edges.push(edge_id);
        decoded.insert(entry.sequence);
    }

    OffsetProjection {
        handled,
        decoded,
        losses,
        wire_edges,
    }
}
