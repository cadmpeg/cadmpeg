// SPDX-License-Identifier: Apache-2.0
//! Copious point, linear-path, and presentation tuple projection.

use super::geometry::{entity_loss, resolve_transform, source_object};
use crate::directory::DirectoryEntry;
use crate::global::{coincident_distance, Global};
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

const MAX_COPIOUS_TUPLES: usize = 1_000_000;
const MAX_FORM_63_VALIDATION_WORK: u64 = 20_000_000;

pub(super) struct CopiousProjection {
    pub(super) handled: BTreeSet<u32>,
    pub(super) decoded: BTreeSet<u32>,
    pub(super) losses: Vec<LossNote>,
    pub(super) wire_edges: Vec<EdgeId>,
    pub(super) free_vertices: Vec<VertexId>,
}

fn expected_interpretation(form: i64) -> Option<i64> {
    match form {
        1 | 11 | 20 | 21 | 31..=38 | 40 | 63 => Some(1),
        2 | 12 => Some(2),
        3 | 13 => Some(3),
        _ => None,
    }
}

fn presentation_form(form: i64) -> bool {
    matches!(form, 20 | 21 | 31..=38 | 40)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SimpleClosedPathError {
    EndpointsDisagree,
    ConsecutiveCoincidentPoints,
    RepeatedPoint,
    SelfIntersection,
    ValidationBudget,
}

impl SimpleClosedPathError {
    const fn message(self) -> &'static str {
        match self {
            Self::EndpointsDisagree => {
                "simple closed path endpoints disagree beyond the minimum resolution"
            }
            Self::ConsecutiveCoincidentPoints => {
                "simple closed path has coincident consecutive points"
            }
            Self::RepeatedPoint => {
                "simple closed path has a repeated point outside its closure endpoints"
            }
            Self::SelfIntersection => {
                "simple closed path intersects or overlaps itself outside its closure endpoint"
            }
            Self::ValidationBudget => "simple closed path validation exceeded its work budget",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SegmentIntersection {
    None,
    Point(Point2),
    Overlap,
}

fn planar_distance(left: Point2, right: Point2) -> f64 {
    let delta_u = left.u - right.u;
    let delta_v = left.v - right.v;
    delta_u.mul_add(delta_u, delta_v * delta_v).sqrt()
}

fn planar_difference(left: Point2, right: Point2) -> Point2 {
    Point2::new(left.u - right.u, left.v - right.v)
}

fn planar_cross(left: Point2, right: Point2) -> f64 {
    left.u * right.v - left.v * right.u
}

fn planar_point_on_segment(point: Point2, start: Point2, end: Point2) -> bool {
    planar_cross(
        planar_difference(end, start),
        planar_difference(point, start),
    ) == 0.0
        && point.u >= start.u.min(end.u)
        && point.u <= start.u.max(end.u)
        && point.v >= start.v.min(end.v)
        && point.v <= start.v.max(end.v)
}

fn segment_intersection(
    first_start: Point2,
    first_end: Point2,
    second_start: Point2,
    second_end: Point2,
) -> SegmentIntersection {
    let first_direction = planar_difference(first_end, first_start);
    let second_direction = planar_difference(second_end, second_start);
    let between_starts = planar_difference(second_start, first_start);
    let determinant = planar_cross(first_direction, second_direction);
    if determinant != 0.0 {
        let first_parameter = planar_cross(between_starts, second_direction) / determinant;
        let second_parameter = planar_cross(between_starts, first_direction) / determinant;
        if !(0.0..=1.0).contains(&first_parameter) || !(0.0..=1.0).contains(&second_parameter) {
            return SegmentIntersection::None;
        }
        let point = if first_parameter == 0.0 {
            first_start
        } else if first_parameter == 1.0 {
            first_end
        } else if second_parameter == 0.0 {
            second_start
        } else if second_parameter == 1.0 {
            second_end
        } else {
            Point2::new(
                first_start.u + first_parameter * first_direction.u,
                first_start.v + first_parameter * first_direction.v,
            )
        };
        return SegmentIntersection::Point(point);
    }

    if planar_cross(between_starts, first_direction) != 0.0 {
        return SegmentIntersection::None;
    }

    let mut points = Vec::with_capacity(4);
    for point in [first_start, first_end, second_start, second_end] {
        if planar_point_on_segment(point, first_start, first_end)
            && planar_point_on_segment(point, second_start, second_end)
            && !points.contains(&point)
        {
            points.push(point);
        }
    }
    match points.as_slice() {
        [] => SegmentIntersection::None,
        [point] => SegmentIntersection::Point(*point),
        _ => SegmentIntersection::Overlap,
    }
}

fn intersection_is_at(intersection: SegmentIntersection, expected: Point2) -> bool {
    matches!(
        intersection,
        SegmentIntersection::Point(point) if point == expected
    )
}

fn intersection_is_at_closure(
    intersection: SegmentIntersection,
    first: Point2,
    last: Point2,
) -> bool {
    matches!(
        intersection,
        SegmentIntersection::Point(point) if point == first || point == last
    )
}

fn simple_closed_path_error(
    points: &[Point2],
    resolution: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Option<SimpleClosedPathError> {
    if budget.is_some_and(|budget| !budget.charge_by(points.len().saturating_mul(points.len()))) {
        return Some(SimpleClosedPathError::ValidationBudget);
    }
    if points.len() < 2 {
        return Some(SimpleClosedPathError::EndpointsDisagree);
    }
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return Some(SimpleClosedPathError::EndpointsDisagree);
    };
    if !coincident_distance(planar_distance(*first, *last), resolution) {
        return Some(SimpleClosedPathError::EndpointsDisagree);
    }
    if points
        .windows(2)
        .any(|pair| coincident_distance(planar_distance(pair[0], pair[1]), resolution))
    {
        return Some(SimpleClosedPathError::ConsecutiveCoincidentPoints);
    }
    for (first_index, first_point) in points.iter().enumerate() {
        for (second_index, second_point) in points.iter().enumerate().skip(first_index + 1) {
            if first_index == 0 && second_index == points.len() - 1 {
                continue;
            }
            if coincident_distance(planar_distance(*first_point, *second_point), resolution) {
                return Some(SimpleClosedPathError::RepeatedPoint);
            }
        }
    }
    for (first_index, first_segment) in points.windows(2).enumerate() {
        for (second_index, second_segment) in points.windows(2).enumerate().skip(first_index + 1) {
            let intersection = segment_intersection(
                first_segment[0],
                first_segment[1],
                second_segment[0],
                second_segment[1],
            );
            let allowed = if second_index == first_index + 1 {
                intersection_is_at(intersection, first_segment[1])
            } else if first_index == 0 && second_index == points.len() - 2 {
                matches!(intersection, SegmentIntersection::None)
                    || intersection_is_at_closure(intersection, *first, *last)
            } else {
                matches!(intersection, SegmentIntersection::None)
            };
            if !allowed {
                return Some(SimpleClosedPathError::SelfIntersection);
            }
        }
    }
    None
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &Global,
    ctx: Option<&DecodeContext<'_>>,
) -> CopiousProjection {
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
    let mut free_vertices = Vec::new();
    let form_63_work_budget = ctx.map(|ctx| ctx.work_budget(MAX_FORM_63_VALIDATION_WORK));

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 106 && expected_interpretation(entry.form).is_some())
    {
        handled.insert(entry.sequence);
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let (Some(interpretation), Some(tuple_count)) = (
            record.integer(1),
            record
                .integer(2)
                .and_then(|value| usize::try_from(value).ok()),
        ) else {
            losses.push(entity_loss(
                entry,
                "interpretation or tuple count is invalid",
            ));
            continue;
        };
        if Some(interpretation) != expected_interpretation(entry.form) {
            losses.push(entity_loss(
                entry,
                "interpretation flag disagrees with the entity form",
            ));
            continue;
        }
        if tuple_count == 0 || tuple_count > MAX_COPIOUS_TUPLES {
            losses.push(entity_loss(
                entry,
                format!("tuple count is outside 1..={MAX_COPIOUS_TUPLES}"),
            ));
            continue;
        }
        if matches!(entry.form, 11..=13 | 63) && tuple_count < 2 {
            losses.push(entity_loss(
                entry,
                "linear paths require at least two tuples",
            ));
            continue;
        }
        if matches!(entry.form, 20 | 21 | 31..=38) && tuple_count % 2 != 0 {
            losses.push(entity_loss(
                entry,
                "paired presentation form has an odd tuple count",
            ));
            continue;
        }
        if entry.form == 40 && (tuple_count < 3 || tuple_count % 2 == 0) {
            losses.push(entity_loss(
                entry,
                "witness lines require an odd tuple count of at least three",
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
        let (tuple_start, tuple_width, common_z) = match interpretation {
            1 => {
                let Some(z) = record.number(3).filter(|value| value.is_finite()) else {
                    losses.push(entity_loss(entry, "common z coordinate is invalid"));
                    continue;
                };
                (4_usize, 2_usize, Some(z))
            }
            2 => (3, 3, None),
            3 => (3, 6, None),
            _ => {
                losses.push(entity_loss(entry, "copious-data interpretation is invalid"));
                continue;
            }
        };
        let Some(value_count) = tuple_count.checked_mul(tuple_width) else {
            losses.push(entity_loss(entry, "tuple value count overflows"));
            continue;
        };
        let Some(tuple_end) = tuple_start.checked_add(value_count) else {
            losses.push(entity_loss(entry, "tuple end offset overflows"));
            continue;
        };
        let Some(values) = (tuple_start..tuple_end)
            .map(|index| record.number(index).filter(|value| value.is_finite()))
            .collect::<Option<Vec<_>>>()
        else {
            losses.push(entity_loss(entry, "tuple array is truncated or non-finite"));
            continue;
        };
        let definition_points = (entry.form == 63).then(|| {
            values
                .chunks_exact(tuple_width)
                .map(|tuple| Point2::new(tuple[0] * factor, tuple[1] * factor))
                .collect::<Vec<_>>()
        });
        let points = values
            .chunks_exact(tuple_width)
            .map(|tuple| {
                let z = match common_z {
                    Some(z) => z,
                    None => tuple[2],
                };
                transform.point(Point3::new(
                    tuple[0] * factor,
                    tuple[1] * factor,
                    z * factor,
                ))
            })
            .collect::<Vec<_>>();
        if presentation_form(entry.form) {
            continue;
        }
        if matches!(entry.form, 1..=3) {
            for (index, position) in points.into_iter().enumerate() {
                let point = PointId(format!(
                    "iges:model:point#D{}-{}",
                    entry.sequence,
                    index + 1
                ));
                let vertex = VertexId(format!(
                    "iges:model:vertex#D{}-{}",
                    entry.sequence,
                    index + 1
                ));
                ir.model.points.push(Point {
                    source_object: None,
                    id: point.clone(),
                    position,
                });
                ir.model.vertices.push(Vertex {
                    id: vertex.clone(),
                    point,
                    tolerance: None,
                });
                free_vertices.push(vertex);
            }
            decoded.insert(entry.sequence);
            continue;
        }
        let resolution = global.minimum_resolution_mm();
        if let Some(definition_points) = definition_points.as_deref() {
            if let Some(error) = simple_closed_path_error(
                definition_points,
                resolution,
                form_63_work_budget.as_ref(),
            ) {
                losses.push(entity_loss(entry, error.message()));
                continue;
            }
        }
        let topology_tolerance = (entry.form == 63 && resolution > 0.0).then_some(resolution);
        let parameter_end = (points.len() - 1) as f64;
        let mut knots = vec![0.0, 0.0];
        knots.extend((1..points.len() - 1).map(|value| value as f64));
        knots.extend([parameter_end, parameter_end]);
        let start = points[0];
        let end = points[points.len() - 1];
        let stem = format!("D{}", entry.sequence);
        let start_point = PointId(format!("iges:model:point#{stem}-start"));
        let end_point = PointId(format!("iges:model:point#{stem}-end"));
        let start_vertex = VertexId(format!("iges:model:vertex#{stem}-start"));
        let end_vertex = if entry.form == 63 {
            start_vertex.clone()
        } else {
            VertexId(format!("iges:model:vertex#{stem}-end"))
        };
        let curve = CurveId(format!("iges:model:curve#{stem}"));
        let edge = EdgeId(format!("iges:model:edge#{stem}"));
        ir.model.points.push(Point {
            source_object: None,
            id: start_point.clone(),
            position: start,
        });
        ir.model.vertices.push(Vertex {
            id: start_vertex.clone(),
            point: start_point,
            tolerance: topology_tolerance,
        });
        if entry.form != 63 {
            ir.model.points.push(Point {
                source_object: None,
                id: end_point.clone(),
                position: end,
            });
            ir.model.vertices.push(Vertex {
                id: end_vertex.clone(),
                point: end_point,
                tolerance: topology_tolerance,
            });
        }
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Nurbs(NurbsCurve {
                degree: 1,
                knots,
                control_points: points,
                weights: None,
                periodic: false,
            }),
            source_object: Some(source_object(entry)),
        });
        ir.model.edges.push(Edge {
            id: edge.clone(),
            curve: Some(curve),
            start: start_vertex,
            end: end_vertex,
            param_range: Some([0.0, parameter_end]),
            tolerance: topology_tolerance,
        });
        wire_edges.push(edge);
        decoded.insert(entry.sequence);
    }

    CopiousProjection {
        handled,
        decoded,
        losses,
        wire_edges,
        free_vertices,
    }
}

#[cfg(test)]
mod tests;
