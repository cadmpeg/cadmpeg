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
mod part;
mod policy;
mod probe;
pub mod scope;
mod space;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub mod test_support;
mod tracking;
pub mod tree;
mod unique;
mod utf16;
mod view;
mod xml_scan;
pub mod zstd;

#[cfg(test)]
mod tests;

pub use arena::DecodeArena;
pub use budget::{work_units, BudgetExhausted, DepthGuard, WorkBudget, WorkBudgetRecursionGuard};
pub use context::{DecodeContext, ExpandSpec, ExpandWriter};
pub use depth::DecodeDepth;
pub use error::{LimitScope, ResourceDimension, ResourceFailure, ResourceLimit, SourceLocation};
pub use part::{PartCapture, PartOutcome};
pub use policy::{DecodeMode, DecodePolicy, InspectOptions, ResourceLimits};
pub use probe::{ParseError, ParseErrorKind};
pub use scope::{ArenaClass, ArenaClasses, ContentScope};
pub use space::{ByteRange, SpaceId};
pub use tracking::{
    ReadTransaction, RecordCapture, RecordCaptureParts, SourceOrigin, SourceSegment, SourceStream,
};
pub use unique::unique_index;
pub use view::{
    bounded_len, id_from_index, index_from_u32, index_from_u64, u64_from_index, BoundedCount, View,
};
pub use xml_scan::{
    XmlElement, XmlElementScan, XmlEncoding, XmlFramingFault, XmlFramingIssue, XmlTermination,
};
