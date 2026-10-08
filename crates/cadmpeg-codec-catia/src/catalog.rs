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
    let mut catalogs = Vec::<Catalog>::new();
    let mut enclosing_end = 0usize;
    let mut markers = ctx.find_bytes_iter(bytes, &[0x7c], "catia_catalog_scan")?;
    while let Some(pos) = ctx.next_charged(&mut markers, "catia_catalog_candidate")? {
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
    let mut storage = ctx.reserve_scoped(0, "catia_catalog_candidate_storage")?;
    let parsed = storage.with_storage(|| {
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
            let (declared_count, mut at) = compact_atom(bytes, pos + 6)?;
            let entry_count = usize::try_from(declared_count.checked_sub(1)?).ok()?;
            if entry_count > end.checked_sub(at)? {
                return None;
            }
            let mut entries = Vec::new();
            let mut ordinals = 0..entry_count;
            while let Some(ordinal) =
                admitted!(ctx.next_charged(&mut ordinals, "catia_catalog_entry_visits"))
            {
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
                let value = admitted!(ctx.copy_retained_text(
                    admitted!(ctx.validate_utf8(raw, "catia_catalog_entry_utf8")).ok()?,
                    "catia_catalog_entry_value"
                ));
                admitted!(ctx.push_vec(
                    &mut entries,
                    CatalogEntry {
                        ordinal: u32::try_from(ordinal).ok()?,
                        pos: at,
                        value,
                    },
                    "catia_catalog_entries"
                ));
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
    })?;
    if parsed.is_some() {
        storage.commit()?;
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests;
