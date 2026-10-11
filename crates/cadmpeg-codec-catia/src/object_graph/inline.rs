// SPDX-License-Identifier: Apache-2.0
//! Complete childless field-incidence production.

use cadmpeg_core::decode::View;

/// One compact or paged entity identity with its inline-body position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FieldReference {
    pub(crate) identity: u32,
    pub(crate) offset: usize,
}

/// Exact schema-tuple dialect, retaining both wide selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchemaTuple {
    Compact,
    Words { outer: u32, field: u32 },
}

/// Three distinct field roles after complete grammar consumption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InlineField {
    pub(crate) self_field: FieldReference,
    pub(crate) anchor_field: FieldReference,
    pub(crate) value_field: Option<FieldReference>,
    pub(crate) schema: SchemaTuple,
}

fn reference(body: &[u8], at: &mut usize) -> Option<FieldReference> {
    let offset = *at;
    let first = *body.get(offset)?;
    let identity = match first {
        0x80..=0xd0 => {
            *at += 1;
            u32::from(first - 0x80)
        }
        0xd1..=0xe4 => {
            let low = *body.get(offset + 1)?;
            *at += 2;
            u32::from(first - 0xd1) * 256 + u32::from(low) + 1
        }
        _ => return None,
    };
    Some(FieldReference { identity, offset })
}

/// Recognize only the complete strict production; opaque bodies have no roles.
pub(crate) fn parse(body: &[u8]) -> Option<InlineField> {
    body.strip_prefix(&[0x10, 0xfe])?;
    let mut at = 2;
    let self_field = reference(body, &mut at)?;
    let schema = if body[at..].starts_with(&[0x82, 0xf2, 0xf0, 0x82]) {
        at += 4;
        SchemaTuple::Compact
    } else {
        use crate::layout::inline_schema_words as words;
        let tuple = body.get(at..at.checked_add(words::LEN)?)?;
        if tuple[words::OPEN_COUNT] != 0x82
            || tuple[words::OUTER_MARKER] != 0x32
            || tuple[words::FIELD_MARKER] != 0x32
            || tuple[words::CLOSE_COUNT] != 0x82
        {
            return None;
        }
        let outer = View::u32_le_at(tuple, words::OUTER_SELECTOR)?;
        let field = View::u32_le_at(tuple, words::FIELD_SELECTOR)?;
        at += words::LEN;
        SchemaTuple::Words { outer, field }
    };
    let anchor_field = reference(body, &mut at)?;
    let value_field = match *body.get(at)? {
        0x81 => {
            at += 1;
            None
        }
        0x82 => {
            at += 1;
            Some(reference(body, &mut at)?)
        }
        _ => return None,
    };
    (body.get(at..) == Some(&[0x06])).then_some(InlineField {
        self_field,
        anchor_field,
        value_field,
        schema,
    })
}

#[cfg(test)]
mod tests;
