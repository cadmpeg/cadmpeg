// SPDX-License-Identifier: Apache-2.0
//! Views and domain projections of sequentially owned Deltas events.

pub(crate) mod census;
pub(crate) mod group;
pub(crate) mod partition_prefix;
pub(crate) mod pmark_state;
pub(crate) mod record_family;
pub(crate) mod record_kind;
pub(crate) mod region;
pub(crate) mod section;
pub(crate) mod transmit_state;

#[cfg(test)]
mod tests;
