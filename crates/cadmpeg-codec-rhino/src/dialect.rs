// SPDX-License-Identifier: Apache-2.0
//! Rhino dialect identity: which registry row a `.3dm` archive is, and how it
//! was admitted.
//!
//! The `*LossCode` template: the enum is internal, [`DialectId::pinned`]
//! strings are the boundary, [`ArchiveVersion::classify`] is the one construction
//! path, and the vocabulary is closed. Every variant here has a row in
//! `docs/dialects.toml`; `tests::every_pinned_id_has_a_registry_row_and_every_row_has_a_variant`
//! fails on drift in either direction.
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
//! [`admission_loss`] charges
//! [`crate::loss::RhinoLossCode::SourceDialectUnverified`] for it.
//!
//! Archive word 5 uses the same typecode, four-byte chunk-value, and CRC
//! framing as words 2 through 4. It therefore selects the same chunked scan.

use crate::chunks::ArchiveVersion;
use crate::RhinoArchiveVersion;
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::TargetDescriptor;
use cadmpeg_ir::report::LossNote;
use std::collections::BTreeMap;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "rhino";

/// The synthesis catalog: the archive versions this writer can produce for any
/// input, one row per [`RhinoArchiveVersion`] variant.
///
/// The chunked band this codec *reads* is wider than the band it writes:
/// archives 1, 2, 3, 4, 5 and 90 decode but have no writer, and archive 5 and
/// the totality row decode as admitted-unverified. None of them is a target,
/// and — unlike IGES — there is no preservation path that could write them
/// anyway (see [`crate::OFF_CATALOG_SOURCE_REASON`]).
///
/// The alias of each row is its bare archive word and its bare Rhino major, so
/// `--to 60` and `--to rhino:archive-60` name the same row.
pub(crate) const TARGETS: &[TargetDescriptor] = &[
    TargetDescriptor {
        id: RhinoArchiveVersion::V5.pinned(),
        label: "Rhino 5 archive (50)",
        aliases: &["5", "50"],
        default: false,
    },
    TargetDescriptor {
        id: RhinoArchiveVersion::V6.pinned(),
        label: "Rhino 6 archive (60)",
        aliases: &["6", "60"],
        default: false,
    },
    TargetDescriptor {
        id: RhinoArchiveVersion::V7.pinned(),
        label: "Rhino 7 archive (70)",
        aliases: &["7", "70"],
        default: false,
    },
    TargetDescriptor {
        id: RhinoArchiveVersion::V8.pinned(),
        label: "Rhino 8 archive (80)",
        aliases: &["8", "80"],
        default: true,
    },
];

impl RhinoArchiveVersion {
    pub(crate) const fn pinned(self) -> &'static str {
        ArchiveVersion::from_write_version(self).pinned()
    }
}

/// The archive version represented by a canonical catalog entry.
pub(crate) fn target_version(target: &TargetDescriptor) -> Result<RhinoArchiveVersion, CodecError> {
    [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ]
    .into_iter()
    .find(|version| version.target() == target.id)
    .ok_or_else(|| {
        CodecError::Malformed("Rhino target catalog does not map to an archive version".into())
    })
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

impl ArchiveVersion {
    const fn from_write_version(version: RhinoArchiveVersion) -> Self {
        match version {
            RhinoArchiveVersion::V5 => Self::V5,
            RhinoArchiveVersion::V6 => Self::V6,
            RhinoArchiveVersion::V7 => Self::V7,
            RhinoArchiveVersion::V8 => Self::V8,
        }
    }

    /// Every dialect this codec can name.
    ///
    /// The registry cross-check is its only consumer, and that is the point:
    /// the list exists so a variant added without a registry row, or a row
    /// added without a variant, fails a test.
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

    /// How a run admitted a document on this row.
    ///
    /// The one predicate behind both the report's [`Admission`] and
    /// [`admission_loss`]. A declared row that this codec reads carries a
    /// verified identity; the totality row carries no declared identity at all,
    /// so it names the row whose strategy was substituted for it.
    fn admission(self) -> Admission {
        if matches!(self, Self::LegacyV5) {
            Admission::AdmittedUnverified {
                nearest: Self::V5.id(),
            }
        } else if matches!(self, Self::Other(_)) {
            Admission::AdmittedUnverified {
                nearest: if self.uses_eight_byte_values() {
                    Self::V9.id()
                } else {
                    Self::V4.id()
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
    pub(crate) fn classify(self, writer_version: Option<i64>) -> DialectMatch {
        let mut declared = BTreeMap::new();
        declared.insert(DECLARED_ARCHIVE_VERSION.into(), self.value().to_string());
        if let Some(stamp) = writer_version {
            declared.insert(DECLARED_OPENNURBS_WRITER_VERSION.into(), stamp.to_string());
        }
        DialectMatch::layer(FORMAT, self.id(), declared, self.admission())
    }

    /// Whether the archive word selects the chunked grammar.
    pub(crate) const fn is_chunked(self) -> bool {
        !matches!(self, Self::V1)
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

#[cfg(test)]
mod tests;
