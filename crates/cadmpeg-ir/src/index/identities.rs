// SPDX-License-Identifier: Apache-2.0
//! Scoped, collision-safe lookup of borrowed identity text.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

pub(crate) struct BorrowedIdentities<'ctx, 'ir, T = ()> {
    values: Vec<(u64, &'ir str, T)>,
    storage: ScopedReservation<'ctx>,
    ctx: &'ctx DecodeContext<'ctx>,
}

impl<'ctx, 'ir, T> BorrowedIdentities<'ctx, 'ir, T> {
    pub(crate) fn build(
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
        Ok(Self { values, storage, ctx })
    }

    pub(crate) fn contains(&self, ctx: &DecodeContext<'_>, id: &str) -> Result<bool, CodecError> {
        Ok(self.get(ctx, id)?.is_some())
    }

    /// Return the last inserted value with the complete identity.
    pub(crate) fn get(&self, ctx: &DecodeContext<'_>, id: &str) -> Result<Option<&T>, CodecError> {
        let (_, _, found, _) = self.position(ctx, id)?;
        Ok(found.map(|position| &self.values[position].2))
    }

    /// Return a value only when the complete identity occurs once.
    pub(crate) fn get_unique(&self, ctx: &DecodeContext<'_>, id: &str) -> Result<Option<&T>, CodecError> {
        let (_, _, found, count) = self.position(ctx, id)?;
        Ok(if count == 1 { found.map(|position| &self.values[position].2) } else { None })
    }

    /// Insert one absent identity, using the context that owns the storage.
    pub(crate) fn insert_unique(&mut self, id: &'ir str, value: T) -> Result<bool, CodecError> {
        let (hash, low, found, _) = self.position(self.ctx, id)?;
        if found.is_some() { return Ok(false); }
        self.insert_at(hash, low, id, value)?;
        Ok(true)
    }

    /// Replace the last matching value or admit a new identity slot.
    pub(crate) fn insert(&mut self, id: &'ir str, value: T) -> Result<Option<T>, CodecError> {
        let (hash, low, found, _) = self.position(self.ctx, id)?;
        if let Some(found) = found {
            return Ok(Some(std::mem::replace(&mut self.values[found].2, value)));
        }
        self.insert_at(hash, low, id, value)?;
        Ok(None)
    }

    /// Remove the last matching slot after admitting element movement.
    pub(crate) fn remove(&mut self, id: &str) -> Result<Option<T>, CodecError> {
        let (_, _, found, _) = self.position(self.ctx, id)?;
        let Some(found) = found else { return Ok(None); };
        self.ctx.charge_work(u64_from_index(self.values.len() - found - 1), "remove validation identity slot")?;
        Ok(Some(self.values.remove(found).2))
    }

    fn insert_at(&mut self, hash: u64, low: usize, id: &'ir str, value: T) -> Result<(), CodecError> {
        self.ctx.charge_work(u64_from_index(self.values.len() - low), "move validation identity slots")?;
        self.storage.with_storage(|| self.ctx.reserve_retained_vec(&mut self.values, 1, "borrowed validation identity slots"))?;
        self.values.insert(low, (hash, id, value));
        Ok(())
    }

    pub(crate) fn get_mut(&mut self, ctx: &DecodeContext<'_>, id: &str) -> Result<Option<&mut T>, CodecError> {
        let (_, _, found, _) = self.position(ctx, id)?;
        Ok(found.map(|position| &mut self.values[position].2))
    }

    pub(crate) fn match_count(&self, ctx: &DecodeContext<'_>, id: &str) -> Result<usize, CodecError> {
        Ok(self.position(ctx, id)?.3)
    }

    pub(crate) fn len(&self) -> usize { self.values.len() }

    pub(crate) fn values(&self) -> impl Iterator<Item = &T> { self.values.iter().map(|(_, _, value)| value) }

    pub(crate) fn identities(&self) -> impl Iterator<Item = &'ir str> + '_ {
        self.values.iter().map(|(_, identity, _)| *identity)
    }

    fn position(&self, ctx: &DecodeContext<'_>, id: &str) -> Result<(u64, usize, Option<usize>, usize), CodecError> {
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
        let mut count = 0;
        for (offset, (candidate, text, _)) in self.values[low..].iter().enumerate() {
            ctx.charge_work(1, "search validation identity collision")?;
            if *candidate != hash { break; }
            if crate::ids::comparison::equal(ctx, text, id, "compare validation identity")? {
                found = Some(low + offset);
                count += 1;
            }
        }
        Ok((hash, low, found, count))
    }

}

impl<'ctx, 'ir> BorrowedIdentities<'ctx, 'ir> {
    pub(crate) fn extend_unique(&mut self, identities: impl IntoIterator<Item = &'ir str>) -> Result<(), CodecError> {
        for identity in identities {
            self.ctx.charge_work(1, "extend validation identity scan")?;
            self.insert_unique(identity, ())?;
        }
        Ok(())
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

    #[test]
    fn borrowed_identity_insertion_preserves_storage_and_hash_refusals() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems, ResourceDimension::WorkUnits] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => panic!("test dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut index = super::BorrowedIdentities::build(&ctx, |_| Ok(())).unwrap();
            let Err(CodecError::ResourceLimit(limit)) = index.insert_unique("test:model:point#new", ()) else { panic!("index must refuse"); };
            assert_eq!(limit.dimension, dimension);
            assert!(index.values.is_empty());
            drop(index);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        }
    }

    #[test]
    fn borrowed_identity_insertion_refuses_movement_before_mutation() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, u64_from_index};
        use cadmpeg_core::CodecError;
        let first = "test:model:point#first";
        let second = "test:model:point#second";
        let (query, stored) = if crate::index::identity_hash(first) < crate::index::identity_hash(second) { (first, second) } else { (second, first) };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64_from_index(query.len()) + 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut index = super::BorrowedIdentities::build(&ctx, |_| Ok(())).unwrap();
        index.values.push((crate::index::identity_hash(stored), stored, ()));
        let Err(CodecError::ResourceLimit(limit)) = index.insert_unique(query, ()) else { panic!("movement must refuse"); };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "move validation identity slots");
        assert_eq!(index.identities().collect::<Vec<_>>(), [stored]);
        drop(index);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }

    #[test]
    fn borrowed_identity_unique_lookup_distinguishes_duplicates_and_collisions() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut index = super::BorrowedIdentities::build(&ctx, |add| {
            add("test:model:point#duplicate", 1)?;
            add("test:model:point#unique", 2)?;
            add("test:model:point#duplicate", 3)
        }).unwrap();
        assert_eq!(index.get_unique(&ctx, "test:model:point#duplicate").unwrap(), None);
        assert_eq!(index.get_unique(&ctx, "test:model:point#unique").unwrap(), Some(&2));
        assert_eq!(index.get(&ctx, "test:model:point#duplicate").unwrap(), Some(&3));
        let missing = "test:model:point#missing";
        for value in &mut index.values { value.0 = crate::index::identity_hash(missing); }
        assert_eq!(index.get_unique(&ctx, missing).unwrap(), None);
    }

    #[test]
    fn borrowed_identity_set_keeps_unique_slots_and_releases_scoped_storage() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 4096;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        {
            let mut index = super::BorrowedIdentities::build(&ctx, |_| Ok(())).unwrap();
            index.extend_unique(["test:model:point#first", "test:model:point#second", "test:model:point#first"]).unwrap();
            assert_eq!(index.values.len(), 2);
            assert!(!index.insert_unique("test:model:point#second", ()).unwrap());
            assert!(index.contains(&ctx, "test:model:point#first").unwrap());
            assert!(index.contains(&ctx, "test:model:point#second").unwrap());
            assert!(!index.contains(&ctx, "test:model:point#missing").unwrap());
        }
        drop(ctx.reserve_scoped(4096, "borrowed set storage released").unwrap());
        ctx.finish_session().unwrap();
    }

    #[test]
    fn borrowed_identity_replacement_mutation_and_counts_keep_shared_lookup_semantics() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let id = "test:model:point#same";
        let mut index = super::BorrowedIdentities::build(&ctx, |add| { add(id, 1)?; add(id, 2) }).unwrap();
        assert_eq!(index.match_count(&ctx, id).unwrap(), 2);
        assert_eq!(index.insert(id, 3).unwrap(), Some(2));
        *index.get_mut(&ctx, id).unwrap().unwrap() = 4;
        assert_eq!(index.get(&ctx, id).unwrap(), Some(&4));
        assert_eq!(index.values().copied().collect::<Vec<_>>(), [1, 4]);
        assert_eq!(index.len(), 2);
        assert_eq!(index.insert("test:model:point#new", 5).unwrap(), None);
        assert_eq!(index.match_count(&ctx, "test:model:point#new").unwrap(), 1);
        assert_eq!(index.match_count(&ctx, "test:model:point#missing").unwrap(), 0);
        assert_eq!(index.len(), 3);
    }

    #[test]
    fn borrowed_identity_remove_keeps_last_duplicate_and_absence_semantics() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let id = "test:model:point#same";
        let mut index = super::BorrowedIdentities::build(&ctx, |add| { add(id, 1)?; add(id, 2) }).unwrap();
        assert_eq!(index.remove(id).unwrap(), Some(2));
        assert_eq!(index.get(&ctx, id).unwrap(), Some(&1));
        assert_eq!(index.remove(id).unwrap(), Some(1));
        assert_eq!(index.remove(id).unwrap(), None);
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn borrowed_identity_lookup_admits_only_actual_collision_comparisons() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, u64_from_index};
        use cadmpeg_core::CodecError;
        let fixture = cadmpeg_test_support::service_decode_context();
        for (stored, query, comparison_work, expected) in [
            ("alpha", "longer", 1, None),
            ("alpha", "blope", 2, None),
            ("é", "ê", 3, None),
            ("same", "same", 5, Some(&7)),
            ("", "", 1, Some(&7)),
        ] {
            let mut index = super::BorrowedIdentities::build(&fixture, |add| add(stored, 7)).unwrap();
            index.values[0].0 = crate::index::identity_hash(query);
            // The hash visits the full query; the search visits one hash and
            // one collision. Equality visits only its actual compared bytes.
            let work = u64_from_index(query.len()) + 2 + comparison_work;
            for allowance in 0..=work {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = allowance;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_collection_items = 0;
                policy.limits.max_recursion_depth = 0;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = index.get(&ctx, query);
                if allowance < work {
                    let CodecError::ResourceLimit(original) = result.unwrap_err() else { panic!("lookup must refuse"); };
                    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                    assert!(matches!(index.get(&ctx, ""), Err(CodecError::ResourceLimit(limit)) if limit == original));
                    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
                } else {
                    assert_eq!(result.unwrap(), expected);
                    ctx.finish_session().unwrap();
                }
            }
        }
    }

}
