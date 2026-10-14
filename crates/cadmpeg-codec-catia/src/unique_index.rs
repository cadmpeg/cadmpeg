use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;

pub(super) struct UniqueIndex<K, V> {
    entries: HashMap<K, Option<V>>,
}

impl<K: Eq + Hash, V> UniqueIndex<K, V> {
    pub(super) fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn insert(&mut self, key: K, value: V) {
        if let Some(entry) = self.entries.get_mut(&key) {
            *entry = None;
        } else {
            self.entries.insert(key, Some(value));
        }
    }

    pub(super) fn get<Q: Eq + Hash + ?Sized>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        self.entries.get(key)?.as_ref()
    }

    pub(super) fn collect(iter: impl IntoIterator<Item = (K, V)>) -> Self {
        let mut index = Self::new();
        for (key, value) in iter {
            index.insert(key, value);
        }
        index
    }
}

#[cfg(test)]
mod tests {
    use super::UniqueIndex;

    #[test]
    fn duplicates_remain_ambiguous_after_further_insertions() {
        let mut index = crate::test_support::with_service_context(|ctx| {
            UniqueIndex::collect([("one".to_string(), 1), ("two".to_string(), 2)])
        })
        .expect("service profile admits index");
        assert_eq!(index.get("one"), Some(&1));
        crate::test_support::with_service_context(|ctx| index.insert("one".to_string(), 3))
            .expect("duplicate does not grow index");
        assert_eq!(index.get("one"), None);
        crate::test_support::with_service_context(|ctx| index.insert("one".to_string(), 4))
            .expect("duplicate remains ambiguous");
        assert_eq!(index.get("one"), None);
        assert_eq!(index.get("two"), Some(&2));
        assert_eq!(index.get("missing"), None);
    }
}
