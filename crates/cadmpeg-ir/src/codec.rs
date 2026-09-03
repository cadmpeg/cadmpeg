// SPDX-License-Identifier: Apache-2.0
//! Interfaces for detecting, inspecting, decoding, and encoding CAD formats.
//!
//! [`Codec`] is object-safe for runtime codec registries. Detection consumes a
//! byte prefix, inspection summarizes a seekable container, and decoding
//! produces a finalized [`CadIr`] plus a [`DecodeReport`].
//!
//! A codec implements only the required [`CodecBackend`] methods. The public
//! [`Codec::inspect`] and [`Codec::decode`] entry points are the
//! single enforcement point for root-input limits and session finalize checks;
//! they live on the sealed [`Codec`] trait, blanket-implemented for every
//! [`CodecBackend`], so a codec cannot override an entry point and drop the
//! enforcement.

use std::fmt;

use crate::document::CadIr;
use crate::report::{
    Coverage, DecodeReport, LossNote as DecodeLoss, StrictConsequence, TransferLedger,
};
use crate::source_fidelity::SourceFidelity;
use crate::ContainerSummary;
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodeMode, DecodePolicy, InspectOptions, View,
};
use cadmpeg_core::dialect::FormatIdentity;
use cadmpeg_core::{CodecError, ReadSeek};
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
/// The sealed [`Codec`] wrapper constructs this value after it stamps the source
/// classification onto the report and finalizes the IR and source fidelity.
/// `#[non_exhaustive]` blocks external struct literals so callers cannot skip
/// finalization. Read through [`Self::ir`], [`Self::report`], and
/// [`Self::source_fidelity`]. Consume with [`Self::into_parts`] before editing
/// either document.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DecodeResult {
    ir: CadIr,
    report: DecodeReport,
    source_fidelity: SourceFidelity,
}

/// A strict-policy refusal bound to the loss in its completed decode report.
///
/// Only the sealed decode wrapper constructs this value. The refusing loss is
/// therefore always an element of [`Self::report`].
#[derive(Debug)]
pub struct StrictDecodeRejection {
    report: Box<DecodeReport>,
    loss_index: usize,
}

#[derive(Debug, Clone, Copy)]
struct RejectingLossIndex(usize);

impl StrictDecodeRejection {
    fn new(report: DecodeReport, loss_index: RejectingLossIndex) -> Self {
        Self {
            report: Box::new(report),
            loss_index: loss_index.0,
        }
    }

    /// Returns the loss that caused the strict-policy refusal.
    #[must_use]
    pub fn loss(&self) -> &DecodeLoss {
        &self.report.losses[self.loss_index]
    }

    /// Returns the completed report that contains the refusing loss.
    #[must_use]
    pub fn report(&self) -> &DecodeReport {
        &self.report
    }
}

impl fmt::Display for StrictDecodeRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let loss = self.loss();
        write!(f, "{}: {}", loss.code, loss.message)
    }
}

/// Failure from the policy-enforcing decode entry point.
///
/// Backend and resource failures remain [`CodecError`] values. A strict-policy
/// refusal is separate because decoding completed and produced a report; the
/// caller must be able to serialize that evidence even though no document is
/// admitted.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DecodeFailure {
    /// The codec or decode context failed before a result was produced.
    #[error(transparent)]
    Codec(#[from] CodecError),
    /// Strict mode rejected the first loss whose floor requires refusal.
    #[error("strict mode rejects {rejection}")]
    StrictRejected {
        /// The completed report and its refusing loss.
        rejection: StrictDecodeRejection,
    },
}

/// What a backend returns from [`CodecBackend::decode_impl`].
///
/// Source identity is authored once, in `ir.source`. The sealed wrapper stamps
/// that classification onto the report; a backend cannot describe the document
/// and its report with two different identities.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    /// The decoded document, with its source metadata authored by the backend.
    pub ir: CadIr,
    /// The report without classification.
    pub body: DecodeBody,
    /// Decode-time annotations and retained source records.
    pub source_fidelity: SourceFidelity,
}

/// A [`DecodeReport`] without its classification.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeBody {
    /// Whether B-rep geometry was transferred into the IR.
    pub geometry_transferred: bool,
    /// Coverage measures keyed by their declared name.
    pub coverage: Coverage,
    /// Losses resolved during decoding.
    pub losses: Vec<DecodeLoss>,
    /// Codec-defined informational notes.
    pub notes: Vec<String>,
    /// Complete source-to-result accounting.
    pub transfer_ledger: TransferLedger,
}

impl DecodeBody {
    /// An empty body with the given B-rep geometry outcome.
    #[must_use]
    pub fn new(geometry_transferred: bool) -> Self {
        Self {
            geometry_transferred,
            coverage: Coverage::default(),
            losses: Vec::new(),
            notes: Vec::new(),
            transfer_ledger: TransferLedger::default(),
        }
    }
}

impl DecodeResult {
    /// Builds a result by stamping the document's source classification onto
    /// the report body, then canonicalizing the IR and source fidelity.
    ///
    /// A document without source metadata yields an unclassified report for
    /// `format`, the codec's registry format.
    #[must_use]
    pub(crate) fn new(decoded: Decoded, format: &str, container_only: bool) -> Self {
        let Decoded {
            mut ir,
            body,
            mut source_fidelity,
        } = decoded;
        let classification = match ir.source.as_ref() {
            Some(source) => source.classification().clone(),
            None => FormatIdentity::unclassified(format),
        };
        ir.finalize();
        source_fidelity.finalize();
        Self {
            ir,
            report: DecodeReport::from_body(classification, body, container_only),
            source_fidelity,
        }
    }

    /// Borrow the finalized IR.
    pub fn ir(&self) -> &CadIr {
        &self.ir
    }

    /// Borrow the transfer report.
    pub fn report(&self) -> &DecodeReport {
        &self.report
    }

    /// Borrow source fidelity.
    pub fn source_fidelity(&self) -> &SourceFidelity {
        &self.source_fidelity
    }

    /// Consume into IR, report, and source fidelity.
    pub fn into_parts(self) -> (CadIr, DecodeReport, SourceFidelity) {
        (self.ir, self.report, self.source_fidelity)
    }
}

/// Decoder and container inspector for one source format.
pub trait CodecBackend {
    /// Registry format namespace this codec decodes, e.g. `"f3d"`.
    ///
    /// The sealed wrapper reports it as [`Codec::id`] and refuses any result
    /// whose primary format names another namespace.
    const FORMAT: &'static str;

    /// Judge, from a leading byte prefix, whether this codec applies.
    fn detect_impl(&self, prefix: &[u8]) -> Confidence;

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
    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError>;
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
///     Codec, CodecBackend, Confidence, DecodeOptions, DecodeResult, Decoded,
/// };
/// use cadmpeg_core::{CodecError, ReadSeek};
/// use cadmpeg_core::decode::{DecodeContext, View};
/// use cadmpeg_core::decode::InspectOptions;
/// use cadmpeg_ir::ContainerSummary;
///
/// struct Rogue;
/// impl CodecBackend for Rogue {
///     const FORMAT: &'static str = "rogue";
///     fn detect_impl(&self, _: &[u8]) -> Confidence { Confidence::No }
///     fn inspect_impl(&self, _: &DecodeContext<'_>, _: View<'_>)
///         -> Result<ContainerSummary, CodecError> { panic!("never runs") }
///     fn decode_impl(&self, _: &DecodeContext<'_>, _: View<'_>)
///         -> Result<Decoded, CodecError> { panic!("never runs") }
/// }
/// impl Codec for Rogue {
///     fn id(&self) -> &'static str { "rogue" }
///     fn detect(&self, _: &[u8]) -> Confidence { Confidence::No }
///     fn inspect(&self, _: &mut dyn ReadSeek, _: &InspectOptions)
///         -> Result<ContainerSummary, CodecError> { panic!("never runs") }
///     fn decode(&self, _: &mut dyn ReadSeek, _: &DecodeOptions)
///         -> Result<DecodeResult, cadmpeg_ir::codec::DecodeFailure> {
///         panic!("never runs")
///     }
/// }
/// ```
pub trait Codec: sealed::Sealed {
    /// Registry format namespace, [`CodecBackend::FORMAT`].
    fn id(&self) -> &'static str;

    /// Judge, from a leading byte prefix, whether this codec applies.
    fn detect(&self, prefix: &[u8]) -> Confidence;

    /// Inspects the source under its input and resource limits.
    fn inspect(
        &self,
        reader: &mut dyn ReadSeek,
        options: &InspectOptions,
    ) -> Result<ContainerSummary, CodecError>;

    /// Decodes the source under its input and resource limits.
    ///
    /// [`DecodeMode::Strict`] refuses the decode with
    /// [`DecodeFailure::StrictRejected`] for the first reported loss whose
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
    ) -> Result<DecodeResult, DecodeFailure>;
}

impl<C: CodecBackend + ?Sized> Codec for C {
    fn id(&self) -> &'static str {
        C::FORMAT
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        self.detect_impl(prefix)
    }

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
        let (ctx, root) = DecodeContext::read_root(reader, &arena, &policy, false)?;
        let result = self.inspect_impl(&ctx, root);
        ctx.finish_session()?;
        let result = result?;
        if result.format() != C::FORMAT {
            return Err(CodecError::WrongFormat(format!(
                "codec {:?} inspected a {:?} container",
                C::FORMAT,
                result.format()
            )));
        }
        Ok(result)
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, DecodeFailure> {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::read_root(reader, &arena, &options.policy, options.container_only)?;
        let decoded = self.decode_impl(&ctx, root);
        ctx.finish_session()?;
        let result = DecodeResult::new(decoded?, C::FORMAT, options.container_only);
        if result.report().format() != C::FORMAT {
            return Err(CodecError::WrongFormat(format!(
                "codec {:?} decoded a {:?} document",
                C::FORMAT,
                result.report().format()
            ))
            .into());
        }
        let strict_loss_index =
            if options.policy.mode == DecodeMode::Strict && !options.container_only {
                result
                    .report()
                    .losses
                    .iter()
                    .position(|loss| loss.strict_consequence() == StrictConsequence::Reject)
                    .map(RejectingLossIndex)
            } else {
                None
            };
        if let Some(loss_index) = strict_loss_index {
            let (_, report, _) = result.into_parts();
            return Err(DecodeFailure::StrictRejected {
                rejection: StrictDecodeRejection::new(report, loss_index),
            });
        }
        Ok(result)
    }
}

pub mod write;

#[cfg(test)]
mod tests;
