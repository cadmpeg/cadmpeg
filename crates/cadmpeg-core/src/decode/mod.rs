// SPDX-License-Identifier: Apache-2.0
//! Bounded ownership and decompression limits for untrusted decode input.
//!
//! [`DecodeArena`] owns stable buffers, [`DecodeContext`] owns session state,
//! and [`View`] provides bounded navigation within one address space.

mod arena;
mod budget;
pub mod collect;
pub mod compare;
mod context;
pub mod cost;
mod deflate;
mod error;
pub mod extend_source;
mod heap;
mod input;
pub mod iter_source;
mod mutate;
mod policy;
mod probe;
pub mod scan;
mod sort;
mod space;
pub mod text;
pub mod text_collect;
mod text_queries;
pub mod tree;
mod unique;
mod utf16;
mod view;
pub mod work_scratch;
pub mod zstd;

#[cfg(test)]
mod tests;

pub use arena::DecodeArena;
pub use budget::{
    refuse_local_limit, work_units, BudgetExhausted, DepthGuard, ScopedReservation, WorkBudget,
    WorkBudgetRecursionGuard,
};
pub use context::{DecodeContext, ExpandSpec, ExpandWriter};
pub use error::{ResourceDimension, ResourceFailure, ResourceLimit, SourceLocation};
pub use policy::{DecodeMode, DecodePolicy, InspectOptions, ResourceLimits};
pub use probe::{ParseError, ParseErrorKind};
pub use space::{ByteRange, SpaceId};
pub use view::{
    bounded_len, id_from_index, index_from_u32, index_from_u64, u64_from_index, BoundedCount, View,
};
