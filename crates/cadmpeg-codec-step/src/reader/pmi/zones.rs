// SPDX-License-Identifier: Apache-2.0
//! Tolerance-zone constraints and their owning geometric tolerances.

use std::collections::{BTreeMap, HashSet};

use crate::reader::instance_is_lost;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::pmi::zones::{ToleranceProjection, ToleranceZone, ToleranceZoneForm};
use cadmpeg_ir::pmi::{PmiDefinition, PmiQuantity, PmiTarget};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Length, PositiveLength};
use cadmpeg_ir::unknown::part::Cause;
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

/// The zone offset and the maximum tolerance that a geometric tolerance's
/// complex instance introduces.
pub(super) struct ZoneLimits {
    pub(super) displacement: Option<Length>,
    pub(super) maximum_upper: Option<PositiveLength>,
}

pub(super) fn limits(
    record: &RawRecord,
    exchange: &Exchange,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<ZoneLimits, CodecError> {
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
    Ok(ZoneLimits {
        displacement,
        maximum_upper: maximum,
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
            exchange.lost().give_up_for(
                id,
                Cause::Unreadable,
                &StepLossCode::PmiZoneUnresolved.note(format!(
                    "projected tolerance zone #{id} has no complete end, boundary, length, and offset"
                )),
            );
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
            .and_then(|(id, form)| NonBlankString::new(form).map(|form| (id, zone_form(&form))));
        let zone_projections = projections.get(&id).map(Vec::as_slice).unwrap_or_default();
        let Some((form_id, form)) = form else {
            give_up_zone(
                exchange,
                id,
                zone_projections,
                Cause::Unreadable,
                &StepLossCode::PmiZoneUnresolved
                    .note(format!("tolerance zone #{id} has no zone form")),
            );
            continue;
        };
        let Some(owners) = introduced(record, "TOLERANCE_ZONE", 4, 0) else {
            continue;
        };
        let owners = references(owners, ctx).collect::<Result<Vec<_>, _>>()?;
        if owners.len() != 1 {
            let cause = if owners.is_empty() {
                Cause::Unreadable
            } else {
                Cause::Undecoded
            };
            give_up_zone(
                exchange,
                id,
                zone_projections,
                cause,
                &StepLossCode::PmiZoneUnresolved.note(format!(
                    "tolerance zone #{id} has {} defining tolerances; shared zone ownership is not transferred",
                    owners.len()
                )),
            );
            continue;
        }
        let mut attached = false;
        for &owner in &owners {
            let Some(index) = annotations.get(owner) else {
                continue;
            };
            let PmiDefinition::GeometricTolerance { zones, .. } =
                &mut ir.model.pmi[index.get()].definition
            else {
                continue;
            };
            ctx.charge_entities(1, "STEP tolerance zone")?;
            ctx.charge_entities(
                cadmpeg_core::decode::u64_from_index(zone_projections.len()),
                "STEP tolerance-zone projections",
            )?;
            zones.push(ToleranceZone {
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
        } else if owners
            .iter()
            .any(|&owner| instance_is_lost(exchange, owner))
        {
            // The zone is a value of the given-up tolerance, which its charge
            // names.
            losses.push(StepLossCode::PmiZoneUnresolved.note(format!(
                "tolerance zone #{id} belongs to a given-up geometric tolerance"
            )));
        } else {
            give_up_zone(
                exchange,
                id,
                zone_projections,
                Cause::Undecoded,
                &StepLossCode::PmiZoneUnresolved.note(format!(
                    "tolerance zone #{id} names no transferred geometric tolerance"
                )),
            );
        }
    }
    Ok(())
}

/// Gives up tolerance zone `id` for `cause` as `why` states, and drops the
/// projections that define it with the zone.
fn give_up_zone(
    exchange: &Exchange,
    id: u64,
    projections: &[(u64, ToleranceProjection)],
    cause: Cause,
    why: &LossNote,
) {
    for (projection, _) in projections {
        exchange.lost().drop_with(*projection, id);
    }
    exchange.lost().give_up_for(id, cause, why);
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
            boundaries: boundaries
                .into_iter()
                .map(cadmpeg_ir::references::InstanceTarget::new)
                .collect(),
            end: PmiTarget::ShapeAspect {
                source_id: super::super::step_source_id(end),
            }
            .into(),
            length,
            offset: offset.flatten(),
        },
    )))
}

/// A `TOLERANCE_ZONE_FORM` name: ISO 10303's `cylindrical or circular` and
/// `spherical` are typed, and another name stays the source's.
fn zone_form(name: &NonBlankString) -> ToleranceZoneForm {
    match name.as_str().trim().to_ascii_lowercase().as_str() {
        "cylindrical or circular" => ToleranceZoneForm::CylindricalOrCircular,
        "spherical" => ToleranceZoneForm::Spherical,
        _ => ToleranceZoneForm::Other(name.as_str().to_owned()),
    }
}
