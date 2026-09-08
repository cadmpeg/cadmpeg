// SPDX-License-Identifier: Apache-2.0
//! Borrowed monotone OM indexes retain the bounds needed to enumerate records.

use super::{EntityRecord, FixedEntityRecord};
use cadmpeg_core::decode::View;

/// Source-affine positions where adjacent little-endian words decrease.
#[derive(Debug)]
pub(super) struct DescendingU32Edges<'a> {
    bytes: &'a [u8],
    offsets_by_alignment: [Vec<usize>; 4],
}

impl<'a> DescendingU32Edges<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        let mut offsets_by_alignment = <[Vec<usize>; 4]>::default();
        for offset in 0..bytes.len().saturating_sub(7) {
            if View::u32_le_at(bytes, offset)
                .zip(View::u32_le_at(bytes, offset + 4))
                .is_some_and(|(current, next)| current > next)
            {
                offsets_by_alignment[offset % 4].push(offset);
            }
        }
        Self {
            bytes,
            offsets_by_alignment,
        }
    }

    pub(super) fn is_nondecreasing(&self, start: usize, end: usize) -> bool {
        if end < start {
            return false;
        }
        if end - start < 8 {
            return true;
        }
        let offsets = &self.offsets_by_alignment[start % 4];
        let first = offsets.partition_point(|offset| *offset < start);
        offsets
            .get(first)
            .is_none_or(|offset| *offset >= end.saturating_sub(4))
    }

    fn records(&self, start: usize, count: usize, base: usize) -> Option<IndexRecords<'a>> {
        if count < 2 {
            return None;
        }
        let end = start.checked_add(count.checked_mul(4)?)?;
        let mut words = View::over_retained(self.bytes.get(start..end)?);
        if !self.is_nondecreasing(start, end) {
            return None;
        }
        let first = base.checked_add(words.u32_le()? as usize)?;
        let last = base.checked_add(View::u32_le_at(self.bytes, end - 4)? as usize)?;
        let source = self.bytes.get(..last)?;
        Some(IndexRecords {
            source,
            base,
            first,
            words,
        })
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
    fn records(self) -> impl Iterator<Item = EntityRecord<'a>> {
        let mut words = self.words;
        let mut start = self.first;
        std::iter::from_fn(move || {
            let end = self.base + words.u32_le()? as usize;
            let record = EntityRecord {
                offset: start,
                bytes: &self.source[start..end],
            };
            start = end;
            Some(record)
        })
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
        edges: &DescendingU32Edges<'a>,
        index_start: usize,
        count: usize,
        base: usize,
        object_id_table_offset: usize,
    ) -> Option<Self> {
        if View::u32_le_at(edges.bytes, index_start) != Some(0) {
            return None;
        }
        let records = edges.records(index_start.checked_add(4)?, count, base)?;
        if records.first == base {
            return None;
        }
        let ids_start = object_id_table_offset.checked_add(8)?;
        let ids_end = ids_start.checked_add(records.words.remaining())?;
        let object_ids = View::over_retained(records.source.get(ids_start..ids_end)?);
        records.source.get(..index_start)?;
        Some(Self {
            index_start,
            object_id_table_offset,
            object_ids,
            records,
        })
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

    pub(super) fn records(self) -> impl Iterator<Item = FixedEntityRecord<'a>> {
        let mut ids = self.object_ids;
        let ids_start = self.object_id_table_offset + 8;
        let ids = std::iter::from_fn(move || {
            let offset = ids_start + ids.position();
            ids.u32_le().map(|value| (value, offset as u64))
        });
        self.records
            .records()
            .zip(ids)
            .map(|(record, object_id)| FixedEntityRecord {
                object_id,
                offset: record.offset,
                bytes: record.bytes,
            })
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
        edges: &DescendingU32Edges<'a>,
        index_start: usize,
        offset_count: usize,
        count_offset: usize,
    ) -> Option<Self> {
        if index_start.checked_add(offset_count.checked_mul(4)?)? != count_offset {
            return None;
        }
        let mut records = edges.records(index_start, offset_count, 0)?;
        if records.first < count_offset.checked_add(4)? {
            return None;
        }
        let control_start = records.first;
        let control_end = records.words.u32_le()? as usize;
        let control = EntityRecord {
            offset: control_start,
            bytes: &records.source[control_start..control_end],
        };
        records.first = control_end;
        Some(Self {
            index_start,
            control,
            records,
        })
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
    pub(super) fn records(&self) -> impl Iterator<Item = EntityRecord<'a>> {
        self.records.records()
    }
}

#[cfg(test)]
mod tests {
    use super::{DescendingU32Edges, FixedIndex, OffsetIndex};

    #[test]
    fn fixed_index_pairs_ids_with_empty_and_nonempty_records() {
        let mut bytes = Vec::new();
        for word in [0u32, 32, 34, 34, 3, 0, 7, 9] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&[0xaa, 0xbb]);
        let edges = DescendingU32Edges::new(&bytes);
        let index = FixedIndex::new(&edges, 0, 3, 0, 16).unwrap();
        let records = index.records().collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].object_id, (7, 24));
        assert_eq!(records[0].bytes, &[0xaa, 0xbb]);
        assert_eq!(records[1].object_id, (9, 28));
        assert_eq!(records[1].offset, 34);
        assert!(records[1].bytes.is_empty());
        assert!(FixedIndex::new(&edges, 0, 3, 1, 16).is_none());
        assert!(FixedIndex::new(&edges, 0, 3, 0, usize::MAX).is_none());
    }

    #[test]
    fn offset_index_retains_control_and_contiguous_storage() {
        let mut bytes = Vec::new();
        for word in [20u32, 22, 24, 24, 2] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&[1, 2, 3, 4]);
        let edges = DescendingU32Edges::new(&bytes);
        let index = OffsetIndex::new(&edges, 0, 4, 16).unwrap();
        assert_eq!(index.control().offset, 20);
        assert_eq!(index.control().bytes, &[1, 2]);
        assert_eq!(index.column_storage(), &[3, 4]);
        let records = index.records().collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].bytes, &[3, 4]);
        assert!(records[1].bytes.is_empty());
        assert!(OffsetIndex::new(&edges, 0, 4, 17).is_none());
        bytes[8..12].copy_from_slice(&21u32.to_le_bytes());
        assert!(OffsetIndex::new(&DescendingU32Edges::new(&bytes), 0, 4, 16).is_none());
    }
}
