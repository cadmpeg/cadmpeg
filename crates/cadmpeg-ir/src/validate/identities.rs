// SPDX-License-Identifier: Apache-2.0
//! Scoped, collision-safe lookup of borrowed identity text.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

pub(super) struct BorrowedIdentities<'ctx, 'ir, T = ()> {
    values: Vec<(u64, &'ir str, T)>,
    _storage: ScopedReservation<'ctx>,
}

impl<'ctx, 'ir, T> BorrowedIdentities<'ctx, 'ir, T> {
    pub(super) fn build(
        ctx: &'ctx DecodeContext<'_>,
        visit: impl FnOnce(&mut dyn FnMut(&'ir str, T) -> Result<(), CodecError>) -> Result<(), CodecError>,
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "borrowed validation identities")?;
        let mut values = Vec::new();
        visit(&mut |id, value| {
            ctx.charge_work(1, "validation identity record scan")?;
            ctx.charge_work(u64_from_index(id.len()), "hash validation identity")?;
            let hash = crate::index::identity_hash(id);
            storage.with_storage(|| ctx.push_retained_vec(&mut values, (hash, id, value), "borrowed validation identity slots"))
        })?;
        ctx.stable_sort_by(&mut values, |left, right| left.0.cmp(&right.0), |_| 1, "sort validation identity hashes")?;
        Ok(Self { values, _storage: storage })
    }

    pub(super) fn contains(&self, ctx: &DecodeContext<'_>, id: &str) -> Result<bool, CodecError> {
        Ok(self.get(ctx, id)?.is_some())
    }

    /// Return the last inserted value with the complete identity.
    pub(super) fn get(&self, ctx: &DecodeContext<'_>, id: &str) -> Result<Option<&T>, CodecError> {
        ctx.charge_work(u64_from_index(id.len()), "hash validation identity lookup")?;
        let hash = crate::index::identity_hash(id);
        let mut low = 0;
        let mut high = self.values.len();
        while low < high {
            ctx.charge_work(1, "search validation identity hashes")?;
            let middle = low + (high - low) / 2;
            if self.values[middle].0 < hash { low = middle + 1; } else { high = middle; }
        }
        let mut found = None;
        for (candidate, text, value) in &self.values[low..] {
            ctx.charge_work(1, "search validation identity collision")?;
            if *candidate != hash { break; }
            ctx.charge_work(u64_from_index(id.len()), "compare validation identity")?;
            if *text == id { found = Some(value); }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn borrowed_identity_lookup_checks_full_text_after_hash_collision() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut index = super::BorrowedIdentities::build(&ctx, |add| add("test:native:record#stored", ())).unwrap();
        let query = "test:native:record#missing";
        index.values[0].0 = crate::index::identity_hash(query);
        assert!(!index.contains(&ctx, query).unwrap());
        index.values[0].0 = crate::index::identity_hash("test:native:record#stored");
        assert!(index.contains(&ctx, "test:native:record#stored").unwrap());
    }
    #[test]
    fn borrowed_identity_values_preserve_last_duplicate_and_full_collision_comparison() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut index = super::BorrowedIdentities::build(&ctx, |add| {
            add("test:model:point#same", 1)?;
            add("test:model:point#other", 2)?;
            add("test:model:point#same", 3)
        }).unwrap();
        assert_eq!(index.get(&ctx, "test:model:point#same").unwrap(), Some(&3));
        assert_eq!(index.get(&ctx, "test:model:point#other").unwrap(), Some(&2));
        let query = "test:model:point#absent";
        for value in &mut index.values { value.0 = crate::index::identity_hash(query); }
        assert_eq!(index.get(&ctx, query).unwrap(), None);
    }

    #[test]
    fn borrowed_identity_value_lookup_preserves_hash_and_comparison_refusals() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let fixture = cadmpeg_test_support::service_decode_context();
        let query = "test:model:point#query";
        let mut index = super::BorrowedIdentities::build(&fixture, |add| add("test:model:point#stored", 7)).unwrap();
        index.values[0].0 = crate::index::identity_hash(query);
        for (cap, operation) in [(0, "hash validation identity lookup"),
            (cadmpeg_core::decode::u64_from_index(query.len()) + 2, "compare validation identity")] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let Err(CodecError::ResourceLimit(limit)) = index.get(&ctx, query) else { panic!("lookup must refuse"); };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, operation);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        }
    }

}
