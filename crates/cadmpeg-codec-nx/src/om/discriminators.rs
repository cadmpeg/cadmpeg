// SPDX-License-Identifier: Apache-2.0
//! Closed object-model discriminator values.

/// Admitted `row_kind` values for an operation-state row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "u8", into = "u8")]
#[repr(u8)]
pub enum OperationStateCounterKind {
    /// Serialized `0x01` form.
    Form01 = 0x01,
    /// Serialized `0x02` form.
    Form02 = 0x02,
}

impl From<OperationStateCounterKind> for u8 {
    fn from(value: OperationStateCounterKind) -> Self {
        value as Self
    }
}

impl TryFrom<u8> for OperationStateCounterKind {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Self::Form01),
            0x02 => Ok(Self::Form02),
            _ => Err("OperationStateCounterKind.row_kind is not an admitted discriminator"),
        }
    }
}

/// Admitted `tag` values for an operation-state row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "u8", into = "u8")]
#[repr(u8)]
pub enum OperationStatePairTag {
    /// Serialized `0x4f` form.
    Form4f = 0x4f,
    /// Serialized `0x48` form.
    Form48 = 0x48,
}

impl From<OperationStatePairTag> for u8 {
    fn from(value: OperationStatePairTag) -> Self {
        value as Self
    }
}

impl TryFrom<u8> for OperationStatePairTag {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x4f => Ok(Self::Form4f),
            0x48 => Ok(Self::Form48),
            _ => Err("OperationStatePairTag.tag is not an admitted discriminator"),
        }
    }
}
