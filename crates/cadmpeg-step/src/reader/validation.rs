// SPDX-License-Identifier: Apache-2.0
//! Geometric validation-property decoding and mesh self-checks.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point3;

use crate::parse::{Exchange, RawRecord, Value};

use super::geometry::GeometryResult;
use crate::vocab::{
    AREA_MEASURE, CARTESIAN_POINT, DERIVED_UNIT, MEASURE_REPRESENTATION_ITEM, PROPERTY_DEFINITION,
    PROPERTY_DEFINITION_REPRESENTATION, REPRESENTATION, SHAPE_REPRESENTATION, VOLUME_MEASURE,
};

pub(super) struct ValidationResult {
    pub typed_records: BTreeSet<u64>,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy)]
enum Expected {
    Area(f64),
    Volume(f64),
    Centroid(Point3),
}

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryResult,
    ir: &mut CadIr,
) -> ValidationResult {
    let representations = exchange
        .records
        .iter()
        .filter_map(|(&id, record)| {
            if !matches!(
                record.simple_name(),
                Some(REPRESENTATION | SHAPE_REPRESENTATION)
            ) {
                return None;
            }
            Some((id, record.parameter(1)?.list()?.first()?.reference()?))
        })
        .collect::<BTreeMap<_, _>>();
    let properties = exchange
        .records
        .iter()
        .filter_map(|(&id, record)| {
            if record.simple_name() == Some(PROPERTY_DEFINITION)
                && record
                    .parameter(0)?
                    .text()?
                    .eq_ignore_ascii_case("geometric validation property")
            {
                Some((
                    id,
                    record
                        .parameter(1)
                        .and_then(ValueExt::text)
                        .unwrap_or_default(),
                ))
            } else {
                None
            }
        })
        .collect::<BTreeMap<_, _>>();
    let computed = mesh_properties(ir);
    let mut typed = BTreeSet::new();
    let mut validation_points = BTreeSet::new();
    let mut validation_representations = BTreeSet::new();
    let mut notes = Vec::new();
    let mut warnings = Vec::new();

    for (&relation_id, relation) in &exchange.records {
        if relation.simple_name() != Some(PROPERTY_DEFINITION_REPRESENTATION) {
            continue;
        }
        let Some(property_id) = relation.parameter(0).and_then(ValueExt::reference) else {
            continue;
        };
        let Some(description) = properties.get(&property_id) else {
            continue;
        };
        let Some(representation_id) = relation.parameter(1).and_then(ValueExt::reference) else {
            continue;
        };
        let Some(&item_id) = representations.get(&representation_id) else {
            continue;
        };
        let Some(item) = exchange.records.get(&item_id) else {
            continue;
        };
        let expected = expected_value(item, exchange, geometry.length_scale);
        let Some(expected) = expected else {
            warnings.push(format!(
                "geometric validation property #{property_id} has an unsupported value"
            ));
            continue;
        };
        if matches!(expected, Expected::Centroid(_)) {
            validation_points.insert(item_id);
        }
        validation_representations.insert(representation_id);
        typed.extend([property_id, relation_id, representation_id, item_id]);
        if let Some(unit) = item.parameter(2).and_then(ValueExt::reference) {
            collect_unit_records(unit, exchange, &mut typed);
        }
        let (kind, expected_text, actual) = match expected {
            Expected::Area(value) => ("surface area", value.to_string(), computed.map(|p| p.area)),
            Expected::Volume(value) => ("volume", value.to_string(), computed.map(|p| p.volume)),
            Expected::Centroid(value) => (
                "centroid",
                format!("({},{},{})", value.x, value.y, value.z),
                computed.map(|p| p.centroid_distance(value)),
            ),
        };
        if let Some(actual) = actual {
            let actual_text = match expected {
                Expected::Centroid(_) => format!("distance {actual}"),
                _ => actual.to_string(),
            };
            notes.push(format!(
                "geometric validation {kind} {description}: expected {expected_text}, tessellation approximation {actual_text}"
            ));
        } else {
            notes.push(format!(
                "geometric validation {kind} {description}: expected {expected_text}"
            ));
        }
    }
    let mut referenced_validation_points = BTreeSet::new();
    for (&record_id, record) in &exchange.records {
        if validation_representations.contains(&record_id) {
            continue;
        }
        for value in record
            .partials
            .iter()
            .flat_map(|partial| &partial.parameters)
        {
            collect_validation_references(
                value,
                &validation_points,
                &mut referenced_validation_points,
            );
        }
    }
    ir.model.points.retain(|point| {
        let id = step_id(&point.id.0);
        !validation_points.contains(&id) || referenced_validation_points.contains(&id)
    });
    ValidationResult {
        typed_records: typed,
        notes,
        warnings,
    }
}

fn expected_value(record: &RawRecord, exchange: &Exchange, scale: f64) -> Option<Expected> {
    if record.simple_name() == Some(CARTESIAN_POINT) {
        let values = record.parameter(1)?.list()?;
        if values.len() != 3 {
            return None;
        }
        return Some(Expected::Centroid(Point3::new(
            values[0].number()? * scale,
            values[1].number()? * scale,
            values[2].number()? * scale,
        )));
    }
    if record.simple_name() != Some(MEASURE_REPRESENTATION_ITEM) {
        return None;
    }
    match record.parameter(1)? {
        Value::Typed(kind, value) if kind == AREA_MEASURE => Some(Expected::Area(
            value.number()? * measure_scale(record, exchange, scale, 2),
        )),
        Value::Typed(kind, value) if kind == VOLUME_MEASURE => Some(Expected::Volume(
            value.number()? * measure_scale(record, exchange, scale, 3),
        )),
        _ => None,
    }
}

fn measure_scale(record: &RawRecord, exchange: &Exchange, fallback: f64, order: i32) -> f64 {
    record
        .parameter(2)
        .and_then(ValueExt::reference)
        .and_then(|unit| exchange.records.get(&unit))
        .and_then(|unit| {
            if unit.simple_name() != Some(DERIVED_UNIT) {
                return None;
            }
            unit.parameter(0)?
                .list()?
                .iter()
                .try_fold(1.0, |scale, element| {
                    let element = exchange.records.get(&element.reference()?)?;
                    let base = element.parameter(0)?.reference()?;
                    let exponent = element.parameter(1)?.number()?;
                    let base =
                        super::geometry::unit_scale_mm(base, exchange, &mut BTreeSet::new())?;
                    Some(scale * base.powf(exponent))
                })
        })
        .unwrap_or_else(|| fallback.powi(order))
}

fn collect_unit_records(id: u64, exchange: &Exchange, typed: &mut BTreeSet<u64>) {
    typed.insert(id);
    let Some(record) = exchange.records.get(&id) else {
        return;
    };
    if record.simple_name() != Some(DERIVED_UNIT) {
        return;
    }
    for element in record
        .parameter(0)
        .and_then(ValueExt::list)
        .into_iter()
        .flatten()
        .filter_map(ValueExt::reference)
    {
        typed.insert(element);
        if let Some(base) = exchange
            .records
            .get(&element)
            .and_then(|record| record.parameter(0))
            .and_then(ValueExt::reference)
        {
            typed.insert(base);
        }
    }
}

#[derive(Clone, Copy)]
struct MeshProperties {
    area: f64,
    volume: f64,
    centroid: Point3,
}

impl MeshProperties {
    fn centroid_distance(self, expected: Point3) -> f64 {
        (self.centroid.x - expected.x)
            .hypot(self.centroid.y - expected.y)
            .hypot(self.centroid.z - expected.z)
    }
}

fn mesh_properties(ir: &CadIr) -> Option<MeshProperties> {
    let body = (ir.model.bodies.len() == 1).then(|| ir.model.bodies[0].id.clone())?;
    let meshes = ir
        .model
        .tessellations
        .iter()
        .filter(|mesh| mesh.body.as_ref() == Some(&body));
    let mut area = 0.0;
    let mut area_centroid = [0.0; 3];
    let mut signed_volume = 0.0;
    let mut volume_centroid = [0.0; 3];
    let mut triangles = 0usize;
    let mut watertight = true;
    let mut coordinate_scale = 0.0_f64;
    for mesh in meshes {
        let mut edge_uses = BTreeMap::<(u32, u32), usize>::new();
        for triangle in &mesh.triangles {
            let [a, b, c] = triangle.map(|index| mesh.vertices.get(index as usize).copied());
            let (Some(a), Some(b), Some(c)) = (a, b, c) else {
                return None;
            };
            for [first, second] in [
                [triangle[0], triangle[1]],
                [triangle[1], triangle[2]],
                [triangle[2], triangle[0]],
            ] {
                *edge_uses
                    .entry((first.min(second), first.max(second)))
                    .or_default() += 1;
            }
            coordinate_scale = coordinate_scale
                .max(a.x.abs())
                .max(a.y.abs())
                .max(a.z.abs())
                .max(b.x.abs())
                .max(b.y.abs())
                .max(b.z.abs())
                .max(c.x.abs())
                .max(c.y.abs())
                .max(c.z.abs());
            let ab = [b.x - a.x, b.y - a.y, b.z - a.z];
            let ac = [c.x - a.x, c.y - a.y, c.z - a.z];
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            let triangle_area = 0.5 * cross[0].hypot(cross[1]).hypot(cross[2]);
            area += triangle_area;
            for axis in 0..3 {
                area_centroid[axis] +=
                    triangle_area * [a.x + b.x + c.x, a.y + b.y + c.y, a.z + b.z + c.z][axis] / 3.0;
            }
            let tetra_volume = (a.x * (b.y * c.z - b.z * c.y)
                + a.y * (b.z * c.x - b.x * c.z)
                + a.z * (b.x * c.y - b.y * c.x))
                / 6.0;
            signed_volume += tetra_volume;
            for axis in 0..3 {
                volume_centroid[axis] +=
                    tetra_volume * [a.x + b.x + c.x, a.y + b.y + c.y, a.z + b.z + c.z][axis] / 4.0;
            }
            triangles += 1;
        }
        watertight &= !edge_uses.is_empty() && edge_uses.values().all(|uses| *uses == 2);
    }
    if triangles == 0 || area == 0.0 {
        return None;
    }
    let volume_epsilon =
        f64::EPSILON * coordinate_scale.max(1.0).powi(3) * (triangles as f64).max(1.0);
    let centroid = if watertight && signed_volume.abs() > volume_epsilon {
        Point3::new(
            volume_centroid[0] / signed_volume,
            volume_centroid[1] / signed_volume,
            volume_centroid[2] / signed_volume,
        )
    } else {
        Point3::new(
            area_centroid[0] / area,
            area_centroid[1] / area,
            area_centroid[2] / area,
        )
    };
    Some(MeshProperties {
        area,
        volume: signed_volume.abs(),
        centroid,
    })
}

fn step_id(id: &str) -> u64 {
    id.rsplit('#')
        .next()
        .and_then(|id| id.parse().ok())
        .unwrap_or(u64::MAX)
}

fn collect_validation_references(
    value: &Value,
    validation_points: &BTreeSet<u64>,
    referenced: &mut BTreeSet<u64>,
) {
    match value {
        Value::Reference(id) if validation_points.contains(id) => {
            referenced.insert(*id);
        }
        Value::List(values) => {
            for value in values {
                collect_validation_references(value, validation_points, referenced);
            }
        }
        Value::Typed(_, value) => {
            collect_validation_references(value, validation_points, referenced);
        }
        _ => {}
    }
}

trait RecordExt {
    fn simple_name(&self) -> Option<&str>;
    fn parameter(&self, index: usize) -> Option<&Value>;
}
impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
    }
    fn parameter(&self, index: usize) -> Option<&Value> {
        self.partials.first()?.parameters.get(index)
    }
}
trait ValueExt {
    fn reference(&self) -> Option<u64>;
    fn list(&self) -> Option<&[Value]>;
    fn number(&self) -> Option<f64>;
    fn text(&self) -> Option<String>;
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
            Value::Integer(value) => Some(*value as f64),
            Value::Real(value) => Some(*value),
            _ => None,
        }
    }
    fn text(&self) -> Option<String> {
        if let Value::String(bytes) = self {
            crate::strings::decode(bytes).ok()
        } else {
            None
        }
    }
}
