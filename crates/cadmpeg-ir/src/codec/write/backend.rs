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
    CensusBasis, EntityCensus, ExportReport, FidelityResolution, LossNote, WritePath,
};
use crate::source_fidelity::SourceFidelity;
use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::target::{TargetDescriptor, TargetRefusal};
use cadmpeg_core::CodecError;

use super::resolve::{resolve_write_request, ResolvedWrite, TargetRequest};

mod domain_sealed {
    pub trait Sealed {}
    impl Sealed for super::DialectFree {}
    impl Sealed for super::Catalog {}
}

/// Where an encoder's targets come from, and what a resolved request looks
/// like for that encoder.
///
/// Implemented only by [`DialectFree`] and [`Catalog`]. A backend names one
/// of them as its [`EncoderBackend::Target`], and `plan_resolved` receives that
/// domain's resolution type and nothing else.
pub trait TargetDomain: domain_sealed::Sealed {
    /// The proof handed to `plan_resolved` for one request.
    type Resolved<'a>;

    /// The static catalog of output flavors this domain lists.
    fn targets(&self) -> &'static [TargetDescriptor];

    /// Resolves one request against this domain; the second element is the
    /// export identity the wrapper stamps, `None` for the neutral document.
    #[doc(hidden)]
    fn resolve<'a>(
        &self,
        ir: &'a CadIr,
        request: TargetRequest<'a>,
        format: &'static str,
    ) -> Result<(Self::Resolved<'a>, Option<DialectId>), CodecError>;
}

/// The neutral representation has no dialect catalog or target identity.
///
/// An encoder in this domain takes [`TargetRequest::Inherit`] only and every
/// export it plans reports no target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DialectFree;

impl TargetDomain for DialectFree {
    type Resolved<'a> = ();

    fn targets(&self) -> &'static [TargetDescriptor] {
        &[]
    }

    fn resolve<'a>(
        &self,
        _ir: &'a CadIr,
        request: TargetRequest<'a>,
        format: &'static str,
    ) -> Result<((), Option<DialectId>), CodecError> {
        match request {
            TargetRequest::Inherit => Ok(((), None)),
            TargetRequest::Explicit(id) => {
                Err(TargetRefusal::unknown_explicit(format, id, &[]).into())
            }
        }
    }
}

/// A native format resolves every request against this complete catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Catalog(pub &'static [TargetDescriptor]);

impl TargetDomain for Catalog {
    type Resolved<'a> = ResolvedWrite<'a>;

    fn targets(&self) -> &'static [TargetDescriptor] {
        self.0
    }

    fn resolve<'a>(
        &self,
        ir: &'a CadIr,
        request: TargetRequest<'a>,
        format: &'static str,
    ) -> Result<(ResolvedWrite<'a>, Option<DialectId>), CodecError> {
        let resolved = resolve_write_request(ir, request, format, self.0)?;
        let identity = resolved.target_id().clone();
        Ok((resolved, Some(identity)))
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
    fn targets(&self) -> &'static [TargetDescriptor];

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

    fn targets(&self) -> &'static [TargetDescriptor] {
        E::TARGET.targets()
    }

    fn plan(
        &self,
        input: EncodeInput<'_>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan, CodecError> {
        let (target, identity) = E::TARGET.resolve(input.ir, request, E::FORMAT)?;
        let body = self.plan_resolved(input, target)?;
        let fidelity = match input.fidelity {
            None => FidelityResolution::NotProvided,
            Some(_) => body.consumption.into(),
        };
        let ExportBody {
            bytes,
            census,
            write_path,
            losses,
            notes,
            consumption: _,
        } = body;
        let report = match identity {
            None => {
                ExportReport::dialect_free(E::FORMAT, census, fidelity, write_path, losses, notes)
            }
            Some(target) => {
                ExportReport::native(target, census, fidelity, write_path, losses, notes)
            }
        };
        Ok(ExportPlan::buffered(report, bytes))
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
/// Reported for every plan; the wrapper maps it onto
/// [`FidelityResolution`] and discards it when the input carried no fidelity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Consumption {
    /// Preserved source content was consumed successfully.
    Replayed,
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
            Consumption::Replayed => Self::Replayed,
            Consumption::NotConsumed => Self::NotConsumed,
            Consumption::Degraded { reason } => Self::Degraded { reason },
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
    /// What the backend did with the fidelity it was given.
    pub consumption: Consumption,
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
            write_path: WritePath::Synthesized,
            losses: Vec::new(),
            notes: Vec::new(),
            consumption: Consumption::NotConsumed,
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
    /// Creates a plan whose bytes have already been materialized.
    ///
    /// The plan reports exactly the report it is given, including fidelity.
    pub fn buffered(report: ExportReport, bytes: Vec<u8>) -> Self {
        Self { report, bytes }
    }

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
