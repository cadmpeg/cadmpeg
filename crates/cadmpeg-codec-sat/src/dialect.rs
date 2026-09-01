// SPDX-License-Identifier: Apache-2.0
//! SAT dialect identity: which registry row a bare stream is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, [`DialectId::pinned`]
//! strings are the boundary, [`classify`] is the one construction path, and the
//! vocabulary is closed. Tests close it directly against the reportable rows
//! in `docs/dialects.toml`.
//!
//! This module owns the primary `sat:` host layer. Each classified stream also
//! emits the non-primary `acis:` kernel layer owned by `cadmpeg-asm`.
//!
//! # Identity and admission are the host grammar
//!
//! The host rows are discriminated by the stream's leading bytes alone
//! ([`StreamKind`]), which are read exactly: `ASM BinaryFile4`/`8`, `ACIS
//! BinaryFile`, or the two text header lines. A recognized stream that has no
//! binary record-stream boundary or complete text framing takes
//! [`Admission::Refused`]. Inspection reports that match, and decode returns
//! [`cadmpeg_core::CodecError::UnsupportedDialect`] carrying the same host and
//! kernel layers.
//!
//! The kernel save format is banded on the separate `acis:` layer. The Spatial
//! ACIS record decoders are verified against majors 217 and 218; the ASM record
//! decoders compare no save format at all. A stream outside the verified band
//! is not refused: its records are read with the verified band's grammar, which
//! is [`Admission::AdmittedUnverified`] on that layer, and `using` names the
//! verified binary `acis:` row whose grammar was substituted. An ACIS text
//! stream outside that band has no declared text-band grammar to name and is
//! admitted unverified without `using`. The recovery is charged as
//! [`SatLossCode::SourceDialectUnverified`] by [`dialect_loss`], on a result
//! that carries whatever those records decoded.
//!
//! [`layers`] classifies both layers once, so the report and result share the
//! same host and kernel decisions.
//!
//! [`SatLossCode::SourceDialectUnverified`]:
//!     crate::loss::SatLossCode::SourceDialectUnverified

use crate::detect::StreamKind;
use cadmpeg_asm::kernel_header::KernelHeader;
use cadmpeg_asm::sat;
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use cadmpeg_ir::report::LossNote;
use std::collections::BTreeMap;

use crate::loss::SatLossCode;

/// Key of the stream encoding in [`DialectMatch::declared`].
const DECLARED_ENCODING: &str = "encoding";
/// Key of the text stream's terminator line in [`DialectMatch::declared`].
/// Absent on the binary branches, which carry no terminator.
const DECLARED_TERMINATOR: &str = "terminator";

impl StreamKind {
    /// Every stream kind that can produce a dialect report.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 3] = [Self::AsmBinary, Self::AcisBinary, Self::Text];

    /// The pinned registry id when this kind reaches classification.
    ///
    /// One row of `docs/dialects.toml` under the `sat` namespace per stream
    /// kind: the discriminant is the leading magic, or the two-line header shape
    /// for text. `detect::confidence` reports [`Confidence::No`] for
    /// [`Self::Unknown`], and both entry points return before classification,
    /// so it has no reportable id.
    ///
    /// Identity here is the detection discriminant and nothing else. This is the
    /// only registry string boundary the enum has.
    ///
    /// [`Confidence::No`]: cadmpeg_ir::codec::Confidence::No
    pub(crate) const fn reportable_id(self) -> Option<DialectId> {
        match self {
            Self::AsmBinary => Some(DialectId::pinned("sat:asm-binary")),
            Self::AcisBinary => Some(DialectId::pinned("sat:acis-binary")),
            Self::Text => Some(DialectId::pinned("sat:text")),
            Self::Unknown => None,
        }
    }
}

/// The text branch's evidence.
///
/// The terminator line selects the branch, and the header carries the save
/// format the ACIS branch is banded on. The band keys on the terminator, not on
/// the product family the header names. Outside the verified band, an ACIS
/// terminator has no declared text-band grammar to name as `using`.
pub(crate) struct TextEvidence<'a> {
    /// Branch the terminator line selected.
    pub(crate) branch: sat::Terminator,
    /// Kernel header the three text header lines carried.
    pub(crate) header: &'a KernelHeader,
}

/// What one stream's own bytes said, as the reading path read them.
///
/// One variant per reportable [`StreamKind`]. `None` inside a variant means
/// the kind's discriminant matched but nothing past it parsed. Inspection
/// reports what it could read, and decode refuses with the same primary match.
pub(crate) enum StreamEvidence<'a> {
    /// `ASM BinaryFile4`/`8`. The header is `None` only if the magic matched
    /// and `asm_header::parse` still declined, which its own contract makes
    /// unreachable; the arm keeps this function total.
    AsmBinary(Option<&'a KernelHeader>),
    /// Parsed ASM header from a stream with no record-stream frame.
    UnframedAsmBinary(&'a KernelHeader),
    /// `ACIS BinaryFile`, under the same header-parse note as
    /// [`Self::AsmBinary`].
    AcisBinary(Option<&'a KernelHeader>),
    /// Parsed ACIS header from a stream with no record-stream frame.
    UnframedAcisBinary(&'a KernelHeader),
    /// Text header lines; `None` when the stream did not parse past them.
    Text(Option<TextEvidence<'a>>),
}

impl StreamEvidence<'_> {
    /// The stream kind this evidence came from.
    const fn kind(&self) -> StreamKind {
        match self {
            Self::AsmBinary(_) => StreamKind::AsmBinary,
            Self::UnframedAsmBinary(_) => StreamKind::AsmBinary,
            Self::AcisBinary(_) => StreamKind::AcisBinary,
            Self::UnframedAcisBinary(_) => StreamKind::AcisBinary,
            Self::Text(_) => StreamKind::Text,
        }
    }
}

/// How the host stream was admitted.
///
/// [`Admission::Refused`] here is structural and nothing else: the host
/// discriminant matched but the stream did not frame. Kernel-band admission is
/// owned by `cadmpeg_asm::dialect::classify` and cannot change this host state.
fn admission(evidence: &StreamEvidence<'_>) -> Admission {
    match evidence {
        StreamEvidence::AsmBinary(Some(_))
        | StreamEvidence::AcisBinary(Some(_))
        | StreamEvidence::Text(Some(_)) => Admission::Admitted,
        StreamEvidence::AsmBinary(None)
        | StreamEvidence::UnframedAsmBinary(_)
        | StreamEvidence::AcisBinary(None)
        | StreamEvidence::UnframedAcisBinary(_)
        | StreamEvidence::Text(None) => Admission::Refused,
    }
}

/// The recovery loss a match charges, if it recovered.
///
/// `Some` exactly when the kernel match reports
/// [`Admission::AdmittedUnverified`]. A binary recovery names the substituted
/// binary row. A text recovery states that no declared text-band grammar was
/// available. The message is not the contract; the code is.
pub(crate) fn dialect_loss(matched: &DialectMatch) -> Option<LossNote> {
    cadmpeg_asm::dialect::unverified_message("the stream", matched)
        .map(|message| SatLossCode::SourceDialectUnverified.note(message))
}

/// Host-framing declarations, verbatim, under keys pinned above.
///
/// Kernel save format belongs only to the separate `acis:` match.
fn declared(evidence: &StreamEvidence<'_>) -> BTreeMap<String, String> {
    let mut declared = BTreeMap::new();
    match evidence {
        StreamEvidence::AsmBinary(_)
        | StreamEvidence::UnframedAsmBinary(_)
        | StreamEvidence::AcisBinary(_)
        | StreamEvidence::UnframedAcisBinary(_) => {
            declared.insert(DECLARED_ENCODING.into(), "binary".into());
        }
        StreamEvidence::Text(text) => {
            declared.insert(DECLARED_ENCODING.into(), "text".into());
            let Some(text) = text else { return declared };
            declared.insert(
                DECLARED_TERMINATOR.into(),
                terminator_line(text.branch).into(),
            );
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
/// [`admission`]. Kernel save-format identity and admission are not copied into
/// this host match.
fn classify(evidence: &StreamEvidence<'_>) -> DialectMatch {
    let dialect = evidence
        .kind()
        .reportable_id()
        .expect("stream evidence is constructed only for reportable kinds");
    DialectMatch::from_admission(dialect, admission(evidence))
        .expect("SAT dialect admissions use only SAT grammar ids")
        .with_declared(declared(evidence))
}

/// Classify the same evidence as the shared non-primary kernel layer.
fn kernel_layer(evidence: &StreamEvidence<'_>) -> DialectMatch {
    let header = match evidence {
        StreamEvidence::AsmBinary(Some(header)) => {
            cadmpeg_asm::dialect::KernelHeaderRef::Asm(header)
        }
        StreamEvidence::UnframedAsmBinary(header) => {
            cadmpeg_asm::dialect::KernelHeaderRef::Asm(header)
        }
        StreamEvidence::AcisBinary(Some(header)) => {
            cadmpeg_asm::dialect::KernelHeaderRef::Acis(header)
        }
        StreamEvidence::UnframedAcisBinary(header) => {
            cadmpeg_asm::dialect::KernelHeaderRef::Acis(header)
        }
        StreamEvidence::Text(Some(text)) => match text.branch {
            sat::Terminator::Asm => cadmpeg_asm::dialect::KernelHeaderRef::TextAsm(text.header),
            sat::Terminator::Acis => cadmpeg_asm::dialect::KernelHeaderRef::TextAcis(text.header),
        },
        StreamEvidence::AsmBinary(None)
        | StreamEvidence::AcisBinary(None)
        | StreamEvidence::Text(None) => cadmpeg_asm::dialect::KernelHeaderRef::Unknown,
    };
    cadmpeg_asm::dialect::classify(header)
}

/// Classifies the host and kernel layers from one evidence value.
pub(crate) fn layers(evidence: &StreamEvidence<'_>) -> (DialectMatch, DialectMatch) {
    (classify(evidence), kernel_layer(evidence))
}

#[cfg(test)]
mod tests;
