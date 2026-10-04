// SPDX-License-Identifier: Apache-2.0
//! Framed CATIA `7C02` UTF-8 string catalogs.

use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::wire::tokens::compact_atom;

const PREFIX: [&str; 4] = ["CATCatalogManager", "catalogManager", "catalogLinks", ""];

/// One exact `7C02` string catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Catalog {
    /// Byte offset of the `7C02` marker.
    pub(crate) pos: usize,
    /// Total framed byte length.
    pub(crate) total_len: usize,
    /// Catalog entries in serialized order.
    pub(crate) entries: Vec<CatalogEntry>,
}

/// One inclusive-length ASCII catalog entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatalogEntry {
    /// Zero-based serialized entry ordinal.
    pub(crate) ordinal: u32,
    /// Byte offset of the inclusive length field.
    pub(crate) pos: usize,
    /// Decoded UTF-8 value. Schema expressions can contain line feeds and
    /// non-ASCII unit symbols.
    pub(crate) value: String,
}

impl DecodeCost for CatalogEntry {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        (&self.ordinal, &self.pos, &self.value).decode_cost(ctx, operation)
    }
}

impl DecodeCost for Catalog {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        (&self.pos, &self.total_len, &self.entries).decode_cost(ctx, operation)
    }
}

/// Parse every exact `7C02` catalog in a complete `CATPart` image.
pub(crate) fn parse(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<Catalog>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "catia_catalog_scan",
    )?;
    let mut catalogs = Vec::<Catalog>::new();
    let mut enclosing_end = 0usize;
    for pos in memchr::memchr_iter(0x7c, bytes) {
        ctx.charge_work(1, "catia_catalog_candidate")?;
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
        let Some(catalog) = parse_candidate(ctx, bytes, pos)? else {
            continue;
        };
        if let Some(catalog_end) = catalog.pos.checked_add(catalog.total_len) {
            enclosing_end = enclosing_end.max(catalog_end);
        }
        ctx.push_vec(&mut catalogs, catalog, "catia_catalogs")?;
    }
    Ok(catalogs)
}

fn parse_candidate(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    pos: usize,
) -> Result<Option<Catalog>, CodecError> {
    (|| -> Option<Result<Catalog, CodecError>> {
        macro_rules! admitted {
            ($result:expr) => {
                match $result {
                    Ok(value) => value,
                    Err(error) => return Some(Err(error)),
                }
            };
        }
        let total_len = usize::try_from(View::u32_le_at(bytes, pos + 2)?).ok()?;
        let end = pos.checked_add(total_len)?;
        if total_len < 8 || end > bytes.len() {
            return None;
        }
        let work = admitted!(cadmpeg_core::decode::u64_from_index(total_len)
            .checked_mul(2)
            .ok_or_else(|| ctx.refuse_codec_limit("catia_catalog_candidate", u64::MAX, u64::MAX)));
        admitted!(ctx.charge_work(work, "catia_catalog_candidate"));
        let (declared_count, mut at) = compact_atom(bytes, pos + 6)?;
        let entry_count = usize::try_from(declared_count.checked_sub(1)?).ok()?;
        if entry_count > end.checked_sub(at)? {
            return None;
        }
        let mut entries = Vec::new();
        admitted!(ctx.reserve_vec(&mut entries, entry_count, "catia_catalog_entries"));
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
            let value = admitted!(
                ctx.copy_retained_text(std::str::from_utf8(raw).ok()?, "catia_catalog_entry_value")
            );
            entries.push(CatalogEntry {
                ordinal: u32::try_from(ordinal).ok()?,
                pos: at,
                value,
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
        Some(Ok(Catalog {
            pos,
            total_len,
            entries,
        }))
    })()
    .transpose()
}

#[cfg(test)]
mod tests;
