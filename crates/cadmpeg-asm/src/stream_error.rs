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

    /// Convert syntax failures to malformed errors with charged diagnostic text.
    pub fn into_malformed(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> cadmpeg_core::CodecError {
        self.into_codec_error(ctx, |error| {
            stream_message(ctx, &error).map(cadmpeg_core::CodecError::Malformed)
        })
    }

    /// Keep the caller's syntax classification and propagate charged formatting failures.
    /// The callback must use context text operations for variable-size diagnostics.
    pub fn into_codec_error(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        parse: impl FnOnce(StreamError) -> Result<cadmpeg_core::CodecError, cadmpeg_core::CodecError>,
    ) -> cadmpeg_core::CodecError {
        match self {
            Self::Parse(error) => parse(error).unwrap_or_else(std::convert::identity),
            Self::Malformed(error) => match stream_message(ctx, &error) {
                Ok(message) => cadmpeg_core::CodecError::Malformed(message),
                Err(refusal) => refusal,
            },
            Self::NotImplemented(error) => match stream_message(ctx, &error) {
                Ok(message) => cadmpeg_core::CodecError::NotImplemented(message),
                Err(refusal) => refusal,
            },
            Self::Resource(error) => error.into(),
            Self::Operation(error) => error.into_codec_error(),
        }
    }
}

fn stream_message(
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

    fn stream_error() -> StreamError {
        StreamError {
            format: StreamFormat::Binary,
            offset: 17,
            reason: "source detail".into(),
        }
    }

    #[test]
    fn stream_parse_keeps_fresh_callback_classification() {
        let mut failures: Vec<_> = (0..64)
            .map(|_| StreamFailure::Parse(stream_error()))
            .collect();
        let mut calls = 0;
        crate::test_support::with_entry_context(|ctx, original| {
            if original.is_some() {
                return;
            }
            let error = failures
                .pop()
                .expect("prepared syntax error")
                .into_codec_error(ctx, |error| {
                    calls += 1;
                    assert_eq!(error.format, StreamFormat::Binary);
                    assert_eq!(error.offset, 17);
                    Ok(CodecError::Malformed(error.reason))
                });
            assert!(matches!(error, CodecError::Malformed(reason) if reason == "source detail"));
        });
        assert_eq!(calls, 64);
        assert!(failures.is_empty());
    }

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
                assert!(matches!(
                    error,
                    CodecError::Malformed(_) | CodecError::NotImplemented(_) | CodecError::Io(_)
                ));
            }
        })
        .expect("service test context");
    }

    #[test]
    fn stream_resource_variant_contains_only_the_original_refusal() {
        crate::test_support::with_service_context(&[], |ctx| {
            let CodecError::ResourceLimit(refusal) = ctx.refuse_codec_limit("stream refusal", 3, 2)
            else {
                panic!("resource-only core refusal");
            };
            assert_eq!(
                OperationFailure::try_from(CodecError::ResourceLimit(refusal)).unwrap_err(),
                refusal
            );
            for failure in [
                StreamFailure::from(refusal),
                StreamFailure::from_operation(CodecError::ResourceLimit(refusal)),
            ] {
                assert!(matches!(&failure, StreamFailure::Resource(limit) if *limit == refusal));
                let error = failure.into_codec_error(ctx, |_| panic!("resource is not framing"));
                assert!(matches!(error, CodecError::ResourceLimit(limit) if limit == refusal));
            }
        })
        .expect("service test context");
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
        let error = StreamFailure::NotImplemented(error).into_malformed(&ctx);
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("expected resource refusal, got {error:?}");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.operation, "ASM stream error text");
    }
    #[test]
    fn syntax_and_malformed_stream_errors_refuse_diagnostic_storage() {
        for malformed in [false, true] {
            let reason = "x".repeat(4096);
            let expected = format!("SAT parse failed at byte 12: {reason}");
            let refusal = crate::test_support::resource_limit_at(
                &[],
                ResourceDimension::RetainedBytes,
                "ASM stream error text",
                |ctx| {
                    let error = StreamError {
                        format: StreamFormat::Text,
                        offset: 12,
                        reason: reason.clone(),
                    };
                    let failure = if malformed {
                        StreamFailure::Malformed(error)
                    } else {
                        StreamFailure::Parse(error)
                    };
                    Err::<(), _>(failure.into_malformed(ctx))
                },
            );
            assert_eq!(refusal.operation, "ASM stream error text");
            assert_eq!(refusal.used, 0);
            assert_eq!(refusal.additional, u64::try_from(expected.len()).unwrap());
            assert_eq!(refusal.limit + 1, refusal.additional);
            {
                let ctx = cadmpeg_test_support::service_decode_context();
                let error = StreamError {
                    format: StreamFormat::Text,
                    offset: 12,
                    reason: reason.clone(),
                };
                let failure = if malformed {
                    StreamFailure::Malformed(error)
                } else {
                    StreamFailure::Parse(error)
                };
                assert!(
                    matches!(failure.into_malformed(&ctx), CodecError::Malformed(message) if message == expected)
                );
                ctx.finish_session().unwrap();
            }
        }
    }

    #[test]
    fn stream_parse_callback_refuses_charged_whole_error_formatting() {
        let reason = "x".repeat(4096);
        let expected = format!("SAB framing failed: SAB framing failed at byte 12: {reason}");
        let refusal = crate::test_support::resource_limit_at(
            &[],
            ResourceDimension::RetainedBytes,
            "test stream callback error text",
            |ctx| {
                let mut output = None;
                let failure = StreamFailure::Parse(StreamError {
                    format: StreamFormat::Binary,
                    offset: 12,
                    reason: reason.clone(),
                });
                let error = failure.into_codec_error(ctx, |error| {
                    let message = ctx.format_retained(
                        format_args!("SAB framing failed: {error}"),
                        "test stream callback error text",
                    )?;
                    output = Some(message.clone());
                    Ok(CodecError::Malformed(message))
                });
                assert!(output.is_none());
                Err::<(), _>(error)
            },
        );
        assert_eq!(refusal.operation, "test stream callback error text");
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.additional, u64::try_from(expected.len()).unwrap());
        assert_eq!(refusal.limit + 1, refusal.additional);
    }
}
