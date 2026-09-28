// SPDX-License-Identifier: Apache-2.0
//! Byte-counted pattern references with derived contiguous token positions.

use super::branch_items::BranchItems;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

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

    pub(crate) fn iter(&self) -> impl Iterator<Item = (u64, PayloadIndexToken, &B)> + Clone {
        let mut at = self.offset + 2;
        self.entries.as_slice().iter().map(move |(token, target)| {
            let offset = at;
            at += token.raw().len() as u64;
            (offset, *token, target)
        })
    }
}

impl CountedPatternReferences<()> {
    pub(crate) fn read(ctx: &DecodeContext<'_>, record: OperationPayload<'_>) -> Result<Option<Self>, CodecError> {
        if record.name() != "Pattern Feature" {
            return Ok(None);
        }
        let bytes = record.payload();
        ctx.charge_work(u64_from_index(bytes.len()), "scan NX counted pattern references")?;
        let shape = |start: usize| {
            if bytes.get(start) != Some(&1) {
                return None;
            }
            let count = bytes.get(start + 1)?.checked_sub(1)?;
            if count == 0 {
                return None;
            }
            let at = start.checked_add(2)?;
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
            record.payload_offset().checked_add(end)?;
            Some((start, count))
        };
        let Some((start, count)) = super::unique_candidate((0..bytes.len()).filter_map(shape)) else { return Ok(None); };
        let count = usize::from(count);
        let count_u64 = u64_from_index(count);
        let slot_bytes = count_u64.checked_mul(u64_from_index(std::mem::size_of::<(PayloadIndexToken, ())>()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX counted pattern references", u64::MAX, u64::MAX))?;
        ctx.charge_collection_items(count_u64, "NX counted pattern references")?;
        ctx.charge_retained(slot_bytes, "NX counted pattern references")?;
        let mut entries = Vec::new();
        entries.try_reserve_exact(count).map_err(|_| ctx.refuse_codec_limit("NX counted pattern references", 0, count_u64))?;
        let Some(mut at) = start.checked_add(2) else { return Ok(None); };
        for _ in 0..count {
            let Some(token) = bytes.get(at..).and_then(PayloadIndexToken::read) else { return Ok(None); };
            at += token.raw().len();
            entries.push((token, ()));
        }
        let Some(offset) = record.payload_offset().checked_add(start) else { return Ok(None); };
        Ok(BranchItems::new(entries).ok().and_then(|entries| Self::new(u64_from_index(offset), entries).ok()))
    }

    pub(crate) fn resolve<B>(
        self,
        ctx: &DecodeContext<'_>,
        file_base: u64,
        mut target: impl FnMut(PayloadIndexToken) -> B,
    ) -> Result<Option<CountedPatternReferences<B>>, CodecError> {
        let Some(offset) = self.offset.checked_add(file_base) else { return Ok(None); };
        let entries = self.entries.map_indexed_charged(ctx, |_, (token, ())| (token, target(token)))?;
        Ok(CountedPatternReferences::new(
            offset,
            entries,
        ).ok())
    }
}
