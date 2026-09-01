// SPDX-License-Identifier: Apache-2.0
//! Rhino dialect identity: which registry row a `.3dm` archive is, and how it
//! was admitted.
//!
//! The `*LossCode` template: the enum is internal, [`DialectId::pinned`]
//! strings are the boundary, [`ArchiveVersion::classify`] is the one construction
//! path, and the vocabulary is closed. Tests close it directly against
//! `docs/dialects.toml`.
//!
//! # One discriminant, read before the parse strategy is chosen
//!
//! The archive-version word occupies bytes 24..32 of the 32-byte start section
//! and is the sole read discriminant of this format
//! ([`ArchiveVersion::from_word`]). It is exact-equality: word 51 is not
//! "archive 50 with extras", it is [`ArchiveVersion::Other`]. Ten words
//! carry their own row and every other positive word lands on the mandatory
//! totality row.
//!
//! The ~235 per-record version words inside an admitted archive never split a
//! dialect (B2). The openNURBS writer-version stamp is shared evidence, not one
//! shared policy. Each record reader owns the counterfactual for the field or
//! topology choice the stamp would verify. Missing-stamp field and layout
//! choices charge
//! [`crate::loss::RhinoLossCode::SourceWriterStampUnverified`]; a body-kind
//! choice charges
//! [`crate::loss::RhinoLossCode::TopologyBodyKindGaugeSubstituted`]. Both are
//! losses inside an admitted archive dialect. Archive-level admission is
//! orthogonal to those record decisions and must never be derived from them.
//!
//! # Residual admission follows the observed archive word
//!
//! Every positive archive word other than 1 selects the chunked route. The
//! value selects the chunk width and version-conditioned record branches, so a
//! word no row claims still selects a scan. The totality row is therefore read,
//! not refused. It is read directly as [`ArchiveVersion::Other`], and record
//! branches continue to read the observed word. No declared archive row is
//! substituted, so admission is [`Admission::AdmittedUnverified`] with no
//! `using` value, and
//! [`admission_loss`] charges
//! [`crate::loss::RhinoLossCode::SourceDialectUnverified`] for it.
//!
//! Archive word 5 uses the same typecode, four-byte chunk-value, and CRC
//! framing as words 2 through 4. It therefore selects the same chunked scan.

use crate::chunks::ArchiveVersion;
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use cadmpeg_ir::report::LossNote;
use std::collections::BTreeMap;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "rhino";

/// Key of the archive-version word in [`DialectMatch::declared`].
///
/// The decimal word from bytes 24..32 of the start section, as
/// [`ArchiveVersion::value`] read it. Leading spaces and leading zeros are not
/// part of the value; a word of zero is not a header at all and never reaches
/// classification.
const DECLARED_ARCHIVE_VERSION: &str = "archive_version";

/// Key of the openNURBS writer-version stamp in [`DialectMatch::declared`].
///
/// The short `TCODE_OPENNURBS_VERSION` value in the properties table, verbatim.
/// Absent when the archive carries no stamp, and absent on every path that does
/// not read the properties table: header-only inspection and the archive-1 flat
/// legacy grammar.
///
/// This is a declaration the decoder branches on — it decides strict-boolean
/// reading and the legacy B-rep field layouts — but it is not an admission
/// discriminant. A stamp-less archive is still read with the grammar its
/// archive word declares; what it loses is charged per record
/// ([`crate::loss::RhinoLossCode::SourceWriterStampUnverified`]), never at the
/// archive level.
const DECLARED_OPENNURBS_WRITER_VERSION: &str = "opennurbs_writer_version";

impl ArchiveVersion {
    /// Every dialect identity this enum can name.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 11] = [
        Self::V1,
        Self::V2,
        Self::V3,
        Self::V4,
        Self::LegacyV5,
        Self::V5,
        Self::V6,
        Self::V7,
        Self::V8,
        Self::V9,
        Self::Other(0),
    ];

    /// The pinned registry id. The only string boundary this enum has.
    pub(crate) const fn id(self) -> DialectId {
        DialectId::pinned(self.pinned())
    }

    const fn pinned(self) -> &'static str {
        match self {
            Self::V1 => "rhino:archive-1",
            Self::V2 => "rhino:archive-2",
            Self::V3 => "rhino:archive-3",
            Self::V4 => "rhino:archive-4",
            Self::LegacyV5 => "rhino:archive-5",
            Self::V5 => "rhino:archive-50",
            Self::V6 => "rhino:archive-60",
            Self::V7 => "rhino:archive-70",
            Self::V8 => "rhino:archive-80",
            Self::V9 => "rhino:archive-90",
            Self::Other(_) => "rhino:unknown",
        }
    }

    /// Classifies one document. The single construction path for a
    /// [`DialectMatch`] in this codec, so a classification and the report can
    /// never disagree.
    ///
    /// `writer_version` is the openNURBS stamp where the run read the
    /// properties table, and `None` where it did not.
    pub(crate) fn classify(self, writer_version: Option<i64>) -> DialectMatch {
        let mut declared = BTreeMap::new();
        declared.insert(DECLARED_ARCHIVE_VERSION.into(), self.value().to_string());
        if let Some(stamp) = writer_version {
            declared.insert(DECLARED_OPENNURBS_WRITER_VERSION.into(), stamp.to_string());
        }
        if matches!(self, Self::Other(_)) {
            DialectMatch::residual(self.id())
        } else {
            DialectMatch::admitted(self.id())
        }
        .with_declared(declared)
    }
}

/// The dialect-unverified loss for a classified layer.
///
/// `None` exactly where `matched.admission` is not
/// [`Admission::AdmittedUnverified`], because this reads that field rather than
/// reclassifying. The biconditional the decode policy requires is therefore
/// structural: the note charged and the admission reported come from one value,
/// not from two authors agreeing.
pub(crate) fn admission_loss(matched: &DialectMatch) -> Option<LossNote> {
    match matched.admission() {
        Admission::AdmittedUnverified { .. } => {}
        Admission::Admitted | Admission::Refused => return None,
    }
    let word = matched
        .declared()
        .get(DECLARED_ARCHIVE_VERSION)
        .map_or("absent", String::as_str);
    let message = format!(
        "archive version word {word} has no declared row, so no declared identity or \
         substituted declared grammar was verified. The document is read by the residual \
         chunked route, with chunk width and version-conditioned record branches selected \
         directly from the observed word."
    );
    Some(crate::loss::RhinoLossCode::SourceDialectUnverified.note(message))
}

#[cfg(test)]
mod tests;
