// SPDX-License-Identifier: Apache-2.0
//! Ordered composite-curve projection.

use super::curve_conversion::{
    circular_arc_nurbs, elliptical_arc_nurbs, parabolic_arc_nurbs, CurveConversionError,
};
use super::geometry::{resolve_transform, source_object, WireProjectionOutcome};
use crate::directory::{DirectoryEntry, Hierarchy, UseFlag};
use crate::global::{GlobalTable, ProjectedGlobal};
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsError, NurbsPoles3, WeightedPole3},
    CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal, PositiveReal};
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const EPS_COMPOSITE_DEGENERATE: f64 = 1.0e-10;

const MAX_COMPOSITE_CHILDREN: usize = 100_000;
const MAX_COMPOSITE_DEGREE: usize = 1024;
const MAX_COMPOSITE_DEPTH: usize = 64;

type HomogeneousNet<'ctx> = (Vec<[f64; 4]>, cadmpeg_core::decode::ScopedReservation<'ctx>);

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
    ctx: &'map DecodeContext<'decode>,
    tolerance: f64,
}

impl CompositePointContext<'_, '_, '_, '_> {
    fn is_point(&self, sequence: u32) -> Result<bool, CodecError> {
        Ok(self
            .ctx
            .get_btree_map(self.entries, &sequence, "iges composite child entry lookup")?
            .is_some_and(|entry| composite_point_member(entry)))
    }

    fn member_point(&self, sequence: u32) -> Result<Option<Point3>, CodecError> {
        let Some(entry) = self
            .ctx
            .get_btree_map(self.entries, &sequence, "iges composite child entry lookup")?
            .copied()
        else {
            return Ok(None);
        };
        if !composite_point_member(entry) {
            return Ok(None);
        }
        let Some(record) = self
            .ctx
            .get_btree_map(
                self.records,
                &sequence,
                "iges composite child parameter lookup",
            )?
            .copied()
        else {
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    index: &CompositeIndex,
    child_sequences: &[u32],
    curve_carriers: &BTreeMap<u32, CurveId>,
    context: &CompositePointContext<'_, '_, '_, '_>,
) -> Result<bool, CodecError> {
    let mut previous_is_point = false;
    let mut next_is_point = None;
    let mut adjacent_children = child_sequences.iter().enumerate();
    while adjacent_children.len() != 0 || ctx.resource_refusal().is_some() {
        let Some((position, sequence)) = ctx.next_charged(&mut adjacent_children, "iges composite point adjacency")? else { break; };
        let is_point = match next_is_point.take() {
            Some(value) => value,
            None => context.is_point(*sequence)?,
        };
        if !is_point {
            previous_is_point = false;
            continue;
        }
        let following_is_point = if position + 1 < child_sequences.len() {
            let value = context.is_point(child_sequences[position + 1])?;
            next_is_point = Some(value);
            value
        } else {
            false
        };
        if child_sequences.len() != 2 && (previous_is_point || following_is_point) {
            return Ok(false);
        }
        let Some(point) = context.member_point(*sequence)? else {
            return Ok(false);
        };
        if position > 0 && !previous_is_point {
            let Some(curve_id) = ctx.get_btree_map(
                curve_carriers,
                &child_sequences[position - 1],
                "iges composite child carrier lookup",
            )?
            else {
                return Ok(false);
            };
            let Some((_, end)) = curve_endpoints(ctx, ir, curve_id, index, context.tolerance)?
            else {
                return Ok(false);
            };
            if !close_with_tolerance(end.get(), point, Some(context.tolerance)) {
                return Ok(false);
            }
        }
        if position + 1 < child_sequences.len() && !following_is_point {
            let Some(curve_id) = ctx.get_btree_map(
                curve_carriers,
                &child_sequences[position + 1],
                "iges composite child carrier lookup",
            )?
            else {
                return Ok(false);
            };
            let Some((start, _)) = curve_endpoints(ctx, ir, curve_id, index, context.tolerance)?
            else {
                return Ok(false);
            };
            if !close_with_tolerance(point, start.get(), Some(context.tolerance)) {
                return Ok(false);
            }
        }
        previous_is_point = true;
    }
    Ok(true)
}

pub(super) fn curve_carrier_id(
    sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<CurveId>, CodecError> {
    let Some(entry) = ctx
        .get_btree_map(entries, &sequence, "iges composite child entry lookup")?
        .copied()
    else {
        return Ok(None);
    };
    let carrier_sequence = if entry.entity_type == 142 && entry.form == 0 {
        // Type 142 is a relationship entity. In a Type 102 constituent its
        // curve geometry is the model-space C pointer; the UV B pointer is
        // not a three-dimensional composite segment. This is the same
        // choice made by OCCT's Curve3D transfer path.
        let Some(carrier_sequence) = ctx
            .get_btree_map(records, &sequence, "iges composite child parameter lookup")?
            .and_then(|record| record.integer(4))
            .and_then(|value| {
                let sequence = u32::try_from(value).ok()?;
                (sequence % 2 == 1).then_some(sequence)
            })
        else {
            return Ok(None);
        };
        carrier_sequence
    } else {
        sequence
    };
    Ok(Some(crate::ids::curve_admitted(
        &crate::ids::Stem::directory(carrier_sequence),
        ctx,
    )?))
}

#[derive(Clone)]
struct CompositeEdge {
    start: VertexId,
    end: VertexId,
    param_range: Option<[f64; 2]>,
}

#[derive(Default)]
#[cfg_attr(test, derive(Clone))]
pub(super) struct CompositeIndex {
    curve_positions: BTreeMap<CurveId, usize>,
    edges: BTreeMap<CurveId, Vec<CompositeEdge>>,
    vertex_points: BTreeMap<VertexId, FinitePoint3>,
    points: BTreeMap<PointId, FinitePoint3>,
    indexed_edges: usize,
    indexed_points: usize,
    indexed_vertices: usize,
}

impl CompositeIndex {
    pub(super) fn from_ir(ir: &CadIr, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(CodecError::from(refusal));
        }
        let mut curve_positions = BTreeMap::new();
        let mut source_index_entries = ir.model.curves.iter().enumerate();
        while source_index_entries.len() != 0 {
            let Some((position, curve)) =
                ctx.next_charged(&mut source_index_entries, "iges composite curve index traversal")?
            else {
                break;
            };
            if !ctx.contains_key_btree_map(
                &curve_positions,
                &curve.id,
                "iges composite curve index lookup",
            )? {
                let key = curve
                    .id
                    .try_clone_for_decode(ctx, "iges composite curve index keys")?;
                ctx.insert_btree_map(
                    &mut curve_positions,
                    key,
                    position,
                    "iges composite curve index nodes",
                )?;
            }
        }
        let mut index = Self {
            curve_positions,
            ..Self::default()
        };
        index.refresh_topology(ir, ctx)?;
        Ok(index)
    }

    /// Extend an index after topology is appended to the same document.
    /// Existing points, vertices and edges keep their identities and values.
    pub(super) fn refresh_topology(
        &mut self,
        ir: &CadIr,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(CodecError::from(refusal));
        }
        if self.indexed_edges > ir.model.edges.len()
            || self.indexed_points > ir.model.points.len()
            || self.indexed_vertices > ir.model.vertices.len()
        {
            return Err(CodecError::malformed(
                "IGES composite topology index source shrank",
            ));
        }
        let mut source_index_entries = ir.model.edges[self.indexed_edges..].iter();
        while source_index_entries.len() != 0 {
            let Some(edge) =
                ctx.next_charged(&mut source_index_entries, "iges composite edge index traversal")?
            else {
                break;
            };
            if let Some(curve) = edge.curve() {
                if !ctx.contains_key_btree_map(
                    &self.edges,
                    curve,
                    "iges composite edge index lookup",
                )? {
                    let key = curve.try_clone_for_decode(ctx, "iges composite edge index keys")?;
                    ctx.insert_btree_map(
                        &mut self.edges,
                        key,
                        Vec::new(),
                        "iges composite edge index nodes",
                    )?;
                }
                let indexed = ctx
                    .get_mut_btree_map(&mut self.edges, curve, "iges composite edge index lookup")?
                    .ok_or_else(|| {
                        CodecError::Malformed("IGES composite edge index is absent".into())
                    })?;
                ctx.reserve_vec(indexed, 1, "iges composite indexed edges")?;
                indexed.push(CompositeEdge {
                    start: edge
                        .start
                        .try_clone_for_decode(ctx, "iges composite indexed start ids")?,
                    end: edge
                        .end
                        .try_clone_for_decode(ctx, "iges composite indexed end ids")?,
                    param_range: edge.param_range().map(cadmpeg_ir::units::FiniteVector::get),
                });
            }
        }
        let mut source_index_entries = ir.model.points[self.indexed_points..].iter();
        while source_index_entries.len() != 0 {
            let Some(point) =
                ctx.next_charged(&mut source_index_entries, "iges composite point index traversal")?
            else {
                break;
            };
            if !ctx.contains_key_btree_map(
                &self.points,
                &point.id,
                "iges composite point index lookup",
            )? {
                let key = point
                    .id
                    .try_clone_for_decode(ctx, "iges composite point index keys")?;
                ctx.insert_btree_map(
                    &mut self.points,
                    key,
                    point.position(),
                    "iges composite point index nodes",
                )?;
            }
        }
        let mut source_index_entries = ir.model.vertices[self.indexed_vertices..].iter();
        while source_index_entries.len() != 0 {
            let Some(vertex) =
                ctx.next_charged(&mut source_index_entries, "iges composite vertex index traversal")?
            else {
                break;
            };
            if let Some(point) = ctx
                .get_btree_map(
                    &self.points,
                    &vertex.point,
                    "iges composite point index lookup",
                )?
                .copied()
            {
                if !ctx.contains_key_btree_map(
                    &self.vertex_points,
                    &vertex.id,
                    "iges composite vertex index lookup",
                )? {
                    let key = vertex
                        .id
                        .try_clone_for_decode(ctx, "iges composite vertex index keys")?;
                    ctx.insert_btree_map(
                        &mut self.vertex_points,
                        key,
                        point,
                        "iges composite vertex index nodes",
                    )?;
                }
            }
        }
        self.indexed_edges = ir.model.edges.len();
        self.indexed_points = ir.model.points.len();
        self.indexed_vertices = ir.model.vertices.len();
        Ok(())
    }

    pub(super) fn curve_by_id<'a>(
        &self,
        ir: &'a CadIr,
        curve_id: &CurveId,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<&'a Curve>, CodecError> {
        Ok(ctx
            .get_btree_map(
                &self.curve_positions,
                curve_id,
                "iges composite curve lookup",
            )?
            .and_then(|position| ir.model.curves.get(*position)))
    }

    fn add_model_entity(
        &mut self,
        curve_id: &CurveId,
        curve_index: usize,
        edge: CompositeEdge,
        endpoints: [(VertexId, FinitePoint3); 2],
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        let position_key =
            curve_id.try_clone_for_decode(ctx, "iges composite added curve index key")?;
        ctx.insert_btree_map(
            &mut self.curve_positions,
            position_key,
            curve_index,
            "iges composite added curve index node",
        )?;
        if !ctx.contains_key_btree_map(&self.edges, curve_id, "iges composite edge index lookup")? {
            let edge_key =
                curve_id.try_clone_for_decode(ctx, "iges composite added edge index key")?;
            ctx.insert_btree_map(
                &mut self.edges,
                edge_key,
                Vec::new(),
                "iges composite added edge index node",
            )?;
        }
        let indexed = ctx
            .get_mut_btree_map(
                &mut self.edges,
                curve_id,
                "iges composite edge index lookup",
            )?
            .ok_or_else(|| {
                CodecError::Malformed("IGES composite added edge index is absent".into())
            })?;
        ctx.reserve_vec(indexed, 1, "iges composite added edge slots")?;
        indexed.push(edge);
        for (vertex, point) in endpoints {
            ctx.insert_btree_map(
                &mut self.vertex_points,
                vertex,
                point,
                "iges composite added vertex index nodes",
            )?;
        }
        Ok(())
    }
}

fn point_for_vertex(
    ir: &CadIr,
    id: &VertexId,
    index: Option<&CompositeIndex>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<FinitePoint3>, CodecError> {
    if let Some(index) = index {
        return Ok(ctx
            .get_btree_map(&index.vertex_points, id, "iges composite vertex lookup")?
            .copied());
    }
    let Some(vertex) = ctx.find_by(
        &ir.model.vertices,
        |vertex| ctx.equal(&vertex.id, id, "iges composite vertex identity comparison"),
        "iges composite vertex search",
    )?
    else {
        return Ok(None);
    };
    Ok(ctx
        .find_by(
            &ir.model.points,
            |point| {
                ctx.equal(
                    &point.id,
                    &vertex.point,
                    "iges composite point identity comparison",
                )
            },
            "iges composite point search",
        )?
        .map(Point::position))
}

fn composite_edge_endpoints_agree(
    ir: &CadIr,
    index: Option<&CompositeIndex>,
    left: &CompositeEdge,
    right: &CompositeEdge,
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    Ok(
        match (
            point_for_vertex(ir, &left.start, index, ctx)?,
            point_for_vertex(ir, &right.start, index, ctx)?,
            point_for_vertex(ir, &left.end, index, ctx)?,
            point_for_vertex(ir, &right.end, index, ctx)?,
        ) {
            (Some(left_start), Some(right_start), Some(left_end), Some(right_end)) => {
                // GE-05: the MUR boundary is excluded; zero still means exact equality.
                close_with_tolerance(left_start.get(), right_start.get(), Some(tolerance))
                    && close_with_tolerance(left_end.get(), right_end.get(), Some(tolerance))
            }
            (None, None, None, None) => true,
            _ => false,
        },
    )
}

fn select_composite_edge(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    index: Option<&CompositeIndex>,
    geometry: &SolvedCurveGeometry,
    candidates: &[CompositeEdge],
    tolerance: f64,
) -> Result<Option<CompositeEdge>, CodecError> {
    let mut first: Option<&CompositeEdge> = None;
    let mut agreement = true;
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(candidates);
    while source_values.len() != 0 {
        let Some(edge) = ctx.next_charged(&mut source_values, "iges composite edge candidates")? else {
            break;
        };
        let Some(range) = edge.param_range else {
            continue;
        };
        if matches!(geometry, SolvedCurveGeometry::Line(_)) {
            let (Some(start), Some(end)) = (
                point_for_vertex(ir, &edge.start, index, ctx)?,
                point_for_vertex(ir, &edge.end, index, ctx)?,
            ) else {
                continue;
            };
            let Some(evaluated_start) =
                finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::curve_point_solved(ctx, geometry, range[0]),
                )?)?
            else {
                continue;
            };
            let Some(evaluated_end) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::curve_point_solved(ctx, geometry, range[1]),
            )?)?
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
                && composite_edge_endpoints_agree(ir, index, edge, previous, tolerance, ctx)?;
        } else {
            first = Some(edge);
        }
    }
    first
        .filter(|_| agreement)
        .map(|edge| {
            Ok(CompositeEdge {
                start: edge
                    .start
                    .try_clone_for_decode(ctx, "iges composite endpoint identity")?,
                end: edge
                    .end
                    .try_clone_for_decode(ctx, "iges composite endpoint identity")?,
                param_range: edge.param_range,
            })
        })
        .transpose()
}

fn homogeneous_point_is_valid(point: &[f64; 4]) -> bool {
    point.iter().all(|value| value.is_finite()) && point[0] > 0.0
}

fn homogeneous_control_points<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    curve: &NurbsCurve,
) -> Result<Option<HomogeneousNet<'ctx>>, CodecError> {
    let control_count = curve.pole_count();
    let storage;
    let (mut homogeneous, result_storage) =
        ctx.temporary_vec(control_count, "iges composite homogeneous control points")?;
    storage = result_storage;
    let mut homogeneous_indices = 0..control_count;
    while !homogeneous_indices.is_empty() || ctx.resource_refusal().is_some() {
        let Some(index) = ctx.next_charged(
        &mut homogeneous_indices,
        "iges composite homogeneous control traversal",
    )? else { break; };
        let Some(point) = curve.pole_rows().point_at(index) else {
            return Ok(None);
        };
        let weight = if matches!(curve.pole_rows(), NurbsPoles3::Rational { .. }) {
            let Some(weight) = curve.pole_rows().weight_at(index) else {
                return Ok(None);
            };
            weight
        } else {
            1.0
        };
        let homogeneous_point = [weight, weight * point.x, weight * point.y, weight * point.z];
        if !homogeneous_point_is_valid(&homogeneous_point) {
            return Ok(None);
        }
        homogeneous.push(homogeneous_point);
    }
    Ok(Some((homogeneous, storage)))
}

struct EuclideanControlNet<'ctx> {
    control_points: Vec<FinitePoint3>,
    weights: Option<Vec<NonZeroReal>>,
    _storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
}

fn euclidean_control_points<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    homogeneous: &[[f64; 4]],
    rational: bool,
) -> Result<Option<EuclideanControlNet<'ctx>>, CodecError> {
    let mut storage = if rational {
        Some(ctx.reserve_scoped(0, "iges composite Euclidean lane storage")?)
    } else {
        None
    };
    let build = || -> Result<_, CodecError> {
        Ok((
            ctx.collection_vec(homogeneous.len(), "iges composite Euclidean control points")?,
            if rational {
                Some(ctx.collection_vec(homogeneous.len(), "iges composite Euclidean weights")?)
            } else {
                None
            },
        ))
    };
    let (mut control_points, mut weights) = match &mut storage {
        Some(storage) => storage.with_storage(build)?,
        None => build()?,
    };
    let mut euclidean_poles = homogeneous.iter();
    while euclidean_poles.len() != 0 || ctx.resource_refusal().is_some() {
        let Some(&[weight, x, y, z]) = ctx.next_charged(
        &mut euclidean_poles,
        "iges composite Euclidean control traversal",
    )? else { break; };
        let Some(weight) = PositiveReal::new(weight) else {
            return Ok(None);
        };
        let point = Point3::new(x / weight.get(), y / weight.get(), z / weight.get());
        let Some(point) = FinitePoint3::new(point) else {
            return Ok(None);
        };
        control_points.push(point);
        if let Some(weights) = &mut weights {
            weights.push(NonZeroReal::from(weight));
        }
    }
    Ok(Some(EuclideanControlNet {
        control_points,
        weights,
        _storage: storage,
    }))
}

fn elevate_bezier_homogeneous<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    control_points: &[[f64; 4]],
    source_degree: usize,
    target_degree: usize,
) -> Result<Option<HomogeneousNet<'ctx>>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(source_count) = source_degree.checked_add(1) else {
        return Ok(None);
    };
    if control_points.len() != source_count || target_degree < source_degree {
        return Ok(None);
    }
    if ctx.any_by(
        control_points,
        |point| Ok(!homogeneous_point_is_valid(point)),
        "iges composite Bezier valid controls",
    )? {
        return Ok(None);
    }
    let mut storage;
    let (mut elevated, result_storage) =
        ctx.copy_temporary_slice(control_points, "iges composite Bezier source copy")?;
    storage = result_storage;
    let mut degree = source_degree;
    let mut elevation_degrees = source_degree..target_degree;
    while (!elevation_degrees.is_empty() || ctx.resource_refusal().is_some()) && ctx
        .next_charged(
            &mut elevation_degrees,
            "iges composite Bezier degree elevation",
        )?
        .is_some() {
        let Some(next_degree) = degree.checked_add(1) else {
            return Ok(None);
        };
        let Some(next_count) = next_degree.checked_add(1) else {
            return Ok(None);
        };
        let next_storage;
        let (mut next, result_next_storage) =
            ctx.temporary_vec(next_count, "iges composite Bezier elevated net")?;
        next_storage = result_next_storage;
        next.push(elevated[0]);
        let mut elevated_indices = 1..=degree;
        while !elevated_indices.is_empty() || ctx.resource_refusal().is_some() {
            let Some(index) = ctx.next_charged(
            &mut elevated_indices,
            "iges composite Bezier elevated controls",
        )? else { break; };
            let Some(index_real) = cadmpeg_core::convert::f64_from_index(index) else {
                return Ok(None);
            };
            let Some(degree_real) = cadmpeg_core::convert::f64_from_index(next_degree) else {
                return Ok(None);
            };
            let alpha = index_real / degree_real;
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
        storage = next_storage;
        degree = next_degree;
    }
    Ok(Some((elevated, storage)))
}

#[derive(Debug)]
struct ConcatenatedNurbs<'ctx, T> {
    nurbs: NurbsCurve,
    endpoints: [FinitePoint3; 2],
    segments: ConcatenatedSegments<'ctx, T>,
}

#[derive(Debug)]
struct ConcatenatedSegment<T> {
    child_start: f64,
    end: f64,
    child: T,
}

#[derive(Debug)]
struct ConcatenatedSegments<'ctx, T> {
    preceding: Vec<ConcatenatedSegment<T>>,
    last: ConcatenatedSegment<T>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<T> ConcatenatedSegments<'_, T> {
    fn end(&self) -> f64 {
        self.last.end
    }
}

/// Reflects a child about its own parameter domain.
///
/// Every answer here is either the reversed child or a named cause: the values
/// read are the child's own stated degree, knots and interval, so a reader that
/// cannot reflect them states which of them it refused.
fn reverse_nurbs(
    ctx: &DecodeContext<'_>,
    curve: NurbsCurve,
    interval: [f64; 2],
) -> Result<(NurbsCurve, [f64; 2]), CompositeCurveError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Ok(degree) = usize::try_from(curve.degree()) else {
        return Err(CompositeCurveError::ReversedChildDegree {
            degree: curve.degree(),
        });
    };
    let control_count = curve.pole_count();
    let [start, end] = interval;
    let (Some(finite_start), Some(finite_end)) = (FiniteReal::new(start), FiniteReal::new(end))
    else {
        return Err(CompositeCurveError::ReversedChildInterval { start, end });
    };
    if start > end {
        return Err(CompositeCurveError::ReversedChildInterval { start, end });
    }
    let (domain_start, domain_end) = (curve.knots()[degree], curve.knots()[control_count]);
    let (Some(lower), Some(upper)) = (FiniteReal::new(domain_start), FiniteReal::new(domain_end))
    else {
        return Err(CompositeCurveError::ReversedChildReflectionNonFinite {
            domain_start,
            domain_end,
        });
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
    ctx.reverse(&mut knots, "iges reversed NURBS knot reversal")?;
    let mut knot_iter = knots.iter_mut();
    while knot_iter.len() != 0 || ctx.resource_refusal().is_some() {
        let Some(knot) = ctx.next_charged(&mut knot_iter, "iges reversed NURBS knot reflection")? else { break; };
        let finite = FiniteReal::new(*knot).ok_or(
            CompositeCurveError::ReversedChildReflectionNonFinite {
                domain_start,
                domain_end,
            },
        )?;
        *knot = reflect(finite)?;
    }
    match &mut poles {
        NurbsPoles3::Polynomial { points } => {
            ctx.reverse(points, "iges reversed NURBS pole reversal")?;
        }
        NurbsPoles3::Rational { points } => {
            ctx.reverse(points, "iges reversed NURBS pole reversal")?;
        }
    }
    let reversed = NurbsCurve::new(ctx, source_degree, knots, poles, periodic)??;
    Ok((reversed, reversed_range))
}

#[derive(Debug)]
struct InsertedKnotNet<'ctx> {
    control_points: Vec<[f64; 4]>,
    knots: Vec<f64>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn insert_homogeneous_knot<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    control_points: &[[f64; 4]],
    knots: &[f64],
    degree: usize,
    value: f64,
) -> Result<Option<InsertedKnotNet<'ctx>>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let control_count = control_points.len();
    let Some(last_control) = control_count.checked_sub(1) else {
        return Ok(None);
    };
    let upper = ctx.partition_point(
        knots,
        |knot| Ok(*knot <= value),
        "iges composite insertion knot bounds",
    )?;
    let Some(span) = upper.checked_sub(1) else {
        return Ok(None);
    };
    let span = if degree == 0 {
        span.min(last_control)
    } else {
        span
    };
    let lower = ctx.partition_point(
        &knots[..upper],
        |knot| Ok(*knot < value),
        "iges composite insertion knot bounds",
    )?;
    let multiplicity = upper - lower;
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
    let Some(inserted_count) = control_count.checked_add(1) else {
        return Ok(None);
    };
    let Some(tail_start) = span.checked_sub(multiplicity) else {
        return Ok(None);
    };
    if tail_start > last_control {
        return Ok(None);
    }
    let Some(left_knots) = knots.get(..=span) else {
        return Ok(None);
    };
    let Some(right_knots) = knots.get(span + 1..) else {
        return Ok(None);
    };
    let mut storage;
    let (mut inserted_knots, result_storage) =
        ctx.temporary_vec(knot_count, "iges composite inserted knots")?;
    storage = result_storage;
    inserted_knots.extend(
        ctx.admit_iter(left_knots, "iges composite inserted knot prefix")?
            .copied(),
    );
    inserted_knots.push(value);
    inserted_knots.extend(
        ctx.admit_iter(right_knots, "iges composite inserted knot suffix")?
            .copied(),
    );
    let mut inserted_control_points = storage.with_storage(|| {
        ctx.collection_vec(
            inserted_count,
            "iges composite knot-insertion control points",
        )
    })?;
    let mut inserted_indices = 0..inserted_count;
    while !inserted_indices.is_empty() || ctx.resource_refusal().is_some() {
        let Some(index) = ctx.next_charged(&mut inserted_indices, "iges composite inserted controls")? else { break; };
        let point = if index <= left_end {
            control_points[index]
        } else if index > tail_start {
            control_points[index - 1]
        } else {
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
            let point = std::array::from_fn(|coordinate| {
                alpha * current[coordinate] + (1.0 - alpha) * previous[coordinate]
            });
            if !homogeneous_point_is_valid(&point) {
                return Ok(None);
            }
            point
        };
        inserted_control_points.push(point);
    }
    Ok(Some(InsertedKnotNet {
        control_points: inserted_control_points,
        knots: inserted_knots,
        storage,
    }))
}

/// Trim a curve to `interval`. `Ok(None)` states an interval the curve cannot
/// be trimmed to; `Err` states trimmed lanes the carrier refuses.
fn trim_nurbs_to_interval(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    interval: [f64; 2],
) -> Result<Option<NurbsCurve>, CompositeCurveError> {
    let _lane_storage;
    let Some((control_points, weights, trimmed_knots, result_lane_storage)) =
        trim_nurbs_lanes(ctx, curve, interval)?
    else {
        return Ok(None);
    };
    _lane_storage = result_lane_storage;
    Ok(Some(NurbsCurve::from_checked_lanes(
        ctx,
        curve.degree(),
        trimmed_knots,
        control_points,
        weights,
        false,
    )??))
}

type TrimmedLanes<'ctx> = (
    Vec<FinitePoint3>,
    Option<Vec<NonZeroReal>>,
    Vec<f64>,
    Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
);

fn trim_nurbs_lanes<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    curve: &NurbsCurve,
    interval: [f64; 2],
) -> Result<Option<TrimmedLanes<'ctx>>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Ok(degree) = usize::try_from(curve.degree()) else {
        return Ok(None);
    };
    let control_count = curve.pole_count();
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
    let mut net_storage;
    let Some((mut homogeneous, result_net_storage)) = homogeneous_control_points(ctx, curve)? else {
        return Ok(None);
    };
    net_storage = result_net_storage;
    let mut knots = net_storage
        .with_storage(|| ctx.copy_slice(curve.knots(), "iges composite trim knot copy"))?;
    for value in [start, end] {
        let Some(target_multiplicity) = degree.checked_add(1) else {
            return Ok(None);
        };
        let lower = ctx.partition_point(
            &knots,
            |knot| Ok(*knot < value),
            "iges composite trim knot multiplicity",
        )?;
        let upper = ctx.partition_point(
            &knots,
            |knot| Ok(*knot <= value),
            "iges composite trim knot multiplicity",
        )?;
        let mut trim_insertions = upper - lower..target_multiplicity;
        while (!trim_insertions.is_empty() || ctx.resource_refusal().is_some()) && ctx
            .next_charged(&mut trim_insertions, "iges composite trim knot insertions")?
            .is_some() {
            let Some(InsertedKnotNet {
                control_points: new_homogeneous,
                knots: new_knots,
                storage,
            }) = insert_homogeneous_knot(ctx, &homogeneous, &knots, degree, value)?
            else {
                return Ok(None);
            };
            homogeneous = new_homogeneous;
            knots = new_knots;
            drop(std::mem::replace(&mut net_storage, storage));
        }
    }
    let (Some(start_knot), Some(end_knot)) = (
        ctx.position_by(
            &knots,
            |knot| Ok(*knot == start),
            "iges composite trim start knot",
        )?,
        ctx.rposition_by(
            &knots,
            |knot| Ok(*knot == end),
            "iges composite trim end knot",
        )?,
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
    let Some(expected_knots) = homogeneous_slice
        .len()
        .checked_add(degree)
        .and_then(|count| count.checked_add(1))
    else {
        return Ok(None);
    };
    if knot_slice.len() != expected_knots {
        return Ok(None);
    }
    let trimmed_knots = ctx.copy_slice(knot_slice, "iges composite trimmed knots")?;
    let lane_storage;
    let Some(EuclideanControlNet {
        control_points,
        weights,
        _storage: result_lane_storage,
    }) = euclidean_control_points(
        ctx,
        homogeneous_slice,
        matches!(curve.pole_rows(), NurbsPoles3::Rational { .. }),
    )?
    else {
        return Ok(None);
    };
    lane_storage = result_lane_storage;
    Ok(Some((control_points, weights, trimmed_knots, lane_storage)))
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
    /// A bounded analytic carrier could not be built.
    #[error(transparent)]
    Conversion(#[from] CurveConversionError),
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

impl From<cadmpeg_core::decode::ResourceLimit> for CompositeCurveError {
    fn from(limit: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Budget(limit.into())
    }
}

impl CompositeCurveError {
    /// Return a decode resource refusal before a caller considers geometric fallback.
    pub(super) fn non_resource(self) -> Result<Self, CodecError> {
        match self {
            Self::Budget(error)
            | Self::ChildWeightAllocation(error)
            | Self::Conversion(CurveConversionError::Resource(error)) => Err(error),
            Self::Elevation(DegreeElevationError::Allocation(error)) => Err(error),
            error => Ok(error),
        }
    }
}

fn elevate_nurbs_to_degree(
    ctx: &DecodeContext<'_>,
    curve: &mut NurbsCurve,
    interval: [f64; 2],
    target_degree: u32,
    join_tolerance: Option<f64>,
) -> Result<(), CompositeCurveError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
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
    let boundary_multiplicity = |value: f64| -> Result<usize, CodecError> {
        let lower = ctx.partition_point(
            curve.knots(),
            |knot| Ok(*knot < value),
            "iges composite boundary knot multiplicity",
        )?;
        let upper = ctx.partition_point(
            curve.knots(),
            |knot| Ok(*knot <= value),
            "iges composite boundary knot multiplicity",
        )?;
        Ok(upper - lower)
    };
    for knot in interval {
        let multiplicity = boundary_multiplicity(knot)?;
        if multiplicity != source_degree + 1 {
            return Err(DegreeElevationError::BoundaryMultiplicity {
                knot,
                multiplicity,
                expected: source_degree + 1,
            }
            .into());
        }
    }
    let mut net_storage;
    let Some((mut homogeneous, result_net_storage)) =
        homogeneous_control_points(ctx, curve).map_err(DegreeElevationError::Allocation)?
    else {
        return Err(DegreeElevationError::HomogeneousControlNet.into());
    };
    net_storage = result_net_storage;
    let mut knots = net_storage
        .with_storage(|| ctx.copy_slice(curve.knots(), "iges composite elevation knot copy"))
        .map_err(DegreeElevationError::Allocation)?;
    let mut internal_storage = ctx.reserve_scoped(0, "iges composite internal knot storage")?;
    let mut internal_values = Vec::new();
    let mut source_values = IntoIterator::into_iter(&knots);
    while source_values.len() != 0 {
        let Some(&knot) = ctx.next_charged(&mut source_values, "iges composite internal knot traversal")? else {
            break;
        };
        if knot > interval[0] && knot < interval[1] && internal_values.last().copied() != Some(knot)
        {
            ctx.reserve_scoped_vec(
                &mut internal_storage,
                &mut internal_values,
                1,
                "iges composite internal knot values",
            )
            .map_err(DegreeElevationError::Allocation)?;
            internal_values.push(knot);
        }
    }
    let mut internal_knots = internal_values.into_iter();
    while internal_knots.len() != 0 || ctx.resource_refusal().is_some() {
        let Some(value) = ctx.next_charged(&mut internal_knots, "iges composite internal values")? else { break; };
        let lower = ctx.partition_point(
            &knots,
            |knot| Ok(*knot < value),
            "iges composite internal knot multiplicity",
        )?;
        let upper = ctx.partition_point(
            &knots,
            |knot| Ok(*knot <= value),
            "iges composite internal knot multiplicity",
        )?;
        let multiplicity = upper - lower;
        if multiplicity > source_degree + 1 {
            return Err(DegreeElevationError::InternalMultiplicity {
                knot: value,
                multiplicity,
                bound: source_degree + 1,
            }
            .into());
        }
        let mut elevation_insertions = multiplicity..source_degree;
        while (!elevation_insertions.is_empty() || ctx.resource_refusal().is_some()) && ctx
            .next_charged(
                &mut elevation_insertions,
                "iges composite elevation knot insertions",
            )?
            .is_some() {
            let Some(InsertedKnotNet {
                control_points: new_points,
                knots: new_knots,
                storage,
            }) = insert_homogeneous_knot(ctx, &homogeneous, &knots, source_degree, value)
                .map_err(DegreeElevationError::Allocation)?
            else {
                return Err(DegreeElevationError::KnotInsertion { knot: value }.into());
            };
            homogeneous = new_points;
            knots = new_knots;
            drop(std::mem::replace(&mut net_storage, storage));
        }
    }
    drop(internal_knots);
    drop(internal_storage);
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
    let rational = matches!(curve.pole_rows(), NurbsPoles3::Rational { .. });
    let mut piece_storage = ctx.reserve_scoped(0, "iges composite elevated span storage")?;
    let mut pieces = Vec::new();
    let mut elevation_spans = source_degree..=refined_count;
    while !elevation_spans.is_empty() || ctx.resource_refusal().is_some() {
        let Some(span) = ctx.next_charged(&mut elevation_spans, "iges composite elevation spans")? else { break; };
        let start = knots[span];
        let end = knots[span + 1];
        if !start.is_finite() || !end.is_finite() || start >= end {
            continue;
        }
        let Some(source_points) = homogeneous.get(span - source_degree..=span) else {
            return Err(DegreeElevationError::SpanControlNet { span }.into());
        };
        let _elevated_storage;
        let Some((elevated, result_elevated_storage)) =
            elevate_bezier_homogeneous(ctx, source_points, source_degree, target_degree)
                .map_err(DegreeElevationError::Allocation)?
        else {
            return Err(DegreeElevationError::SpanElevation {
                span,
                degree: target_degree,
            }
            .into());
        };
        _elevated_storage = result_elevated_storage;
        let _lane_storage;
        let Some(EuclideanControlNet {
            control_points,
            weights,
            _storage: result_lane_storage,
        }) = piece_storage
            .with_storage(|| euclidean_control_points(ctx, &elevated, rational))
            .map_err(DegreeElevationError::Allocation)?
        else {
            return Err(DegreeElevationError::SpanEuclideanNet { span }.into());
        };
        _lane_storage = result_lane_storage;
        let Some(target_knot_count) = target_degree.checked_add(1) else {
            return Err(DegreeElevationError::TargetDegree {
                degree: stated_target,
                bound: MAX_COMPOSITE_DEGREE,
            }
            .into());
        };
        let mut piece_knots = piece_storage
            .with_storage(|| {
                ctx.alloc_filled(target_knot_count, start, "iges composite elevated knots")
            })
            .map_err(DegreeElevationError::Allocation)?;
        piece_storage
            .with_storage(|| {
                ctx.reserve_vec(
                    &mut piece_knots,
                    target_knot_count,
                    "iges composite elevated knot suffix",
                )
            })
            .map_err(DegreeElevationError::Allocation)?;
        let complete_knot_count = piece_knots
            .len()
            .checked_add(target_knot_count)
            .ok_or_else(|| {
                DegreeElevationError::Allocation(ctx.refuse_codec_limit(
                    "iges composite elevated knot suffix",
                    u64::MAX,
                    1,
                ))
            })?;
        piece_knots.extend(
            ctx.admit_iter(
                0..complete_knot_count - piece_knots.len(),
                "iges composite elevated knot suffix fill",
            )?
            .map(|_| end),
        );
        ctx.reserve_scoped_vec(
            &mut piece_storage,
            &mut pieces,
            1,
            "iges composite elevated span",
        )
        .map_err(DegreeElevationError::Allocation)?;
        let piece_degree =
            u32::try_from(target_degree).map_err(|_| DegreeElevationError::TargetDegree {
                degree: stated_target,
                bound: MAX_COMPOSITE_DEGREE,
            })?;
        let piece = piece_storage
            .with_storage(|| {
                NurbsCurve::from_checked_lanes(
                    ctx,
                    piece_degree,
                    piece_knots,
                    control_points,
                    weights,
                    false,
                )
            })
            .map_err(DegreeElevationError::Allocation)??;
        pieces.push((piece, [start, end], ()));
    }
    let Some(concatenated) = concatenate_nurbs(ctx, pieces, join_tolerance)? else {
        return Err(DegreeElevationError::SpansDoNotJoin.into());
    };
    let (elevated_degree, admitted_knots, poles, _) = concatenated.nurbs.into_parts();
    let mut elevated_knots = admitted_knots.into_values();
    for knot in ctx.admit_iter(
        &mut elevated_knots,
        "iges composite elevated knot translation",
    )? {
        *knot += interval[0];
    }
    // Translation must preserve every copy of each clamped source endpoint.
    // Adding the origin back can round past the declared endpoint.
    ctx.fill(
        &mut elevated_knots[..=target_degree],
        interval[0],
        "iges composite elevated start knots",
    )?;
    let end_start = elevated_knots.len() - target_degree - 1;
    ctx.fill(
        &mut elevated_knots[end_start..],
        interval[1],
        "iges composite elevated end knots",
    )?;
    let elevated = NurbsCurve::new(ctx, elevated_degree, elevated_knots, poles, false)
        .map_err(DegreeElevationError::Allocation)??;
    *curve = elevated;
    Ok(())
}

fn concatenate_nurbs<'ctx, T>(
    ctx: &'ctx DecodeContext<'_>,
    children: Vec<(NurbsCurve, [f64; 2], T)>,
    join_tolerance: Option<f64>,
) -> Result<Option<ConcatenatedNurbs<'ctx, T>>, CompositeCurveError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(first) = children.first() else {
        return Err(CompositeCurveError::EmptyChildList);
    };
    let degree = ctx
        .admit_iter(&children, "iges composite child degree traversal")?
        .map(|(curve, _, _)| curve.degree())
        .fold(first.0.degree(), u32::max);
    let mut elevation_storage = ctx.reserve_scoped(0, "iges composite elevated child storage")?;
    let mut children = children;
    let mut children_to_elevate = children.iter_mut();
    while children_to_elevate.len() != 0 || ctx.resource_refusal().is_some() {
        let Some((curve, interval, _)) = ctx.next_charged(
        &mut children_to_elevate,
        "iges composite child elevation traversal",
    )? else { break; };
        if curve.degree() < degree {
            elevation_storage.with_storage(|| {
                elevate_nurbs_to_degree(ctx, curve, *interval, degree, join_tolerance)
            })?;
        }
    }
    let mut child_intervals = children.iter().enumerate();
    while child_intervals.len() != 0 || ctx.resource_refusal().is_some() {
        let Some((child, (curve, interval, _))) = ctx.next_charged(
        &mut child_intervals,
        "iges composite child interval traversal",
    )? else { break; };
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
    let degree_usize = cadmpeg_core::decode::index_from_u32(degree);
    let prepare_child = |(curve, interval, child): (NurbsCurve, [f64; 2], T),
                         cursor: f64|
     -> Result<_, CompositeCurveError> {
        let child_start = interval[0];
        let child_end = interval[1];
        let (_, admitted_knots, poles, _) = curve.into_parts();
        let mut shifted_knots = admitted_knots.into_values();
        for knot in ctx.admit_iter(&mut shifted_knots, "iges composite child knot translation")? {
            *knot = (*knot - child_start) + cursor;
        }
        if let NurbsPoles3::Rational { points } = &poles {
            if let Some(pole) = ctx.find_by(
                points,
                |pole| Ok(pole.weight.get() <= 0.0),
                "iges composite child positive weights",
            )? {
                return Err(CompositeCurveError::ChildWeight {
                    weight: pole.weight.get(),
                });
            }
        }
        let end = cursor + (child_end - child_start);
        if !end.is_finite() {
            return Err(CompositeCurveError::ChildEndParameter { end });
        }
        Ok((
            shifted_knots,
            poles,
            ConcatenatedSegment {
                child_start,
                end,
                child,
            },
        ))
    };
    let child_count = children.len();
    let mut children = children.into_iter();
    let first = ctx
        .next_charged(&mut children, "iges composite child lane traversal")?
        .ok_or(CompositeCurveError::EmptyChildList)?;
    let mut lane_storage;
    let (mut knots, first_poles, last) = prepare_child(first, 0.0)?;
    lane_storage = ctx.reserve_scoped(0, "iges composite joined lane storage")?;
    let mut weight_storage = ctx.reserve_scoped(0, "iges composite weight storage")?;
    let (mut control_points, mut weights) = match first_poles {
        NurbsPoles3::Polynomial { points } => {
            let weights = weight_storage
                .with_storage(|| {
                    ctx.alloc_filled(points.len(), 1.0, "iges composite child weights")
                })
                .map_err(CompositeCurveError::ChildWeightAllocation)?;
            (points, weights)
        }
        NurbsPoles3::Rational { points } => {
            let mut controls = lane_storage.with_storage(|| {
                ctx.collection_vec(points.len(), "iges composite child control points")
            })?;
            let mut weights = weight_storage.with_storage(|| {
                ctx.collection_vec(points.len(), "iges composite child weight copy")
            })?;
            for pole in ctx.admit_iter(points, "iges composite child pole conversion")? {
                controls.push(pole.point);
                weights.push(pole.weight.get());
            }
            (controls, weights)
        }
    };
    let segment_storage;
    let (preceding, result_segment_storage) =
        ctx.temporary_vec(child_count - 1, "iges composite segment slots")?;
    segment_storage = result_segment_storage;
    let mut segments = ConcatenatedSegments {
        preceding,
        last,
        _storage: segment_storage,
    };
    while children.len() != 0 || ctx.resource_refusal().is_some() {
        let Some(child) = ctx.next_charged(&mut children, "iges composite child lane traversal")? else { break; };
        let (shifted_knots, child_poles, next) = prepare_child(child, segments.end())?;
        let Some(child_start) = child_poles.point_at(0) else {
            return Ok(None);
        };
        if !close_with_tolerance(
            control_points[control_points.len() - 1].get(),
            child_start.get(),
            join_tolerance,
        ) {
            return Ok(None);
        }
        let previous_weight = weights[weights.len() - 1];
        let join_weight = child_poles.weight_at(0).unwrap_or(1.0);
        let scale = previous_weight / join_weight;
        let scale_weight = |weight: f64| -> Result<f64, CompositeCurveError> {
            let scaled = weight * scale;
            if scaled.is_finite() && scaled > 0.0 {
                return Ok(scaled);
            }
            match [weight, previous_weight, join_weight].map(FiniteReal::new) {
                [Some(weight), Some(previous_weight), Some(join_weight)] => {
                    cadmpeg_ir::math::multiply_divide(weight, previous_weight, join_weight)
                }
                _ => None,
            }
            .filter(|weight| weight.get() > 0.0)
            .map(FiniteReal::get)
            .ok_or(CompositeCurveError::JoinWeightScale { scale })
        };
        let pole_skip = usize::from(degree_usize != 0);
        let knot_skip = if degree_usize == 0 {
            1
        } else {
            degree_usize + 1
        };
        if degree_usize != 0 {
            knots.pop();
        }
        ctx.reserve_scoped_vec(
            &mut lane_storage,
            &mut knots,
            shifted_knots.len() - knot_skip,
            "iges composite joined knots",
        )?;
        knots.extend(
            ctx.admit_iter(shifted_knots, "iges composite joined knot traversal")?
                .skip(knot_skip),
        );
        let added_poles = child_poles.count() - pole_skip;
        ctx.reserve_scoped_vec(
            &mut lane_storage,
            &mut control_points,
            added_poles,
            "iges composite joined controls",
        )?;
        ctx.reserve_scoped_vec(
            &mut weight_storage,
            &mut weights,
            added_poles,
            "iges composite joined weights",
        )?;
        match child_poles {
            NurbsPoles3::Polynomial { points } => {
                let weight = scale_weight(1.0)?;
                for point in ctx
                    .admit_iter(points, "iges composite joined polynomial poles")?
                    .skip(pole_skip)
                {
                    control_points.push(point);
                    weights.push(weight);
                }
            }
            NurbsPoles3::Rational { points } => {
                let mut poles_iter = points.into_iter().enumerate();
                while poles_iter.len() != 0 || ctx.resource_refusal().is_some() {
                    let Some((position, pole)) = ctx.next_charged(&mut poles_iter, "iges composite joined rational poles")? else { break; };
                    if position < pole_skip {
                        continue;
                    }
                    let weight = scale_weight(pole.weight.get())?;
                    control_points.push(pole.point);
                    weights.push(weight);
                }
            }
        }
        let preceding = std::mem::replace(&mut segments.last, next);
        segments.preceding.push(preceding);
    }
    let cursor = segments.end();
    let rational = if let Some(first) = weights.first() {
        ctx.any_by(
            &weights,
            |weight| Ok(weight != first),
            "iges composite joined weight equality",
        )?
    } else {
        false
    };
    let poles = if rational {
        let mut weighted =
            ctx.collection_vec(control_points.len(), "iges composite joined weighted poles")?;
        let mut points_weights = control_points.into_iter().zip(weights).enumerate();
        while points_weights.len() != 0 || ctx.resource_refusal().is_some() {
            let Some((index, (point, weight))) = ctx.next_charged(
            &mut points_weights,
            "iges composite joined point conversion",
        )? else { break; };
            let Some(admitted_weight) = NonZeroReal::new(weight) else {
                let field = ctx
                    .format_retained(format_args!("poles"), "iges composite weight error field")?;
                return Err(NurbsError::UnusableWeight {
                    field,
                    index,
                    weight,
                }
                .into());
            };
            weighted.push(WeightedPole3 {
                point,
                weight: admitted_weight,
            });
        }
        NurbsPoles3::Rational { points: weighted }
    } else {
        NurbsPoles3::Polynomial {
            points: ctx.copy_slice(&control_points, "iges composite joined polynomial output")?,
        }
    };
    let knots = ctx.copy_slice(&knots, "iges composite joined output knots")?;
    let nurbs = NurbsCurve::new(ctx, degree, knots, poles, false)??;
    let endpoint = |t: f64| -> Result<FinitePoint3, CompositeCurveError> {
        finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::nurbs_curve_point_at(ctx, &nurbs, t),
        )?)
        .map_err(CodecError::from)?
        .ok_or(CompositeCurveError::EndpointEvaluation { t })
    };
    let endpoints = [endpoint(0.0)?, endpoint(cursor)?];
    Ok(Some(ConcatenatedNurbs {
        nurbs,
        endpoints,
        segments,
    }))
}

fn bounded_edge_for_curve(
    ir: &CadIr,
    curve_id: &CurveId,
    tolerance: f64,
    index: Option<&CompositeIndex>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<CompositeEdge>, CodecError> {
    let curve = match index {
        Some(index) => ctx
            .get_btree_map(
                &index.curve_positions,
                curve_id,
                "iges composite curve lookup",
            )?
            .and_then(|position| ir.model.curves.get(*position)),
        None => ctx.find_by(
            &ir.model.curves,
            |curve| {
                ctx.equal(
                    &curve.id,
                    curve_id,
                    "iges composite curve identity comparison",
                )
            },
            "iges composite curve search",
        )?,
    };
    let Some(curve) = curve else {
        return Ok(None);
    };
    let mut candidate_storage = ctx.reserve_scoped(0, "iges composite edge candidate storage")?;
    let edge_candidates: Cow<'_, [CompositeEdge]> =
        candidate_storage.with_storage(|| -> Result<_, CodecError> {
            Ok(match index {
                Some(index) => Cow::Borrowed(
                    ctx.get_btree_map(&index.edges, curve_id, "iges composite edge lookup")?
                        .map_or(&[][..], Vec::as_slice),
                ),
                None => {
                    let mut candidates = Vec::new();
                    if let Some(refusal) = ctx.resource_refusal() {
                        return Err(refusal.into());
                    }
                    let mut source_values = IntoIterator::into_iter(&ir.model.edges);
                    while source_values.len() != 0 {
                        let Some(edge) = ctx.next_charged(&mut source_values, "iges composite scanned edge traversal")? else {
                            break;
                        };
                        let Some(carrier) = edge.curve() else {
                            continue;
                        };
                        if !ctx.equal(carrier, curve_id, "iges composite scanned edge carrier")? {
                            continue;
                        }
                        ctx.reserve_vec(
                            &mut candidates,
                            1,
                            "iges composite scanned edge candidates",
                        )?;
                        candidates.push(CompositeEdge {
                            start: edge.start.try_clone_for_decode(
                                ctx,
                                "iges composite scanned edge start ID",
                            )?,
                            end: edge
                                .end
                                .try_clone_for_decode(ctx, "iges composite scanned edge end ID")?,
                            param_range: edge
                                .param_range()
                                .map(cadmpeg_ir::units::FiniteVector::get),
                        });
                    }
                    Cow::Owned(candidates)
                }
            })
        })?;
    let Some(geometry) = curve.geometry.solved() else {
        return Ok(None);
    };
    select_composite_edge(ctx, ir, index, geometry, &edge_candidates, tolerance)
}

fn bounded_nurbs_for_id(
    ir: &CadIr,
    curve_id: &CurveId,
    depth: usize,
    join_tolerance: Option<f64>,
    ctx: &DecodeContext<'_>,
    index: Option<&CompositeIndex>,
) -> Result<Option<(NurbsCurve, [f64; 2])>, CompositeCurveError> {
    let _nested = ctx.enter_nested("iges_composite_flatten")?;
    let depth_limit = usize::try_from(ctx.policy().limits.max_recursion_depth)
        .ok()
        .map_or(MAX_COMPOSITE_DEPTH, |policy| {
            policy.min(MAX_COMPOSITE_DEPTH)
        });
    if depth >= depth_limit {
        let refusal = |requested| {
            ctx.refuse_codec_limit(
                "iges_composite_depth",
                u64_from_index(depth_limit),
                requested,
            )
        };
        let requested = depth
            .checked_add(1)
            .map(u64_from_index)
            .ok_or_else(|| CompositeCurveError::Budget(refusal(u64::MAX)))?;
        return Err(CompositeCurveError::Budget(refusal(requested)));
    }
    let mut lookup_storage = ctx.reserve_scoped(0, "iges composite flatten lookup storage")?;
    let owned_index;
    let index = match index {
        Some(index) => index,
        None => {
            owned_index = lookup_storage.with_storage(|| CompositeIndex::from_ir(ir, ctx))?;
            &owned_index
        }
    };
    let curve = index.curve_by_id(ir, curve_id, ctx)?;
    let Some(curve) = curve else {
        return Ok(None);
    };
    if let Some(SolvedCurveGeometry::Composite { segments, .. }) = curve.geometry.solved() {
        let mut child_storage;
        let (mut children, result_child_storage) =
            ctx.temporary_vec(segments.len(), "iges composite nested children")?;
        child_storage = result_child_storage;
        let mut nested_segments = segments.iter();
        while nested_segments.len() != 0 || ctx.resource_refusal().is_some() {
            let Some(segment) = ctx.next_charged(
            &mut nested_segments,
            "iges composite nested segment traversal",
        )? else { break; };
            let Some(child) = child_storage.with_storage(|| {
                bounded_nurbs_for_id(
                    ir,
                    &segment.curve,
                    depth + 1,
                    join_tolerance,
                    ctx,
                    Some(index),
                )
            })?
            else {
                return Ok(None);
            };
            let (curve, range) = if segment.same_sense {
                child
            } else {
                reverse_nurbs(ctx, child.0, child.1)?
            };
            children.push((curve, range, ()));
        }
        let Some(concatenated) = concatenate_nurbs(ctx, children, join_tolerance)? else {
            return Ok(None);
        };
        let range = [0.0, concatenated.segments.end()];
        return Ok(Some((concatenated.nurbs, range)));
    }
    let mut edge_storage = ctx.reserve_scoped(0, "iges composite selected edge storage")?;
    let Some(edge) = edge_storage.with_storage(|| {
        bounded_edge_for_curve(
            ir,
            curve_id,
            join_tolerance.unwrap_or(0.0),
            Some(index),
            ctx,
        )
    })?
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
                point_for_vertex(ir, &edge.start, Some(index), ctx)?,
                point_for_vertex(ir, &edge.end, Some(index), ctx)?,
            ) else {
                return Ok(None);
            };
            let mut knots = ctx.collection_vec(4, "iges composite line knots")?;
            knots.extend([0.0, 0.0, 1.0, 1.0]);
            let mut points = ctx.collection_vec(2, "iges composite line points")?;
            points.extend([start, end]);
            Some((
                NurbsCurve::new(ctx, 1, knots, NurbsPoles3::Polynomial { points }, false)??,
                [0.0, 1.0],
            ))
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius();
            let Some(nurbs) =
                circular_arc_nurbs(center, *axis, *ref_direction, radius, interval, ctx)?
            else {
                return Ok(None);
            };
            let Some(nurbs) = anchor_analytic_nurbs_endpoint_poles(
                ctx,
                nurbs,
                interval,
                ir,
                Some(index),
                &edge,
                join_tolerance,
            )?
            else {
                return Ok(None);
            };
            Some((nurbs, interval))
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius();
            let minor_radius = ellipse_curve.minor_radius();
            let Some(nurbs) = elliptical_arc_nurbs(
                center,
                *axis,
                *major_direction,
                major_radius,
                minor_radius,
                interval,
                ctx,
            )?
            else {
                return Ok(None);
            };
            let Some(nurbs) = anchor_analytic_nurbs_endpoint_poles(
                ctx,
                nurbs,
                interval,
                ir,
                Some(index),
                &edge,
                join_tolerance,
            )?
            else {
                return Ok(None);
            };
            Some((nurbs, interval))
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let vertex = parabola_curve.vertex().get();
            let axis = parabola_curve.frame().axis().as_raw();
            let major_direction = parabola_curve.frame().reference().as_raw();
            let focal_distance = parabola_curve.focal_distance();
            let Some(nurbs) = parabolic_arc_nurbs(
                vertex,
                *axis,
                *major_direction,
                focal_distance,
                interval,
                ctx,
            )?
            else {
                return Ok(None);
            };
            let Some(nurbs) = anchor_analytic_nurbs_endpoint_poles(
                ctx,
                nurbs,
                interval,
                ir,
                Some(index),
                &edge,
                join_tolerance,
            )?
            else {
                return Ok(None);
            };
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<(NurbsCurve, [f64; 2])>, CompositeCurveError> {
    bounded_nurbs_for_id(ir, curve_id, 0, Some(join_tolerance), ctx, Some(index))
}

pub(super) fn bounded_nurbs_for_curve(
    ir: &CadIr,
    curve_id: &CurveId,
    ctx: &DecodeContext<'_>,
    index: Option<&CompositeIndex>,
) -> Result<Option<(NurbsCurve, [f64; 2])>, CompositeCurveError> {
    bounded_nurbs_for_id(ir, curve_id, 0, None, ctx, index)
}

pub(super) fn bounded_parameter_range_for_curve(
    ir: &CadIr,
    curve_id: &CurveId,
    tolerance: f64,
    index: Option<&CompositeIndex>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<[f64; 2]>, CodecError> {
    let mut edge_storage = ctx.reserve_scoped(0, "iges composite selected edge storage")?;
    Ok(edge_storage
        .with_storage(|| bounded_edge_for_curve(ir, curve_id, tolerance, index, ctx))?
        .and_then(|edge| edge.param_range))
}

pub(super) fn bounded_nurbs_for_curve_with_tolerance(
    ir: &CadIr,
    curve_id: &CurveId,
    tolerance: Option<f64>,
    ctx: &DecodeContext<'_>,
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    curve_id: &CurveId,
    index: &CompositeIndex,
    tolerance: f64,
) -> Result<Option<(FinitePoint3, FinitePoint3)>, CodecError> {
    let Some(curve_position) = ctx.get_btree_map(
        &index.curve_positions,
        curve_id,
        "iges composite curve lookup",
    )?
    else {
        return Ok(None);
    };
    let Some(curve) = ir.model.curves.get(*curve_position) else {
        return Ok(None);
    };
    let Some(candidates) =
        ctx.get_btree_map(&index.edges, curve_id, "iges composite edge lookup")?
    else {
        return Ok(None);
    };
    let Some(geometry) = curve.geometry.solved() else {
        return Ok(None);
    };
    let mut edge_storage = ctx.reserve_scoped(0, "iges composite selected edge storage")?;
    let edge = edge_storage.with_storage(|| {
        select_composite_edge(ctx, ir, Some(index), geometry, candidates, tolerance)
    })?;
    let Some(edge) = edge else {
        return Ok(None);
    };
    Ok(
        match (
            point_for_vertex(ir, &edge.start, Some(index), ctx)?,
            point_for_vertex(ir, &edge.end, Some(index), ctx)?,
        ) {
            (Some(start), Some(end)) => Some((start, end)),
            _ => None,
        },
    )
}

fn anchor_analytic_nurbs_endpoint_poles(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    nurbs: NurbsCurve,
    interval: [f64; 2],
    ir: &CadIr,
    index: Option<&CompositeIndex>,
    edge: &CompositeEdge,
    tolerance: Option<f64>,
) -> Result<Option<NurbsCurve>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(tolerance) = tolerance else {
        return Ok(Some(nurbs));
    };
    let (Some(start), Some(end)) = (
        point_for_vertex(ir, &edge.start, index, ctx)?,
        point_for_vertex(ir, &edge.end, index, ctx)?,
    ) else {
        return Ok(None);
    };
    let Some(evaluated_start) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
        cadmpeg_ir::eval::decode::nurbs_curve_point_at(ctx, &nurbs, interval[0]),
    )?)?
    else {
        return Ok(None);
    };
    let Some(evaluated_end) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
        cadmpeg_ir::eval::decode::nurbs_curve_point_at(ctx, &nurbs, interval[1]),
    )?)?
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
    let mut nurbs = nurbs;
    Ok(nurbs
        .try_map_control_points(
            |index, point| {
                let mapped = if index == last {
                    end
                } else if index == 0 {
                    start
                } else {
                    point
                };
                Ok::<_, ()>(mapped)
            },
            ctx,
        )?
        .ok()
        .map(|()| nurbs))
}

fn project_native_composite(
    ir: &mut CadIr,
    index: (
        &mut CompositeIndex,
        &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ),
    entry: &DirectoryEntry,
    child_curves: &[&CurveId],
    join_tolerance: f64,
    ctx: &DecodeContext<'_>,
    sequences: &mut super::geometry::SourceSequences<'_>,
) -> Result<Option<EdgeId>, CodecError> {
    let (index, index_storage) = index;
    let mut endpoint_storage = ctx.reserve_scoped(0, "IGES composite native endpoint storage")?;
    let mut endpoints = endpoint_storage.with_storage(|| {
        ctx.collection_vec(child_curves.len(), "iges composite native endpoints")
    })?;
    let mut endpoint_curves = child_curves.iter();
    while endpoint_curves.len() != 0 || ctx.resource_refusal().is_some() {
        let Some(curve_id) = ctx.next_charged(
        &mut endpoint_curves,
        "iges composite native endpoint traversal",
    )? else { break; };
        let Some(endpoint) = curve_endpoints(ctx, ir, curve_id, index, join_tolerance)? else {
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
    let mut segments = ctx.collection_vec(child_curves.len(), "iges composite native segments")?;
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(child_curves).enumerate();
    while source_values.len() != 0 {
        let Some((position, curve)) = ctx.next_charged(&mut source_values, "iges composite native segment traversal")? else {
            break;
        };
        segments.push(CompositeCurveSegment {
            curve: curve.try_clone_for_decode(ctx, "iges composite native segment curve ids")?,
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
    drop(endpoints);
    drop(endpoint_storage);
    let stem = crate::ids::Stem::directory(entry.sequence);
    let start_point = crate::ids::point_admitted(&stem.tail(crate::ids::Word::Start), ctx)?;
    sequences.record_point(&start_point, &stem, ctx)?;
    let end_point = crate::ids::point_admitted(&stem.tail(crate::ids::Word::End), ctx)?;
    sequences.record_point(&end_point, &stem, ctx)?;
    let start_vertex = index_storage
        .with_storage(|| crate::ids::vertex_admitted(&stem.tail(crate::ids::Word::Start), ctx))?;
    let end_vertex = index_storage
        .with_storage(|| crate::ids::vertex_admitted(&stem.tail(crate::ids::Word::End), ctx))?;
    let mut curve_identity_storage =
        ctx.reserve_scoped(0, "iges composite curve identity storage")?;
    let curve_id =
        curve_identity_storage.with_storage(|| crate::ids::curve_admitted(&stem, ctx))?;
    let edge_id = crate::ids::edge_admitted(&stem, ctx)?;
    ctx.reserve_vec(&mut ir.model.points, 2, "iges composite native point slots")?;
    ctx.reserve_vec(
        &mut ir.model.vertices,
        2,
        "iges composite native vertex slots",
    )?;
    ctx.charge_entities(2, "iges_geometry_composites")?;
    ir.model.points.extend([
        Point::new(
            start_point.try_clone_for_decode(ctx, "iges composite projection identity copy")?,
            start,
            None,
        ),
        Point::new(
            end_point.try_clone_for_decode(ctx, "iges composite projection identity copy")?,
            end,
            None,
        ),
    ]);
    ctx.charge_entities(2, "iges_geometry_composites")?;
    ir.model.vertices.extend([
        Vertex {
            id: start_vertex
                .try_clone_for_decode(ctx, "iges composite projection identity copy")?,
            point: start_point,
            tolerance: None,
        },
        Vertex {
            id: end_vertex.try_clone_for_decode(ctx, "iges composite projection identity copy")?,
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
            super::non_resource_error(error, ctx)?;
            return Ok(None);
        }
    };
    ctx.reserve_vec(&mut ir.model.curves, 1, "iges composite native curve slots")?;
    ctx.charge_entities(1, "iges_geometry_composites")?;
    ir.model.curves.push(Curve {
        id: curve_id.try_clone_for_decode(ctx, "iges composite projection identity copy")?,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments,
            self_intersect: None,
        }),
        source_object: Some(source),
    });
    ctx.reserve_vec(&mut ir.model.edges, 1, "iges composite native edge slots")?;
    ctx.charge_entities(1, "iges_geometry_composites")?;
    ir.model.edges.push(Edge {
        id: edge_id.try_clone_for_decode(ctx, "iges composite projection identity copy")?,
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(
            curve_id.try_clone_for_decode(ctx, "iges composite projection identity copy")?,
        )),
        start: start_vertex.try_clone_for_decode(ctx, "iges composite projection identity copy")?,
        end: end_vertex.try_clone_for_decode(ctx, "iges composite projection identity copy")?,
        tolerance: None,
    });
    index_storage.with_storage(|| {
        index.add_model_entity(
            &curve_id,
            ir.model.curves.len() - 1,
            CompositeEdge {
                start: start_vertex
                    .try_clone_for_decode(ctx, "iges composite projection identity copy")?,
                end: end_vertex
                    .try_clone_for_decode(ctx, "iges composite projection identity copy")?,
                param_range: None,
            },
            [(start_vertex, start), (end_vertex, end)],
            ctx,
        )
    })?;
    Ok(Some(edge_id))
}

/// The degraded carrier, if one was built, and the loss it charges either way.
#[derive(Clone, Copy)]
struct CompositeCarrier<'a> {
    entry: &'a DirectoryEntry,
    child_curves: &'a [&'a CurveId],
    join_tolerance: f64,
}

enum CompositeRefusal {
    NoChildCarrier,
    Child(CompositeCurveError),
    JoinedCarrier(CompositeCurveError),
    Elevation(CompositeCurveError),
    Other(CompositeCurveError),
}

impl fmt::Display for CompositeRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoChildCarrier => {
                formatter.write_str("a child has no bounded line or NURBS carrier")
            }
            Self::Child(error) => write!(formatter, "a child states no curve carrier: {error}"),
            Self::JoinedCarrier(error) => write!(
                formatter,
                "the joined children state no curve carrier: {error}"
            ),
            Self::Elevation(error) => write!(
                formatter,
                "a child does not raise to the composite degree: {error}"
            ),
            Self::Other(error) => error.fmt(formatter),
        }
    }
}

fn project_degraded_composite(
    ir: &mut CadIr,
    index: (
        &mut CompositeIndex,
        &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ),
    carrier: CompositeCarrier<'_>,
    reason: impl fmt::Display,
    ctx: &DecodeContext<'_>,
    sequences: &mut super::geometry::SourceSequences<'_>,
    loss_output: (&mut cadmpeg_core::decode::ScopedReservation<'_>, &mut Vec<LossNote>),
) -> Result<Option<EdgeId>, CodecError> {
    let (loss_slots_storage, losses) = loss_output;
    let (index, index_storage) = index;
    let edge = project_native_composite(
        ir,
        (index, index_storage),
        carrier.entry,
        carrier.child_curves,
        carrier.join_tolerance,
        ctx,
        sequences,
    )?;
    if edge.is_some() {
        super::push_attributed_loss_with_scoped_slots(
            ctx, loss_slots_storage, losses, carrier.entry, IgesLossCode::CompositeCarrierDegraded,
            format_args!("IGES Type 102 entity D{} has no admitted concatenated carrier because {reason}; the ordered native composite carrier was retained", carrier.entry.sequence),
        )?;
    } else {
        super::push_entity_loss_with_scoped_slots(
            ctx,
            loss_slots_storage,
            losses,
            carrier.entry,
            format_args!("{reason}, and no ordered native composite carrier can be constructed"),
        )?;
    }
    Ok(edge)
}

pub(super) fn project<'ctx>(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    source: (
        &BTreeMap<u32, &DirectoryEntry>,
        &BTreeMap<u32, &ParameterRecord>,
    ),
    global: &ProjectedGlobal,
    ctx: &'ctx DecodeContext<'_>,
    sequences: &mut super::geometry::SourceSequences<'_>,
) -> Result<WireProjectionOutcome<'ctx>, CodecError> {
    project_with_type_130_policy(ir, directory, source, global, ctx, sequences, false)
}

pub(super) fn project_type_130_children<'ctx>(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    source: (
        &BTreeMap<u32, &DirectoryEntry>,
        &BTreeMap<u32, &ParameterRecord>,
    ),
    global: &ProjectedGlobal,
    ctx: &'ctx DecodeContext<'_>,
    sequences: &mut super::geometry::SourceSequences<'_>,
) -> Result<WireProjectionOutcome<'ctx>, CodecError> {
    project_with_type_130_policy(ir, directory, source, global, ctx, sequences, true)
}

fn has_type_130_child(
    sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    global_table: GlobalTable,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(record) = ctx
        .get_btree_map(records, &sequence, "iges composite child parameter lookup")?
        .copied()
    else {
        return Ok(false);
    };
    let Some(child_count) = record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count <= MAX_COMPOSITE_CHILDREN)
    else {
        return Ok(false);
    };
    ctx.any_by(
        0..child_count,
        |index| {
            let child = match record
                .integer(index + 2)
                .and_then(|value| u32::try_from(value).ok())
            {
                Some(sequence) => ctx
                    .get_btree_map(entries, &sequence, "iges composite child entry lookup")?
                    .copied(),
                None => None,
            };
            Ok(child.is_some_and(|child| {
                child.entity_type == 130
                    && child.form == 0
                    && composite_child_type_allowed(child.entity_type, child.form, global_table)
            }))
        },
        "iges composite Type130 child search",
    )
}

fn project_with_type_130_policy<'ctx>(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    source: (
        &BTreeMap<u32, &DirectoryEntry>,
        &BTreeMap<u32, &ParameterRecord>,
    ),
    global: &ProjectedGlobal,
    ctx: &'ctx DecodeContext<'_>,
    sequences: &mut super::geometry::SourceSequences<'_>,
    only_type_130_children: bool,
) -> Result<WireProjectionOutcome<'ctx>, CodecError> {
    let (entries, records) = source;
    let mut decoded_storage = ctx.reserve_scoped(0, "iges curve family membership storage")?;
    let mut decoded = BTreeSet::new();
    let mut loss_slots_storage = ctx.reserve_scoped(0, "iges entity loss slots")?;
    let mut losses = Vec::new();
    let mut wire_slots_storage = ctx.reserve_scoped(0, "iges composite wire slots")?;
    let mut wire_edges = Vec::new();
    if ctx.all_by_limit(
        directory,
        |entry| Ok(entry.entity_type != 102 || entry.form != 0),
        "iges composite presence search",
    )? {
        return Ok(WireProjectionOutcome {
            decoded,
            decoded_storage,
            losses,
            loss_slots_storage,
            wire_edges,
            wire_slots_storage,
        });
    }
    if only_type_130_children
        && ctx.all_by_limit(
            directory,
            |entry| Ok(entry.entity_type != 130 || entry.form != 0),
            "iges composite Type130 presence search",
        )?
    {
        return Ok(WireProjectionOutcome {
            decoded,
            decoded_storage,
            losses,
            loss_slots_storage,
            wire_edges,
            wire_slots_storage,
        });
    }
    let mut index_storage = ctx.reserve_scoped(0, "IGES composite carrier index")?;
    let mut index = index_storage.with_storage(|| CompositeIndex::from_ir(ir, ctx))?;
    let join_tolerance = global.minimum_resolution_mm();

    let mut directory_entries = directory.iter();
    while directory_entries.len() != 0 || ctx.resource_refusal().is_some() {
        let Some(entry) = ctx.next_charged(&mut directory_entries, "iges composite directory traversal")? else { break; };
        if entry.entity_type != 102 || entry.form != 0 {
            continue;
        }
        if has_type_130_child(entry.sequence, entries, records, global.global_table(), ctx)?
            != only_type_130_children
        {
            continue;
        }
        let Some(use_flag) = entry
            .status
            .use_flag(global.global_table())
            .filter(|use_flag| composite_use_flag_valid(*use_flag, global.global_table()))
        else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Type 102 Entity Use Flag must be 00 in IGES 4.0"),
            )?;
            continue;
        };
        if !composite_line_font_valid(
            entry.line_font,
            entry.status.hierarchy(),
            global.global_table(),
        ) {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "Type 102 Line Font must be nonzero in IGES 4.0 unless Hierarchy is 01"
                ),
            )?;
            continue;
        }
        let Some(record) = ctx
            .get_btree_map(records, &entry.sequence, "iges composite parameter lookup")?
            .copied()
        else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(raw_child_count) = record.integer(1) else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "child count is invalid"),
            )?;
            continue;
        };
        if let Some(observed) = u64::try_from(raw_child_count)
            .ok()
            .filter(|count| *count > cadmpeg_core::decode::u64_from_index(MAX_COMPOSITE_CHILDREN))
        {
            return Err(ctx.refuse_codec_limit(
                "iges_composite_children",
                cadmpeg_core::decode::u64_from_index(MAX_COMPOSITE_CHILDREN),
                observed,
            ));
        }
        let minimum_child_count = composite_minimum_child_count(global.global_table());
        let Some(child_count) = usize::try_from(raw_child_count)
            .ok()
            .filter(|count| *count >= minimum_child_count)
        else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "child count is outside {minimum_child_count}..={MAX_COMPOSITE_CHILDREN}"
                ),
            )?;
            continue;
        };
        let _pointer_storage;
        let (mut child_sequences, result_pointer_storage) =
            ctx.temporary_vec(child_count, "iges composite child pointer slots")?;
        _pointer_storage = result_pointer_storage;
        let mut valid_child_pointers = true;
        let mut pointer_indices = 0..child_count;
        while !pointer_indices.is_empty() || ctx.resource_refusal().is_some() {
            let Some(index) = ctx.next_charged(
            &mut pointer_indices,
            "iges composite child pointer traversal",
        )? else { break; };
            let Some(sequence) = record
                .integer(index + 2)
                .and_then(|value| u32::try_from(value).ok())
            else {
                valid_child_pointers = false;
                break;
            };
            child_sequences.push(sequence);
        }
        if !valid_child_pointers {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "child pointer list is invalid"),
            )?;
            continue;
        }
        let is_logical_connector = child_sequences.len() == 2
            && matches!(
                global.global_table(),
                GlobalTable::V5_0 | GlobalTable::V5Later
            )
            && ctx
                .get_btree_map(
                    entries,
                    &child_sequences[0],
                    "iges composite child entry lookup",
                )?
                .is_some_and(|child| child.entity_type == 132 && child.form == 0)
            && ctx
                .get_btree_map(
                    entries,
                    &child_sequences[1],
                    "iges composite child entry lookup",
                )?
                .is_some_and(|child| child.entity_type == 132 && child.form == 0);
        if !composite_logical_connector_use_valid(
            use_flag,
            is_logical_connector,
            global.global_table(),
        ) {
            super::push_entity_loss_with_scoped_slots(ctx, &mut loss_slots_storage, &mut losses, entry, format_args!("{}", "Type 102 logical connectors made of exactly two Type 132 Connect Points require Entity Use Flag 04 in IGES 5.0 and later"))?;
            continue;
        }
        if entry.transform != 0 {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "placed composite curves require transformed child-carrier projection"
                ),
            )?;
            continue;
        }
        if ctx.any_by(
            &child_sequences,
            |sequence| {
                Ok(ctx
                    .get_btree_map(entries, sequence, "iges composite child entry lookup")?
                    .is_none_or(|child| {
                        !composite_child_type_allowed(
                            child.entity_type,
                            child.form,
                            global.global_table(),
                        ) || !child.status.is_physically_dependent()
                    }))
            },
            "iges composite child admission",
        )? {
            super::push_entity_loss_with_scoped_slots(ctx, &mut loss_slots_storage, &mut losses, entry, format_args!("{}", "composite child is missing, outside the effective specification family, or is not physically dependent"))?;
            continue;
        }
        let point_context = CompositePointContext {
            entries,
            records,
            global,
            ctx,
            tolerance: join_tolerance,
        };
        let is_curve_sequence = |sequence: &u32| -> Result<bool, CodecError> {
            Ok(ctx
                .get_btree_map(entries, sequence, "iges composite child entry lookup")?
                .is_none_or(|entry| !composite_point_member(entry)))
        };
        let mut carrier_storage = ctx.reserve_scoped(0, "iges composite child carrier storage")?;
        let mut curve_carriers = BTreeMap::new();
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut source_values = IntoIterator::into_iter(&child_sequences).copied();
        while source_values.len() != 0 {
            let Some(sequence) = ctx.next_charged(&mut source_values, "iges composite child carrier traversal")? else {
                break;
            };
            if !is_curve_sequence(&sequence)? {
                continue;
            }
            carrier_storage.with_storage(|| -> Result<(), CodecError> {
                if let Some(curve_id) = curve_carrier_id(sequence, entries, records, ctx)? {
                    ctx.insert_btree_map(
                        &mut curve_carriers,
                        sequence,
                        curve_id,
                        "iges composite child carrier nodes",
                    )?;
                }
                Ok(())
            })?;
        }
        if !composite_point_adjacency_valid(
            ctx,
            ir,
            &index,
            &child_sequences,
            &curve_carriers,
            &point_context,
        )? {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "point or connect-point adjacency is invalid"),
            )?;
            continue;
        }
        let mut curve_id_storage =
            ctx.reserve_scoped(0, "iges composite child curve id storage")?;
        let mut curve_ids = Vec::new();
        let mut missing_curve = false;
        let mut sequence_iter = child_sequences.iter();
        while sequence_iter.len() != 0 || ctx.resource_refusal().is_some() {
            let Some(sequence) = ctx.next_charged(&mut sequence_iter, "iges composite child curve traversal")? else { break; };
            if !is_curve_sequence(sequence)? {
                continue;
            }
            let Some(curve) = ctx.get_btree_map(
                &curve_carriers,
                sequence,
                "iges composite child carrier lookup",
            )?
            else {
                missing_curve = true;
                break;
            };
            ctx.push_scoped_vec(
                &mut curve_id_storage,
                &mut curve_ids,
                curve,
                "iges composite child curve ids",
            )?;
        }
        if curve_ids.is_empty() && !missing_curve {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "composite has no parameterized curve constituent"),
            )?;
            continue;
        }
        if missing_curve {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "a Type 142 constituent has no valid model-space curve pointer"
                ),
            )?;
            continue;
        }
        let carrier = CompositeCarrier {
            entry,
            child_curves: &curve_ids,
            join_tolerance,
        };
        let mut child_storage;
        let (mut children, result_child_storage) =
            ctx.temporary_vec(curve_ids.len(), "iges composite projected children")?;
        child_storage = result_child_storage;
        let mut child_refusal = None;
        let mut projected_curves = curve_ids.iter();
        while projected_curves.len() != 0 || ctx.resource_refusal().is_some() {
            let Some(curve_id) = ctx.next_charged(
            &mut projected_curves,
            "iges composite projected child traversal",
        )? else { break; };
            match child_storage
                .with_storage(|| bounded_nurbs(ir, &index, curve_id, join_tolerance, ctx))
            {
                Ok(Some((curve, range))) => children.push((
                    curve,
                    range,
                    curve_id
                        .try_clone_for_decode(ctx, "iges composite projected child curve IDs")?,
                )),
                Ok(None) => {
                    child_refusal = Some(CompositeRefusal::NoChildCarrier);
                    break;
                }
                Err(error) => {
                    let error = error.non_resource()?;
                    child_refusal = Some(CompositeRefusal::Child(error));
                    break;
                }
            }
        }
        if let Some(reason) = child_refusal {
            let edge = project_degraded_composite(
                ir,
                (&mut index, &mut index_storage),
                carrier,
                reason,
                ctx,
                sequences,
                (&mut loss_slots_storage, &mut losses),
            )?;
            if let Some(edge) = edge {
                ctx.reserve_scoped_vec(&mut wire_slots_storage, &mut wire_edges, 1, "iges composite wire edge ids")?;
                wire_edges.push(edge);
                decoded_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut decoded,
                        entry.sequence,
                        "iges composite decoded sequences",
                    )
                })?;
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
                    (&mut index, &mut index_storage),
                    carrier,
                    // The error names its own cause: a carrier the IR
                    // refuses, or a child that does not raise to the
                    // composite degree.
                    match error {
                        error @ CompositeCurveError::Carrier(_) => {
                            CompositeRefusal::JoinedCarrier(error)
                        }
                        error @ CompositeCurveError::Elevation(_) => {
                            CompositeRefusal::Elevation(error)
                        }
                        error => CompositeRefusal::Other(error),
                    },
                    ctx,
                    sequences,
                    (&mut loss_slots_storage, &mut losses),
                )?;
                if let Some(edge) = edge {
                    ctx.reserve_scoped_vec(&mut wire_slots_storage, &mut wire_edges, 1, "iges composite wire edge ids")?;
                    wire_edges.push(edge);
                    decoded_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut decoded,
                            entry.sequence,
                            "iges composite decoded sequences",
                        )
                    })?;
                }
                continue;
            }
        };
        let Some(ConcatenatedNurbs {
            nurbs,
            endpoints: [start, end],
            segments,
        }) = concatenated
        else {
            let edge = project_degraded_composite(
                ir,
                (&mut index, &mut index_storage),
                carrier,
                "child endpoints do not join within the Global minimum resolution",
                ctx,
                sequences,
                (&mut loss_slots_storage, &mut losses),
            )?;
            if let Some(edge) = edge {
                ctx.reserve_scoped_vec(&mut wire_slots_storage, &mut wire_edges, 1, "iges composite wire edge ids")?;
                wire_edges.push(edge);
                decoded_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut decoded,
                        entry.sequence,
                        "iges composite decoded sequences",
                    )
                })?;
                continue;
            }
            continue;
        };
        let cursor = segments.end();
        let stem = crate::ids::Stem::directory(entry.sequence);
        let start_point = crate::ids::point_admitted(&stem.tail(crate::ids::Word::Start), ctx)?;
        sequences.record_point(&start_point, &stem, ctx)?;
        let end_point = crate::ids::point_admitted(&stem.tail(crate::ids::Word::End), ctx)?;
        sequences.record_point(&end_point, &stem, ctx)?;
        let start_vertex = index_storage.with_storage(|| {
            crate::ids::vertex_admitted(&stem.tail(crate::ids::Word::Start), ctx)
        })?;
        let end_vertex = index_storage
            .with_storage(|| crate::ids::vertex_admitted(&stem.tail(crate::ids::Word::End), ctx))?;
        let mut curve_identity_storage =
            ctx.reserve_scoped(0, "iges composite curve identity storage")?;
        let curve_id =
            curve_identity_storage.with_storage(|| crate::ids::curve_admitted(&stem, ctx))?;
        let edge = crate::ids::edge_admitted(&stem, ctx)?;
        ctx.reserve_vec(&mut ir.model.points, 2, "iges composite solved point slots")?;
        ctx.reserve_vec(
            &mut ir.model.vertices,
            2,
            "iges composite solved vertex slots",
        )?;
        ctx.charge_entities(2, "iges_geometry_composites")?;
        ir.model.points.extend([
            Point::new(
                start_point.try_clone_for_decode(ctx, "iges composite projection identity copy")?,
                start,
                None,
            ),
            Point::new(
                end_point.try_clone_for_decode(ctx, "iges composite projection identity copy")?,
                end,
                None,
            ),
        ]);
        ctx.charge_entities(2, "iges_geometry_composites")?;
        ir.model.vertices.extend([
            Vertex {
                id: start_vertex
                    .try_clone_for_decode(ctx, "iges composite projection identity copy")?,
                point: start_point,
                tolerance: None,
            },
            Vertex {
                id: end_vertex
                    .try_clone_for_decode(ctx, "iges composite projection identity copy")?,
                point: end_point,
                tolerance: None,
            },
        ]);
        sequences.record_curve(&curve_id, entry.sequence, ctx)?;
        ctx.reserve_vec(&mut ir.model.curves, 1, "iges composite solved curve slots")?;
        ctx.charge_entities(1, "iges_geometry_composites")?;
        ir.model.curves.push(Curve {
            id: curve_id.try_clone_for_decode(ctx, "iges composite projection identity copy")?,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
            source_object: Some(source_object(entry, ctx)?),
        });
        ctx.reserve_vec(&mut ir.model.edges, 1, "iges composite solved edge slots")?;
        ctx.charge_entities(1, "iges_geometry_composites")?;
        ir.model.edges.push(Edge {
            id: edge.try_clone_for_decode(ctx, "iges composite projection identity copy")?,
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(
                    curve_id
                        .try_clone_for_decode(ctx, "iges composite projection identity copy")?,
                ),
                Some([0.0, cursor]),
            )
            .map_err(CodecError::malformed)?,
            start: start_vertex
                .try_clone_for_decode(ctx, "iges composite projection identity copy")?,
            end: end_vertex.try_clone_for_decode(ctx, "iges composite projection identity copy")?,
            tolerance: None,
        });
        index_storage.with_storage(|| {
            index.add_model_entity(
                &curve_id,
                ir.model.curves.len() - 1,
                CompositeEdge {
                    start: start_vertex
                        .try_clone_for_decode(ctx, "iges composite projection identity copy")?,
                    end: end_vertex
                        .try_clone_for_decode(ctx, "iges composite projection identity copy")?,
                    param_range: Some([0.0, cursor]),
                },
                [(start_vertex, start), (end_vertex, end)],
                ctx,
            )
        })?;
        let component_count = segments.preceding.len().checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("iges composite procedural components", u64::MAX, 1)
        })?;
        let boundary_count = component_count.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("iges composite procedural boundaries", u64::MAX, 1)
        })?;
        let mut boundaries =
            { ctx.collection_vec(boundary_count, "iges composite procedural boundaries")? };
        boundaries.push(0.0);
        let mut components =
            { ctx.collection_vec(component_count, "iges composite procedural components")? };
        for segment in ctx
            .admit_iter(
                segments.preceding,
                "iges composite procedural segment traversal",
            )?
            .chain(std::iter::once(segments.last))
        {
            boundaries.push(segment.end);
            components.push(cadmpeg_ir::geometry::CompoundComponent {
                parameter: segment.child_start,
                component: segment.child,
            });
        }

        ctx.charge_entities(1, "iges_geometry_composites")?;
        let _attached = ir.model.add_procedural_curve(
            ctx,
            &curve_id,
            ProceduralCurve::new(
                crate::ids::procedural_curve_admitted(&stem, ctx)?,
                ProceduralCurveDefinition::Compound(
                    cadmpeg_ir::geometry::CompoundCurveConstruction::try_new(
                        boundaries, components, None,
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                ),
            ),
        )?;
        ctx.reserve_scoped_vec(&mut wire_slots_storage, &mut wire_edges, 1, "iges composite wire edge ids")?;
        wire_edges.push(edge);
        decoded_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut decoded,
                entry.sequence,
                "iges composite decoded sequences",
            )
        })?;
    }

    Ok(WireProjectionOutcome {
        decoded,
        decoded_storage,
        losses,
        loss_slots_storage,
        wire_edges,
        wire_slots_storage,
    })
}

#[cfg(test)]
mod tests;
