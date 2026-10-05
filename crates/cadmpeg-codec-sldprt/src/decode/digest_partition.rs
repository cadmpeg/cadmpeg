// SPDX-License-Identifier: Apache-2.0
//! Move digest-only model entries into a partition and restore their source order.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

pub(super) struct DigestPartition<T> {
    original_len: usize,
    original: Vec<T>,
    kept: Vec<T>,
    excluded: Vec<T>,
    kept_positions: Vec<usize>,
}

pub(super) struct PreparedDigestPartition<'source, 'ctx, T> {
    source: &'source mut Vec<T>,
    decisions: Vec<bool>,
    _decisions: ScopedReservation<'ctx>,
    partition: DigestPartition<T>,
}

impl<T> DigestPartition<T> {
    pub(super) fn prepare<'source, 'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        source: &'source mut Vec<T>,
        mut keep: impl FnMut(&T) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<PreparedDigestPartition<'source, 'ctx, T>, CodecError> {
        let work = source
            .len()
            .checked_mul(3)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), operation)?;
        let (mut decisions, mut reservation) =
            ctx.scoped_vector_storage(source.len(), operation)?;
        let mut kept_count = 0usize;
        for item in source.iter() {
            let decision = keep(item)?;
            reservation.with_storage(|| ctx.push_vec(&mut decisions, decision, operation))?;
            if decision {
                kept_count = kept_count
                    .checked_add(1)
                    .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
            }
        }
        let excluded_count = source.len() - kept_count;
        let kept = ctx.collection_vec(kept_count, operation)?;
        let excluded = ctx.collection_vec(excluded_count, operation)?;
        let kept_positions = ctx.collection_vec(kept_count, operation)?;
        Ok(PreparedDigestPartition {
            partition: Self {
                original_len: source.len(),
                original: Vec::new(),
                kept,
                excluded,
                kept_positions,
            },
            source,
            decisions,
            _decisions: reservation,
        })
    }

    pub(super) fn take_kept(&mut self) -> Vec<T> {
        std::mem::take(&mut self.kept)
    }

    pub(super) fn restore(mut self, kept: Vec<T>) -> Result<Vec<T>, CodecError> {
        let total = kept
            .len()
            .checked_add(self.excluded.len())
            .ok_or_else(|| CodecError::malformed("invalid SLDPRT digest partition size"))?;
        if total != self.original_len || kept.len() != self.kept_positions.len() {
            return Err(CodecError::malformed(
                "invalid SLDPRT digest partition shape",
            ));
        }
        let mut kept = kept.into_iter();
        let mut excluded = self.excluded.into_iter();
        let mut positions = self.kept_positions.into_iter().peekable();
        for position in 0..total {
            let item = if positions.peek() == Some(&position) {
                positions.next();
                kept.next()
            } else {
                excluded.next()
            };
            let item =
                item.ok_or_else(|| CodecError::malformed("invalid SLDPRT digest partition order"))?;
            self.original.push(item);
        }
        Ok(self.original)
    }
}

impl<T> PreparedDigestPartition<'_, '_, T> {
    pub(super) fn move_from(mut self) -> DigestPartition<T> {
        self.partition.original = std::mem::take(self.source);
        for ((position, item), keep) in self
            .partition
            .original
            .drain(..)
            .enumerate()
            .zip(self.decisions)
        {
            if keep {
                self.partition.kept_positions.push(position);
                self.partition.kept.push(item);
            } else {
                self.partition.excluded.push(item);
            }
        }
        self.partition
    }
}

#[cfg(test)]
mod tests {
    use super::DigestPartition;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;

    #[test]
    fn digest_partition_uses_each_admitted_decision_once() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut source = (0..100).collect::<Vec<_>>();
        let original = source.clone();
        let mut calls = 0;
        let prepared = DigestPartition::prepare(
            &ctx,
            &mut source,
            |_| {
                calls += 1;
                Ok(calls == 1)
            },
            "prepare digest test",
        )
        .unwrap();
        let mut partition = prepared.move_from();
        let kept = partition.take_kept();
        assert_eq!(calls, original.len());
        assert_eq!(kept, [0]);
        assert_eq!(partition.restore(kept).unwrap(), original);
        assert!(source.is_empty());
    }

    #[test]
    fn digest_partition_refusal_preserves_the_bound_source() {
        for dimension in [0, 1, 2] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                0 => policy.limits.max_work_units = 0,
                1 => policy.limits.max_collection_items = 0,
                _ => policy.limits.max_materialized_bytes = 0,
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut source = (0..100).collect::<Vec<_>>();
            let original = source.clone();
            let error =
                DigestPartition::prepare(&ctx, &mut source, |_| Ok(true), "prepare digest refusal")
                    .err()
                    .unwrap();
            assert!(matches!(error, CodecError::ResourceLimit(_)));
            assert_eq!(source, original);
        }
    }
}
