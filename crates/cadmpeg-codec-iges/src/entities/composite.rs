// SPDX-License-Identifier: Apache-2.0
//! Ordered composite-curve projection.

use super::curve_conversion::{circular_arc_nurbs, elliptical_arc_nurbs, parabolic_arc_nurbs};
use super::geometry::{resolve_transform, source_object, WireProjectionOutcome};
use crate::decode_resource::{copy_optional_identity, reserve_admitted_vec, reserve_optional_vec, reserve_optional_vec_growth, reserve_vec};
use crate::directory::{DirectoryEntry, Hierarchy, UseFlag};
use crate::global::{GlobalTable, ProjectedGlobal};
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{alloc_filled, refuse_local_limit, u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsError, NurbsPoles3, WeightedPole3}, CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry,
    ProceduralCurve, ProceduralCurveDefinition, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal, PositiveReal};
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

fn composite_use_flag_valid(use_flag: UseFlag, global_table: GlobalTable) -> bool {
    !matches!(global_table, GlobalTable::V4_0) || use_flag == UseFlag::Geometry
}

fn composite_line_font_valid(
    line_font: i64,
    hierarchy: Option<Hierarchy>,
    global_table: GlobalTable,
) -> bool {
    !matches!(global_table, GlobalTable::V4_0)
        || hierarchy == Some(Hierarchy::GlobalDefer)
        || line_font != 0
}

fn composite_logical_connector_use_valid(
    use_flag: UseFlag,
    is_logical_connector: bool,
    global_table: GlobalTable,
) -> bool {
    !is_logical_connector
        || !matches!(global_table, GlobalTable::V5_0 | GlobalTable::V5Later)
        || use_flag == UseFlag::LogicalPositional
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

    fn member_point(&self, sequence: u32) -> Result<Option<Point3>, CodecError> {
        let Some(entry) = self.entries.get(&sequence).copied() else {
            return Ok(None);
        };
        if !composite_point_member(entry) {
            return Ok(None);
        }
        let Some(record) = self.records.get(&sequence).copied() else {
            return Ok(None);
        };
        let [Some(x), Some(y), Some(z)] = [record.number(1), record.number(2), record.number(3)]
        else {
            return Ok(None);
        };
        let transform = match resolve_transform(
            entry.transform,
            self.entries,
            self.records,
            self.global.length_factor_mm(),
            self.global.real_precision(),
            &mut BTreeSet::new(),
            self.ctx,
        ) {
            Ok(transform) => transform,
            Err(error) => {
                error.non_resource()?;
                return Ok(None);
            }
        };
        let point = transform.apply_point(Point3::new(
            x * self.global.length_factor_mm(),
            y * self.global.length_factor_mm(),
            z * self.global.length_factor_mm(),
        ));
        Ok(point.map(FinitePoint3::get))
    }
}

fn composite_point_adjacency_valid(
    ir: &CadIr,
    index: &CompositeIndex,
    child_sequences: &[u32],
    curve_carriers: &BTreeMap<u32, CurveId>,
    context: &CompositePointContext<'_, '_, '_, '_>,
) -> Result<bool, CodecError> {
    let all_points = child_sequences
        .iter()
        .copied()
        .all(|sequence| context.is_point(sequence));
    if child_sequences
        .windows(2)
        .any(|pair| context.is_point(pair[0]) && context.is_point(pair[1]))
        && !(all_points && child_sequences.len() == 2)
    {
        return Ok(false);
    }
    for (position, sequence) in child_sequences.iter().enumerate() {
        if !context.is_point(*sequence) {
            continue;
        }
        let Some(point) = context.member_point(*sequence)? else {
            return Ok(false);
        };
        if position > 0 && !context.is_point(child_sequences[position - 1]) {
            let Some(curve_id) = curve_carriers.get(&child_sequences[position - 1]) else {
                return Ok(false);
            };
            let Some((_, end)) = curve_endpoints(ir, curve_id, index, context.tolerance)? else {
                return Ok(false);
            };
            if !close_with_tolerance(end.get(), point, Some(context.tolerance)) {
                return Ok(false);
            }
        }
        if position + 1 < child_sequences.len() && !context.is_point(child_sequences[position + 1])
        {
            let Some(curve_id) = curve_carriers.get(&child_sequences[position + 1]) else {
                return Ok(false);
            };
            let Some((start, _)) = curve_endpoints(ir, curve_id, index, context.tolerance)? else {
                return Ok(false);
            };
            if !close_with_tolerance(point, start.get(), Some(context.tolerance)) {
                return Ok(false);
            }
        }
    }
    Ok(true)
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
    Some(crate::ids::curve(&crate::ids::Stem::directory(
        carrier_sequence,
    )))
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
    vertex_points: BTreeMap<VertexId, FinitePoint3>,
}

impl CompositeIndex {
    pub(super) fn from_ir(ir: &CadIr, ctx: Option<&DecodeContext<'_>>) -> Result<Self, CodecError> {
        let mut curve_positions = BTreeMap::new();
        for (position, curve) in ir.model.curves.iter().enumerate() {
            if !curve_positions.contains_key(&curve.id) {
                let key = crate::decode_resource::copy_optional_identity(ctx, curve.id.as_str(), "iges composite curve index keys")?;
                crate::decode_resource::insert_optional_btree_map(ctx, &mut curve_positions, key, position, "iges composite curve index nodes")?;
            }
        }
        let mut edges = BTreeMap::new();
        for edge in &ir.model.edges {
            if let Some(curve) = edge.curve() {
                if !edges.contains_key(curve) {
                    let key = crate::decode_resource::copy_optional_identity(ctx, curve.as_str(), "iges composite edge index keys")?;
                    crate::decode_resource::insert_optional_btree_map(ctx, &mut edges, key, Vec::new(), "iges composite edge index nodes")?;
                }
                let indexed = edges.get_mut(curve).ok_or_else(|| CodecError::Malformed("IGES composite edge index is absent".into()))?;
                crate::decode_resource::reserve_optional_vec_growth(ctx, indexed, 1, "iges composite indexed edges")?;
                indexed.push(CompositeEdge {
                    start: crate::decode_resource::copy_optional_identity(ctx, edge.start.as_str(), "iges composite indexed start ids")?,
                    end: crate::decode_resource::copy_optional_identity(ctx, edge.end.as_str(), "iges composite indexed end ids")?,
                    param_range: edge.param_range().map(cadmpeg_ir::units::FiniteVector::get),
                });
            }
        }
        let mut points = BTreeMap::<PointId, FinitePoint3>::new();
        for point in &ir.model.points {
            if !points.contains_key(&point.id) {
                let key = crate::decode_resource::copy_optional_identity(ctx, point.id.as_str(), "iges composite point index keys")?;
                crate::decode_resource::insert_optional_btree_map(ctx, &mut points, key, point.position(), "iges composite point index nodes")?;
            }
        }
        let mut vertex_points = BTreeMap::<VertexId, FinitePoint3>::new();
        for vertex in &ir.model.vertices {
            if let Some(point) = points.get(&vertex.point).copied() {
                if !vertex_points.contains_key(&vertex.id) {
                    let key = crate::decode_resource::copy_optional_identity(ctx, vertex.id.as_str(), "iges composite vertex index keys")?;
                    crate::decode_resource::insert_optional_btree_map(ctx, &mut vertex_points, key, point, "iges composite vertex index nodes")?;
                }
            }
        }
        Ok(Self {
            curve_positions,
            edges,
            vertex_points,
        })
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
        endpoints: [(VertexId, FinitePoint3); 2],
        ctx: Option<&DecodeContext<'_>>,
    ) -> Result<(), CodecError> {
        let position_key = crate::decode_resource::copy_optional_identity(ctx, curve_id.as_str(), "iges composite added curve index key")?;
        crate::decode_resource::insert_optional_btree_map(ctx, &mut self.curve_positions, position_key, curve_index, "iges composite added curve index node")?;
        if !self.edges.contains_key(&curve_id) {
            let edge_key = crate::decode_resource::copy_optional_identity(ctx, curve_id.as_str(), "iges composite added edge index key")?;
            crate::decode_resource::insert_optional_btree_map(ctx, &mut self.edges, edge_key, Vec::new(), "iges composite added edge index node")?;
        }
        let indexed = self.edges.get_mut(&curve_id).ok_or_else(|| CodecError::Malformed("IGES composite added edge index is absent".into()))?;
        crate::decode_resource::reserve_optional_vec_growth(ctx, indexed, 1, "iges composite added edge slots")?;
        indexed.push(edge);
        for (vertex, point) in endpoints {
            crate::decode_resource::insert_optional_btree_map(ctx, &mut self.vertex_points, vertex, point, "iges composite added vertex index nodes")?;
        }
        Ok(())
    }
}

fn point_for_vertex(
    ir: &CadIr,
    id: &VertexId,
    index: Option<&CompositeIndex>,
) -> Option<FinitePoint3> {
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
        .map(Point::position)
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
            close_with_tolerance(left_start.get(), right_start.get(), Some(tolerance))
                && close_with_tolerance(left_end.get(), right_end.get(), Some(tolerance))
        }
        (None, None, None, None) => true,
        _ => false,
    }
}

fn select_composite_edge(
    ir: &CadIr,
    index: Option<&CompositeIndex>,
    geometry: &SolvedCurveGeometry,
    candidates: &[CompositeEdge],
    tolerance: f64,
) -> Result<Option<CompositeEdge>, CodecError> {
    let mut first: Option<&CompositeEdge> = None;
    let mut agreement = true;
    for edge in candidates {
        let Some(range) = edge.param_range else {
            continue;
        };
        if matches!(geometry, SolvedCurveGeometry::Line(_)) {
            let (Some(start), Some(end)) = (
                point_for_vertex(ir, &edge.start, index),
                point_for_vertex(ir, &edge.end, index),
            ) else {
                continue;
            };
            let Some(evaluated_start) =
                finite_or_refusal(cadmpeg_ir::eval::curve_point_solved(geometry, range[0]))?
            else {
                continue;
            };
            let Some(evaluated_end) =
                finite_or_refusal(cadmpeg_ir::eval::curve_point_solved(geometry, range[1]))?
            else {
                continue;
            };
            if !close_with_tolerance(evaluated_start.get(), start.get(), Some(tolerance))
                || !close_with_tolerance(evaluated_end.get(), end.get(), Some(tolerance))
            {
                continue;
            }
        }
        if let Some(previous) = first {
            agreement &= edge.param_range == previous.param_range
                && composite_edge_endpoints_agree(ir, index, edge, previous, tolerance);
        } else {
            first = Some(edge);
        }
    }
    Ok(first.filter(|_| agreement).cloned())
}

fn homogeneous_point_is_valid(point: &[f64; 4]) -> bool {
    point.iter().all(|value| value.is_finite()) && point[0] > 0.0
}

fn homogeneous_control_points(
    ctx: Option<&DecodeContext<'_>>,
    curve: &NurbsCurve,
) -> Result<Option<Vec<[f64; 4]>>, CodecError> {
    let control_count = curve.control_points().len();
    let mut homogeneous = match ctx {
        Some(ctx) => ctx.alloc_filled(
            control_count,
            [0.0; 4],
            "iges composite homogeneous control points",
        )?,
        None => alloc_filled(
            control_count,
            [0.0; 4],
            "iges composite homogeneous control points",
        )?,
    };
    for (index, point) in curve.control_points().iter().enumerate() {
        let Some(weight) = curve.weights().map_or(Some(1.0), |weights| {
            weights.get(index).map(|weight| weight.get())
        }) else {
            return Ok(None);
        };
        let homogeneous_point = [weight, weight * point.x, weight * point.y, weight * point.z];
        if !homogeneous_point_is_valid(&homogeneous_point) {
            return Ok(None);
        }
        homogeneous[index] = homogeneous_point;
    }
    Ok(Some(homogeneous))
}

struct EuclideanControlNet {
    control_points: Vec<FinitePoint3>,
    weights: Option<Vec<PositiveReal>>,
}

fn euclidean_control_points(
    ctx: Option<&DecodeContext<'_>>,
    homogeneous: Vec<[f64; 4]>,
    rational: bool,
) -> Result<Option<EuclideanControlNet>, CodecError> {
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(
            homogeneous.len() as u64,
            "iges composite Euclidean control points",
        )?;
        if rational {
            ctx.charge_collection_items(
                homogeneous.len() as u64,
                "iges composite Euclidean weights",
            )?;
        }
    }
    let mut control_points =
        reserve_admitted_vec(homogeneous.len(), "iges composite Euclidean control points")?;
    let mut weights = if rational {
        Some(reserve_admitted_vec(
            homogeneous.len(),
            "iges composite Euclidean weights",
        )?)
    } else {
        None
    };
    for [weight, x, y, z] in homogeneous {
        let Some(weight) = PositiveReal::new(weight) else {
            return Ok(None);
        };
        let point = Point3::new(x / weight.get(), y / weight.get(), z / weight.get());
        let Some(point) = FinitePoint3::new(point) else {
            return Ok(None);
        };
        control_points.push(point);
        if let Some(weights) = &mut weights {
            weights.push(weight);
        }
    }
    Ok(Some(EuclideanControlNet {
        control_points,
        weights,
    }))
}

fn elevate_bezier_homogeneous(
    ctx: Option<&DecodeContext<'_>>,
    control_points: &[[f64; 4]],
    source_degree: usize,
    target_degree: usize,
) -> Result<Option<Vec<[f64; 4]>>, CodecError> {
    let Some(source_count) = source_degree.checked_add(1) else {
        return Ok(None);
    };
    if control_points.len() != source_count || target_degree < source_degree {
        return Ok(None);
    }
    if control_points
        .iter()
        .any(|point| !homogeneous_point_is_valid(point))
    {
        return Ok(None);
    }
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(
            control_points.len() as u64,
            "iges composite Bezier source copy",
        )?;
    }
    let mut elevated =
        reserve_admitted_vec(control_points.len(), "iges composite Bezier source copy")?;
    elevated.extend_from_slice(control_points);
    let mut degree = source_degree;
    while degree < target_degree {
        let Some(next_degree) = degree.checked_add(1) else {
            return Ok(None);
        };
        let Some(next_count) = next_degree.checked_add(1) else {
            return Ok(None);
        };
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(next_count as u64, "iges composite Bezier elevated net")?;
        }
        let mut next = reserve_admitted_vec(next_count, "iges composite Bezier elevated net")?;
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
                return Ok(None);
            }
            next.push(point);
        }
        let Some(last) = elevated.last() else {
            return Ok(None);
        };
        next.push(*last);
        elevated = next;
        degree = next_degree;
    }
    Ok(Some(elevated))
}

#[derive(Debug)]
struct ConcatenatedNurbs<T> {
    nurbs: NurbsCurve,
    segments: ConcatenatedSegments<T>,
}

#[derive(Debug)]
struct ConcatenatedSegment<T> {
    child_start: f64,
    end: f64,
    child: T,
}

#[derive(Debug)]
struct ConcatenatedSegments<T> {
    preceding: Vec<ConcatenatedSegment<T>>,
    last: ConcatenatedSegment<T>,
}

impl<T> ConcatenatedSegments<T> {
    fn end(&self) -> f64 {
        self.last.end
    }

    fn into_iter(self) -> impl Iterator<Item = ConcatenatedSegment<T>> {
        self.preceding.into_iter().chain(std::iter::once(self.last))
    }
}

// This conversion consumes the input carrier at the typed construction boundary.
#[allow(clippy::needless_pass_by_value)]
/// Reflects a child about its own parameter domain.
///
/// Every answer here is either the reversed child or a named cause: the values
/// read are the child's own stated degree, knots and interval, so a reader that
/// cannot reflect them states which of them it refused.
fn reverse_nurbs(
    curve: NurbsCurve,
    interval: [f64; 2],
) -> Result<(NurbsCurve, [f64; 2]), CompositeCurveError> {
    let Ok(degree) = usize::try_from(curve.degree()) else {
        return Err(CompositeCurveError::ReversedChildDegree {
            degree: curve.degree(),
        });
    };
    let control_count = curve.control_points().len();
    let [start, end] = interval;
    let (Some(finite_start), Some(finite_end)) = (FiniteReal::new(start), FiniteReal::new(end))
    else {
        return Err(CompositeCurveError::ReversedChildInterval { start, end });
    };
    if start > end {
        return Err(CompositeCurveError::ReversedChildInterval { start, end });
    }
    let (domain_start, domain_end) = (curve.knots()[degree], curve.knots()[control_count]);
    let (Some(lower), Some(upper)) = (FiniteReal::new(domain_start), FiniteReal::new(domain_end)) else {
        return Err(CompositeCurveError::ReversedChildReflectionNonFinite { domain_start, domain_end });
    };
    if domain_start >= domain_end {
        return Err(CompositeCurveError::ReversedChildReflectionNonFinite {
            domain_start,
            domain_end,
        });
    }
    if start < domain_start || end > domain_end {
        return Err(CompositeCurveError::ReversedChildIntervalOutsideDomain {
            start,
            end,
            domain_start,
            domain_end,
        });
    }
    let reflect = |parameter| {
        cadmpeg_ir::math::reflect_parameter(parameter, lower, upper)
            .map(FiniteReal::get)
            .ok_or(CompositeCurveError::ReversedChildReflectionNonFinite {
                domain_start,
                domain_end,
            })
    };
    let reversed_range = [reflect(finite_end)?, reflect(finite_start)?];
    let (source_degree, admitted_knots, mut poles, periodic) = curve.into_parts();
    let mut knots = admitted_knots.into_values();
    knots.reverse();
    for knot in &mut knots {
        let finite = FiniteReal::new(*knot).ok_or(CompositeCurveError::ReversedChildReflectionNonFinite { domain_start, domain_end })?;
        *knot = reflect(finite)?;
    }
    poles.reverse();
    let reversed = NurbsCurve::new(source_degree, knots, poles, periodic)?;
    Ok((reversed, reversed_range))
}

#[derive(Debug)]
struct InsertedKnotNet {
    control_points: Vec<[f64; 4]>,
    knots: Vec<f64>,
}

fn insert_homogeneous_knot(
    ctx: Option<&DecodeContext<'_>>,
    control_points: &[[f64; 4]],
    knots: &[f64],
    degree: usize,
    value: f64,
) -> Result<Option<InsertedKnotNet>, CodecError> {
    let control_count = control_points.len();
    let Some(last_control) = control_count.checked_sub(1) else {
        return Ok(None);
    };
    let Some(span) = knots.iter().rposition(|knot| *knot <= value) else {
        return Ok(None);
    };
    let span = if degree == 0 {
        span.min(last_control)
    } else {
        span
    };
    let multiplicity = knots.iter().filter(|knot| **knot == value).count();
    let Some(left_end) = span.checked_sub(degree) else {
        return Ok(None);
    };
    let Some(expected_knots) = control_count
        .checked_add(degree)
        .and_then(|count| count.checked_add(1))
    else {
        return Ok(None);
    };
    if control_count <= degree
        || left_end > last_control
        || multiplicity > degree
        || span < degree
        || knots.len() != expected_knots
    {
        return Ok(None);
    }
    let Some(knot_count) = knots.len().checked_add(1) else {
        return Ok(None);
    };
    let mut inserted_knots = match ctx {
        Some(ctx) => ctx.alloc_filled(knot_count, 0.0, "iges composite inserted knots")?,
        None => alloc_filled(knot_count, 0.0, "iges composite inserted knots")?,
    };
    inserted_knots.clear();
    let Some(left_knots) = knots.get(..=span) else {
        return Ok(None);
    };
    inserted_knots.extend_from_slice(left_knots);
    inserted_knots.push(value);
    let Some(next_knot) = span.checked_add(1) else {
        return Ok(None);
    };
    let Some(right_knots) = knots.get(next_knot..) else {
        return Ok(None);
    };
    inserted_knots.extend_from_slice(right_knots);

    let Some(inserted_count) = control_count.checked_add(1) else {
        return Ok(None);
    };
    let mut inserted_control_points = match ctx {
        Some(ctx) => ctx.alloc_filled(
            inserted_count,
            [0.0; 4],
            "iges composite knot-insertion control points",
        )?,
        None => alloc_filled(
            inserted_count,
            [0.0; 4],
            "iges composite knot-insertion control points",
        )?,
    };
    let Some(tail_start) = span.checked_sub(multiplicity) else {
        return Ok(None);
    };
    if tail_start > last_control {
        return Ok(None);
    }
    inserted_control_points[..=left_end].copy_from_slice(&control_points[..=left_end]);
    inserted_control_points[tail_start + 1..]
        .copy_from_slice(&control_points[tail_start..control_count]);
    let Some(first_interior) = left_end.checked_add(1) else {
        return Ok(None);
    };
    for index in first_interior..=tail_start {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return Ok(None);
        }
        let alpha = (value - knots[index]) / denominator;
        if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
            return Ok(None);
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
            return Ok(None);
        }
        inserted_control_points[index] = point;
    }
    Ok(Some(InsertedKnotNet {
        control_points: inserted_control_points,
        knots: inserted_knots,
    }))
}

/// Trim a curve to `interval`. `Ok(None)` states an interval the curve cannot
/// be trimmed to; `Err` states trimmed lanes the carrier refuses.
fn trim_nurbs_to_interval(
    ctx: Option<&DecodeContext<'_>>,
    curve: &NurbsCurve,
    interval: [f64; 2],
) -> Result<Option<NurbsCurve>, CompositeCurveError> {
    let Some((control_points, weights, trimmed_knots)) = trim_nurbs_lanes(ctx, curve, interval)?
    else {
        return Ok(None);
    };
    let weights = weights.map(|weights| {
        let mut converted = reserve_optional_vec(ctx, weights.len(), "iges composite trimmed weight conversion")?;
        converted.extend(weights.into_iter().map(cadmpeg_ir::scalar::NonZeroReal::from));
        Ok::<_, CodecError>(converted)
    }).transpose()?;
    Ok(Some(NurbsCurve::from_checked_lanes(
        curve.degree(),
        trimmed_knots,
        control_points,
        weights,
        false,
    )?))
}

type TrimmedLanes = (Vec<FinitePoint3>, Option<Vec<PositiveReal>>, Vec<f64>);

fn trim_nurbs_lanes(
    ctx: Option<&DecodeContext<'_>>,
    curve: &NurbsCurve,
    interval: [f64; 2],
) -> Result<Option<TrimmedLanes>, CodecError> {
    let Ok(degree) = usize::try_from(curve.degree()) else {
        return Ok(None);
    };
    let control_count = curve.control_points().len();
    if curve.periodic() {
        return Ok(None);
    }
    let [start, end] = interval;
    if !start.is_finite() || !end.is_finite() || start >= end {
        return Ok(None);
    }
    let (Some(&domain_start), Some(&domain_end)) =
        (curve.knots().get(degree), curve.knots().get(control_count))
    else {
        return Ok(None);
    };
    if !domain_start.is_finite()
        || !domain_end.is_finite()
        || domain_start >= domain_end
        || start < domain_start
        || end > domain_end
    {
        return Ok(None);
    }
    let Some(mut homogeneous) = homogeneous_control_points(ctx, curve)? else {
        return Ok(None);
    };
    let mut knots = reserve_optional_vec(ctx, curve.knots().len(), "iges composite trim knot copy")?;
    knots.extend_from_slice(curve.knots());
    for value in [start, end] {
        let Some(target_multiplicity) = degree.checked_add(1) else {
            return Ok(None);
        };
        while knots.iter().filter(|knot| **knot == value).count() < target_multiplicity {
            let Some(InsertedKnotNet {
                control_points: new_homogeneous,
                knots: new_knots,
            }) = insert_homogeneous_knot(ctx, &homogeneous, &knots, degree, value)?
            else {
                return Ok(None);
            };
            homogeneous = new_homogeneous;
            knots = new_knots;
        }
    }
    let (Some(start_knot), Some(end_knot)) = (
        knots.iter().position(|knot| *knot == start),
        knots.iter().rposition(|knot| *knot == end),
    ) else {
        return Ok(None);
    };
    let Some(control_end) = end_knot.checked_sub(degree) else {
        return Ok(None);
    };
    if start_knot >= control_end {
        return Ok(None);
    }
    let (Some(homogeneous_slice), Some(knot_slice)) = (
        homogeneous.get(start_knot..control_end),
        knots.get(start_knot..=end_knot),
    ) else {
        return Ok(None);
    };
    let mut trimmed_homogeneous = reserve_optional_vec(ctx, homogeneous_slice.len(), "iges composite trimmed controls")?;
    trimmed_homogeneous.extend_from_slice(homogeneous_slice);
    let mut trimmed_knots = reserve_optional_vec(ctx, knot_slice.len(), "iges composite trimmed knots")?;
    trimmed_knots.extend_from_slice(knot_slice);
    let Some(expected_knots) = trimmed_homogeneous
        .len()
        .checked_add(degree)
        .and_then(|count| count.checked_add(1))
    else {
        return Ok(None);
    };
    if trimmed_knots.len() != expected_knots {
        return Ok(None);
    }
    let Some(EuclideanControlNet {
        control_points,
        weights,
    }) = euclidean_control_points(ctx, trimmed_homogeneous, curve.weights().is_some())?
    else {
        return Ok(None);
    };
    Ok(Some((control_points, weights, trimmed_knots)))
}

/// Why a child curve could not be raised to the composite's degree.
///
/// Every refusal in `elevate_nurbs_to_degree` names its own cause, so the
/// caller reports what the source stated instead of attributing the failure
/// to an endpoint join that was never tested.
#[derive(Debug, thiserror::Error)]
pub(super) enum DegreeElevationError {
    /// The child's own degree is not a usable count.
    #[error("the child degree {degree} is not a usable count")]
    SourceDegree {
        /// Degree the child stated.
        degree: u32,
    },
    /// The composite degree is above the supported bound.
    #[error("the composite degree {degree} is above the supported bound {bound}")]
    TargetDegree {
        /// Degree the composite stated.
        degree: u32,
        /// Highest degree this reader raises a child to.
        bound: usize,
    },
    /// The composite degree is below the child's own degree.
    #[error("the composite degree {target} is below the child degree {child}")]
    TargetBelowSource {
        /// Degree the composite stated.
        target: usize,
        /// Degree the child stated.
        child: usize,
    },
    /// The child's knot vector is periodic, non-finite, out of order, or does
    /// not span its declared interval.
    #[error("the child knot vector does not span its declared interval [{start}, {end}]")]
    UnclampedKnots {
        /// Interval start the child declared.
        start: f64,
        /// Interval end the child declared.
        end: f64,
    },
    /// A boundary knot is not clamped to degree + 1 copies.
    #[error(
        "the child states {multiplicity} cop(ies) of the boundary knot {knot}, not {expected}"
    )]
    BoundaryMultiplicity {
        /// The boundary knot value.
        knot: f64,
        /// Copies the child stated.
        multiplicity: usize,
        /// Copies a clamped child states.
        expected: usize,
    },
    /// An internal knot repeats more often than the degree admits.
    #[error("the child states {multiplicity} cop(ies) of the internal knot {knot}, above {bound}")]
    InternalMultiplicity {
        /// The internal knot value.
        knot: f64,
        /// Copies the child stated.
        multiplicity: usize,
        /// Highest copy count the degree admits.
        bound: usize,
    },
    /// The child's poles and weights do not state a usable homogeneous net.
    #[error("the child states no usable homogeneous control net")]
    HomogeneousControlNet,
    /// Knot insertion did not produce a refined net.
    #[error("inserting the internal knot {knot} states no refined control net")]
    KnotInsertion {
        /// The knot being inserted.
        knot: f64,
    },
    /// The refined knot vector and control net do not agree.
    #[error("the refined knot vector and control net do not agree")]
    RefinedKnotVector,
    /// One Bezier span states no control net.
    #[error("Bezier span {span} states no control net")]
    SpanControlNet {
        /// Index of the span in the refined net.
        span: usize,
    },
    /// One Bezier span could not be raised to the composite degree.
    #[error("Bezier span {span} does not raise to degree {degree}")]
    SpanElevation {
        /// Index of the span in the refined net.
        span: usize,
        /// The composite degree.
        degree: usize,
    },
    /// An elevated span states no Euclidean control net.
    #[error("elevated Bezier span {span} states no Euclidean control net")]
    SpanEuclideanNet {
        /// Index of the span in the refined net.
        span: usize,
    },
    /// A checked allocation for the elevated knots was refused.
    #[error("{0}")]
    Allocation(cadmpeg_core::CodecError),
    /// The elevated Bezier spans do not join.
    #[error("the elevated Bezier spans do not join")]
    SpansDoNotJoin,
}

/// Why a composite curve states no joined carrier.
#[derive(Debug, thiserror::Error)]
pub(super) enum CompositeCurveError {
    /// A carrier the IR refuses.
    #[error(transparent)]
    Carrier(#[from] cadmpeg_ir::geometry::nurbs::NurbsError),
    /// A child that does not raise to the composite degree.
    #[error("{0}")]
    Elevation(#[from] DegreeElevationError),
    /// The composite states no child at all.
    #[error("the composite states no child curve")]
    EmptyChildList,
    /// A child states a knot vector with no first or last knot.
    #[error("child {child} states an empty knot vector")]
    ChildKnotsEmpty {
        /// Position of the child in the composite.
        child: usize,
    },
    /// A child's stated interval is not its own knot range.
    #[error(
        "child {child} states the interval [{start}, {end}], which is outside its knot range          [{first}, {last}]"
    )]
    ChildIntervalOutsideKnots {
        /// Position of the child in the composite.
        child: usize,
        /// Stated interval start.
        start: f64,
        /// Stated interval end.
        end: f64,
        /// First knot of the child.
        first: f64,
        /// Last knot of the child.
        last: f64,
    },
    /// The composite nests deeper than the decode policy admits.
    #[error(transparent)]
    Budget(#[from] cadmpeg_core::CodecError),
    /// A child's degree does not fit the platform's index width.
    #[error("a reversed child states the degree {degree}, which is not a usize")]
    ReversedChildDegree {
        /// Degree the child states.
        degree: u32,
    },
    /// A reversed child's stated interval is non-finite or decreasing.
    #[error("a reversed child states the interval [{start}, {end}]")]
    ReversedChildInterval {
        /// Stated interval start.
        start: f64,
        /// Stated interval end.
        end: f64,
    },
    /// A reversed child's stated interval leaves its own knot domain.
    #[error(
        "a reversed child states the interval [{start}, {end}], which is outside its domain [{domain_start}, {domain_end}]"
    )]
    ReversedChildIntervalOutsideDomain {
        /// Stated interval start.
        start: f64,
        /// Stated interval end.
        end: f64,
        /// First domain knot of the child.
        domain_start: f64,
        /// Last domain knot of the child.
        domain_end: f64,
    },
    /// Reflecting a child's domain does not stay finite.
    #[error("reflecting a child about its domain [{domain_start}, {domain_end}] is not finite")]
    ReversedChildReflectionNonFinite {
        /// First domain knot of the child.
        domain_start: f64,
        /// Last domain knot of the child.
        domain_end: f64,
    },
    /// A child's stated interval does not increase.
    #[error("child {child} states the reversed interval [{start}, {end}]")]
    ChildIntervalReversed {
        /// Position of the child in the composite.
        child: usize,
        /// Stated interval start.
        start: f64,
        /// Stated interval end.
        end: f64,
    },
    /// A child still carries a degree below the composite degree.
    #[error(
        "child {child} states degree {degree} after elevation to the composite degree {composite}"
    )]
    ChildDegreeMismatch {
        /// Position of the child in the composite.
        child: usize,
        /// Degree the child states.
        degree: u32,
        /// Degree the composite states.
        composite: u32,
    },
    /// A child states a weight the rational lane cannot carry.
    #[error("a child states the non-positive or non-finite weight {weight}")]
    ChildWeight {
        /// The refused weight.
        weight: f64,
    },
    /// The unit weight lane of a polynomial child could not be allocated.
    #[error("{0}")]
    ChildWeightAllocation(cadmpeg_core::CodecError),
    /// A child's end parameter is not finite once shifted onto the composite.
    #[error("a child states the non-finite end parameter {end}")]
    ChildEndParameter {
        /// The refused end parameter.
        end: f64,
    },
    /// The weight scale that joins two children is not usable.
    #[error("two children join on the non-positive or non-finite weight scale {scale}")]
    JoinWeightScale {
        /// The refused scale.
        scale: f64,
    },
    /// The joined carrier states no point at one of its own endpoints.
    #[error("the joined carrier states no point at the endpoint parameter {t}")]
    EndpointEvaluation {
        /// The endpoint parameter the carrier does not evaluate at.
        t: f64,
    },
}

impl CompositeCurveError {
    /// Return a decode resource refusal before a caller considers geometric fallback.
    pub(super) fn non_resource(self) -> Result<Self, CodecError> {
        match self {
            Self::Budget(error) | Self::ChildWeightAllocation(error) => Err(error),
            Self::Elevation(DegreeElevationError::Allocation(error)) => Err(error),
            error => Ok(error),
        }
    }
}

fn elevate_nurbs_to_degree(
    ctx: Option<&DecodeContext<'_>>,
    curve: &mut NurbsCurve,
    interval: [f64; 2],
    target_degree: u32,
    join_tolerance: Option<f64>,
) -> Result<(), CompositeCurveError> {
    let Ok(source_degree) = usize::try_from(curve.degree()) else {
        return Err(DegreeElevationError::SourceDegree {
            degree: curve.degree(),
        }
        .into());
    };
    let stated_target = target_degree;
    let target_degree = match usize::try_from(target_degree) {
        Ok(target_degree) if target_degree <= MAX_COMPOSITE_DEGREE => target_degree,
        _ => {
            return Err(DegreeElevationError::TargetDegree {
                degree: stated_target,
                bound: MAX_COMPOSITE_DEGREE,
            }
            .into())
        }
    };
    if target_degree < source_degree {
        return Err(DegreeElevationError::TargetBelowSource {
            target: target_degree,
            child: source_degree,
        }
        .into());
    }
    if target_degree == source_degree {
        return Ok(());
    }
    if curve.periodic()
        || curve.knots().first() != Some(&interval[0])
        || curve.knots().last() != Some(&interval[1])
        || !interval[0].is_finite()
        || !interval[1].is_finite()
        || interval[0] >= interval[1]
    {
        return Err(DegreeElevationError::UnclampedKnots {
            start: interval[0],
            end: interval[1],
        }
        .into());
    }
    let boundary_multiplicity =
        |value: f64| curve.knots().iter().filter(|knot| **knot == value).count();
    for knot in interval {
        let multiplicity = boundary_multiplicity(knot);
        if multiplicity != source_degree + 1 {
            return Err(DegreeElevationError::BoundaryMultiplicity {
                knot,
                multiplicity,
                expected: source_degree + 1,
            }
            .into());
        }
    }
    // `homogeneous_control_points` already answers `None` on the first invalid
    // point, so no second pass over the net can observe one.
    let mut homogeneous =
        homogeneous_control_points(ctx, curve).map_err(DegreeElevationError::Allocation)?;
    let mut knots = reserve_optional_vec(ctx, curve.knots().len(), "iges composite elevation knot copy")
        .map_err(DegreeElevationError::Allocation)?;
    knots.extend_from_slice(curve.knots());
    let mut internal_values = Vec::new();
    for &knot in &knots {
        if knot > interval[0] && knot < interval[1] && internal_values.last().copied() != Some(knot)
        {
            reserve_optional_vec_growth(ctx, &mut internal_values, 1, "iges composite internal knot values")
                .map_err(DegreeElevationError::Allocation)?;
            internal_values.push(knot);
        }
    }
    for value in internal_values {
        let multiplicity = knots.iter().filter(|knot| **knot == value).count();
        if multiplicity > source_degree + 1 {
            return Err(DegreeElevationError::InternalMultiplicity {
                knot: value,
                multiplicity,
                bound: source_degree + 1,
            }
            .into());
        }
        for _ in multiplicity..source_degree {
            let Some(points) = homogeneous.take() else {
                return Err(DegreeElevationError::HomogeneousControlNet.into());
            };
            let Some(InsertedKnotNet {
                control_points: new_points,
                knots: new_knots,
            }) = insert_homogeneous_knot(ctx, &points, &knots, source_degree, value)
                .map_err(DegreeElevationError::Allocation)?
            else {
                return Err(DegreeElevationError::KnotInsertion { knot: value }.into());
            };
            homogeneous = Some(new_points);
            knots = new_knots;
        }
    }
    let Some(homogeneous) = homogeneous else {
        return Err(DegreeElevationError::HomogeneousControlNet.into());
    };
    let Some(refined_count) = homogeneous.len().checked_sub(1) else {
        return Err(DegreeElevationError::RefinedKnotVector.into());
    };
    let Some(refined_knot_count) = refined_count
        .checked_add(source_degree)
        .and_then(|value| value.checked_add(2))
    else {
        return Err(DegreeElevationError::RefinedKnotVector.into());
    };
    if knots.len() != refined_knot_count
        || knots.first() != Some(&interval[0])
        || knots.last() != Some(&interval[1])
    {
        return Err(DegreeElevationError::RefinedKnotVector.into());
    }
    let rational = curve.weights().is_some();
    let mut pieces = Vec::new();
    for span in source_degree..=refined_count {
        let start = knots[span];
        let end = knots[span + 1];
        if !start.is_finite() || !end.is_finite() || start >= end {
            continue;
        }
        let Some(source_points) = homogeneous.get(span - source_degree..=span) else {
            return Err(DegreeElevationError::SpanControlNet { span }.into());
        };
        let Some(elevated) =
            elevate_bezier_homogeneous(ctx, source_points, source_degree, target_degree)
                .map_err(DegreeElevationError::Allocation)?
        else {
            return Err(DegreeElevationError::SpanElevation {
                span,
                degree: target_degree,
            }
            .into());
        };
        let Some(EuclideanControlNet {
            control_points,
            weights,
        }) = euclidean_control_points(ctx, elevated, rational)
            .map_err(DegreeElevationError::Allocation)?
        else {
            return Err(DegreeElevationError::SpanEuclideanNet { span }.into());
        };
        let Some(target_knot_count) = target_degree.checked_add(1) else {
            return Err(DegreeElevationError::TargetDegree {
                degree: stated_target,
                bound: MAX_COMPOSITE_DEGREE,
            }
            .into());
        };
        let mut piece_knots = match ctx {
            Some(ctx) => {
                ctx.alloc_filled(target_knot_count, start, "iges composite elevated knots")
            }
            None => alloc_filled(target_knot_count, start, "iges composite elevated knots"),
        }
        .map_err(DegreeElevationError::Allocation)?;
        reserve_optional_vec_growth(ctx, &mut piece_knots, target_knot_count, "iges composite elevated knot suffix")
            .map_err(DegreeElevationError::Allocation)?;
        let complete_knot_count = piece_knots.len().checked_add(target_knot_count)
            .ok_or_else(|| DegreeElevationError::Allocation(refuse_local_limit("iges composite elevated knot suffix", u64::MAX, 1)))?;
        piece_knots.resize(complete_knot_count, end);
        let weights = weights.map(|weights| {
            let mut converted = reserve_optional_vec(ctx, weights.len(), "iges composite elevated weight conversion")?;
            converted.extend(weights.into_iter().map(cadmpeg_ir::scalar::NonZeroReal::from));
            Ok::<_, CodecError>(converted)
        }).transpose().map_err(DegreeElevationError::Allocation)?;
        reserve_optional_vec_growth(ctx, &mut pieces, 1, "iges composite elevated span")
            .map_err(DegreeElevationError::Allocation)?;
        let piece = NurbsCurve::from_checked_lanes(
            target_degree as u32,
            piece_knots,
            control_points,
            weights,
            false,
        )?;
        pieces.push((piece, [start, end], ()));
    }
    let Some(concatenated) = concatenate_nurbs(ctx, pieces, join_tolerance)? else {
        return Err(DegreeElevationError::SpansDoNotJoin.into());
    };
    let (elevated_degree, admitted_knots, poles, _) = concatenated.nurbs.into_parts();
    let mut elevated_knots = admitted_knots.into_values();
    for knot in &mut elevated_knots {
        *knot += interval[0];
    }
    // Translation must preserve every copy of each clamped source endpoint.
    // Adding the origin back can round past the declared endpoint.
    elevated_knots[..=target_degree].fill(interval[0]);
    let end_start = elevated_knots.len() - target_degree - 1;
    elevated_knots[end_start..].fill(interval[1]);
    let elevated = NurbsCurve::new(
        elevated_degree,
        elevated_knots,
        poles,
        false,
    )?;
    *curve = elevated;
    Ok(())
}

fn concatenate_nurbs<T>(
    ctx: Option<&DecodeContext<'_>>,
    children: Vec<(NurbsCurve, [f64; 2], T)>,
    join_tolerance: Option<f64>,
) -> Result<Option<ConcatenatedNurbs<T>>, CompositeCurveError> {
    let mut children = children.into_iter();
    let Some(mut first) = children.next() else {
        return Err(CompositeCurveError::EmptyChildList);
    };
    let degree = children
        .as_slice()
        .iter()
        .map(|(curve, _, _)| curve.degree())
        .fold(first.0.degree(), u32::max);
    for (curve, interval, _) in std::iter::once(&mut first).chain(children.as_mut_slice()) {
        if curve.degree() < degree {
            // A child that does not raise to the composite degree states why.
            // The endpoint-join check below is reached only when every child
            // carries the composite degree.
            elevate_nurbs_to_degree(ctx, curve, *interval, degree, join_tolerance)?;
        }
    }
    for (child, (curve, interval, _)) in std::iter::once(&first)
        .chain(children.as_slice())
        .enumerate()
    {
        let (Some(first_knot), Some(last_knot)) = (curve.knots().first(), curve.knots().last())
        else {
            return Err(CompositeCurveError::ChildKnotsEmpty { child });
        };
        if curve.degree() != degree {
            return Err(CompositeCurveError::ChildDegreeMismatch {
                child,
                degree: curve.degree(),
                composite: degree,
            });
        }
        if interval[0] >= interval[1] {
            return Err(CompositeCurveError::ChildIntervalReversed {
                child,
                start: interval[0],
                end: interval[1],
            });
        }
        if interval != &[*first_knot, *last_knot] {
            return Err(CompositeCurveError::ChildIntervalOutsideKnots {
                child,
                start: interval[0],
                end: interval[1],
                first: *first_knot,
                last: *last_knot,
            });
        }
    }
    let degree_usize = degree as usize;
    // Every refusal below names its own cause; the `Ok(None)` that survives is
    // the endpoint join, and only the endpoint join.
    let prepare_child = |(curve, interval, child): (NurbsCurve, [f64; 2], T),
                         cursor: f64|
     -> Result<_, CompositeCurveError> {
        let child_start = interval[0];
        let child_end = interval[1];
        let control_count = curve.pole_count();
        let (_, admitted_knots, poles, _) = curve.into_parts();
        let mut shifted_knots = admitted_knots.into_values();
        for knot in &mut shifted_knots {
            *knot = (*knot - child_start) + cursor;
        }
        let mut child_control_points = reserve_optional_vec(ctx, control_count, "iges composite child control points")?;
        let child_weights = match poles {
            NurbsPoles3::Polynomial { points } => {
                child_control_points.extend(points);
                match ctx {
                Some(ctx) => ctx.alloc_filled(
                    control_count,
                    1.0,
                    "iges composite child weights",
                ),
                None => alloc_filled(
                    control_count,
                    1.0,
                    "iges composite child weights",
                ),
                }.map_err(CompositeCurveError::ChildWeightAllocation)?
            }
            NurbsPoles3::Rational { points } => {
                let mut weights = reserve_optional_vec(ctx, control_count, "iges composite child weight copy")?;
                for pole in points {
                    child_control_points.push(pole.point);
                    weights.push(pole.weight.get());
                }
                weights
            }
        };
        if let Some(weight) = child_weights.iter().copied().find(|weight| *weight <= 0.0) {
            return Err(CompositeCurveError::ChildWeight { weight });
        }
        let end = cursor + (child_end - child_start);
        if !end.is_finite() {
            return Err(CompositeCurveError::ChildEndParameter { end });
        }
        Ok((
            shifted_knots,
            child_control_points,
            child_weights,
            ConcatenatedSegment {
                child_start,
                end,
                child,
            },
        ))
    };
    let (mut knots, mut control_points, mut weights, last) = prepare_child(first, 0.0)?;
    let mut segments = ConcatenatedSegments {
        preceding: reserve_optional_vec(ctx, children.len(), "iges composite segment slots")?,
        last,
    };
    for child in children {
        let (shifted_knots, child_control_points, mut child_weights, next) =
            prepare_child(child, segments.end())?;
        if !close_with_tolerance(
            control_points[control_points.len() - 1].get(),
            child_control_points[0].get(),
            join_tolerance,
        ) {
            return Ok(None);
        }
        let previous_weight = weights[weights.len() - 1];
        let join_weight = child_weights[0];
        let scale = previous_weight / join_weight;
        for weight in &mut child_weights {
            let scaled = *weight * scale;
            *weight = if scaled.is_finite() && scaled > 0.0 {
                scaled
            } else {
                match [*weight, previous_weight, join_weight].map(FiniteReal::new) {
                    [Some(weight), Some(previous_weight), Some(join_weight)] => {
                        cadmpeg_ir::math::multiply_divide(weight, previous_weight, join_weight)
                    }
                    _ => None,
                }
                .filter(|weight| weight.get() > 0.0)
                .ok_or(CompositeCurveError::JoinWeightScale { scale })?
                .get()
            };
        }
        if degree_usize == 0 {
            reserve_optional_vec_growth(ctx, &mut knots, shifted_knots.len() - 1, "iges composite joined knots")?;
            reserve_optional_vec_growth(ctx, &mut control_points, child_control_points.len(), "iges composite joined controls")?;
            reserve_optional_vec_growth(ctx, &mut weights, child_weights.len(), "iges composite joined weights")?;
            knots.extend_from_slice(&shifted_knots[1..]);
            control_points.extend_from_slice(&child_control_points);
            weights.extend_from_slice(&child_weights);
        } else {
            knots.pop();
            reserve_optional_vec_growth(ctx, &mut knots, shifted_knots.len() - degree_usize - 1, "iges composite joined knots")?;
            reserve_optional_vec_growth(ctx, &mut control_points, child_control_points.len() - 1, "iges composite joined controls")?;
            reserve_optional_vec_growth(ctx, &mut weights, child_weights.len() - 1, "iges composite joined weights")?;
            knots.extend_from_slice(&shifted_knots[degree_usize + 1..]);
            control_points.extend_from_slice(&child_control_points[1..]);
            weights.extend_from_slice(&child_weights[1..]);
        }
        let preceding = std::mem::replace(&mut segments.last, next);
        segments.preceding.push(preceding);
    }
    let cursor = segments.end();
    let rational = weights
        .first()
        .is_some_and(|first| weights.iter().any(|weight| weight != first));
    let poles = if rational {
        let mut weighted = reserve_optional_vec(ctx, control_points.len(), "iges composite joined weighted poles")?;
        for (index, (point, weight)) in control_points.into_iter().zip(weights).enumerate() {
            let weight = NonZeroReal::new(weight).ok_or_else(|| NurbsError::UnusableWeight {
                field: "poles".to_owned(),
                index,
                weight,
            })?;
            weighted.push(WeightedPole3 { point, weight });
        }
        NurbsPoles3::Rational { points: weighted }
    } else {
        NurbsPoles3::Polynomial { points: control_points }
    };
    let nurbs = NurbsCurve::new(degree, knots, poles, false)?;
    // The joined carrier evaluates at both of its own endpoints: reading the
    // two points is the statement, and each names its own parameter when the
    // carrier does not answer.
    let endpoint = |t: f64| -> Result<FinitePoint3, CompositeCurveError> {
        finite_or_refusal(cadmpeg_ir::eval::nurbs_curve_point_at(&nurbs, t))
            .map_err(CodecError::from)?
            .ok_or(CompositeCurveError::EndpointEvaluation { t })
    };
    endpoint(0.0)?;
    endpoint(cursor)?;
    Ok(Some(ConcatenatedNurbs { nurbs, segments }))
}

fn bounded_edge_for_curve(
    ir: &CadIr,
    curve_id: &CurveId,
    tolerance: f64,
    index: Option<&CompositeIndex>,
) -> Result<Option<CompositeEdge>, CodecError> {
    let curve = match index {
        Some(index) => index
            .curve_positions
            .get(curve_id)
            .and_then(|position| ir.model.curves.get(*position)),
        None => ir.model.curves.iter().find(|curve| curve.id == *curve_id),
    };
    let Some(curve) = curve else {
        return Ok(None);
    };
    let edge_candidates: Cow<'_, [CompositeEdge]> = match index {
        Some(index) => Cow::Borrowed(index.edges.get(curve_id).map_or(&[][..], Vec::as_slice)),
        None => Cow::Owned(
            ir.model
                .edges
                .iter()
                .filter(|edge| edge.curve() == Some(curve_id))
                .map(|edge| CompositeEdge {
                    start: edge.start.clone(),
                    end: edge.end.clone(),
                    param_range: edge.param_range().map(cadmpeg_ir::units::FiniteVector::get),
                })
                .collect(),
        ),
    };
    let Some(geometry) = curve.geometry.solved() else {
        return Ok(None);
    };
    select_composite_edge(ir, index, geometry, &edge_candidates, tolerance)
}

fn bounded_nurbs_for_id(
    ir: &CadIr,
    curve_id: &CurveId,
    depth: usize,
    join_tolerance: Option<f64>,
    ctx: Option<&DecodeContext<'_>>,
    index: Option<&CompositeIndex>,
) -> Result<Option<(NurbsCurve, [f64; 2])>, CompositeCurveError> {
    let _nested = ctx
        .map(|ctx| ctx.enter_nested("iges_composite_flatten"))
        .transpose()?;
    let depth_limit = ctx
        .and_then(|ctx| usize::try_from(ctx.policy().limits.max_recursion_depth).ok())
        .map_or(MAX_COMPOSITE_DEPTH, |policy| {
            policy.min(MAX_COMPOSITE_DEPTH)
        });
    if depth >= depth_limit {
        let requested = depth.saturating_add(1) as u64;
        return Err(CompositeCurveError::Budget(match ctx {
            Some(ctx) => {
                ctx.refuse_codec_limit("iges_composite_depth", depth_limit as u64, requested)
            }
            None => refuse_local_limit("iges_composite_depth", depth_limit as u64, requested),
        }));
    }
    let curve = match index {
        Some(index) => index
            .curve_positions
            .get(curve_id)
            .and_then(|position| ir.model.curves.get(*position)),
        None => ir.model.curves.iter().find(|curve| curve.id == *curve_id),
    };
    let Some(curve) = curve else {
        return Ok(None);
    };
    if let Some(SolvedCurveGeometry::Composite { segments, .. }) = curve.geometry.solved() {
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(
                u64_from_index(segments.len()),
                "iges composite nested children",
            )?;
        }
        let mut children = reserve_admitted_vec(segments.len(), "iges composite nested children")?;
        for segment in segments {
            let Some(child) =
                bounded_nurbs_for_id(ir, &segment.curve, depth + 1, join_tolerance, ctx, index)?
            else {
                return Ok(None);
            };
            let (curve, range) = if segment.same_sense {
                child
            } else {
                reverse_nurbs(child.0, child.1)?
            };
            children.push((curve, range, ()));
        }
        let Some(concatenated) = concatenate_nurbs(ctx, children, join_tolerance)? else {
            return Ok(None);
        };
        let range = [0.0, concatenated.segments.end()];
        return Ok(Some((concatenated.nurbs, range)));
    }
    let Some(edge) = bounded_edge_for_curve(ir, curve_id, join_tolerance.unwrap_or(0.0), index)?
    else {
        return Ok(None);
    };
    let Some(interval) = edge.param_range else {
        return Ok(None);
    };
    let Some(solved) = curve.geometry.solved() else {
        return Ok(None);
    };
    Ok(match solved {
        SolvedCurveGeometry::Nurbs(nurbs) => {
            trim_nurbs_to_interval(ctx, nurbs, interval)?.map(|trimmed| (trimmed, interval))
        }
        SolvedCurveGeometry::Line(_) => {
            let (Some(start), Some(end)) = (
                point_for_vertex(ir, &edge.start, index),
                point_for_vertex(ir, &edge.end, index),
            ) else {
                return Ok(None);
            };
            Some((
                NurbsCurve::from_lanes(1, vec![0.0, 0.0, 1.0, 1.0], vec![start, end], None, false)?,
                [0.0, 1.0],
            ))
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius();
            let Some(mut nurbs) =
                circular_arc_nurbs(center, *axis, *ref_direction, radius, interval)?
            else {
                return Ok(None);
            };
            if anchor_analytic_nurbs_endpoint_poles(
                &mut nurbs,
                interval,
                ir,
                index,
                &edge,
                join_tolerance,
            )?
            .is_none()
            {
                return Ok(None);
            }
            Some((nurbs, interval))
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius();
            let minor_radius = ellipse_curve.minor_radius();
            let Some(mut nurbs) = elliptical_arc_nurbs(
                center,
                *axis,
                *major_direction,
                major_radius,
                minor_radius,
                interval,
            )?
            else {
                return Ok(None);
            };
            if anchor_analytic_nurbs_endpoint_poles(
                &mut nurbs,
                interval,
                ir,
                index,
                &edge,
                join_tolerance,
            )?
            .is_none()
            {
                return Ok(None);
            }
            Some((nurbs, interval))
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let vertex = parabola_curve.vertex().get();
            let axis = parabola_curve.frame().axis().as_raw();
            let major_direction = parabola_curve.frame().reference().as_raw();
            let focal_distance = parabola_curve.focal_distance();
            let Some(mut nurbs) =
                parabolic_arc_nurbs(vertex, *axis, *major_direction, focal_distance, interval)?
            else {
                return Ok(None);
            };
            if anchor_analytic_nurbs_endpoint_poles(
                &mut nurbs,
                interval,
                ir,
                index,
                &edge,
                join_tolerance,
            )?
            .is_none()
            {
                return Ok(None);
            }
            Some((nurbs, interval))
        }
        _ => None,
    })
}

fn bounded_nurbs(
    ir: &CadIr,
    index: &CompositeIndex,
    curve_id: &CurveId,
    join_tolerance: f64,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<(NurbsCurve, [f64; 2])>, CompositeCurveError> {
    bounded_nurbs_for_id(ir, curve_id, 0, Some(join_tolerance), ctx, Some(index))
}

pub(super) fn bounded_nurbs_for_curve(
    ir: &CadIr,
    curve_id: &CurveId,
    ctx: Option<&DecodeContext<'_>>,
    index: Option<&CompositeIndex>,
) -> Result<Option<(NurbsCurve, [f64; 2])>, CompositeCurveError> {
    bounded_nurbs_for_id(ir, curve_id, 0, None, ctx, index)
}

pub(super) fn bounded_parameter_range_for_curve(
    ir: &CadIr,
    curve_id: &CurveId,
    tolerance: f64,
    index: Option<&CompositeIndex>,
) -> Result<Option<[f64; 2]>, CodecError> {
    Ok(bounded_edge_for_curve(ir, curve_id, tolerance, index)?.and_then(|edge| edge.param_range))
}

pub(super) fn bounded_nurbs_for_curve_with_tolerance(
    ir: &CadIr,
    curve_id: &CurveId,
    tolerance: Option<f64>,
    ctx: Option<&DecodeContext<'_>>,
    index: Option<&CompositeIndex>,
) -> Result<Option<(NurbsCurve, [f64; 2])>, CompositeCurveError> {
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
    if !left.is_finite() || !right.is_finite() {
        return false;
    }
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
) -> Result<Option<(FinitePoint3, FinitePoint3)>, CodecError> {
    let Some(curve_position) = index.curve_positions.get(curve_id) else {
        return Ok(None);
    };
    let Some(curve) = ir.model.curves.get(*curve_position) else {
        return Ok(None);
    };
    let Some(candidates) = index.edges.get(curve_id) else {
        return Ok(None);
    };
    let Some(geometry) = curve.geometry.solved() else {
        return Ok(None);
    };
    let edge = select_composite_edge(ir, Some(index), geometry, candidates, tolerance)?;
    Ok(edge.and_then(|edge| {
        Some((
            point_for_vertex(ir, &edge.start, Some(index))?,
            point_for_vertex(ir, &edge.end, Some(index))?,
        ))
    }))
}

fn anchor_analytic_nurbs_endpoint_poles(
    nurbs: &mut NurbsCurve,
    interval: [f64; 2],
    ir: &CadIr,
    index: Option<&CompositeIndex>,
    edge: &CompositeEdge,
    tolerance: Option<f64>,
) -> Result<Option<()>, CodecError> {
    let Some(tolerance) = tolerance else {
        return Ok(Some(()));
    };
    let (Some(start), Some(end)) = (
        point_for_vertex(ir, &edge.start, index),
        point_for_vertex(ir, &edge.end, index),
    ) else {
        return Ok(None);
    };
    let Some(evaluated_start) =
        finite_or_refusal(cadmpeg_ir::eval::nurbs_curve_point_at(nurbs, interval[0]))?
    else {
        return Ok(None);
    };
    let Some(evaluated_end) =
        finite_or_refusal(cadmpeg_ir::eval::nurbs_curve_point_at(nurbs, interval[1]))?
    else {
        return Ok(None);
    };
    if !close_with_tolerance(evaluated_start.get(), start.get(), Some(tolerance))
        || !close_with_tolerance(evaluated_end.get(), end.get(), Some(tolerance))
    {
        return Ok(None);
    }
    let Some(last) = nurbs.pole_count().checked_sub(1) else {
        return Ok(None);
    };
    let mut visited = 0usize;
    Ok(nurbs
        .map_control_points_in_place(|point| {
            let mapped = if visited == last {
                end
            } else if visited == 0 {
                start
            } else {
                point
            };
            visited += 1;
            Ok::<_, ()>(mapped)
        })
        .ok())
}

fn project_native_composite(
    ir: &mut CadIr,
    index: &mut CompositeIndex,
    entry: &DirectoryEntry,
    child_curves: &[CurveId],
    join_tolerance: f64,
    ctx: Option<&DecodeContext<'_>>,
    sequences: &mut super::geometry::SourceSequences,
) -> Result<Option<EdgeId>, CodecError> {
    if child_curves
        .iter()
        .any(|curve_id| !index.curve_positions.contains_key(curve_id))
    {
        return Ok(None);
    }
    let mut endpoints = match ctx {
        Some(ctx) => reserve_vec(ctx, child_curves.len(), "iges composite native endpoints")?,
        None => reserve_admitted_vec(child_curves.len(), "iges composite native endpoints")?,
    };
    for curve_id in child_curves {
        let Some(endpoint) = curve_endpoints(ir, curve_id, index, join_tolerance)? else {
            return Ok(None);
        };
        endpoints.push(endpoint);
    }
    let (Some(start), Some(end)) = (
        endpoints.first().map(|pair| pair.0),
        endpoints.last().map(|pair| pair.1),
    ) else {
        return Ok(None);
    };
    let mut segments = match ctx {
        Some(ctx) => reserve_vec(ctx, child_curves.len(), "iges composite native segments")?,
        None => reserve_admitted_vec(child_curves.len(), "iges composite native segments")?,
    };
    for (position, curve) in child_curves.iter().enumerate() {
        segments.push(CompositeCurveSegment {
            curve: copy_optional_identity(ctx, curve.as_str(), "iges composite native segment curve ids")?,
            same_sense: true,
            transition: if position > 0
                && close_with_tolerance(
                    endpoints[position - 1].1.get(),
                    endpoints[position].0.get(),
                    Some(join_tolerance),
                ) {
                CompositeCurveTransition::Continuous
            } else {
                CompositeCurveTransition::Discontinuous
            },
        });
    }
    let stem = crate::ids::Stem::directory(entry.sequence);
    let start_point = crate::ids::point(&stem.tail(crate::ids::Word::Start));
    sequences.record_point(&start_point, &stem, ctx)?;
    let end_point = crate::ids::point(&stem.tail(crate::ids::Word::End));
    sequences.record_point(&end_point, &stem, ctx)?;
    let start_vertex = crate::ids::vertex(&stem.tail(crate::ids::Word::Start));
    let end_vertex = crate::ids::vertex(&stem.tail(crate::ids::Word::End));
    let curve_id = crate::ids::curve(&stem);
    let edge_id = crate::ids::edge(&stem);
    reserve_optional_vec_growth(ctx, &mut ir.model.points, 2, "iges composite native point slots")?;
    reserve_optional_vec_growth(ctx, &mut ir.model.vertices, 2, "iges composite native vertex slots")?;
    ir.model.points.extend([
        Point::new(start_point.clone(), start, None),
        Point::new(end_point.clone(), end, None),
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
    sequences.record_curve(&curve_id, entry.sequence, ctx)?;
    let Some(segments) = cadmpeg_ir::geometry::CompositeCurveSegments::try_from(segments).ok()
    else {
        return Ok(None);
    };
    let source = match source_object(entry, ctx) {
        Ok(source) => source,
        Err(error) => {
            super::non_resource_error(error)?;
            return Ok(None);
        }
    };
    reserve_optional_vec_growth(ctx, &mut ir.model.curves, 1, "iges composite native curve slots")?;
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments,
            self_intersect: None,
        }),
        source_object: Some(source),
    });
    reserve_optional_vec_growth(ctx, &mut ir.model.edges, 1, "iges composite native edge slots")?;
    ir.model.edges.push(Edge {
        id: edge_id.clone(),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve_id.clone())),
        start: start_vertex.clone(),
        end: end_vertex.clone(),
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
        ctx,
    )?;
    Ok(Some(edge_id))
}

/// The degraded carrier, if one was built, and the loss it charges either way.
#[derive(Clone, Copy)]
struct CompositeCarrier<'a> {
    entry: &'a DirectoryEntry,
    child_curves: &'a [CurveId],
    join_tolerance: f64,
}

fn project_degraded_composite(
    ir: &mut CadIr,
    index: &mut CompositeIndex,
    carrier: CompositeCarrier<'_>,
    reason: &str,
    ctx: Option<&DecodeContext<'_>>,
    sequences: &mut super::geometry::SourceSequences,
    losses: &mut Vec<LossNote>,
) -> Result<Option<EdgeId>, CodecError> {
    let edge = project_native_composite(
        ir,
        index,
        carrier.entry,
        carrier.child_curves,
        carrier.join_tolerance,
        ctx,
        sequences,
    )?;
    if edge.is_some() {
        super::push_optional_attributed_loss(
            ctx, losses, carrier.entry, IgesLossCode::CompositeCarrierDegraded,
            format_args!("IGES Type 102 entity D{} has no admitted concatenated carrier because {reason}; the ordered native composite carrier was retained", carrier.entry.sequence),
        )?;
    } else {
        super::push_optional_entity_loss(
            ctx, losses, carrier.entry,
            format_args!("{reason}, and no ordered native composite carrier can be constructed"),
        )?;
    }
    Ok(edge)
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
    sequences: &mut super::geometry::SourceSequences,
) -> Result<WireProjectionOutcome, CodecError> {
    project_with_type_130_policy(ir, directory, parameters, global, ctx, sequences, false)
}

pub(super) fn project_type_130_children(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
    sequences: &mut super::geometry::SourceSequences,
) -> Result<WireProjectionOutcome, CodecError> {
    project_with_type_130_policy(ir, directory, parameters, global, ctx, sequences, true)
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
    sequences: &mut super::geometry::SourceSequences,
    only_type_130_children: bool,
) -> Result<WireProjectionOutcome, CodecError> {
    let mut records = BTreeMap::new();
    for record in parameters {
        crate::decode_resource::insert_optional_btree_map(
            ctx, &mut records, record.directory_sequence, record,
            "iges composite parameter index",
        )?;
    }
    let mut entries = BTreeMap::new();
    for entry in directory {
        crate::decode_resource::insert_optional_btree_map(
            ctx, &mut entries, entry.sequence, entry,
            "iges composite directory index",
        )?;
    }
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut wire_edges = Vec::new();
    let mut index = CompositeIndex::from_ir(ir, ctx)?;
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
        let Some(use_flag) = entry
            .status
            .use_flag(global.global_table())
            .filter(|use_flag| composite_use_flag_valid(*use_flag, global.global_table()))
        else {
            super::push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{}", "Type 102 Entity Use Flag must be 00 in IGES 4.0"))?;
            continue;
        };
        if !composite_line_font_valid(
            entry.line_font,
            entry.status.hierarchy(),
            global.global_table(),
        ) {
            super::push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{}", "Type 102 Line Font must be nonzero in IGES 4.0 unless Hierarchy is 01"))?;
            continue;
        }
        let Some(record) = records.get(&entry.sequence).copied() else {
            super::push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{}", "Parameter Data record is missing"))?;
            continue;
        };
        let Some(raw_child_count) = record.integer(1) else {
            super::push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{}", "child count is invalid"))?;
            continue;
        };
        if let Some(observed) = u64::try_from(raw_child_count)
            .ok()
            .filter(|count| *count > MAX_COMPOSITE_CHILDREN as u64)
        {
            return Err(refuse_local_limit(
                "iges_composite_children",
                MAX_COMPOSITE_CHILDREN as u64,
                observed,
            ));
        }
        let minimum_child_count = composite_minimum_child_count(global.global_table());
        let Some(child_count) = usize::try_from(raw_child_count)
            .ok()
            .filter(|count| *count >= minimum_child_count)
        else {
            super::push_optional_entity_loss(ctx, &mut losses, entry, format_args!("child count is outside {minimum_child_count}..={MAX_COMPOSITE_CHILDREN}"))?;
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
            super::push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{}", "child pointer list is invalid"))?;
            continue;
        };
        let is_logical_connector = child_sequences.len() == 2
            && child_sequences.iter().all(|sequence| {
                entries
                    .get(sequence)
                    .is_some_and(|child| child.entity_type == 132 && child.form == 0)
            });
        if !composite_logical_connector_use_valid(
            use_flag,
            is_logical_connector,
            global.global_table(),
        ) {
            super::push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{}", "Type 102 logical connectors made of exactly two Type 132 Connect Points require Entity Use Flag 04 in IGES 5.0 and later"))?;
            continue;
        }
        if entry.transform != 0 {
            super::push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{}", "placed composite curves require transformed child-carrier projection"))?;
            continue;
        }
        if child_sequences.iter().any(|sequence| {
            entries.get(sequence).is_none_or(|child| {
                !composite_child_type_allowed(child.entity_type, child.form, global.global_table())
                    || !child.status.is_physically_dependent()
            })
        }) {
            super::push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{}", "composite child is missing, outside the effective specification family, or is not physically dependent"))?;
            continue;
        }
        let point_context = CompositePointContext {
            entries: &entries,
            records: &records,
            global,
            ctx,
            tolerance: join_tolerance,
        };
        let is_curve_sequence = |sequence: &u32| {
            entries.get(sequence).is_none_or(|entry| !composite_point_member(entry))
        };
        let mut curve_carriers = BTreeMap::new();
        for sequence in child_sequences.iter().copied().filter(is_curve_sequence) {
            if let Some(curve_id) = curve_carrier_id(sequence, &entries, &records) {
                crate::decode_resource::insert_optional_btree_map(ctx, &mut curve_carriers, sequence, curve_id, "iges composite child carrier nodes")?;
            }
        }
        if !composite_point_adjacency_valid(
            ir,
            &index,
            &child_sequences,
            &curve_carriers,
            &point_context,
        )? {
            super::push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{}", "point or connect-point adjacency is invalid"))?;
            continue;
        }
        let curve_count = child_sequences.iter().filter(|sequence| is_curve_sequence(sequence)).count();
        let mut curve_sequences = match ctx {
            Some(ctx) => reserve_vec(ctx, curve_count, "iges composite curve child sequences")?,
            None => reserve_admitted_vec(curve_count, "iges composite curve child sequences")?,
        };
        curve_sequences.extend(child_sequences.iter().copied().filter(is_curve_sequence));
        if curve_sequences.is_empty() {
            super::push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{}", "composite has no parameterized curve constituent"))?;
            continue;
        }
        let mut curve_ids = match ctx {
            Some(ctx) => reserve_vec(ctx, curve_sequences.len(), "iges composite child curve ids")?,
            None => reserve_admitted_vec(curve_sequences.len(), "iges composite child curve ids")?,
        };
        let mut missing_curve = false;
        for sequence in &curve_sequences {
            let Some(curve) = curve_carriers.get(sequence) else {
                missing_curve = true;
                break;
            };
            curve_ids.push(copy_optional_identity(ctx, curve.as_str(), "iges composite child curve ID copies")?);
        }
        if missing_curve {
            super::push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{}", "a Type 142 constituent has no valid model-space curve pointer"))?;
            continue;
        }
        let carrier = CompositeCarrier {
            entry,
            child_curves: &curve_ids,
            join_tolerance,
        };
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(
                u64_from_index(curve_ids.len()),
                "iges composite projected children",
            )?;
        }
        let mut children =
            reserve_admitted_vec(curve_ids.len(), "iges composite projected children")?;
        let mut child_refusal = None;
        for curve_id in &curve_ids {
            match bounded_nurbs(ir, &index, curve_id, join_tolerance, ctx) {
                Ok(Some((curve, range))) => children.push((curve, range, copy_optional_identity(ctx, curve_id.as_str(), "iges composite projected child curve IDs")?)),
                Ok(None) => {
                    child_refusal = Some("a child has no bounded line or NURBS carrier".to_owned());
                    break;
                }
                Err(error) => {
                    let error = error.non_resource()?;
                    child_refusal = Some(format!("a child states no curve carrier: {error}"));
                    break;
                }
            }
        }
        if let Some(reason) = child_refusal {
            let edge = project_degraded_composite(
                ir, &mut index, carrier, &reason, ctx, sequences, &mut losses,
            )?;
            if let Some(edge) = edge {
                reserve_optional_vec_growth(ctx, &mut wire_edges, 1, "iges composite wire edge ids")?;
                wire_edges.push(edge);
                crate::decode_resource::insert_optional_btree_set(ctx, &mut decoded, entry.sequence, "iges composite decoded sequences")?;
            }
            continue;
        }
        let concatenated = match concatenate_nurbs(ctx, children, Some(join_tolerance)) {
            Ok(Some(concatenated)) => Some(concatenated),
            Ok(None) => None,
            Err(error) => {
                let error = error.non_resource()?;
                let edge = project_degraded_composite(
                    ir,
                    &mut index,
                    carrier,
                    // The error names its own cause: a carrier the IR
                    // refuses, or a child that does not raise to the
                    // composite degree.
                    &match error {
                        CompositeCurveError::Carrier(error) => {
                            format!("the joined children state no curve carrier: {error}")
                        }
                        CompositeCurveError::Elevation(error) => {
                            format!("a child does not raise to the composite degree: {error}")
                        }
                        error => error.to_string(),
                    },
                    ctx,
                    sequences,
                    &mut losses,
                )?;
                if let Some(edge) = edge {
                    reserve_optional_vec_growth(ctx, &mut wire_edges, 1, "iges composite wire edge ids")?;
                    wire_edges.push(edge);
                    crate::decode_resource::insert_optional_btree_set(ctx, &mut decoded, entry.sequence, "iges composite decoded sequences")?;
                }
                continue;
            }
        };
        let Some(ConcatenatedNurbs { nurbs, segments }) = concatenated else {
            let edge = project_degraded_composite(
                ir,
                &mut index,
                carrier,
                "child endpoints do not join within the Global minimum resolution",
                ctx,
                sequences,
                &mut losses,
            )?;
            if let Some(edge) = edge {
                reserve_optional_vec_growth(ctx, &mut wire_edges, 1, "iges composite wire edge ids")?;
                wire_edges.push(edge);
                crate::decode_resource::insert_optional_btree_set(ctx, &mut decoded, entry.sequence, "iges composite decoded sequences")?;
                continue;
            }
            continue;
        };
        let cursor = segments.end();
        let Some(start) = finite_or_refusal(cadmpeg_ir::eval::nurbs_curve_point_at(&nurbs, 0.0))?
        else {
            let edge = project_degraded_composite(
                ir,
                &mut index,
                carrier,
                "its start cannot be evaluated",
                ctx,
                sequences,
                &mut losses,
            )?;
            if let Some(edge) = edge {
                reserve_optional_vec_growth(ctx, &mut wire_edges, 1, "iges composite wire edge ids")?;
                wire_edges.push(edge);
                crate::decode_resource::insert_optional_btree_set(ctx, &mut decoded, entry.sequence, "iges composite decoded sequences")?;
                continue;
            }
            continue;
        };
        let Some(end) = finite_or_refusal(cadmpeg_ir::eval::nurbs_curve_point_at(&nurbs, cursor))?
        else {
            let edge = project_degraded_composite(
                ir,
                &mut index,
                carrier,
                "its end cannot be evaluated",
                ctx,
                sequences,
                &mut losses,
            )?;
            if let Some(edge) = edge {
                reserve_optional_vec_growth(ctx, &mut wire_edges, 1, "iges composite wire edge ids")?;
                wire_edges.push(edge);
                crate::decode_resource::insert_optional_btree_set(ctx, &mut decoded, entry.sequence, "iges composite decoded sequences")?;
                continue;
            }
            continue;
        };
        let stem = crate::ids::Stem::directory(entry.sequence);
        let start_point = crate::ids::point(&stem.tail(crate::ids::Word::Start));
        sequences.record_point(&start_point, &stem, ctx)?;
        let end_point = crate::ids::point(&stem.tail(crate::ids::Word::End));
        sequences.record_point(&end_point, &stem, ctx)?;
        let start_vertex = crate::ids::vertex(&stem.tail(crate::ids::Word::Start));
        let end_vertex = crate::ids::vertex(&stem.tail(crate::ids::Word::End));
        let curve_id = crate::ids::curve(&stem);
        let edge = crate::ids::edge(&stem);
        reserve_optional_vec_growth(ctx, &mut ir.model.points, 2, "iges composite solved point slots")?;
        reserve_optional_vec_growth(ctx, &mut ir.model.vertices, 2, "iges composite solved vertex slots")?;
        ir.model.points.extend([
            Point::new(start_point.clone(), start, None),
            Point::new(end_point.clone(), end, None),
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
        sequences.record_curve(&curve_id, entry.sequence, ctx)?;
        reserve_optional_vec_growth(ctx, &mut ir.model.curves, 1, "iges composite solved curve slots")?;
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
            source_object: Some(source_object(entry, ctx)?),
        });
        reserve_optional_vec_growth(ctx, &mut ir.model.edges, 1, "iges composite solved edge slots")?;
        ir.model.edges.push(Edge {
            id: edge.clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_id.clone()),
                Some([0.0, cursor]),
            )
            .map_err(CodecError::malformed)?,
            start: start_vertex.clone(),
            end: end_vertex.clone(),
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
            ctx,
        )?;
        let component_count = segments.preceding.len().checked_add(1).ok_or_else(|| refuse_local_limit("iges composite procedural components", u64::MAX, 1))?;
        let boundary_count = component_count.checked_add(1).ok_or_else(|| refuse_local_limit("iges composite procedural boundaries", u64::MAX, 1))?;
        let mut boundaries = match ctx {
            Some(ctx) => reserve_vec(ctx, boundary_count, "iges composite procedural boundaries")?,
            None => reserve_admitted_vec(boundary_count, "iges composite procedural boundaries")?,
        };
        boundaries.push(0.0);
        let mut components = match ctx {
            Some(ctx) => reserve_vec(ctx, component_count, "iges composite procedural components")?,
            None => reserve_admitted_vec(component_count, "iges composite procedural components")?,
        };
        for segment in segments.into_iter() {
            boundaries.push(segment.end);
            components.push(cadmpeg_ir::geometry::CompoundComponent {
                parameter: segment.child_start,
                component: segment.child,
            });
        }
        reserve_optional_vec_growth(ctx, &mut ir.model.procedural_curves, 1, "iges composite procedural curve slots")?;
        let _attached = ir.model.add_procedural_curve(
            curve_id,
            ProceduralCurve::new(
                crate::ids::procedural_curve(&stem),
                ProceduralCurveDefinition::Compound(
                    cadmpeg_ir::geometry::CompoundCurveConstruction::try_new(
                        boundaries, components, None,
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                ),
            ),
        );
        reserve_optional_vec_growth(ctx, &mut wire_edges, 1, "iges composite wire edge ids")?;
        wire_edges.push(edge);
        crate::decode_resource::insert_optional_btree_set(ctx, &mut decoded, entry.sequence, "iges composite decoded sequences")?;
    }

    Ok(WireProjectionOutcome {
        decoded,
        losses,
        wire_edges,
    })
}

#[cfg(test)]
mod tests;
