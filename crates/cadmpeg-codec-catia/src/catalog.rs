// SPDX-License-Identifier: Apache-2.0
//! Framed CATIA `7C02` string catalogs.

use cadmpeg_ir::le::u32_at as u32_le;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const PREFIX: [&str; 4] = ["CATCatalogManager", "catalogManager", "catalogLinks", ""];

/// One exact `7C02` string catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Catalog {
    /// Byte offset of the `7C02` marker.
    pub pos: usize,
    /// Total framed byte length.
    pub total_len: usize,
    /// Stored count, equal to the entry population plus one.
    pub declared_count: u32,
    /// Catalog entries in serialized order.
    pub entries: Vec<CatalogEntry>,
}

/// One inclusive-length ASCII catalog entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatalogEntry {
    /// Zero-based serialized entry ordinal.
    pub ordinal: u32,
    /// Byte offset of the inclusive length field.
    pub pos: usize,
    /// Decoded ASCII value.
    pub value: String,
}

/// Parse every exact `7C02` catalog in a complete `CATPart` image.
#[must_use]
pub fn parse(bytes: &[u8]) -> Vec<Catalog> {
    bytes
        .windows(2)
        .enumerate()
        .filter(|(_, marker)| *marker == [0x7c, 0x02])
        .filter_map(|(pos, _)| parse_candidate(bytes, pos))
        .collect()
}

fn parse_candidate(bytes: &[u8], pos: usize) -> Option<Catalog> {
    let total_len = usize::try_from(u32_le(bytes, pos + 2)?).ok()?;
    let end = pos.checked_add(total_len)?;
    if total_len < 8 || end > bytes.len() {
        return None;
    }
    let (declared_count, mut at) = count_atom(bytes, pos + 6)?;
    let entry_count = usize::try_from(declared_count.checked_sub(1)?).ok()?;
    let mut entries = Vec::with_capacity(entry_count);
    for ordinal in 0..entry_count {
        let framed_len = usize::from(*bytes.get(at)?);
        if framed_len == 0 {
            return None;
        }
        let value_start = at + 1;
        let next = at.checked_add(framed_len)?;
        if next > end {
            return None;
        }
        let raw = &bytes[value_start..next];
        if !raw.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
            return None;
        }
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
        declared_count,
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
