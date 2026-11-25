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
    loss::{LossNamespace, LossNote},
    strictness::Strictness,
    Severity,
};
use crate::source_fidelity::SourceFidelity;
use cadmpeg_core::decode::{Companions, DecodeDepth, ResourceLimit};
use cadmpeg_core::target::{TargetCatalog, TargetRefusal};
use cadmpeg_core::CodecError;

pub mod approximation;
mod cages;
mod canonical;
pub use cages::{cage_bodies, cage_bodies_clause, remove_cage_bodies, without_cage_bodies};
pub mod capacity;
mod carried;
mod coverage;
pub mod edge_ranges;
pub mod edge_recovery;
mod loss;
mod native;
pub use loss::{
    carried_source_losses, definition_placed, dependency_cycle_broken, edit_dropped,
    edit_dropped_note, edit_unverified, evaluation_missing, evaluation_stale, face_partial,
    feature_lowered, fit_dropped, fit_restated, general_tolerance_dropped,
    general_tolerance_restated, link_unwritten, native_records_omitted, output_empty,
    parameter_flattened, parameter_renamed, partial_topology_omitted, record_invalid,
    record_residual_unavailable, rule_invalid, solid_partial, value_quantized, value_unwritable,
};
mod partial;
pub mod saved;
pub use partial::{without_faces_and_bodies, without_partial_topology};
mod scene;
pub mod subset_lowering;
pub mod target;

use carried::{EntityLocation, FindingLocator};
pub use coverage::{
    ArenaCoverage, ArenaDisposition, ArenaDispositions, DesignArenaDispositions,
    ProcessArenaDispositions,
};
pub use native::{NativeDisposition, NativeDispositions};
pub use scene::{SceneOptions, UpAxis};
use target::{DialectFree, TargetDomain, TargetRequest, TargetResolution};

/// Implementation surface for one output format.
///
/// Backends declare one target domain and receive only a request already
/// resolved through that domain. Callers use the sealed [`Encoder`] wrapper.
pub trait EncoderBackend {
    /// Stable output format id, taken from the codec that reads the format.
    const FORMAT: FormatId;

    /// The format's name as this write's losses spell it, as in "the IGES
    /// write omits".
    const NAME: &'static str;

    /// The decode depth this backend plans from, a hint for the caller.
    /// [`Encoder::plan`] refuses no document for its scope; a shallower one
    /// is planned with a note.
    const REQUEST: DecodeDepth;

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

    /// The document [`Encoder::plan`] writes in place of `input`'s: one read
    /// from CADIR JSON with the arenas the wire omits restored, for a writer
    /// that reads them ([`saved::restore_history`]). `None` writes `input`'s
    /// document; `notes` takes why a restore that was attempted left it, and
    /// `losses` the edits a restored document leaves out.
    ///
    /// # Errors
    ///
    /// The refusal of the write.
    fn restore(
        &self,
        _input: EncodeInput<'_>,
        _target: &<Self::Target as TargetDomain>::Resolved<'_>,
        _notes: &mut Vec<String>,
        _losses: &mut Vec<LossNote>,
    ) -> Result<Option<CadIr>, CodecError> {
        Ok(None)
    }
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

    /// The decode depth this encoder plans from, [`EncoderBackend::REQUEST`].
    fn request(&self) -> DecodeDepth;

    /// Plans one export without writing to the destination.
    ///
    /// Never refuses on scope. A document that records less depth than
    /// [`Self::request`] is planned from what it holds, and the report notes
    /// the shortfall. Charges `export:record.invalid` for each Error of
    /// [`EncodeInput::validation`] that the output carries, then refuses with
    /// [`EncodeError::StrictRejected`] where [`EncodeInput::strictness`]
    /// refuses an entry of the completed report (§3.4, Refusals).
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

    fn request(&self) -> DecodeDepth {
        E::REQUEST
    }

    fn plan(
        &self,
        input: EncodeInput<'_>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan, EncodeError> {
        let target = E::TARGET
            .resolve(input.ir, request, E::FORMAT)
            .map_err(EncodeError::from_backend)?;
        let (mut restore_notes, mut restore_losses) = (Vec::new(), Vec::new());
        let restored = self
            .restore(input, &target, &mut restore_notes, &mut restore_losses)
            .map_err(EncodeError::from_backend)?;
        let input = match &restored {
            Some(ir) => EncodeInput { ir, ..input },
            None => input,
        };
        let body = self
            .plan_resolved(input, target.clone())
            .map_err(EncodeError::from_backend)?;
        let ExportBody {
            bytes,
            census,
            write_path,
            coverage,
            native,
            losses: body_losses,
            mut notes,
            written_row,
        } = body;
        let namespace = const {
            match LossNamespace::new(E::FORMAT.as_str()) {
                Ok(namespace) => namespace,
                Err(_) => panic!("an encoder format is a loss namespace"),
            }
        };
        let (identity, mut losses) = target.report_target(
            namespace,
            matches!(write_path, WritePath::Synthesized { .. }),
            written_row,
        );
        losses.extend(body_losses);
        notes.extend(restore_notes);
        losses.extend(restore_losses);
        losses.extend(coverage.omission_losses(input.ir, E::FORMAT.as_str()));
        losses.extend(native.omission_losses(input.ir, E::NAME));
        let same_format = input
            .ir
            .source
            .as_ref()
            .is_some_and(|source| source.format() == E::FORMAT.as_str());
        if same_format && matches!(write_path, WritePath::Synthesized { .. }) {
            losses.extend(loss::dropped_source_loss(input.fidelity));
        }
        if !input.ir.model.omitted_arenas_held() && coverage.reads_wire_omitted() {
            notes.push(format!(
                "the document was read from CADIR, which does not carry the arenas {}; the {} \
                 writer writes records of them, and this write holds none",
                crate::document::ArenaName::WIRE_OMITTED
                    .iter()
                    .map(|arena| arena.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                E::FORMAT
            ));
        }
        let recorded = input.ir.scope().depth();
        if recorded < E::REQUEST {
            notes.push(format!(
                "the document records {recorded} depth; the {} writer plans from {}, so it wrote \
                 what the document holds",
                E::FORMAT,
                E::REQUEST
            ));
        }
        let write_path = write_path.into_report(input.fidelity.is_some());
        let report = match identity {
            None => ExportReport::cadir(census, write_path, losses, notes),
            Some(target) => ExportReport::native(target, census, write_path, losses, notes),
        };
        let mut plan = ExportPlan {
            report,
            bytes,
            format: E::FORMAT,
            coverage,
        };
        if let Some(validation) = input.validation {
            plan.charge_carried_errors(input.ir, validation);
        }
        match input.strictness.first_refused(&plan.report.losses, false) {
            None => Ok(plan),
            Some(entry) => Err(EncodeError::StrictRejected(StrictWriteRejection {
                report: Box::new(plan.report),
                entry,
            })),
        }
    }
}

/// A refused write (§3.4, Refusals).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EncodeError {
    /// The requested target names no catalog entry, the catalog has no
    /// default, or the writer cannot deliver the target from this document.
    #[error("{0}")]
    UnsupportedTarget(Box<TargetRefusal>),
    /// A session ceiling of the caller's limits ran out.
    #[error(
        "resource limit on {:?}: {:?} (scope {:?}, limit {}, used {}, requested {})",
        .0.dimension, .0.reason, .0.scope, .0.limit, .0.used, .0.additional
    )]
    ResourceLimit(ResourceLimit),
    /// A write or seek on the destination failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The caller's [`EncodeInput::strictness`] refused an entry of the
    /// completed export report. Nothing is written.
    #[error("{0}")]
    StrictRejected(StrictWriteRejection),
    /// Interim: a backend refusal that §3.4 states as a report entry or as
    /// another variant, kept until each writer moves to that form.
    #[error(transparent)]
    Backend(CodecError),
}

impl EncodeError {
    fn from_backend(error: CodecError) -> Self {
        match error {
            CodecError::UnsupportedTarget(refusal) => Self::UnsupportedTarget(refusal),
            CodecError::ResourceLimit(limit) => Self::ResourceLimit(limit),
            CodecError::Io(error) => Self::Io(error),
            error => Self::Backend(error),
        }
    }
}

/// The strictness of an [`EncodeInput`] that states none: it refuses nothing.
static PERMISSIVE: Strictness = Strictness {
    pinned: false,
    floor: None,
    codes: std::collections::BTreeSet::new(),
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
    /// Scene settings for an encoder of a scene format.
    pub scene: SceneOptions,
    /// The form a writer to a target that holds no feature without its state
    /// gives the features whose states the document lacks (§3.4, Features
    /// without their state). Every other writer ignores it.
    pub missing_states: MissingStateForm,
    /// The report entries on which the caller refuses the write (§3.4,
    /// Refusals). The default refuses none.
    pub strictness: &'a Strictness,
    /// Interim: the caller's validation of the document. The sealed wrapper
    /// charges `export:record.invalid` for each of its Errors that the output
    /// carries (§3.4, Refusals).
    pub validation: Option<&'a ValidationReport>,
}

/// The form a writer gives features whose states the target needs and the
/// document lacks, for a target that holds no feature without its state
/// (§3.4, Features without their state).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MissingStateForm {
    /// The current geometry is written exactly.
    #[default]
    GeometryTrue,
    /// The stated run is written with its states, and the later features
    /// follow the timeline marker rolled back.
    HistoryTrue,
}

impl<'a> EncodeInput<'a> {
    /// Borrows a document and its decode-time fidelity for one export.
    #[must_use]
    pub const fn new(ir: &'a CadIr, fidelity: Option<&'a SourceFidelity>) -> Self {
        Self {
            ir,
            fidelity,
            assets: None,
            scene: SceneOptions::DEFAULT,
            missing_states: MissingStateForm::GeometryTrue,
            strictness: &PERMISSIVE,
            validation: None,
        }
    }

    /// Attaches the source of the files that external assets name.
    #[must_use]
    pub const fn with_assets(mut self, assets: &'a dyn Companions) -> Self {
        self.assets = Some(assets);
        self
    }

    /// Selects the scene settings of the export.
    #[must_use]
    pub const fn with_scene(mut self, scene: SceneOptions) -> Self {
        self.scene = scene;
        self
    }

    /// Selects the form of features whose states the target needs and the
    /// document lacks.
    #[must_use]
    pub const fn with_missing_states(mut self, form: MissingStateForm) -> Self {
        self.missing_states = form;
        self
    }

    /// Selects the report entries on which the caller refuses the write.
    #[must_use]
    pub const fn with_strictness(mut self, strictness: &'a Strictness) -> Self {
        self.strictness = strictness;
        self
    }

    /// Attaches the caller's validation of the document.
    #[must_use]
    pub const fn with_validation(mut self, validation: &'a ValidationReport) -> Self {
        self.validation = Some(validation);
        self
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
    /// What the payload does with each native record. The wrapper charges
    /// `export:native.records-omitted` for every record this drops.
    pub native: NativeDispositions,
    /// Losses charged while planning.
    pub losses: Vec<LossNote>,
    /// Free-form notes.
    pub notes: Vec<String>,
    /// The catalog row the payload is, where the writer wrote another row
    /// than the resolution names; `None` for the resolved row.
    pub written_row: Option<target::WrittenRow>,
}

impl ExportBody {
    /// A synthesized payload counted on IR arenas that consumes no fidelity.
    #[must_use]
    pub fn synthesized(
        bytes: Vec<u8>,
        ir: &CadIr,
        coverage: ArenaCoverage,
        native: NativeDispositions,
    ) -> Self {
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
            native,
            losses: Vec::new(),
            notes: Vec::new(),
            written_row: None,
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
    /// by complete coverage, by the CADIR encoder, which writes every native
    /// namespace, or by their own format's encoder; any other backend reads
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

    /// Charges `export:record.invalid` for each Error of `report` that this
    /// payload carries ([`Self::carried_errors`]): with the kind of the
    /// record's model arena where the finding names one, and naming the rule
    /// otherwise (§3.4, Refusals).
    fn charge_carried_errors(&mut self, ir: &CadIr, report: &ValidationReport) {
        let carried = self.carried_errors(ir, report);
        let locator = FindingLocator::new(ir, carried.iter().copied());
        let notes: Vec<LossNote> = carried
            .iter()
            .map(|finding| {
                let kind = locator.scope(finding).and_then(|locations| {
                    locations.into_iter().find_map(|location| match location {
                        EntityLocation::Model(kind) => Some(kind),
                        EntityLocation::Native(_) => None,
                    })
                });
                match (kind, finding.entity.as_deref()) {
                    (Some(kind), Some(entity)) => record_invalid(
                        kind,
                        format!("{entity}: {} ({})", finding.message, finding.rule),
                    ),
                    _ => rule_invalid(format!("{}: {}", finding.rule, finding.message)),
                }
            })
            .collect();
        self.report.losses.extend(notes);
    }

    fn carries_location(&self, location: EntityLocation<'_>) -> bool {
        match location {
            EntityLocation::Model(kind) => self.coverage.carries(kind),
            EntityLocation::Native(namespace) => {
                self.coverage == ArenaCoverage::Complete
                    || self.format == CadirEncoder::FORMAT
                    || namespace == self.format.as_str()
            }
        }
    }

    /// Writes the planned payload and returns the unchanged plan-time report.
    pub fn write_to(self, writer: &mut dyn Write) -> Result<ExportReport, EncodeError> {
        writer.write_all(&self.bytes)?;
        Ok(self.report)
    }
}

/// A write refused by the caller's [`Strictness`]: the completed export
/// report and the index of the first loss it refuses (§3.4, Refusals).
/// Only the sealed [`Encoder::plan`] wrapper constructs it.
#[derive(Debug, Clone, PartialEq)]
pub struct StrictWriteRejection {
    report: Box<ExportReport>,
    entry: usize,
}

impl StrictWriteRejection {
    /// The completed export report of the refused write.
    #[must_use]
    pub fn report(&self) -> &ExportReport {
        &self.report
    }

    /// The loss that refused the write.
    #[must_use]
    pub fn loss(&self) -> &LossNote {
        &self.report.losses[self.entry]
    }

    /// The index of [`Self::loss`] in the report's losses.
    #[must_use]
    pub const fn entry(&self) -> usize {
        self.entry
    }
}

impl std::fmt::Display for StrictWriteRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let loss = self.loss();
        write!(
            f,
            "the caller's strictness refuses the write at {}: {}",
            crate::report::strictness::code_name(&loss.code),
            loss.message
        )
    }
}

impl std::error::Error for StrictWriteRejection {}

/// Encoder for canonical versioned CADIR JSON.
#[derive(Debug, Clone, Copy, Default)]
pub struct CadirEncoder;

impl EncoderBackend for CadirEncoder {
    const FORMAT: FormatId = FormatId::new("cadir");
    const NAME: &'static str = "CADIR";
    /// The document states its own depth, so a container-only dump is a
    /// faithful CADIR document.
    const REQUEST: DecodeDepth = DecodeDepth::Full;

    /// CADIR writes the neutral document. Its version identifies the CADIR
    /// schema. The serialized export report uses the `cadir` identity variant,
    /// which has no native dialect target.
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(&self, input: EncodeInput<'_>, (): ()) -> Result<ExportBody, CodecError> {
        let malformed = |error: crate::hash::finite_json::CanonicalJsonError| {
            CodecError::Malformed(error.to_string())
        };
        let (canonical, mut losses) = match canonical::canonical_form(input.ir)? {
            Some((ir, losses)) => (Some(ir), losses),
            None => (None, Vec::new()),
        };
        let source = canonical.as_ref().unwrap_or(input.ir);
        let (mut bytes, measured) =
            crate::hash::measured_canonical_json(source).map_err(malformed)?;
        let mut native = NativeDispositions::written();
        let written;
        let ir = if measured.records.is_empty() && measured.outside.is_empty() {
            source
        } else {
            let (ir, unwritable) = within_the_bounds(source, &measured, &mut native)?;
            losses.extend(unwritable);
            bytes = crate::hash::finite_json::to_canonical_json_string(&ir)
                .map_err(malformed)?
                .into_bytes();
            written = ir;
            &written
        };
        bytes.push(b'\n');
        // CADIR is the neutral document itself: there is no container to
        // replay or patch, so this encoder has one path and states it.
        let mut body = ExportBody::synthesized(
            bytes,
            ir,
            ArenaCoverage::Declared(coverage::cadir_dispositions()),
            native,
        );
        body.losses = losses;
        Ok(body)
    }
}

/// A copy of `ir` within the bounds of §2.5 (§3.4). Each value outside every
/// record past a bound is written as an unstated value, each expression past
/// its bound is written in its partial form, and each record still past a
/// bound is dropped with each record that needs it, as §3.3 drops a record;
/// `native` declares each native record so dropped. A copy that holds no
/// record, of a document that held one, is the empty document, with
/// `export:output.empty`.
fn within_the_bounds(
    ir: &CadIr,
    measured: &crate::bounds::Measured,
    native: &mut NativeDispositions,
) -> Result<(CadIr, Vec<LossNote>), CodecError> {
    let holds_records = |ir: &CadIr| {
        ir.entity_count() > 0
            || ir.native.0.values().any(|namespace| {
                namespace
                    .arenas()
                    .values()
                    .any(|records| !records.is_empty())
            })
    };
    let mut ir = ir.clone();
    let held = holds_records(&ir);
    let mut losses = Vec::new();
    for unstated in crate::bounds::unstate_outside(&mut ir, &measured.outside)? {
        losses.extend(value_unwritable(
            CadirEncoder::NAME,
            &format!("writes {}", unstated.what),
            [unstated.field],
        ));
    }
    let (restated, records) = restate_expressions_past_the_bound(&mut ir, &measured.records)?;
    losses.extend(restated);
    losses.extend(drop_records_past_the_bounds(&mut ir, &records, native)?);
    if held && !holds_records(&ir) {
        losses.push(output_empty(
            "the written CADIR document holds no record: each record lies past a bound of \
             §2.5 or needs one that does",
        ));
        ir = CadIr::empty();
    }
    Ok((ir, losses))
}

/// States `Source` with no tokens each expression past its bound in the
/// records of `records` that pass that bound, the partial form §3.3 gives
/// an expression, so that each such record and each record that needs it
/// stays. Each restated record charges one `export:value.unwritable` that
/// names it. Returns those losses and the records of `ir` still past a
/// bound.
fn restate_expressions_past_the_bound(
    ir: &mut CadIr,
    records: &std::collections::BTreeMap<crate::bounds::Place, crate::bounds::Bound>,
) -> Result<
    (
        Vec<LossNote>,
        std::collections::BTreeMap<crate::bounds::Place, crate::bounds::Bound>,
    ),
    CodecError,
> {
    ir.finalize();
    let seeds: Vec<crate::give_up::Seed> = records
        .iter()
        .filter(|(_, bound)| **bound == crate::bounds::Bound::Expression)
        .filter_map(|(place, _)| ir.record_at(place))
        .collect();
    let restated = crate::give_up::restate_past_the_bound(ir, &seeds);
    if restated.is_empty() {
        return Ok((Vec::new(), records.clone()));
    }
    let losses = restated
        .into_iter()
        .filter_map(|seed| {
            let reason = format!(
                "writes each expression of record {} of arena {} deeper than the {} levels of \
                 an expression as Source with no tokens",
                seed.id,
                seed.arena,
                crate::bounds::MAX_EXPRESSION_DEPTH
            );
            value_unwritable(CadirEncoder::NAME, &reason, [seed.id])
        })
        .collect();
    let (_, measured) = crate::hash::measured_canonical_json(&*ir)
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    Ok((losses, measured.records))
}

/// Drops from `ir` each record of `records`, which it holds past a bound of
/// §2.5, and each record that needs one, as §3.3 drops a record (§3.4).
/// `native` declares each native record so dropped, which the wrapper
/// charges as `export:native.records-omitted`. The other records of each
/// drop charge one `export:value.unwritable` that names them, the bound, and
/// the records marked with them. A record of a native `unknowns` arena,
/// which the wrapper never charges, counts as one of those other records.
fn drop_records_past_the_bounds(
    ir: &mut CadIr,
    records: &std::collections::BTreeMap<crate::bounds::Place, crate::bounds::Bound>,
    native: &mut NativeDispositions,
) -> Result<Vec<LossNote>, CodecError> {
    if records.is_empty() {
        return Ok(Vec::new());
    }
    ir.finalize();
    let bounds: std::collections::BTreeMap<crate::give_up::Seed, crate::bounds::Bound> = records
        .iter()
        .filter_map(|(place, bound)| ir.record_at(place).map(|seed| (seed, *bound)))
        .collect();
    let native_codec: std::collections::HashMap<String, String> = ir
        .native
        .0
        .iter()
        .flat_map(|(codec, namespace)| {
            namespace
                .arenas()
                .iter()
                .filter(|(arena, _)| arena.as_str() != native::UNKNOWNS_ARENA)
                .flat_map(|(_, records)| records)
                .map(move |record| (record.id().to_owned(), codec.as_str().to_owned()))
        })
        .collect();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    let removed = crate::give_up::drop_records(&ctx, ir, bounds.keys().cloned().collect())?;
    let mut losses = Vec::new();
    for (seed, released) in removed {
        let bound = bounds
            .get(&seed)
            .map_or_else(String::new, |bound| bound.describe());
        let marked = released
            .marked
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let (dependents, others): (Vec<String>, Vec<String>) = released
            .dropped
            .into_iter()
            .partition(|id| native_codec.contains_key(id));
        for id in &dependents {
            native.drop_records(
                &native_codec[id],
                &format!("that need record {}, which {bound}", seed.id),
                [id.as_str()],
            );
        }
        let Some(codec) = native_codec.get(&seed.id) else {
            losses.extend(value_unwritable(
                CadirEncoder::NAME,
                &format!(
                    "drops record {} of arena {}, which {bound}, with each record that needs \
                     it; marked=[{marked}]",
                    seed.id, seed.arena
                ),
                std::iter::once(seed.id).chain(others),
            ));
            continue;
        };
        native.drop_records(codec, &format!("which {bound}"), [seed.id.as_str()]);
        // The seed is charged as an omitted native record, so this names
        // only the other records its drop drops, or else marks.
        let (what, ids) = if others.is_empty() {
            ("marks", released.marked.into_iter().collect())
        } else {
            ("drops", others)
        };
        losses.extend(value_unwritable(
            CadirEncoder::NAME,
            &format!(
                "{what} these records, which need a native record of arena {} that {bound}; \
                 marked=[{marked}]",
                seed.arena
            ),
            ids,
        ));
    }
    Ok(losses)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod test_support;
