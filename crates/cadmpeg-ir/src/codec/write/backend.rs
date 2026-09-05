// SPDX-License-Identifier: Apache-2.0
//! The sealed encoder contract and its blanket implementation.
//!
//! A backend declares the domain its targets come from, receives only a
//! request resolved through that domain, and returns a bare [`ExportBody`].
//! The sealed [`Encoder::plan`] wrapper is the single author of the export's
//! identity and fidelity resolution: a backend cannot report a format, a
//! target, or a fidelity state other than the one it was given.

use std::io::Write;

use crate::document::CadIr;
use crate::report::{
    CensusBasis, EntityCensus, ExportReport, FidelityResolution, LossNote,
    WritePath as ReportWritePath,
};
use crate::source_fidelity::SourceFidelity;
use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::target::{TargetCatalog, TargetRefusal};
use cadmpeg_core::CodecError;

use super::resolve::{resolve_write_request, ResolvedWrite, TargetRequest};

mod domain_sealed {
    pub trait Sealed {}
    impl Sealed for super::DialectFree {}
    impl Sealed for super::Catalog {}
}

mod resolution_sealed {
    pub trait Sealed {}
    impl Sealed for () {}
    impl Sealed for super::ResolvedWrite<'_> {}
}

/// A target-domain resolution that states the export identity it proves.
pub trait TargetResolution: resolution_sealed::Sealed {
    /// The native target identity, or `None` for the neutral document.
    fn export_target(&self) -> Option<&DialectId>;
}

impl TargetResolution for () {
    fn export_target(&self) -> Option<&DialectId> {
        None
    }
}

impl TargetResolution for ResolvedWrite<'_> {
    fn export_target(&self) -> Option<&DialectId> {
        Some(self.target_id())
    }
}

/// Where an encoder's targets come from, and what a resolved request looks
/// like for that encoder.
///
/// Implemented only by [`DialectFree`] and [`Catalog`]. A backend names one
/// of them as its [`EncoderBackend::Target`], and `plan_resolved` receives that
/// domain's resolution type and nothing else.
pub trait TargetDomain: domain_sealed::Sealed {
    /// The proof handed to `plan_resolved` for one request.
    type Resolved<'a>: TargetResolution;

    /// The static catalog of output flavors this domain lists.
    fn targets(&self) -> TargetCatalog;

    /// Resolves one request against this domain.
    #[doc(hidden)]
    fn resolve<'a>(
        &self,
        ir: &'a CadIr,
        request: TargetRequest<'a>,
        format: &'static str,
    ) -> Result<Self::Resolved<'a>, CodecError>;
}

/// The neutral representation has no dialect catalog or target identity.
///
/// An encoder in this domain takes [`TargetRequest::Inherit`] only and every
/// export it plans reports no target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DialectFree;

impl TargetDomain for DialectFree {
    type Resolved<'a> = ();

    fn targets(&self) -> TargetCatalog {
        TargetCatalog::EMPTY
    }

    fn resolve<'a>(
        &self,
        _ir: &'a CadIr,
        request: TargetRequest<'a>,
        format: &'static str,
    ) -> Result<(), CodecError> {
        match request {
            TargetRequest::Inherit => Ok(()),
            TargetRequest::Explicit(id) => {
                Err(TargetRefusal::unknown_explicit(format, id, TargetCatalog::EMPTY).into())
            }
        }
    }
}

/// A native format resolves every request against this complete catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Catalog(TargetCatalog);

impl Catalog {
    /// Builds a native target domain whose optional default indexes one of
    /// `targets`.
    #[must_use]
    pub const fn new(
        targets: &'static [cadmpeg_core::target::TargetDescriptor],
        default: Option<usize>,
    ) -> Self {
        Self(TargetCatalog::new(targets, default))
    }
}

impl TargetDomain for Catalog {
    type Resolved<'a> = ResolvedWrite<'a>;

    fn targets(&self) -> TargetCatalog {
        self.0
    }

    fn resolve<'a>(
        &self,
        ir: &'a CadIr,
        request: TargetRequest<'a>,
        format: &'static str,
    ) -> Result<ResolvedWrite<'a>, CodecError> {
        resolve_write_request(ir, request, format, self.0)
    }
}

/// Implementation surface for one output format.
///
/// Backends declare one target domain and receive only a request already
/// resolved through that domain. Callers use the sealed [`Encoder`] wrapper.
pub trait EncoderBackend {
    /// Stable output format id.
    const FORMAT: &'static str;

    /// The domain this backend's targets come from.
    type Target: TargetDomain;

    /// The domain value: [`DialectFree`] or a [`Catalog`] of this format's
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
    fn id(&self) -> &'static str;

    /// The static catalog of output flavors this encoder can produce.
    ///
    /// Whether a given input reaches one is resolution's answer, not the
    /// catalog's: a patch-only writer's row is reachable only from a retained
    /// source of that flavor, and `plan` refuses by name where it cannot
    /// deliver. Preservation of dialects outside the catalog is not listed
    /// here; [`TargetRequest::Inherit`] asks for it. Ids come from this
    /// encoder's own format namespace only.
    fn targets(&self) -> TargetCatalog;

    /// Plans one export without writing to the destination.
    fn plan(
        &self,
        input: EncodeInput<'_>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan, CodecError>;
}

impl<E: EncoderBackend> Encoder for E {
    fn id(&self) -> &'static str {
        E::FORMAT
    }

    fn targets(&self) -> TargetCatalog {
        E::TARGET.targets()
    }

    fn plan(
        &self,
        input: EncodeInput<'_>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan, CodecError> {
        let target = E::TARGET.resolve(input.ir, request, E::FORMAT)?;
        let identity = target.export_target().cloned();
        let body = self.plan_resolved(input, target)?;
        let ExportBody {
            bytes,
            census,
            write_path,
            losses,
            notes,
        } = body;
        let report = match identity {
            None => {
                ExportReport::cadir(census, write_path, input.fidelity.is_some(), losses, notes)
            }
            Some(target) => ExportReport::native(
                target,
                census,
                write_path,
                input.fidelity.is_some(),
                losses,
                notes,
            ),
        };
        Ok(ExportPlan { report, bytes })
    }
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
            Consumption::NotConsumed => Self::NotConsumed,
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
    pub(crate) fn into_report(
        self,
        fidelity_provided: bool,
    ) -> (ReportWritePath, FidelityResolution) {
        let (write_path, fidelity) = match self {
            Self::Synthesized { consumption } => (ReportWritePath::Synthesized, consumption.into()),
            Self::Patched {
                consumption: PatchConsumption::Replayed,
            } => (ReportWritePath::Patched, FidelityResolution::Replayed),
            Self::Patched {
                consumption: PatchConsumption::Independent(consumption),
            } => (ReportWritePath::Patched, consumption.into()),
            Self::VerbatimReplay => (
                ReportWritePath::VerbatimReplay,
                FidelityResolution::Replayed,
            ),
        };
        if fidelity_provided {
            (write_path, fidelity)
        } else {
            (write_path, FidelityResolution::NotProvided)
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
    /// Losses charged while planning.
    pub losses: Vec<LossNote>,
    /// Free-form notes.
    pub notes: Vec<String>,
}

impl ExportBody {
    /// A synthesized payload counted on IR arenas that consumes no fidelity.
    #[must_use]
    pub fn synthesized(bytes: Vec<u8>, ir: &CadIr) -> Self {
        Self {
            bytes,
            census: EntityCensus {
                basis: CensusBasis::IrArenas,
                counts: ir.census(),
            },
            write_path: WritePath::Synthesized {
                consumption: Consumption::NotConsumed,
            },
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
    report: ExportReport,
    bytes: Vec<u8>,
}

impl ExportPlan {
    /// Returns the complete plan-time export report.
    pub fn report(&self) -> &ExportReport {
        &self.report
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
    const FORMAT: &'static str = "cadir";

    /// CADIR is the neutral document, not a native format: its version is
    /// data about cadmpeg, never a dialect, and `ExportReport::target` is
    /// `None` on every CADIR write.
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(&self, input: EncodeInput<'_>, (): ()) -> Result<ExportBody, CodecError> {
        let mut bytes = serde_json::to_vec_pretty(input.ir)
            .map_err(|error| CodecError::Malformed(error.to_string()))?;
        bytes.push(b'\n');
        // CADIR is the neutral document itself: there is no container to
        // replay or patch, so this encoder has one path and states it.
        Ok(ExportBody::synthesized(bytes, input.ir))
    }
}
