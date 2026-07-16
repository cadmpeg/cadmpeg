// SPDX-License-Identifier: Apache-2.0
//! Ownership model for decoding attacker-controllable bytes.
//!
//! A decode session has three parts. The [`DecodeArena`] owns every byte
//! buffer and hands out stable `&[u8]` borrows. The [`DecodeContext`] owns all
//! monotonic state — budget counters, the depth gauge, the space registry, the
//! ticket table, and the fuse — behind interior-mutability cells, so charging
//! takes `&self` and composes with recursion. A [`View`] is `Copy` and owns
//! only navigation: a borrowed window over one address space, tagged with its
//! [`SpaceId`].
//!
//! Invariants the types enforce:
//!
//! - A `View` never borrows movable or droppable owned data. Arena buffers are
//!   append-only heap allocations that never move or drop while the decode
//!   runs, so a `Copy` view stays valid across later allocations.
//! - A read past a window's end fails at that read. A `child` window that would
//!   exceed its parent is refused at the request site, never clamped. A `seek`
//!   below the stored lower bound is refused.
//! - Allocation from a decoded count requires two proofs: [`BoundedCount`]
//!   (the elements could fit in unread input) and a budget charge (the decode
//!   may commit the memory). Every reservation is fallible; an allocator
//!   refusal is a [`ResourceLimit`], not a process abort.
//! - Every budget charge that fails fuses the context permanently. A fused
//!   context cannot return `Ok` from [`DecodeContext::finish`], even if
//!   intermediate code swallowed the failure through an `Option` chain, so
//!   budget exhaustion is unswallowable.
//! - Counters never decrease; the depth gauge releases on guard drop. The
//!   input basis grows as expansions finalize, raising input-proportional
//!   allowances without loosening pre-expansion ones.
//! - The commit transition is an explicit type. Probe reads return `Option`;
//!   the [`Probe`] and `req_*` mirror APIs classify failure only after an
//!   interpretation is accepted.

mod arena;
mod budget;
mod context;
mod error;
mod policy;
mod probe;
mod space;
mod view;

#[cfg(test)]
mod tests;

pub use arena::DecodeArena;
pub use context::{
    DecodeContext, DepthGuard, ExactVec, ExpandSpec, ExpandWriter, GrowVec, RecordDisposition,
    RecordKind, RecordTicket,
};
pub use error::{
    ErrorContext, LimitScope, ResourceDimension, ResourceFailure, ResourceLimit, SourceLocation,
};
pub use policy::{DecodeMode, DecodePolicy, InspectOptions, ResourceLimits};
pub use probe::{ParseError, ParseErrorKind, Probe};
pub use space::{ByteRange, SourceSpan, SpaceId, SpaceOrigin, TransformKind};
pub use view::{BoundedCount, View};
