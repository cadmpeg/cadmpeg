// SPDX-License-Identifier: Apache-2.0
//! Type-70 declaration payload and physical tail multiplicity.

use crate::framing::xmt_reference::NonNullXmt;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU16;

#[derive(Clone, Copy)]
pub(crate) enum TrailingCopies {
    One,
    Two,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StateWire", into = "StateWire")]
pub(crate) struct Type70State {
    xmt: NonNullXmt,
    node_id: u32,
    references: [u32; 4],
    count: NonZeroU16,
    trailing_reference: NonNullXmt,
}
impl Type70State {
    pub(crate) fn new(
        xmt: u32,
        node_id: u32,
        references: [u32; 4],
        count: u16,
        trailing_reference: u32,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            xmt: NonNullXmt::try_from(xmt)?,
            node_id,
            references,
            count: NonZeroU16::new(count).ok_or("count: must be positive")?,
            trailing_reference: NonNullXmt::try_from(trailing_reference)
                .map_err(|_| "trailing_reference: must be non-null")?,
        })
    }
    pub(crate) fn xmt(&self) -> u32 {
        self.xmt.into()
    }
    pub(crate) fn node_id(&self) -> u32 {
        self.node_id
    }
    pub(crate) fn references(&self) -> [u32; 4] {
        self.references
    }
    pub(crate) fn trailing_reference(&self) -> NonNullXmt {
        self.trailing_reference
    }
}
#[derive(Serialize, Deserialize)]
struct StateWire {
    xmt: u32,
    node_id: u32,
    references: [u32; 4],
    count: u16,
    trailing_reference: u32,
}
impl From<Type70State> for StateWire {
    fn from(state: Type70State) -> Self {
        Self {
            xmt: state.xmt(),
            node_id: state.node_id,
            references: state.references,
            count: state.count.get(),
            trailing_reference: state.trailing_reference().into(),
        }
    }
}
impl TryFrom<StateWire> for Type70State {
    type Error = &'static str;
    fn try_from(wire: StateWire) -> Result<Self, Self::Error> {
        Self::new(
            wire.xmt,
            wire.node_id,
            wire.references,
            wire.count,
            wire.trailing_reference,
        )
    }
}
#[cfg(test)]
mod tests {
    use super::Type70State;
    #[test]
    fn wire_preserves_open_body_references_and_rejects_null_controls() {
        let json =
            r#"{"xmt":6,"node_id":0,"references":[3,1,1,0],"count":13,"trailing_reference":45}"#;
        let state: Type70State = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&state).unwrap(), json);
        for (field, value) in [("xmt", 1), ("count", 0), ("trailing_reference", 1)] {
            let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
            wire[field] = value.into();
            assert!(serde_json::from_value::<Type70State>(wire)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
    }
}
