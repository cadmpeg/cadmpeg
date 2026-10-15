// SPDX-License-Identifier: Apache-2.0
//! `POINTER_LIS_BLOCK` fields and populated-prefix invariant.

use crate::framing::xmt_reference::NonNullXmt;
use crate::iter_wire::IterWire;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "BlockWire")]
pub(crate) struct PointerListBlockFields {
    n_entries: u32,
    next_block: NonZeroU32,
    entries: Vec<u32>,
}
impl PointerListBlockFields {
    pub(super) fn new(
        n_entries: u32,
        next_block: u32,
        entries: Vec<u32>,
    ) -> Result<Self, &'static str> {
        if entries.is_empty()
            || usize::try_from(n_entries)
                .ok()
                .is_none_or(|n| n > entries.len())
        {
            return Err("n_entries: require nonempty storage and populated prefix within capacity");
        }
        let n = cadmpeg_core::decode::index_from_u32(n_entries);
        if entries[..n]
            .iter()
            .any(|value| NonNullXmt::try_from(*value).is_err())
            || entries[n..].iter().any(|value| *value != 1)
        {
            return Err("references: require non-null populated prefix and null unused entries");
        }
        Ok(Self {
            n_entries,
            next_block: NonZeroU32::new(next_block).ok_or("next_block: zero target")?,
            entries,
        })
    }
    pub(crate) fn references(&self) -> impl Iterator<Item = u32> + Clone + '_ {
        std::iter::once(self.next_block.get()).chain(self.entries.iter().copied())
    }
}
impl Serialize for PointerListBlockFields {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_struct("BlockWire", 5)?;
        wire.serialize_field("entry_array_length", &self.entries.len())?;
        wire.serialize_field("n_entries", &self.n_entries)?;
        wire.serialize_field("index_map_offset", &0u32)?;
        wire.serialize_field("next_block", &self.next_block)?;
        wire.serialize_field("entries", &IterWire(self.entries.iter()))?;
        wire.end()
    }
}
#[derive(Deserialize)]
struct BlockWire {
    entry_array_length: u32,
    n_entries: u32,
    index_map_offset: u32,
    next_block: u32,
    entries: Vec<u32>,
}
impl TryFrom<BlockWire> for PointerListBlockFields {
    type Error = &'static str;
    fn try_from(wire: BlockWire) -> Result<Self, Self::Error> {
        if wire.index_map_offset != 0
            || u32::try_from(wire.entries.len()).ok() != Some(wire.entry_array_length)
        {
            return Err("POINTER_LIS_BLOCK: index_map_offset must be zero and entry_array_length must match storage");
        }
        Self::new(wire.n_entries, wire.next_block, wire.entries)
    }
}

#[cfg(test)]
mod tests;
