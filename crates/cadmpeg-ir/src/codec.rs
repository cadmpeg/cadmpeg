// SPDX-License-Identifier: Apache-2.0
//! Interfaces for detecting, inspecting, decoding, and encoding CAD formats.
//!
//! [`Codec`] is object-safe for runtime codec registries. Detection consumes a
//! byte prefix, inspection summarizes a seekable container, and decoding
//! produces a finalized [`CadIr`] plus a [`DecodeReport`].
//!
//! A codec implements only the required [`Codec`] methods. The public
//! [`CodecEntry::inspect`] and [`CodecEntry::decode`] entry points are the
//! single enforcement point for root-input limits and session finalize checks;
//! they live on the sealed [`CodecEntry`] trait, blanket-implemented for every
//! `Codec`, so a codec cannot override an entry point and drop the
//! enforcement.

use std::fmt;
use std::io::Write;

use crate::document::CadIr;
use crate::report::DecodeReport;
use crate::report::ExportReport;
use crate::report::StrictConsequence;
use crate::source_fidelity::SourceFidelity;
use cadmpeg_codec_core::decode::{
    DecodeArena, DecodeContext, DecodeMode, DecodePolicy, InspectOptions, View,
};
use cadmpeg_codec_core::{CodecError, ContainerSummary, ReadSeek};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How confident a codec is that it can handle a given byte prefix.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DecodeOptions {
    /// Stop after the container layer; do not attempt entity decode.
    pub container_only: bool,
    /// Resource limits and failure-handling mode governing the decode.
    ///
    /// Defaulted on deserialization so options serialized before this field
    /// existed still parse, taking the desktop profile in salvage mode.
    #[serde(default)]
    pub policy: DecodePolicy,
}

/// A decoded document plus its loss report.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeResult {
    /// The decoded IR.
    pub ir: CadIr,
    /// What was transferred and what was lost.
    pub report: DecodeReport,
    /// Decode-time annotations and retained native records.
    pub source_fidelity: SourceFidelity,
}

impl DecodeResult {
    /// Build a result after canonicalizing the document's arena order.
    pub fn new(mut ir: CadIr, report: DecodeReport) -> Self {
        ir.finalize();
        Self {
            ir,
            report,
            source_fidelity: SourceFidelity::default(),
        }
    }

    /// Build a result with an explicit source-fidelity sidecar.
    pub fn with_source_fidelity(
        mut ir: CadIr,
        report: DecodeReport,
        mut source_fidelity: SourceFidelity,
    ) -> Self {
        ir.finalize();
        source_fidelity.finalize();
        Self {
            ir,
            report,
            source_fidelity,
        }
    }
}

/// Decoder and container inspector for one source format.
pub trait Codec {
    /// Stable short id for this codec, e.g. `"f3d"`.
    fn id(&self) -> &'static str;

    /// Judge, from a leading byte prefix, whether this codec applies.
    fn detect(&self, prefix: &[u8]) -> Confidence;

    /// Enumerate the acquired root view's streams/segments without decoding
    /// geometry.
    ///
    /// Implemented by each codec; never called by the CLI or registry. The
    /// [`CodecEntry::inspect`] wrapper acquires the root under the inspection's
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
    /// [`CodecEntry::decode`] wrapper acquires the root and finalizes the
    /// context around this call.
    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError>;
}

mod sealed {
    /// Private bound for the blanket [`CodecEntry`](super::CodecEntry) implementation.
    pub trait Sealed {}
    impl<C: super::Codec + ?Sized> Sealed for C {}
}

/// Public inspection and decoding entry points.
///
/// ```compile_fail
/// use cadmpeg_ir::codec::{
///     Codec, CodecEntry, Confidence, DecodeOptions, DecodeResult,
/// };
/// use cadmpeg_codec_core::{CodecError, ContainerSummary, ReadSeek};
/// use cadmpeg_codec_core::decode::{DecodeContext, View};
/// use cadmpeg_codec_core::decode::InspectOptions;
///
/// struct Rogue;
/// impl Codec for Rogue {
///     fn id(&self) -> &'static str { "rogue" }
///     fn detect(&self, _: &[u8]) -> Confidence { Confidence::No }
///     fn inspect_impl(&self, _: &DecodeContext<'_>, _: View<'_>)
///         -> Result<ContainerSummary, CodecError> { unimplemented!() }
///     fn decode_impl(&self, _: &DecodeContext<'_>, _: View<'_>)
///         -> Result<DecodeResult, CodecError> { unimplemented!() }
/// }
/// impl CodecEntry for Rogue {
///     fn inspect(&self, _: &mut dyn ReadSeek, _: &InspectOptions)
///         -> Result<ContainerSummary, CodecError> { unimplemented!() }
///     fn decode(&self, _: &mut dyn ReadSeek, _: &DecodeOptions)
///         -> Result<DecodeResult, CodecError> { unimplemented!() }
/// }
/// ```
pub trait CodecEntry: Codec + sealed::Sealed {
    /// Inspects the source under its input and resource limits.
    fn inspect(
        &self,
        reader: &mut dyn ReadSeek,
        options: &InspectOptions,
    ) -> Result<ContainerSummary, CodecError>;

    /// Decodes the source under its input and resource limits.
    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError>;
}

impl<C: Codec + ?Sized> CodecEntry for C {
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
        let result = result?;
        if options.policy.mode == DecodeMode::Strict && !result.report.container_only {
            if let Some(loss) = result
                .report
                .losses
                .iter()
                .find(|loss| loss.strict_consequence() == StrictConsequence::Reject)
            {
                return Err(CodecError::Malformed(format!(
                    "strict mode rejects {}: {}",
                    loss.code, loss.message
                )));
            }
        }
        Ok(result)
    }
}

/// A native-format writer.
pub trait Encoder {
    /// Stable output format id.
    fn id(&self) -> &'static str;

    /// Encode one IR document to the target format.
    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<ExportReport, CodecError>;

    /// Encode with decode-time source fidelity when the caller retained it.
    ///
    /// Encoders that do not consume source accounting use the neutral model
    /// through [`Encoder::encode`].
    fn encode_with_source_fidelity(
        &self,
        ir: &CadIr,
        source_fidelity: Option<&SourceFidelity>,
        writer: &mut dyn Write,
    ) -> Result<ExportReport, CodecError> {
        let _ = source_fidelity;
        self.encode(ir, writer)
    }
}

/// Encoder for canonical versioned CADIR JSON.
#[derive(Debug, Clone, Copy, Default)]
pub struct CadirEncoder;

impl Encoder for CadirEncoder {
    fn id(&self) -> &'static str {
        "cadir"
    }

    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<ExportReport, CodecError> {
        let mut json = ir
            .to_canonical_json()
            .map_err(|error| CodecError::Malformed(error.to_string()))?;
        json.push('\n');
        writer.write_all(json.as_bytes())?;
        let validation = crate::validate(ir, Vec::new());
        let total_entities = validation.entity_counts.values().sum();
        Ok(ExportReport {
            format: "cadir".into(),
            entity_counts: validation.entity_counts,
            total_entities,
            losses: Vec::new(),
            notes: Vec::new(),
        })
    }
}
