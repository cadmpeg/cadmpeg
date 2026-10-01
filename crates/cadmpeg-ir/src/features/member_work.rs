// SPDX-License-Identifier: Apache-2.0
//! Admission of member hashes and collision comparisons before index insertion.

use std::hash::{Hash, Hasher};

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceLimit};

struct KeyBytes<'ctx, 'arena> {
    ctx: &'ctx DecodeContext<'arena>,
    bytes: u64,
    refusal: Option<ResourceLimit>,
    operation: &'static str,
}

impl Hasher for KeyBytes<'_, '_> {
    fn finish(&self) -> u64 { 0 }

    fn write(&mut self, bytes: &[u8]) {
        if self.refusal.is_some() { return; }
        if let Err(limit) = self.ctx.charge_work_limit(1, self.operation) {
            self.refusal = Some(limit);
            return;
        }
        match self.bytes.checked_add(u64_from_index(bytes.len())) {
            Some(total) => self.bytes = total,
            None => {
                if let Err(limit) = self.ctx.charge_work_limit(u64::MAX, self.operation) {
                    self.refusal = Some(limit);
                }
            }
        }
    }
}

/// Admit hashing and a full collision chain against the longest admitted key.
pub(super) fn admit_member_work<T: Hash>(
    ctx: &DecodeContext<'_>,
    member: &T,
    count: usize,
    longest: &mut u64,
    operation: &'static str,
) -> Result<(), ResourceLimit> {
    let mut key = KeyBytes { ctx, bytes: 0, refusal: None, operation };
    member.hash(&mut key);
    if let Some(limit) = key.refusal { return Err(limit); }
    *longest = (*longest).max(key.bytes);
    let work = key.bytes.checked_mul(2)
        .and_then(|hash| longest.checked_add(key.bytes).and_then(|comparison| comparison.checked_mul(u64_from_index(count))).and_then(|comparisons| hash.checked_add(comparisons)))
        .and_then(|work| work.checked_add(1));
    match work {
        Some(work) => ctx.charge_work_limit(work, operation),
        None => ctx.charge_work_limit(u64::MAX, operation),
    }
}
