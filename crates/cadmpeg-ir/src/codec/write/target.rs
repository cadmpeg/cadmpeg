// SPDX-License-Identifier: Apache-2.0
//! Target domains, requests, and resolution against an encoder catalog.

use crate::codec::FormatId;
use crate::document::CadIr;
use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::target::{
    DefaultSource, TargetCatalog, TargetDescriptor, TargetRefusal, TargetRefusalKind, TargetToken,
};

use super::EncodeError;

/// What the caller asked an encoder to write, before resolution picks it.
///
/// Native requests resolve against the encoder catalog. An inherited source
/// outside the catalog selects its default and records the displacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetRequest<'a> {
    /// Select the source dialect when it belongs to the catalog.
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
    /// Inheritance selected the catalog default for the stated source relation.
    Default {
        /// Position of `entry` in the encoder's declared catalog.
        index: usize,
        /// The selected catalog row.
        entry: &'static TargetDescriptor,
        /// Why no source dialect could be inherited.
        source: DefaultSource,
    },
}

/// A native write resolved against the encoder catalog and source identity.
///
/// Only the internal `resolve_write_request` operation constructs this proof.
/// Codecs query it; they do not reconstruct the target/source relation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWrite<'a> {
    target: ResolvedTarget<'a>,
}

/// A selected catalog grammar for a synthesized write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthesisTarget<'a> {
    resolved: ResolvedWrite<'a>,
    index: usize,
}

impl<'a> SynthesisTarget<'a> {
    /// The selected grammar's position in the encoder catalog.
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    /// The source relation and actual target of this synthesized write.
    #[must_use]
    pub const fn resolution(&self) -> &ResolvedWrite<'a> {
        &self.resolved
    }
}

impl<'a> ResolvedWrite<'a> {
    /// The resolution, for callers that branch on how the target was chosen.
    #[must_use]
    pub const fn target(&self) -> &ResolvedTarget<'a> {
        &self.target
    }

    /// The resolved catalog row.
    #[must_use]
    pub const fn entry(&self) -> Option<&'static TargetDescriptor> {
        match &self.target {
            ResolvedTarget::Explicit { entry, .. }
            | ResolvedTarget::Inherited { entry, .. }
            | ResolvedTarget::Default { entry, .. } => Some(entry),
        }
    }

    /// Position of the resolved row in the catalog the encoder declared.
    ///
    /// Catalogs projected from a version enumeration in order let codecs index
    /// that enumeration directly.
    #[must_use]
    pub const fn index(&self) -> Option<usize> {
        match &self.target {
            ResolvedTarget::Explicit { index, .. }
            | ResolvedTarget::Inherited { index, .. }
            | ResolvedTarget::Default { index, .. } => Some(*index),
        }
    }

    /// The resolved output dialect.
    #[must_use]
    pub const fn target_id(&self) -> &DialectId {
        match &self.target {
            ResolvedTarget::Explicit { entry, .. }
            | ResolvedTarget::Inherited { entry, .. }
            | ResolvedTarget::Default { entry, .. } => &entry.id,
        }
    }

    /// Whether the resolved dialect is the recorded same-format source
    /// dialect.
    #[must_use]
    pub fn preserves_source(&self) -> bool {
        match &self.target {
            ResolvedTarget::Explicit { entry, source, .. } => source.recorded() == Some(&entry.id),
            ResolvedTarget::Inherited { .. } => true,
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
            ResolvedTarget::Default {
                source: DefaultSource::OffCatalogDialect(source),
                ..
            } => Some(source),
            ResolvedTarget::Inherited { .. } | ResolvedTarget::Default { .. } => None,
        }
    }

    /// Describes the source-dialect displacement selected by this resolution.
    #[must_use]
    pub fn displacement_message(&self) -> Option<String> {
        if matches!(
            self.target,
            ResolvedTarget::Default {
                source: DefaultSource::UnrecordedDialect,
                ..
            }
        ) {
            return Some(format!("the unrecorded same-format source dialect was displaced by target dialect {}; the source dialect identity is not preserved", self.target_id()));
        }
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
            ResolvedTarget::Inherited { .. } => true,
            ResolvedTarget::Default { source, .. } => matches!(
                source,
                DefaultSource::UnrecordedDialect | DefaultSource::OffCatalogDialect(_)
            ),
        }
    }

    /// Whether preservation was eligible before codec-specific retained-image
    /// checks.
    ///
    /// An explicit target with no contradictory recorded source dialect is
    /// eligible. A default selected for an unrecorded or off-catalog inherited
    /// source is a synthesis selection.
    #[must_use]
    pub fn source_preservation_eligible(&self) -> bool {
        !matches!(self.target, ResolvedTarget::Default { .. })
            && self.has_same_format_source()
            && self.displaced_source().is_none()
    }

    /// Returns the selected synthesis grammar.
    pub fn for_synthesis(&self) -> Result<SynthesisTarget<'a>, EncodeError> {
        let index = match &self.target {
            ResolvedTarget::Explicit { index, .. }
            | ResolvedTarget::Inherited { index, .. }
            | ResolvedTarget::Default { index, .. } => *index,
        };
        Ok(SynthesisTarget {
            resolved: self.clone(),
            index,
        })
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
    ) -> Result<Self::Resolved<'a>, EncodeError>;
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
    ) -> Result<(), EncodeError> {
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
    ) -> Result<ResolvedWrite<'a>, EncodeError> {
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
/// Native requests always name a catalog dialect.
/// A dialect-free neutral encoder handles its format identity locally instead
/// of adding an identity case to every native writer.
fn resolve_write_request<'a>(
    ir: &CadIr,
    request: TargetRequest<'a>,
    catalog: TargetCatalog,
) -> Result<ResolvedWrite<'a>, EncodeError> {
    let format = catalog.namespace();
    let refuse = |kind| EncodeError::UnsupportedTarget(Box::new(TargetRefusal::new(kind, catalog)));
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
            SourceIdentity::Unrecorded => {
                let source = DefaultSource::UnrecordedDialect;
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
            SourceIdentity::Recorded(source) => match catalog.find(source.as_str()) {
                Some((index, entry)) => ResolvedTarget::Inherited {
                    index,
                    entry,
                    source,
                },
                None => {
                    let source = DefaultSource::OffCatalogDialect(source);
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
            },
        },
    };
    Ok(ResolvedWrite { target })
}
#[cfg(test)]
mod tests;
