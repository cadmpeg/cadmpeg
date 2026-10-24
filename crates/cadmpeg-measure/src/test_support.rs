// SPDX-License-Identifier: Apache-2.0
//! Measured round-trip comparisons for writer tests, and the document
//! builders and decode context the crate's own tests measure with.

#[cfg(test)]
pub(crate) mod builders;
pub mod round_trip;
