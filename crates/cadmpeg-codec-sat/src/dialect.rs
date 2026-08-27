// SPDX-License-Identifier: Apache-2.0
//! SAT dialect identity: which registry row a bare stream is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, [`DialectId::pinned`]
//! strings are the boundary, [`classify`] is the one construction path, and the
//! vocabulary is closed. Every variant here has a row in `docs/dialects.toml`
//! under the `sat` namespace; `tests::every_pinned_id_has_a_registry_row_and_every_row_has_a_variant`
//! fails on drift in either direction.
//!
//! This module owns the **host** layer only. The `acis:` rows — the kernel
//! save-format bands the streams carry — belong to `cadmpeg-asm` and are cited,
//! not declared, here.
//!
//! # Identity is the magic; admission is the save-format band
//!
//! The host rows are discriminated by the stream's leading bytes alone
//! ([`StreamKind`]), which are read exactly: `ASM BinaryFile4`/`8`, `ACIS
//! BinaryFile`, or the two text header lines. Nothing about that reading is
//! approximate, so this codec has no dialect-unverified state and charges no
//! dialect-unverified loss. [`Admission::AdmittedUnverified`] is unreachable
//! here, and a test pins that.
//!
//! What *is* banded is the kernel save format inside the stream. The Spatial
//! ACIS record decoders cover majors 217 and 218; the ASM record decoders
//! compare no save format at all. A stream outside the covered band is
//! structurally identified — it keeps its `sat:` row — and refused semantic
//! decode, which is exactly [`Admission::Refused`]. That refusal is a refusal
//! mark, not an unverified mark: it is charged as
//! [`SatLossCode::ContainerAcisSaveFormatUnsupported`] on an empty-IR result,
//! never as a recovery.
//!
//! [`admits_semantic_decode`] is the single predicate behind both. The decode
//! paths branch on the admission it produced rather than re-testing the band,
//! so the report and the result can never disagree.
//!
//! [`SatLossCode::ContainerAcisSaveFormatUnsupported`]:
//!     crate::loss::SatLossCode::ContainerAcisSaveFormatUnsupported

use crate::detect::StreamKind;
use cadmpeg_asm::kernel_header::KernelHeader;
use cadmpeg_asm::sat;
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use std::collections::BTreeMap;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "sat";

/// Key of the stream encoding in [`DialectMatch::declared`].
const DECLARED_ENCODING: &str = "encoding";
/// Key of the kernel save format's major component in
/// [`DialectMatch::declared`]. Absent when the header carries no save-format
/// word.
const DECLARED_SAVE_FORMAT_MAJOR: &str = "save_format_major";
/// Key of the kernel save format's minor component in
/// [`DialectMatch::declared`]. Absent when the header carries no save-format
/// word.
const DECLARED_SAVE_FORMAT_MINOR: &str = "save_format_minor";
/// Key of the text stream's terminator line in [`DialectMatch::declared`].
/// Absent on the binary branches, which carry no terminator.
const DECLARED_TERMINATOR: &str = "terminator";

/// Save-format majors the Spatial ACIS record decoders cover.
const ADMITTED_ACIS_MAJORS: [u32; 2] = [217, 218];

/// Whether a Spatial ACIS save format is one the record decoders cover.
///
/// A header without a readable save-format word declares no band, so it is not
/// in the covered one.
fn acis_band_covered(save_format_major: Option<u32>) -> bool {
    save_format_major.is_some_and(|major| ADMITTED_ACIS_MAJORS.contains(&major))
}

/// One row of `docs/dialects.toml` under the `sat` namespace.
///
/// One row per [`StreamKind`]: the discriminant is the leading magic, or the
/// two-line header shape for text. [`Self::Unknown`] is the mandatory totality
/// row (design §3.3, B4); `detect::confidence` reports [`Confidence::No`] for
/// it, so it is unreachable through the normal catalog and exists to keep
/// classification total.
///
/// [`Confidence::No`]: cadmpeg_ir::codec::Confidence::No
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SatDialect {
    AsmBinary,
    AcisBinary,
    Text,
    Unknown,
}

impl SatDialect {
    /// Every dialect this codec can name.
    ///
    /// The registry cross-check is its only consumer, and that is the point:
    /// the list exists so a variant added without a registry row, or a row
    /// added without a variant, fails a test.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 4] =
        [Self::AsmBinary, Self::AcisBinary, Self::Text, Self::Unknown];

    /// The pinned registry id. The only string boundary this enum has.
    pub(crate) const fn id(self) -> DialectId {
        DialectId::pinned(match self {
            Self::AsmBinary => "sat:asm-binary",
            Self::AcisBinary => "sat:acis-binary",
            Self::Text => "sat:text",
            Self::Unknown => "sat:unknown",
        })
    }

    /// The row a detected stream kind satisfies.
    ///
    /// Total and injective: identity here is the detection discriminant and
    /// nothing else.
    pub(crate) const fn from_stream_kind(kind: StreamKind) -> Self {
        match kind {
            StreamKind::AsmBinary => Self::AsmBinary,
            StreamKind::AcisBinary => Self::AcisBinary,
            StreamKind::Text => Self::Text,
            StreamKind::Unknown => Self::Unknown,
        }
    }
}

/// The text branch's evidence.
///
/// The terminator line selects the branch, and the header carries the save
/// format the ACIS branch is gated on. The gate keys on the terminator, not on
/// the product family the header names, so an ASM product string under an ACIS
/// terminator is refused outside the band, symmetrically with binary.
pub(crate) struct TextEvidence<'a> {
    /// Branch the terminator line selected.
    pub(crate) branch: sat::Dialect,
    /// Kernel header the three text header lines carried.
    pub(crate) header: &'a KernelHeader,
}

/// What one stream's own bytes said, as the reading path read them.
///
/// One variant per [`StreamKind`]. `None` inside a variant means the kind's
/// discriminant matched but nothing past it parsed. That state is reachable at
/// inspect, which reports what it could read; decode returns a malformed error
/// on the same bytes and builds no match at all.
pub(crate) enum StreamEvidence<'a> {
    /// `ASM BinaryFile4`/`8`. The header is `None` only if the magic matched
    /// and `asm_header::parse` still declined, which its own contract makes
    /// unreachable; the arm keeps this function total.
    AsmBinary(Option<&'a KernelHeader>),
    /// `ACIS BinaryFile`, under the same header-parse note as
    /// [`Self::AsmBinary`].
    AcisBinary(Option<&'a KernelHeader>),
    /// Text header lines; `None` when the stream did not parse past them.
    Text(Option<TextEvidence<'a>>),
    /// No discriminant matched.
    ///
    /// Never constructed in production: `inspect` and `decode` both return a
    /// malformed error on these bytes before any classification, and detection
    /// reports no confidence for them. The variant keeps [`classify`] total
    /// over `StreamKind`, and the tests exercise it.
    #[allow(dead_code)]
    Unknown,
}

impl StreamEvidence<'_> {
    /// The stream kind this evidence came from.
    const fn kind(&self) -> StreamKind {
        match self {
            Self::AsmBinary(_) => StreamKind::AsmBinary,
            Self::AcisBinary(_) => StreamKind::AcisBinary,
            Self::Text(_) => StreamKind::Text,
            Self::Unknown => StreamKind::Unknown,
        }
    }
}

/// Whether the record decoders cover this stream, and so whether semantic
/// decode proceeds.
///
/// The one predicate behind [`Admission::Refused`] and the
/// `container.acis-save-format-unsupported` refusal mark alike. The ASM binary
/// and ASM text branches compare no save format, so they are covered at any
/// band; both ACIS branches take the same 217/218 gate.
pub(crate) fn admits_semantic_decode(evidence: &StreamEvidence<'_>) -> bool {
    match evidence {
        StreamEvidence::AsmBinary(header) => header.is_some(),
        StreamEvidence::AcisBinary(header) => {
            header.is_some_and(|header| acis_band_covered(header.save_format_major()))
        }
        StreamEvidence::Text(Some(text)) => match text.branch {
            sat::Dialect::Asm => true,
            sat::Dialect::Acis => acis_band_covered(text.header.save_format_major()),
        },
        StreamEvidence::Text(None) | StreamEvidence::Unknown => false,
    }
}

/// The declarations the stream made, verbatim, under keys pinned above.
///
/// Evidence, never a control input. A key is absent when the stream carried no
/// such declaration, which is a different statement from a declaration of zero.
fn declared(evidence: &StreamEvidence<'_>) -> BTreeMap<String, String> {
    let mut declared = BTreeMap::new();
    let header = match evidence {
        StreamEvidence::AsmBinary(header) | StreamEvidence::AcisBinary(header) => {
            declared.insert(DECLARED_ENCODING.into(), "binary".into());
            *header
        }
        StreamEvidence::Text(text) => {
            declared.insert(DECLARED_ENCODING.into(), "text".into());
            let Some(text) = text else { return declared };
            declared.insert(
                DECLARED_TERMINATOR.into(),
                terminator_line(text.branch).into(),
            );
            Some(text.header)
        }
        StreamEvidence::Unknown => None,
    };
    if let Some(header) = header {
        if let Some(major) = header.save_format_major() {
            declared.insert(DECLARED_SAVE_FORMAT_MAJOR.into(), major.to_string());
        }
        if let Some(minor) = header.save_format_minor() {
            declared.insert(DECLARED_SAVE_FORMAT_MINOR.into(), minor.to_string());
        }
    }
    declared
}

/// The terminator line a text branch ends with.
pub(crate) const fn terminator_line(branch: sat::Dialect) -> &'static str {
    match branch {
        sat::Dialect::Asm => "End-of-ASM-data",
        sat::Dialect::Acis => "End-of-ACIS-data",
    }
}

/// Classifies one stream. The single construction path for a [`DialectMatch`]
/// in this codec, so a classification bug and the report can never disagree.
///
/// Identity is the row the leading discriminant satisfies; admission is
/// [`admits_semantic_decode`]. The two are computed independently and never
/// from each other: an ACIS stream outside the covered band keeps its own
/// registry row while its semantic decode is refused.
pub(crate) fn classify(evidence: &StreamEvidence<'_>) -> DialectMatch {
    DialectMatch {
        format: FORMAT.into(),
        dialect: Some(SatDialect::from_stream_kind(evidence.kind()).id()),
        declared: declared(evidence),
        admission: if admits_semantic_decode(evidence) {
            Admission::Admitted
        } else {
            Admission::Refused
        },
    }
}

#[cfg(test)]
mod tests;
