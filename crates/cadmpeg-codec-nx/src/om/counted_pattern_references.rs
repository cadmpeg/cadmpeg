// SPDX-License-Identifier: Apache-2.0
//! Byte-counted pattern references with derived contiguous token positions.

use super::branch_items::BranchItems;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;

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
            .map(|(token, _)| token.raw().len() as u64)
            .sum::<u64>();
        offset
            .checked_add(2 + width + TRAILER.len() as u64)
            .ok_or("source_offset: counted reference frame overflows")?;
        Ok(Self { offset, entries })
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }

    pub(crate) fn declared_count(&self) -> u8 {
        self.entries.declared_count()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (u64, PayloadIndexToken, &B)> {
        let mut at = self.offset + 2;
        self.entries.as_slice().iter().map(move |(token, target)| {
            let offset = at;
            at += token.raw().len() as u64;
            (offset, *token, target)
        })
    }
}

impl CountedPatternReferences<()> {
    pub(crate) fn read(record: OperationPayload<'_>) -> Option<Self> {
        if record.name() != "Pattern Feature" {
            return None;
        }
        let bytes = record.payload();
        let decode = |start: usize| {
            if bytes.get(start) != Some(&1) {
                return None;
            }
            let count = bytes.get(start + 1)?.checked_sub(1)?;
            if count == 0 {
                return None;
            }
            let mut at = start.checked_add(2)?;
            cadmpeg_core::decode::bounded_len(u64::from(count), 2, bytes.len().saturating_sub(at))?;
            let mut scan_at = at;
            for _ in 0..count {
                let token = PayloadIndexToken::read(bytes.get(scan_at..)?)?;
                scan_at += token.raw().len();
            }
            let end = scan_at.checked_add(TRAILER.len())?;
            if bytes.get(scan_at..end) != Some(&TRAILER) {
                return None;
            }
            let mut entries = Vec::with_capacity(usize::from(count));
            for _ in 0..count {
                let token = PayloadIndexToken::read(bytes.get(at..)?)?;
                at += token.raw().len();
                entries.push((token, ()));
            }
            Self::new(
                (record.payload_offset() + start) as u64,
                BranchItems::new(entries).ok()?,
            )
            .ok()
        };
        super::unique_candidate((0..bytes.len()).filter_map(decode))
    }

    pub(crate) fn resolve<B>(
        self,
        file_base: u64,
        mut target: impl FnMut(PayloadIndexToken) -> B,
    ) -> Result<CountedPatternReferences<B>, &'static str> {
        let offset = self
            .offset
            .checked_add(file_base)
            .ok_or("source_offset: counted reference frame overflows")?;
        CountedPatternReferences::new(
            offset,
            self.entries
                .map_indexed(|_, (token, ())| (token, target(token))),
        )
    }
}
