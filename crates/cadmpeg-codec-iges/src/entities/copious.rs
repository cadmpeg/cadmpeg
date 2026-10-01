// SPDX-License-Identifier: Apache-2.0
//! Copious point, linear-path, and presentation tuple projection.

use super::geometry::{resolve_transform, source_object};
use super::push_attributed_loss;

use crate::directory::{DirectoryEntry, UseFlag};
use crate::global::{GlobalTable, ProjectedGlobal};
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{refuse_local_limit, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    nurbs::{KnotVector, NurbsCurve, NurbsPoles3},
    Curve, CurveGeometry, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::{EdgeId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

const MAX_COPIOUS_TUPLES: usize = 1_000_000;

fn push_copious_loss(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    reason: fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    push_attributed_loss(
        ctx,
        losses,
        entry,
        crate::loss::IgesLossCode::EntityNotProjected,
        format_args!(
            "IGES entity type {} form {} was not projected: {reason}",
            entry.entity_type, entry.form
        ),
    )
}

pub(super) struct CopiousProjectionOutcome {
    decoded: BTreeSet<u32>,
    losses: Vec<LossNote>,
    wire_edges: Vec<EdgeId>,
    free_vertices: Vec<VertexId>,
}

impl CopiousProjectionOutcome {
    pub(super) fn merge_into(
        self,
        decoded: &mut BTreeSet<u32>,
        losses: &mut Vec<LossNote>,
        wire_edges: &mut Vec<EdgeId>,
        free_vertices: &mut Vec<VertexId>,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        for sequence in self.decoded {
            ctx.insert_btree_set(decoded, sequence, "iges merged decoded sequences")?;
        }
        ctx.reserve_vec(losses, self.losses.len(), "iges merged loss slots")?;
        losses.extend(self.losses);
        ctx.reserve_vec(
            wire_edges,
            self.wire_edges.len(),
            "iges merged wire edge slots",
        )?;
        wire_edges.extend(self.wire_edges);
        ctx.reserve_vec(
            free_vertices,
            self.free_vertices.len(),
            "iges merged free vertex slots",
        )?;
        free_vertices.extend(self.free_vertices);
        Ok(())
    }
}

pub(crate) fn expected_interpretation(form: i64) -> Option<i64> {
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

fn presentation_use_flag_valid(form: i64, use_flag: Option<UseFlag>) -> bool {
    !presentation_form(form) || use_flag == Some(UseFlag::Annotation)
}

fn points_coincident(left: Point3, right: Point3, resolution: f64) -> bool {
    let distance = left.distance(right);
    distance == 0.0 || distance < resolution
}

fn has_forbidden_form_63_duplicate(
    points: &[Point3],
    resolution: f64,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if points.len() == 2 {
        return Ok(true);
    }
    let allowed_endpoint_pair = |left: usize, right: usize| left == 0 && right + 1 == points.len();
    let exact_key = |point: Point3| {
        let key = |value: f64| {
            if value == 0.0 {
                0
            } else {
                value.to_bits()
            }
        };
        (key(point.x), key(point.y), key(point.z))
    };
    let mut exact_points = None;
    let mut cells = HashMap::new();
    let cell_size = resolution * 0.5;
    for (index, point) in points.iter().copied().enumerate() {
        if cell_size <= 0.0 {
            let exact_points = exact_points.get_or_insert_with(HashMap::new);
            ctx.admit_hash_map_entry(
                exact_points,
                &exact_key(point),
                "iges copious exact-point index",
            )?;
            if let Some(previous) = exact_points.insert(exact_key(point), index) {
                if !allowed_endpoint_pair(previous, index) {
                    return Ok(true);
                }
            }
            continue;
        }
        let cell_index = |value: f64| {
            let index = (value / cell_size).floor();
            cadmpeg_core::convert::truncate_f64_to_i128(index)
        };
        let Some((x, y, z)) = cell_index(point.x)
            .zip(cell_index(point.y))
            .zip(cell_index(point.z))
            .map(|((x, y), z)| (x, y, z))
        else {
            let exact_points = exact_points.get_or_insert_with(HashMap::new);
            ctx.admit_hash_map_entry(
                exact_points,
                &exact_key(point),
                "iges copious exact-point index",
            )?;
            if let Some(previous) = exact_points.insert(exact_key(point), index) {
                if !allowed_endpoint_pair(previous, index) {
                    return Ok(true);
                }
            }
            continue;
        };
        for dx in -2_i128..=2 {
            for dy in -2_i128..=2 {
                for dz in -2_i128..=2 {
                    let Some(neighbor) = x
                        .checked_add(dx)
                        .zip(y.checked_add(dy))
                        .zip(z.checked_add(dz))
                        .map(|((x, y), z)| (x, y, z))
                    else {
                        continue;
                    };
                    let Some(&(previous, previous_point)) = cells.get(&neighbor) else {
                        continue;
                    };
                    if points_coincident(point, previous_point, resolution)
                        && !allowed_endpoint_pair(previous, index)
                    {
                        return Ok(true);
                    }
                }
            }
        }
        ctx.admit_hash_map_entry(&mut cells, &(x, y, z), "iges copious proximity cells")?;
        cells.entry((x, y, z)).or_insert((index, point));
    }
    Ok(false)
}

fn has_form_63_self_intersection(
    points: &[Point3],
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let planar_points =
        ctx.collect_indexed_vec(points.len(), "iges copious planar points", |index| {
            let point = points[index];
            Ok([point.x, point.y])
        })?;
    super::geometry::planar_polyline_has_self_intersection(&planar_points, ctx)
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: &DecodeContext<'_>,
    sequences: &mut super::geometry::SourceSequences,
) -> Result<CopiousProjectionOutcome, CodecError> {
    let mut records = BTreeMap::new();
    for record in parameters {
        ctx.insert_btree_map(
            &mut records,
            record.directory_sequence,
            record,
            "iges copious parameter index",
        )?;
    }
    let mut entries = BTreeMap::new();
    for entry in directory {
        ctx.insert_btree_map(
            &mut entries,
            entry.sequence,
            entry,
            "iges copious directory index",
        )?;
    }
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut wire_edges = Vec::new();
    let mut free_vertices = Vec::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 106 && expected_interpretation(entry.form).is_some())
    {
        if !presentation_use_flag_valid(entry.form, entry.status.use_flag(global.global_table())) {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("Type 106 presentation forms require Entity Use Flag 01"),
            )?;
            continue;
        }
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(interpretation) = record.integer(1) else {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("interpretation is invalid"),
            )?;
            continue;
        };
        let Some(raw_tuple_count) = record.integer(2) else {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("tuple count is invalid"),
            )?;
            continue;
        };
        if let Some(observed) = u64::try_from(raw_tuple_count)
            .ok()
            .filter(|count| *count > cadmpeg_core::decode::u64_from_index(MAX_COPIOUS_TUPLES))
        {
            return Err(refuse_local_limit(
                "iges_copious_tuples",
                cadmpeg_core::decode::u64_from_index(MAX_COPIOUS_TUPLES),
                observed,
            ));
        }
        let Some(tuple_count) = usize::try_from(raw_tuple_count).ok() else {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("tuple count is invalid"),
            )?;
            continue;
        };
        if Some(interpretation) != expected_interpretation(entry.form) {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("interpretation flag disagrees with the entity form"),
            )?;
            continue;
        }
        if tuple_count == 0 {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("tuple count is outside 1..={MAX_COPIOUS_TUPLES}"),
            )?;
            continue;
        }
        if matches!(entry.form, 11..=13) {
            let minimum_tuple_count = if matches!(global.global_table(), GlobalTable::V4_0) {
                1
            } else {
                2
            };
            if tuple_count < minimum_tuple_count {
                push_copious_loss(ctx, &mut losses, entry, format_args!(
                    "linear paths require at least {minimum_tuple_count} tuple(s) under the effective specification family"
                ))?;
                continue;
            }
        }
        if entry.form == 63 && tuple_count < 2 {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("simple closed paths require at least two tuples"),
            )?;
            continue;
        }
        if matches!(entry.form, 20 | 21 | 31..=38) && tuple_count % 2 != 0 {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("paired presentation form has an odd tuple count"),
            )?;
            continue;
        }
        if entry.form == 40 && (tuple_count < 3 || tuple_count % 2 == 0) {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("witness lines require an odd tuple count of at least three"),
            )?;
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
            Err(error) => {
                let message = error.non_resource()?;
                push_copious_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                continue;
            }
        };
        let (tuple_start, tuple_width, common_z) = match interpretation {
            1 => {
                let Some(z) = record.number(3).and_then(FiniteReal::new) else {
                    push_copious_loss(
                        ctx,
                        &mut losses,
                        entry,
                        format_args!("common z coordinate is invalid"),
                    )?;
                    continue;
                };
                (4_usize, 2_usize, Some(z))
            }
            2 => (3, 3, None),
            3 => (3, 6, None),
            _ => {
                push_copious_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("copious-data interpretation is invalid"),
                )?;
                continue;
            }
        };
        let Some(value_count) = tuple_count.checked_mul(tuple_width) else {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("tuple value count overflows"),
            )?;
            continue;
        };
        let Some(tuple_end) = tuple_start.checked_add(value_count) else {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("tuple end offset overflows"),
            )?;
            continue;
        };
        let Some(values) = ctx.collect_options(
            (tuple_start..tuple_end).map(|index| record.number(index).and_then(FiniteReal::new)),
            "iges copious tuple values",
        )?
        else {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("tuple array is truncated or non-finite"),
            )?;
            continue;
        };
        let definition_points =
            ctx.collect_indexed_vec(tuple_count, "iges copious definition points", |index| {
                let tuple = &values[index * tuple_width..(index + 1) * tuple_width];
                let z = match common_z {
                    Some(z) => z,
                    None => tuple[2],
                };
                Ok(Point3::new(
                    tuple[0].get() * factor,
                    tuple[1].get() * factor,
                    z.get() * factor,
                ))
            })?;
        let Some(positions) = ctx.collect_options(
            definition_points
                .iter()
                .copied()
                .map(|point| transform.apply_point(point)),
            "iges copious positioned points",
        )?
        else {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("placement produces non-finite copious points"),
            )?;
            continue;
        };
        if presentation_form(entry.form) {
            push_attributed_loss(
                ctx,
                &mut losses,
                entry,
                crate::loss::IgesLossCode::DisplayDataNotProjected,
                format_args!(
                    "IGES entity type {} form {} display data was not projected: copious presentation tuples have no neutral display carrier",
                    entry.entity_type, entry.form
                ),
            )?;
            continue;
        }
        let projects_as_points = matches!(entry.form, 1..=3)
            || (matches!(entry.form, 11..=13)
                && tuple_count == 1
                && matches!(global.global_table(), GlobalTable::V4_0));
        if projects_as_points {
            for (index, position) in positions.into_iter().enumerate() {
                let point = crate::ids::point_admitted(
                    &crate::ids::Stem::directory(entry.sequence).tail_index(index + 1),
                    ctx,
                )?;
                sequences.record_point(
                    &point,
                    &crate::ids::Stem::directory(entry.sequence),
                    ctx,
                )?;
                let vertex = crate::ids::vertex_admitted(
                    &crate::ids::Stem::directory(entry.sequence).tail_index(index + 1),
                    ctx,
                )?;
                ctx.reserve_vec(&mut ir.model.points, 1, "iges copious neutral points")?;
                ctx.charge_entities(1, "iges_geometry_copious")?;
                ir.model.points.push(Point::new(
                    point.try_clone_for_decode(ctx, "iges copious identity copy")?,
                    position,
                    None,
                ));
                ctx.reserve_vec(&mut ir.model.vertices, 1, "iges copious neutral vertices")?;
                ctx.charge_entities(1, "iges_geometry_copious")?;
                ir.model.vertices.push(Vertex {
                    id: vertex.try_clone_for_decode(ctx, "iges copious identity copy")?,
                    point,
                    tolerance: None,
                });
                ctx.reserve_vec(&mut free_vertices, 1, "iges copious free vertices")?;
                free_vertices.push(vertex);
            }
            ctx.insert_btree_set(
                &mut decoded,
                entry.sequence,
                "iges copious decoded sequences",
            )?;
            continue;
        }
        let points =
            ctx.collect_indexed_vec(positions.len(), "iges copious path points", |index| {
                Ok(positions[index].get())
            })?;
        let resolution = global.minimum_resolution_mm();
        if entry.form == 63 && !points_coincident(points[0], points[points.len() - 1], resolution) {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("simple closed path endpoints disagree beyond the minimum resolution"),
            )?;
            continue;
        }
        if entry.form == 63 && has_forbidden_form_63_duplicate(&points, resolution, ctx)? {
            let reason = if points.len() == 2 {
                "simple closed path has no non-zero segment"
            } else {
                "simple closed path has coincident non-endpoint points"
            };
            push_copious_loss(ctx, &mut losses, entry, format_args!("{reason}"))?;
            continue;
        }
        if entry.form == 63 && has_form_63_self_intersection(&definition_points, ctx)? {
            push_copious_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("simple closed path intersects itself away from shared endpoints"),
            )?;
            continue;
        }
        let topology_tolerance = if entry.form == 63 && resolution > 0.0 {
            let Some(value) = cadmpeg_ir::scalar::PositiveReal::new(resolution) else {
                push_copious_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("topology tolerance must be finite"),
                )?;
                continue;
            };
            Some(value)
        } else {
            None
        };
        let parameter_end = cadmpeg_core::convert::f64_from_index(points.len() - 1)
            .ok_or_else(|| ctx.refuse_codec_limit("iges copious knots", 0, 1))?;
        let knot_count = points
            .len()
            .checked_add(2)
            .ok_or_else(|| refuse_local_limit("iges copious knots", u64::MAX, 1))?;
        let mut knots = ctx.collection_vec(knot_count, "iges copious knots")?;
        knots.extend([0.0, 0.0]);
        for value in 1..points.len() - 1 {
            let knot = cadmpeg_core::convert::f64_from_index(value)
                .ok_or_else(|| ctx.refuse_codec_limit("iges copious knots", 0, 1))?;
            knots.push(knot);
        }
        knots.extend([parameter_end, parameter_end]);
        let start = positions[0];
        let end = positions[positions.len() - 1];
        let stem = crate::ids::Stem::directory(entry.sequence);
        let start_point = crate::ids::point_admitted(&stem.tail(crate::ids::Word::Start), ctx)?;
        sequences.record_point(&start_point, &stem, ctx)?;
        let end_point = crate::ids::point_admitted(&stem.tail(crate::ids::Word::End), ctx)?;
        sequences.record_point(&end_point, &stem, ctx)?;
        let start_vertex = crate::ids::vertex_admitted(&stem.tail(crate::ids::Word::Start), ctx)?;
        let end_vertex = if entry.form == 63 {
            start_vertex.try_clone_for_decode(ctx, "iges copious identity copy")?
        } else {
            crate::ids::vertex_admitted(&stem.tail(crate::ids::Word::End), ctx)?
        };
        let curve = crate::ids::curve_admitted(&stem, ctx)?;
        let edge = crate::ids::edge_admitted(&stem, ctx)?;
        ctx.reserve_vec(&mut ir.model.points, 1, "iges copious neutral points")?;
        ctx.charge_entities(1, "iges_geometry_copious")?;
        ir.model.points.push(Point::new(
            start_point.try_clone_for_decode(ctx, "iges copious identity copy")?,
            start,
            None,
        ));
        ctx.reserve_vec(&mut ir.model.vertices, 1, "iges copious neutral vertices")?;
        ctx.charge_entities(1, "iges_geometry_copious")?;
        ir.model.vertices.push(Vertex {
            id: start_vertex.try_clone_for_decode(ctx, "iges copious identity copy")?,
            point: start_point,
            tolerance: topology_tolerance,
        });
        if entry.form != 63 {
            ctx.reserve_vec(&mut ir.model.points, 1, "iges copious neutral points")?;
            ctx.charge_entities(1, "iges_geometry_copious")?;
            ir.model.points.push(Point::new(
                end_point.try_clone_for_decode(ctx, "iges copious identity copy")?,
                end,
                None,
            ));
            ctx.reserve_vec(&mut ir.model.vertices, 1, "iges copious neutral vertices")?;
            ctx.charge_entities(1, "iges_geometry_copious")?;
            ir.model.vertices.push(Vertex {
                id: end_vertex.try_clone_for_decode(ctx, "iges copious identity copy")?,
                point: end_point,
                tolerance: topology_tolerance,
            });
        }
        sequences.record_curve(&curve, entry.sequence, ctx)?;
        let knots = ctx
            .collect_options(
                knots.into_iter().map(FiniteReal::new),
                "iges copious finite knots",
            )?
            .ok_or_else(|| CodecError::malformed("copious-data curve: knots must be finite"))?;
        let mut raw_knots = ctx.collection_vec(knots.len(), "iges copious admitted knots")?;
        raw_knots.extend(knots.into_iter().map(FiniteReal::get));
        let nurbs = KnotVector::new(raw_knots).and_then(|knots| {
            NurbsPoles3::from_checked_lanes(positions, None)
                .and_then(|poles| NurbsCurve::new(1, knots, poles, false))
        });
        ctx.reserve_vec(&mut ir.model.curves, 1, "iges copious neutral curves")?;
        ctx.charge_entities(1, "iges_geometry_copious")?;
        ir.model.curves.push(Curve {
            id: curve.try_clone_for_decode(ctx, "iges copious identity copy")?,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs.map_err(
                |error| CodecError::malformed(format_args!("copious-data curve: {error}")),
            )?)),
            source_object: Some(source_object(entry, ctx)?),
        });
        ctx.reserve_vec(&mut ir.model.edges, 1, "iges copious neutral edges")?;
        ctx.charge_entities(1, "iges_geometry_copious")?;
        ir.model.edges.push(Edge {
            id: edge.try_clone_for_decode(ctx, "iges copious identity copy")?,
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve),
                Some([0.0, parameter_end]),
            )
            .map_err(CodecError::malformed)?,
            start: start_vertex,
            end: end_vertex,
            tolerance: topology_tolerance,
        });
        ctx.reserve_vec(&mut wire_edges, 1, "iges copious wire edges")?;
        wire_edges.push(edge);
        ctx.insert_btree_set(
            &mut decoded,
            entry.sequence,
            "iges copious decoded sequences",
        )?;
    }

    Ok(CopiousProjectionOutcome {
        decoded,
        losses,
        wire_edges,
        free_vertices,
    })
}

#[cfg(test)]
mod tests;
