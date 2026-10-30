// SPDX-License-Identifier: Apache-2.0
//! Integer sample counts and limit tests that stay on one side of a cut
//! when platform libm noise crosses it.
//!
//! Adaptive tessellation takes `ceil` and `floor` of ratios, and splits a
//! segment when sag or turn is above a limit. Those functions are
//! discontinuous at integers. Count ratios snap to a nearby whole number
//! within [`cadmpeg_ir::compare::FLOAT_TOLERANCE`]. Split tests require sag
//! or turn to pass the limit by [`SPLIT_RATIO`], and rank near-equal
//! lengths on the same quantum, so a 3D chord from platform libm does not
//! pick a different branch.

use cadmpeg_core::convert::truncate_f64_to_usize;
use cadmpeg_ir::compare::floats_agree;

/// Relative quantum of a sag, turn, or length used to decide a split.
pub(crate) const SPLIT_RATIO: f64 = 5e-3;

/// Smallest integer at least `x`, or the nearest integer when `x` is within
/// [`cadmpeg_ir::compare::FLOAT_TOLERANCE`] of one.
pub(crate) fn ceil_count(x: f64) -> Option<usize> {
    if !x.is_finite() {
        return None;
    }
    if x <= 0.0 {
        return Some(0);
    }
    let nearest = x.round();
    let count = if floats_agree(x, nearest) {
        nearest
    } else {
        x.ceil()
    };
    truncate_f64_to_usize(count)
}

/// Largest integer at most `x`, or the nearest integer when `x` is within
/// [`cadmpeg_ir::compare::FLOAT_TOLERANCE`] of one.
pub(crate) fn floor_count(x: f64) -> Option<usize> {
    if !x.is_finite() {
        return None;
    }
    if x <= 0.0 {
        return Some(0);
    }
    let nearest = x.round();
    let count = if floats_agree(x, nearest) {
        nearest
    } else {
        x.floor()
    };
    truncate_f64_to_usize(count)
}

/// `value` in units of `SPLIT_RATIO * |scale|`.
fn ranked(value: f64, scale: f64) -> Option<f64> {
    if !value.is_finite() || !scale.is_finite() {
        return None;
    }
    let quantum = scale.abs().max(f64::MIN_POSITIVE) * SPLIT_RATIO;
    let rank = (value / quantum).round();
    rank.is_finite().then_some(rank)
}

/// Order of two lengths or angles after ranking on [`SPLIT_RATIO`].
pub(crate) fn cmp_ranked(left: f64, right: f64) -> std::cmp::Ordering {
    let scale = left.abs().max(right.abs());
    match (ranked(left, scale), ranked(right, scale)) {
        (Some(left), Some(right)) => left.total_cmp(&right),
        _ => std::cmp::Ordering::Equal,
    }
}

/// Whether `error` is above `limit` by more than [`SPLIT_RATIO`].
pub(crate) fn exceeds_limit(error: f64, limit: f64) -> bool {
    limit.is_finite() && error > limit * (1.0 + SPLIT_RATIO)
}

#[cfg(test)]
mod tests;
