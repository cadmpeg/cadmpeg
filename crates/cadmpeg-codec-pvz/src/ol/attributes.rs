// SPDX-License-Identifier: Apache-2.0
//! The attribute stream (§5): appearance values, appearance references, and
//! the appearance table a tessellation patch indexes.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

use cadmpeg_core::decode::{index_from_u32, View};
use cadmpeg_core::CodecError;

use super::tlv::{self, Record};
use super::MemberResidue;
use crate::layout::{appearance_values, token};
use crate::read::{at, f32x3};

/// Tag of the texture reference that can stand between appearance values and
/// their appearance reference (§5.3).
const TEXTURE_REFERENCE: u32 = 6;
/// Bytes of the `tail` of an appearance reference (§5.2).
const REFERENCE_TAIL_LEN: usize = 17;

/// The colour and shading values of one appearance (§5.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AppearanceValues {
    pub(crate) base: [f32; 3],
    pub(crate) diffuse: [f32; 3],
    pub(crate) specular: [f32; 3],
    pub(crate) shininess: f32,
    pub(crate) emissive: [f32; 3],
    pub(crate) opacity: f32,
}

/// One defined appearance: its values and the name of its defining
/// reference.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Appearance {
    pub(crate) name: String,
    pub(crate) values: AppearanceValues,
}

/// The appearance table of one member (§5.3).
#[derive(Debug, Default)]
pub(crate) struct AppearanceTable {
    /// `base_index` of each tag-7 record, in stored order.
    entries: Vec<u32>,
    /// The appearance defined for each `base_index`.
    pub(crate) defined: BTreeMap<u32, Appearance>,
}

impl AppearanceTable {
    /// The `base_index` whose appearance table entry `index` names, when the
    /// entry exists and its appearance is defined.
    pub(crate) fn resolve(&self, index: u32) -> Option<u32> {
        self.entries
            .get(index_from_u32(index))
            .copied()
            .filter(|base| self.defined.contains_key(base))
    }
}

/// Reads the attribute stream.
pub(crate) fn read(
    mut stream: View<'_>,
    stream_id: u32,
    residue: &mut MemberResidue,
) -> Result<AppearanceTable, CodecError> {
    tlv::stream_index(&mut stream, stream_id)?;
    let mut table = AppearanceTable::default();
    let mut pending: Option<AppearanceValues> = None;
    while let Some(record) = tlv::next(&mut stream)? {
        let values = pending.take();
        match record.tag {
            tag if tag == u32::from(token::ATTR_APPEARANCE_VALUES)
                && record.payload.window().len() == appearance_values::LEN =>
            {
                if values.is_some() {
                    residue.appearance_orphan_values += 1;
                }
                pending = Some(read_values(record.payload)?);
            }
            tag if tag == u32::from(token::ATTR_APPEARANCE_REFERENCE) => {
                table_reference(&mut table, record, values, residue)?;
            }
            TEXTURE_REFERENCE => {
                pending = values;
                residue.attribute_records.count(record.tag);
            }
            _ if record.is_terminator() => {
                if values.is_some() {
                    residue.appearance_orphan_values += 1;
                }
            }
            tag => {
                if values.is_some() {
                    residue.appearance_orphan_values += 1;
                }
                residue.attribute_records.count(tag);
            }
        }
    }
    if pending.is_some() {
        residue.appearance_orphan_values += 1;
    }
    Ok(table)
}

fn table_reference(
    table: &mut AppearanceTable,
    record: Record<'_>,
    values: Option<AppearanceValues>,
    residue: &mut MemberResidue,
) -> Result<(), CodecError> {
    let mut payload = record.payload;
    let name_len = index_from_u32(payload.req_u32_le()?);
    let name_bytes = payload.req_take(name_len)?;
    let base_index = payload.req_u32_le()?;
    if payload.remaining() != REFERENCE_TAIL_LEN {
        return Err(CodecError::malformed(format_args!(
            "appearance reference at offset {} has a {}-byte tail",
            record.payload.start(),
            payload.remaining()
        )));
    }
    residue.appearance_tails += 1;
    table.entries.push(base_index);
    let Some(values) = values else {
        return Ok(());
    };
    let name_end = name_bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(name_bytes.len());
    let name = String::from_utf8_lossy(&name_bytes[..name_end]).into_owned();
    match table.defined.entry(base_index) {
        Entry::Occupied(_) => residue.appearance_redefinitions += 1,
        Entry::Vacant(slot) => {
            slot.insert(Appearance { name, values });
        }
    }
    Ok(())
}

fn read_values(payload: View<'_>) -> Result<AppearanceValues, CodecError> {
    Ok(AppearanceValues {
        base: f32x3(&mut at(payload, appearance_values::BASE_RGB)?)?,
        diffuse: f32x3(&mut at(payload, appearance_values::DIFFUSE_RGB)?)?,
        specular: f32x3(&mut at(payload, appearance_values::SPECULAR_RGB)?)?,
        shininess: at(payload, appearance_values::SHININESS)?.req_f32_le()?,
        emissive: f32x3(&mut at(payload, appearance_values::EMISSIVE_RGB)?)?,
        opacity: at(payload, appearance_values::OPACITY)?.req_f32_le()?,
    })
}
