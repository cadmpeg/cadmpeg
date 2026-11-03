// SPDX-License-Identifier: Apache-2.0
//! Private encode-path error for the SLDPRT writer (#231, #237).
//!
//! `UnsupportedContent` is internal control flow: a patch that cannot express
//! an edit declines to synthesis with `PatchDeclined`. A private typed enum
//! carries that decision so no `CodecError` construction on the encode path
//! doubles as control flow. The enum never leaves the writer; public
//! signatures keep returning `CodecError` until #239 switches them to
//! `EncodeError`.

use cadmpeg_core::CodecError;

/// Why a patch-path attempt did not produce bytes.
#[derive(Debug)]
pub(crate) enum WriteError {
    /// The patch cannot express the edit; synthesis owns the reason.
    Decline(String),
    /// A precondition failure that stays an error on this path.
    Fatal(CodecError),
}

impl WriteError {
    /// The synthesis reason for either variant.
    pub(crate) fn reason(&self) -> String {
        match self {
            Self::Decline(reason) => reason.clone(),
            Self::Fatal(error) => error.to_string(),
        }
    }
}

impl From<CodecError> for WriteError {
    fn from(error: CodecError) -> Self {
        match error {
            CodecError::UnsupportedContent(reason) => Self::Decline(reason),
            error => Self::Fatal(error),
        }
    }
}

impl From<WriteError> for CodecError {
    fn from(error: WriteError) -> Self {
        match error {
            WriteError::Decline(reason) => Self::UnsupportedContent(reason),
            WriteError::Fatal(error) => error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WriteError;

    #[test]
    fn unsupported_content_becomes_a_decline() {
        let error = WriteError::from(cadmpeg_core::CodecError::UnsupportedContent(
            "no layout".into(),
        ));
        assert!(matches!(error, WriteError::Decline(_)));
        assert_eq!(error.reason(), "no layout");
    }

    #[test]
    fn other_errors_stay_fatal_but_name_their_reason() {
        let error = WriteError::from(cadmpeg_core::CodecError::Malformed("bad".into()));
        assert!(matches!(error, WriteError::Fatal(_)));
        assert!(error.reason().contains("bad"));
    }
}
