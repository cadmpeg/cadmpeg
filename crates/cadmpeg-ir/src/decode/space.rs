// SPDX-License-Identifier: Apache-2.0
//! Address-space identifiers.
//!
//! Every byte a decode reads belongs to exactly one address space: the root
//! input, an inflated entry, a reconstructed stream. A [`SpaceId`] names one
//! space; a [`View`](crate::decode::View) carries the id so error locations
//! and error attribution fall out of the type. Coordinates are absolute
//! within a space: offset zero is that space's first byte.

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

/// Append-only registry of the spaces a decode has produced.
///
/// Interior-mutable so registration composes with `&self` charging. Ids are
/// handed out in registration order and never reused.
#[derive(Debug, Default)]
pub(crate) struct SpaceRegistry {
    count: RefCell<u32>,
}

impl SpaceRegistry {
    /// Registers the root space and returns [`SpaceId::ROOT`].
    pub(crate) fn register_root(&self) -> SpaceId {
        self.register()
    }

    /// Registers a derived space and returns its fresh id.
    pub(crate) fn register(&self) -> SpaceId {
        let mut count = self.count.borrow_mut();
        let id = SpaceId(*count);
        *count = count.saturating_add(1);
        id
    }

    /// Returns how many spaces are registered.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        *self.count.borrow() as usize
    }
}
