// SPDX-License-Identifier: Apache-2.0
//! Scoped, collision-safe lookup of borrowed identity text.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

pub(super) struct BorrowedIdentities<'ctx, 'ir> {
    values: Vec<(u64, &'ir str)>,
    _storage: ScopedReservation<'ctx>,
}

impl<'ctx, 'ir> BorrowedIdentities<'ctx, 'ir> {
    pub(super) fn build(
        ctx: &'ctx DecodeContext<'_>,
        visit: impl FnOnce(&mut dyn FnMut(&'ir str) -> Result<(), CodecError>) -> Result<(), CodecError>,
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "borrowed validation identities")?;
        let mut values = Vec::new();
        visit(&mut |id| {
            ctx.charge_work(u64_from_index(id.len()), "hash validation identity")?;
            let hash = crate::index::identity_hash(id);
            storage.with_storage(|| ctx.push_retained_vec(&mut values, (hash, id), "borrowed validation identity slots"))
        })?;
        ctx.sort_unstable_by(&mut values, |left, right| left.0.cmp(&right.0), |_| 1, "sort validation identity hashes")?;
        Ok(Self { values, _storage: storage })
    }

    pub(super) fn contains(&self, ctx: &DecodeContext<'_>, id: &str) -> Result<bool, CodecError> {
        ctx.charge_work(u64_from_index(id.len()), "hash validation identity lookup")?;
        let hash = crate::index::identity_hash(id);
        let mut low = 0;
        let mut high = self.values.len();
        while low < high {
            ctx.charge_work(1, "search validation identity hashes")?;
            let middle = low + (high - low) / 2;
            if self.values[middle].0 < hash { low = middle + 1; } else { high = middle; }
        }
        for (candidate, text) in &self.values[low..] {
            ctx.charge_work(1, "search validation identity collision")?;
            if *candidate != hash { break; }
            ctx.charge_work(u64_from_index(id.len()), "compare validation identity")?;
            if *text == id { return Ok(true); }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn borrowed_identity_lookup_checks_full_text_after_hash_collision() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut index = super::BorrowedIdentities::build(&ctx, |add| add("test:native:record#stored")).unwrap();
        let query = "test:native:record#missing";
        index.values[0].0 = crate::index::identity_hash(query);
        assert!(!index.contains(&ctx, query).unwrap());
        index.values[0].0 = crate::index::identity_hash("test:native:record#stored");
        assert!(index.contains(&ctx, "test:native:record#stored").unwrap());
    }
}
