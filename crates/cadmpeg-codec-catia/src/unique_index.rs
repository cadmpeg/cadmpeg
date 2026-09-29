use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::resource;

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

    pub(super) fn insert(
        &mut self,
        ctx: &DecodeContext<'_>,
        key: K,
        value: V,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(entry) = self.entries.get_mut(&key) {
            *entry = None;
        } else {
            resource::insert_map(ctx, &mut self.entries, key, Some(value), operation)?;
        }
        Ok(())
    }

    pub(super) fn get<Q: Eq + Hash + ?Sized>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        self.entries.get(key)?.as_ref()
    }

    pub(super) fn collect(
        ctx: &DecodeContext<'_>,
        iter: impl IntoIterator<Item = (K, V)>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let mut index = Self::new();
        for (key, value) in iter {
            index.insert(ctx, key, value, operation)?;
        }
        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::UniqueIndex;

    #[test]
    fn duplicates_remain_ambiguous_after_further_insertions() {
        let mut index = crate::test_support::with_service_context(|ctx| {
            UniqueIndex::collect(
                ctx,
                [("one".to_string(), 1), ("two".to_string(), 2)],
                "catia_unique_index_test",
            )
        })
        .expect("service profile admits index");
        assert_eq!(index.get("one"), Some(&1));
        crate::test_support::with_service_context(|ctx| {
            index.insert(ctx, "one".to_string(), 3, "catia_unique_index_test")
        })
        .expect("duplicate does not grow index");
        assert_eq!(index.get("one"), None);
        crate::test_support::with_service_context(|ctx| {
            index.insert(ctx, "one".to_string(), 4, "catia_unique_index_test")
        })
        .expect("duplicate remains ambiguous");
        assert_eq!(index.get("one"), None);
        assert_eq!(index.get("two"), Some(&2));
        assert_eq!(index.get("missing"), None);
    }

    #[test]
    fn unique_index_refuses_unadmitted_entry() {
        let refused = crate::test_support::with_collection_limit(0, |ctx| {
            UniqueIndex::collect(ctx, [("one", 1)], "catia_unique_index_test")
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_unique_index_test")
        );
        let admitted = crate::test_support::with_service_context(|ctx| {
            UniqueIndex::collect(ctx, [("one", 1)], "catia_unique_index_test")
        })
        .expect("service profile admits index entry");
        assert_eq!(admitted.get("one"), Some(&1));
    }
}
