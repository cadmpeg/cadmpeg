// SPDX-License-Identifier: Apache-2.0
//! Byte-counted nullable payload references in a DELETE frame.

use super::branch_items::BranchItems;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

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
    pub(crate) fn read(
        ctx: &DecodeContext<'_>,
        record: OperationPayload<'_>,
    ) -> Result<Option<Self>, CodecError> {
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
        let Some(count) = shape() else {
            return Ok(None);
        };
        let mut slots = ctx.collection_vec(usize::from(count), "NX DELETE reference slots")?;
        let mut at = 7;
        for _ in 0..count {
            let slot = if bytes.get(at) == Some(&0xff) {
                at += 1;
                None
            } else {
                let Some(token) = bytes.get(at..).and_then(PayloadIndexToken::read) else {
                    return Ok(None);
                };
                at += token.raw().len();
                Some((token, ()))
            };
            slots.push(slot);
        }
        Ok(BranchItems::new(slots).ok().and_then(|slots| {
            Self::new(
                cadmpeg_core::decode::u64_from_index(record.payload_offset()),
                bytes[0],
                slots,
            )
            .ok()
        }))
    }

    pub(crate) fn resolve<B>(
        self,
        ctx: &DecodeContext<'_>,
        file_base: u64,
        mut target: impl FnMut(PayloadIndexToken) -> Result<B, CodecError>,
    ) -> Result<Option<DeleteReferences<B>>, CodecError> {
        let Some(offset) = self.offset.checked_add(file_base) else {
            return Ok(None);
        };
        let slots = self.slots.try_map_indexed_charged(ctx, |_, slot| {
            slot.map(|(token, ())| target(token).map(|value| (token, value)))
                .transpose()
        })?;
        Ok(DeleteReferences::new(offset, self.control, slots).ok())
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
        crate::test_support::with_decode_context(|ctx| {
            let record = OperationPayload::new(&payload, 100, "DELETE").unwrap();
            let field = DeleteReferences::read(ctx, record).unwrap().unwrap();
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
                assert!(DeleteReferences::read(ctx, record).unwrap().is_none());
            }
            for count in [0, 1, 6, 8, 255] {
                let mut wrong = payload;
                wrong[6] = count;
                let record = OperationPayload::new(&wrong, 100, "DELETE").unwrap();
                assert!(DeleteReferences::read(ctx, record).unwrap().is_none());
            }
        });
    }

    #[test]
    fn counted_delete_refuses_slot_allocation_before_resolution() {
        let payload = [12, 0, 0, 1, 0, 1, 7, 255, 255, 255, 255, 255, 255, 0];
        crate::test_support::with_decode_context_over(
            &payload,
            |p| p.limits.max_collection_items = 5,
            |ctx| {
                let record = OperationPayload::new(&payload, 100, "DELETE").unwrap();
                assert!(
                    matches!(DeleteReferences::read(ctx, record), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
                );
            },
        );
    }

    #[test]
    fn delete_reference_resolution_returns_collection_refusal() {
        let payload = [
            0x0c, 0, 0, 1, 0, 1, 6, 0xf0, 0x20, 0xff, 0xf1, 2, 8, 0xf1, 2, 9, 0xff, 0,
        ];
        let record = OperationPayload::new(&payload, 100, "DELETE").expect("test DELETE payload");
        let field = crate::test_support::with_decode_context(|ctx| {
            DeleteReferences::read(ctx, record)
                .unwrap()
                .expect("complete DELETE field")
        });

        crate::test_support::with_decode_context_over(
            &payload,
            |policy| {
                policy.limits.max_collection_items = 0;
            },
            |ctx| {
                let error = field
                    .resolve(ctx, 0, |token| {
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
        let field = crate::test_support::with_decode_context(|ctx| {
            DeleteReferences::read(ctx, record)
                .unwrap()
                .expect("complete DELETE field")
        });
        let resolved = crate::test_support::with_decode_context(|ctx| {
            field
                .resolve(ctx, 20, |token| {
                    Ok::<_, cadmpeg_core::CodecError>(Some(token.value()))
                })
                .expect("target resolution")
                .expect("valid relocated field")
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
