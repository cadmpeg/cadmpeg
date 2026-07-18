// SPDX-License-Identifier: Apache-2.0
//! Address spaces and their derivation origins.
//!
//! Every byte a decode reads belongs to exactly one address space: the root
//! input, an inflated entry, a reconstructed stream. A [`SpaceId`] names one
//! space; a [`View`](crate::decode::View) carries the id so error locations
//! and ledger attribution fall out of the type. Coordinates are absolute
//! within a space: offset zero is that space's first byte.
//!
//! The registry is the runtime skeleton of the multi-space fidelity ledger.
//! Origins are recorded here so a later serialization pass can tile each
//! space without re-deriving how it was produced. Serialization is not part
//! of this module.

use std::cell::RefCell;

/// Names one address space within a single decode.
///
/// Ids are dense and assigned in registration order; the root is always
/// [`SpaceId::ROOT`]. Views compare and attribute by id, never by buffer
/// pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpaceId(u32);

impl SpaceId {
    /// The root input space, registered first by every decode.
    pub const ROOT: SpaceId = SpaceId(0);

    /// Returns the dense index of this space.
    pub fn index(self) -> u32 {
        self.0
    }
}

/// A half-open byte range `[start, end)` within one space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// Inclusive start offset.
    pub start: u64,
    /// Exclusive end offset.
    pub end: u64,
}

/// A byte range qualified by the space it lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    /// The space the range indexes.
    pub space: SpaceId,
    /// The range within that space.
    pub range: ByteRange,
}

/// How a derived space was produced from its inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformKind {
    /// The derived bytes are the decompression of the input span.
    Decompress,
}

/// How a space came to exist, relative to earlier spaces.
///
/// The root has no parent. A slice borrows a contiguous parent range without
/// copying. A concatenation assembles multiple extents. A transform (such as
/// decompression) produces new bytes from named inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceOrigin {
    /// The root input space.
    Root,
    /// A contiguous borrowed subrange of a parent space.
    Slice {
        /// The parent space.
        parent: SpaceId,
        /// The borrowed range within the parent.
        range: ByteRange,
    },
    /// An assembly of multiple parent extents.
    Concat {
        /// The ordered input extents.
        segments: Vec<SourceSpan>,
    },
    /// New bytes derived from named inputs by a transform.
    Transform {
        /// The input extents consumed by the transform.
        inputs: Vec<SourceSpan>,
        /// The kind of transform applied.
        kind: TransformKind,
    },
}

/// One registered space: its identity, length, and derivation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpaceRecord {
    pub(crate) id: SpaceId,
    pub(crate) length: u64,
    pub(crate) origin: SpaceOrigin,
}

/// Append-only registry of the spaces a decode has produced.
///
/// Interior-mutable so registration composes with `&self` charging. Ids are
/// handed out in registration order and never reused.
#[derive(Debug, Default)]
pub(crate) struct SpaceRegistry {
    records: RefCell<Vec<SpaceRecord>>,
}

impl SpaceRegistry {
    /// Registers the root space and returns [`SpaceId::ROOT`].
    pub(crate) fn register_root(&self, length: u64) -> SpaceId {
        self.register(length, SpaceOrigin::Root)
    }

    /// Registers a derived space and returns its fresh id.
    pub(crate) fn register(&self, length: u64, origin: SpaceOrigin) -> SpaceId {
        let mut records = self.records.borrow_mut();
        let id = SpaceId(u32::try_from(records.len()).unwrap_or(u32::MAX));
        records.push(SpaceRecord { id, length, origin });
        id
    }

    /// Returns the registered spaces in derivation order.
    pub(crate) fn records(&self) -> Vec<SpaceRecord> {
        self.records.borrow().clone()
    }

    /// Returns how many spaces are registered.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.records.borrow().len()
    }

    /// Returns a registered space's derivation origin, if it exists.
    #[cfg(test)]
    pub(crate) fn origin(&self, id: SpaceId) -> Option<SpaceOrigin> {
        self.records
            .borrow()
            .get(id.index() as usize)
            .map(|record| record.origin.clone())
    }
}
