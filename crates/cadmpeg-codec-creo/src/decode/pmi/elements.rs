// SPDX-License-Identifier: Apache-2.0
//! Annotation-element identities from the model data array.

use cadmpeg_core::bytes::find_iter;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use super::attributes::{expect, integer, Integer};

const PROTOTYPE_END: &[u8] = b"\xe0\x00AE_trf\0";

#[derive(Debug, Clone, Copy)]
pub(super) struct Element {
    pub(super) owner: u32,
    pub(super) owner_type: u32,
    pub(super) annotation: Option<u32>,
    pub(super) annotation_type: Option<u32>,
}

pub(super) fn elements(
    ctx: &DecodeContext<'_>,
    mut section: View<'_>,
) -> Result<Option<Vec<Option<Element>>>, CodecError> {
    const LABEL: &[u8] = b"\xe0\x00annot_elems\0";
    let mut hits = find_iter(section.window(), LABEL);
    let (Some(offset), None) = (hits.next(), hits.next()) else {
        return Ok(None);
    };
    let Some(position) = section.start().checked_add(offset + LABEL.len()) else {
        return Ok(None);
    };
    if section.seek(position).is_none() {
        return Ok(None);
    }
    if matches!(section.unread().first(), Some(0xf1..=0xf3)) && section.skip(1).is_none() {
        return Ok(None);
    }
    if expect(&mut section, b"\xf8").is_none() {
        return Ok(None);
    }
    let Some(count) = integer(&mut section).and_then(Integer::value) else {
        return Ok(None);
    };
    let Some(count) = section.counted(u64::from(count), 1) else {
        return Ok(None);
    };
    if expect(&mut section, b"\xf7").is_none() {
        return Ok(None);
    }
    let Some(class) = integer(&mut section).and_then(Integer::value) else {
        return Ok(None);
    };
    if expect(&mut section, b"\xfb\xe3\xe0\x00AE_Header\0").is_none() {
        return Ok(None);
    }
    let prototype_start = section.position();
    let Some(relative) = find_iter(section.unread(), PROTOTYPE_END).next() else {
        return Ok(None);
    };
    let Some(prototype_end) = section.position().checked_add(relative) else {
        return Ok(None);
    };
    let Some(prototype) = section.child(prototype_start, prototype_end) else {
        return Ok(None);
    };
    let first = named_element(prototype);
    let attribute_count = named_attribute_count(prototype);
    let (Some(first), Some(attribute_count)) = (first, attribute_count) else {
        return Ok(None);
    };
    if section.seek(prototype_end).is_none()
        || expect(&mut section, PROTOTYPE_END).is_none()
        || expect(&mut section, b"\xe1\xf2\xf7").is_none()
        || integer(&mut section) != Some(Integer::Value(class))
        || expect(&mut section, b"\xe3").is_none()
    {
        return Ok(None);
    }
    let mut rows = ctx.alloc_filled(count.get(), None, "Creo annotation element rows")?;
    let Some(first_row) = rows.first_mut() else {
        return Ok(None);
    };
    *first_row = Some(first);
    let last = rows.len();
    for (index, row) in rows.iter_mut().enumerate().skip(1) {
        let Some(element) = positional(&mut section, attribute_count, index + 1 == last) else {
            return Ok(None);
        };
        *row = Some(element);
    }
    Ok(Some(rows))
}

fn named_element(prototype: View<'_>) -> Option<Element> {
    Some(Element {
        owner: named_integer(prototype, b"\xe0\x01AE_Id\0")?.value()?,
        owner_type: named_integer(prototype, b"\xe0\x01AE_Type\0")?.value()?,
        annotation: named_integer(prototype, b"\xe0\x01Annot_Id\0")?.value(),
        annotation_type: named_integer(prototype, b"\xe0\x01Annot_Type\0")?.value(),
    })
}

fn named_integer(mut view: View<'_>, header: &[u8]) -> Option<Integer> {
    let mut hits = find_iter(view.window(), header);
    let (Some(offset), None) = (hits.next(), hits.next()) else {
        return None;
    };
    view.seek(view.start().checked_add(offset + header.len())?)?;
    if view.unread().first() == Some(&0xf1) {
        view.skip(1)?;
    }
    integer(&mut view)
}

fn named_attribute_count(mut view: View<'_>) -> Option<usize> {
    const LABEL: &[u8] = b"\xe0\x01AE_Attr\0";
    let mut hits = find_iter(view.window(), LABEL);
    let (Some(offset), None) = (hits.next(), hits.next()) else {
        return None;
    };
    view.seek(view.start().checked_add(offset + LABEL.len())?)?;
    if view.unread().first() == Some(&0xf1) {
        view.skip(1)?;
    }
    expect(&mut view, b"\xf8")?;
    let count = integer(&mut view)?.value()?;
    Some(view.counted(u64::from(count), 1)?.get())
}

fn positional(view: &mut View<'_>, attributes: usize, last: bool) -> Option<Element> {
    expect(view, b"\xe2\xe2")?;
    let owner = integer(view)?.value()?;
    let owner_type = integer(view)?.value()?;
    for _ in 0..attributes {
        if view.unread().first() == Some(&0xe4) {
            view.skip(1)?;
        } else {
            integer(view)?;
        }
    }
    expect(view, b"\xe2")?;
    let annotation = integer(view)?.value();
    let annotation_type = integer(view)?.value();
    expect(view, b"\xe2")?;
    integer(view)?;
    integer(view)?;
    expect(view, b"\xe2")?;
    integer(view)?;
    integer(view)?;
    integer(view)?;
    expect(view, b"\xe2\xe1")?;
    if !last {
        expect(view, b"\xe3")?;
    }
    Some(Element {
        owner,
        owner_type,
        annotation,
        annotation_type,
    })
}

#[cfg(test)]
mod tests;
