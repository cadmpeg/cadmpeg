// SPDX-License-Identifier: Apache-2.0
//! The sealed encoder contract and its blanket implementation.
//!
//! A backend declares the domain its targets come from, receives only a
//! request resolved through that domain, and returns a bare [`ExportBody`].
//! The sealed [`Encoder::plan`] wrapper is the single author of the export's
//! identity and fidelity resolution: a backend cannot report a format, a
//! target, or a fidelity state other than the one it was given. It applies
//! the caller's [`Strictness`] to the completed report before any byte
//! reaches a destination.

use std::collections::BTreeSet;
use std::fmt;
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
    strictness::Strictness,
    Severity,
};
use crate::source_fidelity::SourceFidelity;
use cadmpeg_core::decode::{Companions, DecodeDepth};
use cadmpeg_core::target::TargetCatalog;
use cadmpeg_core::CodecError;

pub mod approximation;
mod carried;
mod coverage;
pub mod edge_recovery;
mod loss;
pub mod subset_lowering;
pub mod target;

use carried::{EntityLocation, FindingLocator};
pub use coverage::{
    ArenaCoverage, ArenaDisposition, ArenaDispositions, DesignArenaDispositions,
    ProcessArenaDispositions,
};
use loss::ExportLossCode;
use target::{DialectFree, TargetDomain, TargetRequest, TargetResolution};

/// How much of its source a document must record for an encoder to plan it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthNeed {
    /// The encoder reads nothing a shallower decode leaves out and refuses a
    /// document that records less than this depth.
    AtLeast(DecodeDepth),
    /// The encoder writes the document at whatever depth it records, so it
    /// refuses none; a caller decodes at [`DecodeDepth::Full`] for it.
    Recorded,
}

impl DepthNeed {
    /// The depth a caller decodes at to satisfy this need.
    #[must_use]
    pub const fn request(self) -> DecodeDepth {
        match self {
            Self::AtLeast(depth) => depth,
            Self::Recorded => DecodeDepth::Full,
        }
    }

    /// Whether a document recording `depth` satisfies this need.
    #[must_use]
    pub fn admits(self, depth: DecodeDepth) -> bool {
        match self {
            Self::AtLeast(needed) => depth >= needed,
            Self::Recorded => true,
        }
    }
}

/// Implementation surface for one output format.
///
/// Backends declare one target domain and receive only a request already
/// resolved through that domain. Callers use the sealed [`Encoder`] wrapper.
pub trait EncoderBackend {
    /// Stable output format id, taken from the codec that reads the format.
    const FORMAT: FormatId;

    /// The decode depth this backend plans from. [`Encoder::plan`] refuses a
    /// shallower document before the backend runs.
    const NEEDS: DepthNeed;

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
    /// Whether a given input reaches one is resolution's answer, not the
    /// catalog's: a patch-only writer's row is reachable only from a retained
    /// source of that flavor, and `plan` refuses by name where it cannot
    /// deliver. Preservation of dialects outside the catalog is not listed
    /// here; [`TargetRequest::Inherit`] asks for it. Ids come from this
    /// encoder's own format namespace only.
    fn targets(&self) -> TargetCatalog;

    /// The decode depth this encoder plans from, [`EncoderBackend::NEEDS`].
    /// A caller decodes at [`DepthNeed::request`].
    fn needs(&self) -> DepthNeed;

    /// Plans one export without writing to the destination.
    ///
    /// Refuses with [`CodecError::InsufficientDepth`] a document that records
    /// less depth than [`Self::needs`] admits. Charges `export:record.invalid`
    /// for each Error of [`EncodeInput::validation`] in what the output
    /// writes. Refuses with [`EncodeError::StrictRejected`] a completed report
    /// that holds an entry [`EncodeInput::strictness`] refuses.
    fn plan(
        &self,
        input: EncodeInput<'_>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan, EncodeError>;
}

impl<E: EncoderBackend> Encoder for E {
    fn id(&self) -> FormatId {
        E::FORMAT
    }

    fn targets(&self) -> TargetCatalog {
        E::TARGET.targets(E::FORMAT)
    }

    fn needs(&self) -> DepthNeed {
        E::NEEDS
    }

    fn plan(
        &self,
        input: EncodeInput<'_>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan, EncodeError> {
        let recorded = input.ir.scope().depth();
        if !E::NEEDS.admits(recorded) {
            return Err(CodecError::InsufficientDepth {
                format: E::FORMAT.as_str(),
                needed: E::NEEDS.request(),
                recorded,
            }
            .into());
        }
        let target = E::TARGET.resolve(input.ir, request, E::FORMAT)?;
        let identity = target.export_target().cloned();
        let body = self.plan_resolved(input, target)?;
        let ExportBody {
            bytes,
            census,
            write_path,
            coverage,
            mut losses,
            notes,
        } = body;
        losses.extend(coverage.omission_losses(input.ir, E::FORMAT.as_str()));
        if let Some(validation) = input.validation {
            losses.extend(
                carried_errors(input.ir, validation, coverage, E::FORMAT)
                    .into_iter()
                    .map(record_invalid),
            );
        }
        let write_path = write_path.into_report(input.fidelity.is_some());
        let report = match identity {
            None => ExportReport::cadir(census, write_path, losses, notes),
            Some(target) => ExportReport::native(target, census, write_path, losses, notes),
        };
        if let Some(entry) = input.strictness.first_refused(&report.losses, false) {
            return Err(EncodeError::StrictRejected(StrictWriteRejection {
                report: Box::new(report),
                entry,
            }));
        }
        Ok(ExportPlan {
            report,
            bytes,
            format: E::FORMAT,
            coverage,
        })
    }
}

/// The strictness that refuses nothing, the default of [`EncodeInput`].
static PERMISSIVE: Strictness = Strictness {
    pinned: false,
    floor: None,
    codes: BTreeSet::new(),
};

/// Borrowed inputs used to plan an export.
#[derive(Debug, Clone, Copy)]
pub struct EncodeInput<'a> {
    /// Neutral document to export.
    pub ir: &'a CadIr,
    /// Decode-time fidelity state, when available.
    pub fidelity: Option<&'a SourceFidelity>,
    /// The source of the files that external assets of the document name,
    /// for an encoder that writes their content into its output.
    pub assets: Option<&'a dyn Companions>,
    /// The validation of `ir`. The wrapper charges `export:record.invalid`
    /// for each of its Errors in what the output writes.
    pub validation: Option<&'a ValidationReport>,
    /// The report entries on which the caller refuses the write. The default
    /// refuses none.
    pub strictness: &'a Strictness,
}

impl<'a> EncodeInput<'a> {
    /// Borrows a document and its decode-time fidelity for one export.
    #[must_use]
    pub fn new(ir: &'a CadIr, fidelity: Option<&'a SourceFidelity>) -> Self {
        Self {
            ir,
            fidelity,
            assets: None,
            validation: None,
            strictness: &PERMISSIVE,
        }
    }

    /// Attaches the source of the files that external assets name.
    #[must_use]
    pub const fn with_assets(mut self, assets: &'a dyn Companions) -> Self {
        self.assets = Some(assets);
        self
    }

    /// Attaches the validation of the document.
    #[must_use]
    pub const fn with_validation(mut self, validation: &'a ValidationReport) -> Self {
        self.validation = Some(validation);
        self
    }

    /// Sets the report entries on which the caller refuses the write.
    #[must_use]
    pub const fn with_strictness(mut self, strictness: &'a Strictness) -> Self {
        self.strictness = strictness;
        self
    }
}

/// Failure from the sealed [`Encoder::plan`] wrapper.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EncodeError {
    /// The encoder or its planning failed before a report was completed.
    #[error(transparent)]
    Codec(#[from] CodecError),
    /// The completed export report holds an entry that
    /// [`EncodeInput::strictness`] refuses. Nothing is written.
    #[error("strictness refuses {0}")]
    StrictRejected(StrictWriteRejection),
}

/// A strict refusal bound to the entry in its completed export report.
///
/// Only the sealed encode wrapper constructs it, so the refusing entry is
/// always an element of [`Self::report`].
#[derive(Debug)]
pub struct StrictWriteRejection {
    report: Box<ExportReport>,
    entry: usize,
}

impl StrictWriteRejection {
    /// The entry that refused the write.
    #[must_use]
    pub fn entry(&self) -> &LossNote {
        &self.report.losses[self.entry]
    }

    /// The completed report that holds the refusing entry.
    #[must_use]
    pub fn report(&self) -> &ExportReport {
        &self.report
    }
}

impl fmt::Display for StrictWriteRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let entry = self.entry();
        write!(f, "{}: {}", entry.code, entry.message)
    }
}

/// The `export:record.invalid` entry of one carried validation Error,
/// naming its record, or its rule where it names no record.
fn record_invalid(finding: &Finding) -> LossNote {
    let subject = finding
        .entity
        .clone()
        .unwrap_or_else(|| finding.check.to_string());
    ExportLossCode::RecordInvalid.note(format!(
        "{subject}: written despite validation error: {}",
        finding.message
    ))
}

/// The Error findings of `report` that name something an output of `format`
/// with `coverage` may carry. Findings with no resolved entity are carried
/// because they cannot be shown to be outside the output.
fn carried_errors<'r>(
    ir: &CadIr,
    report: &'r ValidationReport,
    coverage: ArenaCoverage,
    format: FormatId,
) -> Vec<&'r Finding> {
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
                    .any(|location| carries_location(coverage, format, location))
            })
        })
        .collect()
}

fn carries_location(
    coverage: ArenaCoverage,
    format: FormatId,
    location: EntityLocation<'_>,
) -> bool {
    match location {
        EntityLocation::Model(kind) => coverage.carries(kind),
        EntityLocation::Native(namespace) => {
            coverage == ArenaCoverage::Complete || namespace == format.as_str()
        }
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
    /// The writer authored every output byte from neutral IR.
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
        carried_errors(ir, report, self.coverage, self.format)
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
    const FORMAT: FormatId = FormatId::new("cadir");
    /// The document states its own depth, so a container-only dump is a
    /// faithful CADIR document.
    const NEEDS: DepthNeed = DepthNeed::Recorded;

    /// CADIR writes the neutral document. Its version identifies the CADIR
    /// schema. The serialized export report uses the `cadir` identity variant,
    /// which has no native dialect target.
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(&self, input: EncodeInput<'_>, (): ()) -> Result<ExportBody, CodecError> {
        let mut bytes = crate::hash::finite_json::to_canonical_json_string(input.ir)
            .map_err(|error| CodecError::Malformed(error.to_string()))?
            .into_bytes();
        bytes.push(b'\n');
        // CADIR is the neutral document itself: there is no container to
        // replay or patch, so this encoder has one path and states it. The
        // serialization carries every arena.
        Ok(ExportBody::synthesized(
            bytes,
            input.ir,
            ArenaCoverage::Complete,
        ))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod test_support;
