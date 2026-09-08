// SPDX-License-Identifier: Apache-2.0
//! Five nullable payload references in a DELETE frame.

use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeleteReferences<B> {
    offset: u64,
    control: u8,
    slots: [Option<(PayloadIndexToken, B)>; 5],
}

impl<B> DeleteReferences<B> {
    pub(crate) fn new(
        offset: u64,
        control: u8,
        slots: [Option<(PayloadIndexToken, B)>; 5],
    ) -> Result<Self, &'static str> {
        let width = slots
            .iter()
            .map(|slot| {
                slot.as_ref()
                    .map_or(1, |(token, _)| token.raw().len() as u64)
            })
            .sum::<u64>();
        offset
            .checked_add(8 + width)
            .ok_or("source_offset: DELETE frame overflows")?;
        Ok(Self {
            offset,
            control,
            slots,
        })
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }
    pub(crate) fn control(&self) -> u8 {
        self.control
    }
    pub(crate) fn slots(&self) -> &[Option<(PayloadIndexToken, B)>; 5] {
        &self.slots
    }
    pub(crate) fn reference_offsets(&self) -> [u64; 5] {
        let mut at = self.offset + 7;
        self.slots.each_ref().map(|slot| {
            let offset = at;
            at += slot
                .as_ref()
                .map_or(1, |(token, _)| token.raw().len() as u64);
            offset
        })
    }
}

impl DeleteReferences<()> {
    pub(crate) fn read(record: OperationPayload<'_>) -> Option<Self> {
        let bytes = record.payload();
        if record.name() != "DELETE" || bytes.get(1..7) != Some(&[0, 0, 1, 0, 1, 6]) {
            return None;
        }
        let control = *bytes.first()?;
        let mut at = 7;
        let [first, second, third, fourth, fifth] = std::array::from_fn::<_, 5, _>(|_| {
            let slot = if bytes.get(at) == Some(&0xff) {
                at += 1;
                None
            } else {
                let token = PayloadIndexToken::read(bytes.get(at..)?)?;
                at += token.raw().len();
                Some((token, ()))
            };
            Some(slot)
        });
        if bytes.get(at) != Some(&0) {
            return None;
        }
        Self::new(
            record.payload_offset() as u64,
            control,
            [first?, second?, third?, fourth?, fifth?],
        )
        .ok()
    }

    pub(crate) fn resolve<B>(
        self,
        file_base: u64,
        mut target: impl FnMut(PayloadIndexToken) -> B,
    ) -> Result<DeleteReferences<B>, &'static str> {
        let offset = self
            .offset
            .checked_add(file_base)
            .ok_or("source_offset: DELETE frame overflows")?;
        DeleteReferences::new(
            offset,
            self.control,
            self.slots
                .map(|slot| slot.map(|(token, ())| (token, target(token)))),
        )
    }
}
