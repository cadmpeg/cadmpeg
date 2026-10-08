// SPDX-License-Identifier: Apache-2.0
//! Identity positions for append-only model arenas.

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

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
    ) -> Result<Option<Option<usize>>, CodecError> {
        self.update(ctx, arena, identity)?;
        Ok(ctx
            .get_hash_map(&self.positions, key, "creo model identity index lookup")?
            .map(|&(position, count)| (count == 1).then_some(position)))
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
    use super::ModelIdentityIndex;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

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
                    Some(Some(0))
                );
                assert_eq!(index.lookup(ctx, &records, String::as_str, "absent")?, None);
                records.push("first".to_owned());
                assert_eq!(
                    index.lookup(ctx, &records, String::as_str, "first")?,
                    Some(None)
                );
                assert_eq!(index.count(ctx, &records, String::as_str, "first")?, 2);
                assert_eq!(
                    index.lookup(ctx, &records, String::as_str, "second")?,
                    Some(Some(1))
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
            Some(Some(0))
        );
        assert!(ctx.resource_refusal().is_none());
    }
}
