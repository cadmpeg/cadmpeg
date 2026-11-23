// SPDX-License-Identifier: Apache-2.0
//! Contain errors at a bounded Design record.

use crate::loss::F3dLossCode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

/// Whether `error` ends the session rather than one record: an I/O error or a
/// resource refusal.
pub(crate) fn is_fatal(error: &CodecError) -> bool {
    matches!(error, CodecError::Io(_) | CodecError::ResourceLimit(_))
}

/// Contains an error at one record. An I/O error or a resource refusal is
/// returned to the caller and ends the session. Every other error, including
/// a refusal scoped to a Part, becomes one loss on `losses`, and the record
/// stays in the source archive; the caller then decodes the next record.
pub(crate) fn recover<T>(
    result: Result<T, CodecError>,
    losses: &mut Vec<LossNote>,
    code: F3dLossCode,
    identity: impl std::fmt::Display,
) -> Result<Option<T>, CodecError> {
    match result {
        Ok(record) => Ok(Some(record)),
        Err(error) if is_fatal(&error) => Err(error),
        Err(error) => {
            losses.push(code.note(format!(
                "{identity} remains in the source archive because its decoding failed: {error}"
            )));
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::recover;
    use crate::loss::F3dLossCode;
    use cadmpeg_core::decode::{LimitScope, ResourceDimension, ResourceFailure, ResourceLimit};
    use cadmpeg_core::CodecError;

    #[test]
    fn record_recovery_preserves_resource_refusals() {
        let limit = ResourceLimit {
            scope: LimitScope::for_dimension(ResourceDimension::RecursionDepth),
            dimension: ResourceDimension::RecursionDepth,
            reason: ResourceFailure::BudgetExceeded,
            limit: 1,
            used: 1,
            additional: 1,
            operation: "synthetic record recursion",
        };
        let mut losses = Vec::new();
        let result = recover::<()>(
            Err(CodecError::ResourceLimit(limit)),
            &mut losses,
            F3dLossCode::SketchPointUndecoded,
            "Sketch point record 42",
        );
        assert!(matches!(result, Err(CodecError::ResourceLimit(_))));
        assert!(losses.is_empty());
    }

    #[test]
    fn record_recovery_keeps_io_errors_fatal() {
        let mut losses = Vec::new();
        let result = recover::<()>(
            Err(CodecError::Io(std::io::Error::other("read failed"))),
            &mut losses,
            F3dLossCode::SketchPointUndecoded,
            "Sketch point record 42",
        );
        assert!(matches!(result, Err(CodecError::Io(_))));
        assert!(losses.is_empty());
    }
}
