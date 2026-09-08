// SPDX-License-Identifier: Apache-2.0
//! Checked schema-reference preamble payload.

use serde::{Deserialize, Serialize};
use std::num::NonZeroU16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StateForm {
    Zero,
    Two,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Type81,
    Type82,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PreambleWire", into = "PreambleWire")]
pub(crate) struct PreambleState {
    identity: u16,
    first_reference: u32,
    linked: bool,
    form: StateForm,
    last_word: u32,
    count: NonZeroU16,
    entries: Vec<(EntryKind, u32)>,
    terminal_value: u16,
}

impl PreambleState {
    pub(crate) fn new(
        identity: u16,
        references: [u32; 2],
        state_references: [u32; 3],
        state_words: [u32; 4],
        count: u16,
        entries: Vec<(u16, u32)>,
        terminal_value: u16,
    ) -> Result<Self, &'static str> {
        if identity <= 1 {
            return Err("identity: must exceed one");
        }
        let [first_reference, second] = references;
        if first_reference <= 1
            || first_reference.checked_add(1) != Some(second)
            || second.checked_add(1).is_none()
        {
            return Err(
                "references: require consecutive non-null references with a state successor",
            );
        }
        let linked = if state_references == [1; 3] {
            false
        } else if state_references == [1, second + 1, 1] {
            true
        } else {
            return Err("state_reference: must be the reference successor between nulls");
        };
        let form = match state_words {
            [0, 0, 1, _] => StateForm::Zero,
            [2, 0, 1, _] => StateForm::Two,
            _ => return Err("state_words: require [0|2, 0, 1, value]"),
        };
        let count = NonZeroU16::new(count).ok_or("count: must be nonzero")?;
        if entries.is_empty() {
            return Err("entries: require at least one entry");
        }
        let entries = entries
            .into_iter()
            .map(|(kind, reference)| {
                if reference <= 1 {
                    return Err("entries.reference: must exceed one");
                }
                let kind = match kind {
                    81 => EntryKind::Type81,
                    82 => EntryKind::Type82,
                    _ => return Err("entries.kind: must be 81 or 82"),
                };
                Ok((kind, reference))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            identity,
            first_reference,
            linked,
            form,
            last_word: state_words[3],
            count,
            entries,
            terminal_value,
        })
    }

    pub(crate) fn identity(&self) -> u16 {
        self.identity
    }
    pub(crate) fn references(&self) -> [u32; 2] {
        [self.first_reference, self.first_reference + 1]
    }
    pub(crate) fn state_reference(&self) -> Option<u32> {
        self.linked.then_some(self.first_reference + 2)
    }
    #[cfg(test)]
    pub(crate) fn state_references(&self) -> [u32; 3] {
        [1, self.state_reference().unwrap_or(1), 1]
    }
    pub(crate) fn state_words(&self) -> [u32; 4] {
        [
            match self.form {
                StateForm::Zero => 0,
                StateForm::Two => 2,
            },
            0,
            1,
            self.last_word,
        ]
    }
    pub(crate) fn count(&self) -> u16 {
        self.count.get()
    }
    pub(crate) fn entries(&self) -> Vec<(u16, u32)> {
        self.entries
            .iter()
            .map(|(kind, reference)| {
                (
                    match kind {
                        EntryKind::Type81 => 81,
                        EntryKind::Type82 => 82,
                    },
                    *reference,
                )
            })
            .collect()
    }
    pub(crate) fn terminal_value(&self) -> u16 {
        self.terminal_value
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct PreambleWire {
    identity: u16,
    references: [u32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    state_reference: Option<u32>,
    state_words: [u32; 4],
    count: u16,
    entries: Vec<(u16, u32)>,
    terminal_value: u16,
}

impl From<PreambleState> for PreambleWire {
    fn from(state: PreambleState) -> Self {
        Self {
            identity: state.identity(),
            references: state.references(),
            state_reference: state.state_reference(),
            state_words: state.state_words(),
            count: state.count(),
            entries: state.entries(),
            terminal_value: state.terminal_value(),
        }
    }
}

impl TryFrom<PreambleWire> for PreambleState {
    type Error = &'static str;
    fn try_from(wire: PreambleWire) -> Result<Self, Self::Error> {
        if wire.state_reference == Some(1) {
            return Err("state_reference: non-null reference required when present");
        }
        Self::new(
            wire.identity,
            wire.references,
            [1, wire.state_reference.unwrap_or(1), 1],
            wire.state_words,
            wire.count,
            wire.entries,
            wire.terminal_value,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::PreambleState;

    #[test]
    // Keep the source-word spelling in this wire fixture.
    #[allow(clippy::unreadable_literal)]
    fn preamble_wire_preserves_independent_count_and_rejects_invalid_lanes() {
        let json = r#"{"identity":300,"references":[40000,40001],"state_words":[2,0,1,55],"count":7,"entries":[[81,4],[82,40000],[81,5]],"terminal_value":9}"#;
        let state: PreambleState = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&state).unwrap(), json);
        assert_eq!(state.state_references(), [1; 3]);
        let valid = serde_json::to_value(&state).unwrap();
        for (field, invalid) in [
            ("identity", serde_json::json!(1)),
            ("references", serde_json::json!([40000, 40002])),
            (
                "references",
                serde_json::json!([4294967294u32, 4294967295u32]),
            ),
            ("state_reference", serde_json::json!(1)),
            ("state_reference", serde_json::json!(40003)),
            ("state_words", serde_json::json!([2, 1, 1, 55])),
            ("state_words", serde_json::json!([1, 0, 1, 55])),
            ("count", serde_json::json!(0)),
            ("entries", serde_json::json!([])),
            ("entries", serde_json::json!([[80, 4]])),
            ("entries", serde_json::json!([[82, 1]])),
        ] {
            let mut wire = valid.clone();
            wire[field] = invalid;
            assert!(serde_json::from_value::<PreambleState>(wire)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
        let linked = json.replace(
            "\"state_words\"",
            "\"state_reference\":40002,\"state_words\"",
        );
        let state: PreambleState = serde_json::from_str(&linked).unwrap();
        assert_eq!(state.state_references(), [1, 40002, 1]);
        assert_eq!(serde_json::to_string(&state).unwrap(), linked);
    }
}
