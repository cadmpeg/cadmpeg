// SPDX-License-Identifier: Apache-2.0
//! SLDPRT dialect identity: which registry row a document is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, [`DialectId::pinned`]
//! strings are the boundary, [`SldprtDialect::classify`] is the one
//! construction path, and the vocabulary is closed. Every variant here has a
//! row in `docs/dialects.toml`; `tests::every_pinned_id_has_a_registry_row`
//! fails on drift in either direction.
//!
//! # The axis is `swVersion`, and it is the only one that selects a layout
//!
//! `[format.sldprt]` declares `complete = false`: the vendor's release space
//! is not enumerable from any published document, so the rows are grammar
//! classes plus the mandatory `unknown` row.
//!
//! Two document-wide discriminants exist and only one is a grammar boundary.
//! The container branch — compound file versus native block envelope
//! ([`crate::container::looks_like_sldprt`]) — is the pre-parse dispatch, but
//! the outer version word it yields is never compared to anything: `scan` hard
//! -sets it to `0` on the compound-file branch and reads a big-endian `u32` at
//! offset 4 on the native branch, and both values only reach the
//! `outer_version` attribute. It is provenance, not evidence, and for that
//! reason it is not a declared key here either: a value cadmpeg synthesizes on
//! one of the two branches is not something the source declared.
//!
//! `swVersion` is the axis. It is not in the container header: it is an
//! attribute of a `swSolidWorks` XML payload extracted after the scan, and it
//! selects the dialect row, whose [`SldprtDialect::form_code_padding`] method owns the byte width of
//! the feature-operation form-code padding — four bytes below 12000, eight at
//! 12000 and above. That padding shifts every feature-operation read, so the
//! boundary is B1.
//!
//! # The declaration is evidence; the id is identity
//!
//! [`DialectMatch::declared`] records the `swVersion` attribute verbatim, as
//! the source wrote it. [`DialectMatch::dialect`] records which registry row
//! the document satisfies. They are different statements and a consumer must
//! not join them: `swVersion="SW2019"` is recorded verbatim and classifies as
//! `sldprt:unknown`, because the row's discriminant is a usable numeric
//! declaration and that string is not one. Parse a version out of an id and
//! the answer is wrong for exactly the files whose declarations are wrong.
//!
//! # The residual row is never `Admitted`
//!
//! Admission verifies a *declared* identity, and `sldprt:unknown` is the
//! absence of one. So the two versioned rows are [`Admission::Admitted`] and
//! `sldprt:unknown` is [`Admission::AdmittedUnverified`], naming *itself* as
//! `nearest`: the strategy substituted for the missing declaration is the
//! row's own declared fallback, and no other row lent its grammar.
//!
//! That fallback is well-defined — the padding filter is not applied and the
//! ambiguity resolver requires the two candidate offsets to agree before it
//! binds an operation code — and it is tempting to call the result verified on
//! that basis. It is not: agreement between candidates is *consistency*, not a
//! declaration. Nothing in the file said which padding it was written with, so
//! nothing was verified against a declaration, and a part that declares
//! nothing must stay distinguishable in the ladder from a part whose
//! declaration was checked. Every golden fixture in this crate is synthetic
//! and version-less, so every one of them sits on this row; making that
//! visible is exactly what the totality row is for.
//!
//! [`Admission::Refused`] is unreachable: this codec refuses only on container
//! framing, I/O, and entity-budget grounds, all of which return
//! [`cadmpeg_core::CodecError`] before any report exists, and none of which is
//! a dialect judgement.
//!
//! Where the padding filter is absent and the candidates disagree, the
//! resolver binds nothing. That is a loss inside a dialect, expressed through
//! the ordinary loss vocabulary, not an admission state.

use crate::container::ContainerScan;
use crate::loss::SldprtLossCode;
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use cadmpeg_ir::codec::TargetDescriptor;
use cadmpeg_ir::LossNote;
use std::collections::BTreeMap;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "sldprt";

/// The one dialect this writer synthesizes.
///
/// `writer::generated_solidworks_xml` emits a `swSolidWorks` block with no
/// `swVersion` attribute, so a synthesized part carries no version declaration
/// and classifies into the registry's totality row. Every versioned row is
/// reachable only by preserving a retained part.
pub(crate) const TARGETS: &[TargetDescriptor] = &[TargetDescriptor {
    id: SldprtDialect::Unknown.pinned(),
    label: "SolidWorks part with no swVersion declaration",
    aliases: &[],
    default: true,
}];

/// Key of the `swSolidWorks` `swVersion` attribute in
/// [`DialectMatch::declared`].
///
/// Absent when the document declares no `swVersion`, which is every document
/// carrying no `swSolidWorks` XML payload at all. The value is the attribute
/// text exactly as written, including declarations that do not read as a
/// number; it is the same string that reaches
/// `SourceMeta::attributes["sw_version"]`.
pub(crate) const DECLARED_SW_VERSION: &str = "sw_version";

/// One row of `docs/dialects.toml` under the `sldprt` namespace.
///
/// Three rows, and the classification is total over them: the two grammar
/// classes selected by the padding boundary, plus the mandatory `unknown` row that
/// absorbs every declaration it cannot use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SldprtDialect {
    SwVersionPre12000,
    SwVersion12000Plus,
    Unknown,
}

impl SldprtDialect {
    /// Every dialect this codec can name.
    ///
    /// The registry cross-check is its only consumer, and that is the point:
    /// the list exists so a variant added without a registry row, or a row
    /// added without a variant, fails a test.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 3] = [
        Self::SwVersionPre12000,
        Self::SwVersion12000Plus,
        Self::Unknown,
    ];

    /// The pinned registry id. The only string boundary this enum has.
    pub(crate) const fn id(self) -> DialectId {
        DialectId::pinned(self.pinned())
    }

    const fn pinned(self) -> &'static str {
        match self {
            Self::SwVersionPre12000 => "sldprt:sw-version-pre-12000",
            Self::SwVersion12000Plus => "sldprt:sw-version-12000-plus",
            Self::Unknown => "sldprt:unknown",
        }
    }

    /// The row a `swVersion` declaration selects.
    ///
    /// The padding this row owns is the discriminant, so a classification bug
    /// and a decode bug cannot be different bugs. The
    /// boundary value 12000 belongs to the `Eight` arm, and every declaration
    /// the padding rule cannot use — absent, non-numeric, negative,
    /// wider than `u32`, or zero — lands on [`Self::Unknown`].
    pub(crate) fn from_declaration(sw_version: Option<&str>) -> Self {
        match sw_version.and_then(|value| value.parse::<u32>().ok()) {
            Some(1..12_000) => Self::SwVersionPre12000,
            Some(12_000..) => Self::SwVersion12000Plus,
            _ => Self::Unknown,
        }
    }

    /// Feature-operation form-code padding width selected by this row.
    pub(crate) const fn form_code_padding(self) -> Option<usize> {
        match self {
            Self::SwVersionPre12000 => Some(4),
            Self::SwVersion12000Plus => Some(8),
            Self::Unknown => None,
        }
    }

    /// The typed row carried by an existing classification.
    pub(crate) fn from_match(matched: &DialectMatch) -> Option<Self> {
        let id = matched.dialect.as_ref()?;
        [
            Self::SwVersionPre12000,
            Self::SwVersion12000Plus,
            Self::Unknown,
        ]
        .into_iter()
        .find(|dialect| dialect.id() == *id)
    }

    /// How a document on this row was admitted.
    ///
    /// The one predicate behind both the report's [`Admission`] and
    /// [`dialect_loss`]: a versioned row carries a declared identity that the
    /// parse verified, and the residual row carries no declared identity at
    /// all, so it names itself as the substituted strategy. Neither answer is
    /// recomputed anywhere else.
    fn admission(self) -> Admission {
        match self {
            Self::SwVersionPre12000 | Self::SwVersion12000Plus => Admission::Admitted,
            Self::Unknown => Admission::AdmittedUnverified {
                nearest: Self::Unknown.id(),
            },
        }
    }

    /// Classifies one document from its `swVersion` declaration. The single
    /// construction path for a [`DialectMatch`] in this codec, so a
    /// classification bug and the report can never disagree.
    pub(crate) fn classify(sw_version: Option<&str>) -> DialectMatch {
        let mut declared = BTreeMap::new();
        if let Some(value) = sw_version {
            declared.insert(DECLARED_SW_VERSION.into(), value.to_owned());
        }
        let dialect = Self::from_declaration(sw_version);
        DialectMatch::layer(FORMAT, dialect.id(), declared, dialect.admission())
    }

    /// Classifies one scanned document, reading the declaration from the scan.
    ///
    /// The read is [`crate::container::declared_sw_version`], which is the same
    /// extraction that fills `SourceMeta::attributes["sw_version"]`. Callers
    /// that have already built those attributes read the key from there and
    /// call [`Self::classify`] instead, so the two never diverge.
    pub(crate) fn classify_scan(scan: &ContainerScan<'_>) -> DialectMatch {
        Self::classify(crate::container::declared_sw_version(scan).as_deref())
    }
}

/// The dialect-unverified loss for a classified layer.
///
/// `None` exactly when `matched.admission` is [`Admission::Admitted`], because
/// this reads that field rather than reclassifying. The biconditional the
/// decode policy requires is therefore structural: the note charged and the
/// admission reported come from one value, not from two authors agreeing.
pub(crate) fn dialect_loss(matched: &DialectMatch) -> Option<LossNote> {
    match &matched.admission {
        Admission::Admitted => None,
        Admission::AdmittedUnverified { nearest } => {
            let declaration = match matched.declared.get(DECLARED_SW_VERSION) {
                Some(value) => format!(
                    "the swSolidWorks swVersion declaration {value:?} does not read as a version \
                     above zero"
                ),
                None => "the document carries no swSolidWorks swVersion declaration".to_owned(),
            };
            Some(SldprtLossCode::SourceDialectUnverified.note(format!(
                "{declaration}, so no declared identity was verified. The document is read on \
                 {nearest}: the feature-operation form-code padding filter is not applied, and an \
                 operation code binds only where the four- and eight-byte candidates agree. \
                 Agreement is consistency, not a declaration."
            )))
        }
        Admission::Refused => None,
    }
}

#[cfg(test)]
mod tests;
