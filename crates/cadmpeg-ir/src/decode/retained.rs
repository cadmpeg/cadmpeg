// SPDX-License-Identifier: Apache-2.0
//! Retained opaque blobs and their subrange references.
//!
//! An opaque payload the decoder cannot interpret is preserved by digest and,
//! when policy permits, by bytes: a *retained blob*. Blob identity is the blob's
//! own content — its lowercase-hex SHA-256 digest — so two decodes of the same
//! file name the same blobs regardless of registration order, and retaining the
//! same bytes twice deduplicates to one blob charged once.
//!
//! One blob backs many opaque spans through containment: a full archive entry is
//! retained once, and each record inside it references a [`RetainedRange`]
//! subrange rather than copying bytes (§6.1). Retained bytes are borrowed, never
//! re-copied: the store holds the address of `&'a [u8]` bytes in the arena (or
//! the caller's input), so blobs survive the context's teardown without a copy —
//! the egress a codec collects through
//! [`DecodeContext::retained_blobs`](super::DecodeContext::retained_blobs) stays
//! valid for the arena's lifetime.
//!
//! Retained bytes charge the
//! [`RetainedBytes`](super::error::ResourceDimension::RetainedBytes) budget
//! dimension. When that budget is exhausted the outcome is mode-defined
//! (§11.10): strict mode fails with a `ResourceLimit` (the charge fuses the
//! context); salvage mode degrades recovery to accounting — it keeps the digest,
//! drops the bytes, records a loss note and a report flag, and never fails the
//! decode for retention alone.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use crate::source_fidelity::{RetainedRef, SerializedRange};

/// A stable retained-blob identity: the lowercase-hex SHA-256 digest of the
/// blob's bytes.
///
/// Content addressing makes identity independent of registration order and makes
/// deduplication exact — identical bytes always produce the same id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RetainedBlobId(String);

impl RetainedBlobId {
    /// Wraps a digest string as a blob id.
    pub(crate) fn new(digest: String) -> Self {
        RetainedBlobId(digest)
    }

    /// Returns the canonical id string (the blob's SHA-256 digest).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the id, returning the owned digest string.
    pub fn into_string(self) -> String {
        self.0
    }
}

/// A half-open `[start, end)` subrange of one retained blob.
///
/// A newly retained blob yields the whole-blob range `[0, len)`;
/// [`RetainedRange::subrange`] narrows it under containment so many opaque spans
/// can reference one blob without duplicating bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedRange {
    blob: RetainedBlobId,
    start: u64,
    end: u64,
}

impl RetainedRange {
    /// Builds a whole-blob range `[0, len)` for a digest.
    pub(crate) fn whole(digest: String, len: u64) -> RetainedRange {
        RetainedRange {
            blob: RetainedBlobId(digest),
            start: 0,
            end: len,
        }
    }

    /// Returns the blob this range references.
    pub fn blob(&self) -> &RetainedBlobId {
        &self.blob
    }

    /// Returns the range start offset within the blob.
    pub fn start(&self) -> u64 {
        self.start
    }

    /// Returns the range end offset within the blob.
    pub fn end(&self) -> u64 {
        self.end
    }

    /// Returns the range length.
    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// Returns whether the range is empty.
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    /// Narrows this range to a contained `[start, end)` subrange, returning
    /// `None` if the requested range is inverted or escapes this range.
    ///
    /// Containment, not exact-extent equality, is the rule (§6.1): every
    /// resulting reference stays inside the one retained blob.
    pub fn subrange(&self, start: u64, end: u64) -> Option<RetainedRange> {
        (self.start <= start && start <= end && end <= self.end).then(|| RetainedRange {
            blob: self.blob.clone(),
            start,
            end,
        })
    }

    /// Converts to the serialized sidecar reference (§6.1, schema v2).
    pub fn to_serialized(&self) -> RetainedRef {
        RetainedRef {
            blob: self.blob.0.clone(),
            range: SerializedRange {
                start: self.start,
                end: self.end,
            },
        }
    }
}

/// The outcome of a retention request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retention {
    /// The bytes were retained; the range recovers them from the blob.
    Retained(RetainedRange),
    /// The retained-byte budget was exhausted in salvage mode: the digest is
    /// kept for accounting, the bytes are not retained, and recovery for this
    /// span is unavailable (§11.10).
    Accounted {
        /// The SHA-256 digest of the bytes that would have been retained.
        digest: String,
    },
}

impl Retention {
    /// Returns the recoverable range, or `None` when retention degraded to
    /// accounting.
    pub fn range(&self) -> Option<&RetainedRange> {
        match self {
            Retention::Retained(range) => Some(range),
            Retention::Accounted { .. } => None,
        }
    }

    /// Returns whether the bytes were retained (recoverable).
    pub fn is_retained(&self) -> bool {
        matches!(self, Retention::Retained(_))
    }
}

/// One retained blob egressed from a decode: its identity and borrowed bytes.
#[derive(Debug, Clone)]
pub struct RetainedBlob<'a> {
    /// The blob's stable content-addressed identity.
    pub id: RetainedBlobId,
    /// The retained bytes, borrowed for the arena's lifetime.
    pub bytes: &'a [u8],
}

/// The address and length of one retained blob's bytes.
///
/// A raw address, not a `&'a [u8]`, is stored on purpose: an interior-mutable
/// `RefCell<..&'a..>` would make [`DecodeContext`](super::DecodeContext)
/// invariant over `'a` and break the covariance codecs rely on to unify a
/// context and a view of different regions. The address originates from a
/// `&'a [u8]` passed to retention, so the bytes live for the arena's lifetime;
/// [`DecodeContext::retained_blobs`](super::DecodeContext::retained_blobs)
/// rebuilds the borrow under that lifetime.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RetainedAddr {
    pub(crate) ptr: *const u8,
    pub(crate) len: usize,
}

/// Content-addressed store of retained blobs for one decode.
///
/// Interior-mutable so retention composes with `&self` charging. Keyed by
/// digest, so identical bytes deduplicate to one entry. Carries no lifetime
/// parameter (see [`RetainedAddr`]).
#[derive(Debug, Default)]
pub(crate) struct RetainedStore {
    blobs: RefCell<BTreeMap<String, RetainedAddr>>,
    degraded: Cell<bool>,
    degraded_bytes: Cell<u64>,
    degraded_count: Cell<u64>,
}

impl RetainedStore {
    /// Returns whether a blob with `digest` is already retained.
    pub(crate) fn contains(&self, digest: &str) -> bool {
        self.blobs.borrow().contains_key(digest)
    }

    /// Inserts a retained blob, keyed by its digest. Idempotent: re-inserting a
    /// present digest keeps the existing address.
    pub(crate) fn insert(&self, digest: String, addr: RetainedAddr) {
        self.blobs.borrow_mut().entry(digest).or_insert(addr);
    }

    /// Records that a retention degraded to accounting under budget exhaustion.
    pub(crate) fn mark_degraded(&self, bytes: u64) {
        self.degraded.set(true);
        self.degraded_bytes
            .set(self.degraded_bytes.get().saturating_add(bytes));
        self.degraded_count
            .set(self.degraded_count.get().saturating_add(1));
    }

    /// Returns whether any retention degraded to accounting.
    pub(crate) fn is_degraded(&self) -> bool {
        self.degraded.get()
    }

    /// Returns how many spans degraded to accounting.
    pub(crate) fn degraded_count(&self) -> u64 {
        self.degraded_count.get()
    }

    /// Returns the total bytes that degraded to accounting.
    pub(crate) fn degraded_bytes(&self) -> u64 {
        self.degraded_bytes.get()
    }

    /// Returns the retained blob addresses in canonical (id-ascending) order.
    pub(crate) fn addrs(&self) -> Vec<(String, RetainedAddr)> {
        self.blobs
            .borrow()
            .iter()
            .map(|(digest, &addr)| (digest.clone(), addr))
            .collect()
    }
}
