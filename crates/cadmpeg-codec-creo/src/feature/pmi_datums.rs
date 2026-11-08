// SPDX-License-Identifier: Apache-2.0
//! Bounded identities from datum-tag and datum-definition array rows.

use cadmpeg_core::bytes::find_iter;
use cadmpeg_core::decode::{BoundedCount, DecodeContext, View};
use cadmpeg_core::CodecError;

const GTOL_DATUM_ARRAY: &[u8] = b"\xe0\x00gtol_datum_def_xar\0";
const SET_DATUM_TAG_ARRAY: &[u8] = b"\xe0\x00set_datum_tag_xar\0";
const TAG_ARRAY_NEXT_FIELD: &[u8] = b"\xe0\x00drf_data_xar\0";
const DATUM_TYPE_FIELD: &[u8] = b"\xe0\x01datum_type\0";
const DATUM_ID_FIELD: &[u8] = b"\xe0\x01datum_id\0";
const FEATURE_ID_FIELD: &[u8] = b"\xe0\x01feat_id\0";
const GTOL_ID_FIELD: &[u8] = b"\xe0\x01gtol_id\0";
const DATUM_TAG_PROTOTYPE: &[u8] = b"\xe3\xe0\x09attributes\0";
const DATUM_DEFINITION_ID_FIELD: &[u8] = b"\xe0\x01dtm_def_id\0";
const SLAVE_INFO_FIELD: &[u8] = b"\xe0\x00p_sld_info\0";
const COSMETIC_FIELD: &[u8] = b"\xe0\x00p_cosm\0";
const GENERAL_INFO_FIELD: &[u8] = b"\xe0\x00gen_info\0";
const TEXT_FIELD: &[u8] = b"\xe0\x00p_text\0";
const ADDITIONAL_TEXT_FIELD: &[u8] = b"\xe0\x00p_addl_text\0";
const ATTACHMENT_FIELD: &[u8] = b"\xe0\x00p_attach\0";
const DATUM_TAG_NAME_FIELD: &[u8] = b"\xe0\x0ap_datum_tag_name\0";
const LEADER_FIELD: &[u8] = b"\xe0\x00p_ldr\0";
const NOTE_ATTACHMENT_FIELD: &[u8] = b"\xe0\x00note_att\0";
const ATTACHMENT_OFFSETS_FIELD: &[u8] = b"\xe0\x00att_offsets\0";
const TYPE_FIELD: &[u8] = b"\xe0\x01type\0";
const ATTACHMENT_ID_FIELD: &[u8] = b"\xe0\x01attach_id\0";
const POSITIONAL_SLAVE_INFO: &[u8] = b"\xe3\x80\xdb";
const POSITIONAL_TEXT_VALUE: &[u8] = b"\xe3\x3e\x12";

/// A datum label with its established definition-slot relationship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DatumTagIdentity {
    /// Absolute offset of the label field in the source view.
    pub(crate) source_offset: usize,
    /// The tag row's `dtm_def_id` value.
    pub(crate) definition_id: Option<u32>,
    /// The matching zero-based `gtol_datum_def_xar` slot.
    pub(crate) definition_slot: Option<u32>,
    /// The text stored in `p_datum_tag_name`.
    pub(crate) identification: String,
    /// The first definition row's typed leader reference.
    pub(crate) attachment: Option<DatumAttachment>,
}

/// The result of reading the datum-tag array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DatumTagOutcome {
    AbsentOrEmpty,
    Decoded {
        tags: Vec<DatumTagIdentity>,
        unresolved_populated_slots: usize,
        unparsed_label_slots: usize,
    },
    Unresolved {
        populated_slots: Option<usize>,
    },
}

impl DatumTagOutcome {
    pub(crate) fn label_roster_complete(&self) -> bool {
        match self {
            Self::AbsentOrEmpty => true,
            Self::Decoded {
                unparsed_label_slots,
                ..
            } => *unparsed_label_slots == 0,
            Self::Unresolved {
                populated_slots: Some(0),
            } => true,
            Self::Unresolved { .. } => false,
        }
    }
}

/// A typed reference stored by a datum leader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DatumAttachment {
    /// The native reference type.
    pub(crate) reference_type: u32,
    /// The identifier in that reference namespace.
    pub(crate) id: u32,
}

/// Read bounded datum-tag rows and join the prototype definition id to its slot.
pub(crate) fn datum_tags(
    ctx: &DecodeContext<'_>,
    section: View<'_>,
) -> Result<DatumTagOutcome, CodecError> {
    let mut tag_fields = find_iter(section.window(), SET_DATUM_TAG_ARRAY);
    let Some(tag_array) = tag_fields.next() else {
        return Ok(DatumTagOutcome::AbsentOrEmpty);
    };
    if tag_fields.next().is_some() {
        return Ok(DatumTagOutcome::Unresolved {
            populated_slots: None,
        });
    }
    let Some(tag_array) = section.start().checked_add(tag_array) else {
        return Ok(DatumTagOutcome::Unresolved {
            populated_slots: None,
        });
    };
    let Some(tag_array_end) =
        unique_field(section, TAG_ARRAY_NEXT_FIELD).filter(|offset| *offset > tag_array)
    else {
        return Ok(DatumTagOutcome::Unresolved {
            populated_slots: None,
        });
    };
    let Some((array, header, leading_nulls)) =
        array_at(section, tag_array, SET_DATUM_TAG_ARRAY, tag_array_end)
    else {
        return Ok(DatumTagOutcome::Unresolved {
            populated_slots: None,
        });
    };
    let Some(populated_slots) = header.slots.checked_sub(leading_nulls) else {
        return Ok(DatumTagOutcome::Unresolved {
            populated_slots: None,
        });
    };
    let Ok(populated_slots_u64) = u64::try_from(populated_slots) else {
        return Ok(DatumTagOutcome::Unresolved {
            populated_slots: Some(populated_slots),
        });
    };
    let Some(populated_slots) = array.counted(populated_slots_u64, 1).map(BoundedCount::get) else {
        return Ok(DatumTagOutcome::Unresolved {
            populated_slots: Some(populated_slots),
        });
    };
    if populated_slots == 0 {
        return Ok(DatumTagOutcome::AbsentOrEmpty);
    }

    let first_row = array.position();
    let Some(row_end) = first_row_end(array, first_row, header.class_id) else {
        return Ok(DatumTagOutcome::Unresolved {
            populated_slots: Some(populated_slots),
        });
    };
    let Some(row) = array.child(first_row, row_end) else {
        return Ok(DatumTagOutcome::Unresolved {
            populated_slots: Some(populated_slots),
        });
    };
    let Some(first_tag) = first_datum_tag(row, header.class_id) else {
        return Ok(DatumTagOutcome::Unresolved {
            populated_slots: Some(populated_slots),
        });
    };
    let definition_array =
        unique_field(section, GTOL_DATUM_ARRAY).filter(|offset| *offset < tag_array);
    let first_definition_slot = definition_array.and_then(|field| {
        first_datum_slot(section, field, tag_array)
            .filter(|slot| Some(*slot) == first_tag.definition_id)
    });
    let first_attachment = first_definition_slot.and_then(|_| {
        definition_array.and_then(|field| leader_attachment(section, field, tag_array))
    });
    let placeholder = DatumTagIdentity {
        source_offset: 0,
        definition_id: None,
        definition_slot: None,
        identification: String::new(),
        attachment: None,
    };
    let mut tags = ctx.alloc_filled(populated_slots, placeholder, "creo datum tag rows")?;
    tags[0] = DatumTagIdentity {
        source_offset: first_tag.source_offset,
        definition_id: first_tag.definition_id,
        definition_slot: first_definition_slot,
        identification: first_tag.identification,
        attachment: first_attachment,
    };

    let mut unresolved_populated_slots = usize::from(first_definition_slot.is_none());
    let remaining_slots = populated_slots - 1;
    if remaining_slots == 0 {
        return Ok(DatumTagOutcome::Decoded {
            tags,
            unresolved_populated_slots,
            unparsed_label_slots: 0,
        });
    }
    let Some(positional_start) = row_end.checked_add(1) else {
        return Ok(DatumTagOutcome::Unresolved {
            populated_slots: Some(populated_slots),
        });
    };
    let parsed_positional = positional_tag_rows(
        array,
        positional_start,
        tag_array_end,
        remaining_slots,
        &mut tags[1..],
    );
    let parsed_positional = match parsed_positional {
        Ok(count) => count,
        Err(count) => count,
    };
    tags.truncate(parsed_positional + 1);
    let unparsed_label_slots = remaining_slots - parsed_positional;
    unresolved_populated_slots += unparsed_label_slots;
    unresolved_populated_slots += tags
        .iter()
        .skip(1)
        .filter(|tag| tag.definition_slot.is_none())
        .count();
    Ok(DatumTagOutcome::Decoded {
        tags,
        unresolved_populated_slots,
        unparsed_label_slots,
    })
}

struct ArrayHeader {
    slots: usize,
    class_id: u32,
}

struct DatumTagPrototype {
    source_offset: usize,
    definition_id: Option<u32>,
    identification: String,
}

enum NullableCompact {
    Null,
    Value(u32),
}

enum PositionalDefinitionId {
    Known(u32),
    Unresolved,
}

fn first_datum_slot(section: View<'_>, field: usize, bound: usize) -> Option<u32> {
    let (array, header, leading_nulls) = array_at(section, field, GTOL_DATUM_ARRAY, bound)?;
    (leading_nulls < header.slots).then_some(())?;
    let row_end = first_row_end(array, array.position(), header.class_id)?;
    let mut row = array.child(array.position(), row_end)?;
    expect(&mut row, &[0xe3])?;
    expect(&mut row, DATUM_TYPE_FIELD)?;
    (compact(&mut row)? == 1).then_some(u32::try_from(leading_nulls).ok()?)
}

fn first_datum_tag(mut row: View<'_>, class_id: u32) -> Option<DatumTagPrototype> {
    if !row.unread().starts_with(DATUM_TAG_PROTOTYPE) {
        return None;
    }

    expect(&mut row, DATUM_TAG_PROTOTYPE)?;
    (compact(&mut row)? == 1).then_some(())?;
    expect(&mut row, DATUM_DEFINITION_ID_FIELD)?;
    let definition_id = match nullable_compact(&mut row)? {
        NullableCompact::Null => None,
        NullableCompact::Value(value) => Some(value),
    };
    expect(&mut row, SLAVE_INFO_FIELD)?;
    expect(&mut row, &[0xe3])?;

    let p_cosm = find_from(row, COSMETIC_FIELD, row.position())?;
    let gen_info = find_from(
        row,
        GENERAL_INFO_FIELD,
        p_cosm.checked_add(COSMETIC_FIELD.len())?,
    )?;
    let text = find_from(
        row,
        TEXT_FIELD,
        gen_info.checked_add(GENERAL_INFO_FIELD.len())?,
    )?;
    let additional_text = find_from(
        row,
        ADDITIONAL_TEXT_FIELD,
        text.checked_add(TEXT_FIELD.len())?,
    )?;
    let attachment = find_from(
        row,
        ATTACHMENT_FIELD,
        additional_text.checked_add(ADDITIONAL_TEXT_FIELD.len())?,
    )?;
    row.seek(attachment.checked_add(ATTACHMENT_FIELD.len())?)?;
    if row.unread().first() == Some(&0xf1) {
        row.skip(1)?;
    }
    expect(&mut row, &[0xe1])?;
    let source_offset = row.position();
    expect(&mut row, DATUM_TAG_NAME_FIELD)?;
    let label_length = row.unread().iter().position(|byte| *byte == 0)?;
    let identification = std::str::from_utf8(row.take(label_length)?)
        .ok()?
        .to_owned();
    if identification.is_empty() {
        return None;
    }
    expect(&mut row, &[0])?;
    expect(&mut row, &[0xf1, 0xf7])?;
    (compact(&mut row)? == class_id).then_some(())?;
    if !row.is_empty() {
        return None;
    }

    Some(DatumTagPrototype {
        source_offset,
        definition_id,
        identification,
    })
}

fn array_at<'a>(
    section: View<'a>,
    field: usize,
    name: &[u8],
    bound: usize,
) -> Option<(View<'a>, ArrayHeader, usize)> {
    let mut view = section.child(field, bound)?;
    view.seek(field.checked_add(name.len())?)?;
    let header = array_header(&mut view)?;
    let leading_nulls = leading_null_slots(&mut view, header.slots)?;
    Some((view, header, leading_nulls))
}

fn first_row_end(view: View<'_>, prototype: usize, class_id: u32) -> Option<usize> {
    let mut marker = [0_u8; 5];
    marker[0] = 0xf1;
    marker[1] = 0xf7;
    let class_length = write_compact(class_id, &mut marker[2..4])?;
    let marker_length = class_length.checked_add(3)?;
    *marker.get_mut(marker_length.checked_sub(1)?)? = 0xe3;
    let close = find_from(view, marker.get(..marker_length)?, prototype)?;
    close.checked_add(marker_length.checked_sub(1)?)
}

fn leader_attachment(
    section: View<'_>,
    datum_array: usize,
    tag_array: usize,
) -> Option<DatumAttachment> {
    let (array, header, leading_nulls) =
        array_at(section, datum_array, GTOL_DATUM_ARRAY, tag_array)?;
    (leading_nulls < header.slots).then_some(())?;
    let row_end = first_row_end(array, array.position(), header.class_id)?;
    let mut row = array.child(array.position(), row_end)?;
    expect(&mut row, &[0xe3])?;
    expect(&mut row, DATUM_TYPE_FIELD)?;
    (compact(&mut row)? == 1).then_some(())?;
    expect(&mut row, DATUM_ID_FIELD)?;
    nullable_compact(&mut row)?;
    expect(&mut row, FEATURE_ID_FIELD)?;
    nullable_compact(&mut row)?;
    expect(&mut row, GTOL_ID_FIELD)?;
    nullable_compact(&mut row)?;
    expect(&mut row, LEADER_FIELD)?;
    expect(&mut row, &[0xe3])?;

    let note_attachment = find_from(row, NOTE_ATTACHMENT_FIELD, row.position())?;
    let offsets = find_from(
        row,
        ATTACHMENT_OFFSETS_FIELD,
        note_attachment.checked_add(NOTE_ATTACHMENT_FIELD.len())?,
    )?;
    let mut note = row.child(
        note_attachment.checked_add(NOTE_ATTACHMENT_FIELD.len())?,
        offsets,
    )?;
    expect(&mut note, &[0xe3])?;
    expect(&mut note, TYPE_FIELD)?;
    let reference_type = compact(&mut note)?;
    expect(&mut note, ATTACHMENT_ID_FIELD)?;
    let id = compact(&mut note)?;
    note.is_empty()
        .then_some(DatumAttachment { reference_type, id })
}

fn array_header(view: &mut View<'_>) -> Option<ArrayHeader> {
    match view.unread() {
        [0xf1..=0xf3, ..] => view.skip(1)?,
        [0xf4, 0x04, ..] => view.skip(2)?,
        _ => {}
    }
    expect(view, &[0xf8])?;
    let slots = view.counted(u64::from(compact(view)?), 1)?.get();
    expect(view, &[0xf7])?;
    let class_id = compact(view)?;
    expect(view, &[0xfb])?;
    Some(ArrayHeader { slots, class_id })
}

fn leading_null_slots(view: &mut View<'_>, slots: usize) -> Option<usize> {
    let mut slot = 0_usize;
    while view.unread().first() == Some(&0xe1) {
        view.skip(1)?;
        slot = slot.checked_add(1)?;
        (slot <= slots).then_some(())?;
    }
    Some(slot)
}

fn positional_tag_rows(
    array: View<'_>,
    start: usize,
    end: usize,
    expected_rows: usize,
    output: &mut [DatumTagIdentity],
) -> Result<usize, usize> {
    let Some(bounded) = array.child(start, end) else {
        return Err(0);
    };
    let Some(row_count) = u64::try_from(expected_rows)
        .ok()
        .and_then(|count| bounded.counted(count, 1))
        .map(BoundedCount::get)
    else {
        return Err(0);
    };
    let actual_leaders = find_iter(bounded.window(), POSITIONAL_SLAVE_INFO).count();
    if actual_leaders != row_count || output.len() != row_count {
        return Err(0);
    }
    let mut cursor = bounded.start();
    for (index, output_row) in output.iter_mut().enumerate() {
        let Some(leader) = find_from(bounded, POSITIONAL_SLAVE_INFO, cursor) else {
            return Err(index);
        };
        let Some(after_leader) = leader.checked_add(POSITIONAL_SLAVE_INFO.len()) else {
            return Err(index);
        };
        let row_end = find_from(bounded, POSITIONAL_SLAVE_INFO, after_leader).unwrap_or(end);
        let Some(row) = array.child(leader, row_end) else {
            return Err(index);
        };
        let Some((identification, source_offset)) = positional_tag_texts(row) else {
            return Err(index);
        };
        let definition_id = match positional_definition_id(array, leader) {
            PositionalDefinitionId::Known(value) => Some(value),
            PositionalDefinitionId::Unresolved => None,
        };
        *output_row = DatumTagIdentity {
            source_offset,
            definition_id,
            definition_slot: None,
            identification,
            attachment: None,
        };
        cursor = after_leader;
    }
    Ok(row_count)
}

fn positional_definition_id(array: View<'_>, leader: usize) -> PositionalDefinitionId {
    let Some(value_offset) = leader.checked_sub(1) else {
        return PositionalDefinitionId::Unresolved;
    };
    let Some(type_offset) = value_offset.checked_sub(1) else {
        return PositionalDefinitionId::Unresolved;
    };
    let Some(mut type_view) = array.child(type_offset, value_offset) else {
        return PositionalDefinitionId::Unresolved;
    };
    let Some(field_type) = type_view.u8() else {
        return PositionalDefinitionId::Unresolved;
    };
    if field_type != 0x01 {
        return PositionalDefinitionId::Unresolved;
    }
    let Some(mut value) = array.child(value_offset, leader) else {
        return PositionalDefinitionId::Unresolved;
    };
    match compact(&mut value) {
        Some(id) if value.is_empty() => PositionalDefinitionId::Known(id),
        _ => PositionalDefinitionId::Unresolved,
    }
}

fn positional_tag_texts(mut row: View<'_>) -> Option<(String, usize)> {
    let (identification, source_offset) = positional_text(&mut row)?;
    let (additional, _) = positional_text(&mut row)?;
    if identification.is_empty()
        || !additional.is_empty()
        || find_from(row, POSITIONAL_TEXT_VALUE, row.position()).is_some()
    {
        return None;
    }
    Some((identification, source_offset))
}

fn positional_text(row: &mut View<'_>) -> Option<(String, usize)> {
    let field = find_from(*row, POSITIONAL_TEXT_VALUE, row.position())?;
    row.seek(field.checked_add(POSITIONAL_TEXT_VALUE.len())?)?;
    let label_length = row.unread().iter().position(|byte| *byte == 0)?;
    let value = std::str::from_utf8(row.take(label_length)?)
        .ok()?
        .to_owned();
    expect(row, &[0])?;
    Some((value, field))
}

fn compact(view: &mut View<'_>) -> Option<u32> {
    let local = view.read_len();
    let (value, end) = crate::psb::packed_compact_int(view.window(), local)?;
    view.seek(view.start().checked_add(end)?)?;
    Some(value)
}

fn nullable_compact(view: &mut View<'_>) -> Option<NullableCompact> {
    if view.unread().first() == Some(&0xf6) {
        view.skip(1)?;
        Some(NullableCompact::Null)
    } else {
        compact(view).map(NullableCompact::Value)
    }
}

fn write_compact(value: u32, output: &mut [u8]) -> Option<usize> {
    match value {
        0..=0x7f => {
            output[0] = u8::try_from(value).ok()?;
            Some(1)
        }
        0x80..=0x3fff => {
            output[0] = 0x80 | u8::try_from(value >> 8).ok()?;
            output[1] = u8::try_from(value & 0xff).ok()?;
            Some(2)
        }
        _ => None,
    }
}

fn expect(view: &mut View<'_>, bytes: &[u8]) -> Option<()> {
    (view.take(bytes.len())? == bytes).then_some(())
}

fn unique_field(view: View<'_>, needle: &[u8]) -> Option<usize> {
    let mut offsets = find_iter(view.window(), needle);
    let (Some(offset), None) = (offsets.next(), offsets.next()) else {
        return None;
    };
    view.start().checked_add(offset)
}

fn find_from(view: View<'_>, needle: &[u8], from: usize) -> Option<usize> {
    let offset = from.checked_sub(view.start())?;
    find_iter(view.window().get(offset..)?, needle)
        .next()
        .and_then(|relative| from.checked_add(relative))
}

#[cfg(test)]
mod tests;
