// SPDX-License-Identifier: Apache-2.0
//! Operation-state slot lanes with token-derived extents.

use super::state_index::{OperationStateIndex, StateIndexToken};
use super::state_slots::StateSlots;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StateSlotLane<O = usize> {
    offset: O,
    end: O,
    slots: StateSlots<Option<StateIndexToken>>,
}

impl<O: Copy> StateSlotLane<O> {
    pub(crate) fn offset(&self) -> O {
        self.offset
    }
    pub(crate) fn slots(&self) -> &StateSlots<Option<StateIndexToken>> {
        &self.slots
    }
    pub(crate) fn into_slots(self) -> StateSlots<Option<StateIndexToken>> {
        self.slots
    }
    pub(crate) fn end_offset(&self) -> O {
        self.end
    }
}

impl StateSlotLane {
    pub(super) fn read<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        bytes: &[u8],
        at: usize,
        end: usize,
        base: usize,
    ) -> Result<Option<(Self, ScopedReservation<'ctx>)>, CodecError> {
        let Some(prefix_end) = at.checked_add(3) else {
            return Ok(None);
        };
        if bytes.get(at..prefix_end) != Some(&[0x02, 0x01, 0x11]) {
            return Ok(None);
        }
        let mut storage = ctx.reserve_scoped(0, "NX state slot candidate storage")?;
        let mut slots = Vec::new();
        let mut cursor = prefix_end;
        while cursor < end {
            ctx.charge_work(1, "scan NX state slots")?;
            let Some(marker_end) = cursor.checked_add(2) else {
                return Ok(None);
            };
            if bytes.get(cursor..marker_end) == Some(&[0x02, 0x11]) {
                let (Some(end), Some(offset), Ok(slots)) = (
                    base.checked_add(marker_end),
                    base.checked_add(at),
                    StateSlots::new(slots),
                ) else {
                    return Ok(None);
                };
                return Ok(Some((Self { offset, end, slots }, storage)));
            }
            let Some(slot) = OperationStateIndex::read_at(bytes, cursor, base) else {
                return Ok(None);
            };
            let Some(next) = cursor.checked_add(slot.raw().len()) else {
                return Ok(None);
            };
            cursor = next;
            storage.with_storage(|| ctx.reserve_vec(&mut slots, 1, "nx state slots"))?;
            slots.push(slot.token());
        }
        Ok(None)
    }

    pub(super) fn end_at(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        at: usize,
        end: usize,
    ) -> Result<Option<usize>, CodecError> {
        let Some(prefix_end) = at.checked_add(3) else {
            return Ok(None);
        };
        if bytes.get(at..prefix_end) != Some(&[0x02, 0x01, 0x11]) {
            return Ok(None);
        }
        let mut cursor = prefix_end;
        while cursor < end {
            ctx.charge_work(1, "NX state slot extent scan")?;
            let Some(marker_end) = cursor.checked_add(2) else {
                return Ok(None);
            };
            if bytes.get(cursor..marker_end) == Some(&[0x02, 0x11]) {
                return Ok(Some(marker_end));
            }
            let Some(slot) = OperationStateIndex::read_at(bytes, cursor, 0) else {
                return Ok(None);
            };
            let Some(next) = cursor.checked_add(slot.raw().len()) else {
                return Ok(None);
            };
            cursor = next;
        }
        Ok(None)
    }
}

impl StateSlotLane<u64> {
    pub(crate) fn new(
        offset: u64,
        slots: StateSlots<Option<StateIndexToken>>,
    ) -> Result<Self, &'static str> {
        let end = match Self::extent(offset, slots.as_slice(), |slots| {
            Ok::<_, std::convert::Infallible>(slots.next())
        }) {
            Ok(end) => end?,
            Err(error) => match error {},
        };
        Ok(Self { offset, end, slots })
    }

    pub(crate) fn from_wire(
        ctx: &DecodeContext<'_>,
        offset: u64,
        slots: StateSlots<Option<StateIndexToken>>,
    ) -> Result<Result<Self, &'static str>, CodecError> {
        Ok(Self::extent(offset, slots.as_slice(), |slots| {
            ctx.next_charged(slots, "NX native state slot width traversal")
        })?
        .map(|end| Self { offset, end, slots }))
    }

    fn extent<'a, E>(
        offset: u64,
        slots: &'a [Option<StateIndexToken>],
        mut next: impl FnMut(
            &mut std::slice::Iter<'a, Option<StateIndexToken>>,
        ) -> Result<Option<&'a Option<StateIndexToken>>, E>,
    ) -> Result<Result<u64, &'static str>, E> {
        let mut end = offset;
        let mut slots = slots.iter();
        while slots.len() > 0 {
            let Some(slot) = next(&mut slots)? else {
                break;
            };
            let Some(next) = end.checked_add(u64::from(slot.map_or(1, StateIndexToken::byte_len)))
            else {
                return Ok(Err("source_offset: slot-lane extent overflows"));
            };
            end = next;
        }
        Ok(end
            .checked_add(5)
            .ok_or("source_offset: slot-lane extent overflows"))
    }
}

#[cfg(test)]
mod tests {
    use super::StateSlotLane;

    #[test]
    fn source_and_native_extents_follow_null_and_variable_width_slots() {
        let bytes = [2, 1, 0x11, 0xff, 1, 0x80, 1, 0x90, 0, 1, 2, 0x11];

        crate::test_support::with_decode_context_over(
            &bytes,
            |_| {},
            |ctx| {
                let (lane, storage) = StateSlotLane::read(ctx, &bytes, 0, bytes.len(), 100)
                    .unwrap()
                    .unwrap();
                assert_eq!(lane.slots().len(), 4);
                assert_eq!(lane.offset(), 100);
                assert_eq!(lane.end_offset(), 112);
                assert_eq!(
                    StateSlotLane::end_at(ctx, &bytes, 0, bytes.len()).unwrap(),
                    Some(12)
                );
                let native = StateSlotLane::new(
                    200 + cadmpeg_core::decode::u64_from_index(lane.offset()),
                    lane.clone().into_slots(),
                )
                .unwrap();
                assert_eq!((native.offset(), native.end_offset()), (300, 312));
                assert!(StateSlotLane::new(
                    u64::MAX - 112 + cadmpeg_core::decode::u64_from_index(lane.offset()),
                    lane.clone().into_slots()
                )
                .is_ok());
                assert!(StateSlotLane::new(
                    u64::MAX - 111 + cadmpeg_core::decode::u64_from_index(lane.offset()),
                    lane.into_slots()
                )
                .is_err());
                drop(storage);
                assert!(
                    StateSlotLane::read(ctx, &bytes, 0, bytes.len(), usize::MAX - 11)
                        .unwrap()
                        .is_none()
                );
            },
        );
    }

    #[test]
    fn slot_candidate_receipt_tracks_and_releases_the_token_vector() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;

        let bytes = [2, 1, 0x11, 0xff, 2, 0x11];
        let token_storage =
            4 * std::mem::size_of::<Option<super::super::state_index::StateIndexToken>>();
        let refusal = crate::test_support::with_decode_context_over(
            &bytes,
            |policy| {
                policy.limits.max_materialized_bytes =
                    cadmpeg_core::decode::u64_from_index(token_storage)
            },
            |ctx| {
                let (first, first_storage) = StateSlotLane::read(ctx, &bytes, 0, bytes.len(), 0)
                    .unwrap()
                    .unwrap();
                let refusal = match StateSlotLane::read(ctx, &bytes, 0, bytes.len(), 0) {
                    Err(CodecError::ResourceLimit(limit)) => limit,
                    Err(error) => {
                        panic!("second slot candidate failed for the wrong reason: {error}")
                    }
                    Ok(_) => panic!("second slot candidate must exceed the exact storage cap"),
                };
                drop(first);
                drop(first_storage);
                refusal
            },
        );
        assert_eq!(refusal.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(refusal.operation, "nx state slots");
        assert_eq!(refusal.additional, cadmpeg_core::decode::u64_from_index(token_storage));

        crate::test_support::with_decode_context_over(
            &bytes,
            |policy| {
                policy.limits.max_materialized_bytes =
                    cadmpeg_core::decode::u64_from_index(token_storage)
            },
            |ctx| {
                let (lane, storage) =
                    StateSlotLane::read(ctx, &bytes, 0, bytes.len(), 0).unwrap().unwrap();
                drop(lane);
                drop(storage);
                assert!(ctx
                    .reserve_scoped(
                        cadmpeg_core::decode::u64_from_index(token_storage),
                        "released NX slot candidate storage",
                    )
                    .is_ok());
            },
        );

        crate::test_support::with_decode_context_over(
            &bytes,
            |policy| {
                policy.limits.max_materialized_bytes =
                    cadmpeg_core::decode::u64_from_index(token_storage)
            },
            |ctx| {
                let (lane, storage) =
                    StateSlotLane::read(ctx, &bytes, 0, bytes.len(), 0).unwrap().unwrap();
                assert_eq!(
                    StateSlotLane::from_wire(ctx, u64::MAX - 5, lane.into_slots()).unwrap(),
                    Err("source_offset: slot-lane extent overflows")
                );
                drop(storage);
                assert!(ctx
                    .reserve_scoped(
                        cadmpeg_core::decode::u64_from_index(token_storage),
                        "released rejected NX slot candidate storage",
                    )
                    .is_ok());
            },
        );
    }
}
