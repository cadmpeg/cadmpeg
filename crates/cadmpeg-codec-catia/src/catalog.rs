// SPDX-License-Identifier: Apache-2.0
//! Framed CATIA `7C02` UTF-8 string catalogs.

use cadmpeg_core::decode::View;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const PREFIX: [&str; 4] = ["CATCatalogManager", "catalogManager", "catalogLinks", ""];

/// One exact `7C02` string catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Catalog {
    /// Byte offset of the `7C02` marker.
    pub pos: usize,
    /// Total framed byte length.
    pub total_len: usize,
    /// Catalog entries in serialized order.
    pub entries: Vec<CatalogEntry>,
}

impl Catalog {
    pub fn declared_count(&self) -> u32 {
        u32::try_from(self.entries.len() + 1).unwrap_or(u32::MAX)
    }
}

/// One inclusive-length ASCII catalog entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatalogEntry {
    /// Zero-based serialized entry ordinal.
    pub ordinal: u32,
    /// Byte offset of the inclusive length field.
    pub pos: usize,
    /// Decoded UTF-8 value. Schema expressions can contain line feeds and
    /// non-ASCII unit symbols.
    pub value: String,
}

/// Parse every exact `7C02` catalog in a complete `CATPart` image.
#[must_use]
pub fn parse(bytes: &[u8]) -> Vec<Catalog> {
    let mut catalogs = Vec::<Catalog>::new();
    let mut enclosing_end = 0usize;
    for pos in memchr::memchr_iter(0x7c, bytes) {
        let Some(marker_tail) = pos.checked_add(1) else {
            continue;
        };
        if bytes.get(marker_tail) != Some(&0x02) {
            continue;
        }
        let declared_end = pos
            .checked_add(2)
            .and_then(|length_offset| View::u32_le_at(bytes, length_offset))
            .and_then(|length| usize::try_from(length).ok())
            .and_then(|length| pos.checked_add(length));
        if pos < enclosing_end && declared_end.is_some_and(|end| end <= enclosing_end) {
            continue;
        }
        let Some(catalog) = parse_candidate(bytes, pos) else {
            continue;
        };
        if let Some(catalog_end) = catalog.pos.checked_add(catalog.total_len) {
            enclosing_end = enclosing_end.max(catalog_end);
        }
        catalogs.push(catalog);
    }
    catalogs
}

fn parse_candidate(bytes: &[u8], pos: usize) -> Option<Catalog> {
    let total_len = usize::try_from(View::u32_le_at(bytes, pos + 2)?).ok()?;
    let end = pos.checked_add(total_len)?;
    if total_len < 8 || end > bytes.len() {
        return None;
    }
    let (declared_count, mut at) = count_atom(bytes, pos + 6)?;
    let entry_count = usize::try_from(declared_count.checked_sub(1)?).ok()?;
    if entry_count > end.checked_sub(at)? {
        return None;
    }
    let mut entries = Vec::with_capacity(entry_count);
    for ordinal in 0..entry_count {
        let (value_len, header_len) = match *bytes.get(at)? {
            0 => (
                usize::try_from(View::u32_le_at(bytes, at + 1)?).ok()?,
                5usize,
            ),
            len => (usize::from(len).checked_sub(1)?, 1usize),
        };
        let value_start = at.checked_add(header_len)?;
        let next = value_start.checked_add(value_len)?;
        if next > end {
            return None;
        }
        let raw = &bytes[value_start..next];
        entries.push(CatalogEntry {
            ordinal: ordinal as u32,
            pos: at,
            value: std::str::from_utf8(raw).ok()?.to_owned(),
        });
        at = next;
    }
    if at != end
        || entries
            .iter()
            .take(PREFIX.len())
            .map(|entry| entry.value.as_str())
            .ne(PREFIX)
    {
        return None;
    }
    Some(Catalog {
        pos,
        total_len,
        entries,
    })
}

fn count_atom(bytes: &[u8], pos: usize) -> Option<(u32, usize)> {
    let byte = *bytes.get(pos)?;
    match byte {
        0x80..=0xd0 => Some((u32::from(byte - 0x80), pos + 1)),
        0xd1..=0xe4 => Some((
            u32::from(byte - 0xd1) * 256 + u32::from(*bytes.get(pos + 1)?) + 1,
            pos + 2,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
