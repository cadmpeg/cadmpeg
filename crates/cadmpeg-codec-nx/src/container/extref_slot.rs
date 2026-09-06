// SPDX-License-Identifier: Apache-2.0
//! The four ID-slot positions in an external-reference handle-set record.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
#[repr(u8)]
pub(crate) enum ExtrefSlot {
    First = 0,
    Second = 1,
    Third = 2,
    Fourth = 3,
}

impl ExtrefSlot {
    pub(crate) const ALL: [Self; 4] = [Self::First, Self::Second, Self::Third, Self::Fourth];

    pub(crate) fn index(self) -> usize { usize::from(u8::from(self)) }

    pub(crate) fn offset(self) -> u64 {
        crate::layout::extrefstream_handle_set_record::ID_SLOTS as u64 + u64::from(u8::from(self)) * 4
    }
}

impl From<ExtrefSlot> for u8 {
    fn from(slot: ExtrefSlot) -> Self { slot as Self }
}

impl TryFrom<u8> for ExtrefSlot {
    type Error = &'static str;
    fn try_from(slot: u8) -> Result<Self, Self::Error> {
        match slot {
            0 => Ok(Self::First),
            1 => Ok(Self::Second),
            2 => Ok(Self::Third),
            3 => Ok(Self::Fourth),
            _ => Err("slot must be one of 0, 1, 2, 3"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ExtrefSlot;

    #[test]
    fn slot_wire_is_numeric_and_bounded() {
        for (slot, (value, offset)) in ExtrefSlot::ALL.into_iter().zip([(0, 7), (1, 11), (2, 15), (3, 19)]) {
            let json = value.to_string();
            assert_eq!(serde_json::to_string(&slot).unwrap(), json);
            assert_eq!(serde_json::from_str::<ExtrefSlot>(&json).unwrap(), slot);
            assert_eq!(slot.offset(), offset);
        }
        for json in ["-1", "4", "255", "256"] {
            assert!(serde_json::from_str::<ExtrefSlot>(json).is_err());
        }
    }
}
