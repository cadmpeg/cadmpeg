// SPDX-License-Identifier: Apache-2.0
//! Rhino dialect identity: which registry row a `.3dm` archive is, and how it
//! was admitted.
//!
//! The `*LossCode` template: the enum is internal, [`DialectId::pinned`]
//! strings are the boundary, [`RhinoDialect::classify`] is the one construction
//! path, and the vocabulary is closed. Every variant here has a row in
//! `docs/dialects.toml`; `tests::every_pinned_id_has_a_registry_row_and_every_row_has_a_variant`
//! fails on drift in either direction.
//!
//! # One discriminant, read before the parse strategy is chosen
//!
//! The archive-version word occupies bytes 24..32 of the 32-byte start section
//! and is the sole read discriminant of this format
//! ([`ArchiveVersion::classify`]). It is exact-equality: word 51 is not
//! "archive 50 with extras", it is [`RhinoDialect::UnknownArchive`]. Ten words
//! carry their own row and every other positive word lands on the mandatory
//! totality row (design §3.3, B4).
//!
//! The ~235 per-record version words inside an admitted archive never split a
//! dialect (B2). They are losses inside a dialect, and the openNURBS
//! writer-version stamp census that charges
//! [`crate::loss::RhinoLossCode::SourceDialectUnverified`] is exactly that: a
//! per-record substitution inside an archive whose own version this codec
//! reads with the grammar declared for it. Archive-level admission is
//! orthogonal to that census and must never be derived from it.
//!
//! # Admission is a function of the row alone
//!
//! Rhino never reads a document with a grammar declared for a different row.
//! Either the codec implements the row's grammar, or it declines the row and
//! reports the header. So [`Admission::AdmittedUnverified`] is unreachable
//! here, and [`RhinoDialect::refuses_decode`] is the single predicate: it
//! decides the [`Admission`] this module reports *and* the refusal
//! `crate::container::decode` returns, so the two can never disagree.

use crate::chunks::ArchiveVersion;
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
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
/// ([`crate::loss::RhinoLossCode::SourceDialectUnverified`]), never at the
/// archive level.
const DECLARED_OPENNURBS_WRITER_VERSION: &str = "opennurbs_writer_version";

/// One row of `docs/dialects.toml` under the `rhino` namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RhinoDialect {
    /// Archive word 1: the flat legacy record grammar.
    Archive1,
    Archive2,
    Archive3,
    Archive4,
    /// Archive word 5: four-byte chunk values and a grammar this codec does not
    /// read. Distinct from word 50.
    Archive5,
    Archive50,
    Archive60,
    Archive70,
    Archive80,
    Archive90,
    /// The mandatory totality row: any positive archive word that is not one of
    /// the ten literals above.
    UnknownArchive,
}

impl RhinoDialect {
    /// Every dialect this codec can name.
    ///
    /// The registry cross-check is its only consumer, and that is the point:
    /// the list exists so a variant added without a registry row, or a row
    /// added without a variant, fails a test.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 11] = [
        Self::Archive1,
        Self::Archive2,
        Self::Archive3,
        Self::Archive4,
        Self::Archive5,
        Self::Archive50,
        Self::Archive60,
        Self::Archive70,
        Self::Archive80,
        Self::Archive90,
        Self::UnknownArchive,
    ];

    /// The pinned registry id. The only string boundary this enum has.
    pub(crate) const fn id(self) -> DialectId {
        DialectId::pinned(match self {
            Self::Archive1 => "rhino:archive-1",
            Self::Archive2 => "rhino:archive-2",
            Self::Archive3 => "rhino:archive-3",
            Self::Archive4 => "rhino:archive-4",
            Self::Archive5 => "rhino:archive-5",
            Self::Archive50 => "rhino:archive-50",
            Self::Archive60 => "rhino:archive-60",
            Self::Archive70 => "rhino:archive-70",
            Self::Archive80 => "rhino:archive-80",
            Self::Archive90 => "rhino:archive-90",
            Self::UnknownArchive => "rhino:unknown-archive",
        })
    }

    /// The row whose `archive_version` discriminant the header word satisfies.
    ///
    /// Total by construction: [`ArchiveVersion`] already partitions the word
    /// space, and `Other` is the residue that no literal row claims.
    pub(crate) const fn from_archive(archive: ArchiveVersion) -> Self {
        match archive {
            ArchiveVersion::V1 => Self::Archive1,
            ArchiveVersion::V2 => Self::Archive2,
            ArchiveVersion::V3 => Self::Archive3,
            ArchiveVersion::V4 => Self::Archive4,
            ArchiveVersion::LegacyV5 => Self::Archive5,
            ArchiveVersion::V5 => Self::Archive50,
            ArchiveVersion::V6 => Self::Archive60,
            ArchiveVersion::V7 => Self::Archive70,
            ArchiveVersion::V8 => Self::Archive80,
            ArchiveVersion::V9 => Self::Archive90,
            ArchiveVersion::Other(_) => Self::UnknownArchive,
        }
    }

    /// Whether this codec declines to decode the row at all.
    ///
    /// The single predicate. `crate::container::decode` refuses exactly the
    /// rows for which this is true, and [`Self::admission`] reports
    /// [`Admission::Refused`] for exactly those rows, so the refusal and the
    /// reported admission cannot drift apart.
    ///
    /// Archive word 5 has no grammar in this codec, and the totality row names
    /// words no grammar was written for. Every other row has one: word 1 the
    /// flat legacy records, words 2 through 90 the chunked scan.
    pub(crate) const fn refuses_decode(self) -> bool {
        matches!(self, Self::Archive5 | Self::UnknownArchive)
    }

    /// How a run admitted a document on this row.
    ///
    /// Derived from [`Self::refuses_decode`] and from nothing else.
    /// [`Admission::AdmittedUnverified`] is unreachable: this codec never
    /// substitutes one row's grammar for another's, so no document is ever read
    /// with a strategy its own row does not declare.
    const fn admission(self) -> Admission {
        if self.refuses_decode() {
            Admission::Refused
        } else {
            Admission::Admitted
        }
    }

    /// Classifies one document. The single construction path for a
    /// [`DialectMatch`] in this codec, so a classification and the report can
    /// never disagree.
    ///
    /// `writer_version` is the openNURBS stamp where the run read the
    /// properties table, and `None` where it did not.
    pub(crate) fn classify(archive: ArchiveVersion, writer_version: Option<i64>) -> DialectMatch {
        let dialect = Self::from_archive(archive);
        let mut declared = BTreeMap::new();
        declared.insert(DECLARED_ARCHIVE_VERSION.into(), archive.value().to_string());
        if let Some(stamp) = writer_version {
            declared.insert(DECLARED_OPENNURBS_WRITER_VERSION.into(), stamp.to_string());
        }
        DialectMatch {
            format: FORMAT.into(),
            dialect: Some(dialect.id()),
            declared,
            admission: dialect.admission(),
        }
    }
}

/// Whether the archive word names a row this codec declines to decode.
///
/// The call site in `crate::container::decode`; it reads the same predicate the
/// reported [`Admission`] reads.
pub(crate) const fn refuses_decode(archive: ArchiveVersion) -> bool {
    RhinoDialect::from_archive(archive).refuses_decode()
}

#[cfg(test)]
mod tests;
