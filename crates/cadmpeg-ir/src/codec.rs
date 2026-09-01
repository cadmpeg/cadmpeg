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
    #[error("strict mode rejects {loss_code}: {message}")]
    StrictRejected {
        /// Stable `namespace/code` form of the refusing loss.
        loss_code: String,
        /// The refusing loss's own message, without any refusal prefix.
        message: String,
        /// Completed decode report containing the refusing loss and all other
        /// evidence recovered before the policy gate ran.
        report: Box<DecodeReport>,
    },
}

/// A decoded document and its report disagree about source identity.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind}")]
#[non_exhaustive]
pub struct DecodeResultError {
    kind: DecodeResultErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
enum DecodeResultErrorKind {
    #[error("classified decode report for {report_format:?} requires source metadata")]
    ClassifiedReportWithoutSource { report_format: String },
    #[error(
        "decode source format {source_format:?} does not match report primary format {report_format:?}"
    )]
    SourceFormatMismatch {
        source_format: String,
        report_format: String,
    },
    #[error(
        "decode source dialect metadata ({source_match}) disagrees with report primary dialect metadata ({report_match})"
    )]
    SourceDialectMismatch {
        source_match: String,
        report_match: String,
    },
    #[error(
        "decode source dialect {source_dialect:?} is classified but report for {report_format:?} is unclassified"
    )]
    ClassifiedSourceWithUnclassifiedReport {
        source_dialect: String,
        report_format: String,
    },
}

fn describe_dialect_match(matched: &cadmpeg_core::dialect::DialectMatch) -> String {
    format!(
        "dialect {}, admission {:?}, instance {:?}, declared {:?}",
        matched.dialect(),
        matched.admission(),
        matched.instance(),
        matched.declared()
    )
}

impl From<DecodeResultError> for CodecError {
    fn from(error: DecodeResultError) -> Self {
        Self::malformed(error)
    }
}

impl DecodeResult {
    /// Build a result with mandatory source fidelity after canonicalizing it and the IR.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeResultError`] when a classified report has no source
    /// metadata, or when source metadata and report classification disagree.
    pub fn new(
        mut ir: CadIr,
        report: DecodeReport,
        mut source_fidelity: SourceFidelity,
    ) -> Result<Self, DecodeResultError> {
        match (
            ir.source.as_mut(),
            report
                .dialects()
                .map(cadmpeg_core::dialect::DialectLayers::primary),
        ) {
            (None, Some(matched)) => {
                return Err(DecodeResultError {
                    kind: DecodeResultErrorKind::ClassifiedReportWithoutSource {
                        report_format: matched.format().to_owned(),
                    },
                });
            }
            (Some(source), Some(matched)) => {
                if source.format() != matched.format() {
                    return Err(DecodeResultError {
                        kind: DecodeResultErrorKind::SourceFormatMismatch {
                            source_format: source.format().to_owned(),
                            report_format: matched.format().to_owned(),
                        },
                    });
                }
                if let Some(source_dialect) = source.dialect() {
                    if source_dialect != matched {
                        return Err(DecodeResultError {
                            kind: DecodeResultErrorKind::SourceDialectMismatch {
                                source_match: describe_dialect_match(source_dialect),
                                report_match: describe_dialect_match(matched),
                            },
                        });
                    }
                }
                *source = crate::document::SourceMeta::classified(
                    matched.clone(),
                    std::mem::take(&mut source.attributes),
                );
            }
            (Some(source), None) => {
                if source.format() != report.format() {
                    return Err(DecodeResultError {
                        kind: DecodeResultErrorKind::SourceFormatMismatch {
                            source_format: source.format().to_owned(),
                            report_format: report.format().to_owned(),
                        },
                    });
                }
                if let Some(source_dialect) = source.dialect() {
                    return Err(DecodeResultError {
                        kind: DecodeResultErrorKind::ClassifiedSourceWithUnclassifiedReport {
                            source_dialect: source_dialect.dialect().to_string(),
                            report_format: report.format().to_owned(),
                        },
                    });
                }
            }
            (None, None) => {}
        }
        ir.finalize();
        source_fidelity.finalize();
        Ok(Self {
            ir,
            report,
            source_fidelity,
        })
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
        self.report.stamp_request_scope(container_only);
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
///         -> Result<DecodeResult, cadmpeg_ir::codec::DecodeFailure> {
///         panic!("never runs")
///     }
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
        let result = result?;
        if result.format() != self.id() {
            return Err(CodecError::ContractViolation {
                codec: self.id(),
                operation: "inspect",
                expected: self.id().to_owned(),
                reported: result.format().to_owned(),
            });
        }
        Ok(result)
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, DecodeFailure> {
        let arena = DecodeArena::new();
        let (mut ctx, root) = DecodeContext::read_root(reader, &arena, &options.policy)?;
        ctx.set_container_only(options.container_only);
        let result = self.decode_impl(&ctx, root);
        ctx.finish_session()?;
        let mut result = result?;
        if result.report().format() != self.id() {
            return Err(CodecError::ContractViolation {
                codec: self.id(),
                operation: "decode",
                expected: self.id().to_owned(),
                reported: result.report().format().to_owned(),
            }
            .into());
        }
        result.stamp_container_only(options.container_only);
        let strict_refusal = if options.policy.mode == DecodeMode::Strict && !options.container_only
        {
            result
                .report()
                .losses
                .iter()
                .find(|loss| loss.strict_consequence() == StrictConsequence::Reject)
                .map(|loss| (loss.code.to_string(), loss.message.clone()))
        } else {
            None
        };
        if let Some((loss_code, message)) = strict_refusal {
            let (_, report, _) = result.into_parts();
            return Err(DecodeFailure::StrictRejected {
                loss_code,
                message,
                report: Box::new(report),
            });
        }
        Ok(result)
    }
}

mod write;
#[cfg(test)]
use write::resolve_write_request;
pub use write::{
    CadirEncoder, EncodeInput, Encoder, EncoderBackend, EncoderTargetDomain, ExportPlan,
    ResolvedEncoderTarget, ResolvedWrite, TargetRequest,
};

#[cfg(test)]
mod tests;
