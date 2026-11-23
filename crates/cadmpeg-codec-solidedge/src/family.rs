// SPDX-License-Identifier: Apache-2.0
//! Family-of-assemblies documents (`docs/formats/solidedge.md` §1.1): the
//! active member and the occurrences the member table lists for it.

use std::collections::BTreeSet;

use cadmpeg_core::decode::View;

/// The stream of the master storage that holds the member table.
pub(crate) const MEMBERS_PATH: &str = "Master\u{1}/Family of Assemblies Members";

/// Tag of a member record, followed by the `u32` member identifier.
const RECORD_TAG: [u8; 4] = *b"QQQ\0";
/// Identifiers of members are `0xa000` plus a nonzero ordinal.
const MEMBER_ID_BASE: u32 = 0xa000;
/// The `u64` that starts every entry of a member's occurrence list.
const ENTRY_WORD: u64 = 0x4000_0000;
/// Bytes of one entry: the `u64` word and the `u32` object identifier.
const ENTRY_LEN: usize = 12;

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    View::u32_le_at(bytes, at)
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    View::u64_le_at(bytes, at)
}

/// A name stored as a `u32` count of UTF-16LE units that includes the
/// terminating zero unit, then the units. Returns the name and the byte length
/// it occupies.
fn name_at(bytes: &[u8], at: usize) -> Option<(String, usize)> {
    let count = usize::try_from(u32_at(bytes, at)?).ok()?;
    let start = at.checked_add(4)?;
    let mut units = (0..count)
        .map(|unit| View::u16_le_at(bytes, start.checked_add(unit.checked_mul(2)?)?))
        .collect::<Option<Vec<u16>>>()?;
    if units.pop() != Some(0) || units.is_empty() || units.contains(&0) {
        return None;
    }
    Some((String::from_utf16(&units).ok()?, 4 + count * 2))
}

/// The name of the active member, from the `FamilyInfo` stream.
pub(crate) fn active_name(bytes: &[u8]) -> Option<String> {
    name_at(bytes, 0).map(|(name, _)| name)
}

/// The record tags of the member table: byte offsets of a record tag followed
/// by a member identifier.
fn record_starts(table: &[u8]) -> Vec<usize> {
    (0..table.len().checked_sub(8).map_or(0, |last| last + 1))
        .filter(|&at| {
            table[at..at + 4] == RECORD_TAG
                && u32_at(table, at + 4)
                    .is_some_and(|id| id > MEMBER_ID_BASE && id & 0xffff_f000 == MEMBER_ID_BASE)
        })
        .collect()
}

/// The occurrence object identifiers the member table lists for the member
/// `name`. The record ends with a `u32` count `n`, `n` entries and a `u32`
/// trailer word.
pub(crate) fn listed_occurrences(table: &[u8], name: &str) -> Option<BTreeSet<u32>> {
    let starts = record_starts(table);
    for (index, &start) in starts.iter().enumerate() {
        let Some((found, _)) = name_at(table, start + 8) else {
            continue;
        };
        if !found.eq_ignore_ascii_case(name) {
            continue;
        }
        let end = starts.get(index + 1).copied().unwrap_or(table.len());
        let record = table.get(start..end)?;
        let body = record.len().checked_sub(4)?;
        let most = body.checked_sub(4)? / ENTRY_LEN;
        return (0..=most).rev().find_map(|count| {
            let first = body.checked_sub(count * ENTRY_LEN)?;
            let declared = u32_at(record, first.checked_sub(4)?)?;
            (usize::try_from(declared) == Ok(count)).then_some(())?;
            (0..count)
                .map(|entry| {
                    let at = first + entry * ENTRY_LEN;
                    (u64_at(record, at)? == ENTRY_WORD).then_some(())?;
                    u32_at(record, at + 8)
                })
                .collect::<Option<BTreeSet<u32>>>()
        });
    }
    None
}

#[cfg(test)]
mod tests;
