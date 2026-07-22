// SPDX-License-Identifier: Apache-2.0
//! Bounded ownership and decompression limits for untrusted decode input.
//!
//! [`DecodeArena`] owns stable buffers, [`DecodeContext`] owns session state,
//! and [`View`] provides bounded navigation within one address space.

mod arena;
mod context;
mod error;
mod policy;
mod probe;
mod space;
mod view;

#[cfg(test)]
mod tests;

pub use arena::DecodeArena;
pub use context::{DecodeContext, ExpandSpec, ExpandWriter};
pub use error::{
    ErrorContext, LimitScope, ResourceDimension, ResourceFailure, ResourceLimit, SourceLocation,
};
pub use policy::{DecodeMode, DecodePolicy, InspectOptions, ResourceLimits};
pub use probe::{ParseError, ParseErrorKind};
pub use space::{ByteRange, SpaceId};
pub use view::{BoundedCount, View};
