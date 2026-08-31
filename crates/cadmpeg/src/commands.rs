// SPDX-License-Identifier: Apache-2.0
//! Command execution, artifact writing, and human-readable reports.

mod reporting;

use reporting::{
    fidelity_diff, fidelity_differs, fidelity_json, losses, print_check_report,
    print_decode_report, print_fidelity_summary, print_id_delta, print_source_diff,
    write_command_report, write_json_report, CommandReportBody,
};

use cadmpeg_ir::codec::TargetRequest;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use cadmpeg_core::decode::InspectOptions;

use cadmpeg_registry::{
    build_encoder, resolve_and_inspect_with, ForcedInput, Format, InputCatalog,
    ResolveInspectionError, ResolvedInspection,
};

use crate::application::refusal::classify_decode_failure;
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
/// Independent of `CadIr.ir_version` and `DECODE_SIDECAR_VERSION`. Version 7
/// always emits the dialect fields: `dialects` on every container summary and
/// decode report, `target` on every export report, and `dialect` and `declared`
/// on every source metadata block. Version 6 added top-level `status` (`ok` |
/// `refused`) and `refusal` (`{ stage, code, message }` or null).
pub(crate) const CLI_SCHEMA_VERSION: u32 = 7;

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
    CommandReportBody {
        decode_report: refusal.decode_report(),
        check_report: refusal.check_report(),
        export: refusal.export_report(),
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
    if !refusal.may_write_report() {
        return;
    }
    if let Err(error) =
        write_command_report(input, output, force, command, refusal_report_body(refusal))
    {
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

/// Inspect a native container and print its entries.
pub fn inspect(
    catalogs: &AppCatalogs,
    path: &Path,
    forced: Option<ForcedInput>,
    json: bool,
    report_path: Option<&Path>,
    force: bool,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<()> {
    if matches!(forced, Some(ForcedInput::Cadir)) {
        bail!("inspect requires a container input, not cadir");
    }
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let resolved = resolve_and_inspect_with(
        &catalogs.inputs,
        &mut file,
        forced,
        &InspectOptions { limits },
    )
    .map_err(|error| match error {
        ResolveInspectionError::Io(error) => inspect_io_error(path, limits.max_input_bytes, error),
        ResolveInspectionError::Resolve(error) => loader::detection_failure(&error),
    })?;
    let ResolvedInspection::Native {
        format,
        confidence,
        inspection,
    } = resolved
    else {
        return Err(inspect_unrecognized(path));
    };
    let summary = inspection
        .map_err(anyhow::Error::new)
        .with_context(|| format!("inspecting {}", path.display()))?;
    debug_assert_eq!(summary.format(), format);
    write_json_report(
        path,
        report_path,
        force,
        "inspect",
        &serde_json::json!({
            "confidence": confidence,
            "summary": summary,
        }),
        None,
    )?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": CLI_SCHEMA_VERSION,
                "command": "inspect",
                "status": "ok",
                "refusal": null,
                "confidence": confidence,
                "summary": summary,
            }))?
        );
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
) -> Result<()> {
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
            let error = classify_decode_failure(error);
            let Some(report_path) = report_path else {
                return Err(error);
            };
            let Some(refusal) = error.downcast_ref::<ConversionRefusal>() else {
                return Err(error);
            };
            write_refusal_command_report(path, Some(report_path), force, "dump", refusal);
            return Err(error);
        }
    };
    print_load_notices(&outcome.notices);
    let loaded = &outcome.document;
    let encoder = build_encoder(Format::Cadir);
    let plan = encoder.plan(
        cadmpeg_ir::codec::EncodeInput::new(&loaded.ir, loaded.fidelity()),
        TargetRequest::Inherit,
    )?;
    emit_export_plan(
        plan,
        Format::Cadir,
        out,
        loaded.decode_report(),
        loaded.fidelity(),
    )?;
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
) -> Result<()> {
    let outcome = loader::load_artifact(&catalogs.inputs, path, args.options(), forced)?;
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
    write_json_report(
        path,
        report_path,
        force,
        "check",
        &serde_json::json!({
            "decode_report": loaded.decode_report(),
            "check_report": report,
        }),
        check_refusal.as_ref(),
    )?;
    if json {
        let mut payload = serde_json::json!({
            "schema_version": CLI_SCHEMA_VERSION,
            "command": "check",
            "decode_report": loaded.decode_report(),
            "check_report": report,
        });
        match &check_refusal {
            Some(refusal) => {
                let fields = refusal.report_fields();
                payload["status"] = fields["status"].clone();
                payload["refusal"] = fields["refusal"].clone();
            }
            None => {
                payload["status"] = serde_json::json!("ok");
                payload["refusal"] = serde_json::Value::Null;
            }
        }
        writeln!(stdout, "{}", serde_json::to_string_pretty(&payload)?)?;
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
) -> Result<()> {
    let selection = match TargetSelection::resolve(to, conversion.policy.destination.path()) {
        Ok(selection) => selection,
        Err(error) => {
            if let Some(refusal) = error.downcast_ref::<ConversionRefusal>() {
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
    let target = match export_target(selection) {
        Ok(target) => target,
        Err(error) => {
            if let Some(refusal) = error.downcast_ref::<ConversionRefusal>() {
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

    let transcoder = Transcoder::new(&catalogs.inputs, &catalogs.validators);
    let source = SourceRequest {
        path,
        forced: conversion.forced_input,
        options: args.options(),
    };
    // A refusal from either stage renders the same way: it carries whatever
    // reports it has, and the command report is written only where the refusal
    // admits one.
    let render_refusal = |error: &anyhow::Error| -> Result<()> {
        let Some(refusal) = error.downcast_ref::<ConversionRefusal>() else {
            return Ok(());
        };
        let mut stderr = io::stderr();
        if let Some(report) = refusal.decode_report() {
            print_decode_report(&mut stderr, report)?;
            writeln!(stderr)?;
        }
        if let Some(validation) = refusal.check_report() {
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
            render_refusal(&error)?;
            return Err(error);
        }
    };

    let planned = match prepared.plan() {
        Ok(planned) => planned,
        Err(error) => {
            render_refusal(&error)?;
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
    let report = planned.write()?;
    write_command_report(
        path,
        conversion.report.as_deref(),
        conversion.report_overwrite,
        "convert",
        CommandReportBody {
            decode_report: decode_report.as_ref(),
            check_report: validation.as_ref(),
            export: Some(&report),
            refusal: None,
        },
    )
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
) -> Result<ExitCode> {
    let left_outcome = loader::load_artifact(&catalogs.inputs, a.path, args.options(), a.forced)?;
    print_load_notices(&left_outcome.notices);
    let right_outcome = loader::load_artifact(&catalogs.inputs, b.path, args.options(), b.forced)?;
    print_load_notices(&right_outcome.notices);
    let left = &left_outcome.document;
    let right = &right_outcome.document;
    let result = cadmpeg_ir::diff(&left.ir, &right.ir);
    let fidelity = fidelity_diff(left.fidelity(), right.fidelity());
    let different = !result.is_empty() || fidelity_differs(&fidelity);
    write_json_report(
        a.path,
        report_path,
        force,
        "diff",
        &serde_json::json!({
            "different": different,
            "diff": result,
            "source_fidelity": fidelity_json(&fidelity),
        }),
        None,
    )?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": CLI_SCHEMA_VERSION,
                "command": "diff",
                "status": "ok",
                "refusal": null,
                "different": different,
                "diff": result,
                "source_fidelity": fidelity_json(&fidelity),
            }))?
        );
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
