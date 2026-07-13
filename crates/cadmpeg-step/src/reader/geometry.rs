// SPDX-License-Identifier: Apache-2.0
//! STEP representation units, placements, and geometry carriers.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{derive_reference_direction, Curve, CurveGeometry};
use cadmpeg_ir::ids::{CurveId, PointId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Point;

use crate::parse::{Exchange, RawRecord, Value};

pub(super) struct GeometryResult {
    pub typed_records: BTreeSet<u64>,
    pub warnings: Vec<String>,
}

pub(super) fn decode(exchange: &Exchange, ir: &mut CadIr) -> GeometryResult {
    let scale = length_scale(exchange).unwrap_or(1.0);
    let mut typed = BTreeSet::new();
    let mut warnings = Vec::new();
    let mut points = BTreeMap::new();
    let mut directions = BTreeMap::new();
    let mut vectors = BTreeMap::new();
    let mut placements = BTreeMap::new();

    for (&id, record) in &exchange.records {
        match record.simple_name() {
            Some("CARTESIAN_POINT") => match coordinates(record, 1, scale) {
                Some(position) => {
                    points.insert(id, position);
                    ir.model.points.push(Point {
                        id: PointId(format!("step:point:#{id}")),
                        position,
                    });
                    typed.insert(id);
                }
                None => warnings.push(format!("CARTESIAN_POINT #{id} has invalid coordinates")),
            },
            Some("DIRECTION") => match vector3(record.parameter(1), 1.0).and_then(normalize) {
                Some(direction) => {
                    directions.insert(id, direction);
                    typed.insert(id);
                }
                None => warnings.push(format!("DIRECTION #{id} is invalid or zero")),
            },
            _ => {}
        }
    }
    for (&id, record) in &exchange.records {
        if record.simple_name() == Some("VECTOR") {
            let value = record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|direction| directions.get(&direction).copied())
                .zip(record.parameter(2).and_then(Value::number))
                .map(|(direction, magnitude)| scale_vector(direction, magnitude * scale));
            if let Some(value) = value {
                vectors.insert(id, value);
                typed.insert(id);
            } else {
                warnings.push(format!(
                    "VECTOR #{id} has an invalid direction or magnitude"
                ));
            }
        }
    }
    for (&id, record) in &exchange.records {
        if record.simple_name() == Some("AXIS2_PLACEMENT_3D") {
            let placement = record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|point| points.get(&point).copied())
                .map(|origin| {
                    let axis = optional_direction(record.parameter(2), &directions)
                        .unwrap_or(Vector3::new(0.0, 0.0, 1.0));
                    let reference = optional_direction(record.parameter(3), &directions)
                        .unwrap_or_else(|| derive_reference_direction(axis));
                    (origin, axis, reference)
                });
            if let Some(placement) = placement {
                placements.insert(id, placement);
                typed.insert(id);
            } else {
                warnings.push(format!("AXIS2_PLACEMENT_3D #{id} has an invalid location"));
            }
        }
    }
    for (&id, record) in &exchange.records {
        let geometry = match record.simple_name() {
            Some("LINE") => record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|point| points.get(&point).copied())
                .zip(
                    record
                        .parameter(2)
                        .and_then(Value::reference)
                        .and_then(|vector| vectors.get(&vector).copied())
                        .and_then(normalize),
                )
                .map(|(origin, direction)| CurveGeometry::Line { origin, direction }),
            Some("CIRCLE") => record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(record.parameter(2).and_then(Value::number))
                .filter(|(_, radius)| radius.is_finite() && *radius > 0.0)
                .map(
                    |((center, axis, ref_direction), radius)| CurveGeometry::Circle {
                        center,
                        axis,
                        ref_direction,
                        radius: radius * scale,
                    },
                ),
            _ => continue,
        };
        if let Some(geometry) = geometry {
            ir.model.curves.push(Curve {
                id: CurveId(format!("step:curve:#{id}")),
                geometry,
                source_object: None,
            });
            typed.insert(id);
        } else {
            warnings.push(format!(
                "{} #{id} has invalid geometry",
                record.simple_name().unwrap()
            ));
        }
    }

    for (&id, record) in &exchange.records {
        if record.partials.iter().any(|partial| {
            matches!(
                partial.name.as_str(),
                "LENGTH_UNIT"
                    | "NAMED_UNIT"
                    | "SI_UNIT"
                    | "GEOMETRIC_REPRESENTATION_CONTEXT"
                    | "GLOBAL_UNIT_ASSIGNED_CONTEXT"
                    | "REPRESENTATION_CONTEXT"
            )
        }) || record.simple_name() == Some("SHAPE_REPRESENTATION")
        {
            typed.insert(id);
        }
    }
    GeometryResult {
        typed_records: typed,
        warnings,
    }
}

fn length_scale(exchange: &Exchange) -> Option<f64> {
    exchange.records.values().find_map(|record| {
        let unit = record.partial("SI_UNIT")?;
        if unit.parameters.get(1)?.enumeration()? != "METRE" {
            return None;
        }
        let prefix = match unit.parameters.first()? {
            Value::Omitted => 1.0,
            Value::Enumeration(prefix) => match prefix.as_str() {
                "EXA" => 1e18,
                "PETA" => 1e15,
                "TERA" => 1e12,
                "GIGA" => 1e9,
                "MEGA" => 1e6,
                "KILO" => 1e3,
                "HECTO" => 1e2,
                "DECA" => 1e1,
                "DECI" => 1e-1,
                "CENTI" => 1e-2,
                "MILLI" => 1e-3,
                "MICRO" => 1e-6,
                "NANO" => 1e-9,
                "PICO" => 1e-12,
                "FEMTO" => 1e-15,
                "ATTO" => 1e-18,
                _ => return None,
            },
            _ => return None,
        };
        Some(prefix * 1000.0)
    })
}

fn coordinates(record: &RawRecord, index: usize, scale: f64) -> Option<Point3> {
    let values = record.parameter(index)?.list()?;
    if values.len() != 3 {
        return None;
    }
    Some(Point3::new(
        values[0].number()? * scale,
        values[1].number()? * scale,
        values[2].number()? * scale,
    ))
}

fn vector3(value: Option<&Value>, scale: f64) -> Option<Vector3> {
    let values = value?.list()?;
    if values.len() != 3 {
        return None;
    }
    Some(Vector3::new(
        values[0].number()? * scale,
        values[1].number()? * scale,
        values[2].number()? * scale,
    ))
}

fn normalize(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm.is_finite() && norm > 0.0).then(|| scale_vector(vector, 1.0 / norm))
}

fn scale_vector(vector: Vector3, scale: f64) -> Vector3 {
    Vector3::new(vector.x * scale, vector.y * scale, vector.z * scale)
}

fn optional_direction(
    value: Option<&Value>,
    directions: &BTreeMap<u64, Vector3>,
) -> Option<Vector3> {
    match value? {
        Value::Omitted => None,
        Value::Reference(id) => directions.get(id).copied(),
        _ => None,
    }
}

trait RecordExt {
    fn simple_name(&self) -> Option<&str>;
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord>;
    fn parameter(&self, index: usize) -> Option<&Value>;
}

impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
    }
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord> {
        self.partials.iter().find(|partial| partial.name == name)
    }
    fn parameter(&self, index: usize) -> Option<&Value> {
        self.partials.first()?.parameters.get(index)
    }
}

trait ValueExt {
    fn number(&self) -> Option<f64>;
    fn reference(&self) -> Option<u64>;
    fn list(&self) -> Option<&[Value]>;
    fn enumeration(&self) -> Option<&str>;
}

impl ValueExt for Value {
    fn number(&self) -> Option<f64> {
        match self {
            Value::Real(v) => Some(*v),
            Value::Integer(v) => Some(*v as f64),
            _ => None,
        }
    }
    fn reference(&self) -> Option<u64> {
        match self {
            Value::Reference(id) => Some(*id),
            _ => None,
        }
    }
    fn list(&self) -> Option<&[Value]> {
        match self {
            Value::List(values) => Some(values),
            _ => None,
        }
    }
    fn enumeration(&self) -> Option<&str> {
        match self {
            Value::Enumeration(value) => Some(value),
            _ => None,
        }
    }
}
