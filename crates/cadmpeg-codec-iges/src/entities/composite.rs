// SPDX-License-Identifier: Apache-2.0
//! Ordered composite-curve projection.

use super::curve_conversion::{circular_arc_nurbs, elliptical_arc_nurbs, parabolic_arc_nurbs};
use super::geometry::{entity_loss, resolve_transform, source_object, WireProjectionOutcome};
use crate::directory::DirectoryEntry;
use crate::global::{GlobalTable, ProjectedGlobal};
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{alloc_filled, refuse_local_limit, DecodeContext};
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

const EPS_COMPOSITE_DEGENERATE: f64 = 1.0e-10;

const MAX_COMPOSITE_CHILDREN: usize = 100_000;
const MAX_COMPOSITE_DEGREE: usize = 1024;
const MAX_COMPOSITE_DEPTH: usize = 64;

fn composite_minimum_child_count(global_table: GlobalTable) -> usize {
    if matches!(global_table, GlobalTable::V4_0) {
        2
    } else {
        1
    }
}

fn composite_child_type_allowed(entity_type: i64, form: i64, global_table: GlobalTable) -> bool {
    if matches!(global_table, GlobalTable::V4_0) {
        return matches!(
            (entity_type, form),
            (100 | 110 | 112 | 116 | 132, 0) | (104, 0..=3) | (126, 0..=5)
        );
    }
    matches!(
        (entity_type, form),
        (100 | 110 | 112 | 116 | 130 | 132 | 142, 0) | (104, 0..=3) | (106, _) | (126, 0..=5)
    )
}

fn composite_use_flag_valid(use_flag: u8, global_table: GlobalTable) -> bool {
    match global_table {
        GlobalTable::V4_0 => use_flag == 0,
        _ => use_flag <= 6,
    }
}

fn composite_line_font_valid(line_font: i64, hierarchy: u8, global_table: GlobalTable) -> bool {
    !matches!(global_table, GlobalTable::V4_0) || hierarchy == 1 || line_font != 0
}

fn composite_logical_connector_use_valid(
    use_flag: u8,
    is_logical_connector: bool,
    global_table: GlobalTable,
) -> bool {
    !is_logical_connector
        || !matches!(
            global_table,
            GlobalTable::V5_0 | GlobalTable::V5_1 | GlobalTable::V5_2 | GlobalTable::V5_3
        )
        || use_flag == 4
}

fn composite_point_member(entry: &DirectoryEntry) -> bool {
    matches!(entry.entity_type, 116 | 132) && entry.form == 0
}

struct CompositePointContext<'map, 'directory, 'parameter, 'decode> {
    entries: &'map BTreeMap<u32, &'directory DirectoryEntry>,
    records: &'map BTreeMap<u32, &'parameter ParameterRecord>,
    global: &'map ProjectedGlobal,
    ctx: Option<&'map DecodeContext<'decode>>,
    tolerance: f64,
}

impl CompositePointContext<'_, '_, '_, '_> {
    fn is_point(&self, sequence: u32) -> bool {
        self.entries
            .get(&sequence)
            .is_some_and(|entry| composite_point_member(entry))
    }

    fn member_point(&self, sequence: u32) -> Option<Point3> {
        let entry = self.entries.get(&sequence).copied()?;
        if !composite_point_member(entry) {
            return None;
        }
        let record = self.records.get(&sequence).copied()?;
        let [x, y, z] = [record.number(1)?, record.number(2)?, record.number(3)?];
        let transform = resolve_transform(
            entry.transform,
            self.entries,
            self.records,
            self.global.length_factor_mm(),
            self.global.real_precision(),
            &mut BTreeSet::new(),
            self.ctx,
        )
        .ok()?;
        let point = transform.point(Point3::new(
            x * self.global.length_factor_mm(),
            y * self.global.length_factor_mm(),
            z * self.global.length_factor_mm(),
        ));
        (point.x.is_finite() && point.y.is_finite() && point.z.is_finite()).then_some(point)
    }
}

fn composite_point_adjacency_valid(
    ir: &CadIr,
    index: &CompositeIndex,
    child_sequences: &[u32],
    curve_carriers: &BTreeMap<u32, CurveId>,
    context: &CompositePointContext<'_, '_, '_, '_>,
) -> bool {
    let all_points = child_sequences
        .iter()
        .copied()
        .all(|sequence| context.is_point(sequence));
    if child_sequences
        .windows(2)
        .any(|pair| context.is_point(pair[0]) && context.is_point(pair[1]))
        && !(all_points && child_sequences.len() == 2)
    {
        return false;
    }
    for (position, sequence) in child_sequences.iter().enumerate() {
        if !context.is_point(*sequence) {
            continue;
        }
        let Some(point) = context.member_point(*sequence) else {
            return false;
        };
        if position > 0 && !context.is_point(child_sequences[position - 1]) {
            let Some(curve_id) = curve_carriers.get(&child_sequences[position - 1]) else {
                return false;
            };
            let Some((_, end)) = curve_endpoints(ir, curve_id, index, context.tolerance) else {
                return false;
            };
            if !close_with_tolerance(end, point, Some(context.tolerance)) {
                return false;
            }
        }
        if position + 1 < child_sequences.len() && !context.is_point(child_sequences[position + 1])
        {
            let Some(curve_id) = curve_carriers.get(&child_sequences[position + 1]) else {
                return false;
            };
            let Some((start, _)) = curve_endpoints(ir, curve_id, index, context.tolerance) else {
                return false;
            };
            if !close_with_tolerance(point, start, Some(context.tolerance)) {
                return false;
            }
        }
    }
    true
}

pub(super) fn curve_carrier_id(
    sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> Option<CurveId> {
    let entry = entries.get(&sequence).copied()?;
    let carrier_sequence = if entry.entity_type == 142 && entry.form == 0 {
        // Type 142 is a relationship entity. In a Type 102 constituent its
        // curve geometry is the model-space C pointer; the UV B pointer is
        // not a three-dimensional composite segment. This is the same
        // choice made by OCCT's Curve3D transfer path.
        records
            .get(&sequence)
            .and_then(|record| record.integer(4))
            .and_then(|value| {
                let sequence = u32::try_from(value).ok()?;
                (sequence % 2 == 1).then_some(sequence)
            })?
    } else {
        sequence
    };
    Some(CurveId(format!("iges:model:curve#D{carrier_sequence}")))
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

    pub(super) fn curve_by_id<'a>(&self, ir: &'a CadIr, curve_id: &CurveId) -> Option<&'a Curve> {
        self.curve_positions
            .get(curve_id)
            .and_then(|position| ir.model.curves.get(*position))
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

fn homogeneous_point_is_valid(point: &[f64; 4]) -> bool {
    point.iter().all(|value| value.is_finite()) && point[0] > 0.0
}

fn homogeneous_control_points(curve: &NurbsCurve) -> Option<Vec<[f64; 4]>> {
    let control_count = curve.control_points.len();
    if curve
        .weights
        .as_ref()
        .is_some_and(|weights| weights.len() != control_count)
    {
        return None;
    }
    let mut homogeneous = alloc_filled(
        control_count,
        [0.0; 4],
        "iges composite homogeneous control points",
    )
    .ok()?;
    for (index, point) in curve.control_points.iter().enumerate() {
        let weight = curve
            .weights
            .as_ref()
            .map_or(Some(1.0), |weights| weights.get(index).copied())?;
        let homogeneous_point = [weight, weight * point.x, weight * point.y, weight * point.z];
        if !homogeneous_point_is_valid(&homogeneous_point) {
            return None;
        }
        homogeneous[index] = homogeneous_point;
    }
    Some(homogeneous)
}

fn euclidean_control_points(
    homogeneous: Vec<[f64; 4]>,
    rational: bool,
) -> Option<(Vec<Point3>, Option<Vec<f64>>)> {
    let mut control_points = Vec::with_capacity(homogeneous.len());
    let mut weights = rational.then(|| Vec::with_capacity(homogeneous.len()));
    for [weight, x, y, z] in homogeneous {
        if !weight.is_finite() || weight <= 0.0 {
            return None;
        }
        let point = Point3::new(x / weight, y / weight, z / weight);
        if !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite() {
            return None;
        }
        control_points.push(point);
        if let Some(weights) = &mut weights {
            weights.push(weight);
        }
    }
    Some((control_points, weights))
}

fn elevate_bezier_homogeneous(
    control_points: &[[f64; 4]],
    source_degree: usize,
    target_degree: usize,
) -> Option<Vec<[f64; 4]>> {
    if control_points.len() != source_degree.checked_add(1)? || target_degree < source_degree {
        return None;
    }
    if control_points
        .iter()
        .any(|point| !homogeneous_point_is_valid(point))
    {
        return None;
    }
    let mut elevated = control_points.to_vec();
    let mut degree = source_degree;
    while degree < target_degree {
        let next_degree = degree.checked_add(1)?;
        let mut next = Vec::with_capacity(next_degree.checked_add(1)?);
        next.push(elevated[0]);
        for index in 1..=degree {
            let alpha = index as f64 / next_degree as f64;
            let previous = elevated[index - 1];
            let current = elevated[index];
            let point = [
                alpha * previous[0] + (1.0 - alpha) * current[0],
                alpha * previous[1] + (1.0 - alpha) * current[1],
                alpha * previous[2] + (1.0 - alpha) * current[2],
                alpha * previous[3] + (1.0 - alpha) * current[3],
            ];
            if !homogeneous_point_is_valid(&point) {
                return None;
            }
            next.push(point);
        }
        next.push(*elevated.last()?);
        elevated = next;
        degree = next_degree;
    }
    Some(elevated)
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

fn insert_homogeneous_knot(
    control_points: &[[f64; 4]],
    knots: &[f64],
    degree: usize,
    value: f64,
) -> Option<(Vec<[f64; 4]>, Vec<f64>)> {
    let control_count = control_points.len();
    let last_control = control_count.checked_sub(1)?;
    let span = knots.iter().rposition(|knot| *knot <= value)?;
    let span = if degree == 0 {
        span.min(last_control)
    } else {
        span
    };
    let multiplicity = knots.iter().filter(|knot| **knot == value).count();
    let left_end = span.checked_sub(degree)?;
    if control_count <= degree
        || left_end > last_control
        || multiplicity > degree
        || span < degree
        || knots.len() != control_count.checked_add(degree)?.checked_add(1)?
    {
        return None;
    }
    let mut inserted_knots = Vec::new();
    inserted_knots
        .try_reserve_exact(knots.len().checked_add(1)?)
        .ok()?;
    inserted_knots.extend_from_slice(knots.get(..=span)?);
    inserted_knots.push(value);
    inserted_knots.extend_from_slice(knots.get(span.checked_add(1)?..)?);

    let inserted_count = control_count.checked_add(1)?;
    let mut inserted_control_points = alloc_filled(
        inserted_count,
        [0.0; 4],
        "iges composite knot-insertion control points",
    )
    .ok()?;
    let tail_start = span.checked_sub(multiplicity)?;
    if tail_start > last_control {
        return None;
    }
    inserted_control_points[..=left_end].copy_from_slice(&control_points[..=left_end]);
    inserted_control_points[tail_start + 1..]
        .copy_from_slice(&control_points[tail_start..control_count]);
    for index in left_end.checked_add(1)?..=tail_start {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return None;
        }
        let alpha = (value - knots[index]) / denominator;
        if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
            return None;
        }
        let previous = control_points[index - 1];
        let current = control_points[index];
        let point = [
            alpha * current[0] + (1.0 - alpha) * previous[0],
            alpha * current[1] + (1.0 - alpha) * previous[1],
            alpha * current[2] + (1.0 - alpha) * previous[2],
            alpha * current[3] + (1.0 - alpha) * previous[3],
        ];
        if !homogeneous_point_is_valid(&point) {
            return None;
        }
        inserted_control_points[index] = point;
    }
    Some((inserted_control_points, inserted_knots))
}

fn trim_nurbs_to_interval(curve: &NurbsCurve, interval: [f64; 2]) -> Option<NurbsCurve> {
    let degree = usize::try_from(curve.degree).ok()?;
    let control_count = curve.control_points.len();
    let expected_knot_count = control_count.checked_add(degree)?.checked_add(1)?;
    if curve.periodic
        || control_count == 0
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
    if !start.is_finite() || !end.is_finite() || start >= end {
        return None;
    }
    let domain_start = *curve.knots.get(degree)?;
    let domain_end = *curve.knots.get(control_count)?;
    if !domain_start.is_finite()
        || !domain_end.is_finite()
        || domain_start >= domain_end
        || start < domain_start
        || end > domain_end
    {
        return None;
    }
    let mut homogeneous = homogeneous_control_points(curve)?;
    let mut knots = curve.knots.clone();
    for value in [start, end] {
        let target_multiplicity = degree.checked_add(1)?;
        while knots.iter().filter(|knot| **knot == value).count() < target_multiplicity {
            let (new_homogeneous, new_knots) =
                insert_homogeneous_knot(&homogeneous, &knots, degree, value)?;
            homogeneous = new_homogeneous;
            knots = new_knots;
        }
    }
    let start_knot = knots.iter().position(|knot| *knot == start)?;
    let end_knot = knots.iter().rposition(|knot| *knot == end)?;
    let control_end = end_knot.checked_sub(degree)?;
    if start_knot >= control_end {
        return None;
    }
    let trimmed_homogeneous = homogeneous.get(start_knot..control_end)?.to_vec();
    let trimmed_knots = knots.get(start_knot..=end_knot)?.to_vec();
    if trimmed_knots.len()
        != trimmed_homogeneous
            .len()
            .checked_add(degree)?
            .checked_add(1)?
    {
        return None;
    }
    let (control_points, weights) =
        euclidean_control_points(trimmed_homogeneous, curve.weights.is_some())?;
    Some(NurbsCurve {
        degree: curve.degree,
        knots: trimmed_knots,
        control_points,
        weights,
        periodic: false,
    })
}

fn elevate_nurbs_to_degree(
    curve: &mut NurbsCurve,
    interval: [f64; 2],
    target_degree: u32,
    join_tolerance: Option<f64>,
) -> bool {
    let Ok(source_degree) = usize::try_from(curve.degree) else {
        return false;
    };
    let target_degree = match usize::try_from(target_degree) {
        Ok(target_degree) if target_degree <= MAX_COMPOSITE_DEGREE => target_degree,
        _ => return false,
    };
    if target_degree < source_degree {
        return false;
    }
    if target_degree == source_degree {
        return true;
    }
    let control_count = curve.control_points.len();
    let Some(expected_knot_count) = control_count
        .checked_add(source_degree)
        .and_then(|value| value.checked_add(1))
    else {
        return false;
    };
    if curve.periodic
        || control_count <= source_degree
        || curve.knots.len() != expected_knot_count
        || curve.knots.first() != Some(&interval[0])
        || curve.knots.last() != Some(&interval[1])
        || !interval[0].is_finite()
        || !interval[1].is_finite()
        || interval[0] >= interval[1]
        || curve.knots.iter().any(|knot| !knot.is_finite())
        || !knots_nondecreasing(&curve.knots)
        || curve
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != control_count)
    {
        return false;
    }
    let boundary_multiplicity =
        |value: f64| curve.knots.iter().filter(|knot| **knot == value).count();
    if boundary_multiplicity(interval[0]) != source_degree + 1
        || boundary_multiplicity(interval[1]) != source_degree + 1
    {
        return false;
    }
    let mut homogeneous = homogeneous_control_points(curve).and_then(|points| {
        points
            .iter()
            .all(homogeneous_point_is_valid)
            .then_some(points)
    });
    let mut knots = curve.knots.clone();
    let mut internal_values = Vec::new();
    for &knot in &knots {
        if knot > interval[0] && knot < interval[1] && internal_values.last().copied() != Some(knot)
        {
            internal_values.push(knot);
        }
    }
    for value in internal_values {
        let multiplicity = knots.iter().filter(|knot| **knot == value).count();
        if multiplicity > source_degree + 1 {
            return false;
        }
        for _ in multiplicity..source_degree {
            let Some(points) = homogeneous.take() else {
                return false;
            };
            let Some((new_points, new_knots)) =
                insert_homogeneous_knot(&points, &knots, source_degree, value)
            else {
                return false;
            };
            homogeneous = Some(new_points);
            knots = new_knots;
        }
    }
    let Some(homogeneous) = homogeneous else {
        return false;
    };
    let Some(refined_count) = homogeneous.len().checked_sub(1) else {
        return false;
    };
    let Some(refined_knot_count) = refined_count
        .checked_add(source_degree)
        .and_then(|value| value.checked_add(2))
    else {
        return false;
    };
    if knots.len() != refined_knot_count
        || knots.first() != Some(&interval[0])
        || knots.last() != Some(&interval[1])
    {
        return false;
    }
    let rational = curve.weights.is_some();
    let mut pieces = Vec::new();
    for span in source_degree..=refined_count {
        let start = knots[span];
        let end = knots[span + 1];
        if !start.is_finite() || !end.is_finite() || start >= end {
            continue;
        }
        let Some(source_points) = homogeneous.get(span - source_degree..=span) else {
            return false;
        };
        let Some(elevated) =
            elevate_bezier_homogeneous(source_points, source_degree, target_degree)
        else {
            return false;
        };
        let Some((control_points, weights)) = euclidean_control_points(elevated, rational) else {
            return false;
        };
        let Some(target_knot_count) = target_degree.checked_add(1) else {
            return false;
        };
        let Ok(mut piece_knots) =
            alloc_filled(target_knot_count, start, "iges composite elevated knots")
        else {
            return false;
        };
        let Ok(end_knots) = alloc_filled(target_knot_count, end, "iges composite elevated knots")
        else {
            return false;
        };
        piece_knots.extend(end_knots);
        let piece = NurbsCurve {
            degree: target_degree as u32,
            knots: piece_knots,
            control_points,
            weights,
            periodic: false,
        };
        pieces.push((piece, [start, end]));
    }
    let Some(concatenated) = concatenate_nurbs(pieces, join_tolerance) else {
        return false;
    };
    curve.degree = concatenated.nurbs.degree;
    let mut elevated_knots: Vec<f64> = concatenated
        .nurbs
        .knots
        .into_iter()
        .map(|knot| knot + interval[0])
        .collect();
    if let Some(first) = elevated_knots.first_mut() {
        *first = interval[0];
    }
    if let Some(last) = elevated_knots.last_mut() {
        *last = interval[1];
    }
    curve.knots = elevated_knots;
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
            && !elevate_nurbs_to_degree(curve, *interval, degree, join_tolerance)
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
        let mut child_weights = match curve.weights {
            Some(weights) => weights,
            None => alloc_filled(
                curve.control_points.len(),
                1.0,
                "iges composite child weights",
            )
            .ok()?,
        };
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
            if degree_usize == 0 {
                knots.extend_from_slice(&shifted_knots[1..]);
                control_points.extend_from_slice(&curve.control_points);
                weights.extend_from_slice(&child_weights);
            } else {
                knots.pop();
                knots.extend_from_slice(&shifted_knots[degree_usize + 1..]);
                control_points.extend_from_slice(&curve.control_points[1..]);
                weights.extend_from_slice(&child_weights[1..]);
            }
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

fn bounded_edge_for_curve(
    ir: &CadIr,
    curve_id: &CurveId,
    tolerance: f64,
    index: Option<&CompositeIndex>,
) -> Option<CompositeEdge> {
    let curve = match index {
        Some(index) => index
            .curve_positions
            .get(curve_id)
            .and_then(|position| ir.model.curves.get(*position))?,
        None => ir.model.curves.iter().find(|curve| curve.id == *curve_id)?,
    };
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
    select_composite_edge(ir, index, &curve.geometry, &edge_candidates, tolerance)
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
    let edge = bounded_edge_for_curve(ir, curve_id, join_tolerance.unwrap_or(0.0), index)?;
    let interval = edge.param_range?;
    match &curve.geometry {
        CurveGeometry::Nurbs(nurbs) => Some((trim_nurbs_to_interval(nurbs, interval)?, interval)),
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
        } => {
            let mut nurbs = circular_arc_nurbs(*center, *axis, *ref_direction, *radius, interval)?;
            anchor_analytic_nurbs_endpoint_poles(
                &mut nurbs,
                interval,
                ir,
                index,
                &edge,
                join_tolerance,
            )?;
            Some((nurbs, interval))
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let mut nurbs = elliptical_arc_nurbs(
                *center,
                *axis,
                *major_direction,
                *major_radius,
                *minor_radius,
                interval,
            )?;
            anchor_analytic_nurbs_endpoint_poles(
                &mut nurbs,
                interval,
                ir,
                index,
                &edge,
                join_tolerance,
            )?;
            Some((nurbs, interval))
        }
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => {
            let mut nurbs =
                parabolic_arc_nurbs(*vertex, *axis, *major_direction, *focal_distance, interval)?;
            anchor_analytic_nurbs_endpoint_poles(
                &mut nurbs,
                interval,
                ir,
                index,
                &edge,
                join_tolerance,
            )?;
            Some((nurbs, interval))
        }
        _ => None,
    }
}

fn bounded_nurbs(
    ir: &CadIr,
    index: &CompositeIndex,
    curve_id: &CurveId,
    join_tolerance: f64,
    ctx: Option<&DecodeContext<'_>>,
) -> Option<(NurbsCurve, [f64; 2])> {
    bounded_nurbs_for_id(ir, curve_id, 0, Some(join_tolerance), ctx, Some(index))
}

pub(super) fn bounded_nurbs_for_curve(
    ir: &CadIr,
    curve_id: &CurveId,
    ctx: Option<&DecodeContext<'_>>,
    index: Option<&CompositeIndex>,
) -> Option<(NurbsCurve, [f64; 2])> {
    bounded_nurbs_for_id(ir, curve_id, 0, None, ctx, index)
}

pub(super) fn bounded_parameter_range_for_curve(
    ir: &CadIr,
    curve_id: &CurveId,
    tolerance: f64,
    index: Option<&CompositeIndex>,
) -> Option<[f64; 2]> {
    bounded_edge_for_curve(ir, curve_id, tolerance, index)?.param_range
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
    (left.x - right.x).abs() <= scale * EPS_COMPOSITE_DEGENERATE
        && (left.y - right.y).abs() <= scale * EPS_COMPOSITE_DEGENERATE
        && (left.z - right.z).abs() <= scale * EPS_COMPOSITE_DEGENERATE
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

fn anchor_analytic_nurbs_endpoint_poles(
    nurbs: &mut NurbsCurve,
    interval: [f64; 2],
    ir: &CadIr,
    index: Option<&CompositeIndex>,
    edge: &CompositeEdge,
    tolerance: Option<f64>,
) -> Option<()> {
    let Some(tolerance) = tolerance else {
        return Some(());
    };
    let start = point_for_vertex(ir, &edge.start, index)?;
    let end = point_for_vertex(ir, &edge.end, index)?;
    let evaluated_start = cadmpeg_ir::eval::nurbs_curve_point(
        nurbs.degree,
        &nurbs.knots,
        &nurbs.control_points,
        nurbs.weights.as_deref(),
        interval[0],
    )?;
    let evaluated_end = cadmpeg_ir::eval::nurbs_curve_point(
        nurbs.degree,
        &nurbs.knots,
        &nurbs.control_points,
        nurbs.weights.as_deref(),
        interval[1],
    )?;
    if !close_with_tolerance(evaluated_start, start, Some(tolerance))
        || !close_with_tolerance(evaluated_end, end, Some(tolerance))
    {
        return None;
    }
    *nurbs.control_points.first_mut()? = start;
    *nurbs.control_points.last_mut()? = end;
    Some(())
}

fn project_native_composite(
    ir: &mut CadIr,
    index: &mut CompositeIndex,
    entry: &DirectoryEntry,
    child_curves: &[CurveId],
    join_tolerance: f64,
) -> Option<EdgeId> {
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
    child_curves: &[CurveId],
    join_tolerance: f64,
    reason: &str,
    losses: &mut Vec<LossNote>,
) -> Option<EdgeId> {
    let edge = project_native_composite(ir, index, entry, child_curves, join_tolerance);
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
    project_with_type_130_policy(ir, directory, parameters, global, ctx, false)
}

pub(super) fn project_type_130_children(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<WireProjectionOutcome, CodecError> {
    project_with_type_130_policy(ir, directory, parameters, global, ctx, true)
}

fn has_type_130_child(
    sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    global_table: GlobalTable,
) -> bool {
    let Some(record) = records.get(&sequence).copied() else {
        return false;
    };
    let Some(child_count) = record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count <= MAX_COMPOSITE_CHILDREN)
    else {
        return false;
    };
    (0..child_count).any(|index| {
        record
            .integer(index + 2)
            .and_then(|value| u32::try_from(value).ok())
            .and_then(|child_sequence| entries.get(&child_sequence).copied())
            .is_some_and(|child| {
                child.entity_type == 130
                    && child.form == 0
                    && composite_child_type_allowed(child.entity_type, child.form, global_table)
            })
    })
}

fn project_with_type_130_policy(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
    only_type_130_children: bool,
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
        if has_type_130_child(entry.sequence, &entries, &records, global.global_table())
            != only_type_130_children
        {
            continue;
        }
        if !composite_use_flag_valid(entry.status.use_flag, global.global_table()) {
            losses.push(entity_loss(
                entry,
                "Type 102 Entity Use Flag must be 00 in IGES 4.0",
            ));
            continue;
        }
        if !composite_line_font_valid(
            entry.line_font,
            entry.status.hierarchy,
            global.global_table(),
        ) {
            losses.push(entity_loss(
                entry,
                "Type 102 Line Font must be nonzero in IGES 4.0 unless Hierarchy is 01",
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
        let minimum_child_count = composite_minimum_child_count(global.global_table());
        let Some(child_count) = usize::try_from(raw_child_count)
            .ok()
            .filter(|count| *count >= minimum_child_count)
        else {
            losses.push(entity_loss(
                entry,
                format!("child count is outside {minimum_child_count}..={MAX_COMPOSITE_CHILDREN}"),
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
        let is_logical_connector = child_sequences.len() == 2
            && child_sequences.iter().all(|sequence| {
                entries
                    .get(sequence)
                    .is_some_and(|child| child.entity_type == 132 && child.form == 0)
            });
        if !composite_logical_connector_use_valid(
            entry.status.use_flag,
            is_logical_connector,
            global.global_table(),
        ) {
            losses.push(entity_loss(
                entry,
                "Type 102 logical connectors made of exactly two Type 132 Connect Points require Entity Use Flag 04 in IGES 5.0 and later",
            ));
            continue;
        }
        if entry.transform != 0 {
            losses.push(entity_loss(
                entry,
                "placed composite curves require transformed child-carrier projection",
            ));
            continue;
        }
        if child_sequences.iter().any(|sequence| {
            entries.get(sequence).is_none_or(|child| {
                !composite_child_type_allowed(child.entity_type, child.form, global.global_table())
                    || !child.status.is_physically_dependent()
            })
        }) {
            losses.push(entity_loss(
                entry,
                "composite child is missing, outside the declared dialect, or is not physically dependent",
            ));
            continue;
        }
        let point_context = CompositePointContext {
            entries: &entries,
            records: &records,
            global,
            ctx,
            tolerance: join_tolerance,
        };
        let curve_carriers = child_sequences
            .iter()
            .copied()
            .filter(|sequence| {
                entries
                    .get(sequence)
                    .is_none_or(|entry| !composite_point_member(entry))
            })
            .filter_map(|sequence| {
                curve_carrier_id(sequence, &entries, &records).map(|curve_id| (sequence, curve_id))
            })
            .collect::<BTreeMap<_, _>>();
        if !composite_point_adjacency_valid(
            ir,
            &index,
            &child_sequences,
            &curve_carriers,
            &point_context,
        ) {
            losses.push(entity_loss(
                entry,
                "point or connect-point adjacency is invalid",
            ));
            continue;
        }
        let curve_sequences = child_sequences
            .iter()
            .copied()
            .filter(|sequence| {
                entries
                    .get(sequence)
                    .is_none_or(|entry| !composite_point_member(entry))
            })
            .collect::<Vec<_>>();
        if curve_sequences.is_empty() {
            losses.push(entity_loss(
                entry,
                "composite has no parameterized curve constituent",
            ));
            continue;
        }
        let Some(curve_ids) = curve_sequences
            .iter()
            .map(|sequence| curve_carriers.get(sequence).cloned())
            .collect::<Option<Vec<_>>>()
        else {
            losses.push(entity_loss(
                entry,
                "a Type 142 constituent has no valid model-space curve pointer",
            ));
            continue;
        };
        let Some(children) = curve_ids
            .iter()
            .map(|curve_id| bounded_nurbs(ir, &index, curve_id, join_tolerance, ctx))
            .collect::<Option<Vec<_>>>()
        else {
            if let Some(edge) = project_degraded_composite(
                ir,
                &mut index,
                entry,
                &curve_ids,
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
                &curve_ids,
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
                &curve_ids,
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
                &curve_ids,
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
                &curve_ids,
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
                components: curve_ids,
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
