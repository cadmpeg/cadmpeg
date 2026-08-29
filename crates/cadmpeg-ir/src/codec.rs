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
use std::ops::{Deref, DerefMut};

use crate::document::CadIr;
use crate::report::DecodeReport;
use crate::report::StrictConsequence;
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
        if let Some(source) = ir.source.as_mut() {
            match report
                .dialects()
                .map(cadmpeg_core::dialect::DialectLayers::primary)
            {
                Some(matched) => {
                    source.dialect = Some(matched.clone());
                }
                None => {
                    source.dialect = None;
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

    /// Record whether the caller requested a container-only decode.
    fn stamp_container_only(&mut self, container_only: bool) {
        self.report.container_only = container_only;
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
        result.stamp_container_only(options.container_only);
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

mod write;
pub use write::*;

#[cfg(test)]
mod tests;
