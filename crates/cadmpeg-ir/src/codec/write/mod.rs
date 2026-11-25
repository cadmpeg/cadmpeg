// SPDX-License-Identifier: Apache-2.0
//! The sealed encoder contract and its blanket implementation.
//!
//! A backend declares the domain its targets come from, receives only a
//! request resolved through that domain, and returns a bare [`ExportBody`].
//! The sealed [`Encoder::plan`] wrapper is the single author of the export's
//! identity and fidelity resolution: a backend cannot report a format, a
//! target, or a fidelity state other than the one it was given.

use std::collections::BTreeMap;
use std::io::Write;

use crate::codec::FormatId;
use crate::document::CadIr;
use crate::ids::ProductDefinitionId;
use crate::products::{ProductDefinition, Storage};
use crate::report::{
    check::{Finding, ValidationReport},
    export::{
        CensusBasis, EntityCensus, ExportReport, FidelityResolution, ReplayFidelity,
        SynthesisCause, SynthesisFidelity, WritePath as ReportWritePath,
    },
    loss::LossNote,
    strictness::Strictness,
    Severity,
};
use crate::source_fidelity::SourceFidelity;
use cadmpeg_core::decode::{
    Companions, DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ResourceLimit,
    ResourceLimits,
};
use cadmpeg_core::target::{TargetCatalog, TargetRefusal};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;

pub mod approximation;
mod cages;
pub use cages::{cage_bodies, cage_bodies_clause, remove_cage_bodies, without_cage_bodies};
pub mod capacity;
mod carried;
mod coverage;
pub mod edge_ranges;
pub mod edge_recovery;
mod encode_set;
mod loss;
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
pub use scene::{SceneOptions, UpAxis};
use target::{DialectFree, TargetDomain, TargetRequest, TargetResolution};

/// Implementation surface for one output format.
///
/// Backends declare one target domain and receive only a request already
/// resolved through that domain. Callers use the sealed [`Encoder`] wrapper.
pub trait EncoderBackend {
    /// Stable output format id, taken from the codec that reads the format.
    const FORMAT: FormatId;

    /// The decode depth this backend plans from, a hint for the caller.
    /// [`Encoder::plan`] refuses no document for its scope; a shallower one
    /// is planned with a note.
    const REQUEST: DecodeDepth;

    /// Whether the writer has a patch or replay path (§3.4, Paths). For such
    /// a writer [`ExportReport::synthesis_cause`] records why a file of a
    /// same-format source was synthesized.
    const REUSES_SOURCE: bool;

    /// Whether the format stores one definition per file (§3.4, Encode
    /// sets). For such a backend [`Encoder::plan`] produces one output per
    /// `File` definition of the document, the root definition included, and
    /// plans each from the part of the document that output holds.
    const DEFINITION_PER_FILE: bool = false;

    /// The domain this backend's targets come from.
    type Target: TargetDomain;

    /// The domain value: [`DialectFree`] or a [`target::Catalog`] of this format's
    /// output flavors.
    const TARGET: Self::Target;

    /// Plans a write from the request resolved by [`Encoder::plan`], in the
    /// encode's session `ctx`: every ceiling of the caller's limits is
    /// charged to it, so that the cumulative ceilings hold per encode.
    fn plan_resolved(
        &self,
        ctx: &DecodeContext<'_>,
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
        _ctx: &DecodeContext<'_>,
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

    /// The definitions whose outputs [`Self::plan`] produces from `ir`, the
    /// root definition's first; `None` keys the output of a document that
    /// states no root definition. The caller names each one in
    /// [`EncodeInput::outputs`] (§3.4, Encode sets).
    fn output_definitions(&self, ir: &CadIr) -> Vec<Option<ProductDefinitionId>>;

    /// Plans one export of [`EncodeInput::target`] without writing to the
    /// destination.
    ///
    /// Refuses with [`EncodeError::OutputUnnamed`] before planning where
    /// [`EncodeInput::outputs`] names no output the writer produces. Never
    /// refuses on scope. A document that records less depth than
    /// [`Self::request`] is planned from what it holds, and the report notes
    /// the shortfall. Charges `export:record.invalid` for each Error of
    /// [`EncodeInput::validation`] that the output carries, then refuses with
    /// [`EncodeError::StrictRejected`] where [`EncodeInput::strictness`]
    /// refuses an entry of the completed report (§3.4, Refusals).
    fn plan(&self, input: EncodeInput<'_>) -> Result<ExportPlan, EncodeError>;
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

    fn output_definitions(&self, ir: &CadIr) -> Vec<Option<ProductDefinitionId>> {
        output_definitions(ir, E::DEFINITION_PER_FILE)
            .into_iter()
            .map(|definition| definition.map(|definition| definition.id.clone()))
            .collect()
    }

    fn plan(&self, input: EncodeInput<'_>) -> Result<ExportPlan, EncodeError> {
        let outputs = planned_outputs(input.ir, input.outputs, E::DEFINITION_PER_FILE)?;
        let unchecked = EncodeInput {
            validation: None,
            ..input
        };
        let arena = DecodeArena::new();
        let policy = DecodePolicy {
            limits: input.limits,
        };
        let session = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
        let (report, coverage, outputs, payloads) = if let [_] = outputs.as_slice() {
            let (report, coverage, bytes) = plan_output(self, &session, unchecked)?;
            (report, coverage, outputs, vec![bytes])
        } else {
            plan_encode_set(self, &session, unchecked, outputs)?
        };
        let mut plan = ExportPlan {
            report,
            payloads,
            format: E::FORMAT,
            coverage,
            outputs,
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

/// Plans the output of one file from `input`'s document, in the encode's
/// session `ctx`: its report, coverage, and payload. The caller charges
/// validation and strictness.
fn plan_output<E: EncoderBackend>(
    encoder: &E,
    ctx: &DecodeContext<'_>,
    input: EncodeInput<'_>,
) -> Result<(ExportReport, ArenaCoverage, Vec<u8>), EncodeError> {
    let target = E::TARGET
        .resolve(input.ir, input.target, E::FORMAT)
        .map_err(EncodeError::from_backend)?;
    let identity = target.export_target().cloned();
    let synthesized_identity = target.synthesized_target().cloned();
    let (mut restore_notes, mut restore_losses) = (Vec::new(), Vec::new());
    let restored = encoder
        .restore(ctx, input, &target, &mut restore_notes, &mut restore_losses)
        .map_err(EncodeError::from_backend)?;
    let input = match &restored {
        Some(ir) => EncodeInput { ir, ..input },
        None => input,
    };
    let body = encoder
        .plan_resolved(ctx, input, target)
        .map_err(EncodeError::from_backend)?;
    let ExportBody {
        bytes,
        census,
        write_path,
        coverage,
        mut losses,
        mut notes,
        synthesis_cause,
    } = body;
    notes.extend(restore_notes);
    losses.extend(restore_losses);
    losses.extend(coverage.omission_losses(input.ir, E::FORMAT.as_str()));
    losses.extend(placed_definition_loss(
        input.ir,
        &coverage,
        E::FORMAT.as_str(),
    ));
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
    let synthesis_cause = match &write_path {
        WritePath::Synthesized { consumption } if E::REUSES_SOURCE && same_format => {
            Some(synthesis_cause.unwrap_or_else(|| consumption.patch_declined(input.fidelity)))
        }
        WritePath::Synthesized { .. } | WritePath::Patched { .. } | WritePath::VerbatimReplay => {
            None
        }
    };
    let identity = match write_path {
        WritePath::Synthesized { .. } => synthesized_identity,
        WritePath::Patched { .. } | WritePath::VerbatimReplay => identity,
    };
    let write_path = write_path.into_report(input.fidelity.is_some());
    let mut report = match identity {
        None => ExportReport::cadir(census, write_path, losses, notes),
        Some(target) => ExportReport::native(target, census, write_path, losses, notes),
    };
    report.synthesis_cause = synthesis_cause;
    Ok((report, coverage, bytes))
}

/// Plans each of `outputs` from the part of `input`'s document that it holds
/// ([`encode_set::output_document`]), in the encode's session `ctx`, and
/// merges their reports into the first written output's, the root's where
/// it is written: the coverage is that output's, and the losses and notes
/// are every output's. Only the root output reads the decode's sidecar.
///
/// An output whose plan fails is not written, and the report charges
/// `export:output.unwritten` naming its definition, while another output is
/// written. The encode refuses where no output can be written, with the
/// first output's refusal, and where the session runs out (§3.4, Refusals).
/// Returns the outputs written with their payloads.
fn plan_encode_set<E: EncoderBackend>(
    encoder: &E,
    ctx: &DecodeContext<'_>,
    input: EncodeInput<'_>,
    outputs: Vec<PlannedOutput>,
) -> Result<PlannedSet, EncodeError> {
    let names: OutputNames = outputs
        .iter()
        .filter_map(|output| Some((output.definition.clone()?, output.name.clone()?)))
        .collect();
    let root = outputs.first().and_then(|output| output.definition.clone());
    let mut planned: Option<(ExportReport, ArenaCoverage)> = None;
    let mut written = Vec::with_capacity(outputs.len());
    let mut payloads = Vec::with_capacity(outputs.len());
    let mut unwritten = Vec::new();
    for output in outputs {
        let definition = output.definition.as_ref();
        let planned_output =
            encode_set::output_document(ctx, input.ir, definition, root.as_ref(), &names)
                .map_err(EncodeError::from_backend)
                .and_then(|document| {
                    let fidelity = if definition == root.as_ref() {
                        input.fidelity
                    } else {
                        None
                    };
                    plan_output(
                        encoder,
                        ctx,
                        EncodeInput {
                            ir: &document,
                            fidelity,
                            ..input
                        },
                    )
                });
        let (report, coverage, bytes) = match planned_output {
            Ok(planned_output) => planned_output,
            Err(EncodeError::ResourceLimit(limit)) => {
                return Err(EncodeError::ResourceLimit(limit))
            }
            Err(error) => {
                unwritten.push((output, error));
                continue;
            }
        };
        match &mut planned {
            None => {
                let mut report = report;
                if output.definition != root {
                    report.notes.insert(0, output_note(&output, &report));
                }
                planned = Some((report, coverage));
            }
            Some((merged, _)) => merge_output(merged, &output, report)?,
        }
        written.push(output);
        payloads.push(bytes);
    }
    let Some((mut report, coverage)) = planned else {
        return Err(unwritten.into_iter().next().map_or_else(
            || EncodeError::Backend(CodecError::malformed("an encode set holds no output")),
            |(_, error)| error,
        ));
    };
    for (output, error) in unwritten {
        let definition = output
            .definition
            .as_ref()
            .map_or("", ProductDefinitionId::as_str);
        report.losses.push(loss::output_unwritten(
            E::FORMAT.as_str(),
            definition,
            &error.to_string(),
        ));
    }
    Ok((report, coverage, written, payloads))
}

/// The merged report and coverage of an encode set, with the outputs
/// written and their payloads.
type PlannedSet = (
    ExportReport,
    ArenaCoverage,
    Vec<PlannedOutput>,
    Vec<Vec<u8>>,
);

/// The note that names the definition `output` holds and the write path of
/// its `report`.
fn output_note(output: &PlannedOutput, report: &ExportReport) -> String {
    let name = output.name.as_ref().map_or("", NonBlankString::as_str);
    let definition = output
        .definition
        .as_ref()
        .map_or("", ProductDefinitionId::as_str);
    format!(
        "the output {name} holds definition {definition}; its write path is {}",
        report.write_path()
    )
}

/// Adds the report of `output`, an output other than the root's, to
/// `merged`: its losses and notes, its census where both count on one basis,
/// and a note naming the output and its write path.
fn merge_output(
    merged: &mut ExportReport,
    output: &PlannedOutput,
    report: ExportReport,
) -> Result<(), EncodeError> {
    let name = output.name.as_ref().map_or("", NonBlankString::as_str);
    merged.notes.push(output_note(output, &report));
    if let Some(cause) = report.synthesis_cause() {
        merged
            .notes
            .push(format!("the output {name} was synthesized: {cause}"));
    }
    if merged.census.basis == report.census.basis {
        for (key, count) in report.census.counts {
            let total = merged.census.counts.entry(key).or_default();
            *total = total.checked_add(count).ok_or_else(|| {
                EncodeError::Backend(CodecError::malformed(
                    "the census of the encode set overflows",
                ))
            })?;
        }
    } else {
        merged.notes.push(format!(
            "the output {name} counts its records on another basis than the root output's, \
             and the census counts the root output's alone"
        ));
    }
    merged.losses.extend(report.losses);
    merged.notes.extend(report.notes);
    Ok(())
}

/// `export:definition.placed` for a payload that carries bodies and no
/// product definition: it writes the bodies of each definition that no
/// occurrence places at the top of the output, and names each such
/// definition that holds a body (§3.4, Encode sets).
fn placed_definition_loss(ir: &CadIr, coverage: &ArenaCoverage, format: &str) -> Option<LossNote> {
    use crate::schema::EntityKind;
    if coverage.carries(EntityKind::ProductDefinition) || !coverage.carries(EntityKind::Body) {
        return None;
    }
    definition_placed(
        format,
        "holds no product definition and writes the bodies of each definition that no \
         occurrence places at the top of the output",
        crate::products::unplaced_definitions(&ir.model)
            .filter(|definition| {
                !definition.bodies.is_empty()
                    || definition
                        .instance_bodies
                        .iter()
                        .any(|instance| !instance.bodies.is_empty())
            })
            .map(|definition| definition.id.as_str().to_owned()),
    )
}

/// A refused write (§3.4, Refusals).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EncodeError {
    /// The requested target names no catalog entry, the catalog has no
    /// default, or the writer cannot deliver the target from this document.
    #[error("{0}")]
    UnsupportedTarget(Box<TargetRefusal>),
    /// An output the writer produces has no name in
    /// [`EncodeInput::outputs`], or the name of an earlier output, or a plan
    /// with more than one output is given one destination
    /// ([`ExportPlan::write_to`]). Nothing is planned or written.
    #[error("the encode names no output for definition {definition}")]
    OutputUnnamed {
        /// The definition whose output has no name.
        definition: ProductDefinitionId,
    },
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
    /// A planning failure. Planning writes into memory and touches no
    /// destination, so an I/O failure it meets is no [`Self::Io`].
    fn from_backend(error: CodecError) -> Self {
        match error {
            CodecError::UnsupportedTarget(refusal) => Self::UnsupportedTarget(refusal),
            CodecError::ResourceLimit(limit) => Self::ResourceLimit(limit),
            error => Self::Backend(error),
        }
    }
}

/// The caller's names of the outputs of one encode, keyed by the definition
/// each output holds (§3.4, Encode sets).
pub type OutputNames = BTreeMap<ProductDefinitionId, NonBlankString>;

/// How the caller names the outputs of one encode (§3.4, Encode sets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outputs<'a> {
    /// No name: the encode writes one output, the root definition's, which
    /// the destination of [`ExportPlan::write_to`] names. An encode that
    /// produces more outputs is refused with [`EncodeError::OutputUnnamed`].
    Root,
    /// The caller's name of every output, keyed by definition.
    Named(&'a OutputNames),
}

/// One output file of an [`ExportPlan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedOutput {
    /// The definition the output holds: the document's root definition, or
    /// `None` where the document states none.
    pub definition: Option<ProductDefinitionId>,
    /// The caller's name of the output; `None` where the destination of
    /// [`ExportPlan::write_to`] names it. A link that another output writes
    /// to this one names it.
    pub name: Option<NonBlankString>,
}

/// The definitions whose outputs a writer produces (§3.4, Encode sets): the
/// root definition, `None` where the document states none, then, for a
/// format that stores one definition per file, every other `File`
/// definition in arena order.
fn output_definitions(ir: &CadIr, per_file: bool) -> Vec<Option<&ProductDefinition>> {
    let definitions = &ir.model.product_definitions;
    let root = definitions.iter().find(|definition| definition.is_root());
    std::iter::once(root)
        .chain(
            definitions
                .iter()
                .filter(|definition| {
                    per_file
                        && matches!(
                            definition.storage,
                            Storage::File {
                                source: Some(_),
                                ..
                            }
                        )
                })
                .map(Some),
        )
        .collect()
}

/// The outputs the writer produces, each with the caller's name. Refuses
/// the first output that `names` leaves unnamed, under [`Outputs::Root`]
/// every output but the root definition's, and the first whose name an
/// earlier output takes.
fn planned_outputs(
    ir: &CadIr,
    names: Outputs<'_>,
    per_file: bool,
) -> Result<Vec<PlannedOutput>, EncodeError> {
    let mut taken = std::collections::BTreeSet::new();
    output_definitions(ir, per_file)
        .into_iter()
        .enumerate()
        .map(|(position, definition)| {
            let definition = definition.map(|definition| definition.id.clone());
            let name = match (names, &definition) {
                (Outputs::Root, _) if position == 0 => None,
                (_, None) => None,
                (Outputs::Root, Some(definition)) => {
                    return Err(EncodeError::OutputUnnamed {
                        definition: definition.clone(),
                    })
                }
                (Outputs::Named(names), Some(definition)) => {
                    let unnamed = || EncodeError::OutputUnnamed {
                        definition: definition.clone(),
                    };
                    let name = names.get(definition).ok_or_else(unnamed)?;
                    if !taken.insert(name) {
                        return Err(unnamed());
                    }
                    Some(name.clone())
                }
            };
            Ok(PlannedOutput { definition, name })
        })
        .collect()
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
    /// The target the caller asks for. The default inherits the source's.
    pub target: TargetRequest<'a>,
    /// The caller's names of the outputs (§3.4, Encode sets).
    pub outputs: Outputs<'a>,
    /// The session ceilings of the write: a writer refuses with
    /// [`EncodeError::ResourceLimit`] only where one of them runs out.
    pub limits: ResourceLimits,
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
            target: TargetRequest::Inherit,
            outputs: Outputs::Root,
            limits: ResourceLimits::desktop(),
            assets: None,
            scene: SceneOptions::DEFAULT,
            missing_states: MissingStateForm::GeometryTrue,
            strictness: &PERMISSIVE,
            validation: None,
        }
    }

    /// Selects the target of the export.
    #[must_use]
    pub const fn with_target(mut self, target: TargetRequest<'a>) -> Self {
        self.target = target;
        self
    }

    /// Names the outputs of the export.
    #[must_use]
    pub const fn with_outputs(mut self, outputs: Outputs<'a>) -> Self {
        self.outputs = outputs;
        self
    }

    /// Selects the session ceilings of the export.
    #[must_use]
    pub const fn with_limits(mut self, limits: ResourceLimits) -> Self {
        self.limits = limits;
        self
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

impl Consumption {
    /// The cause of a same-format synthesis whose backend stated none: the
    /// reason fidelity was not consumed.
    fn patch_declined(&self, fidelity: Option<&SourceFidelity>) -> SynthesisCause {
        let reason = match (self, fidelity) {
            (Self::Degraded { reason }, _) => reason.clone(),
            (Self::NotConsumed, None) => "the encode holds no source fidelity".to_owned(),
            (Self::NotConsumed, Some(_)) => {
                "the writer wrote the file from the document without its source".to_owned()
            }
        };
        SynthesisCause::PatchDeclined { reason }
    }
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
    /// Why a writer with a patch or replay path synthesized a file of a
    /// same-format source. The wrapper reports it only for such a writer and
    /// such a file, and states the fidelity's reason where this is `None`.
    pub synthesis_cause: Option<SynthesisCause>,
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
            synthesis_cause: None,
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
    /// The payload of each of `outputs`, in its order.
    payloads: Vec<Vec<u8>>,
    outputs: Vec<PlannedOutput>,
}

impl ExportPlan {
    /// Returns the complete plan-time export report.
    pub fn report(&self) -> &ExportReport {
        &self.report
    }

    /// The outputs the plan writes (§3.4, Encode sets).
    #[must_use]
    pub fn outputs(&self) -> &[PlannedOutput] {
        &self.outputs
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

    /// Writes the payload of a plan with one output and returns the
    /// unchanged plan-time report. A plan with more outputs writes nothing
    /// and refuses with [`EncodeError::OutputUnnamed`] for the definition of
    /// its second output, which the one destination cannot name; write it
    /// with [`Self::write_each`].
    pub fn write_to(self, writer: &mut dyn Write) -> Result<ExportReport, EncodeError> {
        let [bytes] = self.payloads.as_slice() else {
            return Err(self
                .outputs
                .iter()
                .skip(1)
                .find_map(|output| output.definition.clone())
                .map_or_else(
                    || EncodeError::Backend(CodecError::malformed("the plan holds no output")),
                    |definition| EncodeError::OutputUnnamed { definition },
                ));
        };
        writer.write_all(bytes)?;
        Ok(self.report)
    }

    /// Gives `write` each output and its payload, in the order of
    /// [`Self::outputs`], and returns the unchanged plan-time report. The
    /// first failure stops the write.
    pub fn write_each(
        self,
        mut write: impl FnMut(&PlannedOutput, &[u8]) -> std::io::Result<()>,
    ) -> Result<ExportReport, EncodeError> {
        for (output, bytes) in self.outputs.iter().zip(&self.payloads) {
            write(output, bytes)?;
        }
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
    /// The document states its own depth, so a container-only dump is a
    /// faithful CADIR document.
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;

    /// CADIR writes the neutral document. Its version identifies the CADIR
    /// schema. The serialized export report uses the `cadir` identity variant,
    /// which has no native dialect target.
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        ctx: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        let malformed = |error: crate::hash::finite_json::CanonicalJsonError| {
            CodecError::Malformed(error.to_string())
        };
        let (mut bytes, measured) =
            crate::hash::measured_canonical_json(input.ir).map_err(malformed)?;
        let mut losses = Vec::new();
        let written;
        let ir = if measured.records.is_empty() && measured.outside.is_empty() {
            input.ir
        } else {
            let (ir, unwritable) = within_the_bounds(ctx, input.ir, &measured)?;
            losses = unwritable;
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
        );
        body.losses = losses;
        Ok(body)
    }
}

/// A copy of `ir` within the bounds of §2.5 (§3.4). Each value outside every
/// record past a bound is written as an unstated value, each expression past
/// its bound is written in its partial form, and each record still past a
/// bound is dropped with each record that needs it, as §3.3 drops a record.
/// A copy that holds no record, of a document that held one, is the empty
/// document, with `export:output.empty`.
fn within_the_bounds(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    measured: &crate::bounds::Measured,
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
            CadirEncoder::FORMAT.as_str(),
            &format!("writes {}", unstated.what),
            [unstated.field],
        ));
    }
    let (restated, records) = restate_expressions_past_the_bound(&mut ir, &measured.records)?;
    losses.extend(restated);
    losses.extend(drop_records_past_the_bounds(ctx, &mut ir, &records)?);
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
            value_unwritable(CadirEncoder::FORMAT.as_str(), &reason, [seed.id])
        })
        .collect();
    let (_, measured) = crate::hash::measured_canonical_json(&*ir)
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    Ok((losses, measured.records))
}

/// Drops from `ir` each record of `records`, which it holds past a bound of
/// §2.5, and each record that needs one, as §3.3 drops a record (§3.4). Each
/// such record charges one `export:value.unwritable` that names it, the
/// bound, and the records dropped and marked with it.
fn drop_records_past_the_bounds(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    records: &std::collections::BTreeMap<crate::bounds::Place, crate::bounds::Bound>,
) -> Result<Vec<LossNote>, CodecError> {
    if records.is_empty() {
        return Ok(Vec::new());
    }
    ir.finalize();
    let bounds: std::collections::BTreeMap<crate::give_up::Seed, crate::bounds::Bound> = records
        .iter()
        .filter_map(|(place, bound)| ir.record_at(place).map(|seed| (seed, *bound)))
        .collect();
    let removed = crate::give_up::drop_records(ctx, ir, bounds.keys().cloned().collect())?;
    Ok(removed
        .into_iter()
        .filter_map(|(seed, released)| {
            let reason = format!(
                "drops record {} of arena {}, which {}, with each record that needs it; \
                 marked=[{}]",
                seed.id,
                seed.arena,
                bounds
                    .get(&seed)
                    .map_or_else(String::new, |bound| bound.describe()),
                released
                    .marked
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            value_unwritable(
                CadirEncoder::FORMAT.as_str(),
                &reason,
                std::iter::once(seed.id).chain(released.dropped),
            )
        })
        .collect())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod test_support;
