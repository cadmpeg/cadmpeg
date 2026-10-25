// SPDX-License-Identifier: Apache-2.0
//! Validate the live transaction against its grammar-discovered source prefix.

use super::{reserve_working, u64_from_index, DecodeBudget, ReadTransaction, SpaceId, View};
use crate::CodecError;
use std::ops::Range;

/// Working boundary evidence only: no semantic owner, capture or read credit.
pub(super) fn observe_read(
    budget: &DecodeBudget,
    reads: &mut Vec<(SpaceId, Range<u64>)>,
    space: SpaceId,
    mut range: Range<u64>,
) -> Result<(), CodecError> {
    if reads.last().is_none_or(|(old_space, old)| {
        *old_space < space || (*old_space == space && old.end < range.start)
    }) {
        reserve_working(budget, reads, 1)?;
        reads.push((space, range));
        return Ok(());
    }
    let first = reads.partition_point(|(old_space, old)| {
        *old_space < space || (*old_space == space && old.end < range.start)
    });
    let after = first
        + reads[first..]
            .partition_point(|(old_space, old)| *old_space == space && old.start <= range.end);
    if first < after {
        range.start = range.start.min(reads[first].1.start);
        range.end = range.end.max(reads[after - 1].1.end);
        reads[first] = (space, range);
        drop(reads.drain(first + 1..after));
    } else {
        reserve_working(budget, reads, 1)?;
        reads.insert(first, (space, range));
    }
    Ok(())
}

fn contains_read(reads: &[(SpaceId, Range<u64>)], space: SpaceId, range: &Range<u64>) -> bool {
    let after = reads.partition_point(|(candidate, extent)| {
        *candidate < space || (*candidate == space && extent.start <= range.start)
    });
    after
        .checked_sub(1)
        .and_then(|index| reads.get(index))
        .is_some_and(|(candidate, extent)| {
            *candidate == space && range.start >= extent.start && range.end <= extent.end
        })
}

impl ReadTransaction<'_> {
    pub(in crate::decode) fn observe_part_boundary(&self) -> Result<(), CodecError> {
        self.session.session(|session| {
            let frame = session
                .frames
                .last_mut()
                .filter(|frame| frame.token == self.token && frame.part_attempts.is_some())
                .ok_or_else(|| {
                    CodecError::malformed("discovered part transaction is not active")
                })?;
            if frame.boundary_reads.is_some() {
                return Err(CodecError::malformed(
                    "discovered boundary observation already started",
                ));
            }
            frame.boundary_reads = Some(Vec::new());
            Ok(())
        })
    }

    pub(in crate::decode) fn validate_part_prefix(
        &self,
        prefix: View<'_>,
    ) -> Result<(), CodecError> {
        self.session.session(|session| {
            let frame = session
                .frames
                .last()
                .filter(|frame| frame.token == self.token)
                .ok_or_else(|| CodecError::malformed("discovered row transaction is not active"))?;
            let mut allowed = Vec::new();
            session.streams.project(
                &session.budget,
                prefix.space(),
                u64_from_index(prefix.start())..u64_from_index(prefix.end()),
                |space, range| {
                    if contains_read(&allowed, space, &range) {
                        return Ok(false);
                    }
                    observe_read(&session.budget, &mut allowed, space, range)?;
                    Ok(true)
                },
            )?;
            let covers = |space, range: &Range<u64>| contains_read(&allowed, space, range);
            // Index each projected stream once, independent of its number of
            // disjoint ranges. Names borrow the actual registered inventory.
            let mut spaces = Vec::new();
            for (space, _) in &allowed {
                if spaces.last().is_none_or(|(previous, _)| previous != space) {
                    reserve_working(&session.budget, &mut spaces, 1)?;
                    spaces.push((*space, session.streams.name(*space)?));
                }
            }
            spaces.sort_unstable_by(|(_, left), (_, right)| left.cmp(right));
            let source_space = |name: &str| {
                spaces
                    .binary_search_by(|(_, candidate)| candidate.cmp(&name))
                    .ok()
                    .map(|index| spaces[index].0)
            };
            let observed = frame.boundary_reads.as_ref().ok_or_else(|| {
                CodecError::malformed("discovered boundary observation is missing")
            })?;
            if observed.iter().any(|(space, range)| !covers(*space, range)) {
                return Err(CodecError::malformed(
                    "actual parser read escapes its consumed source prefix",
                ));
            }
            for delta in &frame.deltas {
                if delta.ranges.iter().any(|range| !covers(delta.space, range)) {
                    return Err(CodecError::malformed(
                        "discovered row read escapes its consumed source prefix",
                    ));
                }
            }
            for capture in &frame.captures {
                let owner_space = source_space(&capture.stream);
                let end = capture
                    .offset
                    .checked_add(capture.byte_len)
                    .ok_or_else(|| CodecError::malformed("discovered record extent overflows"))?;
                if !capture
                    .owner
                    .is_some_and(super::owners::CaptureOwner::is_continuation)
                    && !owner_space.is_some_and(|space| covers(space, &(capture.offset..end)))
                {
                    return Err(CodecError::malformed(
                        "discovered record escapes its consumed source prefix",
                    ));
                }
                for (stream, ranges) in &capture.reads {
                    let read_space = source_space(stream);
                    if !read_space
                        .is_some_and(|space| ranges.iter().all(|range| covers(space, range)))
                    {
                        return Err(CodecError::malformed(
                            "discovered record read escapes its consumed source prefix",
                        ));
                    }
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
