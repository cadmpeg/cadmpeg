// SPDX-License-Identifier: Apache-2.0
//! Write target resolution and encoder interfaces.

use std::collections::BTreeSet;
use std::io::Write;

use crate::document::CadIr;
use crate::report::{CensusBasis, EntityCensus, ExportReport, FidelityResolution, WritePath};
use crate::source_fidelity::SourceFidelity;
use cadmpeg_core::dialect::DialectId;
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

/// One dialect that a caller can request from an encoder.
///
/// The catalog states names, aliases, and the cross-format default. It does not
/// guarantee that every input can reach every row. [`Encoder::plan`] applies
/// the resolved request to the input and refuses a row that the writer cannot
/// deliver, such as a patch-only target without a matching retained source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDescriptor {
    /// Registry dialect id, e.g. `step:ap242-e3`.
    pub id: DialectId,
    /// Human-readable name, e.g. `STEP AP242 edition 3`.
    pub label: &'static str,
    /// Short spellings accepted for `id`, e.g. `["6"]` for `rhino:archive-60`.
    pub aliases: &'static [&'static str],
    /// True on at most one entry: the cross-format conversion default.
    ///
    /// A catalog may have no default when the encoder cannot synthesize a
    /// document from a source of another format.
    pub default: bool,
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

/// The typed write refusal, naming what was asked for and the whole catalog.
#[must_use]
pub fn unsupported_target(
    format: &str,
    requested: &str,
    reason: &str,
    targets: &[TargetDescriptor],
) -> CodecError {
    refusal(
        format,
        Some(cadmpeg_core::TargetToken::new(requested)),
        reason,
        targets,
    )
}

/// Why every encoder refuses `Inherit` over a same-format source that records
/// no dialect.
///
/// Preservation needs something to preserve. With no recorded dialect the
/// identity default cannot know what the file is, so writing any catalog row
/// would be choosing an identity the source never declared. An explicit target
/// is the escape.
pub const UNRECORDED_SOURCE_DIALECT_REASON: &str =
    "the source records no dialect, so there is nothing to preserve; name a target to write one";

/// The typed write refusal for `Inherit` over a same-format source that records
/// no dialect.
///
/// Distinct from [`unsupported_target`] in that no dialect id was asked for and
/// the source declares none, so the refusal names no id at all rather than
/// putting a format id in a dialect-id field.
#[must_use]
fn unrecorded_source_dialect(format: &str, targets: &[TargetDescriptor]) -> CodecError {
    refusal(format, None, UNRECORDED_SOURCE_DIALECT_REASON, targets)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedTarget<'a> {
    Catalog {
        entry: &'static TargetDescriptor,
        source: Option<&'a DialectId>,
    },
    Preserved(&'a DialectId),
}

/// A native write resolved against the encoder catalog and source identity.
///
/// Only [`resolve_write_request`] constructs this proof. Its queries keep the
/// catalog target, preservation eligibility, and displaced source consistent;
/// codecs do not reconstruct those relations from public fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWrite<'a> {
    target: ResolvedTarget<'a>,
}

impl<'a> ResolvedWrite<'a> {
    fn catalog(entry: &'static TargetDescriptor, source: SourceIdentity<'a>) -> Self {
        Self {
            target: ResolvedTarget::Catalog {
                entry,
                source: source.recorded(),
            },
        }
    }

    const fn preserved(dialect: &'a DialectId) -> Self {
        ResolvedWrite {
            target: ResolvedTarget::Preserved(dialect),
        }
    }

    /// Returns the resolved catalog row, or `None` when inheritance requires
    /// preservation of an off-catalog source dialect.
    #[must_use]
    pub const fn catalog_entry(&self) -> Option<&'static TargetDescriptor> {
        match self.target {
            ResolvedTarget::Catalog { entry, .. } => Some(entry),
            ResolvedTarget::Preserved(_) => None,
        }
    }

    /// Returns the resolved output dialect.
    #[must_use]
    pub const fn dialect(&self) -> &DialectId {
        match self {
            Self {
                target: ResolvedTarget::Catalog { entry, .. },
                ..
            } => &entry.id,
            Self {
                target: ResolvedTarget::Preserved(dialect),
                ..
            } => dialect,
        }
    }

    /// Whether the resolved dialect is the recorded same-format source
    /// dialect.
    #[must_use]
    pub fn preserves_source(&self) -> bool {
        match self.target {
            ResolvedTarget::Catalog { entry, source } => {
                source.is_some_and(|source| source == &entry.id)
            }
            ResolvedTarget::Preserved(_) => true,
        }
    }

    /// The recorded same-format source dialect replaced by the resolved
    /// target, if any.
    #[must_use]
    pub fn displaced_source(&self) -> Option<&DialectId> {
        match self.target {
            ResolvedTarget::Catalog { entry, source } => {
                source.filter(|source| *source != &entry.id)
            }
            ResolvedTarget::Preserved(_) => None,
        }
    }
}

#[derive(Clone, Copy)]
enum SourceIdentity<'a> {
    Other,
    Unrecorded,
    Recorded(&'a cadmpeg_core::dialect::DialectId),
}

impl<'a> SourceIdentity<'a> {
    const fn recorded(self) -> Option<&'a DialectId> {
        match self {
            Self::Recorded(dialect) => Some(dialect),
            Self::Other | Self::Unrecorded => None,
        }
    }
}

fn source_identity<'a>(ir: &'a CadIr, format: &str) -> SourceIdentity<'a> {
    let Some(source) = ir
        .source
        .as_ref()
        .filter(|source| source.format() == format)
    else {
        return SourceIdentity::Other;
    };
    match source.dialect() {
        Some(matched) => SourceIdentity::Recorded(matched.dialect()),
        None => SourceIdentity::Unrecorded,
    }
}

/// Resolve a native target and inheritance once, before codec-specific delivery.
///
/// Native requests always name a catalog or preserved off-catalog dialect.
/// A dialect-free neutral encoder handles its format identity locally instead
/// of adding an identity case to every native writer.
pub fn resolve_write_request<'a>(
    ir: &'a CadIr,
    request: TargetRequest<'_>,
    format: &str,
    targets: &'static [TargetDescriptor],
) -> Result<ResolvedWrite<'a>, CodecError> {
    let source = source_identity(ir, format);
    match request {
        TargetRequest::Explicit(id) => Ok(ResolvedWrite::catalog(
            find_target(targets, id).ok_or_else(|| {
                unsupported_target(
                    format,
                    id,
                    "not a target this encoder can synthesize",
                    targets,
                )
            })?,
            source,
        )),
        TargetRequest::Inherit => match source {
            SourceIdentity::Other => Ok(ResolvedWrite::catalog(
                default_target(targets).ok_or_else(|| {
                    refusal(
                        format,
                        None,
                        "there is nothing to inherit and this encoder has no synthesis catalog",
                        targets,
                    )
                })?,
                source,
            )),
            SourceIdentity::Unrecorded => Err(unrecorded_source_dialect(format, targets)),
            SourceIdentity::Recorded(dialect) => match find_target(targets, dialect.as_str()) {
                Some(entry) => Ok(ResolvedWrite::catalog(entry, source)),
                None => Ok(ResolvedWrite::preserved(dialect)),
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

fn refusal(
    format: &str,
    requested: Option<cadmpeg_core::TargetToken>,
    reason: &str,
    targets: &[TargetDescriptor],
) -> CodecError {
    let available = targets
        .iter()
        .map(|target| target.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    CodecError::UnsupportedTarget {
        format: format.to_owned(),
        requested,
        reason: reason.to_owned(),
        available: if available.is_empty() {
            "none".to_owned()
        } else {
            available
        },
    }
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
    fn plan<'a>(
        &self,
        input: EncodeInput<'a>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan<'a>, CodecError>;
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

type DeferredExport<'a> = Box<dyn FnOnce(&mut dyn Write) -> Result<(), CodecError> + 'a>;

enum ExportPayload<'a> {
    Buffered(Vec<u8>),
    Deferred(DeferredExport<'a>),
}

/// A fully reported export awaiting its atomic destination write.
pub struct ExportPlan<'a> {
    report: ExportReport,
    payload: ExportPayload<'a>,
}

impl<'a> ExportPlan<'a> {
    /// Creates a plan whose bytes have already been materialized.
    ///
    /// The plan reports exactly the report it is given, including fidelity.
    pub fn buffered(report: ExportReport, bytes: Vec<u8>) -> Self {
        Self {
            report,
            payload: ExportPayload::Buffered(bytes),
        }
    }

    /// Creates a plan that writes through a deferred, report-invariant operation.
    ///
    /// The report is reported verbatim.
    pub fn deferred(
        report: ExportReport,
        write: impl FnOnce(&mut dyn Write) -> Result<(), CodecError> + 'a,
    ) -> Self {
        Self {
            report,
            payload: ExportPayload::Deferred(Box::new(write)),
        }
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
        match self.payload {
            ExportPayload::Buffered(bytes) => writer.write_all(&bytes)?,
            ExportPayload::Deferred(write) => write(writer)?,
        }
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

    fn plan<'a>(
        &self,
        input: EncodeInput<'a>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan<'a>, CodecError> {
        match request {
            TargetRequest::Inherit => {}
            TargetRequest::Explicit(id) => {
                return Err(unsupported_target(
                    self.id(),
                    id,
                    "not a target this encoder can synthesize",
                    self.targets(),
                ));
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
        Ok(ExportPlan::deferred(report, move |writer| {
            serde_json::to_writer_pretty(&mut *writer, input.ir)
                .map_err(|error| CodecError::Malformed(error.to_string()))?;
            writer.write_all(b"\n")?;
            Ok(())
        }))
    }
}
