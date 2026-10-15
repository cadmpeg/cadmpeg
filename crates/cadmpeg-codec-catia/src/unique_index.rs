use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) struct UniqueIndex<K, V> {
    entries: HashMap<K, Option<V>>,
}

impl<K: Eq + Hash + cadmpeg_core::decode::cost::DecodeCost, V> UniqueIndex<K, V> {
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
        match ctx.entry_hash_map(&mut self.entries, key, operation)? {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                *entry.get_mut() = None;
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Some(value));
            }
        }
        Ok(())
    }

    pub(super) fn get<Q: Eq + Hash + cadmpeg_core::decode::cost::DecodeCost + ?Sized>(
        &self,
        ctx: &DecodeContext<'_>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<&V>, CodecError>
    where
        K: Borrow<Q>,
    {
        Ok(ctx
            .get_hash_map(&self.entries, key, operation)?
            .and_then(Option::as_ref))
    }

    pub(super) fn collect(
        ctx: &DecodeContext<'_>,
        iter: impl IntoIterator<Item = (K, V)>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let mut index = Self::new();
        let mut input = iter.into_iter();
        while let Some((key, value)) = ctx.next_charged(&mut input, operation)? {
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
        assert_eq!(
            crate::test_support::with_service_context(|ctx| index.get(
                ctx,
                "one",
                "catia_unique_index_test_lookup"
            ))
            .expect("index lookup"),
            Some(&1)
        );
        crate::test_support::with_service_context(|ctx| {
            index.insert(ctx, "one".to_string(), 3, "catia_unique_index_test")
        })
        .expect("duplicate does not grow index");
        assert_eq!(
            crate::test_support::with_service_context(|ctx| index.get(
                ctx,
                "one",
                "catia_unique_index_test_lookup"
            ))
            .expect("index lookup"),
            None
        );
        crate::test_support::with_service_context(|ctx| {
            index.insert(ctx, "one".to_string(), 4, "catia_unique_index_test")
        })
        .expect("duplicate remains ambiguous");
        assert_eq!(
            crate::test_support::with_service_context(|ctx| index.get(
                ctx,
                "one",
                "catia_unique_index_test_lookup"
            ))
            .expect("index lookup"),
            None
        );
        assert_eq!(
            crate::test_support::with_service_context(|ctx| index.get(
                ctx,
                "two",
                "catia_unique_index_test_lookup"
            ))
            .expect("index lookup"),
            Some(&2)
        );
        assert_eq!(
            crate::test_support::with_service_context(|ctx| index.get(
                ctx,
                "missing",
                "catia_unique_index_test_lookup"
            ))
            .expect("index lookup"),
            None
        );
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
        assert_eq!(
            crate::test_support::with_service_context(|ctx| admitted.get(
                ctx,
                "one",
                "catia_unique_index_test_lookup"
            ))
            .expect("index lookup"),
            Some(&1)
        );
    }
    #[test]
    fn uniqueness_lookup_refuses_before_reading_live_and_ambiguous_keys() {
        let index = crate::test_support::with_service_context(|ctx| {
            let mut index = UniqueIndex::new();
            index.insert(ctx, "one", 1, "catia_unique_index_fixture")?;
            index.insert(ctx, "one", 2, "catia_unique_index_fixture")?;
            index.insert(ctx, "two", 3, "catia_unique_index_fixture")?;
            Ok::<_, cadmpeg_core::CodecError>(index)
        })
        .expect("fixture index");
        for key in ["one", "two", "missing"] {
            let refusal =
                crate::test_support::with_work_refusal("catia_unique_index_lookup", |ctx| {
                    let result = index.get(ctx, key, "catia_unique_index_lookup");
                    if let Err(cadmpeg_core::CodecError::ResourceLimit(ref limit)) = result {
                        assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                    }
                    result
                });
            assert!(
                matches!(refusal, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "catia_unique_index_lookup")
            );
        }
    }
}
