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

/// Admitted branch values for `DraftIdentityBranch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "u8", into = "u8")]
#[repr(u8)]
pub enum DraftIdentityBranch {
    /// Serialized `0x02` form.
    Form02 = 2,
    /// Serialized `0x03` form.
    Form03 = 3,
}
impl From<DraftIdentityBranch> for u8 {
    fn from(value: DraftIdentityBranch) -> Self {
        value as Self
    }
}
impl TryFrom<u8> for DraftIdentityBranch {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            2 => Ok(Self::Form02),
            3 => Ok(Self::Form03),
            _ => Err("DraftIdentityBranch.branch is not an admitted discriminator"),
        }
    }
}

/// Admitted branch values for `DraftBinary32Branch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "u8", into = "u8")]
#[repr(u8)]
pub enum DraftBinary32Branch {
    /// Serialized `0x03` form.
    Form03 = 3,
    /// Serialized `0x04` form.
    Form04 = 4,
}
impl From<DraftBinary32Branch> for u8 {
    fn from(value: DraftBinary32Branch) -> Self {
        value as Self
    }
}
impl TryFrom<u8> for DraftBinary32Branch {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            3 => Ok(Self::Form03),
            4 => Ok(Self::Form04),
            _ => Err("DraftBinary32Branch.branch is not an admitted discriminator"),
        }
    }
}

impl DraftBinary32Branch {
    /// Complete serialized discriminator for this branch.
    pub fn discriminator(self) -> [u8; 18] {
        match self {
            Self::Form04 => [
                0x90, 0x18, 0x45, 0x01, 0x04, 0x01, 0x04, 0x01, 0xc0, 0x45, 0x04, 0x04, 0x80, 0x86,
                0x02, 0x00, 0x03, 0x00,
            ],
            Self::Form03 => [
                0x90, 0x18, 0x45, 0x01, 0x04, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86,
                0x02, 0x00, 0x03, 0x00,
            ],
        }
    }
}

/// Admitted mode values for a surface construction branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "u8", into = "u8")]
#[repr(u8)]
pub enum SurfaceBranchMode {
    /// Serialized `0x16` form.
    Form16 = 0x16,
    /// Serialized `0x40` form.
    Form40 = 0x40,
}

impl From<SurfaceBranchMode> for u8 {
    fn from(value: SurfaceBranchMode) -> Self {
        value as Self
    }
}

impl TryFrom<u8> for SurfaceBranchMode {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x16 => Ok(Self::Form16),
            0x40 => Ok(Self::Form40),
            _ => Err("SurfaceBranchMode.mode is not an admitted discriminator"),
        }
    }
}
