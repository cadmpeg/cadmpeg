// SPDX-License-Identifier: Apache-2.0
//! Copious point, linear-path, and presentation tuple projection.

use super::geometry::{entity_loss, resolve_transform, source_object};
use crate::directory::DirectoryEntry;
use crate::global::{Dialect, ProjectedGlobal};
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{refuse_local_limit, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const MAX_COPIOUS_TUPLES: usize = 1_000_000;

pub(super) struct CopiousProjectionOutcome {
    pub(super) decoded: BTreeSet<u32>,
    pub(super) losses: Vec<LossNote>,
    pub(super) wire_edges: Vec<EdgeId>,
    pub(super) free_vertices: Vec<VertexId>,
}

impl CopiousProjectionOutcome {
    pub(super) fn merge_into(
        self,
        decoded: &mut BTreeSet<u32>,
        losses: &mut Vec<LossNote>,
        wire_edges: &mut Vec<EdgeId>,
        free_vertices: &mut Vec<VertexId>,
    ) {
        decoded.extend(self.decoded);
        losses.extend(self.losses);
        wire_edges.extend(self.wire_edges);
        free_vertices.extend(self.free_vertices);
    }
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

fn presentation_loss(entry: &DirectoryEntry, message: impl Into<String>) -> LossNote {
    IgesLossCode::DisplayDataNotProjected
        .note(format!(
            "IGES entity type {} form {} display data was not projected: {}",
            entry.entity_type,
            entry.form,
            message.into()
        ))
        .with_provenance(entry.loss_provenance())
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
            (index.is_finite() && index >= i128::MIN as f64 && index <= i128::MAX as f64)
                .then_some(index as i128)
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

fn planar_cross(left: [f64; 2], right: [f64; 2], point: [f64; 2]) -> f64 {
    (right[0] - left[0]) * (point[1] - left[1]) - (right[1] - left[1]) * (point[0] - left[0])
}

fn planar_point_on_segment(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> bool {
    planar_cross(start, end, point) == 0.0
        && point[0] >= start[0].min(end[0])
        && point[0] <= start[0].max(end[0])
        && point[1] >= start[1].min(end[1])
        && point[1] <= start[1].max(end[1])
}

fn segment_intersects_beyond_endpoint(
    first: [[f64; 2]; 2],
    second: [[f64; 2]; 2],
    allowed_endpoint: Option<[f64; 2]>,
) -> bool {
    let [a, b] = first;
    let [c, d] = second;
    let orientations = [
        planar_cross(a, b, c),
        planar_cross(a, b, d),
        planar_cross(c, d, a),
        planar_cross(c, d, b),
    ];
    let opposite =
        |left: f64, right: f64| (left > 0.0 && right < 0.0) || (left < 0.0 && right > 0.0);
    if opposite(orientations[0], orientations[1]) && opposite(orientations[2], orientations[3]) {
        return true;
    }

    let mut contacts = [(c, orientations[0]), (d, orientations[1])]
        .into_iter()
        .filter_map(|(point, orientation)| {
            (orientation == 0.0 && planar_point_on_segment(point, a, b)).then_some(point)
        })
        .chain(
            [(a, orientations[2]), (b, orientations[3])]
                .into_iter()
                .filter_map(|(point, orientation)| {
                    (orientation == 0.0 && planar_point_on_segment(point, c, d)).then_some(point)
                }),
        );
    contacts.any(|point| Some(point) != allowed_endpoint)
}

fn has_form_63_self_intersection(points: &[Point3]) -> bool {
    let mut planar_points = points
        .iter()
        .map(|point| [point.x, point.y])
        .collect::<Vec<_>>();
    if planar_points.len() < 3 {
        return false;
    }
    let last = planar_points.len() - 1;
    planar_points[last] = planar_points[0];
    let segment_count = last;
    for first_index in 0..segment_count {
        for second_index in first_index + 1..segment_count {
            let allowed_endpoint = if second_index == first_index + 1 {
                Some(planar_points[second_index])
            } else if first_index == 0 && second_index + 1 == segment_count {
                Some(planar_points[0])
            } else {
                None
            };
            if segment_intersects_beyond_endpoint(
                [planar_points[first_index], planar_points[first_index + 1]],
                [planar_points[second_index], planar_points[second_index + 1]],
                allowed_endpoint,
            ) {
                return true;
            }
        }
    }
    false
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<CopiousProjectionOutcome, CodecError> {
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
    let mut free_vertices = Vec::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 106 && expected_interpretation(entry.form).is_some())
    {
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
        if matches!(entry.form, 11..=13) {
            let minimum_tuple_count = if matches!(global.dialect(), Dialect::V4_0) {
                1
            } else {
                2
            };
            if tuple_count < minimum_tuple_count {
                losses.push(entity_loss(
                    entry,
                    format!(
                        "linear paths require at least {minimum_tuple_count} tuple(s) under the declared dialect"
                    ),
                ));
                continue;
            }
        }
        if entry.form == 63 && tuple_count < 2 {
            losses.push(entity_loss(
                entry,
                "simple closed paths require at least two tuples",
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
        let definition_points = values
            .chunks_exact(tuple_width)
            .map(|tuple| {
                let z = match common_z {
                    Some(z) => z,
                    None => tuple[2],
                };
                Point3::new(tuple[0] * factor, tuple[1] * factor, z * factor)
            })
            .collect::<Vec<_>>();
        let points = definition_points
            .iter()
            .copied()
            .map(|point| transform.point(point))
            .collect::<Vec<_>>();
        if presentation_form(entry.form) {
            losses.push(presentation_loss(
                entry,
                "copious presentation tuples have no neutral display carrier",
            ));
            continue;
        }
        let projects_as_points = matches!(entry.form, 1..=3)
            || (matches!(entry.form, 11..=13)
                && tuple_count == 1
                && matches!(global.dialect(), Dialect::V4_0));
        if projects_as_points {
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
        if entry.form == 63 && !points_coincident(points[0], points[points.len() - 1], resolution) {
            losses.push(entity_loss(
                entry,
                "simple closed path endpoints disagree beyond the minimum resolution",
            ));
            continue;
        }
        if entry.form == 63 && has_forbidden_form_63_duplicate(&points, resolution) {
            losses.push(entity_loss(
                entry,
                if points.len() == 2 {
                    "simple closed path has no non-zero segment"
                } else {
                    "simple closed path has coincident non-endpoint points"
                },
            ));
            continue;
        }
        if entry.form == 63 && has_form_63_self_intersection(&definition_points) {
            losses.push(entity_loss(
                entry,
                "simple closed path intersects itself away from shared endpoints",
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

    Ok(CopiousProjectionOutcome {
        decoded,
        losses,
        wire_edges,
        free_vertices,
    })
}

#[cfg(test)]
mod tests;
