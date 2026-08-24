// SPDX-License-Identifier: Apache-2.0
//! Ordered composite-curve projection.

use super::curve_conversion::{circular_arc_nurbs, elliptical_arc_nurbs, parabolic_arc_nurbs};
use super::geometry::{entity_loss, source_object, WireProjectionOutcome};
use crate::directory::DirectoryEntry;
use crate::global::{Dialect, ProjectedGlobal};
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{refuse_local_limit, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    knots_nondecreasing, CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry,
    NurbsCurve, ProceduralCurve, ProceduralCurveDefinition,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, ProceduralCurveId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

const MAX_COMPOSITE_CHILDREN: usize = 100_000;
const MAX_COMPOSITE_DEGREE: usize = 1024;
const MAX_COMPOSITE_DEPTH: usize = 64;

fn composite_child_type_allowed(entity_type: i64, form: i64, dialect: Dialect) -> bool {
    if matches!(dialect, Dialect::V4_0) {
        return matches!(
            (entity_type, form),
            (100 | 110 | 116 | 132, 0) | (104 | 112, 0..=3) | (126, 0..=5)
        );
    }
    matches!(
        (entity_type, form),
        (100 | 110 | 116 | 130 | 132 | 142, 0) | (104 | 112, 0..=3) | (106, _) | (126, 0..=5)
    )
}

fn composite_use_flag_valid(use_flag: u8, dialect: Dialect) -> bool {
    match dialect {
        Dialect::V4_0 => use_flag == 0,
        _ => use_flag <= 6,
    }
}

fn degraded_carrier_loss(entry: &DirectoryEntry, reason: &str) -> LossNote {
    IgesLossCode::CompositeCarrierDegraded
        .note(format!(
            "IGES Type 102 entity D{} has no admitted concatenated carrier because {reason}; the ordered native composite carrier was retained",
            entry.sequence
        ))
        .with_provenance(entry.loss_provenance())
}

#[derive(Clone)]
struct CompositeEdge {
    start: VertexId,
    end: VertexId,
    param_range: Option<[f64; 2]>,
}

#[derive(Default)]
pub(super) struct CompositeIndex {
    curve_positions: BTreeMap<CurveId, usize>,
    edges: BTreeMap<CurveId, Vec<CompositeEdge>>,
    vertex_points: BTreeMap<VertexId, Point3>,
}

impl CompositeIndex {
    pub(super) fn from_ir(ir: &CadIr) -> Self {
        let mut curve_positions = BTreeMap::new();
        for (position, curve) in ir.model.curves.iter().enumerate() {
            curve_positions.entry(curve.id.clone()).or_insert(position);
        }
        let mut edges = BTreeMap::new();
        for edge in &ir.model.edges {
            if let Some(curve) = &edge.curve {
                edges
                    .entry(curve.clone())
                    .or_insert_with(Vec::new)
                    .push(CompositeEdge {
                        start: edge.start.clone(),
                        end: edge.end.clone(),
                        param_range: edge.param_range,
                    });
            }
        }
        let mut points = BTreeMap::new();
        for point in &ir.model.points {
            points.entry(point.id.clone()).or_insert(point.position);
        }
        let mut vertex_points = BTreeMap::<VertexId, Point3>::new();
        for vertex in &ir.model.vertices {
            if let Some(point) = points.get(&vertex.point).copied() {
                vertex_points.entry(vertex.id.clone()).or_insert(point);
            }
        }
        Self {
            curve_positions,
            edges,
            vertex_points,
        }
    }

    fn add_model_entity(
        &mut self,
        curve_id: CurveId,
        curve_index: usize,
        edge: CompositeEdge,
        endpoints: [(VertexId, Point3); 2],
    ) {
        self.curve_positions.insert(curve_id.clone(), curve_index);
        self.edges.entry(curve_id).or_default().push(edge);
        for (vertex, point) in endpoints {
            self.vertex_points.insert(vertex, point);
        }
    }
}

fn point_for_vertex(ir: &CadIr, id: &VertexId, index: Option<&CompositeIndex>) -> Option<Point3> {
    if let Some(index) = index {
        return index.vertex_points.get(id).copied();
    }
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

fn composite_edge_endpoints_agree(
    ir: &CadIr,
    index: Option<&CompositeIndex>,
    left: &CompositeEdge,
    right: &CompositeEdge,
    tolerance: f64,
) -> bool {
    match (
        point_for_vertex(ir, &left.start, index),
        point_for_vertex(ir, &right.start, index),
        point_for_vertex(ir, &left.end, index),
        point_for_vertex(ir, &right.end, index),
    ) {
        (Some(left_start), Some(right_start), Some(left_end), Some(right_end)) => {
            // GE-05: the MUR boundary is excluded; zero still means exact equality.
            close_with_tolerance(left_start, right_start, Some(tolerance))
                && close_with_tolerance(left_end, right_end, Some(tolerance))
        }
        (None, None, None, None) => true,
        _ => false,
    }
}

fn select_composite_edge(
    ir: &CadIr,
    index: Option<&CompositeIndex>,
    geometry: &CurveGeometry,
    candidates: &[CompositeEdge],
    tolerance: f64,
) -> Option<CompositeEdge> {
    let usable = candidates
        .iter()
        .filter(|edge| {
            let Some(range) = edge.param_range else {
                return false;
            };
            if !matches!(geometry, CurveGeometry::Line { .. }) {
                return true;
            }
            let (Some(start), Some(end)) = (
                point_for_vertex(ir, &edge.start, index),
                point_for_vertex(ir, &edge.end, index),
            ) else {
                return false;
            };
            let (Some(evaluated_start), Some(evaluated_end)) = (
                cadmpeg_ir::eval::curve_point(geometry, range[0]),
                cadmpeg_ir::eval::curve_point(geometry, range[1]),
            ) else {
                return false;
            };
            // GE-05: candidate admission uses the same strict MUR rule as joins.
            close_with_tolerance(evaluated_start, start, Some(tolerance))
                && close_with_tolerance(evaluated_end, end, Some(tolerance))
        })
        .collect::<Vec<_>>();
    let first = usable.first()?;
    usable
        .iter()
        .skip(1)
        .all(|candidate| {
            candidate.param_range == first.param_range
                && composite_edge_endpoints_agree(ir, index, candidate, first, tolerance)
        })
        .then(|| (*first).clone())
}

fn elevate_linear_bezier_to_degree(
    curve: &mut NurbsCurve,
    interval: [f64; 2],
    target_degree: u32,
) -> bool {
    if curve.degree != 1
        || curve.control_points.len() != 2
        || curve.knots != [interval[0], interval[0], interval[1], interval[1]]
        || target_degree < 1
        || curve
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != 2)
    {
        return false;
    }
    let target_degree = match usize::try_from(target_degree) {
        Ok(target_degree) if target_degree <= MAX_COMPOSITE_DEGREE => target_degree,
        _ => return false,
    };
    let rational = curve.weights.is_some();
    let mut homogeneous = curve
        .control_points
        .iter()
        .zip(curve.weights.as_deref().unwrap_or(&[1.0, 1.0]))
        .map(|(point, weight)| {
            (
                *weight,
                *weight * point.x,
                *weight * point.y,
                *weight * point.z,
            )
        })
        .collect::<Vec<_>>();
    if homogeneous.len() != 2
        || homogeneous.iter().any(|(weight, x, y, z)| {
            !weight.is_finite()
                || *weight <= 0.0
                || !x.is_finite()
                || !y.is_finite()
                || !z.is_finite()
        })
    {
        return false;
    }
    let mut degree = 1_usize;
    while degree < target_degree {
        let next_degree = degree + 1;
        let mut elevated = Vec::with_capacity(next_degree + 1);
        elevated.push(homogeneous[0]);
        for index in 1..=degree {
            let alpha = index as f64 / next_degree as f64;
            let previous = homogeneous[index - 1];
            let current = homogeneous[index];
            elevated.push((
                alpha * previous.0 + (1.0 - alpha) * current.0,
                alpha * previous.1 + (1.0 - alpha) * current.1,
                alpha * previous.2 + (1.0 - alpha) * current.2,
                alpha * previous.3 + (1.0 - alpha) * current.3,
            ));
        }
        let Some(last) = homogeneous.last().copied() else {
            return false;
        };
        elevated.push(last);
        homogeneous = elevated;
        degree = next_degree;
    }
    let mut control_points = Vec::with_capacity(homogeneous.len());
    for (weight, x, y, z) in &homogeneous {
        if !weight.is_finite() || *weight <= 0.0 {
            return false;
        }
        control_points.push(Point3::new(x / weight, y / weight, z / weight));
    }
    curve.degree = target_degree as u32;
    curve.knots = [
        vec![interval[0]; target_degree + 1],
        vec![interval[1]; target_degree + 1],
    ]
    .concat();
    curve.control_points = control_points;
    curve.weights = rational.then(|| homogeneous.into_iter().map(|entry| entry.0).collect());
    true
}

#[derive(Debug)]
struct ConcatenatedNurbs {
    nurbs: NurbsCurve,
    boundaries: Vec<f64>,
    child_starts: Vec<f64>,
}

fn reverse_nurbs(curve: NurbsCurve, interval: [f64; 2]) -> Option<(NurbsCurve, [f64; 2])> {
    let degree = usize::try_from(curve.degree).ok()?;
    let control_count = curve.control_points.len();
    let expected_knot_count = control_count.checked_add(degree)?.checked_add(1)?;
    if control_count == 0
        || degree >= control_count
        || curve.knots.len() != expected_knot_count
        || curve
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != control_count)
        || curve.knots.iter().any(|knot| !knot.is_finite())
        || !knots_nondecreasing(&curve.knots)
    {
        return None;
    }
    let [start, end] = interval;
    if !start.is_finite() || !end.is_finite() || start > end {
        return None;
    }
    let domain_start = curve.knots[degree];
    let domain_end = curve.knots[control_count];
    if !domain_start.is_finite()
        || !domain_end.is_finite()
        || domain_start >= domain_end
        || start < domain_start
        || end > domain_end
    {
        return None;
    }
    let sum = domain_start + domain_end;
    let reversed_range = [sum - end, sum - start];
    if !sum.is_finite()
        || reversed_range
            .iter()
            .any(|parameter| !parameter.is_finite())
    {
        return None;
    }
    let knots = curve
        .knots
        .iter()
        .rev()
        .map(|knot| sum - knot)
        .collect::<Vec<_>>();
    if knots.iter().any(|knot| !knot.is_finite()) {
        return None;
    }
    Some((
        NurbsCurve {
            degree: curve.degree,
            knots,
            control_points: curve.control_points.into_iter().rev().collect(),
            weights: curve
                .weights
                .map(|weights| weights.into_iter().rev().collect()),
            periodic: curve.periodic,
        },
        reversed_range,
    ))
}

fn elevate_linear_nurbs_to_degree(
    curve: &mut NurbsCurve,
    interval: [f64; 2],
    target_degree: u32,
    join_tolerance: Option<f64>,
) -> bool {
    if curve.degree != 1
        || curve.control_points.len() < 2
        || curve.knots.len() != curve.control_points.len() + 2
        || curve.knots.first() != Some(&interval[0])
        || curve.knots.last() != Some(&interval[1])
        || interval[0] >= interval[1]
        || target_degree <= 1
        || curve
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != curve.control_points.len())
    {
        return false;
    }
    if curve.knots.windows(2).any(|pair| pair[0] > pair[1]) {
        return false;
    }
    let mut pieces = Vec::new();
    for span in 1..curve.control_points.len() {
        let start = curve.knots[span];
        let end = curve.knots[span + 1];
        if !start.is_finite() || !end.is_finite() || start >= end {
            continue;
        }
        let mut piece = NurbsCurve {
            degree: 1,
            knots: vec![start, start, end, end],
            control_points: vec![curve.control_points[span - 1], curve.control_points[span]],
            weights: curve
                .weights
                .as_ref()
                .map(|weights| vec![weights[span - 1], weights[span]]),
            periodic: false,
        };
        if !elevate_linear_bezier_to_degree(&mut piece, [start, end], target_degree) {
            return false;
        }
        pieces.push((piece, [start, end]));
    }
    let Some(concatenated) = concatenate_nurbs(pieces, join_tolerance) else {
        return false;
    };
    curve.degree = concatenated.nurbs.degree;
    curve.knots = concatenated
        .nurbs
        .knots
        .into_iter()
        .map(|knot| knot + interval[0])
        .collect();
    curve.control_points = concatenated.nurbs.control_points;
    curve.weights = concatenated.nurbs.weights;
    curve.periodic = false;
    true
}

fn concatenate_nurbs(
    mut children: Vec<(NurbsCurve, [f64; 2])>,
    join_tolerance: Option<f64>,
) -> Option<ConcatenatedNurbs> {
    if children.is_empty() {
        return None;
    }
    let degree = children
        .iter()
        .map(|(curve, _)| curve.degree)
        .max()
        .unwrap_or_default();
    for (curve, interval) in &mut children {
        if curve.degree < degree
            && (curve.degree != 1
                || !elevate_linear_nurbs_to_degree(curve, *interval, degree, join_tolerance))
        {
            return None;
        }
    }
    if children.iter().any(|(curve, interval)| {
        let Some(first) = curve.knots.first() else {
            return true;
        };
        let Some(last) = curve.knots.last() else {
            return true;
        };
        curve.degree != degree
            || interval != &[*first, *last]
            || interval[0] >= interval[1]
            || curve.control_points.is_empty()
            || curve.knots.len() != curve.control_points.len() + degree as usize + 1
            || curve
                .weights
                .as_ref()
                .is_some_and(|weights| weights.len() != curve.control_points.len())
    }) {
        return None;
    }
    let degree_usize = degree as usize;
    let mut knots = Vec::new();
    let mut control_points = Vec::new();
    let mut weights = Vec::new();
    let mut boundaries = vec![0.0];
    let mut child_starts = Vec::with_capacity(children.len());
    let mut cursor = 0.0;
    for (child_index, (curve, interval)) in children.into_iter().enumerate() {
        let child_start = interval[0];
        let child_end = interval[1];
        let shifted_knots = curve
            .knots
            .iter()
            .map(|knot| (knot - child_start) + cursor)
            .collect::<Vec<_>>();
        let mut child_weights = curve
            .weights
            .unwrap_or_else(|| vec![1.0; curve.control_points.len()]);
        if child_weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.0)
        {
            return None;
        }
        if child_index == 0 {
            knots = shifted_knots;
            control_points = curve.control_points;
            weights = child_weights;
        } else {
            if !close_with_tolerance(
                control_points[control_points.len() - 1],
                curve.control_points[0],
                join_tolerance,
            ) {
                return None;
            }
            let scale = weights[weights.len() - 1] / child_weights[0];
            if !scale.is_finite() || scale <= 0.0 {
                return None;
            }
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
        if !cursor.is_finite() {
            return None;
        }
        boundaries.push(cursor);
    }
    if knots.len() != control_points.len() + degree_usize + 1 {
        return None;
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
    cadmpeg_ir::eval::nurbs_curve_point(
        degree,
        &nurbs.knots,
        &nurbs.control_points,
        nurbs.weights.as_deref(),
        0.0,
    )?;
    cadmpeg_ir::eval::nurbs_curve_point(
        degree,
        &nurbs.knots,
        &nurbs.control_points,
        nurbs.weights.as_deref(),
        cursor,
    )?;
    Some(ConcatenatedNurbs {
        nurbs,
        boundaries,
        child_starts,
    })
}

fn bounded_nurbs_for_id(
    ir: &CadIr,
    curve_id: &CurveId,
    depth: usize,
    join_tolerance: Option<f64>,
    ctx: Option<&DecodeContext<'_>>,
    index: Option<&CompositeIndex>,
) -> Option<(NurbsCurve, [f64; 2])> {
    let _nested = ctx
        .map(|ctx| ctx.enter_nested("iges_composite_flatten", None))
        .transpose()
        .ok()?;
    let depth_limit = ctx
        .and_then(|ctx| usize::try_from(ctx.policy().limits.max_recursion_depth).ok())
        .map_or(MAX_COMPOSITE_DEPTH, |policy| {
            policy.min(MAX_COMPOSITE_DEPTH)
        });
    if depth >= depth_limit {
        if let Some(ctx) = ctx {
            let _ = ctx.refuse_codec_limit(
                "iges_composite_depth",
                depth_limit as u64,
                depth.saturating_add(1) as u64,
                None,
            );
        }
        return None;
    }
    let curve = match index {
        Some(index) => index
            .curve_positions
            .get(curve_id)
            .and_then(|position| ir.model.curves.get(*position))?,
        None => ir.model.curves.iter().find(|curve| curve.id == *curve_id)?,
    };
    if let CurveGeometry::Composite { segments, .. } = &curve.geometry {
        let children = segments
            .iter()
            .map(|segment| {
                let child = bounded_nurbs_for_id(
                    ir,
                    &segment.curve,
                    depth + 1,
                    join_tolerance,
                    ctx,
                    index,
                )?;
                if segment.same_sense {
                    Some(child)
                } else {
                    reverse_nurbs(child.0, child.1)
                }
            })
            .collect::<Option<Vec<_>>>()?;
        let concatenated = concatenate_nurbs(children, join_tolerance)?;
        let range = [0.0, *concatenated.boundaries.last()?];
        return Some((concatenated.nurbs, range));
    }
    let edge_candidates: Cow<'_, [CompositeEdge]> = match index {
        Some(index) => Cow::Borrowed(index.edges.get(curve_id).map_or(&[][..], Vec::as_slice)),
        None => Cow::Owned(
            ir.model
                .edges
                .iter()
                .filter(|edge| edge.curve.as_ref() == Some(curve_id))
                .map(|edge| CompositeEdge {
                    start: edge.start.clone(),
                    end: edge.end.clone(),
                    param_range: edge.param_range,
                })
                .collect(),
        ),
    };
    let edge = select_composite_edge(
        ir,
        index,
        &curve.geometry,
        &edge_candidates,
        join_tolerance.unwrap_or(0.0),
    )?;
    let interval = edge.param_range?;
    match &curve.geometry {
        CurveGeometry::Nurbs(nurbs) => Some((nurbs.clone(), interval)),
        CurveGeometry::Line { .. } => Some((
            NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![
                    point_for_vertex(ir, &edge.start, index)?,
                    point_for_vertex(ir, &edge.end, index)?,
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
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => Some((
            elliptical_arc_nurbs(
                *center,
                *axis,
                *major_direction,
                *major_radius,
                *minor_radius,
                interval,
            )?,
            interval,
        )),
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => Some((
            parabolic_arc_nurbs(*vertex, *axis, *major_direction, *focal_distance, interval)?,
            interval,
        )),
        _ => None,
    }
}

fn bounded_nurbs(
    ir: &CadIr,
    index: &CompositeIndex,
    sequence: u32,
    join_tolerance: f64,
    ctx: Option<&DecodeContext<'_>>,
) -> Option<(NurbsCurve, [f64; 2])> {
    let curve_id = CurveId(format!("iges:model:curve#D{sequence}"));
    bounded_nurbs_for_id(ir, &curve_id, 0, Some(join_tolerance), ctx, Some(index))
}

pub(super) fn bounded_nurbs_for_curve(
    ir: &CadIr,
    curve_id: &CurveId,
    ctx: Option<&DecodeContext<'_>>,
    index: Option<&CompositeIndex>,
) -> Option<(NurbsCurve, [f64; 2])> {
    bounded_nurbs_for_id(ir, curve_id, 0, None, ctx, index)
}

pub(super) fn bounded_nurbs_for_curve_with_tolerance(
    ir: &CadIr,
    curve_id: &CurveId,
    tolerance: Option<f64>,
    ctx: Option<&DecodeContext<'_>>,
    index: Option<&CompositeIndex>,
) -> Option<(NurbsCurve, [f64; 2])> {
    bounded_nurbs_for_id(
        ir,
        curve_id,
        0,
        tolerance.filter(|tolerance| tolerance.is_finite() && *tolerance >= 0.0),
        ctx,
        index,
    )
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

fn close_with_tolerance(left: Point3, right: Point3, tolerance: Option<f64>) -> bool {
    match tolerance {
        Some(tolerance) if tolerance.is_finite() && tolerance >= 0.0 => {
            let distance = left.distance(right);
            // GE-05: IGES MUR coincidence is strictly less than the declared value.
            if tolerance == 0.0 {
                distance == 0.0
            } else {
                distance < tolerance
            }
        }
        _ => close(left, right),
    }
}

fn curve_endpoints(
    ir: &CadIr,
    curve_id: &CurveId,
    index: &CompositeIndex,
    tolerance: f64,
) -> Option<(Point3, Point3)> {
    let curve_position = index.curve_positions.get(curve_id)?;
    let curve = ir.model.curves.get(*curve_position)?;
    let candidates = index.edges.get(curve_id)?;
    let edge = select_composite_edge(ir, Some(index), &curve.geometry, candidates, tolerance)?;
    Some((
        point_for_vertex(ir, &edge.start, Some(index))?,
        point_for_vertex(ir, &edge.end, Some(index))?,
    ))
}

fn project_native_composite(
    ir: &mut CadIr,
    index: &mut CompositeIndex,
    entry: &DirectoryEntry,
    child_sequences: &[u32],
    join_tolerance: f64,
) -> Option<EdgeId> {
    let child_curves = child_sequences
        .iter()
        .map(|sequence| CurveId(format!("iges:model:curve#D{sequence}")))
        .collect::<Vec<_>>();
    if child_curves
        .iter()
        .any(|curve_id| !index.curve_positions.contains_key(curve_id))
    {
        return None;
    }
    let endpoints = child_curves
        .iter()
        .map(|curve_id| curve_endpoints(ir, curve_id, index, join_tolerance))
        .collect::<Option<Vec<_>>>()?;
    let start = endpoints.first()?.0;
    let end = endpoints.last()?.1;
    let segments = child_curves
        .iter()
        .enumerate()
        .map(|(index, curve)| CompositeCurveSegment {
            curve: curve.clone(),
            same_sense: true,
            transition: if index > 0
                && close_with_tolerance(
                    endpoints[index - 1].1,
                    endpoints[index].0,
                    Some(join_tolerance),
                ) {
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
        curve: Some(curve_id.clone()),
        start: start_vertex.clone(),
        end: end_vertex.clone(),
        param_range: None,
        tolerance: None,
    });
    index.add_model_entity(
        curve_id.clone(),
        ir.model.curves.len() - 1,
        CompositeEdge {
            start: start_vertex.clone(),
            end: end_vertex.clone(),
            param_range: None,
        },
        [(start_vertex, start), (end_vertex, end)],
    );
    Some(edge_id)
}

fn project_degraded_composite(
    ir: &mut CadIr,
    index: &mut CompositeIndex,
    entry: &DirectoryEntry,
    child_sequences: &[u32],
    join_tolerance: f64,
    reason: &str,
    losses: &mut Vec<LossNote>,
) -> Option<EdgeId> {
    let edge = project_native_composite(ir, index, entry, child_sequences, join_tolerance);
    if edge.is_some() {
        losses.push(degraded_carrier_loss(entry, reason));
    } else {
        losses.push(entity_loss(
            entry,
            format!("{reason}, and no ordered native composite carrier can be constructed"),
        ));
    }
    edge
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
    let mut index = CompositeIndex::from_ir(ir);
    let join_tolerance = global.minimum_resolution_mm();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 102 && entry.form == 0)
    {
        if !composite_use_flag_valid(entry.status.use_flag, global.dialect()) {
            losses.push(entity_loss(
                entry,
                "Type 102 Entity Use Flag must be 00 in IGES 4.0",
            ));
            continue;
        }
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(raw_child_count) = record.integer(1) else {
            losses.push(entity_loss(entry, "child count is invalid"));
            continue;
        };
        if raw_child_count > MAX_COMPOSITE_CHILDREN as i64 {
            return Err(refuse_local_limit(
                "iges_composite_children",
                MAX_COMPOSITE_CHILDREN as u64,
                u64::try_from(raw_child_count).unwrap_or(u64::MAX),
                None,
            ));
        }
        let Some(child_count) = usize::try_from(raw_child_count)
            .ok()
            .filter(|count| *count > 0)
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
            entries.get(sequence).is_none_or(|child| {
                !composite_child_type_allowed(child.entity_type, child.form, global.dialect())
                    || !child.status.is_physically_dependent()
            })
        }) {
            losses.push(entity_loss(
                entry,
                "composite child is missing, outside the declared dialect, or is not physically dependent",
            ));
            continue;
        }
        let Some(children) = child_sequences
            .iter()
            .map(|sequence| bounded_nurbs(ir, &index, *sequence, join_tolerance, ctx))
            .collect::<Option<Vec<_>>>()
        else {
            if let Some(edge) = project_degraded_composite(
                ir,
                &mut index,
                entry,
                &child_sequences,
                join_tolerance,
                "a child has no bounded line or NURBS carrier",
                &mut losses,
            ) {
                wire_edges.push(edge);
                decoded.insert(entry.sequence);
                continue;
            }
            continue;
        };
        let Some(ConcatenatedNurbs {
            nurbs,
            boundaries,
            child_starts,
        }) = concatenate_nurbs(children, Some(join_tolerance))
        else {
            if let Some(edge) = project_degraded_composite(
                ir,
                &mut index,
                entry,
                &child_sequences,
                join_tolerance,
                "child endpoints do not join within the Global minimum resolution",
                &mut losses,
            ) {
                wire_edges.push(edge);
                decoded.insert(entry.sequence);
                continue;
            }
            continue;
        };
        let degree = nurbs.degree;
        let Some(cursor) = boundaries.last().copied() else {
            if let Some(edge) = project_degraded_composite(
                ir,
                &mut index,
                entry,
                &child_sequences,
                join_tolerance,
                "its parameter range is empty",
                &mut losses,
            ) {
                wire_edges.push(edge);
                decoded.insert(entry.sequence);
                continue;
            }
            continue;
        };
        let Some(start) = cadmpeg_ir::eval::nurbs_curve_point(
            degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            0.0,
        ) else {
            if let Some(edge) = project_degraded_composite(
                ir,
                &mut index,
                entry,
                &child_sequences,
                join_tolerance,
                "its start cannot be evaluated",
                &mut losses,
            ) {
                wire_edges.push(edge);
                decoded.insert(entry.sequence);
                continue;
            }
            continue;
        };
        let Some(end) = cadmpeg_ir::eval::nurbs_curve_point(
            degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            cursor,
        ) else {
            if let Some(edge) = project_degraded_composite(
                ir,
                &mut index,
                entry,
                &child_sequences,
                join_tolerance,
                "its end cannot be evaluated",
                &mut losses,
            ) {
                wire_edges.push(edge);
                decoded.insert(entry.sequence);
                continue;
            }
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
            start: start_vertex.clone(),
            end: end_vertex.clone(),
            param_range: Some([0.0, cursor]),
            tolerance: None,
        });
        index.add_model_entity(
            curve_id.clone(),
            ir.model.curves.len() - 1,
            CompositeEdge {
                start: start_vertex.clone(),
                end: end_vertex.clone(),
                param_range: Some([0.0, cursor]),
            },
            [(start_vertex, start), (end_vertex, end)],
        );
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

    Ok(WireProjectionOutcome {
        decoded,
        losses,
        wire_edges,
    })
}

#[cfg(test)]
mod tests;
