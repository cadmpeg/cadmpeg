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
        parse: impl FnOnce(StreamError) -> cadmpeg_core::CodecError,
    ) -> cadmpeg_core::CodecError {
        match self {
            Self::Parse(error) => parse(error),
            Self::Malformed(error) => cadmpeg_core::CodecError::malformed(error),
            Self::NotImplemented(error) => {
                cadmpeg_core::CodecError::NotImplemented(error.to_string())
            }
            Self::Resource(error) => error,
        }
    }
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
