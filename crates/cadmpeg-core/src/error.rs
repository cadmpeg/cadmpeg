// SPDX-License-Identifier: Apache-2.0
//! Errors returned by codec parsing and resource enforcement.

use crate::decode::{ErrorContext, ResourceLimit, SourceLocation};

/// Errors a codec can raise.
///
/// Marked `#[non_exhaustive]`: external exhaustive matches must carry a
/// wildcard arm.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CodecError {
    /// The bytes are not this codec's format.
    #[error("not the expected format: {0}")]
    WrongFormat(String),
    /// The container was structurally malformed.
    #[error("malformed container: {0}")]
    Malformed(String),
    /// The document supplied to an encoder violates its input contract.
    #[error("invalid encoder input: {0}")]
    InvalidInput(String),
    /// A required read extended past the end of its window after commitment.
    ///
    /// Distinct from [`CodecError::Malformed`]: a truncation is missing input,
    /// not an inconsistency inside the bytes that are present.
    #[error(
        "truncated input during {} at space {} offset {}",
        .context.operation, .location.space.index(), .location.offset
    )]
    Truncated {
        /// Where the truncated read began.
        location: SourceLocation,
        /// Static context for the failure.
        context: ErrorContext,
    },
    /// A resource limit refused the decode: policy or the allocator.
    ///
    /// Never reported as [`CodecError::Malformed`]: a budget refusal is a
    /// statement about policy, not about the input.
    #[error(
        "resource limit on {:?}: {:?} (limit {}, used {}, requested {})",
        .0.dimension, .0.reason, .0.limit, .0.used, .0.additional
    )]
    ResourceLimit(ResourceLimit),
    /// The codec does not implement a required capability.
    #[error("not implemented yet: {0}")]
    NotImplemented(String),
    /// Underlying I/O failure.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl CodecError {
    /// Builds a malformed-container error from a displayable message.
    pub fn malformed(message: impl std::fmt::Display) -> Self {
        Self::Malformed(message.to_string())
    }

    /// Builds a truncation error at a qualified source location.
    pub const fn truncated(location: SourceLocation, operation: &'static str) -> Self {
        Self::Truncated {
            location,
            context: ErrorContext {
                operation,
                location: Some(location),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CodecError;

    #[test]
    fn malformed_constructor_formats_the_message_once() {
        let error = CodecError::malformed(format_args!("field {} is invalid", 7));

        assert_eq!(error.to_string(), "malformed container: field 7 is invalid");
    }
}
