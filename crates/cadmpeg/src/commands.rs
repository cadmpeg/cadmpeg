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
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use cadmpeg_core::decode::InspectOptions;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::{DecodeReport, ExportReport, ValidationReport};
use cadmpeg_ir::{validate_neutral, validate_neutral_with_source_fidelity, CadIr, SourceFidelity};

use cadmpeg_registry::{
    build_encoder, ForcedInput, Format, InputCatalog, LossPolicy, ResolvedSource,
    DETECTION_PREFIX_LEN,
};

use crate::application::{
    export_target, ArtifactStore, ConversionPolicy, ConversionRefusal, NativeValidatorCatalog,
    SidecarPersistOutcome, SourceRequest, Transcoder,
};
use crate::loader::{self, read_detection_input, LoadNotice};
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

fn validate_ir(
    validators: &NativeValidatorCatalog,
    ir: &CadIr,
    source_fidelity: Option<&SourceFidelity>,
    losses: Vec<cadmpeg_ir::LossNote>,
) -> ValidationReport {
    let mut report = match source_fidelity {
        Some(source_fidelity) => validate_neutral_with_source_fidelity(ir, source_fidelity, losses),
        None => validate_neutral(ir, losses),
    };
    report.findings.extend(validators.validate(ir));
    report
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

/// CLI-facing conversion arguments assembled before [`Transcoder::prepare`].
#[allow(clippy::struct_excessive_bools)]
pub struct ConversionPlan {
    /// Replace an existing output or report file.
    pub force: bool,
    /// Optional path for the versioned JSON command report.
    pub report: Option<PathBuf>,
    /// Stream a binary output format to standard output instead of refusing.
    pub binary_stdout: bool,
    /// Write even if the check finds errors.
    pub allow_errors: bool,
    /// Export a geometry format when decoding transferred no geometry.
    pub allow_empty: bool,
    /// Refuse to export when the decode reported any loss.
    pub reject_decode_losses: bool,
    /// Refuse to export when export planning reported any loss, and construct
    /// writers that reject unrepresentable content before emitting a byte.
    pub reject_export_losses: bool,
    /// Explicit input format selected by the user.
    pub forced_input: Option<ForcedInput>,
}

impl ConversionPlan {
    /// The construction-time loss policy `--reject-lossy`'s scope implies.
    const fn loss_policy(&self) -> LossPolicy {
        if self.reject_export_losses {
            LossPolicy::Reject
        } else {
            LossPolicy::Report
        }
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
    let prefix = read_detection_input(path, DETECTION_PREFIX_LEN, limits.max_input_bytes)?;
    let (codec, confidence) = match catalogs.inputs.resolve_source(&prefix, forced) {
        Ok(ResolvedSource::Native {
            codec, confidence, ..
        }) => (codec, confidence),
        Ok(ResolvedSource::Cadir) => {
            return Err(anyhow!(
                "no codec recognized {}; inspect supports container inputs only, not .cadir.json IR documents; supported: FCStd, f3d, Inventor IPT/IAM, sldprt, CATPart, NX/Creo prt, Rhino 3DM, IGES, STEP; use --input-format to override detection",
                path.display()
            ));
        }
        Err(error) => return Err(loader::detection_failure(&error)),
    };
    let mut file = File::open(path)?;
    let summary = codec
        .inspect(&mut file, &InspectOptions { limits })
        .with_context(|| format!("inspecting {}", path.display()))?;
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
        summary.format,
        confidence.map_or_else(
            || " (forced)".to_string(),
            |value| format!(" (detected {value})")
        ),
        summary.container_kind,
        summary.entries.len()
    );
    if let Some(line) = crate::registry_view::dialect_line(&summary.dialects, &summary.format) {
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
    let outcome = match loader::load_artifact(&catalogs.inputs, path, args.options(), forced) {
        Ok(outcome) => outcome,
        Err(error) => {
            let Some(report_path) = report_path else {
                return Err(error);
            };
            let Some(refusal) = decode_failure_refusal(&error) else {
                return Err(error);
            };
            write_command_report(
                path,
                Some(report_path),
                force,
                "dump",
                CommandReportBody {
                    decode_report: None,
                    check_report: None,
                    export: None,
                    refusal: Some(&refusal),
                },
            )?;
            return Err(refusal.into());
        }
    };
    print_load_notices(&outcome.notices);
    let loaded = &outcome.document;
    export_ir(
        &loaded.ir,
        loaded.decode_report(),
        loaded.fidelity(),
        Format::Cadir,
        out,
        path,
        force,
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

fn decode_failure_refusal(error: &anyhow::Error) -> Option<ConversionRefusal> {
    let codec_error = error.downcast_ref::<CodecError>()?;
    if matches!(codec_error, CodecError::Io(_)) {
        return None;
    }
    Some(ConversionRefusal::DecodeFailed {
        message: format!("decode failed: {error:#}"),
    })
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
    out: Option<&Path>,
    plan: &ConversionPlan,
    args: &DecodeArgs,
) -> Result<()> {
    execute_conversion(catalogs, path, to, out, plan, args)
}

fn execute_conversion(
    catalogs: &AppCatalogs,
    path: &Path,
    to: Option<&str>,
    out: Option<&Path>,
    plan: &ConversionPlan,
    args: &DecodeArgs,
) -> Result<()> {
    let selection = OutputSelection::resolve(to, out)?;
    let format = selection.format;
    let target = export_target(format, selection.dialect.as_deref(), plan.loss_policy());

    let transcoder = Transcoder::new(&catalogs.inputs, &catalogs.validators);
    let source = SourceRequest {
        path,
        forced: plan.forced_input,
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
        if refusal.may_write_report() {
            write_command_report(
                path,
                plan.report.as_deref(),
                plan.force,
                "convert",
                CommandReportBody {
                    decode_report: refusal.decode_report(),
                    check_report: refusal.check_report(),
                    export: None,
                    refusal: Some(refusal),
                },
            )?;
        }
        Ok(())
    };

    let prepared = match transcoder.prepare(
        &source,
        target,
        ConversionPolicy {
            force: plan.force,
            binary_stdout: plan.binary_stdout,
            allow_errors: plan.allow_errors,
            allow_empty: plan.allow_empty,
            reject_decode_losses: plan.reject_decode_losses,
            reject_export_losses: plan.reject_export_losses,
            destination: out.map(Path::to_path_buf),
        },
    ) {
        Ok(prepared) => prepared,
        Err(error) => {
            render_refusal(&error)?;
            return Err(error);
        }
    };

    // The plan is made once, here, and the same plan is written below. It
    // borrows `prepared`, so it stays in this scope rather than travelling
    // inside an owned value beside the document it borrows.
    let planned = match prepared.plan() {
        Ok(planned) => planned,
        Err(error) => {
            render_refusal(&error)?;
            return Err(error);
        }
    };

    print_load_notices(&prepared.notices);
    let mut stderr = io::stderr();
    if let Some(report) = prepared.document.decode_report() {
        print_decode_report(&mut stderr, report)?;
        writeln!(stderr)?;
    }
    if let Some(validation) = &prepared.validation {
        print_check_report(&mut stderr, validation)?;
    }
    let decode_report = prepared.document.decode_report().cloned();
    let validation = prepared.validation.clone();
    let report = planned.write()?;
    write_command_report(
        path,
        plan.report.as_deref(),
        plan.force,
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

/// What `--to` and the output path together say the conversion writes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct OutputSelection {
    /// The output format, resolved before the input is opened.
    pub(crate) format: Format,
    /// The dialect half of `--to`, unresolved: a registry id or a catalog
    /// alias. `None` when `--to` named no dialect, which is the identity
    /// default.
    pub(crate) dialect: Option<String>,
}

impl OutputSelection {
    /// Reads `--to VALUE` against the output path.
    ///
    /// The grammar, in the order it is tried:
    ///
    /// * `FORMAT:DIALECT` — the left half names the output format and the
    ///   whole value names the dialect, respelled under the format's canonical
    ///   id so an alias spelling of the format (`3dm:archive-80`) still
    ///   produces a registry id.
    /// * `FORMAT` — the format, with no dialect stated. `--to step` is the
    ///   same statement `-f step` has always made: it says what kind of file
    ///   to write, not which dialect of it, so a same-format conversion still
    ///   inherits.
    /// * anything else — a dialect of the format the output path implies. This
    ///   is what keeps the native short vocabularies usable (`--to 5.1`,
    ///   `--to 60`, `--to ap242e3`). The value is not checked against a
    ///   catalog here; `plan` refuses it after the read, naming the catalog.
    ///
    /// The third case is unambiguous because no target alias is also an output
    /// format name. `scripts/check-dialect-support.py` and
    /// `cadmpeg_registry::encoders` both prove that.
    fn resolve(to: Option<&str>, out: Option<&Path>) -> Result<Self> {
        let inferred = format_from_path(out);
        let Some(value) = to else {
            let format = inferred.ok_or_else(|| {
                anyhow!("cannot infer format from the output path; pass --to FORMAT")
            })?;
            return Ok(Self {
                format,
                dialect: None,
            });
        };

        if let Some((left, right)) = value.split_once(':') {
            let format = Format::from_name(left).ok_or_else(|| {
                anyhow!(
                    "--to {value}: {left} is not an output format of this build; available: {}",
                    Format::vocabulary()
                )
            })?;
            if right.is_empty() {
                bail!(
                    "--to {value}: nothing after the colon; write --to {left} for the format alone"
                );
            }
            warn_on_extension_disagreement(format, inferred);
            return Ok(Self {
                format,
                dialect: Some(format!("{}:{right}", format.name())),
            });
        }

        if let Some(format) = Format::from_name(value) {
            warn_on_extension_disagreement(format, inferred);
            return Ok(Self {
                format,
                dialect: None,
            });
        }

        let format = inferred.ok_or_else(|| {
            anyhow!(
                "--to {value}: not an output format of this build ({}), and no output path to read \
                 a format from; write --to FORMAT:{value}",
                Format::vocabulary()
            )
        })?;
        Ok(Self {
            format,
            dialect: Some(value.to_owned()),
        })
    }
}

/// Warns when an explicitly named output format disagrees with the output
/// path's extension. The named format wins; the warning says so.
/// The output format an `-o` path implies, read from its extension.
fn format_from_path(path: Option<&Path>) -> Option<Format> {
    path.and_then(Path::extension)
        .and_then(|extension| extension.to_str())
        .and_then(Format::from_extension)
}

fn warn_on_extension_disagreement(named: Format, inferred: Option<Format>) {
    if let Some(inferred) = inferred {
        if inferred != named {
            eprintln!(
                "warning: explicit format {} disagrees with output extension format {}; using {}",
                named.name(),
                inferred.name(),
                named.name()
            );
        }
    }
}

/// Writes CADIR for the dump command (no conversion refusals).
fn export_ir(
    ir: &CadIr,
    decode_report: Option<&DecodeReport>,
    source_fidelity: Option<&SourceFidelity>,
    format: Format,
    out: Option<&Path>,
    input: &Path,
    force: bool,
) -> Result<ExportReport> {
    if let Some(path) = out {
        ArtifactStore::check_output_path(input, path, force)?;
    }
    // Dump writes the neutral document. It has no dialect, so no loss policy
    // of a native writer applies to it.
    let encoder = build_encoder(format, LossPolicy::Report);
    let plan = encoder.plan(
        cadmpeg_ir::codec::EncodeInput {
            ir,
            fidelity: source_fidelity,
        },
        TargetRequest::Inherit,
    )?;
    let needs_sidecar_digest =
        format == Format::Cadir && decode_report.is_some() && source_fidelity.is_some();
    let report = if let Some(path) = out {
        let (report, cadir_sha256) =
            ArtifactStore::write_plan_atomic(path, plan, needs_sidecar_digest)?;
        if format == Format::Cadir {
            match ArtifactStore::persist_decode_sidecar(
                path,
                cadir_sha256.as_deref(),
                decode_report,
                source_fidelity,
            )? {
                SidecarPersistOutcome::Wrote(sidecar) => {
                    eprintln!("wrote decode sidecar {}", sidecar.display());
                }
                SidecarPersistOutcome::RemovedStale(sidecar) => {
                    eprintln!("removed stale decode sidecar {}", sidecar.display());
                }
                SidecarPersistOutcome::Absent => {}
            }
        }
        eprintln!(
            "wrote {} ({} entities)",
            path.display(),
            report.census.total()
        );
        report
    } else {
        let stdout = io::stdout().lock();
        let mut writer = BufWriter::with_capacity(64 * 1024, stdout);
        let report = plan.write_to(&mut writer)?;
        writer.flush()?;
        if format == Format::Cadir && decode_report.is_some() && source_fidelity.is_some() {
            eprintln!("note: CADIR written to stdout cannot carry its decode-fidelity sidecar");
        }
        report
    };
    if !report.losses.is_empty() {
        eprintln!("{} export losses:", report.format);
        for loss in &report.losses {
            eprintln!(
                "  [{}/{}] {}",
                loss.severity,
                loss.code.category(),
                loss.message
            );
        }
    }
    Ok(report)
}
