// SPDX-License-Identifier: Apache-2.0
//! Scoped, collision-safe lookup of borrowed identity text.

use cadmpeg_core::CodecError;

pub(crate) struct BorrowedIdentities<'ir, T = ()> {
    values: Vec<(u64, &'ir str, T)>,
}

impl<'ir, T> BorrowedIdentities<'ir, T> {
    pub(crate) fn build(
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
        Ok(Self { values })
    }

    pub(crate) fn contains(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    /// Return the last inserted value with the complete identity.
    pub(crate) fn get(&self, id: &str) -> Option<&T> {
        let (_, _, found, _) = self.position(id);
        found.map(|position| &self.values[position].2)
    }

    /// Return a value only when the complete identity occurs once.
    pub(crate) fn get_unique(&self, id: &str) -> Option<&T> {
        let (_, _, found, count) = self.position(id);
        if count == 1 {
            found.map(|position| &self.values[position].2)
        } else {
            None
        }
    }

    /// Insert one absent identity, using the context that owns the storage.
    pub(crate) fn insert_unique(&mut self, id: &'ir str, value: T) -> bool {
        let (hash, low, found, _) = self.position(id);
        if found.is_some() {
            return false;
        }
        self.insert_at(hash, low, id, value);
        true
    }

    /// Replace the last matching value or admit a new identity slot.
    pub(crate) fn insert(&mut self, id: &'ir str, value: T) -> Option<T> {
        let (hash, low, found, _) = self.position(id);
        if let Some(found) = found {
            return Some(std::mem::replace(&mut self.values[found].2, value));
        }
        self.insert_at(hash, low, id, value);
        None
    }

    /// Remove the last matching slot after admitting element movement.
    pub(crate) fn remove(&mut self, id: &str) -> Option<T> {
        let (_, _, found, _) = self.position(id);
        let found = found?;

        Some(self.values.remove(found).2)
    }

    fn insert_at(&mut self, hash: u64, low: usize, id: &'ir str, value: T) {
        self.values.insert(low, (hash, id, value));
    }

    pub(crate) fn get_mut(&mut self, id: &str) -> Option<&mut T> {
        let (_, _, found, _) = self.position(id);
        found.map(|position| &mut self.values[position].2)
    }

    pub(crate) fn match_count(&self, id: &str) -> usize {
        self.position(id).3
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

    fn position(&self, id: &str) -> (u64, usize, std::option::Option<usize>, usize) {
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
            if *text == id {
                found = Some(low + offset);
                count += 1;
            }
        }
        (hash, low, found, count)
    }
}

impl<'ir> BorrowedIdentities<'ir> {
    pub(crate) fn extend_unique(&mut self, identities: impl IntoIterator<Item = &'ir str>) {
        for identity in identities {
            self.insert_unique(identity, ());
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn borrowed_identity_lookup_checks_full_text_after_hash_collision() {
        let mut index =
            super::BorrowedIdentities::build(|add| add("test:native:record#stored", ())).unwrap();
        let query = "test:native:record#missing";
        index.values[0].0 = crate::index::identity_hash(query);
        assert!(!index.contains(query));
        index.values[0].0 = crate::index::identity_hash("test:native:record#stored");
        assert!(index.contains("test:native:record#stored"));
    }
    #[test]
    fn borrowed_identity_values_preserve_last_duplicate_and_full_collision_comparison() {
        let mut index = super::BorrowedIdentities::build(|add| {
            add("test:model:point#same", 1)?;
            add("test:model:point#other", 2)?;
            add("test:model:point#same", 3)
        })
        .unwrap();
        assert_eq!(index.get("test:model:point#same"), Some(&3));
        assert_eq!(index.get("test:model:point#other"), Some(&2));
        let query = "test:model:point#absent";
        for value in &mut index.values {
            value.0 = crate::index::identity_hash(query);
        }
        assert_eq!(index.get(query), None);
    }

    #[test]
    fn borrowed_identity_unique_lookup_distinguishes_duplicates_and_collisions() {
        let mut index = super::BorrowedIdentities::build(|add| {
            add("test:model:point#duplicate", 1)?;
            add("test:model:point#unique", 2)?;
            add("test:model:point#duplicate", 3)
        })
        .unwrap();
        assert_eq!(index.get_unique("test:model:point#duplicate"), None);
        assert_eq!(index.get_unique("test:model:point#unique"), Some(&2));
        assert_eq!(index.get("test:model:point#duplicate"), Some(&3));
        let missing = "test:model:point#missing";
        for value in &mut index.values {
            value.0 = crate::index::identity_hash(missing);
        }
        assert_eq!(index.get_unique(missing), None);
    }

    #[test]
    fn borrowed_identity_replacement_mutation_and_counts_keep_shared_lookup_semantics() {
        let id = "test:model:point#same";
        let mut index = super::BorrowedIdentities::build(|add| {
            add(id, 1)?;
            add(id, 2)
        })
        .unwrap();
        assert_eq!(index.match_count(id), 2);
        assert_eq!(index.insert(id, 3), Some(2));
        *index.get_mut(id).unwrap() = 4;
        assert_eq!(index.get(id), Some(&4));
        assert_eq!(index.values().copied().collect::<Vec<_>>(), [1, 4]);
        assert_eq!(index.len(), 2);
        assert_eq!(index.insert("test:model:point#new", 5), None);
        assert_eq!(index.match_count("test:model:point#new"), 1);
        assert_eq!(index.match_count("test:model:point#missing"), 0);
        assert_eq!(index.len(), 3);
    }

    #[test]
    fn borrowed_identity_remove_keeps_last_duplicate_and_absence_semantics() {
        let id = "test:model:point#same";
        let mut index = super::BorrowedIdentities::build(|add| {
            add(id, 1)?;
            add(id, 2)
        })
        .unwrap();
        assert_eq!(index.remove(id), Some(2));
        assert_eq!(index.get(id), Some(&1));
        assert_eq!(index.remove(id), Some(1));
        assert_eq!(index.remove(id), None);
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn borrowed_identity_set_keeps_one_slot_per_identity() {
        let mut index = super::BorrowedIdentities::build(|_| Ok(())).unwrap();
        index.extend_unique([
            "test:model:point#first",
            "test:model:point#second",
            "test:model:point#first",
        ]);
        assert_eq!(index.values.len(), 2);
        assert!(!index.insert_unique("test:model:point#second", ()));
        assert!(index.contains("test:model:point#first"));
        assert!(index.contains("test:model:point#second"));
        assert!(!index.contains("test:model:point#missing"));
    }
}
