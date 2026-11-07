// SPDX-License-Identifier: Apache-2.0
//! ASM `delta_state` history: `BulletinBoard` chain framing and historical
//! `RecordTable` materialization (`docs/formats/asm.md` section 3.2).

use crate::kernel_header::RefWidth;
use crate::sab::{Record, Token};
use cadmpeg_core::decode::View;
use std::collections::BTreeMap;

/// Classified entity change of a `BulletinBoard` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityChange {
    /// Null to entity.
    Insert {
        /// Reference of the inserted entity.
        new: i64,
    },
    /// Entity to null.
    Delete {
        /// Reference of the deleted entity.
        old: i64,
    },
    /// Entity to entity.
    Update {
        /// Reference of the previous revision.
        old: i64,
        /// Reference of the new revision.
        new: i64,
    },
}

/// One framed item of a `BulletinBoard` chain, in stream order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulletinBoardItem {
    /// A board header.
    Board {
        /// Byte offset of the board header.
        offset: usize,
        /// Owner reference.
        owner_ref: i64,
        /// Board number.
        number: i64,
    },
    /// A change of the most recent board.
    Change {
        /// Byte offset of the change.
        offset: usize,
        /// Classified change.
        kind: EntityChange,
    },
}

/// Read a tagged little-endian signed integer of the stream's ref width (4 or
/// 8 bytes) and advance past it.
fn take_int(bytes: &[u8], position: &mut usize, tag: u8, width: RefWidth) -> Option<i64> {
    if bytes.get(*position) != Some(&tag) {
        return None;
    }
    let at = *position + 1;
    let value = match width {
        RefWidth::Four => i64::from(View::i32_le_at(bytes, at)?),
        RefWidth::Eight => View::i64_le_at(bytes, at)?,
    };
    *position += 1 + width.bytes();
    Some(value)
}

/// Walk the `BulletinBoard` chain at `position` and return the offset after
/// it, or `None` when the chain is malformed.
pub fn walk_bulletin_boards(
    bytes: &[u8],
    mut position: usize,
    width: RefWidth,
    mut visit: impl FnMut(BulletinBoardItem),
) -> Option<usize> {
    if bytes.get(position) == Some(&0x11) {
        return Some(position);
    }
    loop {
        let board_offset = position;
        let present = take_int(bytes, &mut position, 0x04, width)?;
        if present == 0 {
            break;
        }
        let owner_ref = take_int(bytes, &mut position, 0x0c, width)?;
        let number = take_int(bytes, &mut position, 0x04, width)?;
        visit(BulletinBoardItem::Board {
            offset: board_offset,
            owner_ref,
            number,
        });
        loop {
            let change_offset = position;
            let present = take_int(bytes, &mut position, 0x04, width)?;
            if present == 0 {
                break;
            }
            let old = take_int(bytes, &mut position, 0x0c, width)?;
            let new = take_int(bytes, &mut position, 0x0c, width)?;
            let kind = match (old >= 0, new >= 0) {
                (false, true) => EntityChange::Insert { new },
                (true, false) => EntityChange::Delete { old },
                (true, true) => EntityChange::Update { old, new },
                (false, false) => return None,
            };
            visit(BulletinBoardItem::Change {
                offset: change_offset,
                kind,
            });
        }
    }
    Some(position)
}

/// Materialize the `RecordTable` of one historical state.
///
/// `versions` maps each live entity slot to its record revision; `lookup`
/// returns the record of a revision. The result is ordered by record index.
/// It is `None` when `versions` is empty, a revision has no record, a
/// record's index differs from its slot, or a non-negative reference leaves
/// the slot set.
pub fn materialize_record_table<'a>(
    versions: &BTreeMap<i64, i64>,
    lookup: impl Fn(i64) -> Option<&'a Record>,
) -> Option<Vec<Record>> {
    if versions.is_empty() {
        return None;
    }
    let mut records = Vec::new();
    for (&entity_ref, &record_ref) in versions {
        let record = lookup(record_ref)?;
        if i64::try_from(record.index).ok() != Some(entity_ref) {
            return None;
        }
        for token in record.tokens.iter() {
            let Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0 && !versions.contains_key(reference) {
                return None;
            }
        }
        records.push(record.clone());
    }
    records.sort_unstable_by_key(|a| a.index);
    Some(records)
}

#[cfg(test)]
mod tests;
