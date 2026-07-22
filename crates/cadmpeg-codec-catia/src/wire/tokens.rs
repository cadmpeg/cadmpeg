//! Compact-int and reference-token readers shared by the `b5` and `e5`
//! families.
//!
//! These free functions expose the `(&[u8], &mut usize)` signature the `b5` and
//! `e5` scan loops read against. `object_ref` and `compact_uint` are thin
//! adapters over [`Cursor`], which owns the byte semantics; `counted_refs`
//! composes `object_ref`. The free-function surface is the settled boundary
//! between those position-threading loops and the cursor.

use super::cursor::Cursor;

/// Reads a reference token at `*position`, advancing `*position` past it.
///
/// `extended` selects the token dialect. `e5` payloads use the restricted
/// dialect (`extended = false`): the lead bytes `0x38`, `0x18`, `0x10`,
/// `0x08`, and any `0x80..=0xff`. `b5` payloads use the extended dialect
/// (`extended = true`), which additionally recognises `0x30`, `0x28`, and
/// `0x20`. Any other lead byte fails the read without advancing.
pub(crate) fn object_ref(bytes: &[u8], position: &mut usize, extended: bool) -> Option<u32> {
    let mut cursor = Cursor::new_at(bytes, *position);
    let value = cursor.object_ref(extended)?;
    *position = cursor.position();
    Some(value)
}

/// Reads a compact unsigned integer at `*position`, advancing past it.
///
/// See [`Cursor::compact_uint`] for the encoding.
pub(crate) fn compact_uint(bytes: &[u8], position: &mut usize) -> Option<u32> {
    let mut cursor = Cursor::new_at(bytes, *position);
    let value = cursor.compact_uint()?;
    *position = cursor.position();
    Some(value)
}

/// Reads a count-prefixed reference list: a lead byte `0x80 + count` followed
/// by `count` reference tokens of the given `extended` dialect.
///
/// Returns the references and the offset just past the last token. Callers
/// that require the payload to be fully consumed check the returned offset
/// against the payload length themselves.
pub(crate) fn counted_refs(payload: &[u8], extended: bool) -> Option<(Vec<u32>, usize)> {
    let count = usize::from(payload.first()?.checked_sub(0x80)?);
    let mut position = 1;
    let mut references = Vec::with_capacity(count);
    for _ in 0..count {
        references.push(object_ref(payload, &mut position, extended)?);
    }
    Some((references, position))
}
