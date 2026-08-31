// SPDX-License-Identifier: Apache-2.0
//! Write target resolution and encoder interfaces.

use std::collections::BTreeSet;
use std::io::Write;

use crate::document::CadIr;
use crate::report::{CensusBasis, EntityCensus, ExportReport, FidelityResolution, WritePath};
use crate::source_fidelity::SourceFidelity;
use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::target::{DefaultSource, TargetDescriptor, TargetRefusal, TargetToken};
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

/// Panics when a static encoder target catalog violates its uniqueness rules.
pub fn assert_valid_target_catalog(targets: &[TargetDescriptor]) {
    let defaults = targets.iter().filter(|target| target.default).count();
    assert!(
        defaults <= 1,
        "target catalog invariant failed: at most one entry may be the default"
    );

    let mut ids = BTreeSet::new();
    for target in targets {
        assert!(
            ids.insert(target.id.as_str()),
            "target catalog invariant failed: duplicate id {:?}",
            target.id
        );
    }

    let mut aliases = BTreeSet::new();
    for target in targets {
        for alias in target.aliases {
            assert!(
                !ids.contains(alias),
                "target catalog invariant failed: alias {alias:?} is also an id"
            );
            assert!(
                aliases.insert(*alias),
                "target catalog invariant failed: duplicate alias {alias:?}"
            );
        }
    }
}

/// The catalog entry `id` names, by full id, format-local id, or alias.
///
/// A format-local id is the part after the first colon. The caller has already
/// selected an encoder, so `archive-60` is unambiguous within the Rhino
/// catalog and lets `--to rhino:archive-60` pass its right half unchanged.
#[must_use]
pub fn find_target<'a>(targets: &'a [TargetDescriptor], id: &str) -> Option<&'a TargetDescriptor> {
    targets.iter().find(|target| {
        target.id.as_str() == id
            || target
                .id
                .as_str()
                .split_once(':')
                .is_some_and(|(_, local)| local == id)
            || target.aliases.contains(&id)
    })
}

/// The catalog's cross-format default, or `None` when none is declared.
#[must_use]
pub fn default_target(targets: &'static [TargetDescriptor]) -> Option<&'static TargetDescriptor> {
    targets.iter().find(|target| target.default)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolvedTarget {
    Explicit {
        entry: &'static TargetDescriptor,
        source: Option<DialectId>,
        requested: TargetToken,
    },
    Inherited {
        entry: &'static TargetDescriptor,
        source: DialectId,
    },
    Default {
        entry: &'static TargetDescriptor,
        source: DefaultSource,
    },
    Preserved(DialectId),
}

/// A native write resolved against the encoder catalog and source identity.
///
/// Only [`resolve_write_request`] constructs this proof. Its queries keep the
/// catalog target, preservation eligibility, and displaced source consistent;
/// codecs do not reconstruct those relations from public fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWrite {
    target: ResolvedTarget,
    available: &'static [TargetDescriptor],
}

impl ResolvedWrite {
    fn explicit(
        entry: &'static TargetDescriptor,
        source: Option<DialectId>,
        requested: TargetToken,
        available: &'static [TargetDescriptor],
    ) -> Self {
        Self {
            target: ResolvedTarget::Explicit {
                entry,
                source,
                requested,
            },
            available,
        }
    }

    fn inherited(
        entry: &'static TargetDescriptor,
        source: DialectId,
        available: &'static [TargetDescriptor],
    ) -> Self {
        Self {
            target: ResolvedTarget::Inherited { entry, source },
            available,
        }
    }

    fn default(
        entry: &'static TargetDescriptor,
        source: DefaultSource,
        available: &'static [TargetDescriptor],
    ) -> Self {
        Self {
            target: ResolvedTarget::Default { entry, source },
            available,
        }
    }

    fn preserved(dialect: DialectId, available: &'static [TargetDescriptor]) -> Self {
        ResolvedWrite {
            target: ResolvedTarget::Preserved(dialect),
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
            ResolvedTarget::Preserved(_) => None,
        }
    }

    /// Returns the resolved output dialect.
    #[must_use]
    pub const fn dialect(&self) -> &DialectId {
        match &self.target {
            ResolvedTarget::Explicit { entry, .. }
            | ResolvedTarget::Inherited { entry, .. }
            | ResolvedTarget::Default { entry, .. } => &entry.id,
            ResolvedTarget::Preserved(dialect) => dialect,
        }
    }

    /// Whether the resolved dialect is the recorded same-format source
    /// dialect.
    #[must_use]
    pub fn preserves_source(&self) -> bool {
        match &self.target {
            ResolvedTarget::Explicit { entry, source, .. } => {
                source.as_ref().is_some_and(|source| source == &entry.id)
            }
            ResolvedTarget::Inherited { .. } => true,
            ResolvedTarget::Default { .. } => false,
            ResolvedTarget::Preserved(_) => true,
        }
    }

    /// The recorded same-format source dialect replaced by the resolved
    /// target, if any.
    #[must_use]
    pub fn displaced_source(&self) -> Option<&DialectId> {
        match &self.target {
            ResolvedTarget::Explicit { entry, source, .. } => {
                source.as_ref().filter(|source| *source != &entry.id)
            }
            ResolvedTarget::Inherited { .. }
            | ResolvedTarget::Default { .. }
            | ResolvedTarget::Preserved(_) => None,
        }
    }

    /// Builds a typed refusal when codec-specific delivery cannot realize the
    /// already-resolved request.
    #[must_use]
    pub fn unavailable(&self, reason: impl Into<String>) -> CodecError {
        let reason = reason.into();
        let refusal = match &self.target {
            ResolvedTarget::Explicit {
                entry, requested, ..
            } => TargetRefusal::ExplicitUnavailable {
                target: entry.id.clone(),
                requested: requested.clone(),
                reason,
                available: self.available,
            },
            ResolvedTarget::Inherited { source, .. } | ResolvedTarget::Preserved(source) => {
                TargetRefusal::InheritedUnavailable {
                    source: source.clone(),
                    reason,
                    available: self.available,
                }
            }
            ResolvedTarget::Default { entry, source } => TargetRefusal::DefaultUnavailable {
                target: entry.id.clone(),
                source: source.clone(),
                reason,
                available: self.available,
            },
        };
        CodecError::UnsupportedTarget(Box::new(refusal))
    }
}

#[derive(Clone)]
enum SourceIdentity {
    Other(DefaultSource),
    Unrecorded,
    Recorded(DialectId),
}

impl SourceIdentity {
    fn recorded(&self) -> Option<DialectId> {
        match self {
            Self::Recorded(dialect) => Some(dialect.clone()),
            Self::Other(_) | Self::Unrecorded => None,
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
pub fn resolve_write_request(
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
            source.recorded(),
            TargetToken::new(id),
            targets,
        )),
        TargetRequest::Inherit => match source {
            SourceIdentity::Other(source) => Ok(ResolvedWrite::default(
                default_target(targets).ok_or_else(|| {
                    CodecError::UnsupportedTarget(Box::new(TargetRefusal::NoDefault {
                        format: format.to_owned(),
                        source: source.clone(),
                        available: targets,
                    }))
                })?,
                source,
                targets,
            )),
            SourceIdentity::Unrecorded => Err(CodecError::UnsupportedTarget(Box::new(
                TargetRefusal::UnrecordedSource {
                    format: format.to_owned(),
                    available: targets,
                },
            ))),
            SourceIdentity::Recorded(dialect) => match find_target(targets, dialect.as_str()) {
                Some(entry) => Ok(ResolvedWrite::inherited(entry, dialect, targets)),
                None => Ok(ResolvedWrite::preserved(dialect, targets)),
            },
        },
    }
}

/// State that a write displaced the source dialect with another target.
#[must_use]
pub fn source_dialect_displaced_message(
    displaced: &cadmpeg_core::dialect::DialectId,
    target: &cadmpeg_core::dialect::DialectId,
) -> String {
    format!(
        "source dialect {displaced} was displaced by target dialect {target}; the source dialect identity is not preserved"
    )
}

/// A native-format writer.
pub trait Encoder {
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

/// A fully reported export awaiting its atomic destination write.
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

    /// Returns how source fidelity was resolved while planning.
    pub fn fidelity_resolution(&self) -> &FidelityResolution {
        &self.report.fidelity
    }

    /// Returns the write path the encoder took to produce this plan's payload.
    pub fn write_path(&self) -> WritePath {
        self.report.write_path
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

impl Encoder for CadirEncoder {
    fn id(&self) -> &'static str {
        "cadir"
    }

    /// Empty. CADIR is the neutral document, not a native format: its version
    /// is data about cadmpeg, never a dialect, and `ExportReport::target` is
    /// `None` on every CADIR write. An encoder with no catalog takes
    /// [`TargetRequest::Inherit`] only.
    fn targets(&self) -> &'static [TargetDescriptor] {
        &[]
    }

    fn plan(
        &self,
        input: EncodeInput<'_>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan, CodecError> {
        match request {
            TargetRequest::Inherit => {}
            TargetRequest::Explicit(id) => {
                return Err(TargetRefusal::unknown_explicit(self.id(), id, self.targets()).into());
            }
        }
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
