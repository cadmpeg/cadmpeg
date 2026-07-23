// SPDX-License-Identifier: Apache-2.0
//! Versioned JSON output: the command envelope, stdout printing, and the
//! atomic writer shared by artifacts and machine-readable command reports.

use std::io::Write;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use cadmpeg_ir::report::{DecodeReport, ExportReport};
use cadmpeg_ir::validate::ValidationReport;

/// Version of the CLI's JSON envelope, independent of `CadIr.ir_version`.
pub(crate) const CLI_SCHEMA_VERSION: u32 = 4;

/// Wrap a payload object in the versioned command envelope.
///
/// The `schema_version` and `command` keys are inserted into the payload's map.
/// `serde_json` serializes maps in key order, so the result is byte-identical to
/// spelling all keys inline in a single `json!` object.
pub(crate) fn envelope(command: &'static str, payload: serde_json::Value) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("schema_version".to_string(), CLI_SCHEMA_VERSION.into());
    map.insert("command".to_string(), command.into());
    if let serde_json::Value::Object(fields) = payload {
        map.extend(fields);
    }
    serde_json::Value::Object(map)
}

/// Print a JSON value as pretty text with a trailing newline to standard output.
pub(crate) fn print_json(value: &serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Writer for a command's versioned JSON report.
///
/// A report is written only when an output path is present. Every report
/// carries all five envelope keys; absent sections serialize as explicit
/// `null`, a shape the CLI contract pins for refusal and success paths alike.
pub(crate) struct ReportSink<'a> {
    /// Input path, protected from being overwritten by the report.
    pub input: &'a Path,
    /// Report destination, or `None` when no report was requested.
    pub output: Option<&'a Path>,
    /// Replace an existing report file.
    pub force: bool,
    /// Command name recorded in the envelope.
    pub command: &'static str,
}

impl ReportSink<'_> {
    /// Write the five-key command report, or nothing when no output path is set.
    pub(crate) fn write(
        &self,
        decode_report: Option<&DecodeReport>,
        validation_report: Option<&ValidationReport>,
        export: Option<&ExportReport>,
    ) -> Result<()> {
        self.write_payload(serde_json::json!({
            "decode_report": decode_report,
            "validation_report": validation_report,
            "export": export,
        }))
    }

    /// Wrap a payload in the command envelope and write it, or nothing when no
    /// output path is set. Analysis commands write the same body they print with
    /// `--json`; the five-key [`ReportSink::write`] is one such payload.
    pub(crate) fn write_payload(&self, payload: serde_json::Value) -> Result<()> {
        let Some(output) = self.output else {
            return Ok(());
        };
        let mut bytes = serde_json::to_vec_pretty(&envelope(self.command, payload))?;
        bytes.push(b'\n');
        write_output(self.input, output, &bytes, self.force)?;
        eprintln!("wrote report {}", output.display());
        Ok(())
    }
}

/// Atomically write `bytes` to `output`, refusing to clobber the input file.
///
/// The write goes through a temporary file in the destination directory and is
/// persisted by rename, so a failed write never leaves a partial artifact.
pub(crate) fn write_output(input: &Path, output: &Path, bytes: &[u8], force: bool) -> Result<()> {
    let input = std::fs::canonicalize(input)
        .with_context(|| format!("canonicalizing {}", input.display()))?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let output_absolute = if output.exists() {
        std::fs::canonicalize(output)?
    } else {
        std::fs::canonicalize(parent)?.join(
            output
                .file_name()
                .ok_or_else(|| anyhow!("output path has no filename"))?,
        )
    };
    if input == output_absolute {
        bail!("refusing to overwrite input {}", input.display());
    }
    if output.exists() && !force {
        bail!("{} exists; pass --force to overwrite", output.display());
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temporary output in {}", parent.display()))?;
    temporary
        .write_all(bytes)
        .with_context(|| format!("writing temporary output for {}", output.display()))?;
    temporary
        .persist(output)
        .map_err(|error| error.error)
        .with_context(|| format!("persisting temporary output to {}", output.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{envelope, CLI_SCHEMA_VERSION};

    #[test]
    fn envelope_bytes_equal_inline_construction() {
        let built = serde_json::to_vec_pretty(&envelope(
            "convert",
            serde_json::json!({
                "decode_report": serde_json::Value::Null,
                "validation_report": { "is_ok": true, "errors": 0 },
                "export": serde_json::Value::Null,
            }),
        ))
        .expect("envelope serializes");
        let inline = serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": CLI_SCHEMA_VERSION,
            "command": "convert",
            "decode_report": serde_json::Value::Null,
            "validation_report": { "is_ok": true, "errors": 0 },
            "export": serde_json::Value::Null,
        }))
        .expect("inline envelope serializes");
        assert_eq!(built, inline);
    }
}
