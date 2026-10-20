// SPDX-License-Identifier: Apache-2.0
//! Modern LIST fields in embedded-schema order.

use crate::framing::xmt_reference::NonNullXmt;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ListWire", into = "ListWire")]
pub(crate) struct ListFields {
    notransmit: bool,
    owner: NonNullXmt,
    next: NonZeroU32,
    previous: NonZeroU32,
    list_length: u32,
    finger_index: u32,
    finger_block: NonNullXmt,
    list_block: NonNullXmt,
}

impl ListFields {
    pub(crate) fn references(&self) -> [u32; 5] {
        [
            self.owner.into(),
            self.next.get(),
            self.previous.get(),
            self.finger_block.into(),
            self.list_block.into(),
        ]
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct ListWire {
    pub(super) node_id: u32,
    pub(super) list_type: u8,
    pub(super) notransmit: bool,
    pub(super) owner: u32,
    pub(super) next: u32,
    pub(super) previous: u32,
    pub(super) list_length: u32,
    pub(super) block_length: u32,
    pub(super) finger_index: u32,
    pub(super) finger_block: u32,
    pub(super) list_block: u32,
}

impl TryFrom<ListWire> for ListFields {
    type Error = &'static str;
    fn try_from(wire: ListWire) -> Result<Self, Self::Error> {
        if wire.node_id != 0 || wire.list_type != 4 || wire.block_length != 20 {
            return Err("LIST: require node_id zero, pointer-list type 4 and block_length 20");
        }
        if wire.finger_index == 0 || wire.finger_index > wire.list_length.max(1) {
            return Err("finger_index: require 1..=max(1, list_length)");
        }
        Ok(Self {
            notransmit: wire.notransmit,
            owner: NonNullXmt::try_from(wire.owner)?,
            next: NonZeroU32::new(wire.next).ok_or("next: zero target")?,
            previous: NonZeroU32::new(wire.previous).ok_or("previous: zero target")?,
            list_length: wire.list_length,
            finger_index: wire.finger_index,
            finger_block: NonNullXmt::try_from(wire.finger_block)?,
            list_block: NonNullXmt::try_from(wire.list_block)?,
        })
    }
}
impl From<ListFields> for ListWire {
    fn from(fields: ListFields) -> Self {
        let [owner, next, previous, finger_block, list_block] = fields.references();
        Self {
            node_id: 0,
            list_type: 4,
            notransmit: fields.notransmit,
            owner,
            next,
            previous,
            list_length: fields.list_length,
            block_length: 20,
            finger_index: fields.finger_index,
            finger_block,
            list_block,
        }
    }
}

#[cfg(test)]
mod tests;
