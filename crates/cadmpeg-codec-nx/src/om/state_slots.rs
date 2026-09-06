// SPDX-License-Identifier: Apache-2.0
//! Slot sequences whose derived ordinals fit the native u32 wire field.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StateSlots<T>(Vec<T>);

impl<T> StateSlots<T> {
    pub(crate) fn new(slots: Vec<T>) -> Result<Self, &'static str> {
        if slots.len().checked_sub(1).is_some_and(|last| u32::try_from(last).is_err()) {
            return Err("slots.ordinal: final slot position exceeds u32");
        }
        Ok(Self(slots))
    }

    pub(crate) fn len(&self) -> usize { self.0.len() }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (u32, &T)> {
        self.0.iter().enumerate().map(|(ordinal, slot)| (ordinal as u32, slot))
    }

    #[cfg(test)]
    pub(crate) fn as_slice(&self) -> &[T] { &self.0 }

    pub(crate) fn map_slots<U>(self, mut map: impl FnMut(u32, T) -> U) -> StateSlots<U> {
        StateSlots(self.0.into_iter().enumerate().map(|(ordinal, slot)| map(ordinal as u32, slot)).collect())
    }

    pub(crate) fn try_map_slots<U, E>(self, mut map: impl FnMut(u32, T) -> Result<U, E>) -> Result<StateSlots<U>, E> {
        Ok(StateSlots(self.0.into_iter().enumerate().map(|(ordinal, slot)| map(ordinal as u32, slot)).collect::<Result<_, _>>()?))
    }
}

#[cfg(test)]
mod tests {
    use super::StateSlots;

    #[test]
    fn slot_maps_preserve_positions_and_empty_sequences() {
        let slots = StateSlots::new(vec![10, 20]).unwrap().map_slots(|ordinal, value| (ordinal, value));
        assert_eq!(slots.as_slice(), &[(0, 10), (1, 20)]);
        assert_eq!(StateSlots::new(Vec::<()>::new()).unwrap().len(), 0);
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn slot_count_bounds_the_last_ordinal_without_allocating_elements() {
        let maximum_count = u32::MAX as usize + 1;
        let maximum = StateSlots::new(vec![(); maximum_count]).unwrap();
        assert_eq!(maximum.len(), maximum_count);
        assert!(StateSlots::new(vec![(); maximum_count + 1]).is_err());
    }
}
