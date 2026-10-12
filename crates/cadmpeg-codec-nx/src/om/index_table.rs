// SPDX-License-Identifier: Apache-2.0
//! Borrowed monotone OM indexes retain the bounds needed to enumerate records.

use super::{EntityRecord, FixedEntityRecord};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;

/// Source-affine positions where adjacent little-endian words decrease.
#[derive(Debug)]
pub(super) struct DescendingU32Edges<'a> {
    bytes: &'a [u8],
    offsets_by_alignment: [Vec<usize>; 4],
}

impl<'a> DescendingU32Edges<'a> {
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        reservation: &mut ScopedReservation<'_>,
        bytes: &'a [u8],
    ) -> Result<Self, CodecError> {
        let mut offsets_by_alignment = <[Vec<usize>; 4]>::default();
        if let Some(range_end) = bytes.len().checked_sub(7) {
            for offset in ctx.admit_iter(&(0..range_end), "NX descending index edge scan")? {
                if View::u32_le_at(bytes, offset)
                    .zip(View::u32_le_at(bytes, offset + 4))
                    .is_some_and(|(current, next)| current > next)
                {
                    let offsets = &mut offsets_by_alignment[offset % 4];
                    ctx.reserve_scoped_vec(reservation, offsets, 1, "nx descending index edges")?;
                    offsets.push(offset);
                }
            }
        }
        Ok(Self {
            bytes,
            offsets_by_alignment,
        })
    }

    fn is_nondecreasing(
        &self,
        ctx: &DecodeContext<'_>,
        start: usize,
        end: usize,
    ) -> Result<bool, CodecError> {
        if end < start {
            return Ok(false);
        }
        if end - start < 8 {
            return Ok(true);
        }
        let Some(last_word) = end.checked_sub(4) else {
            return Ok(false);
        };
        let offsets = &self.offsets_by_alignment[start % 4];
        let first = ctx.partition_point(
            offsets,
            |offset| Ok(*offset < start),
            "NX descending index edge lookup",
        )?;
        Ok(offsets.get(first).is_none_or(|offset| *offset >= last_word))
    }

    fn records(
        &self,
        ctx: &DecodeContext<'_>,
        start: usize,
        count: usize,
        base: usize,
    ) -> Result<Option<IndexRecords<'a>>, CodecError> {
        if count < 2 {
            return Ok(None);
        }
        let Some(end) = count
            .checked_mul(4)
            .and_then(|width| start.checked_add(width))
        else {
            return Ok(None);
        };
        let Some(raw) = self.bytes.get(start..end) else {
            return Ok(None);
        };
        let mut words = View::over_retained(raw);
        if !self.is_nondecreasing(ctx, start, end)? {
            return Ok(None);
        }
        let Some(first) = words
            .u32_le()
            .and_then(|word| base.checked_add(cadmpeg_core::decode::index_from_u32(word)))
        else {
            return Ok(None);
        };
        let Some(last) = View::u32_le_at(self.bytes, end - 4)
            .and_then(|word| base.checked_add(cadmpeg_core::decode::index_from_u32(word)))
        else {
            return Ok(None);
        };
        let Some(source) = self.bytes.get(..last) else {
            return Ok(None);
        };
        Ok(Some(IndexRecords {
            source,
            base,
            first,
            words,
        }))
    }
}

/// `first` precedes the remaining whole words; the terminal word bounds source.
/// Monotonicity and the source bound make each record range infallible.
#[derive(Debug, Clone, Copy)]
struct IndexRecords<'a> {
    source: &'a [u8],
    base: usize,
    first: usize,
    words: View<'a>,
}

impl<'a> IndexRecords<'a> {
    fn records(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<impl Iterator<Item = EntityRecord<'a>>, CodecError> {
        let mut words = self.words;
        let mut start = self.first;
        Ok(ctx
            .admit_iter(0..words.remaining() / 4, "NX OM record bounds traversal")?
            .map_while(move |_| {
                let end = self.base + cadmpeg_core::decode::index_from_u32(words.u32_le()?);
                let record = EntityRecord {
                    offset: start,
                    bytes: &self.source[start..end],
                };
                start = end;
                Some(record)
            }))
    }
}

/// Fixed-table identities and record intervals have the same number of words.
#[derive(Debug, Clone, Copy)]
pub(super) struct FixedIndex<'a> {
    index_start: usize,
    object_id_table_offset: usize,
    object_ids: View<'a>,
    records: IndexRecords<'a>,
}

impl<'a> FixedIndex<'a> {
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        edges: &DescendingU32Edges<'a>,
        index_start: usize,
        count: usize,
        base: usize,
        object_id_table_offset: usize,
    ) -> Result<Option<Self>, CodecError> {
        if View::u32_le_at(edges.bytes, index_start) != Some(0) {
            return Ok(None);
        }
        let Some(start) = index_start.checked_add(4) else {
            return Ok(None);
        };
        let Some(records) = edges.records(ctx, start, count, base)? else {
            return Ok(None);
        };
        if records.first == base {
            return Ok(None);
        }
        let Some(ids_start) = object_id_table_offset.checked_add(8) else {
            return Ok(None);
        };
        let Some(ids_end) = ids_start.checked_add(records.words.remaining()) else {
            return Ok(None);
        };
        let Some(ids) = records.source.get(ids_start..ids_end) else {
            return Ok(None);
        };
        if records.source.get(..index_start).is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            index_start,
            object_id_table_offset,
            object_ids: View::over_retained(ids),
            records,
        }))
    }

    pub(super) fn source(self) -> &'a [u8] {
        self.records.source
    }
    pub(super) fn base(self) -> usize {
        self.records.base
    }
    pub(super) fn index_start(self) -> usize {
        self.index_start
    }
    pub(super) fn object_id_table_offset(self) -> usize {
        self.object_id_table_offset
    }

    pub(super) fn records(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<impl Iterator<Item = FixedEntityRecord<'a>>, CodecError> {
        let ids_start = self.object_id_table_offset + 8;
        let mut words = self.object_ids;
        let ids = ctx
            .admit_iter(0..words.remaining() / 4, "NX OM object identity traversal")?
            .map_while(move |ordinal| {
                words.u32_le().map(|value| {
                    (
                        value,
                        cadmpeg_core::decode::u64_from_index(ids_start + ordinal * 4),
                    )
                })
            });
        Ok(self
            .records
            .records(ctx)?
            .zip(ids)
            .map(|(record, object_id)| FixedEntityRecord {
                object_id,
                offset: record.offset,
                bytes: record.bytes,
            }))
    }
}

#[derive(Debug, Clone)]
pub(super) struct OffsetIndex<'a> {
    index_start: usize,
    control: EntityRecord<'a>,
    records: IndexRecords<'a>,
}

impl<'a> OffsetIndex<'a> {
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        edges: &DescendingU32Edges<'a>,
        index_start: usize,
        offset_count: usize,
        count_offset: usize,
    ) -> Result<Option<Self>, CodecError> {
        if offset_count
            .checked_mul(4)
            .and_then(|width| index_start.checked_add(width))
            != Some(count_offset)
        {
            return Ok(None);
        }
        let Some(mut records) = edges.records(ctx, index_start, offset_count, 0)? else {
            return Ok(None);
        };
        let Some(count_end) = count_offset.checked_add(4) else {
            return Ok(None);
        };
        if records.first < count_end {
            return Ok(None);
        }
        let control_start = records.first;
        let Some(control_end) = records
            .words
            .u32_le()
            .map(cadmpeg_core::decode::index_from_u32)
        else {
            return Ok(None);
        };
        let control = EntityRecord {
            offset: control_start,
            bytes: &records.source[control_start..control_end],
        };
        records.first = control_end;
        Ok(Some(Self {
            index_start,
            control,
            records,
        }))
    }

    pub(super) fn source(&self) -> &'a [u8] {
        self.records.source
    }
    pub(super) fn index_start(&self) -> usize {
        self.index_start
    }
    pub(super) fn control(&self) -> EntityRecord<'a> {
        self.control.clone()
    }
    pub(super) fn column_storage(&self) -> &'a [u8] {
        &self.records.source[self.records.first..]
    }
    pub(super) fn records(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<impl Iterator<Item = EntityRecord<'a>>, CodecError> {
        self.records.records(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::{DescendingU32Edges, FixedIndex, OffsetIndex};

    #[test]
    fn fixed_index_projection_pays_for_records_instead_of_word_bytes() {
        let mut bytes = Vec::new();
        for word in [0_u32, 32, 34, 34, 3, 0, 7, 9] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&[0xaa, 0xbb]);
        let index = crate::test_support::with_decode_context(|ctx| {
            let mut reservation = ctx.reserve_scoped(0, "test index edges").unwrap();
            let edges = DescendingU32Edges::new(ctx, &mut reservation, &bytes).unwrap();
            FixedIndex::new(ctx, &edges, 0, 3, 0, 16)
        })
        .unwrap()
        .unwrap();
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 4,
            |ctx| {
                let records = index.records(ctx).unwrap().collect::<Vec<_>>();
                assert_eq!(records.len(), 2);
                assert_eq!(records[0].object_id, (7, 24));
                assert_eq!(records[0].bytes, &[0xaa, 0xbb]);
                assert_eq!(records[1].object_id, (9, 28));
                assert!(records[1].bytes.is_empty());
                assert_eq!(ctx.resource_refusal(), None);
            },
        );
    }

    #[test]
    fn fixed_index_pairs_ids_with_empty_and_nonempty_records() {
        crate::test_support::with_decode_context(|ctx| {
            let mut reservation = ctx.reserve_scoped(0, "test index edges").unwrap();
            let mut bytes = Vec::new();
            for word in [0u32, 32, 34, 34, 3, 0, 7, 9] {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
            bytes.extend_from_slice(&[0xaa, 0xbb]);
            let edges = DescendingU32Edges::new(ctx, &mut reservation, &bytes).unwrap();
            let index = FixedIndex::new(ctx, &edges, 0, 3, 0, 16).unwrap().unwrap();
            let records = index.records(ctx).unwrap().collect::<Vec<_>>();
            assert_eq!(records.len(), 2);
            assert_eq!(records[0].object_id, (7, 24));
            assert_eq!(records[0].bytes, &[0xaa, 0xbb]);
            assert_eq!(records[1].object_id, (9, 28));
            assert_eq!(records[1].offset, 34);
            assert!(records[1].bytes.is_empty());
            assert!(FixedIndex::new(ctx, &edges, 0, 3, 1, 16).unwrap().is_none());
            assert!(FixedIndex::new(ctx, &edges, 0, 3, 0, usize::MAX)
                .unwrap()
                .is_none());
        });
    }

    #[test]
    fn offset_index_retains_control_and_contiguous_storage() {
        crate::test_support::with_decode_context(|ctx| {
            let mut reservation = ctx.reserve_scoped(0, "test index edges").unwrap();
            let mut bytes = Vec::new();
            for word in [20u32, 22, 24, 24, 2] {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
            bytes.extend_from_slice(&[1, 2, 3, 4]);
            let edges = DescendingU32Edges::new(ctx, &mut reservation, &bytes).unwrap();
            let index = OffsetIndex::new(ctx, &edges, 0, 4, 16).unwrap().unwrap();
            assert_eq!(index.control().offset, 20);
            assert_eq!(index.control().bytes, &[1, 2]);
            assert_eq!(index.column_storage(), &[3, 4]);
            let records = index.records(ctx).unwrap().collect::<Vec<_>>();
            assert_eq!(records.len(), 2);
            assert_eq!(records[0].bytes, &[3, 4]);
            assert!(records[1].bytes.is_empty());
            assert!(OffsetIndex::new(ctx, &edges, 0, 4, 17).unwrap().is_none());
            bytes[8..12].copy_from_slice(&21u32.to_le_bytes());
            assert!(OffsetIndex::new(
                ctx,
                &DescendingU32Edges::new(ctx, &mut reservation, &bytes).unwrap(),
                0,
                4,
                16
            )
            .unwrap()
            .is_none());
        });
    }

    #[test]
    fn om_index_monotone_cache_rejects_a_decrease_inside_a_candidate() {
        crate::test_support::with_decode_context(|ctx| {
            let mut reservation = ctx.reserve_scoped(0, "test index edges").unwrap();
            let words = [10_u32, 20, 30, 25, 40];
            let bytes = words
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>();
            let edges = super::DescendingU32Edges::new(ctx, &mut reservation, &bytes).unwrap();

            assert!(edges.is_nondecreasing(ctx, 0, 12).unwrap());
            assert!(!edges.is_nondecreasing(ctx, 0, 16).unwrap());
            assert!(edges.is_nondecreasing(ctx, 4, 12).unwrap());
            assert!(edges.is_nondecreasing(ctx, 12, 20).unwrap());
        });
    }
}
