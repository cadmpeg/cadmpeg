// SPDX-License-Identifier: Apache-2.0
//! Contain errors at a bounded Design record.

use crate::loss::F3dLossCode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::record_given_up;

/// Whether `error` ends the session rather than one record: an I/O error or a
/// resource refusal.
pub(crate) fn is_fatal(error: &CodecError) -> bool {
    matches!(error, CodecError::Io(_) | CodecError::ResourceLimit(_))
}

/// Contains an error at one record the caller then gives up. An I/O error or
/// a resource refusal is returned to the caller and ends the session. Every
/// other error, including a refusal scoped to a Part, becomes `code`'s loss
/// and one `record.unreadable` naming `identity`, and the record stays in the
/// source archive; the caller then decodes the next record.
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
            losses.extend(record_given_up(
                &identity,
                failure_note(code, &identity, &error),
            ));
            Ok(None)
        }
    }
}

/// Contains an error in a member of a record the caller keeps with less
/// content, as [`recover`] does, charging `code`'s loss alone.
pub(crate) fn recover_partial<T>(
    result: Result<T, CodecError>,
    losses: &mut Vec<LossNote>,
    code: F3dLossCode,
    identity: impl std::fmt::Display,
) -> Result<Option<T>, CodecError> {
    match result {
        Ok(record) => Ok(Some(record)),
        Err(error) if is_fatal(&error) => Err(error),
        Err(error) => {
            losses.push(failure_note(code, identity, &error));
            Ok(None)
        }
    }
}

fn failure_note(
    code: F3dLossCode,
    identity: impl std::fmt::Display,
    error: &CodecError,
) -> LossNote {
    code.note(format!(
        "{identity} remains in the source archive because its decoding failed: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::{recover, recover_partial};
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
    fn a_given_up_record_charges_its_code_and_record_unreadable() {
        let mut losses = Vec::new();
        let result = recover::<()>(
            Err(CodecError::malformed("synthetic point failure")),
            &mut losses,
            F3dLossCode::SketchPointUndecoded,
            "Sketch point record 42 version 11 in Design/BulkStream.dat",
        );
        assert!(matches!(result, Ok(None)));
        assert_eq!(
            crate::test_support::loss_codes(&losses),
            crate::test_support::given_up_codes(F3dLossCode::SketchPointUndecoded)
        );
        assert!(losses[0].message.contains("synthetic point failure"));
        assert_eq!(losses[1].severity, cadmpeg_ir::report::Severity::Error);
        assert!(losses[1]
            .message
            .split_whitespace()
            .any(|token| token == "42"));
        assert!(losses[1]
            .message
            .contains(&F3dLossCode::SketchPointUndecoded.kind().to_string()));
    }

    #[test]
    fn a_partial_record_charges_its_code_alone() {
        let mut losses = Vec::new();
        let result = recover_partial::<()>(
            Err(CodecError::malformed("synthetic geometry failure")),
            &mut losses,
            F3dLossCode::SketchCurveUndecoded,
            "Sketch curve record 41 in Design/BulkStream.dat",
        );
        assert!(matches!(result, Ok(None)));
        assert_eq!(
            crate::test_support::loss_codes(&losses),
            [F3dLossCode::SketchCurveUndecoded.kind()]
        );
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
