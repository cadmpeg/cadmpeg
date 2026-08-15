// SPDX-License-Identifier: Apache-2.0
//! Copious point, linear-path, and presentation tuple projection.

use super::geometry::{entity_loss, resolve_transform, source_object};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{refuse_local_limit, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

const MAX_COPIOUS_TUPLES: usize = 1_000_000;

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

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &Global,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<CopiousProjection, CodecError> {
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
        let Some(interpretation) = record.integer(1) else {
            losses.push(entity_loss(entry, "interpretation is invalid"));
            continue;
        };
        let Some(raw_tuple_count) = record.integer(2) else {
            losses.push(entity_loss(entry, "tuple count is invalid"));
            continue;
        };
        if raw_tuple_count > MAX_COPIOUS_TUPLES as i64 {
            return Err(refuse_local_limit(
                "iges_copious_tuples",
                MAX_COPIOUS_TUPLES as u64,
                u64::try_from(raw_tuple_count).unwrap_or(u64::MAX),
                None,
            ));
        }
        let Some(tuple_count) = usize::try_from(raw_tuple_count).ok() else {
            losses.push(entity_loss(entry, "tuple count is invalid"));
            continue;
        };
        if Some(interpretation) != expected_interpretation(entry.form) {
            losses.push(entity_loss(
                entry,
                "interpretation flag disagrees with the entity form",
            ));
            continue;
        }
        if tuple_count == 0 {
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
        if entry.form == 63 && points[0].distance(points[points.len() - 1]) > resolution {
            losses.push(entity_loss(
                entry,
                "simple closed path endpoints disagree beyond the minimum resolution",
            ));
            continue;
        }
        if entry.form == 63
            && points
                .windows(2)
                .any(|pair| pair[0].distance(pair[1]) <= resolution)
        {
            losses.push(entity_loss(
                entry,
                "simple closed path has coincident consecutive points",
            ));
            continue;
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

    Ok(CopiousProjection {
        handled,
        decoded,
        losses,
        wire_edges,
        free_vertices,
    })
}

#[cfg(test)]
mod tests;
