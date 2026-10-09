// SPDX-License-Identifier: Apache-2.0
//! Identity positions for append-only model arenas.

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ModelIdentityMatch {
    Absent,
    Unique(usize),
    Duplicate,
}

impl ModelIdentityMatch {
    pub(super) fn exists(self) -> bool {
        !matches!(self, Self::Absent)
    }

    pub(super) fn unique_position(self) -> Option<usize> {
        match self {
            Self::Unique(position) => Some(position),
            Self::Absent | Self::Duplicate => None,
        }
    }
}

/// A unique position or a duplicate marker for each identity already indexed.
/// New arena records are indexed once, in arena order. Queries never visit hash order.
pub(super) struct ModelIdentityIndex<'ctx> {
    positions: HashMap<String, (usize, usize)>,
    indexed_len: usize,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx> ModelIdentityIndex<'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            positions: HashMap::new(),
            indexed_len: 0,
            storage: ctx.reserve_scoped(0, "creo model identity index storage")?,
        })
    }

    /// Returns absence, a unique position, or a duplicate marker.
    /// The arena must keep every indexed identity at its original position.
    pub(super) fn lookup<T>(
        &mut self,
        ctx: &DecodeContext<'_>,
        arena: &[T],
        identity: impl Fn(&T) -> &str,
        key: &str,
    ) -> Result<ModelIdentityMatch, CodecError> {
        self.update(ctx, arena, identity)?;
        Ok(
            match ctx.get_hash_map(&self.positions, key, "creo model identity index lookup")? {
                None => ModelIdentityMatch::Absent,
                Some(&(position, 1)) => ModelIdentityMatch::Unique(position),
                Some(_) => ModelIdentityMatch::Duplicate,
            },
        )
    }

    pub(super) fn count<T>(
        &mut self,
        ctx: &DecodeContext<'_>,
        arena: &[T],
        identity: impl Fn(&T) -> &str,
        key: &str,
    ) -> Result<usize, CodecError> {
        self.update(ctx, arena, identity)?;
        Ok(ctx
            .get_hash_map(&self.positions, key, "creo model identity index lookup")?
            .map_or(0, |&(_, count)| count))
    }

    fn update<T>(
        &mut self,
        ctx: &DecodeContext<'_>,
        arena: &[T],
        identity: impl Fn(&T) -> &str,
    ) -> Result<(), CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let Some(new_records) = arena.get(self.indexed_len..) else {
            return Err(CodecError::malformed("model identity index arena shrank"));
        };
        for record in ctx.admit_iter(new_records, "creo model identity index records")? {
            let key = identity(record);
            if let Some((_, count)) = ctx.get_mut_hash_map(
                &mut self.positions,
                key,
                "creo model identity index entries",
            )? {
                *count += 1;
            } else {
                self.storage.with_storage(|| {
                    let key = ctx.copy_retained_text(key, "creo model identity index keys")?;
                    ctx.insert_hash_map(
                        &mut self.positions,
                        key,
                        (self.indexed_len, 1),
                        "creo model identity index entries",
                    )?;
                    Ok::<_, CodecError>(())
                })?;
            }
            self.indexed_len += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{ModelIdentityIndex, ModelIdentityMatch};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;

    fn assert_shrunken_index_refusal(count: bool) {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let records = ["identity", "identity"];
        let mut index = ModelIdentityIndex::new(&ctx).expect("index");
        assert_eq!(
            index.lookup(&ctx, &records, |record| record, "identity").expect("index records"),
            ModelIdentityMatch::Duplicate
        );
        let calls = Cell::new(0);
        let query = |index: &mut ModelIdentityIndex<'_>| {
            if count {
                index.count(&ctx, &records[..1], |record| { calls.set(calls.get() + 1); *record }, "identity").map(|_| ())
            } else {
                index.lookup(&ctx, &records[..1], |record| { calls.set(calls.get() + 1); *record }, "identity").map(|_| ())
            }
        };
        assert!(matches!(query(&mut index), Err(CodecError::Malformed(message))
            if message == "model identity index arena shrank"));
        let original = ctx.charge_work_limit(
            policy.limits.max_work_units + 1,
            "before shrunken model identity query",
        ).expect_err("work cap");
        for _ in 0..2 {
            assert!(matches!(query(&mut index), Err(CodecError::ResourceLimit(actual))
                if actual == original));
            assert_eq!(ctx.resource_refusal(), Some(original));
        }
        assert_eq!(calls.get(), 0);
        assert_eq!(index.indexed_len, records.len());
        assert_eq!(index.positions.len(), 1);
        assert_eq!(index.positions.get("identity"), Some(&(0, 2)));
    }

    #[test]
    fn shrunken_model_identity_lookup_keeps_original_refusal() {
        assert_shrunken_index_refusal(false);
    }

    #[test]
    fn shrunken_model_identity_count_keeps_original_refusal() {
        assert_shrunken_index_refusal(true);
    }

    #[test]
    fn empty_model_identity_queries_are_free() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut index = ModelIdentityIndex::new(&ctx).expect("empty index");
        assert_eq!(index.lookup::<String>(&ctx, &[], String::as_str, "").expect("absence"),
            ModelIdentityMatch::Absent);
        assert_eq!(index.count::<String>(&ctx, &[], String::as_str, "").expect("zero count"), 0);
        assert!(ctx.resource_refusal().is_none());
    }

    #[test]
    fn model_identity_index_tracks_append_and_duplicate_positions() {
        crate::test_support::assert_work_boundaries(
            &[
                "creo model identity index records",
                "creo model identity index keys",
                "creo model identity index entries",
                "creo model identity index lookup",
            ],
            |ctx| {
                let mut records = vec!["first".to_owned(), "second".to_owned()];
                let mut index = ModelIdentityIndex::new(ctx)?;
                assert_eq!(
                    index.lookup(ctx, &records, String::as_str, "first")?,
                    ModelIdentityMatch::Unique(0)
                );
                assert_eq!(
                    index.lookup(ctx, &records, String::as_str, "absent")?,
                    ModelIdentityMatch::Absent
                );
                records.push("first".to_owned());
                assert_eq!(
                    index.lookup(ctx, &records, String::as_str, "first")?,
                    ModelIdentityMatch::Duplicate
                );
                assert_eq!(index.count(ctx, &records, String::as_str, "first")?, 2);
                assert_eq!(
                    index.lookup(ctx, &records, String::as_str, "second")?,
                    ModelIdentityMatch::Unique(1)
                );
                Ok(())
            },
        );
    }

    #[test]
    fn model_identity_index_storage_is_scoped() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let records = ["identity".to_owned()];
        let mut index = ModelIdentityIndex::new(&ctx).expect("empty index");
        assert_eq!(
            index
                .lookup(&ctx, &records, String::as_str, "identity")
                .expect("scoped index"),
            ModelIdentityMatch::Unique(0)
        );
        assert!(ctx.resource_refusal().is_none());
    }
}
