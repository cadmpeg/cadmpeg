// SPDX-License-Identifier: Apache-2.0
//! Subtype reference tables, intcurve subtype classification, and token walkers.

use crate::kernel_header::RefWidth;
use crate::nurbs::reader::INT_WIDTHS;
use crate::sab::{int_le_at, Record};
use cadmpeg_core::decode::View;

/// Byte offsets and names of the subtype definitions `bytes` itself owns: the
/// `0x0f` openings at the outermost nesting level, in stream order, `ref`
/// included. A definition inside a nested scope belongs to that scope's
/// construction, not to `bytes`.
pub(crate) fn owned_subtype_defs(bytes: &[u8], int_width: RefWidth) -> Vec<(usize, &[u8])> {
    let mut owned = Vec::new();
    let mut depth = 0usize;
    let mut pos = 0usize;
    while pos < bytes.len() {
        match bytes[pos] {
            0x0f => {
                if depth == 0 && matches!(bytes.get(pos + 1), Some(0x0d | 0x0e)) {
                    let len = usize::from(*bytes.get(pos + 2).unwrap_or(&0));
                    if let Some(name) = bytes.get(pos + 3..pos + 3 + len) {
                        owned.push((pos, name));
                    }
                }
                depth += 1;
            }
            0x10 => depth = depth.saturating_sub(1),
            _ => {}
        }
        match next_token(bytes, pos, int_width) {
            Some(next) => pos = next,
            None => break,
        }
    }
    owned
}

/// Byte offset of the first subtype definition `bytes` owns whose name matches
/// one of `names`, together with the matched name. Names are tried in order;
/// the first name with a hit wins.
///
/// A construction claims a record only through the definition the record owns.
/// Records nest complete constructions as supports — a rolling-ball blend
/// embeds a variable blend, a variable blend embeds an extrusion — so a decoder
/// that accepted any matching marker anywhere in the record would claim records
/// belonging to the construction that encloses it.
pub(crate) fn find_owned_subtype_marker<'n>(
    bytes: &[u8],
    names: &[&'n [u8]],
    int_width: RefWidth,
) -> Option<(usize, &'n [u8])> {
    let owned = owned_subtype_defs(bytes, int_width);
    names.iter().copied().find_map(|name| {
        owned
            .iter()
            .find(|(_, owned_name)| *owned_name == name)
            .map(|(start, _)| (*start, name))
    })
}

/// Byte offset and name length of the `intcurve` subtype definition `bytes`
/// owns, given the subtype's modern name. The legacy spelling of the same
/// construction is accepted as a second candidate.
pub(crate) fn find_owned_intcurve_subtype(
    bytes: &[u8],
    modern: &[u8],
    int_width: RefWidth,
) -> Option<(usize, usize)> {
    let legacy: &[u8] = match modern {
        b"blend_int_cur" => b"bldcur",
        b"spring_int_cur" => b"blndsprngcur",
        b"exact_int_cur" => b"exactcur",
        b"law_int_cur" => b"lawintcur",
        b"off_int_cur" => b"offintcur",
        b"offset_int_cur" => b"offsetintcur",
        b"off_surf_int_cur" => b"offsurfintcur",
        b"para_silh_int_cur" => b"parasil",
        b"par_int_cur" => b"parcur",
        b"proj_int_cur" => b"projcur",
        b"surf_int_cur" => b"surfcur",
        b"int_int_cur" => b"surfintcur",
        b"skin_int_cur" => b"d5c2_cur",
        b"subset_int_cur" => b"subsetintcur",
        _ => b"",
    };
    let candidates: Vec<&[u8]> = [modern, legacy]
        .into_iter()
        .filter(|name| !name.is_empty())
        .collect();
    find_owned_subtype_marker(bytes, &candidates, int_width)
        .map(|(marker, name)| (marker, name.len()))
}

pub(crate) fn decode_cache_resolving_refs<T>(
    bytes: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
    seen: &mut Vec<usize>,
    decode_inline: fn(&[u8], RefWidth) -> Option<T>,
    int_width: RefWidth,
) -> Option<T> {
    if let Some(decoded) = decode_inline(bytes, int_width) {
        return Some(decoded);
    }
    let table = tables.for_width(int_width);
    for index in subtype_refs(bytes, int_width) {
        if seen.contains(&index) {
            continue;
        }
        let target = *table.get(index)?;
        seen.push(index);
        if let Some(decoded) = decode_cache_resolving_refs(
            subtype_span(active_bytes, target, int_width)?,
            active_bytes,
            tables,
            seen,
            decode_inline,
            int_width,
        ) {
            return Some(decoded);
        }
    }
    None
}

/// Byte positions of the stream's subtype definitions, one table per candidate
/// integer width.
///
/// A subtype definition opens as `0x0f` followed by a `0x0d`/`0x0e` name token
/// other than `ref`; the table indexes definitions in stream order. Definition
/// openings are recognized only at token boundaries — the same byte pattern
/// inside an `f64` payload is data, not a definition — so the table is built by
/// token-walking the framed records, not by scanning raw bytes.
pub struct SubtypeTables {
    tables: [Vec<usize>; INT_WIDTHS.len()],
}

impl SubtypeTables {
    /// Build the tables by token-walking each framed record of `bytes`.
    pub fn from_records(records: &[Record], bytes: &[u8]) -> Self {
        Self {
            tables: INT_WIDTHS.map(|walk_width| {
                let mut table = Vec::new();
                for record in records {
                    collect_defs_in_span(
                        bytes,
                        record.offset,
                        record.offset + record.len,
                        walk_width,
                        &mut table,
                    );
                }
                table
            }),
        }
    }

    /// Build the tables by token-walking `bytes` as one contiguous token run.
    pub fn from_stream(bytes: &[u8]) -> Self {
        Self {
            tables: INT_WIDTHS.map(|walk_width| {
                let mut table = Vec::new();
                collect_defs_in_span(bytes, 0, bytes.len(), walk_width, &mut table);
                table
            }),
        }
    }

    /// The table built for the specified stream width.
    pub fn for_width(&self, int_width: RefWidth) -> &[usize] {
        match int_width {
            RefWidth::Eight => &self.tables[0],
            RefWidth::Four => &self.tables[1],
        }
    }

    /// Return the table index assigned to an absolute subtype-definition offset,
    /// for tests.
    pub fn index_of_offset(&self, int_width: RefWidth, offset: usize) -> Option<usize> {
        self.for_width(int_width)
            .iter()
            .position(|candidate| *candidate == offset)
    }
}

/// Append the token-boundary subtype-definition openings in
/// `bytes[start..end]` to `table`. Stops at the first unwalkable token.
fn collect_defs_in_span(
    bytes: &[u8],
    start: usize,
    end: usize,
    int_width: RefWidth,
    table: &mut Vec<usize>,
) {
    let end = end.min(bytes.len());
    let mut pos = start;
    while pos < end {
        if bytes[pos] == 0x0f && matches!(bytes.get(pos + 1), Some(0x0d | 0x0e)) {
            let len = usize::from(*bytes.get(pos + 2).unwrap_or(&0));
            if let Some(name) = bytes.get(pos + 3..pos + 3 + len) {
                if name != b"ref" {
                    table.push(pos);
                }
            }
        }
        match next_token(bytes, pos, int_width) {
            Some(next) => pos = next,
            None => return,
        }
    }
}

/// Subtype-table reference indices in `bytes`, in token order. References are
/// recognized only at token boundaries, mirroring [`SubtypeTables`].
pub(crate) fn subtype_refs(bytes: &[u8], int_width: RefWidth) -> Vec<usize> {
    let mut refs = Vec::new();
    let marker = b"\x0f\x0d\x03ref\x04";
    let mut pos = 0usize;
    while pos < bytes.len() {
        if bytes[pos..].starts_with(marker) {
            if let Some(index) = int_le_at(bytes, pos + marker.len(), int_width) {
                if index >= 0 {
                    refs.push(index as usize);
                }
            }
        } else if bytes.get(pos) == Some(&0x0f)
            && bytes.get(pos + 1) == Some(&0x04)
            && bytes.get(pos + 2 + int_width.bytes()) == Some(&0x10)
        {
            if let Some(index) = int_le_at(bytes, pos + 2, int_width) {
                if index >= 0 {
                    refs.push(index as usize);
                }
            }
        }
        match next_token(bytes, pos, int_width) {
            Some(next) => pos = next,
            None => break,
        }
    }
    refs
}

/// The byte span of the subtype definition that opens at `start`: from its
/// `0x0f` opening through the matching `0x10` close, nested definitions
/// included.
pub fn subtype_span(bytes: &[u8], start: usize, int_width: RefWidth) -> Option<&[u8]> {
    let mut depth = 0usize;
    let mut pos = start;
    while pos < bytes.len() {
        match bytes[pos] {
            0x0f => depth += 1,
            0x10 => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return bytes.get(start..=pos);
                }
            }
            _ => {}
        }
        pos = next_token(bytes, pos, int_width)?;
    }
    None
}

/// Offset of the token after the one at `pos`, or `None` when the tag is
/// unrecognized or its payload runs past the end. `0x04`, `0x0c` and `0x15`
/// carry an `int_width` payload; `0x09` and `0x12` carry an `int_width` string
/// length prefix, unlike the one- and two-byte prefixes of `0x07` and `0x08`.
pub(crate) fn next_token(bytes: &[u8], pos: usize, int_width: RefWidth) -> Option<usize> {
    let tag = *bytes.get(pos)?;
    let fixed = match tag {
        0x02 => 2,
        0x03 => 3,
        0x04 | 0x0c | 0x15 => 1 + int_width.bytes(),
        0x06 | 0x17 => 9,
        0x05 => 5,
        0x0a | 0x0b | 0x0f | 0x10 | 0x11 => 1,
        0x13 | 0x14 => 25,
        0x16 => 17,
        0x07 | 0x0d | 0x0e => 2 + usize::from(*bytes.get(pos + 1)?),
        0x08 => 3 + usize::from(View::u16_le_at(bytes, pos + 1)?),
        0x09 | 0x12 => {
            let length = int_le_at(bytes, pos + 1, int_width)?;
            1 + int_width.bytes() + usize::try_from(length).ok()?
        }
        _ => return None,
    };
    let next = pos.checked_add(fixed)?;
    (next <= bytes.len()).then_some(next)
}

#[cfg(test)]
mod ownership_tests {
    use super::*;

    /// A subtype definition opening: `0x0f`, name token, length, name bytes.
    fn open(bytes: &mut Vec<u8>, name: &[u8]) {
        bytes.push(0x0f);
        bytes.push(0x0d);
        bytes.push(u8::try_from(name.len()).expect("short name"));
        bytes.extend_from_slice(name);
    }

    /// A `defm_int_cur` record whose bend data nests a complete `int_int_cur`
    /// construction is the deformable curve, not the intersection: the
    /// intersection belongs to the construction the record embeds.
    #[test]
    fn a_nested_intcurve_construction_does_not_own_the_record() {
        for int_width in [RefWidth::Four, RefWidth::Eight] {
            let mut bytes = Vec::new();
            open(&mut bytes, b"defm_int_cur");
            bytes.push(0x04);
            bytes.extend_from_slice(&vec![0u8; int_width.bytes()]);
            open(&mut bytes, b"int_int_cur");
            bytes.push(0x10);
            bytes.push(0x10);

            assert_eq!(
                find_owned_intcurve_subtype(&bytes, b"defm_int_cur", int_width),
                Some((0, b"defm_int_cur".len()))
            );
            assert_eq!(
                find_owned_intcurve_subtype(&bytes, b"int_int_cur", int_width),
                None
            );
            assert_eq!(
                crate::nurbs::toks::owned_construction_subtype(&crate::nurbs::toks::lex_test_span(
                    &bytes, int_width
                ))
                .as_deref(),
                Some("defm_int_cur")
            );
        }
    }
}
