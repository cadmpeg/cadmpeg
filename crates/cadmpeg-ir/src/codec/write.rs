// SPDX-License-Identifier: Apache-2.0
//! Write target resolution and encoder interfaces.

use std::io::Write;

use crate::document::CadIr;
use crate::report::{CensusBasis, EntityCensus, ExportReport, FidelityResolution, WritePath};
use crate::source_fidelity::SourceFidelity;
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
pub(super) fn resolve_write_request(
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

/// How an encoder resolves caller target requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderTargetDomain {
    /// The neutral representation has no dialect catalog or target identity.
    DialectFree,
    /// A native format resolves every request against this complete catalog.
    Catalog(&'static [TargetDescriptor]),
}

impl EncoderTargetDomain {
    const fn targets(self) -> &'static [TargetDescriptor] {
        match self {
            Self::DialectFree => &[],
            Self::Catalog(targets) => targets,
        }
    }
}

/// A target request resolved by the sealed encoder boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedEncoderTarget {
    /// The dialect-free encoder received its only valid request, inherit.
    DialectFree,
    /// A native request resolved against the backend's declared catalog.
    Native(ResolvedWrite),
}

/// Implementation surface for one output format.
///
/// Backends declare one target domain and receive only a request already
/// resolved through that domain. Callers use the sealed [`Encoder`] wrapper.
pub trait EncoderBackend {
    /// Stable output format id.
    const FORMAT: &'static str;

    /// The target grammar this backend implements.
    const TARGET_DOMAIN: EncoderTargetDomain;

    /// Plans a write from the request resolved by [`Encoder::plan`].
    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedEncoderTarget,
    ) -> Result<ExportPlan, CodecError>;
}

mod encoder_sealed {
    pub trait Sealed {}
    impl<E: super::EncoderBackend> Sealed for E {}
}

/// Public planning interface for an output format.
pub trait Encoder: encoder_sealed::Sealed {
    /// Stable output format id.
    fn id(&self) -> &'static str;

    /// The static catalog of output flavors this encoder can produce.
    ///
    /// Whether a given input reaches one is resolution's answer, not the
    /// catalog's: a patch-only writer's row is reachable only from a retained
    /// source of that flavor, and `plan` refuses by name where it cannot
    /// deliver. Preservation of dialects outside the catalog is not listed
    /// here; [`TargetRequest::Inherit`] asks for it. Ids come from this
    /// encoder's own format namespace only.
    fn targets(&self) -> &'static [TargetDescriptor];

    /// Plans one export without writing to the destination.
    fn plan(
        &self,
        input: EncodeInput<'_>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan, CodecError>;
}

impl<E: EncoderBackend> Encoder for E {
    fn id(&self) -> &'static str {
        E::FORMAT
    }

    fn targets(&self) -> &'static [TargetDescriptor] {
        E::TARGET_DOMAIN.targets()
    }

    fn plan(
        &self,
        input: EncodeInput<'_>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan, CodecError> {
        let target = match E::TARGET_DOMAIN {
            EncoderTargetDomain::DialectFree => match request {
                TargetRequest::Inherit => ResolvedEncoderTarget::DialectFree,
                TargetRequest::Explicit(id) => {
                    return Err(TargetRefusal::unknown_explicit(E::FORMAT, id, &[]).into());
                }
            },
            EncoderTargetDomain::Catalog(targets) => ResolvedEncoderTarget::Native(
                resolve_write_request(input.ir, request, E::FORMAT, targets)?,
            ),
        };
        let expected_target = match &target {
            ResolvedEncoderTarget::DialectFree => None,
            ResolvedEncoderTarget::Native(write) => Some(write.dialect().clone()),
        };
        let plan = self.plan_resolved(input, target)?;
        if plan.report().format() != E::FORMAT {
            return Err(CodecError::ContractViolation {
                codec: E::FORMAT,
                operation: "plan",
                expected: E::FORMAT.to_owned(),
                reported: plan.report().format().to_owned(),
            });
        }
        if plan.report().target() != expected_target.as_ref() {
            return Err(CodecError::ContractViolation {
                codec: E::FORMAT,
                operation: "plan target",
                expected: expected_target
                    .as_ref()
                    .map_or_else(|| "none".to_owned(), ToString::to_string),
                reported: plan
                    .report()
                    .target()
                    .map_or_else(|| "none".to_owned(), ToString::to_string),
            });
        }
        Ok(plan)
    }
}

/// Borrowed inputs used to plan an export.
#[derive(Debug, Clone, Copy)]
pub struct EncodeInput<'a> {
    /// Neutral document to export.
    pub ir: &'a CadIr,
    /// Decode-time fidelity state, when available.
    pub fidelity: Option<&'a SourceFidelity>,
}

impl<'a> EncodeInput<'a> {
    /// Borrows a document and its decode-time fidelity for one export.
    #[must_use]
    pub const fn new(ir: &'a CadIr, fidelity: Option<&'a SourceFidelity>) -> Self {
        Self { ir, fidelity }
    }
}

/// A fully reported export awaiting its destination write.
///
/// The plan owns the complete payload. Atomic file staging belongs to the
/// artifact store; [`Self::write_to`] also supports non-file sinks.
pub struct ExportPlan {
    report: ExportReport,
    bytes: Vec<u8>,
}

impl ExportPlan {
    /// Creates a plan whose bytes have already been materialized.
    ///
    /// The plan reports exactly the report it is given, including fidelity.
    pub fn buffered(report: ExportReport, bytes: Vec<u8>) -> Self {
        Self { report, bytes }
    }

    /// Returns the complete plan-time export report.
    pub fn report(&self) -> &ExportReport {
        &self.report
    }

    /// Writes the planned payload and returns the unchanged plan-time report.
    pub fn write_to(self, writer: &mut dyn Write) -> Result<ExportReport, CodecError> {
        writer.write_all(&self.bytes)?;
        Ok(self.report)
    }
}

/// Encoder for canonical versioned CADIR JSON.
#[derive(Debug, Clone, Copy, Default)]
pub struct CadirEncoder;

impl EncoderBackend for CadirEncoder {
    const FORMAT: &'static str = "cadir";

    /// Empty. CADIR is the neutral document, not a native format: its version
    /// is data about cadmpeg, never a dialect, and `ExportReport::target` is
    /// `None` on every CADIR write. An encoder with no catalog takes
    /// [`TargetRequest::Inherit`] only.
    const TARGET_DOMAIN: EncoderTargetDomain = EncoderTargetDomain::DialectFree;

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedEncoderTarget,
    ) -> Result<ExportPlan, CodecError> {
        let ResolvedEncoderTarget::DialectFree = target else {
            unreachable!("a dialect-free encoder receives only dialect-free resolutions")
        };
        let report = ExportReport::cadir(
            EntityCensus {
                basis: CensusBasis::IrArenas,
                counts: input.ir.census(),
            },
            if input.fidelity.is_some() {
                FidelityResolution::NotConsumed
            } else {
                FidelityResolution::NotProvided
            },
            // CADIR is the neutral document itself: there is no container to
            // replay or patch, so this encoder has one path and states it.
            WritePath::Synthesized,
            Vec::new(),
            Vec::new(),
        );
        let mut bytes = serde_json::to_vec_pretty(input.ir)
            .map_err(|error| CodecError::Malformed(error.to_string()))?;
        bytes.push(b'\n');
        Ok(ExportPlan::buffered(report, bytes))
    }
}
