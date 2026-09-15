// SPDX-License-Identifier: Apache-2.0
//! Checked semantic mutations of retained persistence records.

#[cfg(test)]
use crate::native::EntryRecord;
use crate::native::{native_id, PropertyFamily, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;

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
    mutate_property(ir, owner, property_name, |property| {
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
        value_record.attributes.insert(attribute.to_owned(), value);
        Ok(())
    })
}

#[cfg(test)]
pub(crate) fn replace_entry(
    ir: &mut CadIr,
    entry_name: &str,
    bytes: Vec<u8>,
) -> Result<(), CodecError> {
    if entry_name == "Document.xml" {
        return Err(CodecError::NotImplemented(
            "Document.xml must be edited through typed property records".into(),
        ));
    }
    let namespace = ir.native.namespace_mut("fcstd");
    let mut entries = namespace.arena_as::<EntryRecord>("entries")?;
    let entry = entries
        .iter_mut()
        .find(|candidate| candidate.name == entry_name)
        .ok_or_else(|| CodecError::malformed(format_args!("missing FCStd entry {entry_name}")))?;
    entry.data = bytes;
    namespace.set_arena("entries", &entries)?;
    Ok(())
}

fn mutate_property(
    ir: &mut CadIr,
    owner: FcstdPropertyOwner<'_>,
    property_name: &str,
    mutation: impl FnOnce(&mut PropertyRecord) -> Result<(), CodecError>,
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
    mutation(property)?;
    namespace.set_arena("properties", &properties)?;
    Ok(())
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
