// SPDX-License-Identifier: Apache-2.0
//! Write target resolution and encoder interfaces.

use std::collections::BTreeSet;
use std::io::Write;

use crate::document::CadIr;
use crate::report::{CensusBasis, EntityCensus, ExportReport, FidelityResolution, WritePath};
use crate::source_fidelity::SourceFidelity;
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

/// One dialect an encoder can synthesize for any input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetDescriptor {
    /// Registry dialect id, e.g. `step:ap242-e3`.
    pub id: &'static str,
    /// Human-readable name, e.g. `STEP AP242 edition 3`.
    pub label: &'static str,
    /// Short spellings accepted for `id`, e.g. `["6"]` for `rhino:archive-60`.
    pub aliases: &'static [&'static str],
    /// True on exactly one entry: the cross-format conversion default.
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
            ids.insert(target.id),
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
        target.id == id
            || target
                .id
                .split_once(':')
                .is_some_and(|(_, local)| local == id)
            || target.aliases.contains(&id)
    })
}

/// The catalog's default target, or `None` for an encoder with no synthesis
/// catalog.
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

/// A write request resolved against the encoder catalog and source identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteRequest<'a> {
    /// `Inherit` on a dialect-free encoder resolves to format identity.
    Identity,
    /// The request names a catalog row.
    Catalog {
        /// The canonical catalog entry.
        entry: &'static TargetDescriptor,
        /// The same-format source dialect displaced by this target, if any.
        displaced: Option<cadmpeg_core::dialect::DialectId>,
        /// Whether the resolved target preserves the same-format source dialect.
        preserve: bool,
    },
    /// `Inherit` names a same-format source dialect outside the catalog.
    OffCatalog {
        /// The source dialect that must be preserved or refused by name.
        dialect: &'a cadmpeg_core::dialect::DialectId,
    },
}

impl WriteRequest<'_> {
    /// Returns the canonical dialect id, or `None` for format identity.
    #[must_use]
    pub fn dialect_id(&self) -> Option<cadmpeg_core::dialect::DialectId> {
        match self {
            Self::Identity => None,
            Self::Catalog { entry, .. } => Some(cadmpeg_core::dialect::DialectId::pinned(entry.id)),
            Self::OffCatalog { dialect } => Some((*dialect).clone()),
        }
    }
}

/// The source dialect when the document belongs to `format` and records one.
#[must_use]
pub fn same_format_source_dialect<'a>(
    ir: &'a CadIr,
    format: &str,
) -> Option<&'a cadmpeg_core::dialect::DialectMatch> {
    ir.source
        .as_ref()
        .filter(|source| source.format == format)
        .and_then(|source| source.dialect.as_ref())
}

/// Resolve target syntax and inheritance once, before codec-specific delivery.
pub fn resolve_write_request<'a>(
    ir: &'a CadIr,
    request: TargetRequest<'_>,
    format: &str,
    targets: &'static [TargetDescriptor],
) -> Result<WriteRequest<'a>, CodecError> {
    if targets.is_empty() && request == TargetRequest::Inherit {
        return Ok(WriteRequest::Identity);
    }
    let entry = match request {
        TargetRequest::Explicit(id) => find_target(targets, id).ok_or_else(|| {
            unsupported_target(
                format,
                id,
                "not a target this encoder can synthesize",
                targets,
            )
        })?,
        TargetRequest::Inherit => {
            match ir.source.as_ref().filter(|source| source.format == format) {
                None => default_target(targets).ok_or_else(|| {
                    refusal(
                        format,
                        None,
                        "there is nothing to inherit and this encoder has no synthesis catalog",
                        targets,
                    )
                })?,
                Some(_) => {
                    let dialect = same_format_source_dialect(ir, format)
                        .map(cadmpeg_core::dialect::DialectMatch::dialect)
                        .ok_or_else(|| unrecorded_source_dialect(format, targets))?;
                    let Some(entry) = find_target(targets, dialect.as_str()) else {
                        return Ok(WriteRequest::OffCatalog { dialect });
                    };
                    entry
                }
            }
        }
    };
    let displaced = same_format_source_dialect(ir, format)
        .map(cadmpeg_core::dialect::DialectMatch::dialect)
        .filter(|dialect| dialect.as_str() != entry.id)
        .cloned();
    Ok(WriteRequest::Catalog {
        entry,
        preserve: same_format_source_dialect(ir, format)
            .is_some_and(|source| source.dialect().as_str() == entry.id),
        displaced,
    })
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
        .map(|target| target.id)
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
        match resolve_write_request(input.ir, request, self.id(), self.targets())? {
            WriteRequest::Identity => {}
            WriteRequest::Catalog { .. } | WriteRequest::OffCatalog { .. } => {
                unreachable!("an empty target catalog resolves only to identity")
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
