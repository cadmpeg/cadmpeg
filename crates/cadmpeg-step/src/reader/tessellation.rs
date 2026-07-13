// SPDX-License-Identifier: Apache-2.0
//! AP242 indexed tessellation decoding.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::tessellation::Tessellation;

use crate::parse::{Exchange, RawRecord, Value};

use super::geometry::GeometryResult;

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
            if record.simple_name() != Some("COORDINATES_LIST") {
                return None;
            }
            coordinate_rows(record, geometry.length_scale).map(|vertices| (id, vertices))
        })
        .collect::<BTreeMap<_, _>>();
    let mut typed = BTreeSet::new();
    let mut warnings = Vec::new();
    for (&id, record) in &exchange.records {
        if !matches!(
            record.simple_name(),
            Some("TRIANGULATED_FACE")
                | Some("COMPLEX_TRIANGULATED_FACE")
                | Some("TRIANGULATED_SURFACE_SET")
        ) {
            continue;
        }
        let Some((coordinate_id, vertices)) = record
            .parameters()
            .iter()
            .filter_map(ValueExt::reference)
            .find_map(|reference| {
                coordinates
                    .get(&reference)
                    .map(|vertices| (reference, vertices))
            })
        else {
            warnings.push(format!(
                "{} #{id} has no resolved COORDINATES_LIST",
                record.simple_name().unwrap()
            ));
            continue;
        };
        let Some(triangles) = record
            .parameters()
            .iter()
            .filter_map(triangle_rows)
            .find(|triangles| !triangles.is_empty())
        else {
            warnings.push(format!(
                "{} #{id} has no triangle indices",
                record.simple_name().unwrap()
            ));
            continue;
        };
        if triangles
            .iter()
            .flatten()
            .any(|index| *index == 0 || *index as usize > vertices.len())
        {
            warnings.push(format!(
                "{} #{id} has an out-of-range one-based coordinate index",
                record.simple_name().unwrap()
            ));
            continue;
        }
        ir.model.tessellations.push(Tessellation {
            id: format!("step:tessellation:mesh#{id}"),
            body: None,
            source_object: None,
            vertices: vertices.clone(),
            triangles: triangles
                .into_iter()
                .map(|triangle| [triangle[0] - 1, triangle[1] - 1, triangle[2] - 1])
                .collect(),
            strip_lengths: Vec::new(),
            normals: Vec::new(),
            channels: Vec::new(),
        });
        typed.extend([id, coordinate_id]);
    }
    if !ir.model.tessellations.is_empty() {
        for (&id, record) in &exchange.records {
            if matches!(
                record.simple_name(),
                Some("TESSELLATED_SHAPE_REPRESENTATION")
                    | Some("TESSELLATED_SOLID")
                    | Some("TESSELLATED_SHELL")
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
                    Some(Point3::new(
                        values[0].number()? * scale,
                        values[1].number()? * scale,
                        values[2].number()? * scale,
                    ))
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
trait RecordExt {
    fn simple_name(&self) -> Option<&str>;
    fn parameters(&self) -> &[Value];
}
impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
    }
    fn parameters(&self) -> &[Value] {
        &self.partials[0].parameters
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
