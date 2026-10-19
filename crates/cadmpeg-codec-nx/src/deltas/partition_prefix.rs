// SPDX-License-Identifier: Apache-2.0
//! Leading ordinary PARTITION and current PMARK nodes.

use super::pmark_state::PmarkState;
use crate::framing::read_and_advance;
use crate::framing::xmt_reference::NonNullXmt;
use cadmpeg_core::decode::View;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct PartitionState {
    xmt: NonNullXmt,
    current_pmark: NonNullXmt,
    highest_id: u32,
}

/// A leading partition node and the current mark it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PrefixWire")]
pub(crate) struct PartitionPrefixState {
    partition: PartitionState,
    current_pmark: PmarkState,
}

#[derive(Deserialize)]
struct PrefixWire {
    partition: PartitionState,
    current_pmark: PmarkState,
}

impl TryFrom<PrefixWire> for PartitionPrefixState {
    type Error = &'static str;
    fn try_from(wire: PrefixWire) -> Result<Self, Self::Error> {
        if u32::from(wire.partition.current_pmark) != wire.current_pmark.xmt() {
            return Err("current_pmark: must name the following PMARK node");
        }
        if u32::from(wire.partition.xmt) == wire.current_pmark.xmt() {
            return Err("partition.xmt: must differ from the current PMARK identity");
        }
        Ok(Self {
            partition: wire.partition,
            current_pmark: wire.current_pmark,
        })
    }
}

impl PartitionPrefixState {
    /// Read the two first-use schema-match nodes at the header endpoint.
    pub(super) fn read(stream: &[u8], offset: usize) -> Option<(Self, usize)> {
        let mut view = View::over_retained(stream);
        view.seek(offset)?;
        (view.u16_be()? == 3 && view.u8()? == 255).then_some(())?;
        let mut at = view.position();
        let xmt = NonNullXmt::try_from(read_and_advance(stream, &mut at)?).ok()?;
        let current_pmark = NonNullXmt::try_from(read_and_advance(stream, &mut at)?).ok()?;
        view.seek(at)?;
        let highest_id = view.u32_be()?;
        (view.u16_be()? == 4 && view.u8()? == 255).then_some(())?;
        let (mark, end) = PmarkState::read(stream, view.position())?;
        let state = Self::try_from(PrefixWire {
            partition: PartitionState {
                xmt,
                current_pmark,
                highest_id,
            },
            current_pmark: mark,
        })
        .ok()?;
        Some((state, end))
    }

    pub(crate) fn current_pmark(self) -> PmarkState {
        self.current_pmark
    }

    pub(super) fn partition_xmt(self) -> u32 {
        self.partition.xmt.into()
    }

    pub(crate) fn highest_id(self) -> u32 {
        self.partition.highest_id
    }
}

#[cfg(test)]
mod tests {
    use super::PartitionPrefixState;

    #[test]
    fn wire_checks_the_current_pointer_without_a_consecutive_allocation_rule() {
        let value = serde_json::json!({
            "partition": {"xmt": 2, "current_pmark": 9, "highest_id": u32::MAX},
            "current_pmark": {
                "xmt": 9, "preceding": 12, "first_following": 1,
                "next_sibling": 1, "prev_sibling": 1,
                "n_new_nodes": 0, "n_del_nodes": 0, "n_copy_mod_nodes": 0,
                "delta_key": 0, "delta_is_forward": false, "identifier": 0x1234_5678,
            },
        });
        let prefix: PartitionPrefixState = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(prefix).unwrap(), value);
        let mut duplicate = value.clone();
        duplicate["partition"]["xmt"] = 9.into();
        assert!(serde_json::from_value::<PartitionPrefixState>(duplicate).is_err());
        let mut wrong_pointer = value;
        wrong_pointer["partition"]["current_pmark"] = 8.into();
        assert!(serde_json::from_value::<PartitionPrefixState>(wrong_pointer).is_err());
    }
}
