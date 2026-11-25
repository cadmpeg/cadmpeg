// SPDX-License-Identifier: Apache-2.0
//! Target domains, requests, and resolution against an encoder catalog.

use crate::codec::FormatId;
use crate::document::CadIr;
use crate::report::loss::{LossKind, LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind};
use crate::report::Severity;
use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::target::{
    DefaultSource, TargetCatalog, TargetDescriptor, TargetRefusal, TargetRefusalKind, TargetToken,
};
use cadmpeg_core::CodecError;

/// What the caller asked an encoder to write, before resolution picks it.
///
/// Synthesis and preservation are different capabilities. Synthesis is static
/// and input-independent: [`crate::codec::write::Encoder::targets`] is the
/// whole catalog.
/// Preservation is input-conditioned — replaying a retained baseline
/// reproduces dialects no encoder could synthesize for arbitrary input — so it
/// is asked for by [`TargetRequest::Inherit`], never by a catalog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetRequest<'a> {
    /// Preserve the source's dialect. The same-format default.
    Inherit,
    /// A synthesis target from [`crate::codec::write::Encoder::targets`]: an
    /// explicit target flag, or the catalog default for a cross-format
    /// conversion.
    Explicit(&'a str),
}

/// The code of the loss a write charges where the file is not the dialect its
/// same-format source records (§3.2, §3.4).
pub const SOURCE_DIALECT_DISPLACED: &str = "target.source-dialect-displaced";

/// The code of the loss a write charges where the file is not the catalog row
/// the request names (§3.2, §3.4).
pub const REQUEST_DISPLACED: &str = "target.request-displaced";

/// The kind of a `target.*` loss `code` that an encoder charges in its own
/// `namespace`.
#[must_use]
pub fn target_loss_kind(namespace: LossNamespace<'_>, code: &str) -> LossKind {
    NamespacedLossKind::new(namespace, code, LossTaxonomy::SourceDialectDisplaced).into()
}

fn target_loss(format: LossNamespace<'_>, code: &str, message: String) -> LossNote {
    LossNote::new(target_loss_kind(format, code), message).with_severity(Severity::Warning)
}

/// The catalog row a writer wrote in place of the row its resolution names
/// (§3.4, Refusals). The export report names this row as the target, and
/// the wrapper charges the displacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenRow {
    /// The dialect the written payload is.
    pub target: DialectId,
    /// Why the writer wrote this row and not the resolved one.
    pub reason: String,
}

/// The document's source identity relative to one encoder format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceIdentity {
    /// No source, or a source of another format.
    Other(DefaultSource),
    /// A same-format source whose dialect was not classified.
    Unrecorded,
    /// A same-format source with a recorded dialect.
    Recorded(DialectId),
}

impl SourceIdentity {
    /// The recorded same-format source dialect, if any.
    const fn recorded(&self) -> Option<&DialectId> {
        match self {
            Self::Recorded(dialect) => Some(dialect),
            Self::Other(_) | Self::Unrecorded => None,
        }
    }

    /// Whether the document records source metadata for this encoder format,
    /// classified or not.
    const fn is_same_format(&self) -> bool {
        matches!(self, Self::Unrecorded | Self::Recorded(_))
    }
}

/// How a request resolved. Each variant carries exactly the source relation
/// its refusals and queries need, so no query has an impossible case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedTarget<'a> {
    /// An explicit catalog request.
    Explicit {
        /// Position of `entry` in the encoder's declared catalog.
        index: usize,
        /// The selected catalog row.
        entry: &'static TargetDescriptor,
        /// The caller's spelling, verbatim.
        requested: &'a str,
        /// The document's source relative to this format.
        source: SourceIdentity,
        /// The request named a row the encoder reaches only by replaying a
        /// source of that dialect, and `entry` is the catalog default, or the
        /// newest row where the catalog declares none, planned in its place.
        displaced: bool,
    },
    /// Inheritance resolved to a catalog row equal to the recorded source.
    Inherited {
        /// Position of `entry` in the encoder's declared catalog.
        index: usize,
        /// The selected catalog row.
        entry: &'static TargetDescriptor,
        /// The recorded same-format source dialect this row preserves.
        source: DialectId,
    },
    /// Inheritance with no recorded same-format dialect took the catalog
    /// default, or the newest catalog row where the catalog declares no
    /// default.
    Default {
        /// Position of `entry` in the encoder's declared catalog.
        index: usize,
        /// The selected catalog row.
        entry: &'static TargetDescriptor,
        /// Why no source dialect could be inherited.
        source: DefaultSource,
    },
    /// Inheritance of a recorded same-format dialect that is not on the
    /// catalog; only verbatim replay can realize it.
    Preserved {
        /// The recorded source dialect to reproduce.
        source: DialectId,
    },
}

/// A native write resolved against the encoder catalog and source identity.
///
/// Only the internal `resolve_write_request` operation constructs this proof.
/// Codecs query it; they do not reconstruct the target/source relation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWrite<'a> {
    target: ResolvedTarget<'a>,
    available: TargetCatalog,
    replay_rows: &'static [DialectId],
}

impl<'a> ResolvedWrite<'a> {
    /// The resolution, for callers that branch on how the target was chosen.
    #[must_use]
    pub const fn target(&self) -> &ResolvedTarget<'a> {
        &self.target
    }

    /// The resolved catalog row, or `None` for a preserved off-catalog source.
    #[must_use]
    pub const fn entry(&self) -> Option<&'static TargetDescriptor> {
        match &self.target {
            ResolvedTarget::Explicit { entry, .. }
            | ResolvedTarget::Inherited { entry, .. }
            | ResolvedTarget::Default { entry, .. } => Some(entry),
            ResolvedTarget::Preserved { .. } => None,
        }
    }

    /// The catalog row a synthesized write of this resolution writes, with
    /// its position: the resolved row, or the catalog default for a preserved
    /// off-catalog source (§3.4, Refusals, `Inherit`). `None` only for a
    /// preserved source of a catalog that declares no default.
    #[must_use]
    pub fn synthesized_row(&self) -> Option<(usize, &'static TargetDescriptor)> {
        match &self.target {
            ResolvedTarget::Explicit { index, entry, .. }
            | ResolvedTarget::Inherited { index, entry, .. }
            | ResolvedTarget::Default { index, entry, .. } => Some((*index, entry)),
            ResolvedTarget::Preserved { .. } => self.available.default(),
        }
    }

    /// Position of the resolved row in the catalog the encoder declared, or
    /// `None` for a preserved off-catalog source.
    ///
    /// Catalogs projected from a version enumeration in order let codecs index
    /// that enumeration directly.
    #[must_use]
    pub const fn index(&self) -> Option<usize> {
        match &self.target {
            ResolvedTarget::Explicit { index, .. }
            | ResolvedTarget::Inherited { index, .. }
            | ResolvedTarget::Default { index, .. } => Some(*index),
            ResolvedTarget::Preserved { .. } => None,
        }
    }

    /// The resolved output dialect: the catalog row's id, or the preserved
    /// source id.
    #[must_use]
    pub const fn target_id(&self) -> &DialectId {
        match &self.target {
            ResolvedTarget::Explicit { entry, .. }
            | ResolvedTarget::Inherited { entry, .. }
            | ResolvedTarget::Default { entry, .. } => &entry.id,
            ResolvedTarget::Preserved { source } => source,
        }
    }

    /// Whether the resolved dialect is the recorded same-format source
    /// dialect.
    #[must_use]
    pub fn preserves_source(&self) -> bool {
        match &self.target {
            ResolvedTarget::Explicit { entry, source, .. } => source.recorded() == Some(&entry.id),
            ResolvedTarget::Inherited { .. } | ResolvedTarget::Preserved { .. } => true,
            ResolvedTarget::Default { .. } => false,
        }
    }

    /// The recorded same-format source dialect replaced by the resolved
    /// target, if any.
    #[must_use]
    pub fn displaced_source(&self) -> Option<&DialectId> {
        match &self.target {
            ResolvedTarget::Explicit { entry, source, .. } => {
                source.recorded().filter(|source| *source != &entry.id)
            }
            ResolvedTarget::Inherited { .. }
            | ResolvedTarget::Default { .. }
            | ResolvedTarget::Preserved { .. } => None,
        }
    }

    /// The dialect the report names for a write of this resolution, and the
    /// `target.*` losses of that write, in `format`'s namespace (§3.4).
    ///
    /// `synthesized` says the payload was synthesized, so a preserved
    /// off-catalog source was written as [`Self::synthesized_row`]. `row` is the
    /// row the writer states it wrote instead, if any. A named row that the
    /// file is not charges `target.request-displaced`; a recorded same-format
    /// dialect that the file is not, a same-format source that records no
    /// dialect, or a row the writer states in place of the row an `Inherit`
    /// request resolved to, charges `target.source-dialect-displaced`. An
    /// `Inherit` request names no row, so it charges only the source code.
    fn written(
        &self,
        format: LossNamespace<'_>,
        synthesized: bool,
        row: Option<WrittenRow>,
    ) -> (DialectId, Vec<LossNote>) {
        debug_assert!(
            row.as_ref().is_none_or(|row| self.reaches(&row.target)),
            "a writer states a row of its catalog, its replay rows, or its source"
        );
        let planned = match self.synthesized_row() {
            Some((_, entry)) if synthesized => entry.id.clone(),
            _ => self.target_id().clone(),
        };
        let (written, reason) = match row {
            Some(row) if row.target != planned => (row.target, Some(row.reason)),
            _ => (planned.clone(), None),
        };
        let restated = written != planned;
        let because = reason.map_or_else(String::new, |reason| format!("; {reason}"));
        let requested = match &self.target {
            ResolvedTarget::Explicit {
                requested,
                displaced: true,
                ..
            } => (!names_row(&written, requested, format.as_str())).then(|| {
                format!(
                    "requested target {requested} is a row the writer reaches only by \
                     replaying a source of that dialect; the file is written as \
                     {written}{because}"
                )
            }),
            ResolvedTarget::Explicit {
                entry, requested, ..
            } if written != entry.id => Some(format!(
                "requested target {requested} ({}) is a row the writer cannot reach from this \
                 document; the file is written as {written}{because}",
                entry.id
            )),
            _ => None,
        };
        let source = match &self.target {
            ResolvedTarget::Explicit {
                source: SourceIdentity::Recorded(source),
                ..
            } => (*source != written).then(|| source_displacement_message(source, &written)),
            ResolvedTarget::Inherited { source, .. } | ResolvedTarget::Preserved { source } => {
                (*source != written)
                    .then(|| source_displacement_message(source, &written) + &because)
            }
            ResolvedTarget::Default {
                source: DefaultSource::UnrecordedDialect,
                ..
            } => Some(format!(
                "the {} source records no dialect; the file is written as \
                 {written}{because}",
                format.as_str()
            )),
            ResolvedTarget::Default {
                source: DefaultSource::ForeignFormat(other),
                ..
            } if restated => Some(format!(
                "the document's source is {other}, which records no {} dialect; Inherit \
                 resolved to {planned}, and the file is written as {written}{because}",
                format.as_str()
            )),
            ResolvedTarget::Default {
                source: DefaultSource::NoSource,
                ..
            } if restated => Some(format!(
                "the document records no source; Inherit resolved to {planned}, and the file \
                 is written as {written}{because}"
            )),
            ResolvedTarget::Explicit { .. } | ResolvedTarget::Default { .. } => None,
        };
        let losses = requested
            .map(|message| target_loss(format, REQUEST_DISPLACED, message))
            .into_iter()
            .chain(source.map(|message| target_loss(format, SOURCE_DIALECT_DISPLACED, message)))
            .collect();
        (written, losses)
    }

    /// Whether `row` is a row this encoder writes from this document: a
    /// catalog row, a replay row, or the recorded same-format source.
    fn reaches(&self, row: &DialectId) -> bool {
        self.available.find(row.as_str()).is_some()
            || self.replay_rows.contains(row)
            || match &self.target {
                ResolvedTarget::Explicit { source, .. } => source.recorded() == Some(row),
                ResolvedTarget::Inherited { source, .. } | ResolvedTarget::Preserved { source } => {
                    source == row
                }
                ResolvedTarget::Default { .. } => false,
            }
    }

    /// Whether the document records source metadata for this encoder format.
    ///
    /// This includes a same-format source whose dialect was not classified.
    #[must_use]
    pub const fn has_same_format_source(&self) -> bool {
        match &self.target {
            ResolvedTarget::Explicit { source, .. } => source.is_same_format(),
            ResolvedTarget::Inherited { .. } | ResolvedTarget::Preserved { .. } => true,
            ResolvedTarget::Default { .. } => false,
        }
    }

    /// Whether preservation was eligible before codec-specific retained-image
    /// checks.
    ///
    /// An unclassified same-format source is eligible because no contradictory
    /// dialect is recorded. An explicit target that displaces a recorded
    /// source dialect is not.
    #[must_use]
    pub fn source_preservation_eligible(&self) -> bool {
        self.has_same_format_source() && self.displaced_source().is_none()
    }

    /// Builds a typed refusal when codec-specific delivery cannot realize the
    /// already-resolved request.
    #[must_use]
    pub fn unavailable(&self, reason: impl Into<String>) -> CodecError {
        let reason = reason.into();
        let kind = match &self.target {
            ResolvedTarget::Explicit {
                entry, requested, ..
            } => TargetRefusalKind::ExplicitUnavailable {
                target: entry.id.clone(),
                requested: TargetToken::new(*requested),
                reason,
            },
            ResolvedTarget::Inherited { source, .. } | ResolvedTarget::Preserved { source } => {
                TargetRefusalKind::InheritedUnavailable {
                    source: source.clone(),
                    reason,
                }
            }
            ResolvedTarget::Default { entry, source, .. } => {
                TargetRefusalKind::DefaultUnavailable {
                    target: entry.id.clone(),
                    source: source.clone(),
                    reason,
                }
            }
        };
        CodecError::UnsupportedTarget(Box::new(TargetRefusal::new(kind, self.available)))
    }
}

/// Whether the caller's token `requested` names `row` of `format`: its id, or
/// the id without the `format:` prefix.
fn names_row(row: &DialectId, requested: &str, format: &str) -> bool {
    row.as_str() == requested
        || row
            .as_str()
            .strip_prefix(format)
            .and_then(|local| local.strip_prefix(':'))
            == Some(requested)
}

fn source_displacement_message(source: &DialectId, written: &DialectId) -> String {
    format!(
        "source dialect {source} was displaced by target dialect {written}; the source dialect \
         identity is not preserved"
    )
}

mod domain_sealed {
    pub trait Sealed {}
    impl Sealed for super::DialectFree {}
    impl Sealed for super::Catalog {}
}

mod resolution_sealed {
    pub trait Sealed {}
    impl Sealed for () {}
    impl Sealed for super::ResolvedWrite<'_> {}
}

/// A target-domain resolution that states the export identity it proves.
pub trait TargetResolution: resolution_sealed::Sealed + Clone {
    /// The native target the report names for a write of this resolution,
    /// or `None` for the neutral document, and the `target.*` losses of that
    /// write in `format`'s namespace (§3.4). `synthesized` says the payload
    /// was synthesized; `row` is the row the writer states it wrote instead.
    #[doc(hidden)]
    fn report_target(
        &self,
        format: LossNamespace<'_>,
        synthesized: bool,
        row: Option<WrittenRow>,
    ) -> (Option<DialectId>, Vec<LossNote>);
}

impl TargetResolution for () {
    fn report_target(
        &self,
        _format: LossNamespace<'_>,
        _synthesized: bool,
        row: Option<WrittenRow>,
    ) -> (Option<DialectId>, Vec<LossNote>) {
        debug_assert!(row.is_none(), "a dialect-free write states no row");
        (None, Vec::new())
    }
}

impl TargetResolution for ResolvedWrite<'_> {
    fn report_target(
        &self,
        format: LossNamespace<'_>,
        synthesized: bool,
        row: Option<WrittenRow>,
    ) -> (Option<DialectId>, Vec<LossNote>) {
        let (target, losses) = self.written(format, synthesized, row);
        (Some(target), losses)
    }
}

/// Where an encoder's targets come from, and what a resolved request looks
/// like for that encoder.
///
/// Implemented only by [`DialectFree`] and [`Catalog`]. A backend names one
/// of them as its [`super::EncoderBackend::Target`], and `plan_resolved` receives that
/// domain's resolution type and nothing else.
pub trait TargetDomain: domain_sealed::Sealed {
    /// The proof handed to `plan_resolved` for one request.
    type Resolved<'a>: TargetResolution;

    /// The static catalog of output flavors this domain lists.
    fn targets(&self, format: FormatId) -> TargetCatalog;

    /// Resolves one request against this domain.
    #[doc(hidden)]
    fn resolve<'a>(
        &self,
        ir: &'a CadIr,
        request: TargetRequest<'a>,
        format: FormatId,
    ) -> Result<Self::Resolved<'a>, CodecError>;
}

/// The neutral representation has no dialect catalog or target identity.
///
/// An encoder in this domain takes [`TargetRequest::Inherit`] only and every
/// export it plans reports no target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DialectFree;

impl TargetDomain for DialectFree {
    type Resolved<'a> = ();

    fn targets(&self, format: FormatId) -> TargetCatalog {
        TargetCatalog::empty(format.as_str())
    }

    fn resolve<'a>(
        &self,
        _ir: &'a CadIr,
        request: TargetRequest<'a>,
        format: FormatId,
    ) -> Result<(), CodecError> {
        match request {
            TargetRequest::Inherit => Ok(()),
            TargetRequest::Explicit(id) => Err(TargetRefusal::unknown_explicit(
                id,
                TargetCatalog::empty(format.as_str()),
            )
            .into()),
        }
    }
}

/// A native format resolves every request against this complete catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Catalog {
    rows: TargetCatalog,
    fallback: (usize, &'static TargetDescriptor),
    replay_rows: &'static [DialectId],
}

impl Catalog {
    /// Builds a native target domain whose optional default indexes one of
    /// `targets`. An `Inherit` request that inherits no row resolves to the
    /// default, or to the newest (last) row when there is none.
    #[must_use]
    pub const fn new(targets: &'static [TargetDescriptor], default: Option<usize>) -> Self {
        let rows = TargetCatalog::new(targets, default);
        let index = match default {
            Some(index) => index,
            None => targets.len() - 1,
        };
        Self {
            rows,
            fallback: (index, &targets[index]),
            replay_rows: &[],
        }
    }

    /// Declares dialects the encoder reaches only by replaying a source of
    /// that dialect. An explicit request for one resolves to the row an
    /// `Inherit` request without a source dialect takes. A file the writer
    /// states as the requested row charges nothing; any other charges
    /// `target.request-displaced`.
    #[must_use]
    pub const fn with_replay_rows(mut self, rows: &'static [DialectId]) -> Self {
        self.replay_rows = rows;
        self
    }
}

impl TargetDomain for Catalog {
    type Resolved<'a> = ResolvedWrite<'a>;

    fn targets(&self, _format: FormatId) -> TargetCatalog {
        self.rows
    }

    fn resolve<'a>(
        &self,
        ir: &'a CadIr,
        request: TargetRequest<'a>,
        _format: FormatId,
    ) -> Result<ResolvedWrite<'a>, CodecError> {
        resolve_write_request(ir, request, *self)
    }
}

fn source_identity(ir: &CadIr, format: &str) -> SourceIdentity {
    let Some(source) = ir.source.as_ref() else {
        return SourceIdentity::Other(DefaultSource::NoSource);
    };
    if source.format() != format {
        return SourceIdentity::Other(DefaultSource::ForeignFormat(source.format().to_owned()));
    }
    match source.dialect() {
        Some(matched) => SourceIdentity::Recorded(matched.dialect().clone()),
        None => SourceIdentity::Unrecorded,
    }
}

/// Resolve a native target and inheritance once, before codec-specific delivery.
///
/// Native requests always name a catalog or preserved off-catalog dialect.
/// A dialect-free neutral encoder handles its format identity locally instead
/// of adding an identity case to every native writer.
fn resolve_write_request<'a>(
    ir: &CadIr,
    request: TargetRequest<'a>,
    domain: Catalog,
) -> Result<ResolvedWrite<'a>, CodecError> {
    let Catalog {
        rows: catalog,
        fallback: (index, entry),
        replay_rows,
    } = domain;
    let format = catalog.namespace();
    let refuse = |kind| CodecError::UnsupportedTarget(Box::new(TargetRefusal::new(kind, catalog)));
    let source = source_identity(ir, format);
    let default = |source: DefaultSource| ResolvedTarget::Default {
        index,
        entry,
        source,
    };
    let target = match request {
        TargetRequest::Explicit(requested) => {
            let (index, entry, displaced) = match catalog.find(requested) {
                Some((index, entry)) => (index, entry, false),
                None if replay_rows
                    .iter()
                    .any(|row| names_row(row, requested, format)) =>
                {
                    (index, entry, true)
                }
                None => {
                    return Err(refuse(TargetRefusalKind::UnknownExplicit {
                        requested: TargetToken::new(requested),
                    }))
                }
            };
            ResolvedTarget::Explicit {
                index,
                entry,
                requested,
                source,
                displaced,
            }
        }
        TargetRequest::Inherit => match source {
            SourceIdentity::Other(source) => default(source),
            SourceIdentity::Unrecorded => default(DefaultSource::UnrecordedDialect),
            SourceIdentity::Recorded(source) => match catalog.find(source.as_str()) {
                Some((index, entry)) => ResolvedTarget::Inherited {
                    index,
                    entry,
                    source,
                },
                None => ResolvedTarget::Preserved { source },
            },
        },
    };
    Ok(ResolvedWrite {
        target,
        available: catalog,
        replay_rows,
    })
}
#[cfg(test)]
mod tests;
