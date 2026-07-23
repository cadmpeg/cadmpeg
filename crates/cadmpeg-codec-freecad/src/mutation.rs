// SPDX-License-Identifier: Apache-2.0
//! Checked semantic mutations of retained persistence records.

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::wire::hash::sha256_hex;

use crate::native::{native_id, EntryRecord, PropertyRecord};

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
        let value_record = property
            .values
            .iter_mut()
            .find(|record| record.order == value_order)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "FCStd property {} has no value at order {value_order}",
                    property.id
                ))
            })?;
        value_record.attributes.insert(attribute.to_owned(), value);
        Ok(())
    })
}

pub(crate) fn set_value_text(
    ir: &mut CadIr,
    owner: FcstdPropertyOwner<'_>,
    property_name: &str,
    value_order: usize,
    text: Option<String>,
) -> Result<(), CodecError> {
    mutate_property(ir, owner, property_name, |property| {
        let value_record = property
            .values
            .iter_mut()
            .find(|record| record.order == value_order)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "FCStd property {} has no value at order {value_order}",
                    property.id
                ))
            })?;
        value_record.text = text;
        Ok(())
    })
}

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
        .ok_or_else(|| CodecError::Malformed(format!("missing FCStd entry {entry_name}")))?;
    entry.byte_len = bytes.len() as u64;
    entry.sha256 = sha256_hex(&bytes);
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
    let matches = properties
        .iter()
        .enumerate()
        .filter(|(_, property)| property.owner == owner_id && property.name == property_name)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let index = match matches.as_slice() {
        [index] => *index,
        [] => {
            return Err(CodecError::Malformed(format!(
                "missing FCStd property {owner_id}.{property_name}"
            )))
        }
        _ => {
            return Err(CodecError::Malformed(format!(
                "ambiguous FCStd property {owner_id}.{property_name}"
            )))
        }
    };
    mutation(&mut properties[index])?;
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
        Err(CodecError::Malformed(format!(
            "invalid FCStd {role} name {value:?}"
        )))
    }
}
