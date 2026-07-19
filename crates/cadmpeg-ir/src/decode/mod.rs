// SPDX-License-Identifier: Apache-2.0
//! Bounded ownership and resource accounting for untrusted decode input.
//!
//! [`DecodeArena`] owns stable buffers, [`DecodeContext`] owns session state,
//! and [`View`] provides bounded navigation within one address space. Failed
//! budget charges fuse the context and cannot be hidden from `finish`.

mod arena;
mod budget;
mod context;
mod error;
mod policy;
mod probe;
mod retained;
mod space;
mod view;

#[cfg(test)]
mod tests;

pub use arena::DecodeArena;
pub use context::{
    DecodeContext, DepthGuard, DerivedKind, DerivedWriter, ExactVec, ExpandSpec, ExpandWriter,
    GrowVec,
};
pub use error::{
    ErrorContext, LimitScope, ResourceDimension, ResourceFailure, ResourceLimit, SourceLocation,
};
pub use policy::{DecodeMode, DecodePolicy, InspectOptions, ResourceLimits};
pub use probe::{ParseError, ParseErrorKind};
pub use retained::{RetainedBlob, RetainedBlobId, RetainedRange, Retention};
pub use space::{ByteRange, SpaceId};
pub use view::{BoundedCount, View};
