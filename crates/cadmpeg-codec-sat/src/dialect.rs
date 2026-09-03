// SPDX-License-Identifier: Apache-2.0
//! SAT dialect identity: which registry row a bare stream is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, registry-generated
//! [`DialectId`] constants are the boundary, [`classify`] is the one
//! construction path, and the vocabulary is closed.
//!
//! This module owns the primary `sat:` host layer. Each classified stream also
//! emits the non-primary `acis:` kernel layer owned by `cadmpeg-asm`.
//!
//! # Identity and admission are the host grammar
//!
//! The host rows are discriminated by the stream's leading bytes alone
//! ([`crate::detect::StreamKind`]), which are read exactly: `ASM BinaryFile4`/`8`, `ACIS
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
//! is [`Admission::Unverified`] on that layer, and `using` names the verified
//! binary `acis:` row whose grammar was substituted. An ACIS text stream
//! outside that band has no declared text-band grammar to name and is
//! [`Admission::Residual`]. The recovery is charged as
//! [`SatLossCode::SourceDialectUnverified`] by [`dialect_loss`], on a result
//! that carries whatever those records decoded.
//!
//! [`layers`] classifies both layers once, so the report and result share the
//! same host and kernel decisions.
//!
//! [`SatLossCode::SourceDialectUnverified`]:
//!     crate::loss::SatLossCode::SourceDialectUnverified

use crate::{SAT_ACIS_BINARY, SAT_ASM_BINARY, SAT_TEXT};
use cadmpeg_asm::kernel_header::KernelHeader;
use cadmpeg_asm::sat;
use cadmpeg_core::dialect::{DialectId, DialectMatch};
use cadmpeg_ir::report::LossNote;
use std::collections::BTreeMap;

use crate::loss::SatLossCode;

/// Key of the stream encoding in [`DialectMatch::declared`].
const DECLARED_ENCODING: &str = "encoding";
/// Key of the text stream's terminator line in [`DialectMatch::declared`].
/// Absent on the binary branches, which carry no terminator.
const DECLARED_TERMINATOR: &str = "terminator";

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
/// One variant per reportable [`StreamKind`]. Inspection reports what it could
/// read, and decode refuses with the same primary match.
pub(crate) enum StreamEvidence<'a> {
    /// Parsed `ASM BinaryFile4`/`8` header and record-stream frame.
    AsmBinary(&'a KernelHeader),
    /// Parsed ASM header from a stream with no record-stream frame.
    UnframedAsmBinary(&'a KernelHeader),
    /// Parsed `ACIS BinaryFile` header and record-stream frame.
    AcisBinary(&'a KernelHeader),
    /// Parsed ACIS header from a stream with no record-stream frame.
    UnframedAcisBinary(&'a KernelHeader),
    /// Text header lines; `None` when the stream did not parse past them.
    Text(Option<TextEvidence<'a>>),
}

impl StreamEvidence<'_> {
    /// The host row this evidence identifies. Every variant is a reportable
    /// kind, so the id is total: [`StreamKind::Unknown`] never reaches here.
    const fn dialect(&self) -> DialectId {
        match self {
            Self::AsmBinary(_) | Self::UnframedAsmBinary(_) => SAT_ASM_BINARY,
            Self::AcisBinary(_) | Self::UnframedAcisBinary(_) => SAT_ACIS_BINARY,
            Self::Text(_) => SAT_TEXT,
        }
    }
}

/// The host match: identity from the discriminant, admission from framing.
///
/// [`Admission::Refused`] here is structural and nothing else: the host
/// discriminant matched but the stream did not frame. Kernel-band admission is
/// owned by `cadmpeg_asm::dialect::classify` and cannot change this host state.
fn host(evidence: &StreamEvidence<'_>) -> DialectMatch {
    let dialect = evidence.dialect();
    match evidence {
        StreamEvidence::AsmBinary(_)
        | StreamEvidence::AcisBinary(_)
        | StreamEvidence::Text(Some(_)) => DialectMatch::admitted(dialect),
        StreamEvidence::UnframedAsmBinary(_)
        | StreamEvidence::UnframedAcisBinary(_)
        | StreamEvidence::Text(None) => DialectMatch::refused(dialect),
    }
}

/// The recovery loss a match charges, if it recovered.
///
/// `Some` exactly when the kernel match reports [`Admission::Unverified`] or
/// [`Admission::Residual`]. A binary recovery names the substituted binary
/// row. A text recovery states that no declared text-band grammar was
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
/// [`host`]. Kernel save-format identity and admission are not copied into
/// this host match.
fn classify(evidence: &StreamEvidence<'_>) -> DialectMatch {
    host(evidence).with_declared(declared(evidence))
}

/// Classify the same evidence as the shared non-primary kernel layer.
fn kernel_layer(evidence: &StreamEvidence<'_>) -> DialectMatch {
    let header = match evidence {
        StreamEvidence::AsmBinary(header) => cadmpeg_asm::dialect::KernelHeaderRef::Asm(header),
        StreamEvidence::UnframedAsmBinary(header) => {
            cadmpeg_asm::dialect::KernelHeaderRef::Asm(header)
        }
        StreamEvidence::AcisBinary(header) => cadmpeg_asm::dialect::KernelHeaderRef::Acis(header),
        StreamEvidence::UnframedAcisBinary(header) => {
            cadmpeg_asm::dialect::KernelHeaderRef::Acis(header)
        }
        StreamEvidence::Text(Some(text)) => match text.branch {
            sat::Terminator::Asm => cadmpeg_asm::dialect::KernelHeaderRef::TextAsm(text.header),
            sat::Terminator::Acis => cadmpeg_asm::dialect::KernelHeaderRef::TextAcis(text.header),
        },
        StreamEvidence::Text(None) => cadmpeg_asm::dialect::KernelHeaderRef::Unknown,
    };
    cadmpeg_asm::dialect::classify(header)
}

/// Classifies the host and kernel layers from one evidence value.
pub(crate) fn layers(evidence: &StreamEvidence<'_>) -> (DialectMatch, DialectMatch) {
    (classify(evidence), kernel_layer(evidence))
}

#[cfg(test)]
mod tests;
