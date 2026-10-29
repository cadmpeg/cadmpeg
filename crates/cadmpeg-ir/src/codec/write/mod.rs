// SPDX-License-Identifier: Apache-2.0
//! The sealed encoder contract and its blanket implementation.
//!
//! A backend declares the domain its targets come from, receives only a
//! request resolved through that domain, and returns a bare [`ExportBody`].
//! The sealed [`Encoder::plan`] wrapper is the single author of the export's
//! identity and fidelity resolution: a backend cannot report a format, a
//! target, or a fidelity state other than the one it was given.

use std::io::Write;

use crate::codec::FormatId;
use crate::document::CadIr;
use crate::report::{
    check::{Finding, ValidationReport},
    export::{
        CensusBasis, EntityCensus, ExportReport, FidelityResolution, ReplayFidelity,
        SynthesisFidelity, WritePath as ReportWritePath,
    },
    loss::LossNote,
    Severity,
};
use crate::source_fidelity::SourceFidelity;
use cadmpeg_core::decode::{DecodeDepth, ResourceLimits};
use cadmpeg_core::target::TargetCatalog;
use cadmpeg_core::CodecError;

pub mod approximation;
mod carried;
mod coverage;
pub mod edge_recovery;
mod error;
mod loss;
pub mod subset_lowering;
pub mod target;

pub use crate::validate::ValidatedFinding;
pub use cadmpeg_core::encode::{EncodeBuffer, EncodeSession};
use carried::{EntityLocation, FindingLocator};
pub use coverage::{ArenaCoverage, ArenaDisposition, ArenaDispositions};
pub use error::{CapacityField, EncodeError};
pub use loss::ExportLossCode;
pub use native::{
    NativeArenaDisposition, NativeDisposition, NativeDispositions, NativeNamespaceDispositions,
};
mod native;
use target::{DialectFree, TargetDomain, TargetRequest, TargetResolution};

/// Why a writer synthesized a payload instead of patching retained layout.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "cause", rename_all = "snake_case", deny_unknown_fields)]
pub enum SynthesisCause {
    /// Retained source content is absent from the typed document.
    SourceUnrepresented {
        /// Classes of content that make source-byte reuse ineligible.
        classes: crate::document::ArenaClasses,
    },
    /// The patch path could not express the edited document.
    PatchDeclined {
        /// The precondition or edit that declined the patch path.
        reason: String,
    },
}

/// Implementation surface for one output format.
///
/// Backends declare one target domain and receive only a request already
/// resolved through that domain. Callers use the sealed [`Encoder`] wrapper.
pub trait EncoderBackend {
    /// Stable output format id, taken from the codec that reads the format.
    const FORMAT: FormatId;

    /// The decode depth a caller requests for complete output.
    const REQUEST: DecodeDepth;

    /// Whether this backend can patch or replay retained source bytes.
    const REUSES_SOURCE: bool;

    /// Whether this format writes the document's recompute state as its own flag.
    const WRITES_RECOMPUTE: bool = false;

    /// The domain this backend's targets come from.
    type Target: TargetDomain;

    /// The domain value: [`DialectFree`] or a [`target::Catalog`] of this format's
    /// output flavors.
    const TARGET: Self::Target;

    /// Plans a write from the request resolved by [`Encoder::plan`].
    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: <Self::Target as TargetDomain>::Resolved<'_>,
    ) -> Result<ExportBody, CodecError>;
}

mod encoder_sealed {
    pub trait Sealed {}
    impl<E: super::EncoderBackend> Sealed for E {}
}

/// Public planning interface for an output format.
pub trait Encoder: encoder_sealed::Sealed {
    /// Stable output format id.
    fn id(&self) -> FormatId;

    /// The static catalog of output flavors this encoder can produce.
    ///
    /// Every native writer synthesizes its catalog targets from the document.
    /// Preservation of dialects outside the catalog is not listed here;
    /// [`TargetRequest::Inherit`] asks for it. Ids come from this
    /// encoder's own format namespace only.
    fn targets(&self) -> TargetCatalog;

    /// The decode depth a caller requests for complete output.
    fn request(&self) -> DecodeDepth;

    /// Plans one export without writing to the destination.
    fn plan(
        &self,
        input: EncodeInput<'_>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan, CodecError>;
}

impl<E: EncoderBackend> Encoder for E {
    fn id(&self) -> FormatId {
        E::FORMAT
    }

    fn targets(&self) -> TargetCatalog {
        E::TARGET.targets(E::FORMAT)
    }

    fn request(&self) -> DecodeDepth {
        E::REQUEST
    }

    fn plan(
        &self,
        input: EncodeInput<'_>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan, CodecError> {
        // (C) Excluded populated arenas contradict the document declaration.
        // Valid partial scopes remain writable; this is the state invariant.
        input.ir.check_scope().map_err(CodecError::malformed)?;
        let target = E::TARGET.resolve(input.ir, request, E::FORMAT)?;
        let identity = target.export_target().cloned();
        let synthesis_identity = target.synthesis_target().map(|target| target.cloned());
        let unrepresented = input.source_unrepresented();
        // Retained bytes and their eligibility travel together. A backend sees
        // no source bytes when the document does not represent their content.
        let backend_input = if unrepresented.is_empty() {
            input
        } else {
            EncodeInput {
                fidelity: None,
                ..input
            }
        };
        let mut body = self.plan_resolved(backend_input, target)?;
        if E::REUSES_SOURCE && !unrepresented.is_empty() {
            body.synthesis_cause = Some(SynthesisCause::SourceUnrepresented {
                classes: unrepresented,
            });
        }
        debug_assert!(
            backend_input.fidelity.is_some()
                || matches!(body.write_path, WritePath::Synthesized { .. }),
            "encoder {} reported source-byte reuse without source fidelity",
            E::FORMAT.as_str(),
        );
        let ExportBody {
            bytes,
            census,
            write_path,
            coverage,
            mut native_dispositions,
            synthesis_cause,
            mut losses,
            mut notes,
        } = body;
        if matches!(write_path, WritePath::Synthesized { .. }) {
            let required = E::REQUEST.required_scope();
            let missing = input.ir.scope.missing_from(required);
            if !missing.is_empty() {
                notes.push(format!(
                    "the document's neutral scope excludes {missing}; no neutral records of those classes are supplied."
                ));
            }
        }
        losses.extend(coverage.omission_losses(&input.ir.model, E::FORMAT.as_str()));
        native_dispositions.complete(input.ir);
        losses.extend(native_dispositions.omission_losses(input.ir, E::FORMAT.as_str()));
        if !E::WRITES_RECOMPUTE {
            let stale: Vec<_> = input
                .ir
                .model
                .features
                .iter()
                .filter(|feature| feature.needs_recompute)
                .map(|feature| feature.id.as_str())
                .collect();
            if !stale.is_empty() {
                losses.push(loss::ExportLossCode::EvaluationStale.note(format!(
                    "the {} output carries stored evaluation that needs recompute for features: {}",
                    E::FORMAT.as_str(),
                    stale.join(", "),
                )));
            }
        }
        let identity = if matches!(write_path, WritePath::Synthesized { .. }) {
            synthesis_identity?
        } else {
            identity
        };
        let write_path = write_path.into_report(input.fidelity.is_some());
        let mut report = match identity {
            None => ExportReport::cadir(census, write_path, losses, notes),
            Some(target) => ExportReport::native(target, census, write_path, losses, notes),
        };
        report.coverage = coverage;
        report.native_dispositions = native_dispositions;
        report.synthesis_cause = synthesis_cause;
        Ok(ExportPlan {
            report,
            bytes,
            format: E::FORMAT,
            coverage,
        })
    }
}

/// Borrowed inputs used to plan an export.
#[derive(Debug, Clone, Copy)]
pub struct EncodeInput<'a> {
    /// Neutral document to export.
    pub ir: &'a CadIr,
    /// Decode-time fidelity state, when available.
    pub fidelity: Option<&'a SourceFidelity>,
    /// Caller ceilings for work performed while encoding.
    pub limits: ResourceLimits,
}

impl<'a> EncodeInput<'a> {
    /// Borrows a document and its fidelity with desktop resource ceilings.
    #[must_use]
    pub const fn new(ir: &'a CadIr, fidelity: Option<&'a SourceFidelity>) -> Self {
        Self {
            ir,
            fidelity,
            limits: ResourceLimits::desktop(),
        }
    }

    /// Whether another format owns typed native content in this document.
    ///
    /// This is the same arena-level distinction used by native export coverage:
    /// the reserved `unknowns` arena holds opaque retained bytes and is excluded.
    /// Empty arenas and this writer's own namespace do not contribute. Codecs
    /// use this query instead of inspecting another codec's native records.
    #[must_use]
    pub fn has_foreign_native_content(self, format: FormatId) -> bool {
        self.ir.native.0.iter().any(|(namespace, native)| {
            namespace.as_str() != format.as_str()
                && native
                    .arenas()
                    .iter()
                    .any(|(arena, records)| arena.as_str() != "unknowns" && !records.is_empty())
        })
    }

    /// Classes of source content not held in any typed document arena.
    pub fn source_unrepresented(self) -> crate::document::ArenaClasses {
        self.fidelity.map_or_else(
            crate::document::ArenaClasses::new,
            SourceFidelity::unrepresented,
        )
    }

    /// Whether a patch or replay is eligible to consume retained source bytes.
    ///
    /// Ineligibility selects synthesis; it is not a writer refusal.
    pub fn can_reuse_source(self) -> bool {
        self.fidelity.is_some() && self.source_unrepresented().is_empty()
    }
}

/// What a backend did with the source fidelity it was given.
///
/// Carried only by synthesized writes and patches that did not replay source
/// content. The wrapper maps it onto [`FidelityResolution`] and discards it
/// when the input carried no fidelity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Consumption {
    /// This backend does not consume source fidelity.
    NotConsumed,
    /// Fidelity was available but could not be consumed.
    Degraded {
        /// Explanation of the degradation.
        reason: String,
    },
}

impl From<Consumption> for FidelityResolution {
    fn from(consumption: Consumption) -> Self {
        match consumption {
            Consumption::NotConsumed => Self::NotConsumed {},
            Consumption::Degraded { reason } => Self::Degraded { reason },
        }
    }
}

impl From<Consumption> for SynthesisFidelity {
    fn from(consumption: Consumption) -> Self {
        match consumption {
            Consumption::NotConsumed => Self::NotConsumed {},
            Consumption::Degraded { reason } => Self::Degraded { reason },
        }
    }
}

/// How a patched write handled the source fidelity supplied to it.
///
/// Some patchers edit retained sidecar bytes. Others patch native records
/// already stored in the IR and either do not consume the sidecar or report
/// why an eligible sidecar path was unavailable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchConsumption {
    /// Retained source content was consumed successfully.
    Replayed,
    /// The patch did not replay retained source content.
    Independent(Consumption),
}

/// Structurally valid backend write paths and their fidelity consumption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WritePath {
    /// The writer authored output from neutral arenas and its own typed native
    /// namespace, consuming no retained source bytes.
    Synthesized {
        /// How the synthesized write handled available source fidelity.
        consumption: Consumption,
    },
    /// The writer rewrote part of a container it did not author in full.
    Patched {
        /// How the patch handled available source fidelity.
        consumption: PatchConsumption,
    },
    /// Retained source bytes were copied unchanged.
    VerbatimReplay,
}

impl WritePath {
    fn into_report(self, fidelity_provided: bool) -> ReportWritePath {
        match self {
            Self::Synthesized { consumption } => ReportWritePath::Synthesized {
                fidelity: if fidelity_provided {
                    consumption.into()
                } else {
                    SynthesisFidelity::NotProvided {}
                },
            },
            Self::Patched {
                consumption: PatchConsumption::Replayed,
            } => ReportWritePath::Patched {
                fidelity: if fidelity_provided {
                    FidelityResolution::Replayed {}
                } else {
                    FidelityResolution::NotProvided {}
                },
            },
            Self::Patched {
                consumption: PatchConsumption::Independent(consumption),
            } => ReportWritePath::Patched {
                fidelity: if fidelity_provided {
                    consumption.into()
                } else {
                    FidelityResolution::NotProvided {}
                },
            },
            Self::VerbatimReplay => ReportWritePath::VerbatimReplay {
                fidelity: if fidelity_provided {
                    ReplayFidelity::Replayed {}
                } else {
                    ReplayFidelity::NotProvided {}
                },
            },
        }
    }
}

/// What a backend returns from `plan_resolved`.
///
/// Identity is not here: the wrapper stamps the resolved target, and the
/// fidelity resolution, onto the report it builds from this body.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportBody {
    /// The complete materialized payload.
    pub bytes: Vec<u8>,
    /// Entity counts and the semantic basis on which they were measured.
    pub census: EntityCensus,
    /// How the payload was produced.
    pub write_path: WritePath,
    /// Which model arenas the payload carries. The wrapper charges a loss for
    /// every non-empty arena this coverage omits.
    pub coverage: ArenaCoverage,
    /// Coverage of the output format's own typed native arenas.
    pub native_dispositions: NativeDispositions,
    /// Why a retained-source patch was replaced by synthesis, when applicable.
    pub synthesis_cause: Option<SynthesisCause>,
    /// Losses charged while planning.
    pub losses: Vec<LossNote>,
    /// Free-form notes.
    pub notes: Vec<String>,
}

impl ExportBody {
    /// A synthesized payload counted on IR arenas that consumes no fidelity.
    #[must_use]
    pub fn synthesized(bytes: Vec<u8>, ir: &CadIr, coverage: ArenaCoverage) -> Self {
        Self {
            bytes,
            census: EntityCensus {
                basis: CensusBasis::IrArenas,
                counts: ir.census(),
            },
            write_path: WritePath::Synthesized {
                consumption: Consumption::NotConsumed,
            },
            coverage,
            native_dispositions: NativeDispositions::default(),
            synthesis_cause: None,
            losses: Vec::new(),
            notes: Vec::new(),
        }
    }
}

/// A fully reported export awaiting its destination write.
///
/// The plan owns the complete payload. Atomic file staging belongs to the
/// artifact store; [`Self::write_to`] also supports non-file sinks.
#[derive(Debug)]
pub struct ExportPlan {
    format: FormatId,
    coverage: ArenaCoverage,
    report: ExportReport,
    bytes: Vec<u8>,
}

impl ExportPlan {
    /// Records the caller's empty-output observation without changing the payload.
    pub fn note_empty_output(&mut self) {
        if !self.report.losses.iter().any(|loss| {
            loss.code.namespace() == "export" && loss.code.local_code() == "output.empty"
        }) {
            self.report.losses.push(
                ExportLossCode::OutputEmpty
                    .note("output contains no decoded or exportable geometry"),
            );
        }
    }

    /// Returns the complete plan-time export report.
    pub fn report(&self) -> &ExportReport {
        &self.report
    }

    /// The error findings of `report` that name something this payload may carry.
    ///
    /// `ir` is the document this plan was built from. Only findings at or above
    /// `Severity::Error` are considered. Findings with no resolved entity are
    /// carried because they cannot be shown to be outside the output. A located
    /// finding is carried if any entity or related id location is carried. Model
    /// locations follow the plan's arena coverage. Native locations are carried
    /// by complete coverage or by their own format's encoder; a backend reads
    /// no native namespace other than its own format's.
    pub fn carried_errors<'r>(&self, ir: &CadIr, report: &'r ValidationReport) -> Vec<&'r Finding> {
        let errors: Vec<_> = report
            .findings
            .iter()
            .filter(|finding| finding.severity >= Severity::Error)
            .collect();
        if errors.is_empty() {
            return errors;
        }
        let locator = FindingLocator::new(ir, errors.iter().copied());
        errors
            .into_iter()
            .filter(|finding| {
                locator.scope(finding).is_none_or(|locations| {
                    locations
                        .into_iter()
                        .any(|location| self.carries_location(location))
                })
            })
            .collect()
    }

    fn carries_location(&self, location: EntityLocation<'_>) -> bool {
        match location {
            EntityLocation::Model(kind) => self.coverage.carries(kind),
            EntityLocation::Native(namespace) => {
                self.coverage == ArenaCoverage::Complete || namespace == self.format.as_str()
            }
        }
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
    const REUSES_SOURCE: bool = false;
    const FORMAT: FormatId = FormatId::new("cadir");
    /// A full decode requests all content CADIR can hold.
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const WRITES_RECOMPUTE: bool = true;

    /// CADIR writes the neutral document. Its version identifies the CADIR
    /// schema. The serialized export report uses the `cadir` identity variant,
    /// which has no native dialect target.
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(&self, input: EncodeInput<'_>, (): ()) -> Result<ExportBody, CodecError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy {
            mode: cadmpeg_core::decode::DecodeMode::Salvage,
            limits: input.limits,
        };
        let ctx = cadmpeg_core::decode::DecodeContext::new(&arena, &policy, DecodeDepth::Full);
        let mut bytes = Vec::new();
        crate::hash::finite_json::write_cadir_json(&ctx, &mut bytes, input.ir).map_err(
            |error| match error {
                crate::hash::finite_json::CanonicalJsonError::Resource(error) => error,
                error => CodecError::Malformed(error.to_string()),
            },
        )?;
        ctx.finish_session()?;
        bytes.push(b'\n');
        // CADIR is the neutral document itself: there is no container to
        // replay or patch, so this encoder has one path and states it. The
        // serialization carries every arena.
        let mut body = ExportBody::synthesized(bytes, input.ir, ArenaCoverage::Complete);
        body.native_dispositions = NativeDispositions::written(input.ir);
        Ok(body)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod test_support;
