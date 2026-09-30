// SPDX-License-Identifier: Apache-2.0
//! Move digest-only model entries into a partition and restore their source order.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) struct DigestPartition<T> {
    original_len: usize,
    original: Vec<T>,
    kept: Vec<T>,
    excluded: Vec<T>,
    kept_positions: Vec<usize>,
}

impl<T> DigestPartition<T> {
    pub(super) fn prepare(
        ctx: &DecodeContext<'_>,
        source: &[T],
        mut keep: impl FnMut(&T) -> bool,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let work = source.len().checked_mul(3).ok_or_else(|| {
            ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
        })?;
        ctx.charge_work(
            u64::try_from(work).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?,
            operation,
        )?;
        let mut kept_count = 0usize;
        for item in source {
            if keep(item) {
                kept_count = kept_count.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
                })?;
            }
        }
        let excluded_count = source.len() - kept_count;
        let mut kept = Vec::new();
        let mut excluded = Vec::new();
        let mut kept_positions = Vec::new();
        ctx.reserve_collection_vec(&mut kept, kept_count, operation)?;
        ctx.reserve_collection_vec(&mut excluded, excluded_count, operation)?;
        ctx.reserve_collection_vec(&mut kept_positions, kept_count, operation)?;
        Ok(Self {
            original_len: source.len(),
            original: Vec::new(),
            kept,
            excluded,
            kept_positions,
        })
    }

    pub(super) fn move_from(&mut self, source: &mut Vec<T>, mut keep: impl FnMut(&T) -> bool) {
        self.original = std::mem::take(source);
        for (position, item) in self.original.drain(..).enumerate() {
            if keep(&item) {
                self.kept_positions.push(position);
                self.kept.push(item);
            } else {
                self.excluded.push(item);
            }
        }
    }

    pub(super) fn kept(&self) -> &[T] {
        &self.kept
    }

    pub(super) fn take_kept(&mut self) -> Vec<T> {
        std::mem::take(&mut self.kept)
    }

    pub(super) fn restore(mut self, kept: Vec<T>) -> Result<Vec<T>, CodecError> {
        let total = kept.len().checked_add(self.excluded.len()).ok_or_else(|| {
            CodecError::malformed("invalid SLDPRT digest partition size")
        })?;
        if total != self.original_len || kept.len() != self.kept_positions.len() {
            return Err(CodecError::malformed("invalid SLDPRT digest partition shape"));
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
            let item = item.ok_or_else(|| {
                CodecError::malformed("invalid SLDPRT digest partition order")
            })?;
            self.original.push(item);
        }
        Ok(self.original)
    }
}
