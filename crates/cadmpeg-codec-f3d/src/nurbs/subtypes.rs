// SPDX-License-Identifier: Apache-2.0
//! Subtype reference tables, intcurve subtype classification, and token walkers.

use crate::nurbs::reader::INT_WIDTHS;
use crate::sab::Record;
use cadmpeg_ir::wire::le::int_at as read_int;

pub(crate) fn first_construction_subtype(bytes: &[u8]) -> Option<String> {
    for pos in 0..bytes.len().saturating_sub(3) {
        if bytes[pos] != 0x0f || !matches!(bytes.get(pos + 1), Some(0x0d | 0x0e)) {
            continue;
        }
        let len = usize::from(*bytes.get(pos + 2)?);
        let name = bytes.get(pos + 3..pos + 3 + len)?;
        if name != b"ref" {
            return Some(canonical_intcurve_kind(name).into());
        }
    }
    None
}

fn canonical_intcurve_kind(name: &[u8]) -> &str {
    match name {
        b"bldcur" => "blend_int_cur",
        b"blndsprngcur" => "spring_int_cur",
        b"exactcur" => "exact_int_cur",
        b"lawintcur" => "law_int_cur",
        b"offintcur" => "off_int_cur",
        b"offsetintcur" => "offset_int_cur",
        b"offsurfintcur" => "off_surf_int_cur",
        b"parasil" => "para_silh_int_cur",
        b"parcur" => "par_int_cur",
        b"projcur" => "proj_int_cur",
        b"surfcur" => "surf_int_cur",
        b"surfintcur" => "int_int_cur",
        b"d5c2_cur" => "skin_int_cur",
        b"subsetintcur" => "subset_int_cur",
        _ => std::str::from_utf8(name).unwrap_or("intcurve"),
    }
}

/// Byte offset of the first subtype-definition opening whose name matches one of
/// `names`, together with the matched name. A definition opens as `0x0f`, a
/// `0x0d`/`0x0e` name token, the name length, then the name bytes. Names are
/// tried in order; the first name with a hit wins.
pub(crate) fn find_subtype_marker<'n>(
    bytes: &[u8],
    names: &[&'n [u8]],
) -> Option<(usize, &'n [u8])> {
    names.iter().copied().find_map(|name| {
        bytes
            .windows(name.len() + 3)
            .position(|window| {
                window[0] == 0x0f
                    && matches!(window[1], 0x0d | 0x0e)
                    && usize::from(window[2]) == name.len()
                    && &window[3..] == name
            })
            .map(|start| (start, name))
    })
}

pub(crate) fn find_intcurve_subtype(bytes: &[u8], modern: &[u8]) -> Option<(usize, usize)> {
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
    find_subtype_marker(bytes, &candidates).map(|(marker, name)| (marker, name.len()))
}

pub(crate) fn decode_cache_resolving_refs<T>(
    bytes: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
    seen: &mut Vec<usize>,
    decode_inline: fn(&[u8], usize) -> Option<T>,
    int_width: usize,
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

    pub(crate) fn for_width(&self, int_width: usize) -> &[usize] {
        INT_WIDTHS
            .iter()
            .position(|&width| width == int_width)
            .map_or(&[], |slot| self.tables[slot].as_slice())
    }

    /// Return the table index assigned to an absolute subtype-definition offset.
    #[cfg(test)]
    pub(crate) fn index_of_offset(&self, int_width: usize, offset: usize) -> Option<usize> {
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
    int_width: usize,
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
pub(crate) fn subtype_refs(bytes: &[u8], int_width: usize) -> Vec<usize> {
    let mut refs = Vec::new();
    let marker = b"\x0f\x0d\x03ref\x04";
    let mut pos = 0usize;
    while pos < bytes.len() {
        if bytes[pos..].starts_with(marker) {
            if let Some(index) = read_int(bytes, pos + marker.len(), int_width) {
                if index >= 0 {
                    refs.push(index as usize);
                }
            }
        } else if bytes.get(pos) == Some(&0x0f)
            && bytes.get(pos + 1) == Some(&0x04)
            && bytes.get(pos + 2 + int_width) == Some(&0x10)
        {
            if let Some(index) = read_int(bytes, pos + 2, int_width) {
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

pub(crate) fn subtype_span(bytes: &[u8], start: usize, int_width: usize) -> Option<&[u8]> {
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

pub(crate) fn next_token(bytes: &[u8], pos: usize, int_width: usize) -> Option<usize> {
    let tag = *bytes.get(pos)?;
    let fixed = match tag {
        0x02 => 2,
        0x03 => 3,
        0x04 | 0x0c | 0x15 => 1 + int_width,
        0x06 | 0x17 => 9,
        0x05 => 5,
        0x0a | 0x0b | 0x0f | 0x10 | 0x11 => 1,
        0x13 | 0x14 => 25,
        0x16 => 17,
        0x07 | 0x0d | 0x0e => 2 + usize::from(*bytes.get(pos + 1)?),
        0x08 => {
            3 + usize::from(u16::from_le_bytes(
                bytes.get(pos + 1..pos + 3)?.try_into().ok()?,
            ))
        }
        0x09 | 0x12 => {
            5 + usize::try_from(u32::from_le_bytes(
                bytes.get(pos + 1..pos + 5)?.try_into().ok()?,
            ))
            .ok()?
        }
        _ => return None,
    };
    let next = pos.checked_add(fixed)?;
    (next <= bytes.len()).then_some(next)
}
