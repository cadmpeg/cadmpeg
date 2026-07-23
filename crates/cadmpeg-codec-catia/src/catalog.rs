// SPDX-License-Identifier: Apache-2.0
//! Framed CATIA `7C02` UTF-8 string catalogs.

use cadmpeg_ir::wire::le::u32_at as u32_le;
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
    /// Decoded UTF-8 value. Schema expressions can contain line feeds and
    /// non-ASCII unit symbols.
    pub value: String,
}

/// Parse every exact `7C02` catalog in a complete `CATPart` image.
#[must_use]
pub fn parse(bytes: &[u8]) -> Vec<Catalog> {
    let candidates = bytes
        .windows(2)
        .enumerate()
        .filter(|(_, marker)| *marker == [0x7c, 0x02])
        .filter_map(|(pos, _)| parse_candidate(bytes, pos))
        .collect::<Vec<_>>();
    let mut catalogs = Vec::<Catalog>::new();
    for catalog in candidates {
        let catalog_end = catalog.pos + catalog.total_len;
        if catalogs
            .iter()
            .any(|outer| outer.pos < catalog.pos && outer.pos + outer.total_len >= catalog_end)
        {
            continue;
        }
        catalogs.push(catalog);
    }
    catalogs
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
        let (value_len, header_len) = match *bytes.get(at)? {
            0 => (usize::try_from(u32_le(bytes, at + 1)?).ok()?, 5usize),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_accepts_utf8_and_expression_line_feeds() {
        let entries = [
            "CATCatalogManager",
            "catalogManager",
            "catalogLinks",
            "",
            "angle\n°",
        ];
        let mut body = vec![0x86];
        for entry in entries {
            body.push(u8::try_from(entry.len() + 1).expect("fixture entry fits in u8"));
            body.extend_from_slice(entry.as_bytes());
        }
        let total_len = 6 + body.len();
        let mut bytes = vec![0x7c, 0x02];
        bytes.extend_from_slice(
            &u32::try_from(total_len)
                .expect("fixture catalog length fits in u32")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&body);
        let catalogs = parse(&bytes);
        assert_eq!(catalogs.len(), 1);
        assert_eq!(catalogs[0].entries[4].value, "angle\n°");
    }

    #[test]
    fn catalog_accepts_zero_tagged_u32_entry_lengths() {
        let long = "x".repeat(300);
        let entries = ["CATCatalogManager", "catalogManager", "catalogLinks", ""];
        let mut body = vec![0x86];
        for entry in entries {
            body.push(u8::try_from(entry.len() + 1).expect("fixture entry fits in u8"));
            body.extend_from_slice(entry.as_bytes());
        }
        body.push(0);
        body.extend_from_slice(
            &u32::try_from(long.len())
                .expect("fixture entry length fits in u32")
                .to_le_bytes(),
        );
        body.extend_from_slice(long.as_bytes());
        let total_len = 6 + body.len();
        let mut bytes = vec![0x7c, 0x02];
        bytes.extend_from_slice(
            &u32::try_from(total_len)
                .expect("fixture catalog length fits in u32")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&body);
        let catalogs = parse(&bytes);
        assert_eq!(catalogs.len(), 1);
        assert_eq!(catalogs[0].entries[4].value, long);
    }

    #[test]
    fn catalog_owns_catalog_shaped_bytes_inside_an_entry() {
        let mut nested = vec![0x7c, 0x02, 0, 0, 0, 0, 0xd1, 0x80];
        for entry in PREFIX {
            nested.push(u8::try_from(entry.len() + 1).expect("fixture entry length"));
            nested.extend_from_slice(entry.as_bytes());
        }
        nested.extend(std::iter::repeat_n(1, 123));
        nested.push(79);
        nested.extend(std::iter::repeat_n(b'x', 78));
        assert_eq!(nested.len(), 257);
        nested[2..6].copy_from_slice(&257u32.to_le_bytes());
        assert!(std::str::from_utf8(&nested).is_ok());

        let mut outer = vec![0x7c, 0x02, 0, 0, 0, 0, 0x86];
        for entry in PREFIX {
            outer.push(u8::try_from(entry.len() + 1).expect("fixture entry length"));
            outer.extend_from_slice(entry.as_bytes());
        }
        outer.push(0);
        outer.extend_from_slice(&257u32.to_le_bytes());
        outer.extend_from_slice(&nested);
        let outer_len = u32::try_from(outer.len()).expect("fixture catalog length");
        outer[2..6].copy_from_slice(&outer_len.to_le_bytes());

        let catalogs = parse(&outer);
        assert_eq!(catalogs.len(), 1);
        assert_eq!(catalogs[0].pos, 0);
        assert_eq!(catalogs[0].entries.len(), 5);
    }
}
