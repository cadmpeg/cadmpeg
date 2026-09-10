// SPDX-License-Identifier: Apache-2.0
//! Declaration codes for legacy ASCII persistence.

/// Stored attribute grammar selected by a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyTypeCode {
    /// Type 0 object node.
    Object,
    /// Type 1 signed decimal integer.
    Integer,
    /// Type 2 compact real.
    Real,
    /// Type 3 nullable byte-string scalar.
    NullableString,
    /// Type 4 byte-string scalar, including literal `NULL` bytes.
    ByteString,
    /// Type 5 unsigned decimal integer.
    Unsigned5,
    /// Type 6 compact real.
    Real6,
    /// Type 7 unsigned decimal integer.
    Unsigned7,
    /// Type 9 unsigned decimal integer.
    Unsigned9,
    /// Type 10 nullable byte-string scalar or array.
    String,
    /// Type 11 unsigned decimal integer.
    Unsigned11,
    /// A declaration code outside the defined grammars.
    Other(UnknownTypeCode),
}

/// An unrecognized code; known codes can only select their named variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnknownTypeCode(u8);

impl LegacyTypeCode {
    /// The identity token that names this grammar in a value record id.
    pub(crate) const fn identity_token(self) -> &'static str {
        match self {
            Self::Object => "object",
            Self::Integer => "integer",
            Self::Real => "real",
            Self::NullableString => "type_3",
            Self::ByteString => "type_4",
            Self::Unsigned5 => "type_5",
            Self::Real6 => "type_6",
            Self::Unsigned7 => "type_7",
            Self::Unsigned9 => "type_9",
            Self::String => "string",
            Self::Unsigned11 => "type_11",
            Self::Other(_) => "other",
        }
    }
}

impl From<u8> for LegacyTypeCode {
    fn from(code: u8) -> Self {
        match code {
            0 => Self::Object,
            1 => Self::Integer,
            2 => Self::Real,
            3 => Self::NullableString,
            4 => Self::ByteString,
            5 => Self::Unsigned5,
            6 => Self::Real6,
            7 => Self::Unsigned7,
            9 => Self::Unsigned9,
            10 => Self::String,
            11 => Self::Unsigned11,
            code => Self::Other(UnknownTypeCode(code)),
        }
    }
}
