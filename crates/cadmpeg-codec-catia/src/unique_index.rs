use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;

pub(super) struct UniqueIndex<K, V> {
    entries: HashMap<K, Option<V>>,
}

impl<K: Eq + Hash, V> UniqueIndex<K, V> {
    fn insert(&mut self, key: K, value: V) {
        self.entries
            .entry(key)
            .and_modify(|entry| *entry = None)
            .or_insert(Some(value));
    }

    pub(super) fn get<Q: Eq + Hash + ?Sized>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        self.entries.get(key)?.as_ref()
    }
}

impl<K: Eq + Hash, V> FromIterator<(K, V)> for UniqueIndex<K, V> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let mut index = Self {
            entries: HashMap::new(),
        };
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
        let mut index = [("one".to_string(), 1), ("two".to_string(), 2)]
            .into_iter()
            .collect::<UniqueIndex<_, _>>();
        assert_eq!(index.get("one"), Some(&1));
        index.insert("one".to_string(), 3);
        assert_eq!(index.get("one"), None);
        index.insert("one".to_string(), 4);
        assert_eq!(index.get("one"), None);
        assert_eq!(index.get("two"), Some(&2));
        assert_eq!(index.get("missing"), None);
    }
}
