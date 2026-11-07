// SPDX-License-Identifier: Apache-2.0
//! Private encode-path error for the SLDPRT writer (#231, #237).
//!
//! `UnsupportedContent` is internal control flow: a patch that cannot express
//! an edit declines to synthesis with `PatchDeclined`. A private typed enum
//! carries that decision and typed target capacities through planning.

use cadmpeg_core::CodecError;

/// Why a patch-path attempt did not produce bytes.
#[derive(Debug)]
pub(crate) enum WriteError {
    /// The patch cannot express the edit; synthesis owns the reason.
    Decline(String),
    /// A precondition failure that stays an error on this path.
    Fatal(CodecError),
    /// An output write or seek failed.
    Io(std::io::Error),
    Capacity {
        field: &'static cadmpeg_ir::codec::write::CapacityField,
        value: u64,
    },
}

impl WriteError {
    /// The synthesis reason for either variant.
    pub(crate) fn reason(&self) -> String {
        match self {
            Self::Decline(reason) => reason.clone(),
            Self::Fatal(error) => error.to_string(),
            Self::Io(error) => error.to_string(),
            Self::Capacity { field, value } => format!(
                "target capacity exceeded for {}: limit {}, value {value}",
                field.field, field.limit
            ),
        }
    }
}

impl From<CodecError> for WriteError {
    fn from(error: CodecError) -> Self {
        match error {
            CodecError::UnsupportedContent(reason) => Self::Decline(reason),
            CodecError::Io(error) => Self::Io(error),
            error => Self::Fatal(error),
        }
    }
}

impl From<cadmpeg_ir::native::NativeConvertError> for WriteError {
    fn from(error: cadmpeg_ir::native::NativeConvertError) -> Self {
        CodecError::from(error).into()
    }
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decline(reason) => write!(formatter, "unsupported writer content: {reason}"),
            _ => formatter.write_str(&self.reason()),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<std::io::Error> for WriteError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<WriteError> for cadmpeg_ir::codec::write::EncodeError {
    fn from(error: WriteError) -> Self {
        match error {
            WriteError::Decline(reason) => cadmpeg_ir::codec::write::transitional_from_codec(
                CodecError::UnsupportedContent(reason),
            ),
            WriteError::Fatal(error) => cadmpeg_ir::codec::write::transitional_from_codec(error),
            WriteError::Io(error) => Self::Io(error),
            WriteError::Capacity { field, value } => Self::TargetCapacity {
                field,
                limit: field.limit,
                value,
            },
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

    #[test]
    fn destination_io_error_keeps_its_kind() {
        let error: WriteError = std::io::Error::from(std::io::ErrorKind::BrokenPipe).into();
        let result: cadmpeg_ir::codec::write::EncodeError = error.into();
        assert!(
            matches!(result, cadmpeg_ir::codec::write::EncodeError::Io(error)
            if error.kind() == std::io::ErrorKind::BrokenPipe)
        );
    }

    #[test]
    fn codec_io_error_keeps_its_kind() {
        let source =
            cadmpeg_core::CodecError::Io(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
        let result: cadmpeg_ir::codec::write::EncodeError = WriteError::from(source).into();
        assert!(
            matches!(result, cadmpeg_ir::codec::write::EncodeError::Io(error)
            if error.kind() == std::io::ErrorKind::BrokenPipe)
        );
    }
}
