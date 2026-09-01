// SPDX-License-Identifier: Apache-2.0
//! Write target resolution against an encoder catalog.


use crate::document::CadIr;
use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::target::{
    default_target, find_target, DefaultSource, TargetDescriptor, TargetRefusal, TargetRefusalKind,
    TargetToken,
};
use cadmpeg_core::CodecError;

/// What the caller asked an encoder to write, before resolution picks it.
///
/// Synthesis and preservation are different capabilities. Synthesis is static
/// and input-independent: [`Encoder::targets`] is the whole catalog.
/// Preservation is input-conditioned — replaying a retained baseline
/// reproduces dialects no encoder could synthesize for arbitrary input — so it
/// is asked for by [`TargetRequest::Inherit`], never by a catalog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetRequest<'a> {
    /// Preserve the source's dialect. The same-format default.
    Inherit,
    /// A synthesis target from [`Encoder::targets`]: an explicit target flag,
    /// or the catalog default for a cross-format conversion.
    Explicit(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolvedTarget {
    Explicit {
        entry: &'static TargetDescriptor,
        requested: TargetToken,
    },
    Inherited {
        entry: &'static TargetDescriptor,
    },
    Default {
        entry: &'static TargetDescriptor,
    },
    Preserved,
}

/// A native write resolved against the encoder catalog and source identity.
///
/// Only the internal `resolve_write_request` operation constructs this proof. Its queries keep the
/// catalog target, preservation eligibility, and displaced source consistent;
/// codecs do not reconstruct those relations from public fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWrite {
    target: ResolvedTarget,
    source: SourceIdentity,
    available: &'static [TargetDescriptor],
}

impl ResolvedWrite {
    fn explicit(
        entry: &'static TargetDescriptor,
        source: SourceIdentity,
        requested: TargetToken,
        available: &'static [TargetDescriptor],
    ) -> Self {
        Self {
            target: ResolvedTarget::Explicit { entry, requested },
            source,
            available,
        }
    }

    fn inherited(
        entry: &'static TargetDescriptor,
        source: SourceIdentity,
        available: &'static [TargetDescriptor],
    ) -> Self {
        Self {
            target: ResolvedTarget::Inherited { entry },
            source,
            available,
        }
    }

    fn default(
        entry: &'static TargetDescriptor,
        source: SourceIdentity,
        available: &'static [TargetDescriptor],
    ) -> Self {
        Self {
            target: ResolvedTarget::Default { entry },
            source,
            available,
        }
    }

    fn preserved(source: SourceIdentity, available: &'static [TargetDescriptor]) -> Self {
        ResolvedWrite {
            target: ResolvedTarget::Preserved,
            source,
            available,
        }
    }

    /// Returns the resolved catalog row, or `None` when inheritance requires
    /// preservation of an off-catalog source dialect.
    #[must_use]
    pub const fn catalog_entry(&self) -> Option<&'static TargetDescriptor> {
        match &self.target {
            ResolvedTarget::Explicit { entry, .. }
            | ResolvedTarget::Inherited { entry, .. }
            | ResolvedTarget::Default { entry, .. } => Some(entry),
            ResolvedTarget::Preserved => None,
        }
    }

    /// Returns the resolved output dialect.
    #[must_use]
    pub const fn dialect(&self) -> &DialectId {
        match &self.target {
            ResolvedTarget::Explicit { entry, .. }
            | ResolvedTarget::Inherited { entry, .. }
            | ResolvedTarget::Default { entry, .. } => &entry.id,
            ResolvedTarget::Preserved => self
                .source
                .recorded()
                .expect("preserved writes carry a recorded same-format source"),
        }
    }

    /// Whether the resolved dialect is the recorded same-format source
    /// dialect.
    #[must_use]
    pub fn preserves_source(&self) -> bool {
        match &self.target {
            ResolvedTarget::Explicit { entry, .. } => self.source.recorded() == Some(&entry.id),
            ResolvedTarget::Inherited { .. } => true,
            ResolvedTarget::Default { .. } => false,
            ResolvedTarget::Preserved => true,
        }
    }

    /// The recorded same-format source dialect replaced by the resolved
    /// target, if any.
    #[must_use]
    pub fn displaced_source(&self) -> Option<&DialectId> {
        match &self.target {
            ResolvedTarget::Explicit { entry, .. } => {
                self.source.recorded().filter(|source| *source != &entry.id)
            }
            ResolvedTarget::Inherited { .. }
            | ResolvedTarget::Default { .. }
            | ResolvedTarget::Preserved => None,
        }
    }

    /// Describes the source-dialect displacement selected by this resolution.
    #[must_use]
    pub fn displacement_message(&self) -> Option<String> {
        self.displaced_source().map(|displaced| {
            format!(
                "source dialect {displaced} was displaced by target dialect {}; the source \
                 dialect identity is not preserved",
                self.dialect()
            )
        })
    }

    /// Whether the document records source metadata for this encoder format.
    ///
    /// This includes a same-format source whose dialect was not classified.
    #[must_use]
    pub const fn has_same_format_source(&self) -> bool {
        matches!(
            self.source,
            SourceIdentity::Unrecorded | SourceIdentity::Recorded(_)
        )
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
        let refusal = match &self.target {
            ResolvedTarget::Explicit {
                entry, requested, ..
            } => TargetRefusal::new(
                TargetRefusalKind::ExplicitUnavailable {
                    target: entry.id.clone(),
                    requested: requested.clone(),
                    reason,
                },
                self.available,
            ),
            ResolvedTarget::Inherited { .. } | ResolvedTarget::Preserved => TargetRefusal::new(
                TargetRefusalKind::InheritedUnavailable {
                    source: self
                        .source
                        .recorded()
                        .expect("inherited writes carry a recorded same-format source")
                        .clone(),
                    reason,
                },
                self.available,
            ),
            ResolvedTarget::Default { entry } => TargetRefusal::new(
                TargetRefusalKind::DefaultUnavailable {
                    target: entry.id.clone(),
                    source: self
                        .source
                        .default_source()
                        .expect("default writes carry an absent or foreign source")
                        .clone(),
                    reason,
                },
                self.available,
            ),
        };
        CodecError::UnsupportedTarget(Box::new(refusal))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SourceIdentity {
    Other(DefaultSource),
    Unrecorded,
    Recorded(DialectId),
}

impl SourceIdentity {
    const fn recorded(&self) -> Option<&DialectId> {
        match self {
            Self::Recorded(dialect) => Some(dialect),
            Self::Other(_) | Self::Unrecorded => None,
        }
    }

    const fn default_source(&self) -> Option<&DefaultSource> {
        match self {
            Self::Other(source) => Some(source),
            Self::Unrecorded | Self::Recorded(_) => None,
        }
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
pub(in crate::codec) fn resolve_write_request(
    ir: &CadIr,
    request: TargetRequest<'_>,
    format: &str,
    targets: &'static [TargetDescriptor],
) -> Result<ResolvedWrite, CodecError> {
    let source = source_identity(ir, format);
    match request {
        TargetRequest::Explicit(id) => Ok(ResolvedWrite::explicit(
            find_target(targets, id).ok_or_else(|| {
                CodecError::from(TargetRefusal::unknown_explicit(format, id, targets))
            })?,
            source,
            TargetToken::new(id),
            targets,
        )),
        TargetRequest::Inherit => match source {
            SourceIdentity::Other(_) => Ok(ResolvedWrite::default(
                default_target(targets).ok_or_else(|| {
                    CodecError::UnsupportedTarget(Box::new(TargetRefusal::new(
                        TargetRefusalKind::NoDefault {
                            format: format.to_owned(),
                            source: source
                                .default_source()
                                .expect("the matched source identity is absent or foreign")
                                .clone(),
                        },
                        targets,
                    )))
                })?,
                source,
                targets,
            )),
            SourceIdentity::Unrecorded => {
                Err(CodecError::UnsupportedTarget(Box::new(TargetRefusal::new(
                    TargetRefusalKind::UnrecordedSource {
                        format: format.to_owned(),
                    },
                    targets,
                ))))
            }
            SourceIdentity::Recorded(ref dialect) => match find_target(targets, dialect.as_str()) {
                Some(entry) => Ok(ResolvedWrite::inherited(entry, source, targets)),
                None => Ok(ResolvedWrite::preserved(source, targets)),
            },
        },
    }
}
