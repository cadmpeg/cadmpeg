// SPDX-License-Identifier: Apache-2.0
//! Crate-local `CodecError` constructors.

use cadmpeg_core::CodecError;

pub(crate) fn malformed(message: impl Into<String>) -> CodecError {
    CodecError::Malformed(message.into())
}
