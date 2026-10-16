// SPDX-License-Identifier: Apache-2.0
//! Scoped lookup of borrowed identity text.

use std::collections::{hash_map::Entry, HashMap};

use cadmpeg_core::CodecError;

pub(crate) struct BorrowedIdentities<'ir, T = ()> {
    slots: HashMap<&'ir str, Slots<T>>,
    /// Number of slots, counting every duplicate of an identity.
    len: usize,
}

/// The values stored under one identity, in insertion order.
struct Slots<T> {
    first: T,
    /// Duplicate slots after `first`; empty, and unallocated, for a unique identity.
    later: Vec<T>,
}

impl<T> Slots<T> {
    fn new(first: T) -> Self {
        Self {
            first,
            later: Vec::new(),
        }
    }

    fn count(&self) -> usize {
        1 + self.later.len()
    }

    fn last(&self) -> &T {
        self.later.last().unwrap_or(&self.first)
    }

    fn last_mut(&mut self) -> &mut T {
        self.later.last_mut().unwrap_or(&mut self.first)
    }

    fn iter(&self) -> impl Iterator<Item = &T> {
        std::iter::once(&self.first).chain(&self.later)
    }
}

impl<'ir, T> BorrowedIdentities<'ir, T> {
    pub(crate) fn build(
        visit: impl FnOnce(
            &mut dyn FnMut(&'ir str, T) -> Result<(), CodecError>,
        ) -> Result<(), CodecError>,
    ) -> Result<Self, CodecError> {
        let mut index = Self {
            slots: HashMap::new(),
            len: 0,
        };
        visit(&mut |id, value| {
            match index.slots.entry(id) {
                Entry::Vacant(entry) => {
                    entry.insert(Slots::new(value));
                }
                Entry::Occupied(mut entry) => entry.get_mut().later.push(value),
            }
            index.len += 1;
            Ok(())
        })?;
        Ok(index)
    }

    pub(crate) fn contains(&self, id: &str) -> bool {
        self.slots.contains_key(id)
    }

    /// Return the last inserted value with the complete identity.
    pub(crate) fn get(&self, id: &str) -> Option<&T> {
        self.slots.get(id).map(Slots::last)
    }

    /// Return a value only when the complete identity occurs once.
    pub(crate) fn get_unique(&self, id: &str) -> Option<&T> {
        self.slots
            .get(id)
            .filter(|slots| slots.later.is_empty())
            .map(|slots| &slots.first)
    }

    /// Insert one absent identity.
    pub(crate) fn insert_unique(&mut self, id: &'ir str, value: T) -> bool {
        match self.slots.entry(id) {
            Entry::Occupied(_) => false,
            Entry::Vacant(entry) => {
                entry.insert(Slots::new(value));
                self.len += 1;
                true
            }
        }
    }

    /// Replace the last matching value or admit a new identity slot.
    pub(crate) fn insert(&mut self, id: &'ir str, value: T) -> Option<T> {
        match self.slots.entry(id) {
            Entry::Occupied(mut entry) => {
                Some(std::mem::replace(entry.get_mut().last_mut(), value))
            }
            Entry::Vacant(entry) => {
                entry.insert(Slots::new(value));
                self.len += 1;
                None
            }
        }
    }

    /// Remove the last matching slot, removing the identity when no slots remain.
    pub(crate) fn remove(&mut self, id: &str) -> Option<T> {
        let slots = self.slots.get_mut(id)?;
        self.len -= 1;
        match slots.later.pop() {
            Some(value) => Some(value),
            None => self.slots.remove(id).map(|slots| slots.first),
        }
    }

    pub(crate) fn get_mut(&mut self, id: &str) -> Option<&mut T> {
        self.slots.get_mut(id).map(Slots::last_mut)
    }

    pub(crate) fn match_count(&self, id: &str) -> usize {
        self.slots.get(id).map_or(0, Slots::count)
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Yield one value per slot in unspecified iteration order.
    pub(crate) fn values(&self) -> impl Iterator<Item = &T> {
        self.slots.values().flat_map(Slots::iter)
    }

    /// Yield one identity per slot, including duplicates, in unspecified iteration order.
    pub(crate) fn identities(&self) -> impl Iterator<Item = &'ir str> + '_ {
        self.slots
            .iter()
            .flat_map(|(id, slots)| std::iter::repeat_n(*id, slots.count()))
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
    fn borrowed_identity_values_preserve_last_duplicate() {
        let index = super::BorrowedIdentities::build(|add| {
            add("test:model:point#same", 1)?;
            add("test:model:point#other", 2)?;
            add("test:model:point#same", 3)
        })
        .unwrap();
        assert_eq!(index.get("test:model:point#same"), Some(&3));
        assert_eq!(index.get("test:model:point#other"), Some(&2));
        let query = "test:model:point#absent";
        assert_eq!(index.get(query), None);
    }

    #[test]
    fn borrowed_identity_unique_lookup_distinguishes_duplicates() {
        let index = super::BorrowedIdentities::build(|add| {
            add("test:model:point#duplicate", 1)?;
            add("test:model:point#unique", 2)?;
            add("test:model:point#duplicate", 3)
        })
        .unwrap();
        assert_eq!(index.get_unique("test:model:point#duplicate"), None);
        assert_eq!(index.get_unique("test:model:point#unique"), Some(&2));
        assert_eq!(index.get("test:model:point#duplicate"), Some(&3));
        let missing = "test:model:point#missing";
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
        assert_eq!(index.len(), 2);
        assert!(!index.insert_unique("test:model:point#second", ()));
        assert!(index.contains("test:model:point#first"));
        assert!(index.contains("test:model:point#second"));
        assert!(!index.contains("test:model:point#missing"));
    }

    #[test]
    fn borrowed_identity_large_incremental_index_preserves_duplicate_slots() {
        const COUNT: usize = 200_000;
        let ids: Vec<_> = (0..COUNT)
            .map(|index| format!("test:model:point#{index}"))
            .collect();
        let mut index = super::BorrowedIdentities::build(|_| Ok(())).unwrap();
        for (value, id) in ids.iter().enumerate() {
            assert!(index.insert_unique(id, value));
        }
        assert_eq!(index.len(), COUNT);
        for (value, id) in ids.iter().enumerate() {
            assert_eq!(index.match_count(id), 1);
            assert_eq!(index.get(id), Some(&value));
            assert_eq!(index.get_unique(id), Some(&value));
        }
        assert!(!index.insert_unique(&ids[42], COUNT));
        assert_eq!(index.len(), COUNT);
        assert_eq!(index.remove(&ids[42]), Some(42));
        assert_eq!(index.match_count(&ids[42]), 0);
        assert_eq!(index.len(), COUNT - 1);

        let duplicates = [0, 42, COUNT - 1];
        let mut index = super::BorrowedIdentities::build(|add| {
            for (value, id) in ids.iter().enumerate() {
                add(id, value)?;
            }
            for value in duplicates {
                add(&ids[value], COUNT + value)?;
                add(&ids[value], 2 * COUNT + value)?;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(index.len(), COUNT + 6);
        assert_eq!(index.values().count(), COUNT + 6);
        assert_eq!(index.identities().count(), COUNT + 6);
        assert_eq!(index.get_unique(&ids[1]), Some(&1));
        for value in duplicates {
            let id = ids[value].as_str();
            assert_eq!(index.match_count(id), 3);
            assert_eq!(index.get(id), Some(&(2 * COUNT + value)));
            assert_eq!(index.get_unique(id), None);
            assert!(!index.insert_unique(id, COUNT));
            assert_eq!(index.insert(id, 3 * COUNT + value), Some(2 * COUNT + value));
            assert_eq!(index.remove(id), Some(3 * COUNT + value));
            assert_eq!(index.match_count(id), 2);
            assert_eq!(index.get(id), Some(&(COUNT + value)));
            assert_eq!(index.remove(id), Some(COUNT + value));
            assert_eq!(index.match_count(id), 1);
            assert_eq!(index.get_unique(id), Some(&value));
            assert_eq!(index.remove(id), Some(value));
            assert_eq!(index.remove(id), None);
            assert_eq!(index.match_count(id), 0);
            assert_eq!(index.get(id), None);
            assert_eq!(index.get_unique(id), None);
        }
        assert_eq!(index.len(), COUNT - 3);
    }
}
