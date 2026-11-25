// SPDX-License-Identifier: Apache-2.0
//! `cadmpeg measure`: geometric measurement of a loaded model.
//!
//! The command loads a native file or CADIR at geometry depth, measures the
//! selected rows under one session, and prints text, TSV or the JSON command
//! report. A document whose scope holds no model class measures no body and
//! charges `measure:output.empty`, which `--reject-empty` refuses.

mod args;
pub(crate) mod geometry_diff;
mod render;

use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use anyhow::anyhow;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth};
use cadmpeg_ir::report::decode::DecodeReport;
use cadmpeg_ir::report::strictness::{code_name, Strictness};
use cadmpeg_ir::DecodeOptions;
use cadmpeg_measure::report::MeasureReport;
use cadmpeg_measure::{MeasureError, MeasureInput, MeasureOptions, Measurer, WorkProfile};
use cadmpeg_registry::{ForcedInput, InputCatalog};
use serde::Serialize;

pub(crate) use args::SelectionArgs;

use crate::application::artifact_store::FileDestination;
use crate::application::document::{LoadOrigin, LoadedDocument};
use crate::application::refusal::{ApplicationError, ConversionRefusal, DepthReport, RefusalStage};
use crate::commands::report_failure;
use crate::commands::reporting::{command_report_json, print_decode_report, write_json_report};
use crate::loader;

/// How the command prints its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Output {
    /// Human-readable blocks.
    Text,
    /// One tab-separated table.
    Tsv,
    /// The JSON command report.
    Json,
}

/// One `measure` invocation.
pub(crate) struct MeasureCommand<'a> {
    pub(crate) path: &'a Path,
    pub(crate) selection: &'a SelectionArgs,
    pub(crate) output: Output,
    pub(crate) report: Option<&'a FileDestination>,
    pub(crate) forced: Option<ForcedInput>,
    pub(crate) options: DecodeOptions,
    pub(crate) profile: WorkProfile,
    pub(crate) density: Option<cadmpeg_measure::Density>,
    /// Refuse a measurement that charges `measure:output.empty`.
    pub(crate) reject_empty: bool,
}

/// Where the measured document came from.
#[derive(Serialize)]
struct InputSummary<'a> {
    format: Option<&'a str>,
    origin: &'static str,
    decode_depth: DecodeDepth,
}

#[derive(Serialize)]
struct MeasureBody<'a> {
    input: InputSummary<'a>,
    #[serde(flatten)]
    report: &'a MeasureReport,
}

/// The `measure` body of a command report.
#[derive(Serialize)]
pub(crate) struct MeasurePayload<'a> {
    decode_report: Option<&'a DecodeReport>,
    measure: MeasureBody<'a>,
}

fn input_summary(document: &LoadedDocument) -> InputSummary<'_> {
    InputSummary {
        format: document.decode_report().map(DecodeReport::format),
        origin: match document.origin {
            LoadOrigin::Neutral => "neutral",
            LoadOrigin::Restored { .. } => "restored",
            LoadOrigin::Decoded { .. } => "decoded",
        },
        decode_depth: document.ir.scope().depth(),
    }
}

/// Measure a CAD file and print the result.
pub(crate) fn measure(
    inputs: &InputCatalog,
    command: &MeasureCommand<'_>,
) -> Result<ExitCode, ApplicationError> {
    let json = command.output == Output::Json;
    let result = (|| {
        if let Some(report) = command.report {
            report.check(command.path)?;
        }
        if command.output == Output::Tsv && command.selection.levels() > 1 {
            return Err(
                anyhow!("--tsv prints one table; select one of faces, edges or surfaces").into(),
            );
        }
        let loaded = loader::load_artifact(inputs, command.path, &command.options, command.forced)?;
        if let Some(report) = loaded.decode_report() {
            print_decode_report(&mut io::stderr(), report)?;
        }
        let selection = command.selection.resolve(&loaded.ir)?;
        let geometry_transferred = loaded
            .decode_report()
            .is_none_or(DecodeReport::geometry_transferred);
        let report = run(
            Session {
                options: &command.options,
                profile: command.profile,
                density: command.density,
                reject_empty: command.reject_empty,
            },
            &loaded,
            &selection,
            geometry_transferred,
        )?;
        for loss in report.losses() {
            eprintln!(
                "{}: {}: {}",
                loss.severity,
                code_name(&loss.code),
                loss.message
            );
        }
        if !geometry_transferred {
            eprintln!("note: the decode transferred no geometry");
        }
        if let Some(note) = command.selection.empty_note(&report) {
            eprintln!("{note}");
        }
        let payload = MeasurePayload {
            decode_report: loaded.decode_report(),
            measure: MeasureBody {
                input: input_summary(&loaded),
                report: &report,
            },
        };
        write_json_report(command.path, command.report, "measure", &payload)?;
        let mut stdout = io::stdout();
        match command.output {
            Output::Json => writeln!(stdout, "{}", command_report_json("measure", &payload)?)?,
            Output::Tsv => {
                write!(
                    stdout,
                    "{}",
                    render::tsv(&report, command.selection.level())
                )?;
            }
            Output::Text => render::text(&mut stdout, &report)?,
        }
        Ok(ExitCode::SUCCESS)
    })();
    report_failure(result, command.path, command.report, "measure", json)
}

/// The decode policy and work profile one measurement session runs under.
#[derive(Clone, Copy)]
struct Session<'a> {
    options: &'a DecodeOptions,
    profile: WorkProfile,
    density: Option<cadmpeg_measure::Density>,
    /// Refuse a measurement that charges `measure:output.empty`.
    reject_empty: bool,
}

impl Session<'_> {
    /// The measurement's strictness.
    fn strictness(self) -> Strictness {
        if self.reject_empty {
            Strictness::default().with_code("measure:output.empty")
        } else {
            Strictness::default()
        }
    }
}

/// Refuse a document that holds no geometry, for `diff --geometry`.
fn require_geometry(loaded: &LoadedDocument) -> Result<(), ApplicationError> {
    if !loaded.ir.scope().model {
        return Err(ConversionRefusal::InsufficientMeasureDepth {
            depth: DepthReport {
                needed: DecodeDepth::Geometry,
                recorded: DecodeDepth::Container,
            },
            decode_report: loaded.decode_report().cloned(),
        }
        .into());
    }
    Ok(())
}

/// Measure the loaded document under one session.
fn run(
    session: Session<'_>,
    loaded: &LoadedDocument,
    selection: &cadmpeg_measure::Selection,
    geometry_transferred: bool,
) -> Result<MeasureReport, ApplicationError> {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_loaded_input(&arena, &session.options.policy, loaded.input_bytes)
        .map_err(|error| ApplicationError::from(error).at_stage(RefusalStage::Measure))?;
    let input = MeasureInput {
        ir: &loaded.ir,
        fidelity: loaded.fidelity(),
        geometry_transferred,
    };
    let measured = Measurer::new(
        input,
        MeasureOptions {
            work: session.profile,
            density: session.density,
            strictness: session.strictness(),
        },
    )
    .measure(&ctx, selection);
    let finished = ctx.finish_session();
    let report = match measured {
        Ok(report) => report,
        Err(MeasureError::Codec(error)) => {
            return Err(ApplicationError::from(error).at_stage(RefusalStage::Measure))
        }
        Err(MeasureError::StrictRejected { report, entry }) => {
            return Err(ConversionRefusal::StrictMeasureRejected {
                decode_report: loaded.decode_report().cloned(),
                loss: Box::new(report.losses()[entry].clone()),
            }
            .into())
        }
        Err(MeasureError::OutsideScope { entity }) => {
            return Err(anyhow!("{entity} is not in the --body scope").into())
        }
        Err(
            error @ (MeasureError::UnknownEntity { .. } | MeasureError::OutsideOccurrence { .. }),
        ) => return Err(anyhow!("{error}").into()),
    };
    finished.map_err(|error| ApplicationError::from(error).at_stage(RefusalStage::Measure))?;
    Ok(report)
}
