// SPDX-License-Identifier: Apache-2.0
//! AP242 indexed tessellation decoding.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, ShellId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::Tessellation;

use crate::parse::{Exchange, RawRecord, Value};

use super::geometry::GeometryResult;
use crate::vocab::{
    COMPLEX_TRIANGULATED_FACE, COMPLEX_TRIANGULATED_SURFACE_SET, COORDINATES_LIST,
    TESSELLATED_SHAPE_REPRESENTATION, TESSELLATED_SHELL, TESSELLATED_SOLID, TRIANGULATED_FACE,
    TRIANGULATED_SURFACE_SET,
};

pub(super) struct TessellationResult {
    pub typed_records: BTreeSet<u64>,
    pub warnings: Vec<String>,
}

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryResult,
    ir: &mut CadIr,
) -> TessellationResult {
    let coordinates = exchange
        .records
        .iter()
        .filter_map(|(&id, record)| {
            if record.simple_name() != Some(COORDINATES_LIST) {
                return None;
            }
            coordinate_rows(record, geometry.length_scale).map(|vertices| (id, vertices))
        })
        .collect::<BTreeMap<_, _>>();
    let mut typed = BTreeSet::new();
    let mut warnings = Vec::new();
    let mut item_bodies = BTreeMap::new();
    for (&id, record) in &exchange.records {
        let body = match record.simple_name() {
            Some(TESSELLATED_SOLID) => record
                .parameter(2)
                .and_then(ValueExt::reference)
                .map(|solid| BodyId(format!("step:data:body#{solid}")))
                .filter(|body| {
                    ir.model
                        .bodies
                        .iter()
                        .any(|candidate| candidate.id == *body)
                }),
            Some(TESSELLATED_SHELL) => record
                .parameter(2)
                .and_then(ValueExt::reference)
                .and_then(|shell| body_for_shell(shell, ir)),
            _ => continue,
        };
        let Some(body) = body else {
            if !matches!(record.parameter(2), None | Some(Value::Omitted)) {
                warnings.push(format!(
                    "{} #{id} has no decoded exact body link",
                    record.simple_name().expect("matched tessellated body")
                ));
            }
            continue;
        };
        let Some(items) = record.parameter(1).and_then(ValueExt::list) else {
            warnings.push(format!(
                "{} #{id} has no structured items",
                record.simple_name().expect("matched tessellated body")
            ));
            continue;
        };
        for item in items.iter().filter_map(ValueExt::reference) {
            item_bodies.insert(item, body.clone());
        }
        typed.insert(id);
    }
    for (&id, record) in &exchange.records {
        if !matches!(
            record.simple_name(),
            Some(
                TRIANGULATED_FACE
                    | COMPLEX_TRIANGULATED_FACE
                    | TRIANGULATED_SURFACE_SET
                    | COMPLEX_TRIANGULATED_SURFACE_SET
            )
        ) {
            continue;
        }
        let Some(coordinate_id) = record.parameter(1).and_then(ValueExt::reference) else {
            warnings.push(format!(
                "{} #{id} has no COORDINATES_LIST reference",
                record.simple_name().expect("matched simple name")
            ));
            continue;
        };
        let Some(vertices) = coordinates.get(&coordinate_id) else {
            warnings.push(format!(
                "{} #{id} has no resolved COORDINATES_LIST",
                record.simple_name().expect("matched simple name")
            ));
            continue;
        };
        let (triangles, strip_lengths) = match record.simple_name() {
            Some(TRIANGULATED_FACE) => (record.parameter(6).and_then(triangle_rows), Vec::new()),
            Some(TRIANGULATED_SURFACE_SET) => {
                (record.parameter(5).and_then(triangle_rows), Vec::new())
            }
            Some(COMPLEX_TRIANGULATED_FACE) => {
                complex_triangles(record.parameter(6), record.parameter(7))
            }
            Some(COMPLEX_TRIANGULATED_SURFACE_SET) => {
                complex_triangles(record.parameter(5), record.parameter(6))
            }
            _ => (None, Vec::new()),
        };
        let Some(triangles) = triangles.filter(|triangles| !triangles.is_empty()) else {
            warnings.push(format!(
                "{} #{id} has no triangle indices",
                record.simple_name().expect("matched simple name")
            ));
            continue;
        };
        let pnindex_parameter = match record.simple_name() {
            Some(TRIANGULATED_FACE | COMPLEX_TRIANGULATED_FACE) => 5,
            _ => 4,
        };
        let pnindex = match record.parameter(pnindex_parameter) {
            None | Some(Value::Omitted) => Vec::new(),
            Some(value) => {
                let Some(indices) = index_list(Some(value)) else {
                    warnings.push(format!(
                        "{} #{id} has an invalid pnindex",
                        record.simple_name().expect("matched simple name")
                    ));
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
                    "{} #{id} has an out-of-range one-based coordinate index",
                    record.simple_name().expect("matched simple name")
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
                    "{} #{id} has an out-of-range one-based tessellation index",
                    record.simple_name().expect("matched simple name")
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
        let source_normals = normal_rows(record.parameter(3)).unwrap_or_default();
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
                    "{} #{id} carries {count} normals for {} coordinates",
                    record.simple_name().expect("matched simple name"),
                    local_vertices.len()
                ));
                Vec::new()
            }
        };
        ir.model.tessellations.push(Tessellation {
            faces: Vec::new(),
            chordal_deflection: None,
            id: format!("step:tessellation:mesh#{id}"),
            body: item_bodies.get(&id).cloned(),
            source_object: None,
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
            if matches!(
                record.simple_name(),
                Some(TESSELLATED_SHAPE_REPRESENTATION | TESSELLATED_SOLID | TESSELLATED_SHELL)
            ) {
                typed.insert(id);
            }
        }
    }
    TessellationResult {
        typed_records: typed,
        warnings,
    }
}

fn body_for_shell(shell_step: u64, ir: &CadIr) -> Option<BodyId> {
    let shell = ir
        .model
        .shells
        .iter()
        .find(|shell| shell.id == ShellId(format!("step:data:shell#{shell_step}")))?;
    ir.model
        .regions
        .iter()
        .find(|region| region.id == shell.region)
        .map(|region| region.body.clone())
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
        .parameters()
        .iter()
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
    fn simple_name(&self) -> Option<&str>;
    fn parameters(&self) -> &[Value];
    fn parameter(&self, index: usize) -> Option<&Value>;
}
impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
    }
    fn parameters(&self) -> &[Value] {
        self.partials
            .first()
            .map(|partial| partial.parameters.as_slice())
            .unwrap_or_default()
    }
    fn parameter(&self, index: usize) -> Option<&Value> {
        self.parameters().get(index)
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
