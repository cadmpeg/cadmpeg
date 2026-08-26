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
    pub fn new(mut ir: CadIr, report: DecodeReport, mut source_fidelity: SourceFidelity) -> Self {
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

/// A native-format writer.
pub trait Encoder {
    /// Stable output format id.
    fn id(&self) -> &'static str;

    /// Plans one export without writing to the destination.
    fn plan<'a>(&self, input: EncodeInput<'a>) -> Result<ExportPlan<'a>, CodecError>;
}

/// Borrowed inputs used to plan an export.
#[derive(Debug, Clone, Copy)]
pub struct EncodeInput<'a> {
    /// Neutral document to export.
    pub ir: &'a CadIr,
    /// Decode-time fidelity state, when available.
    pub fidelity: Option<&'a SourceFidelity>,
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

    fn plan<'a>(&self, input: EncodeInput<'a>) -> Result<ExportPlan<'a>, CodecError> {
        let report = ExportReport {
            format: "cadir".into(),
            census: EntityCensus {
                basis: CensusBasis::IrArenas,
                counts: input.ir.census(),
            },
            fidelity: if input.fidelity.is_some() {
                FidelityResolution::NotConsumed
            } else {
                FidelityResolution::NotProvided
            },
            // CADIR is the neutral document itself: there is no container to
            // replay or patch, so this encoder has one path and states it.
            write_path: WritePath::Synthesized,
            losses: Vec::new(),
            notes: Vec::new(),
        };
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
