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
use super::super::string_text;
use super::super::{named_parameter, RecordExt, ValueExt};
use super::{measure, measure_context, references, targets, Annotations, MeasureContext};
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
        match projection(id, record, exchange, geometry, losses, ctx)? {
            Ok((zone, projection)) => projections.entry(zone).or_default().push((id, projection)),
            Err(Untransferred::Needs(needs)) => exchange.lost().drop_with(id, needs),
            Err(Untransferred::GiveUp(cause, why)) => exchange.lost().give_up_for(
                id,
                cause,
                &StepLossCode::PmiZoneUnresolved
                    .note(format!("projected tolerance zone #{id} {why}")),
            ),
        }
    }
    for (id, record) in exchange.entities("TOLERANCE_ZONE") {
        let zone_projections = projections.get(&id).map(Vec::as_slice).unwrap_or_default();
        let (form_id, form) = match zone_form_of(exchange, record) {
            Ok(form) => form,
            Err(untransferred) => {
                settle_zone(exchange, id, zone_projections, untransferred);
                continue;
            }
        };
        let Some(owners) = introduced(record, "TOLERANCE_ZONE", 4, 0) else {
            settle_zone(
                exchange,
                id,
                zone_projections,
                Untransferred::GiveUp(Cause::Unreadable, "names no defining tolerance".into()),
            );
            continue;
        };
        let owners = references(owners, ctx).collect::<Result<Vec<_>, _>>()?;
        if owners.len() != 1 {
            let (cause, why) = if owners.is_empty() {
                (Cause::Unreadable, "names no defining tolerance".into())
            } else {
                (
                    Cause::Undecoded,
                    format!(
                        "has {} defining tolerances; shared zone ownership is not transferred",
                        owners.len()
                    ),
                )
            };
            settle_zone(
                exchange,
                id,
                zone_projections,
                Untransferred::GiveUp(cause, why),
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
        } else if let Some(&owner) = owners
            .iter()
            .find(|&&owner| instance_is_lost(exchange, owner))
        {
            settle_zone(exchange, id, zone_projections, Untransferred::Needs(owner));
        } else {
            let cause = if owners
                .iter()
                .all(|owner| exchange.records().get(owner).is_some_and(is_zone_target))
            {
                Cause::Undecoded
            } else {
                Cause::Unreadable
            };
            settle_zone(
                exchange,
                id,
                zone_projections,
                Untransferred::GiveUp(cause, "names no transferred geometric tolerance".into()),
            );
        }
    }
    Ok(())
}

/// Why a zone record transfers no value: a lost instance that it needs, or
/// the cause and reason of its own give-up.
enum Untransferred {
    Needs(u64),
    GiveUp(Cause, String),
}

/// `Needs(reference)` where the parser or the reader gave up or dropped
/// `reference`.
fn needs_lost(exchange: &Exchange, reference: u64) -> Result<(), Untransferred> {
    if instance_is_lost(exchange, reference) {
        Err(Untransferred::Needs(reference))
    } else {
        Ok(())
    }
}

/// The `TOLERANCE_ZONE_FORM` of tolerance zone `record` and the form it
/// names.
fn zone_form_of(
    exchange: &Exchange,
    record: &RawRecord,
) -> Result<(u64, ToleranceZoneForm), Untransferred> {
    let unreadable = |why: String| Untransferred::GiveUp(Cause::Unreadable, why);
    let form_id = introduced(record, "TOLERANCE_ZONE", 4, 1)
        .and_then(ValueExt::reference)
        .ok_or_else(|| unreadable("has no zone form reference".into()))?;
    needs_lost(exchange, form_id)?;
    let name = exchange
        .records()
        .get(&form_id)
        .and_then(|form| named_parameter(form, "TOLERANCE_ZONE_FORM", 0))
        .ok_or_else(|| {
            unreadable(format!(
                "names #{form_id}, which is not a TOLERANCE_ZONE_FORM"
            ))
        })?;
    let name = string_text(exchange, name, form_id, "tolerance zone form")
        .map_err(|error| unreadable(format!("names a zone form that does not decode: {error}")))?
        .ok_or_else(|| {
            unreadable(format!(
                "names TOLERANCE_ZONE_FORM #{form_id}, whose name is not a string"
            ))
        })?;
    let name = NonBlankString::new(name).ok_or_else(|| {
        Untransferred::GiveUp(
            Cause::Undecoded,
            format!("names TOLERANCE_ZONE_FORM #{form_id}, whose name is blank"),
        )
    })?;
    Ok((form_id, zone_form(&name)))
}

/// Whether `record` is of a type that `TOLERANCE_ZONE.defining_tolerance`
/// admits (ISO 10303-47 `tolerance_zone_target`): a geometric tolerance, a
/// dimensional location or size, or a general datum reference.
fn is_zone_target(record: &RawRecord) -> bool {
    record.partials.iter().any(|partial| {
        let name = partial.name.as_str();
        name.starts_with("GEOMETRIC_TOLERANCE")
            || (name.ends_with("_TOLERANCE") && name != "PLUS_MINUS_TOLERANCE")
            || super::is_dimension_name(name)
            || matches!(
                name,
                "GENERAL_DATUM_REFERENCE"
                    | "DATUM_REFERENCE_COMPARTMENT"
                    | "DATUM_REFERENCE_ELEMENT"
            )
    })
}

/// Settles tolerance zone `id`, which transfers no value: the projections
/// that define it drop with it, and it drops with the instance it needs or
/// gives up as its own.
fn settle_zone(
    exchange: &Exchange,
    id: u64,
    projections: &[(u64, ToleranceProjection)],
    untransferred: Untransferred,
) {
    for (projection, _) in projections {
        exchange.lost().drop_with(*projection, id);
    }
    match untransferred {
        Untransferred::Needs(needs) => exchange.lost().drop_with(id, needs),
        Untransferred::GiveUp(cause, why) => exchange.lost().give_up_for(
            id,
            cause,
            &StepLossCode::PmiZoneUnresolved.note(format!("tolerance zone #{id} {why}")),
        ),
    }
}

/// Checks that `aspect`, which a projected zone names as a boundary or as its
/// projection end, is a shape aspect.
fn projection_aspect(exchange: &Exchange, aspect: u64) -> Result<(), Untransferred> {
    needs_lost(exchange, aspect)?;
    if exchange.records().get(&aspect).is_some_and(|record| {
        record
            .partials
            .iter()
            .any(|partial| super::is_shape_aspect_name(&partial.name))
    }) {
        Ok(())
    } else {
        Err(Untransferred::GiveUp(
            Cause::Unreadable,
            format!("names #{aspect}, which is not a shape aspect"),
        ))
    }
}

fn projection(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    geometry: &GeometryData,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Result<(u64, ToleranceProjection), Untransferred>, CodecError> {
    let unreadable = |why: &str| Ok(Err(Untransferred::GiveUp(Cause::Unreadable, why.into())));
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
        return unreadable("has no zone reference");
    };
    if let Err(untransferred) = needs_lost(exchange, zone) {
        return Ok(Err(untransferred));
    }
    let Some(boundaries @ Value::List(_)) = parameter(1) else {
        return unreadable("has no boundary set");
    };
    let boundary_ids = references(boundaries, ctx).collect::<Result<Vec<_>, _>>()?;
    let Some(end) = parameter(2).and_then(ValueExt::reference) else {
        return unreadable("has no projection end");
    };
    for &aspect in boundary_ids.iter().chain([&end]) {
        if let Err(untransferred) = projection_aspect(exchange, aspect) {
            return Ok(Err(untransferred));
        }
    }
    let boundaries = targets(boundary_ids.into_iter().map(Ok))?;
    let Some(value) = parameter(3) else {
        return unreadable("has no projected length");
    };
    let offset_value = introduced(record, "PROJECTED_ZONE_DEFINITION_WITH_OFFSET", 4, 0);
    for reference in [Some(value), offset_value]
        .into_iter()
        .flatten()
        .filter_map(ValueExt::reference)
    {
        if let Err(untransferred) = needs_lost(exchange, reference) {
            return Ok(Err(untransferred));
        }
    }
    let mut measurements = measure_context(geometry, id, losses);
    let Some(length) = length(value, exchange, &mut measurements, ctx)? else {
        return Ok(Err(Untransferred::GiveUp(
            Cause::Undecoded,
            "has a projected length that the reader does not read as a length".into(),
        )));
    };
    let Some(length) = PositiveLength::new(length.get()) else {
        return unreadable("has a projected length that is not positive");
    };
    let offset = offset_value
        .map(|value| self::length(value, exchange, &mut measurements, ctx))
        .transpose()?;
    if matches!(offset, Some(None)) {
        return Ok(Err(Untransferred::GiveUp(
            Cause::Undecoded,
            "has an offset that the reader does not read as a length".into(),
        )));
    }
    Ok(Ok((
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
