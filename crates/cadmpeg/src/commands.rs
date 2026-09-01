// SPDX-License-Identifier: Apache-2.0
//! Command execution, artifact writing, and human-readable reports.

pub(crate) mod reporting;

use reporting::{
    command_report_json, fidelity_diff, fidelity_differs, losses, print_check_report,
    print_decode_report, print_export_emission, print_fidelity_summary, print_id_delta,
    print_source_diff, write_command_report, write_json_report, CommandReportBody,
};

use cadmpeg_ir::codec::write::TargetRequest;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use cadmpeg_core::decode::InspectOptions;
use serde::Serialize;

use cadmpeg_registry::{
    build_encoder, resolve_and_inspect_with, ForcedInput, Format, InputCatalog, InspectError,
    Inspected,
};

use crate::application::refusal::ApplicationError;
use crate::application::transcoder::{emit_export_plan, TargetSelection};
use crate::application::validators::validate_ir;
use crate::application::{
    export_target, ArtifactStore, ConversionPolicy, ConversionRefusal, NativeValidatorCatalog,
    SourceRequest, Transcoder,
};
use crate::loader::{self, LoadNotice};
use crate::DecodeArgs;

/// CLI command-report envelope version.
///
/// Independent of `CadIr.ir_version` and `DECODE_SIDECAR_VERSION`. Version 8
/// carries the four-state dialect admission wire (`admitted`, `unverified`,
/// `residual`, `refused`). Version 7 made the dialect fields unconditional:
/// `dialects` on every container summary and decode report, `target` on every
/// export report, and `dialect` on every source metadata block. Version 6
/// added top-level `status` (`ok` | `refused`) and `refusal`
/// (`{ stage, code, message, dialects?, target? }` or null).
pub(crate) const CLI_SCHEMA_VERSION: u32 = 8;

type CommandResult<T> = std::result::Result<T, ApplicationError>;

/// Catalogs required by CLI command handlers.
pub struct AppCatalogs {
    /// Input detection and codec lookup.
    pub inputs: InputCatalog,
    /// Native namespace validators.
    pub validators: NativeValidatorCatalog,
}

fn print_load_notices(notices: &[LoadNotice]) {
    for notice in notices {
        match notice {
            LoadNotice::LowConfidenceDetection {
                format_id,
                confidence,
            } => {
                eprintln!(
                    "warning: detected {format_id} with {confidence} confidence; use --input-format to override"
                );
            }
        }
    }
}

/// CLI-facing conversion arguments assembled from argv.
pub struct ConversionArgs {
    /// Application conversion policy.
    pub policy: ConversionPolicy,
    /// Replace an existing command report.
    pub report_overwrite: bool,
    /// Optional path for the versioned JSON command report.
    pub report: Option<PathBuf>,
    /// Explicit input format selected by the user.
    pub forced_input: Option<ForcedInput>,
}

fn refusal_report_body(refusal: &ConversionRefusal) -> CommandReportBody<'_> {
    let reports = refusal.evidence().reports;
    CommandReportBody {
        decode_report: reports.decode,
        check_report: reports.check,
        export: reports.export,
        refusal: Some(refusal),
    }
}

/// Attempt to persist a semantic refusal without replacing it with a report
/// I/O failure. The original refusal controls the process exit status; report
/// persistence failure remains visible on stderr.
fn write_refusal_command_report(
    input: &Path,
    output: Option<&Path>,
    force: bool,
    command: &'static str,
    refusal: &ConversionRefusal,
) {
    let body = refusal_report_body(refusal);
    write_refusal_report(input, output, force, command, &body, refusal);
}

fn write_refusal_report<P: Serialize>(
    input: &Path,
    output: Option<&Path>,
    force: bool,
    command: &'static str,
    payload: &P,
    refusal: &ConversionRefusal,
) {
    if !refusal.may_write_report() {
        return;
    }
    if let Err(error) = write_json_report(input, output, force, command, payload, Some(refusal)) {
        eprintln!(
            "warning: could not write {command} refusal report: {error:#}; preserving the original {} refusal",
            refusal.code()
        );
    }
}

/// One input to a structural diff and its optional format override.
#[derive(Clone, Copy)]
pub(crate) struct DiffInput<'a> {
    /// Model path to load.
    pub(crate) path: &'a Path,
    /// Explicit reader selection for this input.
    pub(crate) forced: Option<ForcedInput>,
}

#[derive(Serialize)]
struct InspectReportPayload<'a> {
    confidence: Option<cadmpeg_ir::codec::Confidence>,
    summary: Option<&'a cadmpeg_ir::ContainerSummary>,
}

#[derive(Serialize)]
struct CheckReportPayload<'a> {
    decode_report: Option<&'a cadmpeg_ir::DecodeReport>,
    check_report: &'a cadmpeg_ir::ValidationReport,
}

#[derive(Serialize)]
struct DiffReportPayload<'a> {
    different: bool,
    diff: &'a cadmpeg_ir::IrDiff,
    source_fidelity: &'a reporting::FidelitySummary,
}

/// Inspect a native container and print its entries.
pub fn inspect(
    catalogs: &AppCatalogs,
    path: &Path,
    forced: Option<ForcedInput>,
    json: bool,
    report_path: Option<&Path>,
    force: bool,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> CommandResult<()> {
    if matches!(forced, Some(ForcedInput::Cadir)) {
        return Err(anyhow!("inspect requires a container input, not cadir").into());
    }
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let Inspected {
        selection, summary, ..
    } = match resolve_and_inspect_with(
        &catalogs.inputs,
        &mut file,
        forced,
        &InspectOptions { limits },
    ) {
        Ok(inspected) => inspected,
        Err(InspectError::Io(error)) => {
            return Err(inspect_io_error(path, limits.max_input_bytes, error).into())
        }
        Err(InspectError::Unresolved(error)) => {
            return Err(loader::detection_failure(&error).into())
        }
        Err(InspectError::Cadir | InspectError::Unrecognized) => {
            return Err(inspect_unrecognized(path).into())
        }
        Err(InspectError::Codec {
            selection,
            error: cadmpeg_core::CodecError::UnsupportedDialect { dialects, message },
            ..
        }) => {
            let refusal = ConversionRefusal::unsupported_dialect(dialects, message);
            let payload = InspectReportPayload {
                confidence: selection.confidence(),
                summary: None,
            };
            write_refusal_report(path, report_path, force, "inspect", &payload, &refusal);
            if json {
                println!(
                    "{}",
                    command_report_json("inspect", &payload, Some(&refusal))?
                );
            }
            return Err(refusal.into());
        }
        Err(InspectError::Codec { error, .. }) => {
            return Err(anyhow::Error::new(error)
                .context(format!("inspecting {}", path.display()))
                .into())
        }
    };
    let confidence = selection.confidence();
    let payload = InspectReportPayload {
        confidence,
        summary: Some(&summary),
    };
    write_json_report(path, report_path, force, "inspect", &payload, None)?;
    if json {
        println!("{}", command_report_json("inspect", &payload, None)?);
        return Ok(());
    }
    println!(
        "format: {}{}\ncontainer: {}\nentries: {}",
        summary.format(),
        confidence.map_or_else(
            || " (forced)".to_string(),
            |value| format!(" (detected {value})")
        ),
        summary.container_kind,
        summary.entries.len()
    );
    for line in crate::registry_view::dialect_lines(summary.dialects()) {
        println!("{line}");
    }
    println!();
    for entry in &summary.entries {
        println!(
            "  {:<14} {:>10} → {:<10}  {}",
            entry.role, entry.compressed_size, entry.uncompressed_size, entry.name
        );
        for (key, value) in &entry.attributes {
            println!("        {key} = {value}");
        }
    }
    if !summary.notes.is_empty() {
        println!("\nnotes:");
        for note in &summary.notes {
            println!("  - {note}");
        }
    }
    Ok(())
}

fn inspect_unrecognized(path: &Path) -> anyhow::Error {
    anyhow!(
        "no codec recognized {}; inspect supports container inputs only, not .cadir.json IR documents; supported: FCStd, f3d, Inventor IPT/IAM, sldprt, CATPart, NX/Creo prt, Rhino 3DM, IGES, STEP; use --input-format to override detection",
        path.display()
    )
}

fn inspect_io_error(path: &Path, max_input_bytes: u64, error: io::Error) -> anyhow::Error {
    if error.kind() == io::ErrorKind::FileTooLarge {
        anyhow!(
            "{} exceeds the configured {}-byte input limit",
            path.display(),
            max_input_bytes
        )
    } else {
        anyhow!(error).context(format!("inspecting {}", path.display()))
    }
}

/// Dump a native CAD file and write CADIR JSON.
pub fn dump(
    catalogs: &AppCatalogs,
    path: &Path,
    out: Option<&Path>,
    force: bool,
    report_path: Option<&Path>,
    forced: Option<ForcedInput>,
    args: &DecodeArgs,
) -> CommandResult<()> {
    if let Some(out) = out {
        ArtifactStore::check_output_path(path, out, force)?;
    }
    if let Some(report_path) = report_path {
        ArtifactStore::check_output_path(path, report_path, force)?;
        if let Some(out) = out {
            ArtifactStore::check_distinct_output_paths(
                out,
                "CADIR output",
                report_path,
                "command report",
            )?;
        }
    }
    let outcome = match loader::load_artifact(&catalogs.inputs, path, args.options(), forced) {
        Ok(outcome) => outcome,
        Err(error) => {
            let Some(report_path) = report_path else {
                return Err(error);
            };
            if let Some(refusal) = error.refusal() {
                write_refusal_command_report(path, Some(report_path), force, "dump", refusal);
            }
            return Err(error);
        }
    };
    print_load_notices(&outcome.notices);
    let loaded = &outcome.document;
    let encoder = build_encoder(Format::Cadir);
    let plan = encoder.plan(
        cadmpeg_ir::codec::write::EncodeInput::new(&loaded.ir, loaded.fidelity()),
        TargetRequest::Inherit,
    )?;
    let emission = emit_export_plan(
        plan,
        Format::Cadir,
        out,
        loaded.decode_report(),
        loaded.fidelity(),
    )?;
    print_export_emission(&mut io::stderr(), &emission)?;
    if let Some(report) = loaded.decode_report() {
        print_decode_report(&mut io::stderr(), report)?;
    }
    // Dump does not check. Convert/check compose validate_neutral +
    // fidelity + native; salvage mode may emit IR with findings.
    eprintln!("check: not run (a successful dump is not a checked model; run `cadmpeg check`)");
    write_command_report(
        path,
        report_path,
        force,
        "dump",
        CommandReportBody {
            decode_report: loaded.decode_report(),
            check_report: None,
            export: None,
            refusal: None,
        },
    )?;
    Ok(())
}

/// Load and check CADIR, printing a human-readable or JSON report.
pub fn check_cmd(
    catalogs: &AppCatalogs,
    path: &Path,
    forced: Option<ForcedInput>,
    args: &DecodeArgs,
    json: bool,
    report_path: Option<&Path>,
    force: bool,
) -> CommandResult<()> {
    let outcome = match loader::load_artifact(&catalogs.inputs, path, args.options(), forced) {
        Ok(outcome) => outcome,
        Err(error) => {
            if let Some(refusal) = error.refusal() {
                write_refusal_command_report(path, report_path, force, "check", refusal);
            }
            return Err(error);
        }
    };
    print_load_notices(&outcome.notices);
    let loaded = &outcome.document;
    let mut stdout = io::stdout();
    if let Some(report) = loaded.decode_report() {
        print_decode_report(&mut io::stderr(), report)?;
    }
    let report = validate_ir(
        &catalogs.validators,
        &loaded.ir,
        loaded.fidelity(),
        losses(loaded.decode_report()),
    );
    let check_refusal = (!report.is_ok()).then(|| ConversionRefusal::CheckFailed {
        message: format!("check found {} error(s)", report.error_count()),
        decode_report: loaded.decode_report().cloned(),
        validation: report.clone(),
    });
    let payload = CheckReportPayload {
        decode_report: loaded.decode_report(),
        check_report: &report,
    };
    write_json_report(
        path,
        report_path,
        force,
        "check",
        &payload,
        check_refusal.as_ref(),
    )?;
    if json {
        writeln!(
            stdout,
            "{}",
            command_report_json("check", &payload, check_refusal.as_ref())?
        )?;
    } else {
        print_check_report(&mut stdout, &report)?;
    }
    if let Some(refusal) = check_refusal {
        return Err(refusal.into());
    }
    Ok(())
}

/// Convert a CAD file to another format.
pub fn convert(
    catalogs: &AppCatalogs,
    path: &Path,
    to: Option<&str>,
    conversion: &ConversionArgs,
    args: &DecodeArgs,
) -> CommandResult<()> {
    let selection = match TargetSelection::resolve(to, conversion.policy.destination.path()) {
        Ok(selection) => selection,
        Err(error) => {
            if let Some(refusal) = error.refusal() {
                write_refusal_command_report(
                    path,
                    conversion.report.as_deref(),
                    conversion.report_overwrite,
                    "convert",
                    refusal,
                );
            }
            return Err(error);
        }
    };
    let target = export_target(selection);
    if let Some(report_path) = conversion.report.as_deref() {
        ArtifactStore::check_output_path(path, report_path, conversion.report_overwrite)?;
        if let Some(destination) = conversion.policy.destination.path() {
            ArtifactStore::check_distinct_output_paths(
                destination,
                "CAD output",
                report_path,
                "command report",
            )?;
        }
    }

    let transcoder = Transcoder::new(&catalogs.inputs, &catalogs.validators);
    let source = SourceRequest {
        path,
        forced: conversion.forced_input,
        options: args.options(),
    };
    // A refusal from either stage renders the same way: it carries whatever
    // reports it has, and the command report is written only where the refusal
    // admits one.
    let render_refusal = |refusal: &ConversionRefusal| -> Result<()> {
        let mut stderr = io::stderr();
        let reports = refusal.evidence().reports;
        if let Some(report) = reports.decode {
            print_decode_report(&mut stderr, report)?;
            writeln!(stderr)?;
        }
        if let Some(validation) = reports.check {
            print_check_report(&mut stderr, validation)?;
        }
        write_refusal_command_report(
            path,
            conversion.report.as_deref(),
            conversion.report_overwrite,
            "convert",
            refusal,
        );
        Ok(())
    };

    let prepared = match transcoder.prepare(&source, target, &conversion.policy) {
        Ok(prepared) => prepared,
        Err(error) => {
            if let Some(refusal) = error.refusal() {
                render_refusal(refusal)?;
            }
            return Err(error);
        }
    };

    let planned = match prepared.plan() {
        Ok(planned) => planned,
        Err(error) => {
            if let Some(refusal) = error.refusal() {
                render_refusal(refusal)?;
            }
            return Err(error);
        }
    };

    let (decode_report, validation) = {
        let prepared = planned.prepared();
        print_load_notices(&prepared.notices);
        let mut stderr = io::stderr();
        if let Some(report) = prepared.document.decode_report() {
            print_decode_report(&mut stderr, report)?;
            writeln!(stderr)?;
        }
        if let Some(validation) = &prepared.validation {
            print_check_report(&mut stderr, validation)?;
        }
        (
            prepared.document.decode_report().cloned(),
            prepared.validation.clone(),
        )
    };
    let emission = planned.write()?;
    print_export_emission(&mut io::stderr(), &emission)?;
    if let Err(error) = write_command_report(
        path,
        conversion.report.as_deref(),
        conversion.report_overwrite,
        "convert",
        CommandReportBody {
            decode_report: decode_report.as_ref(),
            check_report: validation.as_ref(),
            export: Some(&emission.report),
            refusal: None,
        },
    ) {
        eprintln!(
            "warning: CAD output was written, but the convert report could not be written: {error:#}"
        );
    }
    Ok(())
}

/// Compare two CAD files.
pub fn diff(
    catalogs: &AppCatalogs,
    a: DiffInput<'_>,
    b: DiffInput<'_>,
    args: &DecodeArgs,
    json: bool,
    report_path: Option<&Path>,
    force: bool,
) -> CommandResult<ExitCode> {
    let left_outcome = loader::load_artifact(&catalogs.inputs, a.path, args.options(), a.forced)?;
    print_load_notices(&left_outcome.notices);
    let right_outcome = loader::load_artifact(&catalogs.inputs, b.path, args.options(), b.forced)?;
    print_load_notices(&right_outcome.notices);
    let left = &left_outcome.document;
    let right = &right_outcome.document;
    let result = cadmpeg_ir::diff(&left.ir, &right.ir);
    let fidelity = fidelity_diff(left.fidelity(), right.fidelity());
    let different = !result.is_empty() || fidelity_differs(&fidelity);
    let payload = DiffReportPayload {
        different,
        diff: &result,
        source_fidelity: &fidelity,
    };
    write_json_report(a.path, report_path, force, "diff", &payload, None)?;
    if json {
        println!("{}", command_report_json("diff", &payload, None)?);
        return Ok(if different {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        });
    }
    println!("diff {} vs {}", a.path.display(), b.path.display());
    if let Some((before, after)) = &result.unit_change {
        println!("  units: {before:?} → {after:?}");
    }
    if let Some((before, after)) = &result.tolerance_change {
        println!("  tolerances: {before:?} → {after:?}");
    }
    print_source_diff(&result.source);
    for arena in &result.per_arena {
        if arena.added.is_empty() && arena.removed.is_empty() && arena.modified.is_empty() {
            continue;
        }
        println!(
            "  {}: +{} -{} ~{}",
            arena.kind,
            arena.added.len(),
            arena.removed.len(),
            arena.modified.len()
        );
        print_id_delta("removed", &arena.removed);
        print_id_delta("added", &arena.added);
        let modified: Vec<String> = arena
            .modified
            .iter()
            .map(|item| format!("{} ({})", item.id, item.fields.join(", ")))
            .collect();
        print_id_delta("modified", &modified);
    }
    print_fidelity_summary(&fidelity);
    if different {
        Ok(ExitCode::from(1))
    } else {
        println!("  identical");
        Ok(ExitCode::SUCCESS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::decode::ResourceLimits;

    fn catalogs() -> AppCatalogs {
        AppCatalogs {
            inputs: InputCatalog::with_builtins(),
            validators: NativeValidatorCatalog::with_builtins(),
        }
    }

    #[test]
    fn inspect_open_errors_name_the_path() {
        let path = Path::new("missing-inspect-input.3dm");
        let error = inspect(
            &catalogs(),
            path,
            None,
            false,
            None,
            false,
            ResourceLimits::desktop(),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "opening missing-inspect-input.3dm");
    }

    #[test]
    fn inspect_input_limit_errors_name_the_path_and_limit() {
        let path = Path::new("oversize.3dm");
        let error = inspect_io_error(
            path,
            2,
            io::Error::new(io::ErrorKind::FileTooLarge, "input limit exceeded"),
        );

        assert_eq!(
            error.to_string(),
            format!(
                "{} exceeds the configured 2-byte input limit",
                path.display()
            )
        );
    }
}
