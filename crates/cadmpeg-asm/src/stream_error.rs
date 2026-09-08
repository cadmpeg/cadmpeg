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
