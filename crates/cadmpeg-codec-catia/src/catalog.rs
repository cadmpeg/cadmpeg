// SPDX-License-Identifier: Apache-2.0
//! Framed CATIA `7C02` string catalogs.
#![deny(clippy::disallowed_methods)]

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::decode::{DecodeContext, View};
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
pub fn parse<'a>(ctx: &DecodeContext<'a>, view: View<'a>) -> Result<Vec<Catalog>, CodecError> {
    let bytes = view.window();
    let mut catalogs = Vec::new();
    for pos in 0..bytes.len().saturating_sub(1) {
        if bytes[pos..pos + 2] != [0x7c, 0x02] {
            continue;
        }
        if let Some(catalog) = parse_candidate(ctx, view, bytes, pos)? {
            catalogs.push(catalog);
        }
    }
    Ok(catalogs)
}

fn parse_candidate<'a>(
    ctx: &DecodeContext<'a>,
    view: View<'a>,
    bytes: &[u8],
    pos: usize,
) -> Result<Option<Catalog>, CodecError> {
    let Some(total_len) = u32_le(bytes, pos + 2).and_then(|len| usize::try_from(len).ok()) else {
        return Ok(None);
    };
    let Some(end) = pos.checked_add(total_len) else {
        return Ok(None);
    };
    if total_len < 8 || end > bytes.len() {
        return Ok(None);
    }
    // Charge the nested entry walk over this candidate frame. Without this an
    // input packed with many adjacent `7C02` positions, each with a large valid
    // `total_len`, would run a per-candidate entry walk whose aggregate CPU is
    // quadratic in the image while only the whole-image scan (`n` bytes) was
    let Some((declared_count, at)) = count_atom(bytes, pos + 6) else {
        return Ok(None);
    };
    let Some(entry_count) = declared_count.checked_sub(1).map(|c| c as usize) else {
        return Ok(None);
    };
    // Prove the declared entry count fits the remaining frame before reserving.
    // Each entry consumes at least its one-byte inclusive-length field, so the
    // minimum element size is one byte. `at`/`end` are window-relative indices
    // into `bytes`, so they are rebased onto the view's absolute space (correct
    // even when the view is a non-root child, `view.start() > 0`).
    let Some(region) = view.child(view.start() + at, view.start() + end) else {
        return Ok(None);
    };
    let Some(bounded) = region.counted(entry_count as u64, 1) else {
        return Ok(None);
    };
    let mut entries = ctx.exact_vec::<CatalogEntry>(bounded)?;
    let mut at = at;
    for ordinal in 0..entry_count {
        let framed_len = usize::from(*bytes.get(at).unwrap_or(&0));
        if framed_len == 0 {
            return Ok(None);
        }
        let value_start = at + 1;
        let Some(next) = at.checked_add(framed_len) else {
            return Ok(None);
        };
        if next > end {
            return Ok(None);
        }
        let raw = &bytes[value_start..next];
        if !raw.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
            return Ok(None);
        }
        let Ok(value) = std::str::from_utf8(raw) else {
            return Ok(None);
        };
        entries.push(CatalogEntry {
            ordinal: ordinal as u32,
            pos: at,
            value: value.to_owned(),
        })?;
        at = next;
    }
    let entries = entries.finish();
    if at != end
        || entries
            .iter()
            .take(PREFIX.len())
            .map(|entry| entry.value.as_str())
            .ne(PREFIX)
    {
        return Ok(None);
    }
    Ok(Some(Catalog {
        pos,
        total_len,
        declared_count,
        entries,
    }))
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
