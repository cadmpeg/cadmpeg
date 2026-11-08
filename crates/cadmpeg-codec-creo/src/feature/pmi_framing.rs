// SPDX-License-Identifier: Apache-2.0
//! Typed prefixes of binary geometric-tolerance rows.

use cadmpeg_core::bytes::find_iter;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

const GEOM_TOL_ARRAY: &[u8] = b"\xe0\x00geom_tol_array\0";
const DIM_ARRAY: &[u8] = b"\xe0\x00dim_array\0";
const ARRAY_OPEN: u8 = 0xf8;
const ARRAY_CLOSE: u8 = 0xfb;
const CLASS_REFERENCE: u8 = 0xf7;
const COMPOUND_OPEN: u8 = 0xe3;
const COMPOUND_VALUE: u8 = 0xe2;
const NULL_ELEMENT: u8 = 0xe1;
const NULL_FEATURE: u8 = 0xf6;
const ATTRIBUTE_PREFIX: u8 = 0xf5;
const VALUE_PREFIX: u8 = 0xf0;
const TEXT_PREFIX: u8 = 0xe8;
const EXTENDED_ATTRIBUTE: u8 = 0xeb;
const NAMED_ITEM_FIELDS: &[u8] = b"\xe2\xe2";
const NAMED_ITEM_LABEL: &[u8] = b"\xe0\x00dtl_named_item\0";
const ITEM_LABEL: &[u8] = b"\xe0\x00dtl_item\0";
const NAME_LABEL: &[u8] = b"\xe0\x0aname\0";
const TYPE_FIELD: &[u8] = b"\xe0\x01type\0";
const ATTACH_ID_FIELD: &[u8] = b"\xe0\x01attach_id\0";
const ATT_OFFSETS_FIELD: &[u8] = b"\xe0\x00att_offsets\0";
const REF_TYPE_FIELD: &[u8] = b"\xe0\x01ref_type\0";
const REF_ID_FIELD: &[u8] = b"\xe0\x01ref_id\0";

/// The typed prefix of one geometric-tolerance row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GeometricTolerancePrefix<'a> {
    pub(crate) owner_id: u32,
    pub(crate) tolerance_type: u32,
    pub(crate) value_string: &'a [u8],
    pub(crate) placement: Option<PlacementPrefix>,
}

/// The fields available at the beginning of a geometric-tolerance placement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlacementPrefix {
    pub(crate) placement_type: u32,
    pub(crate) id: u32,
    pub(crate) attachment: Option<AttachmentPrefix>,
}

/// The first typed fields of an optional placement attachment.
///
/// The first attachment of a section names its `type`, `attach_id`, and
/// `att_offsets` fields; later attachments replay the same fields positionally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentPrefix {
    Other,
    Inline { reference_type: u32, id: u32 },
}

struct ArrayHeader {
    count: usize,
}

struct NamedPrefix<'a> {
    prefix: GeometricTolerancePrefix<'a>,
    record_offset: usize,
}

/// Find the unique typed row whose nested text owner equals an annotation slot.
pub(crate) fn prefixes_for_annotation_slots<'a>(
    ctx: &DecodeContext<'_>,
    section: View<'a>,
    annotation_slots: &[u32],
) -> Result<Option<Vec<Option<GeometricTolerancePrefix<'a>>>>, CodecError> {
    let mut prefixes = ctx.alloc_filled(
        annotation_slots.len(),
        None,
        "Creo geometric-tolerance prefixes",
    )?;
    let mut ambiguous = ctx.alloc_filled(
        annotation_slots.len(),
        false,
        "Creo ambiguous geometric-tolerance slots",
    )?;
    let mut slot_index = ctx.alloc_filled(
        annotation_slots.len(),
        (0_u32, None),
        "Creo geometric-tolerance slot index",
    )?;
    for (mapped, (index, slot)) in slot_index
        .iter_mut()
        .zip(annotation_slots.iter().copied().enumerate())
    {
        *mapped = (slot, Some(index));
    }
    slot_index.sort_unstable_by_key(|(slot, _)| *slot);
    for index in 0..slot_index.len() {
        let Some((slot, _)) = slot_index.get(index).copied() else {
            continue;
        };
        let duplicate = index
            .checked_sub(1)
            .and_then(|previous| slot_index.get(previous))
            .is_some_and(|(previous, _)| *previous == slot)
            || index
                .checked_add(1)
                .and_then(|next| slot_index.get(next))
                .is_some_and(|(next, _)| *next == slot);
        if duplicate {
            let Some((_, position)) = slot_index.get_mut(index) else {
                continue;
            };
            *position = None;
        }
    }
    if annotation_slots.is_empty() {
        return Ok(Some(prefixes));
    }
    let Some(()) = (|| {
        let array_offset = unique_field(section, GEOM_TOL_ARRAY)?;
        let sibling_offset = unique_field(section, DIM_ARRAY)?;
        if sibling_offset <= array_offset {
            return None;
        }
        let mut array = section.child(array_offset, sibling_offset)?;
        array.seek(array_offset.checked_add(GEOM_TOL_ARRAY.len())?)?;
        let header = array_header(&mut array)?;
        let leading_nulls = skip_leading_nulls(&mut array, header.count)?;
        let named = named_prefix(section.child(array.position(), sibling_offset)?)?;
        let array_count = u32::try_from(header.count).ok()?;
        if named.prefix.owner_id >= array_count {
            return None;
        }
        if leading_nulls != usize::try_from(named.prefix.owner_id).ok()? {
            return None;
        }
        retain_prefix(&slot_index, &mut prefixes, &mut ambiguous, named.prefix);

        let positional_start = named.record_offset.checked_add(1)?;
        let positional_lane = section.child(positional_start, sibling_offset)?;
        for offset in find_iter(positional_lane.window(), &[COMPOUND_OPEN]) {
            let source_offset = positional_start.checked_add(offset)?;
            let row_view = section.child(source_offset, sibling_offset)?;
            let Some(prefix) = positional_prefix(row_view) else {
                continue;
            };
            if prefix.owner_id <= named.prefix.owner_id || prefix.owner_id >= array_count {
                continue;
            }
            retain_prefix(&slot_index, &mut prefixes, &mut ambiguous, prefix);
        }
        Some(())
    })() else {
        return Ok(None);
    };
    for (index, is_ambiguous) in ambiguous.into_iter().enumerate() {
        if is_ambiguous {
            let Some(prefix) = prefixes.get_mut(index) else {
                return Ok(None);
            };
            *prefix = None;
        }
    }
    Ok(Some(prefixes))
}

fn retain_prefix<'a>(
    slot_index: &[(u32, Option<usize>)],
    prefixes: &mut [Option<GeometricTolerancePrefix<'a>>],
    ambiguous: &mut [bool],
    prefix: GeometricTolerancePrefix<'a>,
) {
    let Some(index) = slot_index
        .binary_search_by_key(&prefix.owner_id, |(slot, _)| *slot)
        .ok()
        .and_then(|index| slot_index.get(index).and_then(|(_, index)| *index))
    else {
        return;
    };
    let (Some(stored), Some(is_ambiguous)) = (prefixes.get_mut(index), ambiguous.get_mut(index))
    else {
        return;
    };
    if stored.is_some() {
        *is_ambiguous = true;
    } else {
        *stored = Some(prefix);
    }
}

/// The typed attachments of the `dim_array` element `span` whose `p_sld_info`
/// reference is the annotation owner `(ref_id, ref_type)`.
///
/// The `attach_ptr` chain follows `p_sld_info`; each link stores the
/// attachment fields of a tolerance placement. `None` means the owner
/// reference is absent or repeated.
pub(crate) fn dimension_attachments(
    section: View<'_>,
    span: crate::feature::dimensions::ElementSpan,
    owner: (u32, u32),
) -> Option<Vec<(u32, u32)>> {
    let start = section.start().checked_add(span.start)?;
    let end = section.start().checked_add(span.end)?;
    let element = section.child(start, end)?;
    let anchor = owner_reference_end(element, owner)?;
    let lane = section.child(anchor, end)?;
    let mut attachments = Vec::new();
    for offset in find_iter(lane.window(), &[COMPOUND_OPEN]) {
        let link = section.child(anchor.checked_add(offset)?, end)?;
        if let Some(AttachmentPrefix::Inline { reference_type, id }) = attachment_prefix(link) {
            attachments.push((reference_type, id));
        }
    }
    Some(attachments)
}

/// The end of the unique `p_sld_info` compound prefix that names `owner`.
fn owner_reference_end(element: View<'_>, (owner, owner_type): (u32, u32)) -> Option<usize> {
    let mut ends = find_iter(element.window(), &[COMPOUND_OPEN]).filter_map(|offset| {
        let mut view = element.child(element.start().checked_add(offset)?, element.end())?;
        expect(&mut view, &[COMPOUND_OPEN])?;
        if view.unread().first() == Some(&CLASS_REFERENCE) {
            class_reference(&mut view)?;
        }
        let named = view.unread().starts_with(REF_TYPE_FIELD);
        if named {
            expect(&mut view, REF_TYPE_FIELD)?;
            framing(&mut view)?;
        }
        (compact(&mut view)? == owner_type).then_some(())?;
        if named {
            expect(&mut view, REF_ID_FIELD)?;
            framing(&mut view)?;
        }
        (compact(&mut view)? == owner).then_some(view.position())
    });
    let (Some(end), None) = (ends.next(), ends.next()) else {
        return None;
    };
    Some(end)
}

/// Read one positional row from its `e3` element opener.
pub(crate) fn positional_prefix(mut row: View<'_>) -> Option<GeometricTolerancePrefix<'_>> {
    expect(&mut row, &[COMPOUND_OPEN])?;
    if row.unread().first() == Some(&CLASS_REFERENCE) {
        class_reference(&mut row)?;
    }
    expect(&mut row, NAMED_ITEM_FIELDS)?;
    item_name(&mut row)?;
    let tolerance_type = compact(&mut row)?;
    compact(&mut row)?;
    if row.unread().first() == Some(&NULL_FEATURE) {
        row.skip(1)?;
    } else {
        compact(&mut row)?;
    }
    let value_string = positional_value_strings(&mut row)?;
    let owner_id = positional_texts(&mut row)?;
    let placement = placement_prefix(row);
    Some(GeometricTolerancePrefix {
        owner_id,
        tolerance_type,
        value_string,
        placement,
    })
}

fn named_prefix(mut row: View<'_>) -> Option<NamedPrefix<'_>> {
    let source_offset = row.position();
    expect(&mut row, &[COMPOUND_OPEN])?;
    expect(&mut row, NAMED_ITEM_LABEL)?;
    expect(&mut row, &[COMPOUND_VALUE])?;
    expect(&mut row, ITEM_LABEL)?;
    expect(&mut row, &[COMPOUND_VALUE])?;
    expect(&mut row, NAME_LABEL)?;
    framing(&mut row)?;
    item_name(&mut row)?;
    expect(&mut row, b"\xe0\x01tol_type\0")?;
    framing(&mut row)?;
    let tolerance_type = compact(&mut row)?;
    expect(&mut row, b"\xe0\x01tol_class\0")?;
    compact(&mut row)?;
    expect(&mut row, b"\xe0\x01feat_id\0")?;
    if row.unread().first() == Some(&NULL_FEATURE) {
        row.skip(1)?;
    } else {
        compact(&mut row)?;
    }
    expect(&mut row, b"\xe0\x0avalue_string\0")?;
    if row.unread().first() != Some(&ARRAY_OPEN) {
        return None;
    }
    let value_string = positional_value_strings(&mut row)?;
    named_text(&mut row)?;
    let owner_id = named_text_suffix(&mut row)?;
    expect(&mut row, b"\xe0\x00gtol_place_ptr\0")?;
    let placement = placement_prefix(row);
    Some(NamedPrefix {
        prefix: GeometricTolerancePrefix {
            owner_id,
            tolerance_type,
            value_string,
            placement,
        },
        record_offset: source_offset,
    })
}

/// Consume a null or NUL-terminated `dtl_item` name.
fn item_name(row: &mut View<'_>) -> Option<()> {
    if row.unread().first() == Some(&NULL_ELEMENT) {
        row.skip(1)
    } else {
        nul_terminated(row).map(|_| ())
    }
}

/// Read the first segment string of a `value_string` array.
///
/// The `f8 <count>` header is absent when the count repeats the preceding
/// row's count; the strings then run to the `tol_text` array header.
fn positional_value_strings<'a>(row: &mut View<'a>) -> Option<&'a [u8]> {
    if row.unread().first() == Some(&ARRAY_OPEN) {
        row.skip(1)?;
        let count = compact(row)?;
        let count = row.counted(u64::from(count), 1)?.get();
        let first = value_text(row)?;
        for _ in 1..count {
            value_text(row)?;
        }
        return Some(first);
    }
    let first = value_text(row)?;
    while row.unread().first() != Some(&ARRAY_OPEN) {
        value_text(row)?;
    }
    Some(first)
}

/// Read a displayed magnitude: printable UTF-8 after an optional `e8` byte.
fn value_text<'a>(row: &mut View<'a>) -> Option<&'a [u8]> {
    let value = nul_terminated(row)?;
    let text = value.strip_prefix(&[TEXT_PREFIX]).unwrap_or(value);
    std::str::from_utf8(text)
        .ok()?
        .chars()
        .all(|character| !character.is_control())
        .then_some(value)
}

/// Read the positional `tol_text` elements and return their common owner slot.
fn positional_texts(row: &mut View<'_>) -> Option<u32> {
    expect(row, &[ARRAY_OPEN])?;
    let count = compact(row)?;
    let count = row.counted(u64::from(count), 1)?.get();
    class_reference(row)?;
    expect(row, &[ARRAY_CLOSE])?;
    let mut owner = None;
    for index in 0..count {
        if index > 0 && row.unread().first() == Some(&0xf1) {
            row.skip(1)?;
        }
        if row.unread().first() == Some(&CLASS_REFERENCE) {
            class_reference(row)?;
        }
        expect(row, &[COMPOUND_OPEN])?;
        if row.unread().first() == Some(&CLASS_REFERENCE) {
            class_reference(row)?;
        }
        compact(row)?;
        compact(row)?;
        nul_terminated(row)?;
        let element_owner = text_suffix(row)?;
        if owner.is_some_and(|owner| owner != element_owner) {
            return None;
        }
        owner = Some(element_owner);
    }
    owner
}

fn named_text(row: &mut View<'_>) -> Option<()> {
    expect(row, b"\xe0\x00tol_text\0")?;
    expect(row, &[ARRAY_OPEN])?;
    let count = compact(row)?;
    let text_array_count = row.counted(u64::from(count), 1)?.get();
    if text_array_count != 1 {
        return None;
    }
    class_reference(row)?;
    expect(row, &[ARRAY_CLOSE, COMPOUND_OPEN])?;
    expect(row, b"\xe0\x01db_type\0")?;
    compact(row)?;
    expect(row, b"\xe0\x01text_type\0")?;
    compact(row)?;
    expect(row, b"\xe0\x0atext_value\0")?;
    nul_terminated(row)?;
    Some(())
}

fn text_suffix(view: &mut View<'_>) -> Option<u32> {
    text_attributes(view)?;
    compact(view)?;
    let owner_id = compact(view)?;
    compact(view)?;
    compact(view)?;
    expect(view, &[COMPOUND_VALUE])?;
    compact(view)?;
    compact(view)?;
    compact(view)?;
    Some(owner_id)
}

fn named_text_suffix(view: &mut View<'_>) -> Option<u32> {
    expect(view, b"\xe0\x01txt_attrib\0")?;
    text_attributes(view)?;
    expect(view, b"\xe0\x01txt_attrib2\0")?;
    compact(view)?;
    expect(view, b"\xe0\x01owner_id\0")?;
    let owner_id = compact(view)?;
    expect(view, b"\xe0\x02height\0")?;
    compact(view)?;
    expect(view, b"\xe0\x02width_factor\0")?;
    compact(view)?;
    expect(view, b"\xe0\x00format_info\0")?;
    expect(view, &[COMPOUND_VALUE])?;
    expect(view, b"\xe0\x02thickness\0")?;
    framing(view)?;
    compact(view)?;
    expect(view, b"\xe0\x02actual_vertical_extent_start\0")?;
    compact(view)?;
    expect(view, b"\xe0\x02actual_vertical_extent_end\0")?;
    compact(view)?;
    Some(owner_id)
}

fn text_attributes(view: &mut View<'_>) -> Option<()> {
    if view.unread().first() == Some(&ATTRIBUTE_PREFIX) {
        view.skip(1)?;
    }
    if view.unread().first() == Some(&EXTENDED_ATTRIBUTE) {
        view.skip(5)?;
    } else {
        compact(view)?;
    }
    Some(())
}

fn array_header(view: &mut View<'_>) -> Option<ArrayHeader> {
    match view.unread() {
        [0xf1..=0xf3, ..] => view.skip(1)?,
        [0xf4, 0x04, ..] => view.skip(2)?,
        _ => {}
    }
    expect(view, &[ARRAY_OPEN])?;
    let encoded_count = compact(view)?;
    let count = view.counted(u64::from(encoded_count), 1)?.get();
    class_reference(view)?;
    expect(view, &[ARRAY_CLOSE])?;
    Some(ArrayHeader { count })
}

fn skip_leading_nulls(view: &mut View<'_>, count: usize) -> Option<usize> {
    let mut slots = 0_usize;
    while view.unread().first() == Some(&NULL_ELEMENT) {
        view.skip(1)?;
        slots = slots.checked_add(1)?;
        if slots >= count {
            return None;
        }
    }
    Some(slots)
}

fn placement_prefix(mut view: View<'_>) -> Option<PlacementPrefix> {
    if matches!(view.unread().first(), Some(0xf1..=0xf3)) {
        view.skip(1)?;
    }
    if view.unread().first() == Some(&CLASS_REFERENCE) {
        class_reference(&mut view)?;
    }
    expect(&mut view, &[COMPOUND_OPEN])?;
    let named = view.unread().starts_with(TYPE_FIELD);
    let (placement_type, id) = if named {
        expect(&mut view, TYPE_FIELD)?;
        framing(&mut view)?;
        let placement_type = compact(&mut view)?;
        expect(&mut view, b"\xe0\x01id\0")?;
        framing(&mut view)?;
        let id = compact(&mut view)?;
        expect(&mut view, b"\xe0\x01attribute\0")?;
        framing(&mut view)?;
        compact(&mut view)?;
        (placement_type, id)
    } else {
        let placement_type = compact(&mut view)?;
        let id = compact(&mut view)?;
        compact(&mut view)?;
        (placement_type, id)
    };
    if named {
        expect(&mut view, b"\xe0\x02coord\0")?;
        expect(&mut view, &[ARRAY_OPEN])?;
        (compact(&mut view)? == 3).then_some(())?;
    }
    let after_coordinates = point(view);
    let attachment = after_coordinates.and_then(|mut tail| {
        if named {
            expect(&mut tail, b"\xe0\x00gtol_attach_pnt\0")?;
        }
        attachment_prefix(tail)
    });
    Some(PlacementPrefix {
        placement_type,
        id,
        attachment,
    })
}

/// Skip a three-coordinate point whose `e4` to `e6` tokens repeat the
/// preceding coordinate one to three times.
fn point(mut view: View<'_>) -> Option<View<'_>> {
    let mut coordinates = 0_u8;
    while coordinates < 3 {
        let head = view.u8()?;
        let repeats = match head {
            0xe4..=0xe6 if coordinates > 0 => head - 0xe3,
            _ => {
                crate::assembly::frame_scalar(head, &mut view)?;
                1
            }
        };
        coordinates = coordinates.checked_add(repeats)?;
    }
    (coordinates == 3).then_some(view)
}

fn attachment_prefix(mut view: View<'_>) -> Option<AttachmentPrefix> {
    if view.unread().first() == Some(&NULL_ELEMENT) {
        return Some(AttachmentPrefix::Other);
    }
    expect(&mut view, &[COMPOUND_OPEN])?;
    let (reference_type, id) = if view.unread().starts_with(TYPE_FIELD) {
        expect(&mut view, TYPE_FIELD)?;
        optional(&mut view, VALUE_PREFIX)?;
        let reference_type = compact(&mut view)?;
        expect(&mut view, ATTACH_ID_FIELD)?;
        framing(&mut view)?;
        let id = compact(&mut view)?;
        expect(&mut view, ATT_OFFSETS_FIELD)?;
        (reference_type, id)
    } else {
        optional(&mut view, VALUE_PREFIX)?;
        if view.unread().first() == Some(&CLASS_REFERENCE) {
            class_reference(&mut view)?;
        }
        (compact(&mut view)?, compact(&mut view)?)
    };
    if view.unread().first() == Some(&CLASS_REFERENCE) {
        class_reference(&mut view)?;
    }
    expect(&mut view, &[ARRAY_OPEN])?;
    Some(AttachmentPrefix::Inline { reference_type, id })
}

fn optional(view: &mut View<'_>, byte: u8) -> Option<()> {
    if view.unread().first() == Some(&byte) {
        view.skip(1)?;
    }
    Some(())
}

fn nul_terminated<'a>(view: &mut View<'a>) -> Option<&'a [u8]> {
    let length = view.unread().iter().position(|byte| *byte == 0)?;
    let value = view.take(length)?;
    expect(view, &[0])?;
    Some(value)
}

fn class_reference(view: &mut View<'_>) -> Option<u32> {
    expect(view, &[CLASS_REFERENCE])?;
    compact(view)
}

fn framing(view: &mut View<'_>) -> Option<()> {
    if view.unread().first() == Some(&0xf1) {
        view.skip(1)?;
    }
    Some(())
}

fn compact(view: &mut View<'_>) -> Option<u32> {
    let local = view.read_len();
    let (value, end) = crate::psb::packed_compact_int(view.window(), local)?;
    view.seek(view.start().checked_add(end)?)?;
    Some(value)
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

#[cfg(test)]
mod tests;
