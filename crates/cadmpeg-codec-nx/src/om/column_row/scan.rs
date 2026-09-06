// SPDX-License-Identifier: Apache-2.0
//! Complete source-frame admission for compact-index column rows.

use super::{IndexRow, LinkedRow, TargetRow};
use crate::om::compact::LocatedCompactIndex;
use crate::om::{discriminators, color::PaletteIndex};

/// Decode complete self-framed index rows from contiguous column storage.
pub(crate) fn index_rows(bytes: &[u8]) -> Vec<IndexRow> {
    use super::{INDEX_PREFIX as PREFIX, INDEX_MIDDLE as MIDDLE, INDEX_SUFFIX as SUFFIX};
    let mut rows = Vec::new();
    let mut start = 0;
    while start + PREFIX.len() <= bytes.len() {
        if bytes.get(start..start + PREFIX.len()) != Some(&PREFIX) {
            start += 1;
            continue;
        }
        let first_index_offset = start + PREFIX.len();
        let Some(first_token) = LocatedCompactIndex::read(bytes, first_index_offset) else {
            start += 1;
            continue;
        };
        let marker = first_token.offset + first_token.atom.raw().len();
        if bytes.get(marker..marker + 2) != Some(&MIDDLE[..2]) {
            start += 1;
            continue;
        }
        let Some(flag) = bytes.get(marker + 2).copied().and_then(|value| discriminators::LinkedIndexFlag::try_from(value).ok()) else {
            start += 1;
            continue;
        };
        let mut at = marker + 3;
        let Some(index_tokens) = LocatedCompactIndex::read_array(bytes, &mut at) else {
            start += 1;
            continue;
        };
        let Some(end) = at.checked_add(SUFFIX.len()) else {
            start += 1;
            continue;
        };
        if bytes.get(at..end) != Some(&SUFFIX) {
            start += 1;
            continue;
        }
        if let Some(row) = IndexRow::<(), usize>::new(first_token.atom, flag, index_tokens.map(|token| token.atom.into()), start) { rows.push(row); }
        start = end;
    }
    rows
}

/// Decode complete linked index rows from contiguous column storage.
pub(crate) fn linked_rows(bytes: &[u8]) -> Vec<LinkedRow> {
    use super::{TARGET_MIDDLE as MIDDLE, ROW_SUFFIX as SUFFIX};
    let mut rows = Vec::new();
    let mut start = 0;
    while start + 2 <= bytes.len() {
        if bytes.get(start..start + 2) != Some(&super::LINKED_PREFIX) {
            start += 1;
            continue;
        }
        let first_offset = start + 2;
        let Some(first_token) = LocatedCompactIndex::read(bytes, first_offset) else {
            start += 1;
            continue;
        };
        let marker = first_token.offset + first_token.atom.raw().len();
        if bytes.get(marker..marker + 2) != Some(&super::LINKED_MIDDLE) {
            start += 1;
            continue;
        }
        let Some(discriminator) = bytes
            .get(marker + 2)
            .copied()
            .and_then(|value| discriminators::LinkedIndexDiscriminator::try_from(value).ok())
        else {
            start += 1;
            continue;
        };
        let target_offset = marker + 3;
        let Some(target_token) = LocatedCompactIndex::read(bytes, target_offset) else {
            start += 1;
            continue;
        };
        let mut at = target_token.offset + target_token.atom.raw().len();
        if bytes.get(at..at + MIDDLE.len()) != Some(&MIDDLE) {
            start += 1;
            continue;
        }
        at += MIDDLE.len();
        let Some(index_tokens) = LocatedCompactIndex::read_array(bytes, &mut at) else {
            start += 1;
            continue;
        };
        if bytes.get(at..at + 2) != Some(&[0x00, 0x47]) {
            start += 1;
            continue;
        }
        let Some(flag) = bytes
            .get(at + 2)
            .copied()
            .and_then(|value| discriminators::LinkedIndexFlag::try_from(value).ok())
        else {
            start += 1;
            continue;
        };
        let Some(mode) = bytes
            .get(at + 3)
            .copied()
            .and_then(|value| discriminators::IndexRowMode::try_from(value).ok())
        else {
            start += 1;
            continue;
        };
        let Some(end) = at.checked_add(4 + SUFFIX.len()) else {
            start += 1;
            continue;
        };
        if bytes.get(at + 4..end) != Some(&SUFFIX) {
            start += 1;
            continue;
        }
        if let Some(row) = LinkedRow::<(), usize>::new(first_token.atom, discriminator, target_token.atom.into(), index_tokens.map(|token| token.atom.into()), flag, mode, start) { rows.push(row); }
        start = end;
    }
    rows
}

/// Decode complete target-index rows from contiguous column storage.
pub(crate) fn target_rows(bytes: &[u8]) -> Vec<TargetRow> {
    use super::TARGET_PREFIX as PREFIX;
    use super::{TARGET_MIDDLE as MIDDLE, ROW_SUFFIX as SUFFIX};
    let mut rows = Vec::new();
    let mut start = 0;
    while start + PREFIX.len() <= bytes.len() {
        if bytes.get(start..start + PREFIX.len()) != Some(&PREFIX) {
            start += 1;
            continue;
        }
        let target_offset = start + PREFIX.len();
        let Some(target_token) = LocatedCompactIndex::read(bytes, target_offset) else {
            start += 1;
            continue;
        };
        let mut at = target_token.offset + target_token.atom.raw().len();
        if bytes.get(at..at + MIDDLE.len()) != Some(&MIDDLE) {
            start += 1;
            continue;
        }
        at += MIDDLE.len();
        let Some(index_tokens) = LocatedCompactIndex::read_array(bytes, &mut at) else {
            start += 1;
            continue;
        };
        if bytes.get(at..at + 3) != Some(&[0x00, 0x47, 0x03]) {
            start += 1;
            continue;
        }
        let Some(mode) = bytes
            .get(at + 3)
            .copied()
            .and_then(|value| discriminators::IndexRowMode::try_from(value).ok())
        else {
            start += 1;
            continue;
        };
        let Some(end) = at.checked_add(4 + SUFFIX.len()) else {
            start += 1;
            continue;
        };
        if bytes.get(at + 4..end) != Some(&SUFFIX) {
            start += 1;
            continue;
        }
        if let Some(row) = TargetRow::<(), usize>::new(target_token.atom.into(), index_tokens.map(|token| token.atom.into()), mode, start) { rows.push(row); }
        start = end;
    }
    rows
}

pub(crate) fn preceding_color(bytes: &[u8], row_offset: usize) -> Option<PaletteIndex> {
    use super::ROW_SUFFIX as PRECEDING_SUFFIX;
    for width in [1, 2] {
        let Some(offset) = row_offset.checked_sub(width) else { continue; };
        let Some(prefix_offset) = offset.checked_sub(PRECEDING_SUFFIX.len()) else { continue; };
        if bytes.get(prefix_offset..offset) != Some(&PRECEDING_SUFFIX) { continue; }
        if let Some(color_index) = PaletteIndex::read_display(bytes.get(offset..row_offset)?) {
            return Some(color_index);
        }
    }
    None
}

