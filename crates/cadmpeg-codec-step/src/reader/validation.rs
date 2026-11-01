// SPDX-License-Identifier: Apache-2.0
//! Geometric validation-property decoding into declared measures.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::declared_measures::{DeclaredMeasure, DeclaredQuantity, DeclaredTarget};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::ids::{DeclaredMeasureId, IdentityKey, ProductDefinitionId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;

use crate::ids::{self, kind};
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text;
use super::geometry::GeometryData;
use super::topology::TopologyData;
use super::StageOutcome;
use super::{RecordExt, ValueExt};

/// The quantity that one validation item states, before unit scaling.
enum Stated {
    Volume(f64),
    SolidArea(f64),
    WettedArea(f64),
    SolidCentroid([f64; 3]),
    SheetArea(f64),
    SheetCentroid([f64; 3]),
}

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryData,
    topology: &TopologyData,
    definitions_by_shape: &BTreeMap<u64, ProductDefinitionId>,
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    if !exchange.has_entity("PROPERTY_DEFINITION")
        || !exchange.has_entity("PROPERTY_DEFINITION_REPRESENTATION")
    {
        return Ok(StageOutcome {
            value: (),
            claims: HashSet::new(),
            notes: Vec::new(),
            losses: Vec::new(),
        });
    }
    let mut losses = Vec::new();
    let mut representations = BTreeMap::new();
    for (&id, record) in exchange.records() {
        let Some(representation_items) = super::representation::items(record) else {
            continue;
        };
        let items: BTreeSet<u64> = representation_items.collect();
        if !items.is_empty() {
            representations.insert(id, items);
        }
    }
    let mut properties = BTreeMap::new();
    let mut property_definitions = BTreeMap::new();
    for (id, record) in exchange.entities("PROPERTY_DEFINITION") {
        let Some(property) = record.partial("PROPERTY_DEFINITION") else {
            continue;
        };
        let definition = property.parameters.get(2).and_then(ValueExt::reference);
        if let Some(definition) = definition {
            property_definitions.insert(id, definition);
        }
        let name = property.parameters.first().and_then(|value| {
            decode_text(
                exchange,
                value,
                &mut losses,
                id,
                "validation property name",
                StepLossCode::MetadataStringInvalid,
            )
        });
        let Some(name) = name else {
            continue;
        };
        if name.eq_ignore_ascii_case("geometric validation property") {
            let description = property.parameters.get(1).and_then(|value| {
                decode_text(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "validation property description",
                    StepLossCode::MetadataStringInvalid,
                )
            });
            properties.insert(id, (description, definition));
        }
    }
    let aspect_items = aspect_items(exchange, &property_definitions, &representations);
    let mut resolved_targets = BTreeMap::new();
    let mut measures = BTreeMap::new();
    let mut typed = HashSet::new();
    let mut validation_points = BTreeSet::new();
    let mut validation_representations = BTreeSet::new();

    for (relation_id, relation) in exchange.entities("PROPERTY_DEFINITION_REPRESENTATION") {
        let Some(relation) = relation.partial("PROPERTY_DEFINITION_REPRESENTATION") else {
            continue;
        };
        let Some(property_id) = relation.parameters.first().and_then(ValueExt::reference) else {
            continue;
        };
        let Some((description, definition)) = properties.get(&property_id) else {
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
        let target = definition.map_or(DeclaredTarget::Unresolved, |definition| {
            resolved_targets
                .entry(definition)
                .or_insert_with(|| {
                    resolve_target(
                        definition,
                        exchange,
                        topology,
                        definitions_by_shape,
                        &aspect_items,
                    )
                })
                .clone()
        });
        for &item_id in item_ids {
            let Some(item) = exchange.records().get(&item_id) else {
                continue;
            };
            if item.partial("CARTESIAN_POINT").is_some() {
                validation_points.insert(item_id);
            }
            let scale = geometry.units.length([item_id, representation_id]).get();
            let quantity = stated(item_id, item, exchange, &mut losses)
                .map(|stated| scaled(stated, item_id, item, exchange, scale, &mut losses, ctx))
                .transpose()?
                .flatten();
            let Some(quantity) = quantity else {
                losses.push(StepLossCode::ValidationItemUnsupported.note(format!(
                    "geometric validation property #{property_id} has unsupported item #{item_id}"
                )));
                continue;
            };
            for id in [property_id, relation_id, representation_id, item_id] {
                typed.insert(id);
            }
            if let Some(unit) = measure_unit(item) {
                collect_unit_records(unit, exchange, &mut typed);
            }
            let id = DeclaredMeasureId::from(ids::data(
                kind!("declared_measure"),
                IdentityKey::from(property_id).dash(item_id),
            ));
            measures.insert(
                id.clone(),
                DeclaredMeasure {
                    id,
                    target: target.clone(),
                    quantity,
                    label: description.clone(),
                    native_ref: Some(format!("#{item_id}")),
                },
            );
        }
    }
    ir.model.declared_measures.extend(measures.into_values());
    ir.model
        .declared_measures
        .sort_by(|left, right| left.id.cmp(&right.id));
    let mut referenced_validation_points = BTreeSet::new();
    if !validation_points.is_empty() {
        for (&record_id, record) in exchange.records() {
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
                    ctx,
                )?;
            }
        }
    }
    ir.model.points.retain(|point| {
        // A point whose identity names no entity is not a validation point.
        let Some(id) = step_id(point.id.as_str()) else {
            return true;
        };
        !validation_points.contains(&id) || referenced_validation_points.contains(&id)
    });
    Ok(StageOutcome {
        value: (),
        claims: typed,
        notes: Vec::new(),
        losses,
    })
}

/// The representation items that identify the geometry of each shape
/// aspect: the items of a `SHAPE_DEFINITION_REPRESENTATION` whose
/// definition is a property of the aspect, and the identified items of a
/// `GEOMETRIC_ITEM_SPECIFIC_USAGE` or `ITEM_IDENTIFIED_REPRESENTATION_USAGE`
/// whose definition is the aspect.
fn aspect_items(
    exchange: &Exchange,
    property_definitions: &BTreeMap<u64, u64>,
    representations: &BTreeMap<u64, BTreeSet<u64>>,
) -> BTreeMap<u64, BTreeSet<u64>> {
    let mut items = BTreeMap::<u64, BTreeSet<u64>>::new();
    for (_, record) in exchange.entities("SHAPE_DEFINITION_REPRESENTATION") {
        let Some(relation) = record.partial("SHAPE_DEFINITION_REPRESENTATION") else {
            continue;
        };
        let Some(aspect) = relation
            .parameters
            .first()
            .and_then(ValueExt::reference)
            .and_then(|property| property_definitions.get(&property))
        else {
            continue;
        };
        let Some(representation) = relation
            .parameters
            .get(1)
            .and_then(ValueExt::reference)
            .and_then(|representation| representations.get(&representation))
        else {
            continue;
        };
        items
            .entry(*aspect)
            .or_default()
            .extend(representation.iter().copied());
    }
    for (_, record) in exchange.records() {
        let Some(usage) = record.partials.iter().find(|partial| {
            partial.name == "GEOMETRIC_ITEM_SPECIFIC_USAGE"
                || partial.name == "ITEM_IDENTIFIED_REPRESENTATION_USAGE"
        }) else {
            continue;
        };
        let plain = record.partials.iter().all(|partial| {
            partial.name == "GEOMETRIC_ITEM_SPECIFIC_USAGE"
                || partial.name == "ITEM_IDENTIFIED_REPRESENTATION_USAGE"
        });
        if !plain {
            continue;
        }
        let Some(aspect) = usage.parameters.get(2).and_then(ValueExt::reference) else {
            continue;
        };
        let identified = match usage.parameters.get(4) {
            Some(Value::Reference(item)) => vec![*item],
            Some(Value::List(values)) => values.iter().filter_map(ValueExt::reference).collect(),
            _ => continue,
        };
        items.entry(aspect).or_default().extend(identified);
    }
    items
}

/// The bodies that the property definition `definition` names.
fn resolve_target(
    definition: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    definitions_by_shape: &BTreeMap<u64, ProductDefinitionId>,
    aspect_items: &BTreeMap<u64, BTreeSet<u64>>,
) -> DeclaredTarget {
    let Some(record) = exchange.records().get(&definition) else {
        return DeclaredTarget::Unresolved;
    };
    if record.partial("PRODUCT_DEFINITION_SHAPE").is_some() {
        return definitions_by_shape.get(&definition).map_or(
            DeclaredTarget::Unresolved,
            |definition| DeclaredTarget::Definition {
                definition: definition.clone(),
            },
        );
    }
    if record.partial("SHAPE_ASPECT").is_none() {
        return DeclaredTarget::Unresolved;
    }
    let Some(items) = aspect_items.get(&definition) else {
        return DeclaredTarget::Unresolved;
    };
    let mut bodies = BTreeSet::new();
    for item in items {
        let placement = exchange.records().get(item).is_some_and(|record| {
            record.partial("AXIS2_PLACEMENT_3D").is_some()
                || record.partial("AXIS2_PLACEMENT_2D").is_some()
        });
        if placement {
            continue;
        }
        let Some(item_bodies) = topology.body_by_root.get(item) else {
            return DeclaredTarget::Unresolved;
        };
        bodies.extend(item_bodies.iter().cloned());
    }
    if bodies.is_empty() {
        return DeclaredTarget::Unresolved;
    }
    DeclaredTarget::Bodies {
        bodies: bodies.into_iter().collect(),
    }
}

/// The quantity that the validation item `record` names, before unit
/// scaling, or `None` when its name and value name no carried quantity.
fn stated(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    losses: &mut Vec<LossNote>,
) -> Option<Stated> {
    let name = item_name(record)
        .and_then(|value| {
            decode_text(
                exchange,
                value,
                losses,
                id,
                "validation item name",
                StepLossCode::MetadataStringInvalid,
            )
        })?
        .to_ascii_lowercase();
    if let Some(point) = record.partial("CARTESIAN_POINT") {
        let values = point.parameters.get(1).and_then(ValueExt::list)?;
        let [x, y, z] = values else {
            return None;
        };
        let coordinates = [x.number()?, y.number()?, z.number()?];
        return match name.as_str() {
            "centre point" => Some(Stated::SolidCentroid(coordinates)),
            "surface centre point" => Some(Stated::SheetCentroid(coordinates)),
            _ => None,
        };
    }
    record.partial("MEASURE_REPRESENTATION_ITEM")?;
    let (kind, value) = record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
        .find_map(area_or_volume_measure)?;
    match (kind, name.as_str()) {
        ("VOLUME_MEASURE", "volume measure") => Some(Stated::Volume(value)),
        ("AREA_MEASURE", "surface area measure") => Some(Stated::SolidArea(value)),
        ("AREA_MEASURE", "wetted area measure") => Some(Stated::WettedArea(value)),
        ("AREA_MEASURE", "independent surface area measure") => Some(Stated::SheetArea(value)),
        _ => None,
    }
}

/// The `name` of a representation item, simple or complex.
fn item_name(record: &RawRecord) -> Option<&Value> {
    [
        "REPRESENTATION_ITEM",
        "MEASURE_REPRESENTATION_ITEM",
        "CARTESIAN_POINT",
    ]
    .into_iter()
    .find_map(|name| record.partial(name))
    .and_then(|partial| partial.parameters.first())
}

/// The quantity in millimetre units: lengths scale by the context length
/// unit, areas and volumes by their measure unit.
fn scaled(
    stated: Stated,
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    length_scale: f64,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<DeclaredQuantity>, CodecError> {
    let point = |[x, y, z]: [f64; 3]| {
        FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            x * length_scale,
            y * length_scale,
            z * length_scale,
        ))
    };
    let mut measure = |value: f64, kind: &str| {
        measure_scale(id, record, exchange, kind, losses, ctx)
            .map(|scale| scale.and_then(|scale| FiniteReal::new(value * scale)))
    };
    Ok(match stated {
        Stated::Volume(value) => {
            measure(value, "VOLUME_MEASURE")?.map(|value| DeclaredQuantity::Volume { value })
        }
        Stated::SolidArea(value) => {
            measure(value, "AREA_MEASURE")?.map(|value| DeclaredQuantity::SolidArea { value })
        }
        Stated::WettedArea(value) => {
            measure(value, "AREA_MEASURE")?.map(|value| DeclaredQuantity::WettedArea { value })
        }
        Stated::SheetArea(value) => {
            measure(value, "AREA_MEASURE")?.map(|value| DeclaredQuantity::SheetArea { value })
        }
        Stated::SolidCentroid(value) => {
            point(value).map(|value| DeclaredQuantity::SolidCentroid { value })
        }
        Stated::SheetCentroid(value) => {
            point(value).map(|value| DeclaredQuantity::SheetCentroid { value })
        }
    })
}

fn measure_scale(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    kind: &str,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<f64>, CodecError> {
    let resolved = measure_unit(record)
        .and_then(|unit| exchange.records().get(&unit))
        .and_then(derived_unit_elements)
        .and_then(ValueExt::list);
    let resolved = if let Some(elements) = resolved {
        let mut scale = Some(1.0);
        let mut dimension = 0.0;
        for element in elements {
            let fields = (|| {
                let element = exchange.records().get(&element.reference()?)?;
                let element = element.partial("DERIVED_UNIT_ELEMENT")?;
                Some((
                    element.parameters.first()?.reference()?,
                    element.parameters.get(1)?.number()?,
                ))
            })();
            let Some((base, exponent)) = fields else {
                scale = None;
                break;
            };
            let Some(base) =
                super::geometry::unit_scale_mm(base, exchange, &mut BTreeSet::new(), ctx)?
            else {
                scale = None;
                break;
            };
            dimension += exponent;
            scale = scale.map(|scale| scale * base.get().powf(exponent));
        }
        scale.filter(|scale| {
            dimension == if kind == "AREA_MEASURE" { 2.0 } else { 3.0 }
                && scale.is_finite()
                && *scale > 0.0
        })
    } else {
        None
    };
    match resolved {
        Some(scale) => Ok(Some(scale)),
        None => {
            losses.push(StepLossCode::ValidationMeasureUnitUnresolved.note(format!(
                "geometric validation {kind} measure #{id} unit scale did not resolve; the quantity is not compared",
            )));
            Ok(None)
        }
    }
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
    let Some(record) = exchange.records().get(&id) else {
        return;
    };
    let Some(elements) = derived_unit_elements(record).and_then(ValueExt::list) else {
        return;
    };
    for element in elements.iter().filter_map(ValueExt::reference) {
        typed.insert(element);
        if let Some(base) = exchange
            .records()
            .get(&element)
            .and_then(|record| record.partial("DERIVED_UNIT_ELEMENT"))
            .and_then(|record| record.parameters.first())
            .and_then(ValueExt::reference)
        {
            typed.insert(base);
        }
    }
}

/// The numeric entity identifier an IR identity ends with, or `None` when it
/// names none.
fn step_id(id: &str) -> Option<u64> {
    id.rsplit('#').next().and_then(|id| id.parse().ok())
}

fn collect_validation_references(
    value: &Value,
    validation_points: &BTreeSet<u64>,
    referenced: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("step_validation_reference_walk")?;
    match value {
        Value::Reference(id) if validation_points.contains(id) => {
            referenced.insert(*id);
        }
        Value::List(values) => {
            for value in values {
                collect_validation_references(value, validation_points, referenced, ctx)?;
            }
        }
        Value::Typed(_, value) => {
            collect_validation_references(value, validation_points, referenced, ctx)?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests;
