// SPDX-License-Identifier: Apache-2.0
//! Content-addressed opaque blobs and their subrange references.

use std::cell::RefCell;
use std::collections::BTreeMap;

/// Lowercase-hex SHA-256 identity of a retained blob.
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

    /// Narrows this range to a contained `[start, end)` subrange.
    pub fn subrange(&self, start: u64, end: u64) -> Option<RetainedRange> {
        (self.start <= start && start <= end && end <= self.end).then(|| RetainedRange {
            blob: self.blob.clone(),
            start,
            end,
        })
    }
}

/// The outcome of a retention request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retention {
    /// The bytes were retained; the range recovers them from the blob.
    Retained(RetainedRange),
    /// The retained-byte budget was exhausted in salvage mode: the digest is
    /// kept for accounting, the bytes are not retained, and recovery for this
    /// span is unavailable.
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
    /// Digests that degraded to accounting, keyed by digest with the blob's byte
    /// length. Keying by digest reconciles degradation against what is actually
    /// retained: a digest is recorded here at most once regardless of how many
    /// opaque spans reference it, and a later successful [`insert`](Self::insert)
    /// of the same digest removes it, so a blob retained after an early
    /// over-budget attempt is not falsely reported as lost.
    degraded: RefCell<BTreeMap<String, u64>>,
}

impl RetainedStore {
    /// Returns whether a blob with `digest` is already retained.
    pub(crate) fn contains(&self, digest: &str) -> bool {
        self.blobs.borrow().contains_key(digest)
    }

    /// Inserts a retained blob, keyed by its digest. Idempotent: re-inserting a
    /// present digest keeps the existing address. Reconciles degradation: if the
    /// digest was earlier marked degraded (an over-budget attempt before the
    /// retained allowance grew), retaining it now clears that record so it is not
    /// reported as lost.
    pub(crate) fn insert(&self, digest: String, addr: RetainedAddr) {
        self.degraded.borrow_mut().remove(&digest);
        self.blobs.borrow_mut().entry(digest).or_insert(addr);
    }

    /// Records that a retention degraded to accounting under budget exhaustion,
    /// keyed by digest so repeated attempts on the same blob count once.
    pub(crate) fn mark_degraded(&self, digest: &str, bytes: u64) {
        self.degraded
            .borrow_mut()
            .entry(digest.to_owned())
            .or_insert(bytes);
    }

    /// Returns whether any retention degraded to accounting and was not later
    /// reconciled by a successful retention of the same blob.
    pub(crate) fn is_degraded(&self) -> bool {
        !self.degraded.borrow().is_empty()
    }

    /// Returns how many unique blobs degraded to accounting.
    pub(crate) fn degraded_count(&self) -> u64 {
        self.degraded.borrow().len() as u64
    }

    /// Returns the total bytes of unique blobs that degraded to accounting.
    pub(crate) fn degraded_bytes(&self) -> u64 {
        self.degraded.borrow().values().sum()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn addr_of(bytes: &[u8]) -> RetainedAddr {
        RetainedAddr {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        }
    }

    #[test]
    fn later_retention_reconciles_earlier_degradation() {
        let store = RetainedStore::default();
        let bytes = [7u8; 16];
        store.mark_degraded("digest-a", 16);
        assert!(store.is_degraded());
        assert_eq!(store.degraded_count(), 1);
        assert_eq!(store.degraded_bytes(), 16);
        store.insert("digest-a".to_owned(), addr_of(&bytes));
        assert!(!store.is_degraded());
        assert_eq!(store.degraded_count(), 0);
        assert_eq!(store.degraded_bytes(), 0);
    }

    #[test]
    fn repeated_degradation_of_one_blob_counts_once() {
        let store = RetainedStore::default();
        store.mark_degraded("digest-b", 32);
        store.mark_degraded("digest-b", 32);
        store.mark_degraded("digest-b", 32);
        assert_eq!(store.degraded_count(), 1);
        assert_eq!(store.degraded_bytes(), 32);
    }

    #[test]
    fn distinct_degraded_blobs_sum_independently() {
        let store = RetainedStore::default();
        store.mark_degraded("digest-c", 10);
        store.mark_degraded("digest-d", 20);
        assert_eq!(store.degraded_count(), 2);
        assert_eq!(store.degraded_bytes(), 30);
    }
}
