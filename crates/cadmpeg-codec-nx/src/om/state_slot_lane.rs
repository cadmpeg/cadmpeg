// SPDX-License-Identifier: Apache-2.0
//! Operation-state slot lanes with token-derived extents.

use super::state_index::{OperationStateIndex, StateIndexToken};
use super::state_slots::StateSlots;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StateSlotLane<O = usize> {
    offset: O,
    slots: StateSlots<Option<StateIndexToken>>,
}

impl<O: Copy + From<u8> + std::ops::Add<Output = O>> StateSlotLane<O> {
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
        self.slots
            .iter()
            .fold(self.offset + O::from(5), |end, (_, slot)| {
                end + O::from(slot.map_or(1, StateIndexToken::byte_len))
            })
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
                let (Some(_), Some(offset), Ok(slots)) = (
                    base.checked_add(marker_end),
                    base.checked_add(at),
                    StateSlots::new(slots),
                ) else {
                    return Ok(None);
                };
                return Ok(Some(Self { offset, slots }));
            }
            let Some(slot) = OperationStateIndex::read_at(bytes, cursor, base) else {
                return Ok(None);
            };
            let Some(next) = cursor.checked_add(slot.raw().len()) else {
                return Ok(None);
            };
            cursor = next;
            ctx.charge_work(1, "scan NX state slots")?;
            ctx.charge_collection_items(1, "nx state slots")?;
            ctx.charge_retained(
                u64_from_index(std::mem::size_of::<Option<StateIndexToken>>()),
                "retain NX state slot",
            )?;
            slots
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("nx state slots", 0, 1))?;
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
        let byte_len = slots.iter().fold(5u64, |len, (_, slot)| {
            len + u64::from(slot.map_or(1, StateIndexToken::byte_len))
        });
        offset
            .checked_add(byte_len)
            .ok_or("source_offset: slot-lane extent overflows")?;
        Ok(Self { offset, slots })
    }
}

#[cfg(test)]
mod tests {
    use super::StateSlotLane;

    #[test]
    fn source_and_native_extents_follow_null_and_variable_width_slots() {
        let bytes = [2, 1, 0x11, 0xff, 1, 0x80, 1, 0x90, 0, 1, 2, 0x11];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let lane = StateSlotLane::read(&ctx, &bytes, 0, bytes.len(), 100)
            .unwrap()
            .unwrap();
        assert_eq!(lane.slots().len(), 4);
        assert_eq!(lane.offset(), 100);
        assert_eq!(lane.end_offset(), 112);
        assert_eq!(StateSlotLane::end_at(&bytes, 0, bytes.len()), Some(12));
        let native =
            StateSlotLane::new(200 + lane.offset() as u64, lane.clone().into_slots()).unwrap();
        assert_eq!((native.offset(), native.end_offset()), (300, 312));
        assert!(StateSlotLane::new(
            u64::MAX - 112 + lane.offset() as u64,
            lane.clone().into_slots()
        )
        .is_ok());
        assert!(
            StateSlotLane::new(u64::MAX - 111 + lane.offset() as u64, lane.into_slots()).is_err()
        );
        assert!(StateSlotLane::read(&ctx, &bytes, 0, bytes.len(), usize::MAX - 11)
            .unwrap()
            .is_none());
    }
}
