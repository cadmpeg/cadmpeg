// SPDX-License-Identifier: Apache-2.0
//! Interfaces for detecting, inspecting, decoding, and encoding CAD formats.
//!
//! [`Codec`] is object-safe for runtime codec registries. Detection consumes a
//! byte prefix, inspection summarizes a seekable container, and decoding
//! produces a finalized [`CadIr`] plus a [`DecodeReport`].
//!
//! A codec implements only the required [`Codec`] methods. The public
//! [`Codec::inspect`] and [`Codec::decode`] entry points are the
//! single enforcement point for root-input limits and session finalize checks;
//! they live on the sealed [`Codec`] trait, blanket-implemented for every
//! `Codec`, so a codec cannot override an entry point and drop the
//! enforcement.

use std::collections::BTreeSet;
use std::fmt;
use std::io::Write;
use std::ops::{Deref, DerefMut};

use crate::document::CadIr;
use crate::report::DecodeReport;
use crate::report::StrictConsequence;
use crate::report::{CensusBasis, EntityCensus, ExportReport, FidelityResolution, WritePath};
use crate::source_fidelity::SourceFidelity;
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodeMode, DecodePolicy, InspectOptions, View,
};
use cadmpeg_core::{CodecError, ContainerSummary, ReadSeek};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How confident a codec is that it can handle a given byte prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Definitely not this format.
    No,
    /// Weak signal, such as a generic container signature.
    Low,
    /// Plausible but not conclusive.
    Medium,
    /// Strong, format-specific signal.
    High,
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::No => "no",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        })
    }
}

/// Options controlling source decoding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DecodeOptions {
    /// Stop after the container layer; do not attempt entity decode.
    pub container_only: bool,
    /// Resource limits and failure-handling mode governing the decode.
    #[serde(default)]
    pub policy: DecodePolicy,
}

/// A decoded document plus its loss report.
///
/// Construct only through [`DecodeResult::new`], which finalizes the IR and
/// source fidelity. `#[non_exhaustive]` blocks external struct literals so
/// callers cannot skip finalization. Read through [`Self::ir`], [`Self::report`],
/// and [`Self::source_fidelity`]. Consume with [`Self::into_parts`]. The edit
/// guards returned by [`Self::ir_mut`] and [`Self::source_fidelity_mut`]
/// restore canonical order before the result can be read again.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DecodeResult {
    ir: CadIr,
    report: DecodeReport,
    source_fidelity: SourceFidelity,
}

impl DecodeResult {
    /// Build a result with mandatory source fidelity after canonicalizing it and the IR.
    ///
    /// A populated dialect report must have exactly one primary layer. A
    /// producer contradiction panics in every build because it is a codec bug,
    /// not an input refusal.
    pub fn new(mut ir: CadIr, report: DecodeReport, mut source_fidelity: SourceFidelity) -> Self {
        let primary = if report.dialects.is_empty() {
            None
        } else {
            Some(
                cadmpeg_core::dialect::primary_layer(&report.dialects, &report.format)
                    .unwrap_or_else(|| {
                        panic!(
                            "primary-layer invariant failed: populated dialects for format {:?} must contain exactly one entry naming it",
                            report.format
                        )
                    }),
            )
        };
        if let Some(source) = ir.source.as_mut() {
            match primary {
                Some(matched) => {
                    source.dialect.clone_from(&matched.dialect);
                    source.declared.clone_from(&matched.declared);
                }
                None => {
                    source.dialect = None;
                    source.declared.clear();
                }
            }
        }
        ir.finalize();
        source_fidelity.finalize();
        Self {
            ir,
            report,
            source_fidelity,
        }
    }

    /// Borrow the finalized IR.
    pub fn ir(&self) -> &CadIr {
        &self.ir
    }

    /// Edits the IR and finalizes it when the returned guard is dropped.
    pub fn ir_mut(&mut self) -> impl DerefMut<Target = CadIr> + '_ {
        FinalizingEdit::new(&mut self.ir, CadIr::finalize)
    }

    /// Borrow the transfer report.
    pub fn report(&self) -> &DecodeReport {
        &self.report
    }

    /// Borrow the transfer report mutably.
    ///
    /// This does not re-project source metadata after construction. Post-hoc
    /// report mutation is a separate consistency responsibility.
    pub fn report_mut(&mut self) -> &mut DecodeReport {
        &mut self.report
    }

    /// Borrow source fidelity.
    pub fn source_fidelity(&self) -> &SourceFidelity {
        &self.source_fidelity
    }

    /// Edits source fidelity and finalizes it when the returned guard is dropped.
    pub fn source_fidelity_mut(&mut self) -> impl DerefMut<Target = SourceFidelity> + '_ {
        FinalizingEdit::new(&mut self.source_fidelity, SourceFidelity::finalize)
    }

    /// Consume into IR, report, and source fidelity.
    pub fn into_parts(self) -> (CadIr, DecodeReport, SourceFidelity) {
        (self.ir, self.report, self.source_fidelity)
    }
}

#[must_use = "the guard keeps the DecodeResult mutably borrowed until the edit is finalized"]
struct FinalizingEdit<'a, T> {
    value: &'a mut T,
    finalize: fn(&mut T),
}

impl<'a, T> FinalizingEdit<'a, T> {
    fn new(value: &'a mut T, finalize: fn(&mut T)) -> Self {
        Self { value, finalize }
    }
}

impl<T> Deref for FinalizingEdit<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<T> DerefMut for FinalizingEdit<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}

impl<T> Drop for FinalizingEdit<'_, T> {
    fn drop(&mut self) {
        (self.finalize)(self.value);
    }
}

/// Decoder and container inspector for one source format.
pub trait CodecBackend {
    /// Stable short id for this codec, e.g. `"f3d"`.
    fn id(&self) -> &'static str;

    /// Judge, from a leading byte prefix, whether this codec applies.
    fn detect(&self, prefix: &[u8]) -> Confidence;

    /// Enumerate the acquired root view's streams/segments without decoding
    /// geometry.
    ///
    /// Implemented by each codec; never called by the CLI or registry. The
    /// [`Codec::inspect`] wrapper acquires the root under the inspection's
    /// input limit and runs this under an internal context.
    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError>;

    /// Decode the acquired root view, reporting incomplete or approximate
    /// transfer.
    ///
    /// Implemented by each codec; never called by the CLI or registry. The
    /// [`Codec::decode`] wrapper acquires the root and finalizes the
    /// context around this call.
    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError>;
}

mod sealed {
    /// Private bound for the blanket [`Codec`](super::Codec) implementation.
    pub trait Sealed {}
    impl<C: super::CodecBackend + ?Sized> Sealed for C {}
}

/// Public inspection and decoding entry points.
///
/// ```compile_fail
/// use cadmpeg_ir::codec::{
///     Codec, CodecBackend, Confidence, DecodeOptions, DecodeResult,
/// };
/// use cadmpeg_core::{CodecError, ContainerSummary, ReadSeek};
/// use cadmpeg_core::decode::{DecodeContext, View};
/// use cadmpeg_core::decode::InspectOptions;
///
/// struct Rogue;
/// impl CodecBackend for Rogue {
///     fn id(&self) -> &'static str { "rogue" }
///     fn detect(&self, _: &[u8]) -> Confidence { Confidence::No }
///     fn inspect_impl(&self, _: &DecodeContext<'_>, _: View<'_>)
///         -> Result<ContainerSummary, CodecError> { panic!("never runs") }
///     fn decode_impl(&self, _: &DecodeContext<'_>, _: View<'_>)
///         -> Result<DecodeResult, CodecError> { panic!("never runs") }
/// }
/// impl Codec for Rogue {
///     fn inspect(&self, _: &mut dyn ReadSeek, _: &InspectOptions)
///         -> Result<ContainerSummary, CodecError> { panic!("never runs") }
///     fn decode(&self, _: &mut dyn ReadSeek, _: &DecodeOptions)
///         -> Result<DecodeResult, CodecError> { panic!("never runs") }
/// }
/// ```
pub trait Codec: CodecBackend + sealed::Sealed {
    /// Inspects the source under its input and resource limits.
    ///
    /// This is the only path from a backend's `ContainerSummary` to a caller,
    /// so it is where the primary-layer invariant is checked: a populated
    /// [`ContainerSummary::dialects`] names the summary's own `format` exactly
    /// once. See [`cadmpeg_core::dialect::debug_assert_primary_layer`].
    fn inspect(
        &self,
        reader: &mut dyn ReadSeek,
        options: &InspectOptions,
    ) -> Result<ContainerSummary, CodecError>;

    /// Decodes the source under its input and resource limits.
    ///
    /// [`DecodeMode::Strict`] refuses the decode with
    /// [`CodecError::StrictRefusal`] for the first reported loss whose
    /// [`StrictConsequence`] is [`StrictConsequence::Reject`]. The gate
    /// evaluates full-decode reports only: a container-only decode keeps its
    /// losses and is never refused. This gate owns the refusal predicate and
    /// the refusal class for every codec. A backend reports its losses with
    /// their strict floors and adds no strict gate of its own; a local gate
    /// widens the predicate and reclassifies the refusal without the caller
    /// seeing it.
    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError>;
}

impl<C: CodecBackend + ?Sized> Codec for C {
    fn inspect(
        &self,
        reader: &mut dyn ReadSeek,
        options: &InspectOptions,
    ) -> Result<ContainerSummary, CodecError> {
        let arena = DecodeArena::new();
        let policy = DecodePolicy {
            mode: DecodeMode::Salvage,
            limits: options.limits,
        };
        let (ctx, root) = DecodeContext::read_root(reader, &arena, &policy)?;
        let result = self.inspect_impl(&ctx, root);
        ctx.finish_session()?;
        if let Ok(summary) = &result {
            cadmpeg_core::dialect::debug_assert_primary_layer(&summary.dialects, &summary.format);
        }
        result
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError> {
        let arena = DecodeArena::new();
        let (mut ctx, root) = DecodeContext::read_root(reader, &arena, &options.policy)?;
        ctx.set_container_only(options.container_only);
        let result = self.decode_impl(&ctx, root);
        ctx.finish_session()?;
        let mut result = result?;
        result.report_mut().container_only = options.container_only;
        if options.policy.mode == DecodeMode::Strict && !options.container_only {
            if let Some(loss) = result
                .report()
                .losses
                .iter()
                .find(|loss| loss.strict_consequence() == StrictConsequence::Reject)
            {
                return Err(CodecError::StrictRefusal {
                    loss_code: loss.code.to_string(),
                    message: loss.message.clone(),
                });
            }
        }
        Ok(result)
    }
}

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

fn assert_valid_target_catalog(targets: &[TargetDescriptor]) {
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

/// The catalog entry `id` names, by id or by alias.
#[must_use]
pub fn find_target<'a>(targets: &'a [TargetDescriptor], id: &str) -> Option<&'a TargetDescriptor> {
    assert_valid_target_catalog(targets);
    targets
        .iter()
        .find(|target| target.id == id || target.aliases.contains(&id))
}

/// The catalog's default target, or `None` for an encoder with no synthesis
/// catalog.
#[must_use]
pub fn default_target(targets: &'static [TargetDescriptor]) -> Option<&'static TargetDescriptor> {
    assert_valid_target_catalog(targets);
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
    refusal(format, Some(requested.to_owned()), reason, targets)
}

/// Why every encoder refuses `Inherit` over a same-format source that records
/// no dialect (design §8.2).
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
    /// The request names a catalog row.
    Catalog {
        /// The canonical catalog entry.
        entry: &'static TargetDescriptor,
        /// The same-format source dialect displaced by this target, if any.
        displaced: Option<cadmpeg_core::dialect::DialectId>,
    },
    /// `Inherit` names a same-format source dialect outside the catalog.
    OffCatalog {
        /// The source dialect that must be preserved or refused by name.
        dialect: &'a cadmpeg_core::dialect::DialectId,
    },
}

/// Resolve target syntax and inheritance once, before codec-specific delivery.
pub fn resolve_write_request<'a>(
    ir: &'a CadIr,
    request: TargetRequest<'_>,
    format: &str,
    targets: &'static [TargetDescriptor],
) -> Result<WriteRequest<'a>, CodecError> {
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
                Some(source) => {
                    let dialect = source
                        .dialect
                        .as_ref()
                        .ok_or_else(|| unrecorded_source_dialect(format, targets))?;
                    let Some(entry) = find_target(targets, dialect.as_str()) else {
                        return Ok(WriteRequest::OffCatalog { dialect });
                    };
                    entry
                }
            }
        }
    };
    let displaced = ir
        .source
        .as_ref()
        .filter(|source| source.format == format)
        .and_then(|source| source.dialect.as_ref())
        .filter(|dialect| dialect.as_str() != entry.id)
        .cloned();
    Ok(WriteRequest::Catalog { entry, displaced })
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

/// The whole write resolution of a synthesis-only encoder (design §8.2): the
/// writer target a request names, and why the source's own dialect is not it.
///
/// A synthesis-only encoder has no retained-image path, so every export is
/// built from the neutral IR and the catalog is the exact set of dialects it
/// can produce. That makes the resolution a function of the request, the
/// catalog, and the source alone, identical in every such codec:
///
/// - `Explicit(id)` — resolve it, or refuse it as outside the catalog.
/// - `Inherit` with nothing to inherit — the catalog default.
/// - `Inherit` over a same-format source — that source's own dialect, or a
///   refusal naming it and the catalog when `parse` rejects it.
///
/// `off_catalog_source_reason` states why *this* writer cannot reproduce a
/// source dialect the catalog does not carry — the one sentence that is
/// genuinely per-codec, because the reason is the codec's own write model.
///
/// The returned dialect is the same-format source dialect displaced by the
/// selected catalog row. It is absent when the write keeps the source dialect
/// or when there is no same-format source.
///
/// Not for a codec that preserves off-catalog dialects by patch or replay
/// (`FCStd`, IGES). There a source dialect outside the catalog is written back
/// from the retained image rather than refused, so the third bullet is a
/// different law.
pub fn resolve_catalog_write(
    ir: &CadIr,
    request: TargetRequest<'_>,
    format: &str,
    targets: &'static [TargetDescriptor],
    off_catalog_source_reason: &str,
) -> Result<
    (
        &'static TargetDescriptor,
        Option<cadmpeg_core::dialect::DialectId>,
    ),
    CodecError,
> {
    match resolve_write_request(ir, request, format, targets)? {
        WriteRequest::Catalog { entry, displaced } => Ok((entry, displaced)),
        WriteRequest::OffCatalog { dialect } => Err(unsupported_target(
            format,
            dialect.as_str(),
            off_catalog_source_reason,
            targets,
        )),
    }
}

fn refusal(
    format: &str,
    requested: Option<String>,
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
        // The whole of resolution for an encoder with no catalog and no
        // dialect. With no synthesis catalog there is no row to fall back to
        // and no row to preserve, so `Inherit` is the
        // only writable request and every explicit id is outside the catalog.
        if let TargetRequest::Explicit(id) = request {
            return Err(unsupported_target(
                self.id(),
                id,
                "not a target this encoder can synthesize",
                self.targets(),
            ));
        }
        let report = ExportReport::cadir(
            "cadir".into(),
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

#[cfg(test)]
mod tests;
