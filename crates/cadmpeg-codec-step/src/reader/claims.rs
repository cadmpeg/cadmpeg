// SPDX-License-Identifier: Apache-2.0
//! Source claims collected by decode stages.

use std::collections::HashSet;

pub(super) trait SourceClaims {
    fn contains(&self, id: u64) -> bool;
    fn remove(&mut self, id: u64) -> bool;
    fn retain(&mut self, keep: &mut dyn FnMut(&u64) -> bool);
}

impl SourceClaims for HashSet<u64> {
    fn contains(&self, id: u64) -> bool {
        HashSet::contains(self, &id)
    }
    fn remove(&mut self, id: u64) -> bool {
        HashSet::remove(self, &id)
    }
    fn retain(&mut self, keep: &mut dyn FnMut(&u64) -> bool) {
        HashSet::retain(self, keep);
    }
}

#[derive(Default)]
pub(super) struct ClaimSets {
    sets: Vec<HashSet<u64>>,
}

impl ClaimSets {
    pub(super) fn admit(&mut self, claims: HashSet<u64>) {
        if !claims.is_empty() {
            self.sets.push(claims);
        }
    }
}

impl SourceClaims for ClaimSets {
    fn contains(&self, id: u64) -> bool {
        self.sets.iter().any(|set| set.contains(&id))
    }
    fn remove(&mut self, id: u64) -> bool {
        let mut removed = false;
        for set in &mut self.sets {
            removed |= set.remove(&id);
        }
        removed
    }
    fn retain(&mut self, keep: &mut dyn FnMut(&u64) -> bool) {
        for set in &mut self.sets {
            set.retain(&mut *keep);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ClaimSets, SourceClaims};

    #[test]
    fn stage_storage_transfers_without_copying_claims_and_withdraws_every_copy() {
        let first = (0..1000).collect();
        let second = (500..1500).collect();
        let mut claims = ClaimSets::default();
        claims.admit(first);
        claims.admit(second);
        for id in 0..1500 {
            assert!(claims.contains(id));
        }
        assert!(!claims.contains(1500));
        assert!(claims.remove(750));
        assert!(!claims.contains(750));
        claims.retain(&mut |id| id % 2 == 0);
        assert!(claims.contains(748));
        assert!(!claims.contains(749));
        assert!(!claims.contains(750));
    }
}
