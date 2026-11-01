// SPDX-License-Identifier: Apache-2.0
//! Command wrappers for the reproduction library.
use crate::application::{
    artifact_store::FileDestination,
    refusal::{ApplicationError, CheckOperation, ConversionRefusal},
};
use anyhow::{bail, Context, Result};
use cadmpeg_ir::report::check::Finding;
use cadmpeg_registry::{
    repro::{check, cut, Bundle, CutOptions},
    InputCatalog,
};
use clap::Args;
use std::io::Write as _;
use std::path::{Path, PathBuf};

const MAX_REPRO_INPUT_BYTES: u64 = 256 * 1024 * 1024;

/// Select a finding from a persisted command report.
#[derive(Debug, Args)]
pub(crate) struct ReproArgs {
    /// Command report written by check.
    #[arg(long)]
    from: PathBuf,
    /// Zero-based finding index.
    #[arg(long)]
    finding: usize,
    /// Bundle destination.
    #[arg(short, long)]
    output: PathBuf,
    /// Return an error after writing a bundle that needs more context.
    #[arg(long)]
    require_reproduction: bool,
}

pub(crate) fn run(inputs: &InputCatalog, args: &ReproArgs) -> Result<()> {
    let text =
        crate::application::artifact_store::read_bounded_text(&args.from, MAX_REPRO_INPUT_BYTES)?;
    let report: serde_json::Value = serde_json::from_str(&text)?;
    let finding: Finding = serde_json::from_value(
        report["check_report"]["findings"]
            .get(args.finding)
            .context("finding index is outside the report")?
            .clone(),
    )?;
    let input = report["input_path"]
        .as_str()
        .context("report needs input_path; run check --report with the current build")?;
    let document = crate::loader::load_artifact(
        inputs,
        Path::new(input),
        cadmpeg_ir::codec::DecodeOptions::default(),
        None,
    )?;
    let bundle = cut(&document.ir, &finding, &CutOptions::default())?;
    let bytes = serde_json::to_vec(&bundle)?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args.output)?;
    output.write_all(&bytes)?;
    if let Some(reason) = &bundle.incomplete_reason {
        eprintln!("bundle written; incomplete: {reason}");
        if args.require_reproduction {
            bail!("bundle requires more context to reproduce its selected finding");
        }
    }
    Ok(())
}

pub(crate) fn check_fragment(
    path: &Path,
    json: bool,
    output: Option<&FileDestination>,
) -> Result<(), ApplicationError> {
    let text = crate::application::artifact_store::read_bounded_text(path, MAX_REPRO_INPUT_BYTES)?;
    let bundle: Bundle = serde_json::from_str(&text).map_err(anyhow::Error::from)?;
    if !bundle.document.fragment {
        return Err(anyhow::anyhow!("bundle document must carry fragment=true").into());
    }
    let findings = check(&bundle);
    let report = cadmpeg_ir::report::check::ValidationReport {
        entity_counts: bundle.document.census(),
        findings,
        losses: Vec::new(),
    };
    let body = serde_json::json!({"check_report": report});
    let refusal = (!report.is_ok()).then(|| ConversionRefusal::CheckFailed {
        operation: CheckOperation::Check,
        decode_report: None,
        validation: report.clone(),
    });
    let text = match refusal.as_ref() {
        Some(refusal) => crate::commands::reporting::payload_report_json(
            "check",
            crate::commands::reporting::Payload::Refused(&body, refusal),
        )?,
        None => crate::commands::reporting::command_report_json("check", &body)?,
    };
    crate::commands::reporting::write_report_text(path, output, &text)?;
    if json {
        println!("{text}");
    } else {
        crate::commands::reporting::print_check_report(&mut std::io::stdout(), &report)?;
    }
    match refusal {
        Some(refusal) => Err(refusal.into()),
        None => Ok(()),
    }
}
