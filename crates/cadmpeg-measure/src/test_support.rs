// SPDX-License-Identifier: Apache-2.0
//! Measured round-trip comparisons for writer tests, the box pass against
//! the full pass, and the document builders and decode context the crate's
//! own tests measure with.

pub mod box_pass;
#[cfg(test)]
pub(crate) mod builders;
pub mod round_trip;
