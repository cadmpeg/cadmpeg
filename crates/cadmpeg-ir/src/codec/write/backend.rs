// SPDX-License-Identifier: Apache-2.0
//! The sealed encoder contract and its blanket implementation.

use std::io::Write;

use crate::document::CadIr;
use crate::report::{CensusBasis, EntityCensus, ExportReport, FidelityResolution, WritePath};
use crate::source_fidelity::SourceFidelity;
use cadmpeg_core::target::{TargetDescriptor, TargetRefusal};
use cadmpeg_core::CodecError;

use super::resolve::{resolve_write_request, ResolvedWrite, TargetRequest};

/// How an encoder resolves caller target requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderTargetDomain {
    /// The neutral representation has no dialect catalog or target identity.
    DialectFree,
    /// A native format resolves every request against this complete catalog.
    Catalog(&'static [TargetDescriptor]),
}

impl EncoderTargetDomain {
    const fn targets(self) -> &'static [TargetDescriptor] {
        match self {
            Self::DialectFree => &[],
            Self::Catalog(targets) => targets,
        }
    }
}

/// A target request resolved by the sealed encoder boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedEncoderTarget {
    /// The dialect-free encoder received its only valid request, inherit.
    DialectFree,
    /// A native request resolved against the backend's declared catalog.
    Native(ResolvedWrite),
}

/// Implementation surface for one output format.
///
/// Backends declare one target domain and receive only a request already
/// resolved through that domain. Callers use the sealed [`Encoder`] wrapper.
pub trait EncoderBackend {
    /// Stable output format id.
    const FORMAT: &'static str;

    /// The target grammar this backend implements.
    const TARGET_DOMAIN: EncoderTargetDomain;

    /// Plans a write from the request resolved by [`Encoder::plan`].
    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedEncoderTarget,
    ) -> Result<ExportPlan, CodecError>;
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
        E::TARGET_DOMAIN.targets()
    }

    fn plan(
        &self,
        input: EncodeInput<'_>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan, CodecError> {
        let target = match E::TARGET_DOMAIN {
            EncoderTargetDomain::DialectFree => match request {
                TargetRequest::Inherit => ResolvedEncoderTarget::DialectFree,
                TargetRequest::Explicit(id) => {
                    return Err(TargetRefusal::unknown_explicit(E::FORMAT, id, &[]).into());
                }
            },
            EncoderTargetDomain::Catalog(targets) => ResolvedEncoderTarget::Native(
                resolve_write_request(input.ir, request, E::FORMAT, targets)?,
            ),
        };
        let expected_target = match &target {
            ResolvedEncoderTarget::DialectFree => None,
            ResolvedEncoderTarget::Native(write) => Some(write.dialect().clone()),
        };
        let plan = self.plan_resolved(input, target)?;
        if plan.report().format() != E::FORMAT {
            return Err(CodecError::ContractViolation {
                codec: E::FORMAT,
                operation: "plan",
                expected: E::FORMAT.to_owned(),
                reported: plan.report().format().to_owned(),
            });
        }
        if plan.report().target() != expected_target.as_ref() {
            return Err(CodecError::ContractViolation {
                codec: E::FORMAT,
                operation: "plan target",
                expected: expected_target
                    .as_ref()
                    .map_or_else(|| "none".to_owned(), ToString::to_string),
                reported: plan
                    .report()
                    .target()
                    .map_or_else(|| "none".to_owned(), ToString::to_string),
            });
        }
        Ok(plan)
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

/// A fully reported export awaiting its destination write.
///
/// The plan owns the complete payload. Atomic file staging belongs to the
/// artifact store; [`Self::write_to`] also supports non-file sinks.
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

    /// Empty. CADIR is the neutral document, not a native format: its version
    /// is data about cadmpeg, never a dialect, and `ExportReport::target` is
    /// `None` on every CADIR write. An encoder with no catalog takes
    /// [`TargetRequest::Inherit`] only.
    const TARGET_DOMAIN: EncoderTargetDomain = EncoderTargetDomain::DialectFree;

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedEncoderTarget,
    ) -> Result<ExportPlan, CodecError> {
        let ResolvedEncoderTarget::DialectFree = target else {
            unreachable!("a dialect-free encoder receives only dialect-free resolutions")
        };
        let report = ExportReport::cadir(
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
        let mut bytes = serde_json::to_vec_pretty(input.ir)
            .map_err(|error| CodecError::Malformed(error.to_string()))?;
        bytes.push(b'\n');
        Ok(ExportPlan::buffered(report, bytes))
    }
}
