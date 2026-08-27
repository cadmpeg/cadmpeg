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
//! "archive 50 with extras", it is [`RhinoDialect::Unknown`]. Ten words
//! carry their own row and every other positive word lands on the mandatory
//! totality row (design §3.3, B4).
//!
//! The ~235 per-record version words inside an admitted archive never split a
//! dialect (B2). They are losses inside a dialect, and the openNURBS
//! writer-version stamp census that charges
//! [`crate::loss::RhinoLossCode::SourceWriterStampUnverified`] is exactly that: a
//! per-record substitution inside an archive whose own version this codec
//! reads with the grammar declared for it. Archive-level admission is
//! orthogonal to that census and must never be derived from it.
//!
//! # Admission follows the selected chunk width
//!
//! Archive words 2 through 90 are one chunked grammar: the value alone selects
//! the chunk value width and the begin-chunk form, so a word no row claims
//! still selects a scan. The totality row is therefore read, not refused. It
//! is read with the strategy selected by its chunk width: `rhino:archive-4`
//! below word 50 and `rhino:archive-90` at or above word 50. Admission is
//! [`Admission::AdmittedUnverified`] with that row as `nearest`, and
//! [`dialect_loss`] charges
//! [`crate::loss::RhinoLossCode::SourceDialectUnverified`] for it.
//!
//! Word 5 is the one structural refusal: the pre-chunk grammar it names has no
//! reader here, so no scan applies to it at all.
//! [`RhinoDialect::refuses_decode`] is the single predicate: it decides the
//! [`Admission`] this module reports *and* the refusal
//! `crate::container::decode` returns, so the two can never disagree.

use crate::chunks::ArchiveVersion;
use crate::RhinoArchiveVersion;
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use cadmpeg_ir::codec::{find_target, TargetDescriptor};
use cadmpeg_ir::report::LossNote;
use std::collections::BTreeMap;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "rhino";

/// The synthesis catalog: the archive versions this writer can produce for any
/// input, one row per [`RhinoArchiveVersion`] variant.
///
/// The chunked band this codec *reads* is wider than the band it writes:
/// archives 2, 3, 4 and 90 decode but have no writer, and archives 1, 5 and the
/// totality row decode not at all. None of them is a target, and — unlike IGES
/// — there is no preservation path that could write them anyway (see
/// [`crate::OFF_CATALOG_SOURCE_REASON`]).
///
/// The alias of each row is its bare archive word and its bare Rhino major, so
/// `--to 60` and `--to rhino:archive-60` name the same row.
pub(crate) const TARGETS: &[TargetDescriptor] = &[
    TargetDescriptor {
        id: "rhino:archive-50",
        label: "Rhino 5 archive (50)",
        aliases: &["5", "50"],
        default: false,
    },
    TargetDescriptor {
        id: "rhino:archive-60",
        label: "Rhino 6 archive (60)",
        aliases: &["6", "60"],
        default: false,
    },
    TargetDescriptor {
        id: "rhino:archive-70",
        label: "Rhino 7 archive (70)",
        aliases: &["7", "70"],
        default: false,
    },
    TargetDescriptor {
        id: "rhino:archive-80",
        label: "Rhino 8 archive (80)",
        aliases: &["8", "80"],
        default: true,
    },
];

/// The archive version a target id names, by id or by alias, or `None` when the
/// id is outside [`TARGETS`].
///
/// This is also the inherit-path predicate: a source dialect resolves to the
/// version that reproduces it exactly when it is a catalog row.
pub(crate) fn target_version(id: &str) -> Option<RhinoArchiveVersion> {
    let target = find_target(TARGETS, id)?;
    [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ]
    .into_iter()
    .find(|version| version.target() == target.id)
}

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
    Unknown,
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
        Self::Unknown,
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
            Self::Unknown => "rhino:unknown",
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
            ArchiveVersion::Other(_) => Self::Unknown,
        }
    }

    /// Whether this codec declines to decode the row at all.
    ///
    /// The single predicate. `crate::container::decode` refuses exactly the
    /// rows for which this is true, and [`Self::admission`] reports
    /// [`Admission::Refused`] for exactly those rows, so the refusal and the
    /// reported admission cannot drift apart.
    ///
    /// Archive word 5 alone has no grammar in this codec: it names the
    /// pre-chunk archive form, which no reader here implements. Every other row
    /// has one: word 1 the flat legacy records, words 2 through 90 the chunked
    /// scan, and the totality row that same chunked scan under
    /// [`Admission::AdmittedUnverified`].
    pub(crate) const fn refuses_decode(self) -> bool {
        matches!(self, Self::Archive5)
    }

    /// How a run admitted a document on this row.
    ///
    /// The one predicate behind both the report's [`Admission`] and
    /// [`dialect_loss`]. A declared row that this codec reads carries a
    /// verified identity; the totality row carries no declared identity at all,
    /// so it names the row whose strategy was substituted for it; and word 5 is
    /// refused, which [`Self::refuses_decode`] decides for the decode branch
    /// too.
    fn admission(self, archive: ArchiveVersion) -> Admission {
        if self.refuses_decode() {
            Admission::Refused
        } else if matches!(self, Self::Unknown) {
            Admission::AdmittedUnverified {
                nearest: if archive.uses_eight_byte_values() {
                    Self::Archive90.id()
                } else {
                    Self::Archive4.id()
                },
            }
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
            admission: dialect.admission(archive),
        }
    }
}

/// The dialect-unverified loss for a classified layer.
///
/// `None` exactly where `matched.admission` is not
/// [`Admission::AdmittedUnverified`], because this reads that field rather than
/// reclassifying. The biconditional the decode policy requires is therefore
/// structural: the note charged and the admission reported come from one value,
/// not from two authors agreeing.
pub(crate) fn dialect_loss(matched: &DialectMatch) -> Option<LossNote> {
    let Admission::AdmittedUnverified { nearest } = &matched.admission else {
        return None;
    };
    let word = matched
        .declared
        .get(DECLARED_ARCHIVE_VERSION)
        .map_or("absent", String::as_str);
    Some(
        crate::loss::RhinoLossCode::SourceDialectUnverified.note(format!(
        "archive version word {word} has no declared row, so no declared identity was verified. \
         The document is read on {nearest}, which uses the chunk value width selected by the \
         archive word."
    )),
    )
}

/// Whether the archive word names a row this codec declines to decode.
///
/// The call site in `crate::container::decode`; it reads the same predicate the
/// reported [`Admission`] reads.
pub(crate) const fn refuses_decode(archive: ArchiveVersion) -> bool {
    RhinoDialect::from_archive(archive).refuses_decode()
}

/// Whether the archive word selects the chunked grammar.
pub(crate) const fn is_chunked(archive: ArchiveVersion) -> bool {
    !refuses_decode(archive) && !matches!(archive, ArchiveVersion::V1)
}

#[cfg(test)]
mod tests;
