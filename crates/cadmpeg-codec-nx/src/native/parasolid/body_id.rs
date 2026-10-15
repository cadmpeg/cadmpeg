// SPDX-License-Identifier: Apache-2.0
//! A named `body_id` extension with one actual scalar integer value record.

use super::{
    ParasolidAttributeDefinition, ParasolidEntity51Record, ParasolidEntity52IntegerRecord,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(in crate::native) struct BodyIdMatch<'a> {
    pub(in crate::native) stream: u32,
    pub(in crate::native) body_xmt: u32,
    pub(in crate::native) definition: &'a str,
    pub(in crate::native) entity: &'a str,
    pub(in crate::native) value_record: &'a str,
}

/// The operation family fixes the declared field and identity convention.
#[derive(Clone, Copy)]
pub(in crate::native) enum BodyIdEncoding {
    CurveMeshIgnored,
    BrepInteger,
}

/// Duplicate definitions, instances, values, or matching streams are unresolved.
pub(in crate::native) fn resolve<'a>(
    ctx: &DecodeContext<'_>,
    identity: u8,
    encoding: BodyIdEncoding,
    definitions: &'a [ParasolidAttributeDefinition],
    entities: &'a [ParasolidEntity51Record],
    integers: &'a [ParasolidEntity52IntegerRecord],
) -> Result<Option<BodyIdMatch<'a>>, CodecError> {
    let (field, value) = match encoding {
        BodyIdEncoding::CurveMeshIgnored => (super::AttributeField::Ignored, u32::from(identity)),
        BodyIdEncoding::BrepInteger => {
            let Some(value) = identity.checked_sub(1) else {
                return Ok(None);
            };
            (super::AttributeField::Integer, u32::from(value))
        }
    };
    let mut found = None;
    for definition in definitions {
        ctx.charge_work(1, "NX named body_id definition")?;
        if definition.name.as_str() != "body_id" {
            continue;
        }
        let stream = definition.stream_ordinal;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(definitions.len()),
            "NX body_id definition uniqueness",
        )?;
        if definitions
            .iter()
            .filter(|d| {
                d.stream_ordinal == stream
                    && (d.name.as_str() == "body_id" || d.xmt == definition.xmt)
            })
            .count()
            != 1
            || definition.field_codes.as_slice() != [field]
            || (matches!(encoding, BodyIdEncoding::BrepInteger) && definition.type_id.get() != 9000)
        {
            continue;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(entities.len()),
            "NX body_id attribute instances",
        )?;
        let mut instances = entities.iter().filter(|e| {
            e.stream_ordinal == stream && e.definition_xmt == u32::from(definition.xmt)
        });
        let Some(entity) = instances.next() else {
            continue;
        };
        if instances.next().is_some() {
            continue;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(entities.len()),
            "NX body_id instance identity uniqueness",
        )?;
        if entities
            .iter()
            .filter(|e| e.stream_ordinal == stream && e.xmt == entity.xmt)
            .count()
            != 1
        {
            continue;
        }
        let [target] = entity.trailing_references.values() else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(integers.len()),
            "NX body_id scalar storage",
        )?;
        let mut storage = integers
            .iter()
            .filter(|r| r.stream_ordinal == stream && u32::from(r.xmt) == *target);
        let Some(integer) = storage.next() else {
            continue;
        };
        if storage.next().is_some() {
            continue;
        }
        let [stored] = integer.values.as_slice() else {
            continue;
        };
        if *stored != value {
            continue;
        }
        let body_xmt = entity.leading_references[0];
        if body_xmt <= 1 {
            continue;
        }
        if found.is_some() {
            return Ok(None);
        }
        found = Some(BodyIdMatch {
            stream,
            body_xmt,
            definition: &definition.id,
            entity: &entity.id,
            value_record: &integer.id,
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests;
