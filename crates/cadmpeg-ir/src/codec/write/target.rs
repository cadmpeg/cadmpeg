// SPDX-License-Identifier: Apache-2.0
//! Target domains, requests, and resolution against an encoder catalog.

use crate::codec::FormatId;
use crate::document::CadIr;
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
    /// Inheritance from an absent or foreign source took the catalog default.
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

    /// Describes the source-dialect displacement selected by this resolution.
    #[must_use]
    pub fn displacement_message(&self) -> Option<String> {
        self.displaced_source().map(|displaced| {
            format!(
                "source dialect {displaced} was displaced by target dialect {}; the source \
                 dialect identity is not preserved",
                self.target_id()
            )
        })
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
pub trait TargetResolution: resolution_sealed::Sealed {
    /// The native target identity, or `None` for the neutral document.
    fn export_target(&self) -> Option<&DialectId>;
}

impl TargetResolution for () {
    fn export_target(&self) -> Option<&DialectId> {
        None
    }
}

impl TargetResolution for ResolvedWrite<'_> {
    fn export_target(&self) -> Option<&DialectId> {
        Some(self.target_id())
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
pub struct Catalog(TargetCatalog);

impl Catalog {
    /// Builds a native target domain whose optional default indexes one of
    /// `targets`.
    #[must_use]
    pub const fn new(
        targets: &'static [cadmpeg_core::target::TargetDescriptor],
        default: Option<usize>,
    ) -> Self {
        Self(TargetCatalog::new(targets, default))
    }
}

impl TargetDomain for Catalog {
    type Resolved<'a> = ResolvedWrite<'a>;

    fn targets(&self, _format: FormatId) -> TargetCatalog {
        self.0
    }

    fn resolve<'a>(
        &self,
        ir: &'a CadIr,
        request: TargetRequest<'a>,
        _format: FormatId,
    ) -> Result<ResolvedWrite<'a>, CodecError> {
        resolve_write_request(ir, request, self.0)
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
    catalog: TargetCatalog,
) -> Result<ResolvedWrite<'a>, CodecError> {
    let format = catalog.namespace();
    let refuse = |kind| CodecError::UnsupportedTarget(Box::new(TargetRefusal::new(kind, catalog)));
    let source = source_identity(ir, format);
    let target = match request {
        TargetRequest::Explicit(requested) => {
            let (index, entry) = catalog.find(requested).ok_or_else(|| {
                refuse(TargetRefusalKind::UnknownExplicit {
                    requested: TargetToken::new(requested),
                })
            })?;
            ResolvedTarget::Explicit {
                index,
                entry,
                requested,
                source,
            }
        }
        TargetRequest::Inherit => match source {
            SourceIdentity::Other(source) => {
                let (index, entry) = catalog.default().ok_or_else(|| {
                    refuse(TargetRefusalKind::NoDefault {
                        source: source.clone(),
                    })
                })?;
                ResolvedTarget::Default {
                    index,
                    entry,
                    source,
                }
            }
            SourceIdentity::Unrecorded => return Err(refuse(TargetRefusalKind::UnrecordedSource)),
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
    })
}
#[cfg(test)]
mod tests;
