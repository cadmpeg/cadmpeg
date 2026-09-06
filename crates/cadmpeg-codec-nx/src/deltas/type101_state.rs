// SPDX-License-Identifier: Apache-2.0
//! Type-101 bound state with derived form words.

use serde::{Deserialize, Serialize};
use super::xmt_reference::NonNullXmt;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Form { Populated, Empty }

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StateWire", into = "StateWire")]
pub(crate) struct Type101State {
    references: [u32; 4],
    anchor_reference: Option<NonNullXmt>,
    form: Form,
    last_word: u32,
    terminal_value: u64,
}
impl Type101State {
    pub(crate) fn new(references: [u32; 4], anchor_reference: Option<u32>, state_words: [u32; 3], terminal_value: u64) -> Result<Self, &'static str> {
        let anchor_reference = anchor_reference.map(NonNullXmt::try_from).transpose().map_err(|_| "anchor_reference: require a non-null reference")?;
        let form = match (state_words[0], state_words[1]) {
            (19, 9) => Form::Populated,
            (0, 0) => Form::Empty,
            _ => return Err("state_words: require leading [19, 9] or [0, 0]"),
        };
        if terminal_value > 0xff_ffff_ffff { return Err("terminal_value: exceeds unsigned 40-bit range"); }
        Ok(Self { references, anchor_reference, form, last_word: state_words[2], terminal_value })
    }
    pub(crate) fn prefix_state(&self) -> [u8; 3] {
        match self.form { Form::Populated => [3, 4, 1], Form::Empty => [1, 1, 0] }
    }
    pub(crate) fn state_words(&self) -> [u32; 3] {
        match self.form { Form::Populated => [19, 9, self.last_word], Form::Empty => [0, 0, self.last_word] }
    }
    #[cfg(test)]
    pub(crate) fn terminal_value(&self) -> u64 { self.terminal_value }
    #[cfg(test)]
    pub(crate) fn anchor_reference(&self) -> Option<u32> { self.anchor_reference.map(u32::from) }
}
#[derive(Serialize, Deserialize)]
struct StateWire {
    references: [u32; 4],
    anchor_reference: Option<u32>,
    state_words: [u32; 3],
    terminal_value: u64,
}
impl From<Type101State> for StateWire {
    fn from(state: Type101State) -> Self {
        Self { references: state.references, anchor_reference: state.anchor_reference.map(u32::from), state_words: state.state_words(), terminal_value: state.terminal_value }
    }
}
impl TryFrom<StateWire> for Type101State {
    type Error = &'static str;
    fn try_from(wire: StateWire) -> Result<Self, Self::Error> {
        Self::new(wire.references, wire.anchor_reference, wire.state_words, wire.terminal_value)
    }
}
#[cfg(test)]
mod tests {
    use super::Type101State;
    #[test]
    fn wire_preserves_both_forms_and_rejects_invalid_state() {
        for json in [
            r#"{"references":[40000,3,1,9],"anchor_reference":11,"state_words":[19,9,27],"terminal_value":258}"#,
            r#"{"references":[0,1,2,3],"anchor_reference":null,"state_words":[0,0,493],"terminal_value":1099511627775}"#,
        ] {
            let state: Type101State = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&state).unwrap(), json);
            for (field, value) in [("anchor_reference", serde_json::json!(1)), ("state_words", serde_json::json!([19,0,27])), ("terminal_value", serde_json::json!(1099511627776u64))] {
                let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
                wire[field] = value;
                assert!(serde_json::from_value::<Type101State>(wire).unwrap_err().to_string().contains(field));
            }
        }
    }
}
