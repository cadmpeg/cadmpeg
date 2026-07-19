// SPDX-License-Identifier: Apache-2.0
//! Retained-byte budget state.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

/// The outcome of a retention request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retention {
    /// The caller may retain the bytes.
    Retained {
        /// SHA-256 digest of the bytes.
        digest: String,
    },
    /// The caller must not retain the bytes because the salvage budget is exhausted.
    DigestOnly {
        /// SHA-256 digest of the bytes.
        digest: String,
    },
}

/// Digests charged or refused during one decode.
#[derive(Debug, Default)]
pub(crate) struct RetainedStore {
    retained: RefCell<BTreeSet<String>>,
    degraded: RefCell<BTreeMap<String, u64>>,
}

impl RetainedStore {
    /// Returns whether a blob with `digest` is already retained.
    pub(crate) fn contains(&self, digest: &str) -> bool {
        self.retained.borrow().contains(digest)
    }

    /// Records a successful retained-byte charge.
    pub(crate) fn insert(&self, digest: String) {
        self.degraded.borrow_mut().remove(&digest);
        self.retained.borrow_mut().insert(digest);
    }

    /// Records digest-only retention under budget exhaustion,
    /// keyed by digest so repeated attempts on the same blob count once.
    pub(crate) fn mark_degraded(&self, digest: &str, bytes: u64) {
        self.degraded
            .borrow_mut()
            .entry(digest.to_owned())
            .or_insert(bytes);
    }

    /// Returns whether any retention degraded to digest-only.
    pub(crate) fn is_degraded(&self) -> bool {
        !self.degraded.borrow().is_empty()
    }

    /// Returns how many unique blobs degraded to digest-only.
    pub(crate) fn degraded_count(&self) -> u64 {
        self.degraded.borrow().len() as u64
    }

    /// Returns the total bytes of unique blobs degraded to digest-only.
    pub(crate) fn degraded_bytes(&self) -> u64 {
        self.degraded.borrow().values().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_retention_reconciles_earlier_degradation() {
        let store = RetainedStore::default();
        store.mark_degraded("digest-a", 16);
        assert!(store.is_degraded());
        assert_eq!(store.degraded_count(), 1);
        assert_eq!(store.degraded_bytes(), 16);
        store.insert("digest-a".to_owned());
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
