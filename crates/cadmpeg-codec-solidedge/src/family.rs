// SPDX-License-Identifier: Apache-2.0
//! Family-of-assemblies documents (`docs/formats/solidedge.md` §1.1): the
//! active member, the occurrences the member table lists for it and the
//! occurrences it excludes.

use std::collections::BTreeSet;

use cadmpeg_core::decode::View;

/// The stream of the master storage that holds the member table.
pub(crate) const MEMBERS_PATH: &str = "Master\u{1}/Family of Assemblies Members";

/// Tag of a member record, followed by the `u32` member identifier.
const RECORD_TAG: [u8; 4] = *b"QQQ\0";
/// Identifiers of members are `0xa000` plus a nonzero ordinal.
const MEMBER_ID_BASE: u32 = 0xa000;
/// Bytes of one entry of the list that precedes the occurrence list, and the
/// bytes that follow that list.
const PREFIX_ENTRY_LEN: usize = 4;
const PREFIX_TAIL_LEN: usize = 12;
/// Bytes of one block before the occurrence count: a `u32` `1`, a 16-byte
/// class identifier and a `u32` `0`.
const BLOCK_LEN: usize = 24;
/// Bit of the last word of the first record that is set while the first
/// member is the active member.
const FIRST_ACTIVE_BIT: u32 = 2;
/// Bytes of one occurrence entry: the `u32` object identifier, the 13 `f64`
/// words of the placement (§9.2) and three bytes.
const ENTRY_LEN: usize = 4 + 13 * 8 + 3;

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    View::u32_le_at(bytes, at)
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

/// One member record of the member table.
struct Record {
    name: String,
    /// The identifiers of the first list: the occurrences the member excludes.
    excluded: BTreeSet<u32>,
    /// The identifiers of the second list: the occurrences of the family.
    listed: BTreeSet<u32>,
    /// The last word of the record.
    word: u32,
}

/// The occurrences of the family and the exclusions of the active member.
pub(crate) struct Selection {
    /// The occurrence object identifiers the active member's record lists.
    pub(crate) listed: BTreeSet<u32>,
    /// The occurrence object identifiers the active member excludes.
    pub(crate) excluded: BTreeSet<u32>,
    /// Whether the table states that a member other than the named one is
    /// active and does not state which.
    pub(crate) unresolved: bool,
}

/// `count` entries of `u32` words from `at`, when they lie in `record`.
fn words_at(record: &[u8], at: usize, count: usize) -> Option<BTreeSet<u32>> {
    (count <= record.len().checked_sub(at)? / 4).then_some(())?;
    (0..count)
        .map(|entry| u32_at(record, at.checked_add(entry * 4)?))
        .collect()
}

/// The record at `start` of `table`, which ends at `end`. After the name the
/// record holds a `u32` count `c`, `c` entries of `PREFIX_ENTRY_LEN` bytes
/// and `PREFIX_TAIL_LEN` bytes, two blocks of `BLOCK_LEN` bytes, a `u32`
/// count `n`, `n` entries of `ENTRY_LEN` bytes that start with the `u32`
/// object identifier, and further bytes that end with a `u32` word.
fn record_at(table: &[u8], start: usize, end: usize) -> Option<Record> {
    let (name, name_len) = name_at(table, start + 8)?;
    let record = table.get(start..end)?;
    let after_name = 8 + name_len;
    let prefix = usize::try_from(u32_at(record, after_name)?).ok()?;
    let first_at = after_name.checked_add(4)?;
    let excluded = words_at(record, first_at, prefix)?;
    let blocks = first_at
        .checked_add(prefix.checked_mul(PREFIX_ENTRY_LEN)?)?
        .checked_add(PREFIX_TAIL_LEN)?;
    for block in 0..2 {
        let at = blocks.checked_add(block * BLOCK_LEN)?;
        (u32_at(record, at)? == 1 && u32_at(record, at.checked_add(BLOCK_LEN - 4)?)? == 0)
            .then_some(())?;
    }
    let count_at = blocks.checked_add(2 * BLOCK_LEN)?;
    let count = usize::try_from(u32_at(record, count_at)?).ok()?;
    let first = count_at.checked_add(4)?;
    let room = record.len().checked_sub(first)? / ENTRY_LEN;
    (count <= room).then_some(())?;
    let listed = (0..count)
        .map(|entry| u32_at(record, first.checked_add(entry * ENTRY_LEN)?))
        .collect::<Option<BTreeSet<u32>>>()?;
    let after_list = first.checked_add(count * ENTRY_LEN)?;
    let word = u32_at(record, record.len().checked_sub(4)?)?;
    (record.len() >= after_list.checked_add(4)?).then_some(())?;
    Some(Record {
        name,
        excluded,
        listed,
        word,
    })
}

/// The active member's occurrences and exclusions. `named` is the member the
/// `FamilyInfo` stream names. When the last word of the first record lacks
/// `FIRST_ACTIVE_BIT` and the table holds exactly two readable records, the
/// second record is the active member's; with more records the active member is not
/// determined and the named member's record is used.
pub(crate) fn select(table: &[u8], named: &str) -> Option<Selection> {
    let starts = record_starts(table);
    let records: Vec<Option<Record>> = starts
        .iter()
        .enumerate()
        .map(|(index, &start)| {
            let end = starts.get(index + 1).copied().unwrap_or(table.len());
            record_at(table, start, end)
        })
        .collect();
    let named_index = records.iter().position(|record| {
        record
            .as_ref()
            .is_some_and(|record| record.name.eq_ignore_ascii_case(named))
    })?;
    let mut active = named_index;
    let mut unresolved = false;
    let first_inactive = named_index == 0
        && records[0]
            .as_ref()
            .is_some_and(|record| record.word & FIRST_ACTIVE_BIT == 0);
    if first_inactive {
        if records.len() == 2 && records[1].is_some() {
            active = 1;
        } else {
            unresolved = true;
        }
    }
    let record = records.into_iter().nth(active)??;
    Some(Selection {
        listed: record.listed,
        excluded: record.excluded,
        unresolved,
    })
}

#[cfg(test)]
mod tests;
