// SPDX-License-Identifier: Apache-2.0
//! Private encode-path error for the SLDPRT writer (#231, #237).
//!
//! A patch that cannot express an edit declines to synthesis with
//! `Decline`. A private typed enum carries that decision and typed target
//! capacities through planning (#239).

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
            CodecError::Io(error) => Self::Io(error),
            // Writer-internal content the target cannot express declines to
            // synthesis (#239 §5.0; declined content).
            CodecError::InvalidInput(reason) => Self::Decline(reason),
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

/// The encode failure a write error is; a content failure stays a write error.
impl TryFrom<WriteError> for cadmpeg_ir::codec::write::EncodeError {
    type Error = WriteError;

    fn try_from(error: WriteError) -> Result<Self, WriteError> {
        match error {
            WriteError::Io(error) | WriteError::Fatal(CodecError::Io(error)) => Ok(Self::Io(error)),
            WriteError::Capacity { field, value } => Ok(Self::TargetCapacity {
                field,
                limit: field.limit,
                value,
            }),
            WriteError::Fatal(CodecError::ResourceLimit(limit)) => Ok(Self::ResourceLimit(limit)),
            WriteError::Fatal(CodecError::UnsupportedTarget(refusal)) => {
                Ok(Self::UnsupportedTarget(refusal))
            }
            error => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WriteError;

    #[test]
    fn decline_carries_reason() {
        let error = WriteError::Decline("no layout".into());
        assert_eq!(error.reason(), "no layout");
    }

    #[test]
    fn other_errors_stay_fatal_but_name_their_reason() {
        let error = WriteError::from(cadmpeg_core::CodecError::Malformed("bad".into()));
        assert!(matches!(error, WriteError::Fatal(_)));
        assert!(error.reason().contains("bad"));
    }
}
