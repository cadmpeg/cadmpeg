// SPDX-License-Identifier: Apache-2.0
//! Declared storage families of type-80 attribute fields.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum AttributeField {
    Ignored,
    Integer,
    Real,
    Character,
    Point,
    Vector,
    Direction,
    Axis,
    Tag,
    Pointer,
    Unicode,
}

impl AttributeField {
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Ignored => 0,
            Self::Integer => 1,
            Self::Real => 2,
            Self::Character => 3,
            Self::Point => 4,
            Self::Vector => 5,
            Self::Direction => 6,
            Self::Axis => 7,
            Self::Tag => 8,
            Self::Pointer => 9,
            Self::Unicode => 10,
        }
    }
}

impl From<AttributeField> for u8 {
    fn from(value: AttributeField) -> Self { value.code() }
}

impl TryFrom<u8> for AttributeField {
    type Error = &'static str;

    fn try_from(code: u8) -> Result<Self, Self::Error> {
        match code {
            0 => Ok(Self::Ignored),
            1 => Ok(Self::Integer),
            2 => Ok(Self::Real),
            3 => Ok(Self::Character),
            4 => Ok(Self::Point),
            5 => Ok(Self::Vector),
            6 => Ok(Self::Direction),
            7 => Ok(Self::Axis),
            8 => Ok(Self::Tag),
            9 => Ok(Self::Pointer),
            10 => Ok(Self::Unicode),
            _ => Err("field_codes must contain declared attribute storage codes 0 through 10"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AttributeField;

    #[test]
    fn declared_fields_preserve_numeric_wire_and_reject_unknown_codes() {
        for code in 0..=10 {
            let wire = code.to_string();
            let field: AttributeField = serde_json::from_str(&wire).unwrap();
            assert_eq!(field.code(), code);
            assert_eq!(serde_json::to_string(&field).unwrap(), wire);
        }
        for code in [11, 255] {
            assert!(serde_json::from_str::<AttributeField>(&code.to_string()).is_err());
        }
    }
}
