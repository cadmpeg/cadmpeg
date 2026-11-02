// SPDX-License-Identifier: Apache-2.0
//! Views and domain projections of sequentially owned Deltas events.

pub(crate) mod census;
pub mod group;
pub mod partition_prefix;
pub mod pmark_state;
pub mod record_family;
pub(crate) mod record_kind;
pub mod region;
pub mod section;
pub mod transmit_state;

#[cfg(test)]
mod tests;
