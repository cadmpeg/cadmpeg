// SPDX-License-Identifier: Apache-2.0
//! Tolerance-zone constraints and their owning geometric tolerances.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::pmi::zones::{ToleranceConstraints, ToleranceProjection, ToleranceZone};
use cadmpeg_ir::pmi::{PmiDefinition, PmiQuantity, PmiTarget};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Length, PositiveLength};
use cadmpeg_ir::CadIr;

use super::super::geometry::GeometryData;
use super::super::{named_parameter, RecordExt, ValueExt};
use super::{
    decode_text, measure, measure_context, references, targets, Annotations, MeasureContext,
};
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

fn introduced<'a>(
    record: &'a RawRecord,
    entity: &str,
    inherited: usize,
    index: usize,
) -> Option<&'a Value> {
    if record.simple_name() == Some(entity) {
        record.parameter(inherited + index)
    } else {
        named_parameter(record, entity, index)
    }
}

fn length(
    value: &Value,
    exchange: &Exchange,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Length>, CodecError> {
    Ok(measure(value, exchange, measurements, ctx)?
        .filter(|value| value.quantity == PmiQuantity::Length)
        .and_then(|value| Length::new(value.value.get())))
}

pub(super) fn constraints(
    record: &RawRecord,
    exchange: &Exchange,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<ToleranceConstraints, CodecError> {
    let displacement = introduced(record, "UNEQUALLY_DISPOSED_GEOMETRIC_TOLERANCE", 4, 0)
        .map(|value| length(value, exchange, measurements, ctx))
        .transpose()?
        .flatten();
    let maximum = introduced(record, "GEOMETRIC_TOLERANCE_WITH_MAXIMUM_TOLERANCE", 5, 0)
        .map(|value| length(value, exchange, measurements, ctx))
        .transpose()?
        .flatten()
        .and_then(|value| PositiveLength::new(value.get()));
    for (entity, complete) in [
        (
            "UNEQUALLY_DISPOSED_GEOMETRIC_TOLERANCE",
            displacement.is_some(),
        ),
        (
            "GEOMETRIC_TOLERANCE_WITH_MAXIMUM_TOLERANCE",
            maximum.is_some(),
        ),
    ] {
        if record.partials.iter().any(|partial| partial.name == entity) && !complete {
            measurements.losses.push(
                StepLossCode::PmiZoneUnresolved
                    .note(format!("{entity} has no valid length constraint")),
            );
        }
    }
    Ok(ToleranceConstraints {
        displacement,
        maximum,
        ..ToleranceConstraints::default()
    })
}

pub(super) fn attach(
    exchange: &Exchange,
    geometry: &GeometryData,
    ir: &mut CadIr,
    annotations: &Annotations,
    typed: &mut HashSet<u64>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut projections = BTreeMap::<u64, Vec<(u64, ToleranceProjection)>>::new();
    for id in exchange.matching_entity_ids(|name| {
        matches!(
            name,
            "PROJECTED_ZONE_DEFINITION" | "PROJECTED_ZONE_DEFINITION_WITH_OFFSET"
        )
    }) {
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        if let Some((zone, projection)) = projection(id, record, exchange, geometry, losses, ctx)? {
            projections.entry(zone).or_default().push((id, projection));
        } else {
            losses.push(StepLossCode::PmiZoneUnresolved.note(format!(
                "projected tolerance zone #{id} has no complete end, boundary, length, and offset"
            )));
        }
    }
    for (id, record) in exchange.entities("TOLERANCE_ZONE") {
        let form = introduced(record, "TOLERANCE_ZONE", 4, 1)
            .and_then(ValueExt::reference)
            .and_then(|id| exchange.records().get(&id).map(|record| (id, record)))
            .and_then(|(id, record)| {
                named_parameter(record, "TOLERANCE_ZONE_FORM", 0).map(|value| (id, value))
            })
            .and_then(|(id, value)| {
                decode_text(
                    exchange,
                    value,
                    losses,
                    id,
                    "tolerance zone form",
                    StepLossCode::MetadataStringInvalid,
                )
                .map(|form| (id, form))
            })
            .and_then(|(id, form)| NonBlankString::new(form).map(|form| (id, form)));
        let Some((form_id, form)) = form else {
            losses.push(
                StepLossCode::PmiZoneUnresolved
                    .note(format!("tolerance zone #{id} has no zone form")),
            );
            continue;
        };
        let Some(owners) = introduced(record, "TOLERANCE_ZONE", 4, 0) else {
            continue;
        };
        let owners = references(owners, ctx).collect::<Result<Vec<_>, _>>()?;
        if owners.len() != 1 {
            losses.push(StepLossCode::PmiZoneUnresolved.note(format!(
                "tolerance zone #{id} has {} defining tolerances; shared zone ownership is not transferred",
                owners.len()
            )));
            continue;
        }
        let zone_projections = projections.get(&id).map(Vec::as_slice).unwrap_or_default();
        let mut attached = false;
        for owner in owners {
            let Some(index) = annotations.get(owner) else {
                continue;
            };
            let PmiDefinition::GeometricTolerance { constraints, .. } =
                &mut ir.model.pmi[index.get()].definition
            else {
                continue;
            };
            ctx.charge_entities(1, "STEP tolerance zone")?;
            ctx.charge_entities(
                cadmpeg_core::decode::u64_from_index(zone_projections.len()),
                "STEP tolerance-zone projections",
            )?;
            constraints.zones.push(ToleranceZone {
                form: form.clone(),
                projections: zone_projections
                    .iter()
                    .map(|(_, projection)| projection.clone())
                    .collect(),
            });
            attached = true;
        }
        if attached {
            typed.extend([id, form_id]);
            typed.extend(zone_projections.iter().map(|(id, _)| *id));
        } else {
            losses.push(StepLossCode::PmiZoneUnresolved.note(format!(
                "tolerance zone #{id} has no transferred geometric tolerance"
            )));
        }
    }
    Ok(())
}

fn projection(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    geometry: &GeometryData,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(u64, ToleranceProjection)>, CodecError> {
    let simple = record.simple_name().is_some();
    let parameter = |index| {
        if simple {
            record.parameter(index)
        } else if index < 2 {
            named_parameter(record, "TOLERANCE_ZONE_DEFINITION", index)
        } else {
            named_parameter(record, "PROJECTED_ZONE_DEFINITION", index - 2)
        }
    };
    let Some(zone) = parameter(0).and_then(ValueExt::reference) else {
        return Ok(None);
    };
    let Some(boundaries) = parameter(1) else {
        return Ok(None);
    };
    if !matches!(boundaries, Value::List(_)) {
        return Ok(None);
    }
    let boundary_ids = references(boundaries, ctx).collect::<Result<Vec<_>, _>>()?;
    if boundary_ids.iter().any(|id| {
        !exchange.records().get(id).is_some_and(|record| {
            record
                .partials
                .iter()
                .any(|partial| super::is_shape_aspect_name(&partial.name))
        })
    }) {
        return Ok(None);
    }
    let boundaries = targets(boundary_ids.into_iter().map(Ok))?;
    let Some(end) = parameter(2).and_then(ValueExt::reference) else {
        return Ok(None);
    };
    if !exchange.records().get(&end).is_some_and(|record| {
        record
            .partials
            .iter()
            .any(|partial| super::is_shape_aspect_name(&partial.name))
    }) {
        return Ok(None);
    }
    let Some(value) = parameter(3) else {
        return Ok(None);
    };
    let mut measurements = measure_context(geometry, id, losses);
    let Some(length) = length(value, exchange, &mut measurements, ctx)?
        .and_then(|value| PositiveLength::new(value.get()))
    else {
        return Ok(None);
    };
    let offset = introduced(record, "PROJECTED_ZONE_DEFINITION_WITH_OFFSET", 4, 0)
        .map(|value| self::length(value, exchange, &mut measurements, ctx))
        .transpose()?;
    if matches!(offset, Some(None)) {
        return Ok(None);
    }
    Ok(Some((
        zone,
        ToleranceProjection {
            boundaries,
            end: PmiTarget::ShapeAspect {
                source_id: super::super::step_source_id(end),
            },
            length,
            offset: offset.flatten(),
        },
    )))
}
