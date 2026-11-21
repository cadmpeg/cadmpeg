// SPDX-License-Identifier: Apache-2.0
//! Reader refusal conversion.
use cadmpeg_core::{decode::ParseError, CodecError};

#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error("{0}")]
    Decode(String),
    #[error(transparent)]
    Codec(#[from] CodecError),
}
impl From<ParseError> for Error {
    fn from(value: ParseError) -> Self {
        Self::Codec(value.into())
    }
}
impl From<Error> for CodecError {
    fn from(value: Error) -> Self {
        match value {
            Error::Decode(message) => Self::malformed(message),
            Error::Codec(error) => error,
        }
    }
}
