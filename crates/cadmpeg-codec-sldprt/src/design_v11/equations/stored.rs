// SPDX-License-Identifier: Apache-2.0
//! The values a relation stores beside its names (`docs/formats/sldprt.md`):
//! the f64 after each dimension reference, in metres, radians, or a count, on
//! the assigned side and in the expression.
//!
//! A relation whose byte 1 after its text is `02` or `03` stores its sides as
//! tokens. The assigned side holds one operand; the assigned name follows,
//! repeated, then ten bytes, then the expression side's tag and token count.
//! The offsets are records of `docs/layouts/sldprt.toml`.
//! A dimension reference opens its operand and holds the value; its handle
//! objects follow, then its name token, a counted string with four words of
//! which the first is 1. Operators and numbers are class-tagged tokens whose
//! first word is not 1.

use cadmpeg_core::decode::View;

use super::line::{self, Line, Segment};
use super::{counted, find, REPEAT, STRING};
use crate::layout::equation_dimension_reference_token as reference;
use crate::layout::equation_expression_separator as separator;
use crate::layout::equation_token_words as words;

/// The declaration of the relation-dimension class and of its wrapper, which
/// holds the dimension's value.
const DIMENSION: &[u8] = b"\xff\xff\x01\x00\x10\x00moRelDimension_c";
const WRAPPER: &[u8] = b"\xff\xff\x01\x00\x11\x00moDimRefWrapper_c";
/// A new-class tag and schema 1, which open a class declaration; the u16
/// length of the class name and the name follow.
const NEW_CLASS: &[u8] = b"\xff\xff\x01\x00";
/// The wrapper's repeat token is the relation-dimension token plus this.
const WRAPPER_AFTER_DIMENSION: u16 = 2;
/// The first word of a name token.
const NAME_LEAD: u32 = 1;

/// The values stored beside a line's names.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Stored {
    /// The value on the assigned side, where it is a dimension reference.
    pub(super) assigned: Option<f64>,
    /// The value beside each quoted name of the expression, in order, where
    /// it is a dimension reference.
    pub(super) reads: Vec<Option<f64>>,
}

/// The stored values of the relation of `line` whose first side's tag ends at
/// `side`, in `payload` up to the next relation; `form` is byte 1 of the six
/// bytes after its text. `None` where the sides are not read.
pub(super) fn read(payload: &[u8], side: usize, form: u8, line: &Line) -> Option<Stored> {
    if !matches!(form, 2 | 3) {
        return None;
    }
    let count = View::u16_le_at(payload, side)?;
    if count == 0 {
        return None;
    }
    let (assigned, end) = operand(payload, side + 2, &line.name)?;
    let (_, repeated) = counted(payload, end)?;
    let lead = &separator::SEPARATOR_LEAD_VALUE;
    if payload.get(repeated..repeated.checked_add(lead.len())?)? != lead {
        return None;
    }
    let mut cursor = class_end(payload, repeated.checked_add(separator::LEN)?)?;
    let tokens = View::u16_le_at(payload, cursor)?;
    cursor += 2;
    let mut names = line.segments.iter().filter_map(|segment| match segment {
        Segment::Name(name) => Some(name.as_str()),
        Segment::Text(_) => None,
    });
    let mut next = names.next();
    let mut reads = Vec::new();
    for _ in 0..tokens {
        if dimension(payload, cursor).is_none() {
            if let Some((text, lead, end)) = token(payload, cursor) {
                if lead != NAME_LEAD {
                    cursor = end;
                    continue;
                }
                if !same_name(&text, next?) {
                    return None;
                }
                reads.push(None);
                cursor = end;
                next = names.next();
                continue;
            }
        }
        let (value, end) = operand(payload, cursor, next?)?;
        reads.push(value);
        cursor = end;
        next = names.next();
    }
    next.is_none().then_some(Stored { assigned, reads })
}

/// The operand at `at` that names `name`: the value of a dimension reference
/// that opens there, and the offset after its name token.
fn operand(payload: &[u8], at: usize, name: &str) -> Option<(Option<f64>, usize)> {
    let (value, from) = match dimension(payload, at) {
        Some((value, after)) => (Some(value).filter(|value| value.is_finite()), after),
        None => (None, at),
    };
    Some((value, name_token(payload, from, name)?))
}

/// The value of the dimension reference at `at`, and the offset after it: the
/// relation-dimension class tag, four zero bytes, the wrapper's class tag, and
/// a little-endian f64. A stream's first reference declares the two classes;
/// a later one repeats them by token (`docs/layouts/sldprt.toml`
/// `equation_dimension_reference_token`).
fn dimension(payload: &[u8], at: usize) -> Option<(f64, usize)> {
    let pad = &reference::DIMENSION_PAD_VALUE;
    let value = if payload.get(at..)?.starts_with(DIMENSION) {
        let from = at.checked_add(DIMENSION.len())?;
        let wrapper = from.checked_add(pad.len())?;
        if payload.get(from..wrapper)? != pad {
            return None;
        }
        if payload.get(wrapper..)?.starts_with(WRAPPER) {
            wrapper.checked_add(WRAPPER.len())?
        } else {
            repeat_token(payload, wrapper)?;
            wrapper.checked_add(std::mem::size_of::<u16>())?
        }
    } else {
        let token = repeat_token(payload, at)?;
        let from = at.checked_add(reference::DIMENSION_PAD)?;
        if payload.get(from..from.checked_add(pad.len())?)? != pad
            || View::u16_le_at(payload, at.checked_add(reference::WRAPPER_CLASS_TOKEN)?)
                != token.checked_add(WRAPPER_AFTER_DIMENSION)
        {
            return None;
        }
        at.checked_add(reference::STORED_VALUE)?
    };
    Some((
        View::f64_le_at(payload, value)?,
        value.checked_add(std::mem::size_of::<f64>())?,
    ))
}

/// The class repeat token at `at`: bit 15 set, and not the new-class tag.
fn repeat_token(payload: &[u8], at: usize) -> Option<u16> {
    View::u16_le_at(payload, at).filter(|token| token & REPEAT != 0 && *token != u16::MAX)
}

/// The offset after the class tag at `at`: a declaration or a repeat token.
fn class_end(payload: &[u8], at: usize) -> Option<usize> {
    if payload.get(at..)?.starts_with(NEW_CLASS) {
        let length = View::u16_le_at(payload, at + NEW_CLASS.len())?;
        return (at + NEW_CLASS.len() + 2).checked_add(usize::from(length));
    }
    repeat_token(payload, at).map(|_| at + 2)
}

/// The token at `at`, after its class tag if it has one: its text, its lead
/// word, and the offset after its words (`docs/layouts/sldprt.toml`
/// `equation_token_words`, whose first field is the lead word).
fn token(payload: &[u8], at: usize) -> Option<(String, u32, usize)> {
    let start = if payload.get(at..)?.starts_with(STRING) {
        at
    } else {
        class_end(payload, at)?
    };
    let (text, end) = counted(payload, start)?;
    let lead = View::u32_le_at(payload, end)?;
    let after = end.checked_add(words::LEN)?;
    (after <= payload.len()).then_some((text, lead, after))
}

/// The offset after the first name token from `from` whose text is `name`
/// quoted, in any letter case.
fn name_token(payload: &[u8], from: usize, name: &str) -> Option<usize> {
    let mut at = find(payload, STRING, from)?;
    loop {
        if let Some((text, lead, end)) = token(payload, at) {
            if lead == NAME_LEAD && same_name(&text, name) {
                return Some(end);
            }
        }
        at = find(payload, STRING, at + 1)?;
    }
}

/// Whether a token's text is `name` quoted, in any letter case.
fn same_name(text: &str, name: &str) -> bool {
    text.to_lowercase() == line::quoted(name).to_lowercase()
}
