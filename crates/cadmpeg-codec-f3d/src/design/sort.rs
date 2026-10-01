// SPDX-License-Identifier: Apache-2.0
//! Stable Design ordering with admitted temporary index storage.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use std::cmp::Ordering;

const SMALL_SORT_LEN: usize = 20;
const VISITED: usize = 1 << (usize::BITS - 1);

pub(super) fn sort_by<T>(
    ctx: &DecodeContext<'_>,
    values: &mut [T],
    mut compare: impl FnMut(&T, &T) -> Ordering,
) -> Result<(), CodecError> {
    if values.len() <= SMALL_SORT_LEN {
        for end in 1..values.len() {
            let start = values[..end]
                .partition_point(|value| compare(value, &values[end]) != Ordering::Greater);
            values[start..=end].rotate_right(1);
        }
        return Ok(());
    }

    if std::mem::size_of::<T>() == 0 {
        return Ok(());
    }
    let count = values.len();
    if count & VISITED != 0 {
        return Err(ctx.refuse_codec_limit("f3d stable sort permutation", 0, 1));
    }
    let work = u64_from_index(count)
        .checked_mul(u64::from(count.ilog2()) + 1)
        .ok_or_else(|| ctx.refuse_codec_limit("f3d stable sort work", 0, 1))?;
    ctx.charge_work(work, "f3d stable sort work")?;

    let bytes = count
        .checked_mul(std::mem::size_of::<usize>())
        .ok_or_else(|| ctx.refuse_codec_limit("f3d stable sort scratch", 0, 1))?;
    let _scratch = ctx.reserve_scoped(u64_from_index(bytes), "f3d stable sort scratch")?;
    let mut permutation = Vec::new();
    ctx.reserve_vec(&mut permutation, count, "f3d stable sort permutation")?;
    permutation.extend(0..count);
    ctx.sort_unstable_by(
        &mut permutation,
        |left, right| compare(&values[*left], &values[*right]).then_with(|| left.cmp(right)),
        |_| 0,
        "f3d stable sort permutation order",
    )?;
    // Invert source indices into destination indices, marking each completed cycle.
    for start in 0..count {
        if permutation[start] & VISITED != 0 {
            continue;
        }
        let mut previous = start;
        let mut current = permutation[start];
        while current != start {
            let next = permutation[current];
            permutation[current] = previous | VISITED;
            previous = current;
            current = next;
        }
        permutation[start] = previous | VISITED;
    }
    for destination in &mut permutation {
        *destination &= !VISITED;
    }
    for start in 0..count {
        while permutation[start] != start {
            let destination = permutation[start];
            values.swap(start, destination);
            permutation.swap(start, destination);
        }
    }
    Ok(())
}

pub(super) fn sort_by_key<T, K: Ord>(
    ctx: &DecodeContext<'_>,
    values: &mut [T],
    mut key: impl FnMut(&T) -> K,
) -> Result<(), CodecError> {
    sort_by(ctx, values, |left, right| key(left).cmp(&key(right)))
}

#[cfg(test)]
mod tests;
