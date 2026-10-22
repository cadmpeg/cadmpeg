// SPDX-License-Identifier: Apache-2.0
//! Interval partition of semantic reads, framing and unconsumed source bytes.

use std::collections::BTreeMap;
use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit};
use cadmpeg_core::CodecError;

use super::RecordLayout;

/// Typed source-record ownership and neutral arena links.
pub mod provenance;

/// Immutable owner-derived classes of unconsumed spans.
pub mod classification;

/// Atomic B1 classification, report attribution and source closure.
pub mod finalization;

const PARTITION: &str = "partition source byte coverage";

/// Disjoint, sorted byte ranges covering one complete source stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamPartition {
    stream: String,
    byte_len: u64,
    reads: Vec<Range<u64>>,
    exempt: Vec<Range<u64>>,
    b1: Vec<Range<u64>>,
}

impl StreamPartition {
    /// Exact stream whose bytes were partitioned.
    pub fn stream(&self) -> &str {
        &self.stream
    }

    /// Complete length of the stream whose bytes were partitioned.
    pub const fn byte_len(&self) -> u64 {
        self.byte_len
    }

    /// Bytes successfully consumed by records outside given-up parts.
    pub fn reads(&self) -> &[Range<u64>] {
        &self.reads
    }

    /// Framing or insignificant bytes that no record consumed.
    pub fn exempt(&self) -> &[Range<u64>] {
        &self.exempt
    }

    /// Maximal spans that no record was built from.
    pub fn b1(&self) -> &[Range<u64>] {
        &self.b1
    }
}

/// Partitions a named stream using completed record captures and source framing.
///
/// `framing` and `insignificant` are admitted by the owning format's framer.
/// Given-up content is B1, including overlaps with another record's capture
/// and insignificant bytes inside the part; framing bytes remain exempt.
/// This function does not assign classes, mutate fidelity, or charge
/// losses; those steps require the record-owner and part-tree inventories.
pub fn partition_stream(
    ctx: &DecodeContext<'_>,
    stream: &str,
    byte_len: u64,
    layouts: &BTreeMap<String, RecordLayout>,
    framing: &[Range<u64>],
    insignificant: &[Range<u64>],
    given_up: &[Range<u64>],
) -> Result<StreamPartition, CodecError> {
    if stream.is_empty() {
        return Err(CodecError::malformed(
            "source coverage requires a named stream",
        ));
    }
    for layout in layouts.values().filter(|layout| layout.stream() == stream) {
        if layout
            .offset()
            .checked_add(layout.byte_len())
            .is_none_or(|end| end > byte_len)
        {
            return Err(CodecError::malformed(
                "record extent exceeds its source stream",
            ));
        }
    }
    let read_ranges = layouts
        .values()
        .filter_map(|layout| layout.reads().get(stream))
        .flatten();
    let reads = normalize(ctx, read_ranges, byte_len)?;
    let given_up = normalize(ctx, given_up.iter(), byte_len)?;
    let insignificant = normalize(ctx, insignificant.iter(), byte_len)?;
    let insignificant = difference(ctx, &insignificant, &given_up)?;
    let exempt = normalize(ctx, framing.iter().chain(&insignificant), byte_len)?;
    let reads = difference(ctx, &reads, &given_up)?;
    let exempt = difference(ctx, &exempt, &reads)?;
    let covered = normalize(ctx, reads.iter().chain(&exempt), byte_len)?;
    let b1 = difference(ctx, &[0..byte_len], &covered)?;
    let stream_bytes = u64_from_index(stream.len());
    ctx.retain_bytes(stream_bytes, PARTITION)?;
    let mut stream_name = String::new();
    stream_name.try_reserve_exact(stream.len()).map_err(|_| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            stream_bytes,
            PARTITION,
        )
    })?;
    stream_name.push_str(stream);
    Ok(StreamPartition {
        stream: stream_name,
        byte_len,
        reads,
        exempt,
        b1,
    })
}

fn allocate_ranges(ctx: &DecodeContext<'_>, count: usize) -> Result<Vec<Range<u64>>, CodecError> {
    let bytes = u64_from_index(count)
        .checked_mul(u64_from_index(std::mem::size_of::<Range<u64>>()))
        .ok_or_else(|| CodecError::malformed("source interval allocation size overflow"))?;
    ctx.retain_bytes(bytes, PARTITION)?;
    let mut ranges = ctx.alloc_filled(count, 0..0, PARTITION)?;
    ranges.clear();
    Ok(ranges)
}

fn normalize<'a>(
    ctx: &DecodeContext<'_>,
    input: impl Iterator<Item = &'a Range<u64>> + Clone,
    byte_len: u64,
) -> Result<Vec<Range<u64>>, CodecError> {
    let mut ranges = allocate_ranges(ctx, input.clone().count())?;
    for range in input {
        if range.start >= range.end || range.end > byte_len {
            return Err(CodecError::malformed(
                "source interval is empty, inverted or out of bounds",
            ));
        }
        ranges.push(range.clone());
    }
    ranges.sort_unstable_by_key(|range| (range.start, range.end));
    let mut output = 0;
    for index in 0..ranges.len() {
        if output > 0 && ranges[index].start <= ranges[output - 1].end {
            ranges[output - 1].end = ranges[output - 1].end.max(ranges[index].end);
        } else {
            ranges[output] = ranges[index].clone();
            output += 1;
        }
    }
    ranges.truncate(output);
    Ok(ranges)
}

fn difference(
    ctx: &DecodeContext<'_>,
    left: &[Range<u64>],
    right: &[Range<u64>],
) -> Result<Vec<Range<u64>>, CodecError> {
    let capacity = left
        .len()
        .checked_add(right.len())
        .ok_or_else(|| CodecError::malformed("source interval count overflow"))?;
    let mut output = allocate_ranges(ctx, capacity)?;
    let mut first = 0;
    for range in left {
        let mut start = range.start;
        while first < right.len() && right[first].end <= start {
            first += 1;
        }
        for cut in &right[first..] {
            if cut.start >= range.end {
                break;
            }
            if cut.start > start {
                output.push(start..cut.start);
            }
            start = start.max(cut.end);
            if start >= range.end {
                break;
            }
        }
        if start < range.end {
            output.push(start..range.end);
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
