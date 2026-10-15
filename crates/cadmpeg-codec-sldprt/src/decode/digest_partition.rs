// SPDX-License-Identifier: Apache-2.0
//! Move digest-only model entries into a partition and restore their source order.

use cadmpeg_core::CodecError;

pub(super) struct DigestPartition<T> {
    original_len: usize,
    kept: Vec<T>,
    excluded: Vec<T>,
    kept_positions: Vec<usize>,
}

impl<T> DigestPartition<T> {
    /// Moves every item out of `source`, deciding each with one `keep` call.
    pub(super) fn split(source: &mut Vec<T>, mut keep: impl FnMut(&T) -> bool) -> Self {
        let mut partition = Self {
            original_len: source.len(),
            kept: Vec::new(),
            excluded: Vec::new(),
            kept_positions: Vec::new(),
        };
        for (position, item) in std::mem::take(source).into_iter().enumerate() {
            if keep(&item) {
                partition.kept_positions.push(position);
                partition.kept.push(item);
            } else {
                partition.excluded.push(item);
            }
        }
        partition
    }

    pub(super) fn take_kept(&mut self) -> Vec<T> {
        std::mem::take(&mut self.kept)
    }

    pub(super) fn restore(self, kept: Vec<T>) -> Result<Vec<T>, CodecError> {
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
        let mut original = Vec::new();
        for position in 0..total {
            let item = if positions.peek() == Some(&position) {
                positions.next();
                kept.next()
            } else {
                excluded.next()
            };
            let item =
                item.ok_or_else(|| CodecError::malformed("invalid SLDPRT digest partition order"))?;
            original.push(item);
        }
        Ok(original)
    }
}

#[cfg(test)]
mod tests {
    use super::DigestPartition;

    #[test]
    fn digest_partition_uses_each_admitted_decision_once() {
        let mut source = (0..100).collect::<Vec<_>>();
        let original = source.clone();
        let mut calls = 0;
        let mut partition = DigestPartition::split(&mut source, |_| {
            calls += 1;
            calls == 1
        });
        let kept = partition.take_kept();
        assert_eq!(calls, original.len());
        assert_eq!(kept, [0]);
        assert_eq!(partition.restore(kept).unwrap(), original);
        assert!(source.is_empty());
    }
}
