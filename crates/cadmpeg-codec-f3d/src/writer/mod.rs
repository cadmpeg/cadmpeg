// SPDX-License-Identifier: Apache-2.0
//! Encode source-less F3D archives and apply supported edits to retained source
//! archives.

pub(crate) mod capacity;
pub(crate) mod generate;
pub(crate) mod patch;
pub(crate) mod primitives;
mod synthesis;
pub(crate) mod target;

/// Why the F3D writer cannot write content as it stands (#217 §5.0). It never
/// leaves the writer: synthesis reduces the content, each reduction with a
/// loss that names it.
#[derive(Debug)]
pub(crate) enum WriteError {
    /// The writer has no form for the content.
    Unsupported(String),
    /// The content does not hold what its F3D record needs.
    Unwritable(String),
    /// A read or a resource account the writer uses failed.
    Codec(cadmpeg_core::CodecError),
}

impl WriteError {
    pub(crate) fn unwritable(message: impl std::fmt::Display) -> Self {
        Self::Unwritable(message.to_string())
    }
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(message) | Self::Unwritable(message) => formatter.write_str(message),
            Self::Codec(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<cadmpeg_core::CodecError> for WriteError {
    fn from(error: cadmpeg_core::CodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<cadmpeg_core::decode::ResourceLimit> for WriteError {
    fn from(limit: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Codec(limit.into())
    }
}

impl From<std::io::Error> for WriteError {
    fn from(error: std::io::Error) -> Self {
        Self::Codec(error.into())
    }
}

impl From<cadmpeg_ir::geometry::nurbs::NurbsError> for WriteError {
    fn from(error: cadmpeg_ir::geometry::nurbs::NurbsError) -> Self {
        Self::Codec(error.into())
    }
}

impl From<cadmpeg_ir::native::NativeConvertError> for WriteError {
    fn from(error: cadmpeg_ir::native::NativeConvertError) -> Self {
        Self::Codec(error.into())
    }
}

#[cfg(test)]
mod tests;
