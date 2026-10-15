// SPDX-License-Identifier: Apache-2.0
//! Scoped, collision-safe lookup of borrowed identity text.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(crate) struct BorrowedIdentities<'ctx, 'ir, T = ()> {
    values: Vec<(u64, &'ir str, T)>,
    ctx: &'ctx DecodeContext<'ctx>,
}

impl<'ctx, 'ir, T> BorrowedIdentities<'ctx, 'ir, T> {
    pub(crate) fn build(
        ctx: &'ctx DecodeContext<'_>,
        visit: impl FnOnce(
            &mut dyn FnMut(&'ir str, T) -> Result<(), CodecError>,
        ) -> Result<(), CodecError>,
    ) -> Result<Self, CodecError> {
        let mut values = Vec::new();
        visit(&mut |id, value| {
            values.push((crate::index::identity_hash(id), id, value));
            Ok(())
        })?;
        // A stable sort keeps insertion order among equal hashes.
        values.sort_by_key(|value| value.0);
        Ok(Self { values, ctx })
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
    pub(crate) fn get_unique(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<Option<&T>, CodecError> {
        let (_, _, found, count) = self.position(ctx, id)?;
        Ok(if count == 1 {
            found.map(|position| &self.values[position].2)
        } else {
            None
        })
    }

    /// Insert one absent identity, using the context that owns the storage.
    pub(crate) fn insert_unique(&mut self, id: &'ir str, value: T) -> Result<bool, CodecError> {
        let (hash, low, found, _) = self.position(self.ctx, id)?;
        if found.is_some() {
            return Ok(false);
        }
        self.insert_at(hash, low, id, value);
        Ok(true)
    }

    /// Replace the last matching value or admit a new identity slot.
    pub(crate) fn insert(&mut self, id: &'ir str, value: T) -> Result<Option<T>, CodecError> {
        let (hash, low, found, _) = self.position(self.ctx, id)?;
        if let Some(found) = found {
            return Ok(Some(std::mem::replace(&mut self.values[found].2, value)));
        }
        self.insert_at(hash, low, id, value);
        Ok(None)
    }

    /// Remove the last matching slot after admitting element movement.
    pub(crate) fn remove(&mut self, id: &str) -> Result<Option<T>, CodecError> {
        let (_, _, found, _) = self.position(self.ctx, id)?;
        let Some(found) = found else {
            return Ok(None);
        };

        Ok(Some(self.values.remove(found).2))
    }

    fn insert_at(&mut self, hash: u64, low: usize, id: &'ir str, value: T) {
        self.values.insert(low, (hash, id, value));
    }

    pub(crate) fn get_mut(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<Option<&mut T>, CodecError> {
        let (_, _, found, _) = self.position(ctx, id)?;
        Ok(found.map(|position| &mut self.values[position].2))
    }

    pub(crate) fn match_count(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<usize, CodecError> {
        Ok(self.position(ctx, id)?.3)
    }

    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &T> {
        self.values.iter().map(|(_, _, value)| value)
    }

    pub(crate) fn identities(&self) -> impl Iterator<Item = &'ir str> + '_ {
        self.values.iter().map(|(_, identity, _)| *identity)
    }

    fn position(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<(u64, usize, Option<usize>, usize), CodecError> {
        let hash = crate::index::identity_hash(id);
        let mut low = 0;
        let mut high = self.values.len();
        while low < high {
            let middle = low + (high - low) / 2;
            if self.values[middle].0 < hash {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let mut found = None;
        let mut count = 0;
        for (offset, (candidate, text, _)) in self.values[low..].iter().enumerate() {
            if *candidate != hash {
                break;
            }
            if crate::ids::comparison::equal(ctx, text, id, "compare validation identity")? {
                found = Some(low + offset);
                count += 1;
            }
        }
        Ok((hash, low, found, count))
    }
}

impl<'ir> BorrowedIdentities<'_, 'ir> {
    pub(crate) fn extend_unique(
        &mut self,
        identities: impl IntoIterator<Item = &'ir str>,
    ) -> Result<(), CodecError> {
        for identity in identities {
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
        let mut index =
            super::BorrowedIdentities::build(&ctx, |add| add("test:native:record#stored", ()))
                .unwrap();
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
        })
        .unwrap();
        assert_eq!(index.get(&ctx, "test:model:point#same").unwrap(), Some(&3));
        assert_eq!(index.get(&ctx, "test:model:point#other").unwrap(), Some(&2));
        let query = "test:model:point#absent";
        for value in &mut index.values {
            value.0 = crate::index::identity_hash(query);
        }
        assert_eq!(index.get(&ctx, query).unwrap(), None);
    }

    #[test]
    fn borrowed_identity_unique_lookup_distinguishes_duplicates_and_collisions() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut index = super::BorrowedIdentities::build(&ctx, |add| {
            add("test:model:point#duplicate", 1)?;
            add("test:model:point#unique", 2)?;
            add("test:model:point#duplicate", 3)
        })
        .unwrap();
        assert_eq!(
            index
                .get_unique(&ctx, "test:model:point#duplicate")
                .unwrap(),
            None
        );
        assert_eq!(
            index.get_unique(&ctx, "test:model:point#unique").unwrap(),
            Some(&2)
        );
        assert_eq!(
            index.get(&ctx, "test:model:point#duplicate").unwrap(),
            Some(&3)
        );
        let missing = "test:model:point#missing";
        for value in &mut index.values {
            value.0 = crate::index::identity_hash(missing);
        }
        assert_eq!(index.get_unique(&ctx, missing).unwrap(), None);
    }

    #[test]
    fn borrowed_identity_replacement_mutation_and_counts_keep_shared_lookup_semantics() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let id = "test:model:point#same";
        let mut index = super::BorrowedIdentities::build(&ctx, |add| {
            add(id, 1)?;
            add(id, 2)
        })
        .unwrap();
        assert_eq!(index.match_count(&ctx, id).unwrap(), 2);
        assert_eq!(index.insert(id, 3).unwrap(), Some(2));
        *index.get_mut(&ctx, id).unwrap().unwrap() = 4;
        assert_eq!(index.get(&ctx, id).unwrap(), Some(&4));
        assert_eq!(index.values().copied().collect::<Vec<_>>(), [1, 4]);
        assert_eq!(index.len(), 2);
        assert_eq!(index.insert("test:model:point#new", 5).unwrap(), None);
        assert_eq!(index.match_count(&ctx, "test:model:point#new").unwrap(), 1);
        assert_eq!(
            index.match_count(&ctx, "test:model:point#missing").unwrap(),
            0
        );
        assert_eq!(index.len(), 3);
    }

    #[test]
    fn borrowed_identity_remove_keeps_last_duplicate_and_absence_semantics() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let id = "test:model:point#same";
        let mut index = super::BorrowedIdentities::build(&ctx, |add| {
            add(id, 1)?;
            add(id, 2)
        })
        .unwrap();
        assert_eq!(index.remove(id).unwrap(), Some(2));
        assert_eq!(index.get(&ctx, id).unwrap(), Some(&1));
        assert_eq!(index.remove(id).unwrap(), Some(1));
        assert_eq!(index.remove(id).unwrap(), None);
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn borrowed_identity_set_keeps_one_slot_per_identity() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut index = super::BorrowedIdentities::build(&ctx, |_| Ok(())).unwrap();
        index
            .extend_unique([
                "test:model:point#first",
                "test:model:point#second",
                "test:model:point#first",
            ])
            .unwrap();
        assert_eq!(index.values.len(), 2);
        assert!(!index.insert_unique("test:model:point#second", ()).unwrap());
        assert!(index.contains(&ctx, "test:model:point#first").unwrap());
        assert!(index.contains(&ctx, "test:model:point#second").unwrap());
        assert!(!index.contains(&ctx, "test:model:point#missing").unwrap());
    }
}
