// SPDX-License-Identifier: Apache-2.0
//! The seven serialized event-action codes of a type-80 declaration.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum AttributeAction {
    Code0,
    Code1,
    Code2,
    Code3,
    Code4,
    Code5,
    Code6,
}

impl AttributeAction {
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Code0 => 0,
            Self::Code1 => 1,
            Self::Code2 => 2,
            Self::Code3 => 3,
            Self::Code4 => 4,
            Self::Code5 => 5,
            Self::Code6 => 6,
        }
    }
}

impl From<AttributeAction> for u8 {
    fn from(value: AttributeAction) -> Self {
        value.code()
    }
}

impl TryFrom<u8> for AttributeAction {
    type Error = &'static str;

    fn try_from(code: u8) -> Result<Self, Self::Error> {
        match code {
            0 => Ok(Self::Code0),
            1 => Ok(Self::Code1),
            2 => Ok(Self::Code2),
            3 => Ok(Self::Code3),
            4 => Ok(Self::Code4),
            5 => Ok(Self::Code5),
            6 => Ok(Self::Code6),
            _ => Err("action_codes must contain codes 0 through 6"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AttributeAction;

    #[test]
    fn action_codes_preserve_wire_and_reject_unknown_codes() {
        for code in 0..=6 {
            let wire = code.to_string();
            let action: AttributeAction = serde_json::from_str(&wire).unwrap();
            assert_eq!(action.code(), code);
            assert_eq!(serde_json::to_string(&action).unwrap(), wire);
        }
        for code in [7, 255] {
            assert!(serde_json::from_str::<AttributeAction>(&code.to_string()).is_err());
        }
    }
}
