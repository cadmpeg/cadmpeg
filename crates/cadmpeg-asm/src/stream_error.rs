// SPDX-License-Identifier: Apache-2.0
//! Parse failures shared by text and binary model streams.

/// Encoding whose parser rejected a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFormat {
    /// Text SAT records.
    Text,
    /// Binary SAB records.
    Binary,
}

/// A stream parse failure and the byte where parsing stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamError {
    /// Stream encoding.
    pub format: StreamFormat,
    /// Byte offset in the stream.
    pub offset: usize,
    /// Parse failure detail.
    pub reason: String,
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let operation = match self.format {
            StreamFormat::Text => "SAT parse",
            StreamFormat::Binary => "SAB framing",
        };
        write!(
            f,
            "{operation} failed at byte {}: {}",
            self.offset, self.reason
        )
    }
}

impl std::error::Error for StreamError {}

/// A framing failure with its decode refusal class intact.
#[derive(Debug)]
pub enum StreamFailure {
    /// The stream did not frame under its declared encoding.
    Parse(StreamError),
    /// A recognized grammar carried inconsistent values.
    Malformed(StreamError),
    /// A valid source length cannot be represented in the destination unit.
    NotImplemented(StreamError),
    /// The active decode policy refused materialization or work.
    Resource(cadmpeg_core::decode::ResourceLimit),
    /// A context operation failed outside the stream grammar.
    Operation(OperationFailure),
}

/// An operation failure whose codec class is not a resource refusal.
#[derive(Debug)]
pub struct OperationFailure(cadmpeg_core::CodecError);

impl TryFrom<cadmpeg_core::CodecError> for OperationFailure {
    type Error = cadmpeg_core::decode::ResourceLimit;

    fn try_from(error: cadmpeg_core::CodecError) -> Result<Self, Self::Error> {
        match error {
            cadmpeg_core::CodecError::ResourceLimit(refusal) => Err(refusal),
            error => Ok(Self(error)),
        }
    }
}

impl OperationFailure {
    /// Return the operation's existing codec classification.
    pub fn into_codec_error(self) -> cadmpeg_core::CodecError {
        self.0
    }
}

impl std::fmt::Display for OperationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl StreamFailure {
    /// Classify a context operation without labeling other errors as resources.
    pub(crate) fn from_operation(error: cadmpeg_core::CodecError) -> Self {
        match OperationFailure::try_from(error) {
            Ok(error) => Self::Operation(error),
            Err(refusal) => Self::Resource(refusal),
        }
    }

    /// Keep a caller's existing framing classification for syntax errors.
    pub fn into_codec_error(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        parse: impl FnOnce(StreamError) -> cadmpeg_core::CodecError,
    ) -> cadmpeg_core::CodecError {
        match self {
            Self::Parse(error) => parse(error),
            Self::Malformed(error) => cadmpeg_core::CodecError::malformed(error),
            Self::NotImplemented(error) => match unsupported_message(ctx, &error) {
                Ok(message) => cadmpeg_core::CodecError::NotImplemented(message),
                Err(refusal) => refusal,
            },
            Self::Resource(error) => error.into(),
            Self::Operation(error) => error.into_codec_error(),
        }
    }
}

fn unsupported_message(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    error: &StreamError,
) -> Result<String, cadmpeg_core::CodecError> {
    let prefix = match error.format {
        StreamFormat::Text => "SAT parse",
        StreamFormat::Binary => "SAB framing",
    };
    ctx.format_retained(
        format_args!("{prefix} failed at byte {}: {}", error.offset, error.reason),
        "ASM stream error text",
    )
}

impl From<StreamError> for StreamFailure {
    fn from(error: StreamError) -> Self {
        Self::Parse(error)
    }
}

impl From<cadmpeg_core::decode::ResourceLimit> for StreamFailure {
    fn from(error: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Resource(error)
    }
}

impl std::fmt::Display for StreamFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) | Self::Malformed(error) | Self::NotImplemented(error) => {
                error.fmt(f)
            }
            Self::Resource(error) => cadmpeg_core::CodecError::ResourceLimit(*error).fmt(f),
            Self::Operation(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for StreamFailure {}

#[cfg(test)]
mod tests {
    use super::{OperationFailure, StreamError, StreamFailure, StreamFormat};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn stream_operation_errors_keep_their_codec_class() {
        crate::test_support::with_service_context(&[], |ctx| {
            for error in [
                CodecError::Malformed("malformed operation".into()),
                CodecError::NotImplemented("unsupported operation".into()),
                CodecError::Io(std::io::Error::other("I/O operation")),
            ] {
                let expected = error.to_string();
                let failure = StreamFailure::from_operation(error);
                assert!(matches!(&failure, StreamFailure::Operation(_)));
                let error = failure.into_codec_error(ctx, |_| panic!("operation is not framing"));
                assert_eq!(error.to_string(), expected);
                assert!(matches!(error, CodecError::Malformed(_) | CodecError::NotImplemented(_) | CodecError::Io(_)));
            }
        }).expect("service test context");
    }

    #[test]
    fn stream_resource_variant_contains_only_the_original_refusal() {
        crate::test_support::with_service_context(&[], |ctx| {
            let CodecError::ResourceLimit(refusal) = ctx.refuse_codec_limit("stream refusal", 3, 2)
                else { panic!("resource-only core refusal"); };
            assert_eq!(OperationFailure::try_from(CodecError::ResourceLimit(refusal)).unwrap_err(), refusal);
            for failure in [
                StreamFailure::from(refusal),
                StreamFailure::from_operation(CodecError::ResourceLimit(refusal)),
            ] {
                assert!(matches!(&failure, StreamFailure::Resource(limit) if *limit == refusal));
                let error = failure.into_codec_error(ctx, |_| panic!("resource is not framing"));
                assert!(matches!(error, CodecError::ResourceLimit(limit) if limit == refusal));
            }
        }).expect("service test context");
    }

    #[test]
    fn unsupported_stream_error_text_refuses_retained_limit() {
        let error = StreamError {
            format: StreamFormat::Text,
            offset: 12,
            reason: "x".repeat(32),
        };
        let expected = error.to_string();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(expected.len()) - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty test input fits input limit");
        let error =
            StreamFailure::NotImplemented(error).into_codec_error(&ctx, CodecError::malformed);
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("expected resource refusal, got {error:?}");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.operation, "ASM stream error text");
    }
}
