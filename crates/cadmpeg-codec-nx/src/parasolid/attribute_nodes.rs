// SPDX-License-Identifier: Apache-2.0
//! Attribute records read from a terminated node sequence.

use super::{
    AttributeAction, AttributeDefinition, AttributeField, Entity51Record, EntityReferences,
    FieldNamesRecord, LegalOwnerFlags, NameReferences,
};
use cadmpeg_core::printable::PrintableString;
use cadmpeg_parasolid::records::node_type::{ATTRIBUTE, ATTRIB_DEF, FIELD_NAMES, KEY};
use cadmpeg_parasolid::records::primary::Node;
use cadmpeg_parasolid::records::PrimaryRecords;
use cadmpeg_parasolid::xmt::{NonNullXmt, XmtTarget};
use std::collections::BTreeMap;
use std::num::NonZeroU32;

/// Every `FIELD_NAMES` node with a complete non-null name list.
pub(super) fn field_names(primary: &PrimaryRecords<'_>) -> Vec<FieldNamesRecord> {
    primary
        .nodes()
        .filter(|node| node.kind() == FIELD_NAMES)
        .filter_map(|node| {
            let names = node
                .pointer_list("names")?
                .into_iter()
                .map(|name| NonNullXmt::try_from(name).ok())
                .collect::<Option<Vec<_>>>()?;
            Some(FieldNamesRecord {
                offset: node.range().start,
                byte_len: node.range().len(),
                xmt: NonNullXmt::try_from(node.index()).ok()?,
                name_xmts: NameReferences::try_from(names).ok()?,
            })
        })
        .collect()
}

/// Every `ATTRIBUTE` node.
pub(super) fn attributes(primary: &PrimaryRecords<'_>) -> Vec<Entity51Record> {
    primary
        .nodes()
        .filter(|node| node.kind() == ATTRIBUTE)
        .filter_map(|node| {
            Some(Entity51Record {
                offset: node.range().start,
                byte_len: node.range().len(),
                xmt: NonNullXmt::try_from(node.index()).ok()?,
                sequence: NonZeroU32::new(node.integer("node_id")?)?,
                definition_xmt: node.pointer("definition")?,
                leading_references: node.pointers([
                    "owner",
                    "next",
                    "previous",
                    "next_of_type",
                    "previous_of_type",
                ])?,
                trailing_references: EntityReferences::new(node.pointer_list("fields")?).ok()?,
            })
        })
        .collect()
}

/// Every `ATTRIB_DEF` node joined to the `KEY` node it names; an index that two
/// `KEY` nodes share names no key.
pub(super) fn definitions<'a>(
    bytes: &'a [u8],
    primary: &PrimaryRecords<'_>,
) -> Vec<AttributeDefinition<'a>> {
    let mut identifiers = BTreeMap::<u32, Option<(usize, PrintableString<&'a str>)>>::new();
    for node in primary.nodes().filter(|node| node.kind() == KEY) {
        let identifier = bytes
            .get(node.field_range("string").unwrap_or_default())
            .and_then(|name| PrintableString::from_bytes(name).ok())
            .map(|name| (node.range().start, name));
        identifiers
            .entry(node.index())
            .and_modify(|entry| *entry = None)
            .or_insert(identifier);
    }
    primary
        .nodes()
        .filter(|node| node.kind() == ATTRIB_DEF)
        .filter_map(|node| {
            let identifier_xmt = node.pointer("identifier")?;
            let (identifier_offset, name) = identifiers.get(&identifier_xmt)?.as_ref()?;
            Some(AttributeDefinition {
                offset: node.range().start,
                xmt: NonNullXmt::try_from(node.index()).ok()?,
                next_definition_xmt: XmtTarget::from_wire(node.pointer("next")?),
                identifier_xmt: NonNullXmt::try_from(identifier_xmt).ok()?,
                identifier_offset: *identifier_offset,
                name: *name,
                type_id: NonZeroU32::new(node.integer("type_id")?)?,
                action_codes: node
                    .byte_array::<8>("actions")?
                    .map(AttributeAction::try_from)
                    .into_iter()
                    .collect::<Result<Vec<_>, _>>()
                    .ok()?
                    .try_into()
                    .ok()?,
                field_names_xmt: XmtTarget::from_wire(node.pointer("field_names")?),
                legal_owner_flags: LegalOwnerFlags::try_from(field_bytes(
                    bytes,
                    &node,
                    "legal_owners",
                )?)
                .ok()?,
                field_codes: node
                    .bytes("fields")?
                    .into_iter()
                    .map(AttributeField::try_from)
                    .collect::<Result<Vec<_>, _>>()
                    .ok()?,
            })
        })
        .collect()
}

fn field_bytes<'a>(bytes: &'a [u8], node: &Node<'_>, name: &str) -> Option<&'a [u8]> {
    bytes.get(node.field_range(name)?)
}

#[cfg(test)]
mod tests;
