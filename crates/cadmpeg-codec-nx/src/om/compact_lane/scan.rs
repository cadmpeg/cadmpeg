// SPDX-License-Identifier: Apache-2.0
//! Complete counted and ABR lane admission.

use super::{AbrLane, CountedLane, COUNTED_PREFIX, COUNTED_TERMINATOR, ABR_TERMINATOR};
use crate::om::compact::{CountedIndexMembers, LocatedCompactIndex, NullableCompactIndex};

/// Decode fixed-width `ABR` block-reference lanes from contiguous column storage.
pub(crate) fn abr_lanes(bytes: &[u8]) -> Vec<AbrLane> {
    let mut lanes = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        if bytes[start] != 0x11 {
            start += 1;
            continue;
        }
        let mut at = start + 1;
        let tokens = (|| {
            let mut slots = [None; 16];
            for slot in &mut slots {
                let token = NullableCompactIndex::read(bytes, at)?;
                at += token.raw().len();
                *slot = token.atom.map(Into::into);
            }
            Some(slots)
        })();
        let Some(tokens) = tokens else {
            start += 1;
            continue;
        };
        let Some(end) = at.checked_add(ABR_TERMINATOR.len()) else {
            start += 1;
            continue;
        };
        if bytes.get(at..end) == Some(&ABR_TERMINATOR) {
            if let Some(lane) = AbrLane::<(), usize>::new(tokens, start) { lanes.push(lane); }
            start = end;
        } else {
            start += 1;
        }
    }
    lanes
}

/// Decode complete counted compact-index lanes from one bounded store block.
///
/// A lane is `01, count:u8, anchor, member[count-2], 01 11`, with
/// `count >= 3`. Compact indices use the ordinary direct/extended encoding;
/// null indices reject the candidate atomically.
pub(crate) fn counted_lanes(bytes: &[u8]) -> Vec<CountedLane> {
    let decode = |start: usize| {
        (bytes.get(start) == Some(&0x01)).then_some(())?;
        let declared_count = *bytes.get(start + 1)?;
        (declared_count >= 3).then_some(())?;
        let anchor = LocatedCompactIndex::read(bytes, start + usize::from(COUNTED_PREFIX))?;
        let members_start = anchor.offset + anchor.atom.raw().len();
        let mut at = members_start;
        for _ in 0..usize::from(declared_count) - 2 {
            at += LocatedCompactIndex::read(bytes, at)?.atom.raw().len();
        }
        let end = at.checked_add(COUNTED_TERMINATOR.len())?;
        (bytes.get(at..end) == Some(&COUNTED_TERMINATOR)).then_some(())?;
        at = members_start;
        let members = (0..usize::from(declared_count) - 2)
            .map(|_| {
                let token = LocatedCompactIndex::read(bytes, at)?;
                at += token.atom.raw().len();
                Some(token.atom.into())
            })
            .collect::<Option<Vec<_>>>()?;
        Some((
            CountedLane::<(), usize>::new(anchor.atom.into(), CountedIndexMembers::new(members).ok()?, start)?,
            end,
        ))
    };
    let mut lanes = Vec::new();
    let mut start = 0;
    while start + 4 <= bytes.len() {
        if let Some((lane, end)) = decode(start) {
            lanes.push(lane);
            start = end;
        } else {
            start += 1;
        }
    }
    lanes
}

