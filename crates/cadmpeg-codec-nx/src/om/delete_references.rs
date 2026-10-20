// SPDX-License-Identifier: Apache-2.0
//! Byte-counted nullable payload references in a DELETE frame.

use super::branch_items::BranchItems;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeleteReferences<B> {
    offset: u64,
    control: u8,
    slots: BranchItems<Option<(PayloadIndexToken, B)>>,
}

impl<B> DeleteReferences<B> {
    pub(crate) fn new(
        offset: u64,
        control: u8,
        slots: BranchItems<Option<(PayloadIndexToken, B)>>,
    ) -> Result<Self, &'static str> {
        let width = slots
            .as_slice()
            .iter()
            .map(|slot| {
                slot.as_ref().map_or(1, |(token, _)| {
                    cadmpeg_core::decode::u64_from_index(token.raw().len())
                })
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
    pub(crate) fn slots(&self) -> &[Option<(PayloadIndexToken, B)>] {
        self.slots.as_slice()
    }
    pub(crate) fn reference_offsets(&self) -> impl Iterator<Item = u64> + Clone + '_ {
        let mut at = self.offset + 7;
        self.slots.as_slice().iter().map(move |slot| {
            let offset = at;
            at += slot.as_ref().map_or(1, |(token, _)| {
                cadmpeg_core::decode::u64_from_index(token.raw().len())
            });
            offset
        })
    }
}

impl DeleteReferences<()> {
    pub(crate) fn read(record: OperationPayload<'_>) -> Option<Self> {
        let bytes = record.payload();
        let shape = || {
            if record.name() != "DELETE" || bytes.get(1..6) != Some(&[0, 0, 1, 0, 1]) {
                return None;
            }
            let count = bytes.get(6)?.checked_sub(1)?;
            if count == 0 {
                return None;
            }
            let mut at = 7;
            for _ in 0..count {
                if bytes.get(at) == Some(&0xff) {
                    at += 1;
                } else {
                    at += PayloadIndexToken::read(bytes.get(at..)?)?.raw().len();
                }
            }
            (bytes.get(at) == Some(&0)).then_some(count)
        };
        let count = shape()?;
        let mut slots = Vec::new();
        let mut at = 7;
        for _ in 0..count {
            let slot = if bytes.get(at) == Some(&0xff) {
                at += 1;
                None
            } else {
                let token = bytes.get(at..).and_then(PayloadIndexToken::read)?;
                at += token.raw().len();
                Some((token, ()))
            };
            slots.push(slot);
        }
        BranchItems::new(slots).ok().and_then(|slots| {
            Self::new(
                cadmpeg_core::decode::u64_from_index(record.payload_offset()),
                bytes[0],
                slots,
            )
            .ok()
        })
    }

    pub(crate) fn resolve<B>(
        self,
        file_base: u64,
        mut target: impl FnMut(PayloadIndexToken) -> B,
    ) -> Option<DeleteReferences<B>> {
        let offset = self.offset.checked_add(file_base)?;
        let slots = self
            .slots
            .map_indexed(|_, slot| slot.map(|(token, ())| (token, target(token))));
        DeleteReferences::new(offset, self.control, slots).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::DeleteReferences;
    use crate::om::operation_record::OperationPayload;

    #[test]
    fn counted_delete_retains_six_mixed_width_nullable_references() {
        let payload = [
            12, 0, 0, 1, 0, 1, 7, 0xf0, 32, 0xff, 0xf1, 2, 8, 0xf0, 6, 0xff, 0xf1, 2, 9, 0,
        ];
        crate::test_support::with_decode_context(|_ctx| {
            let record = OperationPayload::new(&payload, 100, "DELETE").unwrap();
            let field = DeleteReferences::read(record).unwrap();
            assert_eq!(
                field
                    .slots()
                    .iter()
                    .map(|s| s.as_ref().map(|(t, ())| t.value()))
                    .collect::<Vec<_>>(),
                [Some(32), None, Some(520), Some(6), None, Some(521)]
            );
            assert_eq!(
                field.reference_offsets().collect::<Vec<_>>(),
                [107, 109, 110, 113, 115, 116]
            );
            for end in 0..payload.len() {
                let record = OperationPayload::new(&payload[..end], 100, "DELETE").unwrap();
                assert!(DeleteReferences::read(record).is_none());
            }
            for count in [0, 1, 6, 8, 255] {
                let mut wrong = payload;
                wrong[6] = count;
                let record = OperationPayload::new(&wrong, 100, "DELETE").unwrap();
                assert!(DeleteReferences::read(record).is_none());
            }
        });
    }

    #[test]
    fn delete_reference_resolution_preserves_nullable_slots() {
        let payload = [
            0x0c, 0, 0, 1, 0, 1, 6, 0xf0, 0x20, 0xff, 0xf1, 2, 8, 0xf1, 2, 9, 0xff, 0,
        ];
        let record = OperationPayload::new(&payload, 100, "DELETE").expect("test DELETE payload");
        let field = crate::test_support::with_decode_context(|_ctx| {
            DeleteReferences::read(record).unwrap()
        });
        let resolved = crate::test_support::with_decode_context(|_ctx| {
            field
                .resolve(20, |token| Some(token.value()))
                .expect("target resolution")
        });
        assert_eq!(resolved.offset(), 120);
        assert_eq!(
            resolved.reference_offsets().collect::<Vec<_>>(),
            [127, 129, 130, 133, 136]
        );
        assert_eq!(
            resolved
                .slots()
                .iter()
                .map(|slot| slot.as_ref().map(|(_, value)| *value))
                .collect::<Vec<_>>(),
            [Some(Some(32)), None, Some(Some(520)), Some(Some(521)), None]
        );
    }
}
