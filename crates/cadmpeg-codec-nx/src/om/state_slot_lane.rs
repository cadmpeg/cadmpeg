// SPDX-License-Identifier: Apache-2.0
//! Operation-state slot lanes with token-derived extents.

use super::state_index::{OperationStateIndex, StateIndexToken};
use super::state_slots::StateSlots;
use cadmpeg_core::decode::DecodeContext;
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
    pub(super) fn read(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        at: usize,
        end: usize,
        base: usize,
    ) -> Result<Option<Self>, CodecError> {
        let Some(prefix_end) = at.checked_add(3) else {
            return Ok(None);
        };
        if bytes.get(at..prefix_end) != Some(&[0x02, 0x01, 0x11]) {
            return Ok(None);
        }
        let mut slots = Vec::new();
        let mut cursor = prefix_end;
        while cursor < end {
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
                return Ok(Some(Self { offset, end, slots }));
            }
            let Some(slot) = OperationStateIndex::read_at(bytes, cursor, base) else {
                return Ok(None);
            };
            let Some(next) = cursor.checked_add(slot.raw().len()) else {
                return Ok(None);
            };
            cursor = next;
            ctx.charge_work(1, "scan NX state slots")?;
            ctx.reserve_vec(&mut slots, 1, "nx state slots")?;
            slots.push(slot.token());
        }
        Ok(None)
    }

    pub(super) fn end_at(bytes: &[u8], at: usize, end: usize) -> Option<usize> {
        if bytes.get(at..at.checked_add(3)?) != Some(&[0x02, 0x01, 0x11]) {
            return None;
        }
        let mut cursor = at + 3;
        while cursor < end {
            if bytes.get(cursor..cursor.checked_add(2)?) == Some(&[0x02, 0x11]) {
                return cursor.checked_add(2);
            }
            let slot = OperationStateIndex::read_at(bytes, cursor, 0)?;
            cursor = cursor.checked_add(slot.raw().len())?;
        }
        None
    }
}

impl StateSlotLane<u64> {
    pub(crate) fn new(
        offset: u64,
        slots: StateSlots<Option<StateIndexToken>>,
    ) -> Result<Self, &'static str> {
        let end = match Self::extent(offset, slots.as_slice(), |slots| {
            Ok::<_, std::convert::Infallible>(slots.iter())
        }) {
            Ok(end) => end?,
            Err(error) => match error {},
        };
        Ok(Self { offset, end, slots })
    }

    fn extent<'a, E, I: Iterator<Item = &'a Option<StateIndexToken>>>(
        offset: u64, slots: &'a [Option<StateIndexToken>],
        admit: impl FnOnce(&'a [Option<StateIndexToken>]) -> Result<I, E>,
    ) -> Result<Result<u64, &'static str>, E> {
        let end = admit(slots)?.try_fold(offset, |end, slot| {
            end.checked_add(u64::from(slot.map_or(1, StateIndexToken::byte_len)))
        }).and_then(|end| end.checked_add(5));
        Ok(end.ok_or("source_offset: slot-lane extent overflows"))
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
                let lane = StateSlotLane::read(ctx, &bytes, 0, bytes.len(), 100)
                    .unwrap()
                    .unwrap();
                assert_eq!(lane.slots().len(), 4);
                assert_eq!(lane.offset(), 100);
                assert_eq!(lane.end_offset(), 112);
                assert_eq!(StateSlotLane::end_at(&bytes, 0, bytes.len()), Some(12));
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
                assert!(
                    StateSlotLane::read(ctx, &bytes, 0, bytes.len(), usize::MAX - 11)
                        .unwrap()
                        .is_none()
                );
            },
        );
    }

}
