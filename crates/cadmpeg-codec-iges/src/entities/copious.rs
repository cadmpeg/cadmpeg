// SPDX-License-Identifier: Apache-2.0
//! Copious point, linear-path, and presentation tuple projection.

use super::geometry::{resolve_transform, source_object};
use super::push_attributed_loss;

use crate::directory::{DirectoryEntry, UseFlag};
use crate::global::{GlobalTable, ProjectedGlobal};
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    nurbs::{KnotVector, NurbsCurve},
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
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    reason: fmt::Arguments<'_>,
) {
    push_attributed_loss(
        losses,
        entry,
        if presentation_form(entry.form) {
            crate::loss::IgesLossCode::EntityNotProjected
        } else {
            crate::loss::IgesLossCode::GeometryNotProjected
        },
        format_args!(
            "IGES entity type {} form {} was not projected: {reason}",
            entry.entity_type, entry.form
        ),
    );
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
    ) {
        for sequence in self.decoded {
            decoded.insert(sequence);
        }

        losses.extend(self.losses);

        wire_edges.extend(self.wire_edges);

        free_vertices.extend(self.free_vertices);
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

fn has_forbidden_form_63_duplicate(points: &[Point3], resolution: f64) -> bool {
    if points.len() == 2 {
        return true;
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

            if let Some(previous) = exact_points.insert(exact_key(point), index) {
                if !allowed_endpoint_pair(previous, index) {
                    return true;
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

            if let Some(previous) = exact_points.insert(exact_key(point), index) {
                if !allowed_endpoint_pair(previous, index) {
                    return true;
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
                        return true;
                    }
                }
            }
        }

        cells.entry((x, y, z)).or_insert((index, point));
    }
    false
}

fn has_form_63_self_intersection(points: &[Point3]) -> bool {
    let planar_points = (0..points.len())
        .map(|index| {
            let point = points[index];
            [point.x, point.y]
        })
        .collect::<Vec<_>>();
    super::geometry::planar_polyline_has_self_intersection(&planar_points)
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
        records.insert(record.directory_sequence, record);
    }
    let mut entries = BTreeMap::new();
    for entry in directory {
        entries.insert(entry.sequence, entry);
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
                &mut losses,
                entry,
                format_args!("Type 106 presentation forms require Entity Use Flag 01"),
            );
            continue;
        }
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("Parameter Data record is missing"),
            );
            continue;
        };
        let Some(interpretation) = record.integer(1) else {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("interpretation is invalid"),
            );
            continue;
        };
        let Some(raw_tuple_count) = record.integer(2) else {
            push_copious_loss(&mut losses, entry, format_args!("tuple count is invalid"));
            continue;
        };
        let Some(tuple_count) = usize::try_from(raw_tuple_count).ok() else {
            push_copious_loss(&mut losses, entry, format_args!("tuple count is invalid"));
            continue;
        };
        if tuple_count > MAX_COPIOUS_TUPLES {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("tuple count exceeds {MAX_COPIOUS_TUPLES}"),
            );
            continue;
        }
        if Some(interpretation) != expected_interpretation(entry.form) {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("interpretation flag disagrees with the entity form"),
            );
            continue;
        }
        if tuple_count == 0 {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("tuple count is outside 1..={MAX_COPIOUS_TUPLES}"),
            );
            continue;
        }
        if matches!(entry.form, 11..=13) {
            let minimum_tuple_count = if matches!(global.global_table(), GlobalTable::V4_0) {
                1
            } else {
                2
            };
            if tuple_count < minimum_tuple_count {
                push_copious_loss(&mut losses, entry, format_args!(
                    "linear paths require at least {minimum_tuple_count} tuple(s) under the effective specification family"
                ));
                continue;
            }
        }
        if entry.form == 63 && tuple_count < 2 {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("simple closed paths require at least two tuples"),
            );
            continue;
        }
        if matches!(entry.form, 20 | 21 | 31..=38) && tuple_count % 2 != 0 {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("paired presentation form has an odd tuple count"),
            );
            continue;
        }
        if entry.form == 40 && (tuple_count < 3 || tuple_count % 2 == 0) {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("witness lines require an odd tuple count of at least three"),
            );
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
                push_copious_loss(&mut losses, entry, format_args!("{message}"));
                continue;
            }
        };
        let (tuple_start, tuple_width, common_z) = match interpretation {
            1 => {
                let Some(z) = record.number(3).and_then(FiniteReal::new) else {
                    push_copious_loss(
                        &mut losses,
                        entry,
                        format_args!("common z coordinate is invalid"),
                    );
                    continue;
                };
                (4_usize, 2_usize, Some(z))
            }
            2 => (3, 3, None),
            3 => (3, 6, None),
            _ => {
                push_copious_loss(
                    &mut losses,
                    entry,
                    format_args!("copious-data interpretation is invalid"),
                );
                continue;
            }
        };
        let Some(value_count) = tuple_count.checked_mul(tuple_width) else {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("tuple value count overflows"),
            );
            continue;
        };
        let Some(tuple_end) = tuple_start.checked_add(value_count) else {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("tuple end offset overflows"),
            );
            continue;
        };
        let Some(values) = (tuple_start..tuple_end)
            .map(|index| record.number(index).and_then(FiniteReal::new))
            .collect::<Option<Vec<_>>>()
        else {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("tuple array is truncated or non-finite"),
            );
            continue;
        };
        let definition_points = (0..tuple_count)
            .map(|index| {
                let tuple = &values[index * tuple_width..(index + 1) * tuple_width];
                let z = match common_z {
                    Some(z) => z,
                    None => tuple[2],
                };
                Point3::new(
                    tuple[0].get() * factor,
                    tuple[1].get() * factor,
                    z.get() * factor,
                )
            })
            .collect::<Vec<_>>();
        let Some(positions) = definition_points
            .iter()
            .copied()
            .map(|point| transform.apply_point(point))
            .collect::<Option<Vec<_>>>()
        else {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("placement produces non-finite copious points"),
            );
            continue;
        };
        if presentation_form(entry.form) {
            push_attributed_loss(
                &mut losses,
                entry,
                crate::loss::IgesLossCode::DisplayDataNotProjected,
                format_args!(
                    "IGES entity type {} form {} display data was not projected: copious presentation tuples have no neutral display carrier",
                    entry.entity_type, entry.form
                ),
            );
            continue;
        }
        let projects_as_points = matches!(entry.form, 1..=3)
            || (matches!(entry.form, 11..=13)
                && tuple_count == 1
                && matches!(global.global_table(), GlobalTable::V4_0));
        if projects_as_points {
            for (index, position) in positions.into_iter().enumerate() {
                let point = crate::ids::point(
                    &crate::ids::Stem::directory(entry.sequence).tail_index(index + 1),
                );
                sequences.record_point(&point, &crate::ids::Stem::directory(entry.sequence));
                let vertex = crate::ids::vertex(
                    &crate::ids::Stem::directory(entry.sequence).tail_index(index + 1),
                );

                ctx.charge_entities(1, "iges_geometry_copious")?;
                ir.model
                    .points
                    .push(Point::new(point.clone(), position, None));

                ctx.charge_entities(1, "iges_geometry_copious")?;
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
        let points: Vec<_> = positions.iter().map(|position| position.get()).collect();
        let resolution = global.minimum_resolution_mm();
        if entry.form == 63 && !points_coincident(points[0], points[points.len() - 1], resolution) {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("simple closed path endpoints disagree beyond the minimum resolution"),
            );
            continue;
        }
        if entry.form == 63 && has_forbidden_form_63_duplicate(&points, resolution) {
            let reason = if points.len() == 2 {
                "simple closed path has no non-zero segment"
            } else {
                "simple closed path has coincident non-endpoint points"
            };
            push_copious_loss(&mut losses, entry, format_args!("{reason}"));
            continue;
        }
        if entry.form == 63 && has_form_63_self_intersection(&definition_points) {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("simple closed path intersects itself away from shared endpoints"),
            );
            continue;
        }
        let topology_tolerance = if entry.form == 63 && resolution > 0.0 {
            let Some(value) = cadmpeg_ir::scalar::PositiveReal::new(resolution) else {
                push_copious_loss(
                    &mut losses,
                    entry,
                    format_args!("topology tolerance must be finite"),
                );
                continue;
            };
            Some(value)
        } else {
            None
        };
        let mut knots = vec![0.0, 0.0];
        let Some(interior) = (1..points.len() - 1)
            .chain([points.len() - 1, points.len() - 1])
            .map(cadmpeg_core::convert::f64_from_index)
            .collect::<Option<Vec<_>>>()
        else {
            push_copious_loss(
                &mut losses,
                entry,
                format_args!("tuple index is not an exact parameter"),
            );
            continue;
        };
        knots.extend(interior);
        let parameter_end = knots[knots.len() - 1];
        let start = positions[0];
        let end = positions[positions.len() - 1];
        let stem = crate::ids::Stem::directory(entry.sequence);
        let start_point = crate::ids::point(&stem.tail(crate::ids::Word::Start));
        sequences.record_point(&start_point, &stem);
        let end_point = crate::ids::point(&stem.tail(crate::ids::Word::End));
        sequences.record_point(&end_point, &stem);
        let start_vertex = crate::ids::vertex(&stem.tail(crate::ids::Word::Start));
        let end_vertex = if entry.form == 63 {
            start_vertex.clone()
        } else {
            crate::ids::vertex(&stem.tail(crate::ids::Word::End))
        };
        let curve = crate::ids::curve(&stem);
        let edge = crate::ids::edge(&stem);

        ctx.charge_entities(1, "iges_geometry_copious")?;
        ir.model
            .points
            .push(Point::new(start_point.clone(), start, None));

        ctx.charge_entities(1, "iges_geometry_copious")?;
        ir.model.vertices.push(Vertex {
            id: start_vertex.clone(),
            point: start_point,
            tolerance: topology_tolerance,
        });
        if entry.form != 63 {
            ctx.charge_entities(1, "iges_geometry_copious")?;
            ir.model
                .points
                .push(Point::new(end_point.clone(), end, None));

            ctx.charge_entities(1, "iges_geometry_copious")?;
            ir.model.vertices.push(Vertex {
                id: end_vertex.clone(),
                point: end_point,
                tolerance: topology_tolerance,
            });
        }
        sequences.record_curve(&curve, entry.sequence);
        let knots = knots
            .into_iter()
            .map(FiniteReal::new)
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| CodecError::malformed("copious-data curve: knots must be finite"))?;
        let mut raw_knots = Vec::new();
        raw_knots.extend(knots.into_iter().map(FiniteReal::get));
        let nurbs = match KnotVector::new(ctx, raw_knots)? {
            Err(error) => Err(error),
            Ok(knots) => NurbsCurve::from_checked_lanes(ctx, 1, knots, positions, None, false)?,
        };

        ctx.charge_entities(1, "iges_geometry_copious")?;
        ir.model.curves.push(Curve {
            parameter_range: None,
            id: curve.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs.map_err(
                |error| CodecError::malformed(format_args!("copious-data curve: {error}")),
            )?)),
            source_object: Some(source_object(entry)?),
        });

        ctx.charge_entities(1, "iges_geometry_copious")?;
        ir.model.edges.push(Edge {
            id: edge.clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve),
                Some([0.0, parameter_end]),
            )
            .map_err(CodecError::malformed)?,
            start: start_vertex,
            end: end_vertex,
            tolerance: topology_tolerance,
        });

        wire_edges.push(edge);
        decoded.insert(entry.sequence);
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
