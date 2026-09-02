// SPDX-License-Identifier: Apache-2.0
//! Fidelity-payload projection and stream extraction for
//! `cadmpeg query fidelity`.
//!
//! The decode sidecar (`<stem>.fidelity.json`) retains source bytes as
//! base64 `retained_records`. The bare view lists them as a table — the
//! extraction address space — and `--stream NAME` reassembles one
//! stream's retained bytes, byte-exactly, into `-o FILE` (or stdout with
//! `--binary-stdout`). The view migrates supported legacy sidecars and validates
//! retained record identity, length, and digest before projecting or extracting.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Args;

use super::{detect, print_json, read_input, Artifact};

/// Input selection for `query fidelity`.
#[derive(Debug, Args)]
pub struct FidelityArgs {
    /// Decode sidecar (`<stem>.fidelity.json`), or `-` for standard input.
    pub file: PathBuf,
    /// Extract the retained bytes of this source stream instead of
    /// printing the table.
    #[arg(long, value_name = "NAME")]
    pub stream: Option<String>,
    /// Write the extracted bytes to this file.
    #[arg(short = 'o', long, value_name = "FILE", requires = "stream")]
    pub output: Option<PathBuf>,
    /// Replace an existing output file.
    #[arg(long, requires = "output")]
    pub force: bool,
    /// Stream the extracted bytes to stdout even though they are binary.
    #[arg(long, requires = "stream")]
    pub binary_stdout: bool,
    /// Print the projected table as JSON (record metadata, not the bytes).
    #[arg(long, conflicts_with = "stream")]
    pub json: bool,
}

/// Runs `query fidelity` against one artifact.
pub fn run(args: &FidelityArgs) -> Result<()> {
    let bytes = read_input(&args.file)?;
    match detect(&bytes, &args.file)? {
        Artifact::Sidecar(_) => {}
        Artifact::Cadir(_) => bail!(
            "{} is a CADIR document; the fidelity payload lives in the \
             `<stem>.fidelity.json` decode sidecar `cadmpeg dump` writes \
             next to it",
            args.file.display()
        ),
        Artifact::Report(_) => bail!(
            "{} is a command report; the fidelity payload lives in the \
             `<stem>.fidelity.json` decode sidecar `cadmpeg dump` writes",
            args.file.display()
        ),
    }
    let text = std::str::from_utf8(&bytes).with_context(|| {
        format!(
            "reading the decode sidecar {} as UTF-8",
            args.file.display()
        )
    })?;
    let sidecar = cadmpeg_ir::DecodeSidecar::from_json(text)
        .with_context(|| format!("validating the decode sidecar {}", args.file.display()))?;
    let payload = &sidecar.fidelity;

    match &args.stream {
        Some(stream) => extract(args, payload, stream),
        None if args.json => {
            let records: Vec<serde_json::Value> = payload
                .retained_records
                .iter()
                .map(|record| {
                    serde_json::json!({
                        "id": record.id(),
                        "stream": record.stream(),
                        "offset": record.offset(),
                        "byte_len": record.byte_len(),
                        "data_retained": record.data().is_some(),
                    })
                })
                .collect();
            let value = serde_json::json!({
                "annotations": {
                    "streams": payload.annotations.streams.len(),
                    "provenance": payload.annotations.provenance.len(),
                    "exactness": payload.annotations.exactness.len(),
                },
                "retained_records": records,
            });
            print_json("fidelity", &value);
            Ok(())
        }
        None => {
            println!("stream\toffset\tbytes\tdata\tid");
            for record in &payload.retained_records {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    super::cell(record.stream()),
                    record.offset(),
                    record.byte_len(),
                    if record.data().is_some() { "yes" } else { "no" },
                    super::cell(record.id()),
                );
            }
            if payload.retained_records.is_empty() {
                eprintln!("(this sidecar retains no source records)");
            }
            eprintln!(
                "annotations: {} streams, {} provenance entries, {} exactness \
                 notes; extract retained bytes with `cadmpeg query fidelity \
                 FILE --stream NAME -o OUT`",
                payload.annotations.streams.len(),
                payload.annotations.provenance.len(),
                payload.annotations.exactness.len(),
            );
            Ok(())
        }
    }
}

/// Reassembles one stream's retained bytes and writes them byte-exactly.
fn extract(args: &FidelityArgs, payload: &cadmpeg_ir::SourceFidelity, stream: &str) -> Result<()> {
    const SHOWN: usize = 20;
    let mut matched: Vec<&cadmpeg_ir::RetainedSourceRecord> = payload
        .retained_records
        .iter()
        .filter(|record| record.stream() == stream)
        .collect();
    if matched.is_empty() {
        if payload.retained_records.is_empty() {
            bail!("this sidecar retains no source records");
        }
        let mut streams: Vec<&str> = payload
            .retained_records
            .iter()
            .map(|record| record.stream())
            .collect();
        streams.sort_unstable();
        streams.dedup();
        let shown: Vec<&str> = streams.iter().take(SHOWN).copied().collect();
        bail!(
            "no retained record has stream {stream:?}; retained streams: {}{}",
            shown.join(", "),
            if streams.len() > SHOWN { ", …" } else { "" }
        );
    }
    let missing: Vec<&str> = matched
        .iter()
        .filter(|record| record.data().is_none())
        .map(|record| record.id())
        .collect();
    if !missing.is_empty() {
        bail!(
            "stream {stream:?} is retained without bytes (extent and digest \
             only) for: {}",
            missing.join(", ")
        );
    }
    matched.sort_by_key(|record| record.offset());
    let mut assembled: Vec<u8> = Vec::new();
    let mut expected_offset: Option<u64> = None;
    for record in &matched {
        let data = record.data().expect("missing data handled above");
        if let Some(expected) = expected_offset {
            if record.offset() != expected {
                let extents: Vec<String> = matched
                    .iter()
                    .map(|record| {
                        format!(
                            "{}+{} ({})",
                            record.offset(),
                            record.byte_len(),
                            record.id()
                        )
                    })
                    .collect();
                bail!(
                    "the retained extents of stream {stream:?} are not \
                     contiguous: {}; extract records individually (their \
                     `data` fields are base64)",
                    extents.join(", ")
                );
            }
        }
        expected_offset = Some(record.offset() + record.byte_len());
        assembled.extend_from_slice(data);
    }

    if let Some(path) = &args.output {
        if path.exists() && !args.force {
            bail!("{} exists; pass --force to replace it", path.display());
        }
        std::fs::write(path, &assembled)
            .with_context(|| format!("writing {} bytes to {}", assembled.len(), path.display()))?;
        eprintln!(
            "wrote {} bytes from {} record(s) of stream {stream:?} to {}",
            assembled.len(),
            matched.len(),
            path.display()
        );
        return Ok(());
    }
    if args.binary_stdout {
        std::io::stdout()
            .write_all(&assembled)
            .context("writing extracted bytes to stdout")?;
        return Ok(());
    }
    bail!(
        "retained stream bytes are binary output that a terminal cannot read; \
         pass `-o FILE` or `--binary-stdout` to stream the bytes anyway"
    )
}
