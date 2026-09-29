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
    Resource(cadmpeg_core::CodecError),
}

impl StreamFailure {
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
            Self::Resource(error) => error,
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

impl From<cadmpeg_core::CodecError> for StreamFailure {
    fn from(error: cadmpeg_core::CodecError) -> Self {
        Self::Resource(error)
    }
}

impl std::fmt::Display for StreamFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) | Self::Malformed(error) | Self::NotImplemented(error) => {
                error.fmt(f)
            }
            Self::Resource(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for StreamFailure {}

#[cfg(test)]
mod tests {
    use super::{StreamError, StreamFailure, StreamFormat};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

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
