// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for IGES Fixed ASCII decoding.
//!
//! Every fallback, drop, and untyped-record report the decoder emits binds a
//! fixed subsystem category and severity to a code drawn from [`IgesLossCode`].
//! Fixing the category and severity on the code keeps the many sites that
//! report the same concept from labelling it inconsistently, and stops a new
//! report path from choosing a category and severity that disagree.
//!
//! [`IgesLossCode::note`] builds a message-only note. [`IgesLossCode::note_for`]
//! builds the entity-projection note, prefixing the message with the reporting
//! record's entity type and form. Both leave provenance absent: the decoder
//! attributes a loss through its message and the record identity it names, not
//! a source span. The enum is crate-private vocabulary; it carries no stable
//! string identifier because the emitted [`LossNote`] already carries the
//! shared [`LossCode`].

use crate::directory::DirectoryEntry;
use cadmpeg_ir::report::{LossCategory, LossCode, LossNote, Severity};

/// A crate-private identifier for one IGES transfer loss.
///
/// Each variant fixes the shared [`LossCode`], the subsystem [`LossCategory`],
/// and the [`Severity`] of the loss it names, so a site cannot mislabel a loss
/// it reports. Variants that share a [`LossCode`] but disagree on category are
/// kept distinct on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IgesLossCode {
    /// Product-occurrence expansion stopped at its configured output limit.
    ProductOccurrenceTruncated,
    /// A directory record was admitted or handled without a neutral projection.
    RecordRetainedUntyped,
    /// A geometry entity record decoded but yielded no neutral geometry.
    GeometryEntityNotProjected,
    /// A presentation entity's display data was not projected.
    PresentationEntityNotProjected,
}

impl IgesLossCode {
    /// The subsystem category this loss belongs to.
    pub(crate) const fn category(self) -> LossCategory {
        match self {
            Self::GeometryEntityNotProjected => LossCategory::Geometry,
            Self::PresentationEntityNotProjected => LossCategory::Material,
            Self::ProductOccurrenceTruncated | Self::RecordRetainedUntyped => LossCategory::Other,
        }
    }

    /// The severity of this loss. Every IGES loss is currently a warning: the
    /// affected record is retained or reported, never a hard decode failure.
    pub(crate) const fn severity(self) -> Severity {
        match self {
            Self::ProductOccurrenceTruncated
            | Self::RecordRetainedUntyped
            | Self::GeometryEntityNotProjected
            | Self::PresentationEntityNotProjected => Severity::Warning,
        }
    }

    /// The shared IR loss kind stored on the emitted [`LossNote`].
    const fn shared_code(self) -> LossCode {
        match self {
            Self::ProductOccurrenceTruncated => LossCode::DecodeDiagnostic,
            Self::RecordRetainedUntyped | Self::GeometryEntityNotProjected => {
                LossCode::RecordNotTyped
            }
            Self::PresentationEntityNotProjected => LossCode::MaterialNotTransferred,
        }
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// Category and severity come from the code, so a site cannot mislabel a
    /// loss it names. Provenance is left absent.
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote {
            code: self.shared_code(),
            category: self.category(),
            severity: self.severity(),
            message: message.into(),
            provenance: None,
        }
    }

    /// Build a [`LossNote`] for an entity-projection loss, prefixing `message`
    /// with the reporting record's entity type and form.
    ///
    /// Used by the geometry and presentation projections, which report against
    /// a [`DirectoryEntry`]. The entry supplies only the message prefix;
    /// provenance is left absent, matching [`note`](Self::note).
    pub(crate) fn note_for(self, entry: &DirectoryEntry, message: impl Into<String>) -> LossNote {
        let detail = match self {
            Self::PresentationEntityNotProjected => "display data was not projected",
            _ => "was not projected",
        };
        self.note(format!(
            "IGES entity type {} form {} {}: {}",
            entry.entity_type,
            entry.form,
            detail,
            message.into()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::IgesLossCode;

    const ALL: &[IgesLossCode] = &[
        IgesLossCode::ProductOccurrenceTruncated,
        IgesLossCode::RecordRetainedUntyped,
        IgesLossCode::GeometryEntityNotProjected,
        IgesLossCode::PresentationEntityNotProjected,
    ];

    /// The note builder fixes category and severity from the code so a call
    /// site cannot mislabel a loss it names, and leaves provenance absent.
    #[test]
    fn note_takes_category_and_severity_from_the_code() {
        for &code in ALL {
            let note = code.note("x");
            assert_eq!(note.category, code.category());
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
