// SPDX-License-Identifier: Apache-2.0
//! Checked sparse membership for STEP record closures.

use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub(super) struct ClosureIds {
    blocks: BTreeMap<u64, u64>,
}

impl ClosureIds {
    pub(super) fn contains(&self, id: u64) -> bool {
        self.blocks
            .get(&(id / u64::from(u64::BITS)))
            .is_some_and(|bits| bits & (1_u64 << (id % u64::from(u64::BITS))) != 0)
    }

    pub(super) fn insert(&mut self, id: u64) -> bool {
        let block = id / u64::from(u64::BITS);
        let mask = 1_u64 << (id % u64::from(u64::BITS));

        let bits = self.blocks.entry(block).or_default();
        let inserted = *bits & mask == 0;
        *bits |= mask;
        inserted
    }
}

#[cfg(test)]
mod tests {
    use super::ClosureIds;

    #[test]
    fn membership_reports_each_identity_once() {
        let mut ids = ClosureIds::default();
        for id in 0..65 {
            assert!(ids.insert(id));
            assert!(ids.contains(id));
            assert!(!ids.insert(id));
        }
        assert!(!ids.contains(65));
    }

    #[test]
    fn sparse_and_maximum_identities_do_not_require_dense_storage() {
        let mut ids = ClosureIds::default();
        for id in [0, 63, 64, 127, u64::MAX - 1, u64::MAX] {
            assert!(ids.insert(id));
            assert!(ids.contains(id));
        }
        assert!(!ids.contains(128));
        assert!(!ids.contains(u64::MAX - 2));
    }
}
