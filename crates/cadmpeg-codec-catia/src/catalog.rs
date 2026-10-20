// SPDX-License-Identifier: Apache-2.0
//! Framed CATIA `7C02` UTF-8 string catalogs.

use cadmpeg_core::decode::View;

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

/// Entry bytes a catalog scan may walk per input byte, across every candidate.
///
/// Catalogs in a real image do not overlap, so the scan walks each entry byte
/// about once. Candidates that overlap an accepted catalog or fail late walk
/// the same bytes again; a candidate that would pass the allowance is skipped.
pub(crate) const MAX_CATALOG_WALK_BYTES_PER_INPUT_BYTE: usize = 4;

/// The catalogs of one image and the candidates the walk allowance skipped.
#[derive(Debug, Default)]
pub(crate) struct CatalogScan {
    /// Exact catalogs in source order.
    pub(crate) catalogs: Vec<Catalog>,
    /// `7C02` candidates skipped at `MAX_CATALOG_WALK_BYTES_PER_INPUT_BYTE`.
    pub(crate) bounded_candidates: usize,
}

/// Parse every exact `7C02` catalog in a complete `CATPart` image.
pub(crate) fn parse(bytes: &[u8]) -> CatalogScan {
    let mut scan = CatalogScan::default();
    let mut walk_allowance = bytes
        .len()
        .checked_mul(MAX_CATALOG_WALK_BYTES_PER_INPUT_BYTE)
        .unwrap_or(bytes.len());
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
        let catalog = match parse_candidate(bytes, pos, &mut walk_allowance) {
            Candidate::Catalog(catalog) => catalog,
            Candidate::Rejected => continue,
            Candidate::Bounded => {
                scan.bounded_candidates += 1;
                continue;
            }
        };
        if let Some(catalog_end) = catalog.pos.checked_add(catalog.total_len) {
            enclosing_end = enclosing_end.max(catalog_end);
        }
        scan.catalogs.push(catalog);
    }
    scan
}

enum Candidate {
    Catalog(Catalog),
    Rejected,
    /// The candidate's entries pass the remaining walk allowance.
    Bounded,
}

/// Parse one `7C02` candidate, charging its walked entry bytes to `allowance`.
///
/// The prefix entries are compared as they are read, and entry text is copied
/// only once the whole frame is exact.
fn parse_candidate(bytes: &[u8], pos: usize, allowance: &mut usize) -> Candidate {
    let Some(frame) = candidate_frame(bytes, pos) else {
        return Candidate::Rejected;
    };
    let CandidateFrame {
        total_len,
        end,
        entry_count,
        mut at,
    } = frame;
    let mut entries = Vec::new();
    for ordinal in 0..entry_count {
        let Some((value_len, header_len)) = entry_header(bytes, at) else {
            return Candidate::Rejected;
        };
        let Some(next) = at
            .checked_add(header_len)
            .and_then(|value_start| value_start.checked_add(value_len))
            .filter(|next| *next <= end)
        else {
            return Candidate::Rejected;
        };
        let Some(remaining) = allowance.checked_sub(next - at) else {
            return Candidate::Bounded;
        };
        *allowance = remaining;
        let raw = &bytes[at + header_len..next];
        if PREFIX
            .get(ordinal)
            .is_some_and(|prefix| raw != prefix.as_bytes())
        {
            return Candidate::Rejected;
        }
        let (Ok(value), Ok(ordinal)) = (std::str::from_utf8(raw), u32::try_from(ordinal)) else {
            return Candidate::Rejected;
        };
        entries.push((ordinal, at, value));
        at = next;
    }
    if at != end || entries.len() < PREFIX.len() {
        return Candidate::Rejected;
    }
    Candidate::Catalog(Catalog {
        pos,
        total_len,
        entries: entries
            .into_iter()
            .map(|(ordinal, pos, value)| CatalogEntry {
                ordinal,
                pos,
                value: value.to_owned(),
            })
            .collect(),
    })
}

struct CandidateFrame {
    total_len: usize,
    end: usize,
    entry_count: usize,
    at: usize,
}

fn candidate_frame(bytes: &[u8], pos: usize) -> Option<CandidateFrame> {
    let total_len = usize::try_from(View::u32_le_at(bytes, pos + 2)?).ok()?;
    let end = pos.checked_add(total_len)?;
    if total_len < 8 || end > bytes.len() {
        return None;
    }
    let (declared_count, at) = compact_atom(bytes, pos + 6)?;
    let entry_count = usize::try_from(declared_count.checked_sub(1)?).ok()?;
    if entry_count > end.checked_sub(at)? {
        return None;
    }
    Some(CandidateFrame {
        total_len,
        end,
        entry_count,
        at,
    })
}

/// Value length and header length of the inclusive-length entry at `at`.
fn entry_header(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    match *bytes.get(at)? {
        0 => Some((usize::try_from(View::u32_le_at(bytes, at + 1)?).ok()?, 5)),
        len => Some((usize::from(len).checked_sub(1)?, 1)),
    }
}

#[cfg(test)]
mod tests;
