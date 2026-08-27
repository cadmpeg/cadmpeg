// SPDX-License-Identifier: Apache-2.0
//! SAT dialect identity: which registry row a bare stream is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, [`DialectId::pinned`]
//! strings are the boundary, [`classify`] is the one construction path, and the
//! vocabulary is closed. Every [`StreamKind`] has a row in `docs/dialects.toml`
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
//! BinaryFile`, or the two text header lines. A stream that stops at its own
//! discriminant — the magic matched and nothing past it parsed — is
//! structurally unframed and takes [`Admission::Refused`]. That state is
//! reachable at inspect only; decode returns a malformed error on the same
//! bytes.
//!
//! What *is* banded is the kernel save format inside the stream. The Spatial
//! ACIS record decoders are verified against majors 217 and 218; the ASM record
//! decoders compare no save format at all. A stream outside the verified band
//! is not refused: its records are read with the verified band's grammar, which
//! is [`Admission::AdmittedUnverified`] exactly, and `nearest` names the
//! verified `acis:` row whose grammar was substituted. The recovery is charged
//! as [`SatLossCode::SourceDialectUnverified`] by [`dialect_loss`], on a result
//! that carries whatever those records decoded.
//!
//! [`admission`] is the single construction path for both, so the report and
//! the result can never disagree.
//!
//! [`SatLossCode::SourceDialectUnverified`]:
//!     crate::loss::SatLossCode::SourceDialectUnverified

use crate::detect::StreamKind;
use cadmpeg_asm::dialect::{acis_band_verified, nearest_verified_acis};
use cadmpeg_asm::kernel_header::KernelHeader;
use cadmpeg_asm::sat;
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use cadmpeg_ir::report::LossNote;
use std::collections::BTreeMap;

use crate::loss::SatLossCode;
use crate::FORMAT;

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

impl StreamKind {
    /// Every dialect this codec can name.
    ///
    /// The registry cross-check is its only consumer, and that is the point:
    /// the list exists so a stream kind added without a registry row, or a row
    /// added without a stream kind, fails a test.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 4] =
        [Self::AsmBinary, Self::AcisBinary, Self::Text, Self::Unknown];

    /// The pinned registry id.
    ///
    /// One row of `docs/dialects.toml` under the `sat` namespace per stream
    /// kind: the discriminant is the leading magic, or the two-line header shape
    /// for text. [`Self::Unknown`] is the mandatory totality row (design §3.3,
    /// B4); `detect::confidence` reports [`Confidence::No`] for it, so it is
    /// unreachable through the normal catalog and exists to keep classification
    /// total.
    ///
    /// Identity here is the detection discriminant and nothing else. This is the
    /// only registry string boundary the enum has.
    ///
    /// [`Confidence::No`]: cadmpeg_ir::codec::Confidence::No
    pub(crate) const fn id(self) -> DialectId {
        DialectId::pinned(match self {
            Self::AsmBinary => "sat:asm-binary",
            Self::AcisBinary => "sat:acis-binary",
            Self::Text => "sat:text",
            Self::Unknown => "sat:unknown",
        })
    }
}

/// The text branch's evidence.
///
/// The terminator line selects the branch, and the header carries the save
/// format the ACIS branch is banded on. The band keys on the terminator, not on
/// the product family the header names, so an ASM product string under an ACIS
/// terminator recovers outside the band, symmetrically with binary.
pub(crate) struct TextEvidence<'a> {
    /// Branch the terminator line selected.
    pub(crate) branch: sat::Terminator,
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

/// How this stream was admitted.
///
/// The one construction path for the admission, and so for the
/// `source.dialect-unverified` recovery mark [`dialect_loss`] charges from it.
/// [`Admission::Refused`] here is structural and nothing else: the discriminant
/// matched but the stream did not frame. Both ACIS branches take the same band
/// comparison, and both recover outside it; the ASM binary and ASM text
/// branches compare no save format, so they are admitted at any band.
fn admission(evidence: &StreamEvidence<'_>) -> Admission {
    let major = match evidence {
        StreamEvidence::AsmBinary(Some(_)) => return Admission::Admitted,
        StreamEvidence::AcisBinary(Some(header)) => header.save_format_major(),
        StreamEvidence::Text(Some(text)) => match text.branch {
            sat::Terminator::Asm => return Admission::Admitted,
            sat::Terminator::Acis => text.header.save_format_major(),
        },
        StreamEvidence::AsmBinary(None)
        | StreamEvidence::AcisBinary(None)
        | StreamEvidence::Text(None)
        | StreamEvidence::Unknown => return Admission::Refused,
    };
    if acis_band_verified(major) {
        Admission::Admitted
    } else {
        Admission::AdmittedUnverified {
            nearest: nearest_verified_acis(major),
        }
    }
}

/// The recovery loss a match charges, if it recovered.
///
/// `Some` exactly when [`classify`] reported [`Admission::AdmittedUnverified`],
/// which is exactly when the Spatial ACIS record grammar of a verified band was
/// substituted for the band the stream declared. The message states the
/// declaration and the substitution; it is not the contract, the code is.
pub(crate) fn dialect_loss(matched: &DialectMatch) -> Option<LossNote> {
    let Admission::AdmittedUnverified { nearest } = &matched.admission else {
        return None;
    };
    let declared = match (
        matched.declared.get(DECLARED_SAVE_FORMAT_MAJOR),
        matched.declared.get(DECLARED_SAVE_FORMAT_MINOR),
    ) {
        (Some(major), Some(minor)) => format!("save format {major}.{minor}"),
        (Some(major), None) => format!("save format major {major}"),
        (None, _) => "no save format".to_owned(),
    };
    Some(SatLossCode::SourceDialectUnverified.note(format!(
        "the stream declares {declared}, which no verified Spatial ACIS band declares; its \
         records were read with the grammar `{nearest}` declares, and what they decoded is \
         reported as it decoded"
    )))
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
pub(crate) const fn terminator_line(branch: sat::Terminator) -> &'static str {
    match branch {
        sat::Terminator::Asm => "End-of-ASM-data",
        sat::Terminator::Acis => "End-of-ACIS-data",
    }
}

/// Classifies one stream. The single construction path for a [`DialectMatch`]
/// in this codec, so a classification bug and the report can never disagree.
///
/// Identity is the row the leading discriminant satisfies; admission is
/// [`admission`]. The two are computed independently and never from each other:
/// an ACIS stream outside the verified band keeps its own registry row while
/// its records are read with a verified band's grammar.
pub(crate) fn classify(evidence: &StreamEvidence<'_>) -> DialectMatch {
    DialectMatch {
        format: FORMAT.into(),
        dialect: Some(evidence.kind().id()),
        declared: declared(evidence),
        admission: admission(evidence),
    }
}

#[cfg(test)]
mod tests;
