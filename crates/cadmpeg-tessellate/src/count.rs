// SPDX-License-Identifier: Apache-2.0
//! Integer sample counts and limit tests that stay on one side of a whole
//! number when last-place libm noise crosses it.
//!
//! Adaptive tessellation takes `ceil` and `floor` of ratios, and splits a
//! segment when sag or turn is above a limit. glibc, the MSVC runtime, and
//! Apple's libm disagree by one or two units in the last place, and those
//! functions are discontinuous at integers: a semicircle that divides to
//! exactly four eighth-turns becomes five on one platform. The same
//! discontinuity adds a seed-grid row or an extra refinement split. The
//! counts and tests here snap to the nearby integer within
//! [`cadmpeg_ir::compare::FLOAT_TOLERANCE`], the budget this workspace already
//! treats as platform libm rather than a different model.

use cadmpeg_core::convert::truncate_f64_to_usize;
use cadmpeg_ir::compare::floats_agree;

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

/// Whether `error` is above `limit` by more than last-place libm disagreement.
pub(crate) fn exceeds_limit(error: f64, limit: f64) -> bool {
    error > limit && !floats_agree(error, limit)
}

#[cfg(test)]
mod tests;
