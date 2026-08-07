// SPDX-License-Identifier: Apache-2.0
//! Ordered composite-curve projection.

use super::curve_conversion::circular_arc_nurbs;
use super::geometry::{entity_loss, source_object};
use crate::directory::DirectoryEntry;
use crate::parameter::ParameterRecord;
use cadmpeg_ir::geometry::{
    CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry, NurbsCurve,
    ProceduralCurve, ProceduralCurveDefinition,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, ProceduralCurveId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

const MAX_COMPOSITE_CHILDREN: usize = 100_000;

pub(super) struct CompositeProjection {
    pub(super) handled: BTreeSet<u32>,
    pub(super) decoded: BTreeSet<u32>,
    pub(super) losses: Vec<LossNote>,
    pub(super) wire_edges: Vec<EdgeId>,
}

fn point_for_vertex(ir: &CadIr, id: &VertexId) -> Option<Point3> {
    let point = &ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == *id)?
        .point;
    ir.model
        .points
        .iter()
        .find(|candidate| candidate.id == *point)
        .map(|candidate| candidate.position)
}

fn elevate_linear_bezier(curve: &mut NurbsCurve, interval: [f64; 2]) -> bool {
    if curve.degree != 1
        || curve.control_points.len() != 2
        || curve.knots != [interval[0], interval[0], interval[1], interval[1]]
    {
        return false;
    }
    let [start, end] = [curve.control_points[0], curve.control_points[1]];
    let middle = if let Some(weights) = curve.weights.as_ref() {
        if weights.len() != 2
            || weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
        {
            return false;
        }
        let [start_weight, end_weight] = [weights[0], weights[1]];
        let total_weight = start_weight + end_weight;
        if !total_weight.is_finite() || total_weight <= 0.0 {
            return false;
        }
        curve.weights = Some(vec![start_weight, total_weight * 0.5, end_weight]);
        Point3::new(
            (start_weight * start.x + end_weight * end.x) / total_weight,
            (start_weight * start.y + end_weight * end.y) / total_weight,
            (start_weight * start.z + end_weight * end.z) / total_weight,
        )
    } else {
        Point3::new(
            (start.x + end.x) * 0.5,
            (start.y + end.y) * 0.5,
            (start.z + end.z) * 0.5,
        )
    };
    curve.degree = 2;
    curve.knots = vec![
        interval[0],
        interval[0],
        interval[0],
        interval[1],
        interval[1],
        interval[1],
    ];
    curve.control_points = vec![start, middle, end];
    true
}

fn bounded_nurbs(ir: &CadIr, sequence: u32) -> Option<(NurbsCurve, [f64; 2])> {
    let curve_id = CurveId(format!("iges:model:curve#D{sequence}"));
    let curve = ir.model.curves.iter().find(|curve| curve.id == curve_id)?;
    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.as_ref() == Some(&curve_id))?;
    let interval = edge.param_range?;
    match &curve.geometry {
        CurveGeometry::Nurbs(nurbs) => Some((nurbs.clone(), interval)),
        CurveGeometry::Line { .. } => Some((
            NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![
                    point_for_vertex(ir, &edge.start)?,
                    point_for_vertex(ir, &edge.end)?,
                ],
                weights: None,
                periodic: false,
            },
            [0.0, 1.0],
        )),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => Some((
            circular_arc_nurbs(*center, *axis, *ref_direction, *radius, interval)?,
            interval,
        )),
        _ => None,
    }
}

fn close(left: Point3, right: Point3) -> bool {
    let scale = left
        .x
        .abs()
        .max(left.y.abs())
        .max(left.z.abs())
        .max(right.x.abs())
        .max(right.y.abs())
        .max(right.z.abs())
        .max(1.0);
    (left.x - right.x).abs() <= scale * 1.0e-10
        && (left.y - right.y).abs() <= scale * 1.0e-10
        && (left.z - right.z).abs() <= scale * 1.0e-10
}

fn curve_endpoints(ir: &CadIr, curve_id: &CurveId) -> Option<(Point3, Point3)> {
    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.as_ref() == Some(curve_id))?;
    Some((
        point_for_vertex(ir, &edge.start)?,
        point_for_vertex(ir, &edge.end)?,
    ))
}

fn project_native_composite(
    ir: &mut CadIr,
    entry: &DirectoryEntry,
    child_sequences: &[u32],
) -> Option<EdgeId> {
    let child_curves = child_sequences
        .iter()
        .map(|sequence| CurveId(format!("iges:model:curve#D{sequence}")))
        .collect::<Vec<_>>();
    if child_curves
        .iter()
        .any(|curve_id| !ir.model.curves.iter().any(|curve| curve.id == *curve_id))
    {
        return None;
    }
    let endpoints = child_curves
        .iter()
        .map(|curve_id| curve_endpoints(ir, curve_id))
        .collect::<Option<Vec<_>>>()?;
    let start = endpoints.first()?.0;
    let end = endpoints.last()?.1;
    let segments = child_curves
        .iter()
        .enumerate()
        .map(|(index, curve)| CompositeCurveSegment {
            curve: curve.clone(),
            same_sense: true,
            transition: if index > 0 && close(endpoints[index - 1].1, endpoints[index].0) {
                CompositeCurveTransition::Continuous
            } else {
                CompositeCurveTransition::Discontinuous
            },
        })
        .collect();
    let stem = format!("D{}", entry.sequence);
    let start_point = PointId(format!("iges:model:point#{stem}-start"));
    let end_point = PointId(format!("iges:model:point#{stem}-end"));
    let start_vertex = VertexId(format!("iges:model:vertex#{stem}-start"));
    let end_vertex = VertexId(format!("iges:model:vertex#{stem}-end"));
    let curve_id = CurveId(format!("iges:model:curve#{stem}"));
    let edge_id = EdgeId(format!("iges:model:edge#{stem}"));
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
        id: curve_id.clone(),
        geometry: CurveGeometry::Composite {
            segments,
            self_intersect: None,
        },
        source_object: Some(source_object(entry)),
    });
    ir.model.edges.push(Edge {
        id: edge_id.clone(),
        curve: Some(curve_id),
        start: start_vertex,
        end: end_vertex,
        param_range: None,
        tolerance: None,
    });
    Some(edge_id)
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
) -> CompositeProjection {
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
        .filter(|entry| entry.entity_type == 102 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(child_count) = record
            .integer(1)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|count| *count > 0 && *count <= MAX_COMPOSITE_CHILDREN)
        else {
            losses.push(entity_loss(
                entry,
                format!("child count is outside 1..={MAX_COMPOSITE_CHILDREN}"),
            ));
            continue;
        };
        let Some(child_sequences) = (0..child_count)
            .map(|index| {
                record
                    .integer(index + 2)
                    .and_then(|value| u32::try_from(value).ok())
            })
            .collect::<Option<Vec<_>>>()
        else {
            losses.push(entity_loss(entry, "child pointer list is invalid"));
            continue;
        };
        if entry.transform != 0 {
            losses.push(entity_loss(
                entry,
                "placed composite curves require transformed child-carrier projection",
            ));
            continue;
        }
        if child_sequences.iter().any(|sequence| {
            entries
                .get(sequence)
                .is_none_or(|child| child.status.subordinate != 1)
        }) {
            losses.push(entity_loss(
                entry,
                "composite child is missing or is not physically dependent",
            ));
            continue;
        }
        let Some(mut children) = child_sequences
            .iter()
            .map(|sequence| bounded_nurbs(ir, *sequence))
            .collect::<Option<Vec<_>>>()
        else {
            if let Some(edge) = project_native_composite(ir, entry, &child_sequences) {
                wire_edges.push(edge);
                decoded.insert(entry.sequence);
                continue;
            }
            losses.push(entity_loss(
                entry,
                "composite child has no exact bounded line or NURBS carrier",
            ));
            continue;
        };
        let degree = children
            .iter()
            .map(|(curve, _)| curve.degree)
            .max()
            .unwrap_or_default();
        if degree == 2 {
            let mut elevated = true;
            for (curve, interval) in &mut children {
                if curve.degree == 1 && !elevate_linear_bezier(curve, *interval) {
                    elevated = false;
                    break;
                }
            }
            if !elevated {
                if let Some(edge) = project_native_composite(ir, entry, &child_sequences) {
                    wire_edges.push(edge);
                    decoded.insert(entry.sequence);
                    continue;
                }
                losses.push(entity_loss(
                    entry,
                    "composite linear child cannot be elevated exactly",
                ));
                continue;
            }
        }
        if children.iter().any(|(curve, interval)| {
            let Some(first) = curve.knots.first() else {
                return true;
            };
            let Some(last) = curve.knots.last() else {
                return true;
            };
            curve.degree != degree || interval != &[*first, *last] || interval[0] >= interval[1]
        }) {
            if let Some(edge) = project_native_composite(ir, entry, &child_sequences) {
                wire_edges.push(edge);
                decoded.insert(entry.sequence);
                continue;
            }
            losses.push(entity_loss(
                entry,
                "composite children do not share a degree or use their complete knot domains",
            ));
            continue;
        }
        let degree_usize = degree as usize;
        let mut knots = Vec::new();
        let mut control_points = Vec::new();
        let mut weights = Vec::new();
        let mut boundaries = vec![0.0];
        let mut child_starts = Vec::with_capacity(children.len());
        let mut cursor = 0.0;
        let mut valid = true;
        for (child_index, (curve, interval)) in children.into_iter().enumerate() {
            let child_start = interval[0];
            let child_end = interval[1];
            let shift = cursor - child_start;
            let shifted_knots = curve
                .knots
                .iter()
                .map(|knot| knot + shift)
                .collect::<Vec<_>>();
            let mut child_weights = curve
                .weights
                .unwrap_or_else(|| vec![1.0; curve.control_points.len()]);
            if child_index == 0 {
                knots = shifted_knots;
                control_points = curve.control_points;
                weights = child_weights;
            } else {
                if !close(
                    control_points[control_points.len() - 1],
                    curve.control_points[0],
                ) {
                    valid = false;
                    break;
                }
                let scale = weights[weights.len() - 1] / child_weights[0];
                for weight in &mut child_weights {
                    *weight *= scale;
                }
                knots.pop();
                knots.extend_from_slice(&shifted_knots[degree_usize + 1..]);
                control_points.extend_from_slice(&curve.control_points[1..]);
                weights.extend_from_slice(&child_weights[1..]);
            }
            child_starts.push(child_start);
            cursor += child_end - child_start;
            boundaries.push(cursor);
        }
        if !valid || knots.len() != control_points.len() + degree_usize + 1 {
            if let Some(edge) = project_native_composite(ir, entry, &child_sequences) {
                wire_edges.push(edge);
                decoded.insert(entry.sequence);
                continue;
            }
            losses.push(entity_loss(
                entry,
                "composite children are discontinuous or cannot be concatenated",
            ));
            continue;
        }
        let rational = weights
            .first()
            .is_some_and(|first| weights.iter().any(|weight| weight != first));
        let nurbs = NurbsCurve {
            degree,
            knots,
            control_points,
            weights: rational.then_some(weights),
            periodic: false,
        };
        let Some(start) = cadmpeg_ir::eval::nurbs_curve_point(
            degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            0.0,
        ) else {
            if let Some(edge) = project_native_composite(ir, entry, &child_sequences) {
                wire_edges.push(edge);
                decoded.insert(entry.sequence);
                continue;
            }
            losses.push(entity_loss(entry, "composite start cannot be evaluated"));
            continue;
        };
        let Some(end) = cadmpeg_ir::eval::nurbs_curve_point(
            degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            cursor,
        ) else {
            if let Some(edge) = project_native_composite(ir, entry, &child_sequences) {
                wire_edges.push(edge);
                decoded.insert(entry.sequence);
                continue;
            }
            losses.push(entity_loss(entry, "composite end cannot be evaluated"));
            continue;
        };
        let stem = format!("D{}", entry.sequence);
        let start_point = PointId(format!("iges:model:point#{stem}-start"));
        let end_point = PointId(format!("iges:model:point#{stem}-end"));
        let start_vertex = VertexId(format!("iges:model:vertex#{stem}-start"));
        let end_vertex = VertexId(format!("iges:model:vertex#{stem}-end"));
        let curve_id = CurveId(format!("iges:model:curve#{stem}"));
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
            id: curve_id.clone(),
            geometry: CurveGeometry::Nurbs(nurbs),
            source_object: Some(source_object(entry)),
        });
        ir.model.edges.push(Edge {
            id: edge.clone(),
            curve: Some(curve_id.clone()),
            start: start_vertex,
            end: end_vertex,
            param_range: Some([0.0, cursor]),
            tolerance: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: ProceduralCurveId(format!("iges:model:procedural-curve#{stem}")),
            curve: curve_id,
            definition: ProceduralCurveDefinition::Compound {
                parameters: boundaries,
                component_parameters: child_starts,
                components: child_sequences
                    .iter()
                    .map(|sequence| CurveId(format!("iges:model:curve#D{sequence}")))
                    .collect(),
            },
            cache_fit_tolerance: None,
        });
        wire_edges.push(edge);
        decoded.insert(entry.sequence);
    }

    CompositeProjection {
        handled,
        decoded,
        losses,
        wire_edges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_linear_degree_elevation_preserves_the_curve() {
        let mut curve = NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
            weights: Some(vec![1.0, 3.0]),
            periodic: false,
        };
        let before = cadmpeg_ir::eval::nurbs_curve_point(
            curve.degree,
            &curve.knots,
            &curve.control_points,
            curve.weights.as_deref(),
            0.25,
        )
        .expect("valid rational linear NURBS evaluates before degree elevation");
        assert!(elevate_linear_bezier(&mut curve, [0.0, 1.0]));
        let after = cadmpeg_ir::eval::nurbs_curve_point(
            curve.degree,
            &curve.knots,
            &curve.control_points,
            curve.weights.as_deref(),
            0.25,
        )
        .expect("valid rational quadratic NURBS evaluates after degree elevation");
        assert!(before.distance(after) <= 1.0e-12);
        assert_eq!(curve.control_points[1], Point3::new(1.5, 0.0, 0.0));
        assert_eq!(curve.weights, Some(vec![1.0, 2.0, 3.0]));
    }
}
