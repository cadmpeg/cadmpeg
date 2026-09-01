// SPDX-License-Identifier: Apache-2.0
//! Errors returned by codec parsing and resource enforcement.

use crate::decode::{ErrorContext, ResourceLimit, SourceLocation};
use crate::dialect::DialectLayers;
use crate::target::TargetRefusal;

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
    /// A codec backend returned a result for a format other than its own id.
    ///
    /// This is an implementation contract failure, not a statement about the
    /// input bytes. Public codec wrappers construct it after a backend returns.
    #[error(
        "codec {codec:?} {operation} returned format {reported:?}; the backend must report its own format"
    )]
    ContractViolation {
        /// Codec id whose backend ran.
        codec: &'static str,
        /// Sealed operation that detected the violation.
        operation: &'static str,
        /// Format returned by the backend.
        reported: String,
    },
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
    /// The document was identified, and its dialect is not supported.
    ///
    /// Never reported as [`CodecError::WrongFormat`]: the bytes are this
    /// codec's format, and the codec says so by carrying the identification it
    /// made. Identity survives refusal, so a caller can name what it was handed
    /// even though nothing was decoded.
    ///
    /// The SAT codec constructs this variant for a recognized stream kind that
    /// does not frame. The STEP codec constructs it for the Part 26 HDF5, Part
    /// 28 XML, and AP242 business-object XML encodings that it identifies but
    /// does not decode.
    ///
    /// The identification is boxed: it is the widest payload any variant of
    /// this enum carries, and every `Result<_, CodecError>` in the workspace
    /// would otherwise grow to its width.
    #[error("unsupported {} dialect {}: {message}", .dialects.primary().format(), .dialects.primary().dialect())]
    UnsupportedDialect {
        /// Every format layer identified before the refusal.
        dialects: Box<DialectLayers>,
        /// Why the identified dialect is not supported.
        message: String,
    },
    /// The encoder could not resolve or deliver a write target.
    #[error("{0}")]
    UnsupportedTarget(Box<TargetRefusal>),
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

impl From<TargetRefusal> for CodecError {
    fn from(refusal: TargetRefusal) -> Self {
        Self::UnsupportedTarget(Box::new(refusal))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::CodecError;
    use crate::dialect::{DialectId, DialectLayers, DialectMatch};

    #[test]
    fn malformed_constructor_formats_the_message_once() {
        let error = CodecError::malformed(format_args!("field {} is invalid", 7));

        assert_eq!(error.to_string(), "malformed container: field 7 is invalid");
    }

    #[test]
    fn a_dialect_refusal_keeps_the_identification_it_refused() {
        let error = CodecError::UnsupportedDialect {
            dialects: Box::new(
                DialectLayers::new(
                    DialectMatch::refused(DialectId::pinned("acis:save-format-binary-other"))
                        .with_declared(BTreeMap::from([(
                            "save_format".to_owned(),
                            "700".to_owned(),
                        )])),
                    vec![DialectMatch::refused(DialectId::pinned("sat:binary"))],
                )
                .expect("the test layers have distinct format keys"),
            ),
            message: "save format 700 has no read grammar".into(),
        };

        assert_eq!(
            error.to_string(),
            "unsupported acis dialect acis:save-format-binary-other: save format 700 has no read grammar"
        );
        let CodecError::UnsupportedDialect { dialects, .. } = &error else {
            panic!("the variant just built is the one matched");
        };
        assert_eq!(
            dialects.primary().dialect().as_str(),
            "acis:save-format-binary-other"
        );
        assert_eq!(dialects.primary().declared()["save_format"], "700");
        assert_eq!(dialects.iter().count(), 2);
    }
}
