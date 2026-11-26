// SPDX-License-Identifier: Apache-2.0
//! Contain errors at a bounded Design record.

use crate::loss::F3dLossCode;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::GivenUpRecord;
use cadmpeg_ir::{SourceProvenance, StreamName};

/// Whether `error` ends the session rather than one record: an I/O error or a
/// resource refusal.
pub(crate) fn is_fatal(error: &CodecError) -> bool {
    matches!(error, CodecError::Io(_) | CodecError::ResourceLimit(_))
}

/// A Design record that a give-up names: the record with record index
/// `entity` in the archive stream `stream`, and its byte span in that stream
/// where the caller holds it. Its id is `f3d:<stream>:design-entity#<entity>`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DesignRecord<'a> {
    stream: &'a str,
    entity: u64,
    offset: Option<usize>,
    length: Option<usize>,
}

impl<'a> DesignRecord<'a> {
    /// The record with record index `entity` in `stream`.
    pub(crate) fn entity(stream: &'a str, entity: impl Into<u64>) -> Self {
        Self {
            stream,
            entity: entity.into(),
            offset: None,
            length: None,
        }
    }

    /// The record starts at byte `offset` of its stream.
    pub(crate) const fn at(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    /// The record spans bytes `start..end` of its stream.
    pub(crate) const fn spanning(mut self, start: usize, end: usize) -> Self {
        self.offset = Some(start);
        self.length = end.checked_sub(start);
        self
    }

    /// The record's id token.
    pub(crate) fn id(&self) -> String {
        crate::ids::native_scoped_id(self.stream, "design-entity", self.entity)
    }

    fn provenance(&self) -> Option<SourceProvenance> {
        let stream = StreamName::try_from(self.stream.to_owned()).ok()?;
        Some(SourceProvenance::in_stream(
            "f3d",
            stream,
            u64_from_index(self.offset?),
        ))
    }

    /// The losses of the record given up whole for the codec loss `why`:
    /// `why` at the record's position, then the `record.unreadable` that
    /// names the record by its id.
    pub(crate) fn given_up(&self, why: LossNote) -> [LossNote; 2] {
        let why = match self.provenance() {
            Some(provenance) => why.with_provenance(provenance),
            None => why,
        };
        let mut unreadable = GivenUpRecord::caused_by(self.id(), &why);
        if let Some(length) = self.length {
            unreadable = unreadable.spanning(u64_from_index(length));
        }
        let unreadable = unreadable.loss();
        [why, unreadable]
    }
}

/// Contains an error at one record the caller then gives up. An I/O error or
/// a resource refusal is returned to the caller and ends the session. Every
/// other error becomes `code`'s loss, whose message describes the record as
/// `what`, and one `record.unreadable` naming `record` by its id, and the
/// record stays in the source archive; the caller then decodes the next
/// record.
pub(crate) fn recover<T>(
    result: Result<T, CodecError>,
    losses: &mut Vec<LossNote>,
    code: F3dLossCode,
    record: DesignRecord<'_>,
    what: impl std::fmt::Display,
) -> Result<Option<T>, CodecError> {
    match result {
        Ok(record) => Ok(Some(record)),
        Err(error) if is_fatal(&error) => Err(error),
        Err(error) => {
            losses.extend(record.given_up(failure_note(code, what, &error)));
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
    what: impl std::fmt::Display,
) -> Result<Option<T>, CodecError> {
    match result {
        Ok(record) => Ok(Some(record)),
        Err(error) if is_fatal(&error) => Err(error),
        Err(error) => {
            losses.push(failure_note(code, what, &error));
            Ok(None)
        }
    }
}

fn failure_note(code: F3dLossCode, what: impl std::fmt::Display, error: &CodecError) -> LossNote {
    code.note(format!(
        "{what} remains in the source archive because its decoding failed: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::{recover, recover_partial, DesignRecord};
    use crate::loss::F3dLossCode;
    use cadmpeg_core::decode::{LimitScope, ResourceDimension, ResourceFailure, ResourceLimit};
    use cadmpeg_core::CodecError;

    const STREAM: &str = "Design/BulkStream.dat";

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
            DesignRecord::entity(STREAM, 42_u64),
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
            DesignRecord::entity(STREAM, 42_u64).spanning(100, 160),
            "Sketch point record 42 version 11 in Design/BulkStream.dat",
        );
        assert!(matches!(result, Ok(None)));
        assert_eq!(
            crate::test_support::loss_codes(&losses),
            crate::test_support::given_up_codes(F3dLossCode::SketchPointUndecoded)
        );
        assert!(losses[0].message.contains("synthetic point failure"));
        assert_eq!(losses[1].severity, cadmpeg_ir::report::Severity::Error);
        assert!(losses[1].message.starts_with(&format!(
            "record f3d:{STREAM}:design-entity#42 stream=\"{STREAM}\" offset=100 length=60 given up: {}: ",
            F3dLossCode::SketchPointUndecoded.kind()
        )));
        assert!(losses[1].message.ends_with("; dropped=[]"));
        assert_eq!(losses[0].provenance, losses[1].provenance);
        assert_eq!(losses[1].provenance.as_ref().map(|at| at.offset), Some(100));
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
            DesignRecord::entity(STREAM, 42_u64),
            "Sketch point record 42",
        );
        assert!(matches!(result, Err(CodecError::Io(_))));
        assert!(losses.is_empty());
    }
}
