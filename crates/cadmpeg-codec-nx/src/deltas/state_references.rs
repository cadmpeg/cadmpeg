// SPDX-License-Identifier: Apache-2.0
//! Complete reference-lane forms of a state frame.

use serde::{Deserialize, Serialize};
use super::xmt_reference::NonNullXmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "[u32; 4]", into = "[u32; 4]")]
pub(crate) enum StateReferences {
    Leading { references: [NonNullXmt; 3], last: Option<NonNullXmt> },
    Interleaved([NonNullXmt; 2]),
}

impl TryFrom<[u32; 4]> for StateReferences {
    type Error = &'static str;
    fn try_from([a, b, c, d]: [u32; 4]) -> Result<Self, Self::Error> {
        let error = "references: require three leading non-null references or two interleaved non-null references";
        let a = NonNullXmt::try_from(a).map_err(|_| error)?;
        let c = NonNullXmt::try_from(c).map_err(|_| error)?;
        if b == 1 && d == 1 { return Ok(Self::Interleaved([a, c])); }
        let b = NonNullXmt::try_from(b).map_err(|_| error)?;
        let last = if d == 1 { None } else { Some(NonNullXmt::try_from(d).map_err(|_| error)?) };
        Ok(Self::Leading { references: [a, b, c], last })
    }
}

impl From<StateReferences> for [u32; 4] {
    fn from(value: StateReferences) -> Self {
        match value {
            StateReferences::Leading { references: [a, b, c], last } => [a.into(), b.into(), c.into(), last.map_or(1, u32::from)],
            StateReferences::Interleaved([a, c]) => [a.into(), 1, c.into(), 1],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StateReferences;

    #[test]
    fn state_reference_forms_preserve_wire_and_reject_invalid_null_positions() {
        for json in ["[2,3,4,1]", "[2,3,4,5]", "[2,1,4,1]"] {
            let state: StateReferences = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&state).unwrap(), json);
        }
        for json in ["[0,3,4,1]", "[1,3,4,1]", "[2,1,4,5]", "[2,3,1,1]", "[2,3,4,0]"] {
            assert!(serde_json::from_str::<StateReferences>(json).unwrap_err().to_string().contains("references"));
        }
    }
}
