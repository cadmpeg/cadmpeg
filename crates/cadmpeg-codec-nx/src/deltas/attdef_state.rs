// SPDX-License-Identifier: Apache-2.0
//! ATTDEF_LIST active references followed by null slots.

use serde::{Deserialize, Serialize};
use crate::framing::xmt_reference::NonNullXmt;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StateWire", into = "StateWire")]
pub(crate) struct AttdefState {
    xmt: NonNullXmt,
    active: Vec<NonNullXmt>,
    null_count: u32,
}
impl AttdefState {
    pub(crate) fn new(xmt: u32, slot_count: u32, active_count: u32, references: Vec<u32>) -> Result<Self, &'static str> {
        let xmt = NonNullXmt::try_from(xmt)?;
        if slot_count == 0 || u32::try_from(references.len()).ok() != Some(slot_count) {
            return Err("slot_count: require a nonempty reference vector of the declared length");
        }
        if active_count > slot_count { return Err("active_count: exceeds slot_count"); }
        let active_len = active_count as usize;
        if references[active_len..].iter().any(|reference| *reference != 1) {
            return Err("references: inactive slots must be null");
        }
        let active = references.into_iter().take(active_len).map(NonNullXmt::try_from).collect::<Result<Vec<_>, _>>().map_err(|_| "references: active slots must be non-null")?;
        Ok(Self { xmt, active, null_count: slot_count - active_count })
    }
    pub(crate) fn xmt(&self) -> u32 { self.xmt.into() }
    pub(crate) fn active_count(&self) -> u32 { self.active.len() as u32 }
    pub(crate) fn slot_count(&self) -> u32 { self.active_count() + self.null_count }
    pub(crate) fn references(&self) -> impl Iterator<Item = u32> + '_ {
        self.active.iter().copied().map(u32::from).chain(std::iter::repeat_n(1, self.null_count as usize))
    }
}
#[derive(Serialize, Deserialize)]
struct StateWire {
    xmt: u32,
    slot_count: u32,
    active_count: u32,
    references: Vec<u32>,
}
impl From<AttdefState> for StateWire {
    fn from(state: AttdefState) -> Self {
        Self { xmt: state.xmt(), slot_count: state.slot_count(), active_count: state.active_count(), references: state.references().collect() }
    }
}
impl TryFrom<StateWire> for AttdefState {
    type Error = &'static str;
    fn try_from(wire: StateWire) -> Result<Self, Self::Error> { Self::new(wire.xmt, wire.slot_count, wire.active_count, wire.references) }
}
#[cfg(test)]
mod tests {
    use super::AttdefState;
    #[test]
    fn wire_derives_counts_and_rejects_invalid_slot_partitions() {
        for json in [
            r#"{"xmt":43,"slot_count":4,"active_count":2,"references":[143,155,1,1]}"#,
            r#"{"xmt":2,"slot_count":1,"active_count":0,"references":[1]}"#,
            r#"{"xmt":2,"slot_count":1,"active_count":1,"references":[3]}"#,
        ] {
            let state: AttdefState = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&state).unwrap(), json);
        }
        for (slots, active, refs, field) in [(0,0,vec![],"slot_count"),(2,1,vec![2],"slot_count"),(1,2,vec![2],"active_count"),(1,1,vec![1],"reference"),(1,0,vec![2],"references")] {
            assert!(AttdefState::new(2, slots, active, refs).unwrap_err().contains(field));
        }
    }
}
