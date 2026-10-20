// SPDX-License-Identifier: Apache-2.0
//! Bounded ownership and decompression limits for untrusted decode input.
//!
//! [`DecodeArena`] owns stable buffers, [`DecodeContext`] owns session state,
//! and [`View`] provides bounded navigation within one address space.

mod arena;
mod budget;
pub mod collect;
mod context;
mod deflate;
mod depth;
mod error;
mod input;
mod policy;
mod probe;
mod space;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub mod test_support;
pub mod tree;
mod unique;
mod utf16;
mod view;
pub mod zstd;

#[cfg(test)]
mod tests;

pub use arena::DecodeArena;
pub use budget::{work_units, BudgetExhausted, DepthGuard, WorkBudget, WorkBudgetRecursionGuard};
pub use context::{DecodeContext, ExpandSpec, ExpandWriter};
pub use depth::DecodeDepth;
pub use error::{ResourceDimension, ResourceFailure, ResourceLimit, SourceLocation};
pub use policy::{DecodeMode, DecodePolicy, InspectOptions, ResourceLimits};
pub use probe::{ParseError, ParseErrorKind};
pub use space::{ByteRange, SpaceId};
pub use unique::unique_index;
pub use view::{
    bounded_len, id_from_index, index_from_u32, index_from_u64, u64_from_index, BoundedCount, View,
};
