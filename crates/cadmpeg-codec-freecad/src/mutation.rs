// SPDX-License-Identifier: Apache-2.0
//! Checked semantic mutations of retained persistence records.

use crate::native::{native_id, ObjectRecord, PropertyFamily, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;

const RECOMPUTE_EXEMPT_PROPERTY_STATUS: u64 = (1 << 7) | (1 << 23) | (1 << 27);

/// Selects the owner of a persisted property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FcstdPropertyOwner<'a> {
    /// The document itself.
    Document,
    /// A declared application object, selected by persisted name.
    Object(&'a str),
}

pub(crate) fn set_value_attribute(
    ir: &mut CadIr,
    owner: FcstdPropertyOwner<'_>,
    property_name: &str,
    value_order: usize,
    attribute: &str,
    value: String,
) -> Result<(), CodecError> {
    valid_xml_name(attribute, "attribute")?;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::desktop(),
    )?;
    mutate_property(&ctx, ir, owner, property_name, |property| {
        let property_id = property.id.clone();
        if !matches!(
            property.family,
            PropertyFamily::Scalar
                | PropertyFamily::Quantity
                | PropertyFamily::Enumeration
                | PropertyFamily::Vector
                | PropertyFamily::Matrix
                | PropertyFamily::Placement
                | PropertyFamily::String
        ) {
            return Err(CodecError::NotImplemented(format!(
                "editing FCStd {} property {} requires a graph-aware serializer",
                format_args!("{:?}", property.family)
                    .to_string()
                    .to_lowercase(),
                property_id
            )));
        }
        if matches!(attribute, "file" | "File") {
            return Err(CodecError::NotImplemented(format!(
                "editing FCStd property {property_id} changes its link or side-entry graph"
            )));
        }
        let value_record = property
            .values_mut()
            .into_iter()
            .flatten()
            .find(|record| record.order == value_order)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "FCStd property {property_id} has no value at order {value_order}"
                ))
            })?;
        if !value_record.attributes.contains_key(attribute) {
            return Err(CodecError::NotImplemented(format!(
                "adding FCStd value attribute {attribute} requires a typed serializer"
            )));
        }
        if value_record.attributes.get(attribute) == Some(&value) {
            return Ok(false);
        }
        value_record.attributes.insert(attribute.to_owned(), value);
        Ok(true)
    })
}

fn mutate_property(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    owner: FcstdPropertyOwner<'_>,
    property_name: &str,
    mutation: impl FnOnce(&mut PropertyRecord) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    let owner_id = match owner {
        FcstdPropertyOwner::Document => native_id("document", "0"),
        FcstdPropertyOwner::Object(name) => native_id("object", name),
    };
    let namespace = ir.native.namespace_mut("fcstd");
    let mut properties = namespace.arena_as::<PropertyRecord>("properties")?;
    let property = crate::native::unique_property(properties.iter_mut(), |property| {
        property.owner == owner_id && property.name == property_name
    })
    .map_err(|_| {
        CodecError::malformed(format_args!(
            "ambiguous FCStd property {owner_id}.{property_name}"
        ))
    })?
    .ok_or_else(|| {
        CodecError::malformed(format_args!(
            "missing FCStd property {owner_id}.{property_name}"
        ))
    })?;
    let changed = mutation(property)?;
    let recompute =
        changed && matches!(owner, FcstdPropertyOwner::Object(_)) && requires_recompute(property);
    let mut objects = if recompute {
        namespace.arena_as::<ObjectRecord>("objects")?
    } else {
        Vec::new()
    };
    if recompute {
        let object = objects
            .iter_mut()
            .find(|object| object.id().as_str() == owner_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("missing FCStd property owner {owner_id}"))
            })?;
        object
            .attributes
            .insert("Touched".to_owned(), "1".to_owned());
    }
    namespace.set_arena(ctx, "properties", &properties)?;
    if recompute {
        namespace.set_arena(ctx, "objects", &objects)?;
    }
    Ok(())
}

pub(crate) fn requires_recompute(property: &PropertyRecord) -> bool {
    !matches!(property.name.as_str(), "Label" | "Label2")
        && property.status.unwrap_or(0) & RECOMPUTE_EXEMPT_PROPERTY_STATUS == 0
}

fn valid_xml_name(value: &str, role: &str) -> Result<(), CodecError> {
    let mut characters = value.chars();
    let valid = characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| {
            character == '_'
                || character == '-'
                || character == '.'
                || character.is_ascii_alphanumeric()
        });
    if valid {
        Ok(())
    } else {
        Err(CodecError::malformed(format_args!(
            "invalid FCStd {role} name {value:?}"
        )))
    }
}
