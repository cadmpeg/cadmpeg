// SPDX-License-Identifier: Apache-2.0
//! Geometric validation-property decoding and mesh self-checks.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::LossNote;

use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text;
use super::geometry::GeometryData;
use super::StageOutcome;

#[derive(Clone, Copy)]
enum Expected {
    Area(f64),
    Volume(f64),
    Centroid(Point3),
}

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryData,
    ir: &mut CadIr,
) -> StageOutcome<()> {
    if !exchange.has_entity("PROPERTY_DEFINITION")
        || !exchange.has_entity("PROPERTY_DEFINITION_REPRESENTATION")
    {
        return StageOutcome {
            value: (),
            claims: HashSet::new(),
            notes: Vec::new(),
            warnings: Vec::new(),
            losses: Vec::new(),
        };
    }
    let mut losses = Vec::new();
    let representations = exchange
        .records
        .iter()
        .filter_map(|(&id, record)| {
            let items = super::representation::items(record)?
                .into_iter()
                .collect::<BTreeSet<_>>();
            (!items.is_empty()).then_some((id, items))
        })
        .collect::<BTreeMap<_, _>>();
    let properties = exchange
        .entities("PROPERTY_DEFINITION")
        .filter_map(|(id, record)| {
            let property = record.partial("PROPERTY_DEFINITION")?;
            let name = property.parameters.first().and_then(|value| {
                decode_text(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "validation property name",
                    StepLossCode::MetadataStringInvalid,
                )
            })?;
            if name.eq_ignore_ascii_case("geometric validation property") {
                Some((
                    id,
                    property
                        .parameters
                        .get(1)
                        .and_then(|value| {
                            decode_text(
                                exchange,
                                value,
                                &mut losses,
                                id,
                                "validation property description",
                                StepLossCode::MetadataStringInvalid,
                            )
                        })
                        .unwrap_or_default(),
                ))
            } else {
                None
            }
        })
        .collect::<BTreeMap<_, _>>();
    let computed = mesh_properties(ir);
    let mut typed = HashSet::new();
    let mut validation_points = BTreeSet::new();
    let mut validation_representations = BTreeSet::new();
    let mut notes = Vec::new();
    let mut warnings = Vec::new();

    for (relation_id, relation) in exchange.entities("PROPERTY_DEFINITION_REPRESENTATION") {
        let Some(relation) = relation.partial("PROPERTY_DEFINITION_REPRESENTATION") else {
            continue;
        };
        let Some(property_id) = relation.parameters.first().and_then(ValueExt::reference) else {
            continue;
        };
        let Some(description) = properties.get(&property_id) else {
            continue;
        };
        let Some(representation_id) = relation.parameters.get(1).and_then(ValueExt::reference)
        else {
            continue;
        };
        let Some(item_ids) = representations.get(&representation_id) else {
            continue;
        };
        validation_representations.insert(representation_id);
        for &item_id in item_ids {
            let Some(item) = exchange.records.get(&item_id) else {
                continue;
            };
            let scale = geometry.units.length([item_id, representation_id]);
            let expected = expected_value(item_id, item, exchange, scale, &mut losses);
            let Some(expected) = expected else {
                warnings.push(format!(
                    "geometric validation property #{property_id} has unsupported item #{item_id}"
                ));
                continue;
            };
            if matches!(expected, Expected::Centroid(_)) {
                validation_points.insert(item_id);
            }
            typed.extend([property_id, relation_id, representation_id, item_id]);
            if let Some(unit) = measure_unit(item) {
                collect_unit_records(unit, exchange, &mut typed);
            }
            let (kind, expected_text, actual) = match expected {
                Expected::Area(value) => {
                    ("surface area", value.to_string(), computed.map(|p| p.area))
                }
                Expected::Volume(value) => {
                    ("volume", value.to_string(), computed.map(|p| p.volume))
                }
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
    }
    let mut referenced_validation_points = BTreeSet::new();
    if !validation_points.is_empty() {
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
    }
    ir.model.points.retain(|point| {
        let id = step_id(&point.id.as_str());
        !validation_points.contains(&id) || referenced_validation_points.contains(&id)
    });
    StageOutcome {
        value: (),
        claims: typed,
        notes,
        warnings,
        losses,
    }
}

fn expected_value(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    scale: f64,
    losses: &mut Vec<LossNote>,
) -> Option<Expected> {
    if let Some(point) = record.partial("CARTESIAN_POINT") {
        let values = point.parameters.get(1)?.list()?;
        if values.len() != 3 {
            return None;
        }
        return Some(Expected::Centroid(Point3::new(
            values[0].number()? * scale,
            values[1].number()? * scale,
            values[2].number()? * scale,
        )));
    }
    record.partial("MEASURE_REPRESENTATION_ITEM")?;
    let (kind, value) = record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
        .find_map(area_or_volume_measure)?;
    let scale = measure_scale(id, record, exchange, scale, kind, losses);
    Some(match kind {
        "AREA_MEASURE" => Expected::Area(value * scale),
        "VOLUME_MEASURE" => Expected::Volume(value * scale),
        _ => return None,
    })
}

fn measure_scale(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    fallback: f64,
    kind: &str,
    losses: &mut Vec<LossNote>,
) -> f64 {
    measure_unit(record)
        .and_then(|unit| exchange.records.get(&unit))
        .and_then(derived_unit_elements)
        .and_then(ValueExt::list)
        .and_then(|elements| {
            elements.iter().try_fold(1.0, |scale, element| {
                let element = exchange.records.get(&element.reference()?)?;
                let element = element.partial("DERIVED_UNIT_ELEMENT")?;
                let base = element.parameters.first()?.reference()?;
                let exponent = element.parameters.get(1)?.number()?;
                let base =
                    super::geometry::unit_scale_mm(base, exchange, &mut BTreeSet::new())?;
                Some(scale * base.powf(exponent))
            })
        })
        .unwrap_or_else(|| {
            losses.push(StepLossCode::ValidationMeasureUnitUnresolved.note(format!(
                    "geometric validation {kind} measure #{} unit scale did not resolve; the document length scale was used",
                    id,
                )));
            fallback.powi(if kind == "AREA_MEASURE" { 2 } else { 3 })
        })
}

fn area_or_volume_measure(value: &Value) -> Option<(&str, f64)> {
    match value {
        Value::Typed(kind, value) if matches!(kind.as_str(), "AREA_MEASURE" | "VOLUME_MEASURE") => {
            Some((kind.as_str(), value.number()?))
        }
        Value::Typed(_, value) => area_or_volume_measure(value),
        Value::List(values) => values.iter().find_map(area_or_volume_measure),
        _ => None,
    }
}

fn measure_unit(record: &RawRecord) -> Option<u64> {
    record
        .partial("MEASURE_WITH_UNIT")
        .and_then(|partial| {
            partial
                .parameters
                .iter()
                .rev()
                .find_map(ValueExt::reference)
        })
        .or_else(|| {
            record
                .partial("MEASURE_REPRESENTATION_ITEM")
                .and_then(|partial| partial.parameters.get(2))
                .and_then(ValueExt::reference)
        })
}

fn derived_unit_elements(record: &RawRecord) -> Option<&Value> {
    record
        .partial("DERIVED_UNIT")
        .or_else(|| record.partial("AREA_UNIT"))
        .or_else(|| record.partial("VOLUME_UNIT"))
        .and_then(|partial| partial.parameters.first())
}

fn collect_unit_records(id: u64, exchange: &Exchange, typed: &mut HashSet<u64>) {
    typed.insert(id);
    let Some(record) = exchange.records.get(&id) else {
        return;
    };
    let Some(elements) = derived_unit_elements(record).and_then(ValueExt::list) else {
        return;
    };
    for element in elements.iter().filter_map(ValueExt::reference) {
        typed.insert(element);
        if let Some(base) = exchange
            .records
            .get(&element)
            .and_then(|record| record.partial("DERIVED_UNIT_ELEMENT"))
            .and_then(|record| record.parameters.first())
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
        for triangle in mesh.triangles() {
            let [a, b, c] = triangle.map(|index| mesh.vertices().get(index as usize).copied());
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
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord>;
}
impl RecordExt for RawRecord {
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord> {
        self.partials.iter().find(|partial| partial.name == name)
    }
}
trait ValueExt {
    fn reference(&self) -> Option<u64>;
    fn list(&self) -> Option<&[Value]>;
    fn number(&self) -> Option<f64>;
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
}

#[cfg(test)]
mod tests;
