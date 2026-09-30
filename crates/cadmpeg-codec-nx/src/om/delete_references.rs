// SPDX-License-Identifier: Apache-2.0
//! Five nullable payload references in a DELETE frame.

use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use cadmpeg_core::CodecError;

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
    pub(crate) fn slots(&self) -> &[Option<(PayloadIndexToken, B)>; 5] {
        &self.slots
    }
    pub(crate) fn reference_offsets(&self) -> [u64; 5] {
        let mut at = self.offset + 7;
        self.slots.each_ref().map(|slot| {
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
            cadmpeg_core::decode::u64_from_index(record.payload_offset()),
            control,
            [first?, second?, third?, fourth?, fifth?],
        )
        .ok()
    }

    pub(crate) fn resolve<B>(
        self,
        file_base: u64,
        mut target: impl FnMut(PayloadIndexToken) -> Result<B, CodecError>,
    ) -> Result<Option<DeleteReferences<B>>, CodecError> {
        let Some(offset) = self.offset.checked_add(file_base) else {
            return Ok(None);
        };
        let mut map_slot = |slot: Option<(PayloadIndexToken, ())>| {
            slot.map(|(token, ())| target(token).map(|value| (token, value)))
                .transpose()
        };
        let [first, second, third, fourth, fifth] = self.slots;
        Ok(DeleteReferences::new(
            offset,
            self.control,
            [
                map_slot(first)?,
                map_slot(second)?,
                map_slot(third)?,
                map_slot(fourth)?,
                map_slot(fifth)?,
            ],
        )
        .ok())
    }
}

#[cfg(test)]
mod tests {
    use super::DeleteReferences;
    use crate::om::operation_record::OperationPayload;

    #[test]
    fn delete_reference_resolution_returns_collection_refusal() {
        let payload = [
            0x0c, 0, 0, 1, 0, 1, 6, 0xf0, 0x20, 0xff, 0xf1, 2, 8, 0xf1, 2, 9, 0xff, 0,
        ];
        let record = OperationPayload::new(&payload, 100, "DELETE").expect("test DELETE payload");
        let field = DeleteReferences::read(record).expect("complete DELETE field");

        crate::test_support::with_decode_context_over(
            &payload,
            |policy| {
                policy.limits.max_collection_items = 0;
            },
            |ctx| {
                let error = field
                    .resolve(0, |token| {
                        ctx.charge_collection_items(1, "NX DELETE target")?;
                        Ok(Some(token.value()))
                    })
                    .expect_err("target resolution refusal");
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
                );
            },
        );
    }

    #[test]
    fn delete_reference_resolution_preserves_nullable_slots() {
        let payload = [
            0x0c, 0, 0, 1, 0, 1, 6, 0xf0, 0x20, 0xff, 0xf1, 2, 8, 0xf1, 2, 9, 0xff, 0,
        ];
        let record = OperationPayload::new(&payload, 100, "DELETE").expect("test DELETE payload");
        let field = DeleteReferences::read(record).expect("complete DELETE field");
        let resolved = field
            .resolve(20, |token| {
                Ok::<_, cadmpeg_core::CodecError>(Some(token.value()))
            })
            .expect("target resolution")
            .expect("valid relocated field");
        assert_eq!(resolved.offset(), 120);
        assert_eq!(resolved.reference_offsets(), [127, 129, 130, 133, 136]);
        assert_eq!(
            resolved
                .slots()
                .each_ref()
                .map(|slot| slot.as_ref().map(|(_, value)| *value)),
            [Some(Some(32)), None, Some(Some(520)), Some(Some(521)), None]
        );
    }
}
