// SPDX-License-Identifier: Apache-2.0
//! Byte-counted pattern references with derived contiguous token positions.

use super::branch_items::BranchItems;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use cadmpeg_core::decode::u64_from_index;

const TRAILER: [u8; 19] = [
    0, 0, 0, 0x37, 0xff, 0xff, 1, 0, 0, 0, 0x38, 0xff, 1, 0xff, 0xff, 0xff, 0xff, 1, 0xff,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CountedPatternReferences<B> {
    offset: u64,
    entries: BranchItems<(PayloadIndexToken, B)>,
}

impl<B> CountedPatternReferences<B> {
    pub(crate) fn new(
        offset: u64,
        entries: BranchItems<(PayloadIndexToken, B)>,
    ) -> Result<Self, &'static str> {
        let width = entries
            .as_slice()
            .iter()
            .map(|(token, _)| cadmpeg_core::decode::u64_from_index(token.raw().len()))
            .sum::<u64>();
        offset
            .checked_add(2 + width + cadmpeg_core::decode::u64_from_index(TRAILER.len()))
            .ok_or("source_offset: counted reference frame overflows")?;
        Ok(Self { offset, entries })
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }

    pub(crate) fn declared_count(&self) -> u8 {
        self.entries.declared_count()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (u64, PayloadIndexToken, &B)> + Clone {
        let mut at = self.offset + 2;
        self.entries.as_slice().iter().map(move |(token, target)| {
            let offset = at;
            at += cadmpeg_core::decode::u64_from_index(token.raw().len());
            (offset, *token, target)
        })
    }
}

impl CountedPatternReferences<()> {
    pub(crate) fn read(record: OperationPayload<'_>) -> Option<CountedPatternReferences<()>> {
        if record.name() != "Pattern Feature" {
            return None;
        }
        let bytes = record.payload();
        let shape = |start: usize| {
            if bytes.get(start) != Some(&1) {
                return None;
            }
            let count = bytes.get(start + 1)?.checked_sub(1)?;
            if count == 0 {
                return None;
            }
            let at = start.checked_add(2)?;
            cadmpeg_core::decode::bounded_len(u64::from(count), 2, bytes.len().checked_sub(at)?)?;
            let mut scan_at = at;
            for _ in 0..count {
                let token = PayloadIndexToken::read(bytes.get(scan_at..)?)?;
                scan_at += token.raw().len();
            }
            let end = scan_at.checked_add(TRAILER.len())?;
            if bytes.get(scan_at..end) != Some(&TRAILER) {
                return None;
            }
            record.payload_offset().checked_add(end)?;
            Some((start, count))
        };
        let (start, count) = super::unique_candidate((0..bytes.len()).filter_map(shape))?;
        let count = usize::from(count);

        let mut entries = Vec::new();
        let mut at = start.checked_add(2)?;
        for _ in 0..count {
            let token = bytes.get(at..).and_then(PayloadIndexToken::read)?;
            at += token.raw().len();
            entries.push((token, ()));
        }
        let offset = record.payload_offset().checked_add(start)?;
        BranchItems::new(entries)
            .ok()
            .and_then(|entries| Self::new(u64_from_index(offset), entries).ok())
    }

    pub(crate) fn resolve<B>(
        self,
        file_base: u64,
        mut target: impl FnMut(PayloadIndexToken) -> B,
    ) -> Option<CountedPatternReferences<B>> {
        let offset = self.offset.checked_add(file_base)?;
        let entries = self
            .entries
            .map_indexed(|_, (token, ())| (token, target(token)));
        CountedPatternReferences::new(offset, entries).ok()
    }
}
