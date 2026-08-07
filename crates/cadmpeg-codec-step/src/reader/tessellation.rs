// SPDX-License-Identifier: Apache-2.0
//! AP242 indexed tessellation decoding.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::{LossKind, LossNote};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::SourceObjectAssociation;

use crate::parse::{Exchange, RawRecord, Value};

use super::geometry::GeometryResult;
use super::topology::TopologyResult;

pub(super) struct TessellationResult {
    pub typed_records: BTreeSet<u64>,
    pub warnings: Vec<String>,
    pub losses: Vec<LossNote>,
}

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryResult,
    topology: &TopologyResult,
    ir: &mut CadIr,
) -> TessellationResult {
    let coordinates = exchange
        .records
        .iter()
        .filter_map(|(&id, record)| {
            if !has_entity(record, "COORDINATES_LIST") {
                return None;
            }
            coordinate_rows(record, geometry.length_scale).map(|vertices| (id, vertices))
        })
        .collect::<BTreeMap<_, _>>();
    let mut typed = BTreeSet::new();
    let mut warnings = Vec::new();
    let mut losses = Vec::new();
    let mut item_bodies = BTreeMap::<u64, BTreeSet<BodyId>>::new();
    let mut unresolved_items = BTreeSet::new();
    let mut declared_items = BTreeSet::new();
    for (&id, record) in &exchange.records {
        let Some(kind) = entity_kind(record, &["TESSELLATED_SOLID", "TESSELLATED_SHELL"]) else {
            continue;
        };
        let Some(items) = entity_parameter(record, kind, 0, 1).and_then(ValueExt::list) else {
            warnings.push(format!("{kind} #{id} has no structured items"));
            continue;
        };
        let item_ids = items
            .iter()
            .filter_map(ValueExt::reference)
            .collect::<Vec<_>>();
        declared_items.extend(item_ids.iter().copied());
        let candidates = linked_bodies(record, kind, topology);
        if candidates.is_empty() {
            unresolved_items.extend(item_ids.iter().copied());
            warnings.push(format!("{kind} #{id} has no decoded exact body link"));
        }
        for item in item_ids {
            item_bodies
                .entry(item)
                .or_default()
                .extend(candidates.iter().cloned());
        }
        typed.insert(id);
    }
    for (&item, bodies) in &item_bodies {
        if unresolved_items.contains(&item) || bodies.len() != 1 {
            let detail = if bodies.is_empty() {
                "no decoded body"
            } else if bodies.len() > 1 {
                "multiple candidate bodies"
            } else {
                "an unresolved container association"
            };
            let message =
                format!("tessellation item #{item} has {detail}; mesh retained as detached");
            warnings.push(message.clone());
            losses.push(LossNote::new(LossKind::ReferenceGraphNotClosed, message));
        }
    }
    for (&id, record) in &exchange.records {
        let Some(kind) = entity_kind(
            record,
            &[
                "TRIANGULATED_FACE",
                "COMPLEX_TRIANGULATED_FACE",
                "TRIANGULATED_SURFACE_SET",
                "COMPLEX_TRIANGULATED_SURFACE_SET",
            ],
        ) else {
            continue;
        };
        let base_kind = if matches!(kind, "TRIANGULATED_FACE" | "COMPLEX_TRIANGULATED_FACE") {
            "TESSELLATED_FACE"
        } else {
            "TESSELLATED_SURFACE_SET"
        };
        let Some(coordinate_id) =
            inherited_parameter(record, base_kind, 0).and_then(ValueExt::reference)
        else {
            warnings.push(format!("{kind} #{id} has no COORDINATES_LIST reference"));
            continue;
        };
        let Some(vertices) = coordinates.get(&coordinate_id) else {
            warnings.push(format!("{kind} #{id} has no resolved COORDINATES_LIST"));
            continue;
        };
        let (triangles, strip_lengths) = match kind {
            "TRIANGULATED_FACE" | "TRIANGULATED_SURFACE_SET" => (
                entity_parameter(record, kind, 1, own_parameter_offset(kind))
                    .and_then(triangle_rows),
                Vec::new(),
            ),
            "COMPLEX_TRIANGULATED_FACE" | "COMPLEX_TRIANGULATED_SURFACE_SET" => complex_triangles(
                entity_parameter(record, kind, 1, own_parameter_offset(kind)),
                entity_parameter(record, kind, 2, own_parameter_offset(kind)),
            ),
            _ => unreachable!("tessellation kind was checked above"),
        };
        let Some(triangles) = triangles.filter(|triangles| !triangles.is_empty()) else {
            warnings.push(format!("{kind} #{id} has no triangle indices"));
            continue;
        };
        let pnindex = match entity_parameter(record, kind, 0, own_parameter_offset(kind)) {
            None | Some(Value::Omitted) => Vec::new(),
            Some(value) => {
                let Some(indices) = index_list(Some(value)) else {
                    warnings.push(format!("{kind} #{id} has an invalid pnindex"));
                    continue;
                };
                indices
            }
        };
        let (local_vertices, local_triangles, coordinate_indices) = if pnindex.is_empty() {
            if triangles
                .iter()
                .flatten()
                .any(|index| *index == 0 || *index as usize > vertices.len())
            {
                warnings.push(format!(
                    "{kind} #{id} has an out-of-range one-based coordinate index"
                ));
                continue;
            }
            let coordinate_indices = triangles.iter().flatten().copied().collect::<BTreeSet<_>>();
            let local_index = coordinate_indices
                .iter()
                .enumerate()
                .map(|(local, global)| (*global, local as u32))
                .collect::<BTreeMap<_, _>>();
            let local_vertices = coordinate_indices
                .iter()
                .map(|index| vertices[*index as usize - 1])
                .collect::<Vec<_>>();
            let local_triangles = triangles
                .iter()
                .map(|triangle| triangle.map(|index| local_index[&index]))
                .collect::<Vec<_>>();
            (local_vertices, local_triangles, Some(coordinate_indices))
        } else {
            if pnindex
                .iter()
                .any(|index| *index == 0 || *index as usize > vertices.len())
                || triangles
                    .iter()
                    .flatten()
                    .any(|index| *index == 0 || *index as usize > pnindex.len())
            {
                warnings.push(format!(
                    "{kind} #{id} has an out-of-range one-based tessellation index"
                ));
                continue;
            }
            (
                pnindex
                    .iter()
                    .map(|index| vertices[*index as usize - 1])
                    .collect(),
                triangles
                    .iter()
                    .map(|triangle| triangle.map(|index| index - 1))
                    .collect(),
                None,
            )
        };
        let source_normals =
            normal_rows(inherited_parameter(record, base_kind, 2)).unwrap_or_default();
        let normals = match source_normals.len() {
            0 => Vec::new(),
            1 => vec![source_normals[0]; local_vertices.len()],
            count if count == local_vertices.len() => source_normals,
            count if pnindex.is_empty() && count == vertices.len() => coordinate_indices
                .expect("coordinate indices exist without pnindex")
                .iter()
                .map(|index| source_normals[*index as usize - 1])
                .collect(),
            count => {
                warnings.push(format!(
                    "{kind} #{id} carries {count} normals for {} coordinates",
                    local_vertices.len()
                ));
                Vec::new()
            }
        };
        if let Some(surface_step) = complex_triangulated_face_surface(record) {
            let surface_id = format!("step:data:surface#{surface_step}");
            if let Some(surface) = ir
                .model
                .surfaces
                .iter_mut()
                .find(|surface| surface.id.0 == surface_id)
            {
                surface
                    .source_object
                    .get_or_insert_with(|| SourceObjectAssociation {
                        format: "step".into(),
                        object_id: format!("#{id}"),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    });
            }
        }
        if !declared_items.contains(&id) {
            let message = format!(
                "tessellation item #{id} is not declared by an exact body container; mesh retained as detached"
            );
            warnings.push(message.clone());
            losses.push(LossNote::new(LossKind::ReferenceGraphNotClosed, message));
        }
        ir.model.tessellations.push(Tessellation {
            faces: Vec::new(),
            chordal_deflection: None,
            id: format!("step:tessellation:mesh#{id}"),
            body: (!unresolved_items.contains(&id))
                .then(|| item_bodies.get(&id))
                .flatten()
                .filter(|bodies| bodies.len() == 1)
                .and_then(|bodies| bodies.iter().next().cloned()),
            source_object: (!declared_items.contains(&id)
                || unresolved_items.contains(&id)
                || item_bodies.get(&id).is_none_or(|bodies| bodies.len() != 1))
            .then(|| SourceObjectAssociation {
                format: "step".into(),
                object_id: format!("#{id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
            vertices: local_vertices,
            triangles: local_triangles,
            strip_lengths,
            normals,
            channels: Vec::new(),
        });
        typed.extend([id, coordinate_id]);
    }
    if !ir.model.tessellations.is_empty() {
        for (&id, record) in &exchange.records {
            if has_entity(record, "TESSELLATED_SHAPE_REPRESENTATION")
                || has_entity(record, "TESSELLATED_SOLID")
                || has_entity(record, "TESSELLATED_SHELL")
            {
                typed.insert(id);
            }
        }
    }
    TessellationResult {
        typed_records: typed,
        warnings,
        losses,
    }
}

fn complex_triangulated_face_surface(record: &RawRecord) -> Option<u64> {
    has_entity(record, "COMPLEX_TRIANGULATED_FACE")
        .then(|| inherited_parameter(record, "TESSELLATED_FACE", 3))
        .flatten()
        .and_then(ValueExt::reference)
}

fn linked_bodies(record: &RawRecord, kind: &str, topology: &TopologyResult) -> BTreeSet<BodyId> {
    let Some(link) = entity_parameter(record, kind, 1, 1).and_then(ValueExt::reference) else {
        return BTreeSet::new();
    };
    match kind {
        "TESSELLATED_SOLID" => topology
            .body_by_root
            .get(&link)
            .into_iter()
            .flatten()
            .cloned()
            .collect(),
        "TESSELLATED_SHELL" => topology
            .body_by_shell
            .get(&link)
            .cloned()
            .unwrap_or_default(),
        _ => BTreeSet::new(),
    }
}

fn index_list(value: Option<&Value>) -> Option<Vec<u32>> {
    value?
        .list()?
        .iter()
        .map(|value| u32::try_from(value.integer()?).ok())
        .collect()
}

fn coordinate_rows(record: &RawRecord, scale: f64) -> Option<Vec<Point3>> {
    record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
        .filter_map(ValueExt::list)
        .find_map(|rows| {
            rows.iter()
                .map(|row| {
                    let values = row.list()?;
                    if values.len() != 3 {
                        return None;
                    }
                    let point = Point3::new(
                        values[0].number()? * scale,
                        values[1].number()? * scale,
                        values[2].number()? * scale,
                    );
                    [point.x, point.y, point.z]
                        .iter()
                        .all(|coordinate| coordinate.is_finite())
                        .then_some(point)
                })
                .collect::<Option<Vec<_>>>()
                .filter(|vertices| !vertices.is_empty())
        })
}

fn has_entity(record: &RawRecord, name: &str) -> bool {
    entity_kind(record, &[name]).is_some()
}

fn entity_kind<'a>(record: &'a RawRecord, names: &[&str]) -> Option<&'a str> {
    record
        .partials
        .iter()
        .find(|partial| names.iter().any(|name| *name == partial.name))
        .map(|partial| partial.name.as_str())
}

fn entity_parameter<'a>(
    record: &'a RawRecord,
    entity: &str,
    index: usize,
    simple_offset: usize,
) -> Option<&'a Value> {
    let partial = record
        .partials
        .iter()
        .find(|partial| partial.name == entity)?;
    let offset = if record.partials.len() == 1 {
        simple_offset
    } else {
        0
    };
    partial.parameters.get(index + offset)
}

fn own_parameter_offset(entity: &str) -> usize {
    match entity {
        "TRIANGULATED_FACE" | "COMPLEX_TRIANGULATED_FACE" => 5,
        "TRIANGULATED_SURFACE_SET" | "COMPLEX_TRIANGULATED_SURFACE_SET" => 4,
        _ => unreachable!("tessellation entity has no indexed subtype fields"),
    }
}

fn inherited_parameter<'a>(record: &'a RawRecord, entity: &str, index: usize) -> Option<&'a Value> {
    if record.partials.len() == 1 {
        record.parameter(index + 1)
    } else {
        entity_parameter(record, entity, index, 0)
    }
}

fn triangle_rows(value: &Value) -> Option<Vec<[u32; 3]>> {
    let rows = value.list()?;
    rows.iter()
        .map(|row| {
            let values = row.list()?;
            if values.len() != 3 {
                return None;
            }
            Some([
                u32::try_from(values[0].integer()?).ok()?,
                u32::try_from(values[1].integer()?).ok()?,
                u32::try_from(values[2].integer()?).ok()?,
            ])
        })
        .collect::<Option<Vec<_>>>()
}

fn complex_triangles(
    strips: Option<&Value>,
    fans: Option<&Value>,
) -> (Option<Vec<[u32; 3]>>, Vec<u32>) {
    let strips = index_rows(strips).unwrap_or_default();
    let fans = index_rows(fans).unwrap_or_default();
    let mut triangles = Vec::new();
    for strip in strips {
        for index in 0..strip.len().saturating_sub(2) {
            triangles.push(if index % 2 == 0 {
                [strip[index], strip[index + 1], strip[index + 2]]
            } else {
                [strip[index + 1], strip[index], strip[index + 2]]
            });
        }
    }
    for fan in fans {
        for index in 1..fan.len().saturating_sub(1) {
            triangles.push([fan[0], fan[index], fan[index + 1]]);
        }
    }
    ((!triangles.is_empty()).then_some(triangles), Vec::new())
}

fn index_rows(value: Option<&Value>) -> Option<Vec<Vec<u32>>> {
    Some(
        value?
            .list()?
            .iter()
            .filter_map(|row| {
                let indices = row
                    .list()?
                    .iter()
                    .map(|value| u32::try_from(value.integer()?).ok())
                    .collect::<Option<Vec<_>>>()?;
                (indices.len() >= 3).then_some(indices)
            })
            .collect(),
    )
}

fn normal_rows(value: Option<&Value>) -> Option<Vec<Vector3>> {
    value?
        .list()?
        .iter()
        .map(|row| {
            let values = row.list()?;
            if values.len() != 3 {
                return None;
            }
            let normal = Vector3::new(
                values[0].number()?,
                values[1].number()?,
                values[2].number()?,
            );
            let length = normal.norm();
            (length.is_finite() && length > 0.0)
                .then(|| Vector3::new(normal.x / length, normal.y / length, normal.z / length))
        })
        .collect()
}
trait RecordExt {
    fn parameter(&self, index: usize) -> Option<&Value>;
}
impl RecordExt for RawRecord {
    fn parameter(&self, index: usize) -> Option<&Value> {
        self.partials
            .first()
            .and_then(|partial| partial.parameters.get(index))
    }
}
trait ValueExt {
    fn reference(&self) -> Option<u64>;
    fn list(&self) -> Option<&[Value]>;
    fn number(&self) -> Option<f64>;
    fn integer(&self) -> Option<i64>;
}
impl ValueExt for Value {
    fn reference(&self) -> Option<u64> {
        if let Value::Reference(id) = self {
            Some(*id)
        } else {
            None
        }
    }
    fn list(&self) -> Option<&[Value]> {
        if let Value::List(values) = self {
            Some(values)
        } else {
            None
        }
    }
    fn number(&self) -> Option<f64> {
        match self {
            Value::Real(value) => Some(*value),
            Value::Integer(value) => Some(*value as f64),
            _ => None,
        }
    }
    fn integer(&self) -> Option<i64> {
        if let Value::Integer(value) = self {
            Some(*value)
        } else {
            None
        }
    }
}
