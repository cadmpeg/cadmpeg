// SPDX-License-Identifier: Apache-2.0
//! Conic-arc classification and bounded neutral projection.

use super::geometry::{entity_loss, resolve_transform, source_object};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::ParameterRecord;
use cadmpeg_ir::geometry::{Curve, CurveGeometry};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct ConicProjection {
    pub(super) handled: BTreeSet<u32>,
    pub(super) decoded: BTreeSet<u32>,
    pub(super) losses: Vec<LossNote>,
    pub(super) wire_edges: Vec<EdgeId>,
}

fn dot(left: Vector3, right: Vector3) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}

fn cross(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(
        left.y * right.z - left.z * right.y,
        left.z * right.x - left.x * right.z,
        left.x * right.y - left.y * right.x,
    )
}

fn scale(vector: Vector3, factor: f64) -> Vector3 {
    Vector3::new(vector.x * factor, vector.y * factor, vector.z * factor)
}

fn normalized(vector: Vector3) -> Option<(Vector3, f64)> {
    let norm = vector.norm();
    (norm.is_finite() && norm > 0.0).then(|| (scale(vector, 1.0 / norm), norm))
}

fn difference(point: Point3, origin: Point3) -> Vector3 {
    Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z)
}

fn add_bounded_curve(
    ir: &mut CadIr,
    entry: &DirectoryEntry,
    geometry: CurveGeometry,
    start: Point3,
    end: Point3,
    parameter_range: [f64; 2],
) -> EdgeId {
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
        geometry,
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
    edge
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &Global,
) -> ConicProjection {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut handled = BTreeSet::new();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut wire_edges = Vec::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 104 && (0..=3).contains(&entry.form))
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
        let Some(values) = (1..=11)
            .map(|index| record.number(index).filter(|value| value.is_finite()))
            .collect::<Option<Vec<_>>>()
        else {
            losses.push(entity_loss(
                entry,
                "conic coefficients or endpoints are invalid",
            ));
            continue;
        };
        let [coeff_a, coeff_b, coeff_c, coeff_d, coeff_e, coeff_f, plane_z, start_x, start_y, end_x, end_y] =
            values.as_slice()
        else {
            losses.push(entity_loss(entry, "conic parameter count is invalid"));
            continue;
        };
        let coefficient_scale = coeff_a
            .abs()
            .max(coeff_b.abs())
            .max(coeff_c.abs())
            .max(coeff_d.abs())
            .max(coeff_e.abs())
            .max(coeff_f.abs())
            .max(1.0);
        let zero = |value: f64| value.abs() <= coefficient_scale * 1.0e-12;
        if !zero(*coeff_b) || !zero(*coeff_d) {
            losses.push(entity_loss(
                entry,
                "conic is not in the required axis-aligned standard position",
            ));
            continue;
        }
        let transform = match resolve_transform(
            entry.transform,
            &entries,
            &records,
            factor,
            &mut BTreeSet::new(),
        ) {
            Ok(transform) => transform,
            Err(message) => {
                losses.push(entity_loss(entry, message));
                continue;
            }
        };
        let Some((basis_x, scale_x)) = normalized(transform.vector(Vector3::new(1.0, 0.0, 0.0)))
        else {
            losses.push(entity_loss(entry, "conic placement collapses the x axis"));
            continue;
        };
        let Some((basis_y, scale_y)) = normalized(transform.vector(Vector3::new(0.0, 1.0, 0.0)))
        else {
            losses.push(entity_loss(entry, "conic placement collapses the y axis"));
            continue;
        };
        if dot(basis_x, basis_y).abs() > 1.0e-10 {
            losses.push(entity_loss(
                entry,
                "conic placement produces non-orthogonal principal axes",
            ));
            continue;
        }
        let Some((mut axis, _)) = normalized(cross(basis_x, basis_y)) else {
            losses.push(entity_loss(entry, "conic placement collapses its plane"));
            continue;
        };
        let plane_origin = transform.point(Point3::new(0.0, 0.0, *plane_z * factor));
        let start = transform.point(Point3::new(
            *start_x * factor,
            *start_y * factor,
            *plane_z * factor,
        ));
        let end = transform.point(Point3::new(
            *end_x * factor,
            *end_y * factor,
            *plane_z * factor,
        ));

        let geometry_and_range = if zero(*coeff_e) && coeff_a * coeff_c > 0.0 {
            let radius_x_squared = -*coeff_f / *coeff_a;
            let radius_y_squared = -*coeff_f / *coeff_c;
            if radius_x_squared <= 0.0 || radius_y_squared <= 0.0 {
                None
            } else {
                let radius_x = radius_x_squared.sqrt() * factor * scale_x;
                let radius_y = radius_y_squared.sqrt() * factor * scale_y;
                let (major_direction, minor_direction, major_radius, minor_radius) =
                    if radius_x >= radius_y {
                        (basis_x, basis_y, radius_x, radius_y)
                    } else {
                        (basis_y, scale(basis_x, -1.0), radius_y, radius_x)
                    };
                let parameter = |point: Point3| {
                    let delta = difference(point, plane_origin);
                    (dot(delta, minor_direction) / minor_radius)
                        .atan2(dot(delta, major_direction) / major_radius)
                        .rem_euclid(std::f64::consts::TAU)
                };
                let start_parameter = parameter(start);
                let sweep = (parameter(end) - start_parameter).rem_euclid(std::f64::consts::TAU);
                (sweep > 0.0).then_some((
                    CurveGeometry::Ellipse {
                        center: plane_origin,
                        axis,
                        major_direction,
                        major_radius,
                        minor_radius,
                    },
                    [start_parameter, start_parameter + sweep],
                ))
            }
        } else if zero(*coeff_e) && coeff_a * coeff_c < 0.0 {
            let (major, minor, major_squared, minor_squared) = if -*coeff_f / *coeff_a > 0.0 {
                (basis_x, basis_y, -*coeff_f / *coeff_a, *coeff_f / *coeff_c)
            } else {
                (
                    basis_y,
                    scale(basis_x, -1.0),
                    -*coeff_f / *coeff_c,
                    *coeff_f / *coeff_a,
                )
            };
            if major_squared <= 0.0 || minor_squared <= 0.0 {
                None
            } else {
                let major_scale = if dot(major, basis_x).abs() > 0.5 {
                    scale_x
                } else {
                    scale_y
                };
                let minor_scale = if dot(minor, basis_x).abs() > 0.5 {
                    scale_x
                } else {
                    scale_y
                };
                let major_radius = major_squared.sqrt() * factor * major_scale;
                let minor_radius = minor_squared.sqrt() * factor * minor_scale;
                let branch = if dot(difference(start, plane_origin), major) < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let major_direction = scale(major, branch);
                let parameter = |point: Point3, axis: Vector3| {
                    let minor_direction = cross(axis, major_direction);
                    (dot(difference(point, plane_origin), minor_direction) / minor_radius).asinh()
                };
                let mut start_parameter = parameter(start, axis);
                let mut end_parameter = parameter(end, axis);
                if end_parameter < start_parameter {
                    axis = scale(axis, -1.0);
                    start_parameter = parameter(start, axis);
                    end_parameter = parameter(end, axis);
                }
                (end_parameter > start_parameter).then_some((
                    CurveGeometry::Hyperbola {
                        center: plane_origin,
                        axis,
                        major_direction,
                        major_radius,
                        minor_radius,
                    },
                    [start_parameter, end_parameter],
                ))
            }
        } else if zero(*coeff_c) && zero(*coeff_f) && !zero(*coeff_a) && !zero(*coeff_e) {
            let opening = if -*coeff_a / *coeff_e >= 0.0 {
                1.0
            } else {
                -1.0
            };
            let major_direction = scale(basis_y, opening);
            let focal_distance =
                (coeff_e / (4.0 * coeff_a)).abs() * factor * scale_x * scale_x / scale_y;
            let parameter = |point: Point3, axis: Vector3| {
                dot(
                    difference(point, plane_origin),
                    cross(axis, major_direction),
                ) / (2.0 * focal_distance)
            };
            let mut start_parameter = parameter(start, axis);
            let mut end_parameter = parameter(end, axis);
            if end_parameter < start_parameter {
                axis = scale(axis, -1.0);
                start_parameter = parameter(start, axis);
                end_parameter = parameter(end, axis);
            }
            (focal_distance > 0.0 && end_parameter > start_parameter).then_some((
                CurveGeometry::Parabola {
                    vertex: plane_origin,
                    axis,
                    major_direction,
                    focal_distance,
                },
                [start_parameter, end_parameter],
            ))
        } else if zero(*coeff_a) && zero(*coeff_f) && !zero(*coeff_c) && !zero(*coeff_d) {
            let opening = if -*coeff_c / *coeff_d >= 0.0 {
                1.0
            } else {
                -1.0
            };
            let major_direction = scale(basis_x, opening);
            let focal_distance =
                (coeff_d / (4.0 * coeff_c)).abs() * factor * scale_y * scale_y / scale_x;
            let parameter = |point: Point3, axis: Vector3| {
                dot(
                    difference(point, plane_origin),
                    cross(axis, major_direction),
                ) / (2.0 * focal_distance)
            };
            let mut start_parameter = parameter(start, axis);
            let mut end_parameter = parameter(end, axis);
            if end_parameter < start_parameter {
                axis = scale(axis, -1.0);
                start_parameter = parameter(start, axis);
                end_parameter = parameter(end, axis);
            }
            (focal_distance > 0.0 && end_parameter > start_parameter).then_some((
                CurveGeometry::Parabola {
                    vertex: plane_origin,
                    axis,
                    major_direction,
                    focal_distance,
                },
                [start_parameter, end_parameter],
            ))
        } else {
            None
        };

        let Some((geometry, parameter_range)) = geometry_and_range else {
            losses.push(entity_loss(
                entry,
                "standard-position coefficients do not define a nondegenerate conic arc",
            ));
            continue;
        };
        let edge = add_bounded_curve(ir, entry, geometry, start, end, parameter_range);
        wire_edges.push(edge);
        decoded.insert(entry.sequence);
    }

    ConicProjection {
        handled,
        decoded,
        losses,
        wire_edges,
    }
}
