// SPDX-License-Identifier: Apache-2.0
//! CADIR-plus-sidecar persistence for the CLI.
//!
//! CADIR and its decode-fidelity sidecar are written as two separate atomic
//! renames. Those renames are not transactional: a crash between them can leave
//! a stale sidecar beside a newer CADIR. Digest verification on the next load
//! is what saves the caller, not a transactional pair: a sidecar that does not
//! describe the CADIR bytes is ignored with `sidecar.stale`, and the document
//! loads without it.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{ExportPlan, PlannedOutput};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::report::export::{EntityCensus, ExportReport};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::source_fidelity::sidecar_stale_kind;
use cadmpeg_ir::{decode_sidecar_path, DecodeSidecar};
use sha2::{Digest, Sha256};

use super::document::LoadOrigin;
use super::refusal::ApplicationError;

/// File output path and its overwrite policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileDestination {
    /// Output path.
    pub(crate) path: PathBuf,
    /// Replace an existing output.
    pub(super) overwrite: bool,
}

impl FileDestination {
    /// Checks the file output against its source and overwrite policy.
    pub(crate) fn check(&self, input: &Path) -> Result<()> {
        check_output_path(input, &self.path, self.overwrite)
    }

    /// Writes checked output bytes atomically.
    pub(crate) fn write(&self, input: &Path, bytes: &[u8]) -> Result<()> {
        self.check(input)?;
        write_bytes_atomic(&self.path, bytes)
    }
}

/// An optional file output admitted with its overwrite policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OptionalFileDestination(pub Option<FileDestination>);

impl clap::Args for OptionalFileDestination {
    fn augment_args(command: clap::Command) -> clap::Command {
        command
            .arg(
                clap::Arg::new("output")
                    .short('o')
                    .long("output")
                    .help("Output file; omit to write to standard output")
                    .value_parser(clap::value_parser!(PathBuf)),
            )
            .arg(
                clap::Arg::new("force")
                    .long("force")
                    .help("Replace an existing output or report file")
                    .action(clap::ArgAction::SetTrue),
            )
    }

    fn augment_args_for_update(command: clap::Command) -> clap::Command {
        Self::augment_args(command)
    }
}

fn file_from_matches(matches: &clap::ArgMatches, id: &str) -> Option<FileDestination> {
    matches.get_one::<PathBuf>(id).map(|path| FileDestination {
        path: path.clone(),
        overwrite: matches.get_flag("force"),
    })
}

impl clap::FromArgMatches for OptionalFileDestination {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        Ok(Self(file_from_matches(matches, "output")))
    }

    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

/// Primary and report files admitted under one overwrite policy.
#[derive(Debug)]
pub(crate) struct OutputDestinations {
    /// Optional primary file output.
    pub(crate) output: OptionalFileDestination,
    /// Optional command report file.
    pub(crate) report: Option<FileDestination>,
}

impl clap::Args for OutputDestinations {
    fn augment_args(command: clap::Command) -> clap::Command {
        OptionalFileDestination::augment_args(command).arg(
            clap::Arg::new("report")
                .long("report")
                .help("Write a JSON report to this file")
                .value_parser(clap::value_parser!(PathBuf)),
        )
    }

    fn augment_args_for_update(command: clap::Command) -> clap::Command {
        Self::augment_args(command)
    }
}

impl clap::FromArgMatches for OutputDestinations {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        Ok(Self {
            output: OptionalFileDestination::from_arg_matches(matches)?,
            report: file_from_matches(matches, "report"),
        })
    }

    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

/// Reads UTF-8 input under a typed input-byte limit.
pub(crate) fn read_bounded_text(path: &Path, max_bytes: u64) -> Result<String, CodecError> {
    let mut file = File::open(path)?;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_input_bytes = max_bytes;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let mut bytes = Vec::new();
    ctx.complete_input(&mut file, &mut bytes)?;
    let text =
        String::from_utf8(bytes).map_err(|_| CodecError::Malformed("input is not UTF-8".into()))?;
    ctx.finish_session()?;
    Ok(text)
}

/// The decode sidecar beside a CADIR document.
#[derive(Debug)]
pub(crate) enum SidecarLoad {
    /// No sidecar is beside the document.
    Absent,
    /// The sidecar describes the document's bytes.
    Matching(Box<DecodeSidecar>),
    /// The sidecar is ignored: the loss says why.
    Stale(LossNote),
}

/// Load and parse a decode sidecar, verifying it against CADIR bytes.
///
/// A sidecar that cannot be read, does not parse, or describes other bytes
/// is ignored with `sidecar.stale`.
///
/// # Errors
///
/// A CADIR path that names no file, or a resource refusal while reading the
/// sidecar.
pub(crate) fn load_matching_sidecar(
    cadir_path: &Path,
    cadir_bytes: &[u8],
    max_bytes: u64,
) -> Result<SidecarLoad, ApplicationError> {
    let Some(path) = decode_sidecar_path(cadir_path) else {
        return Err(anyhow!(
            "{} names no file, so it has no decode sidecar",
            cadir_path.display()
        )
        .into());
    };
    if !path.exists() {
        return Ok(SidecarLoad::Absent);
    }
    let stale = |reason: String| {
        SidecarLoad::Stale(
            LossNote::new(
                sidecar_stale_kind(),
                format!("decode sidecar {} is ignored: {reason}", path.display()),
            )
            .with_severity(Severity::Warning),
        )
    };
    let text = match read_bounded_text(&path, max_bytes) {
        Ok(text) => text,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error.into()),
        Err(error) => return Ok(stale(error.to_string())),
    };
    let sidecar = match DecodeSidecar::from_json(&text) {
        Ok(sidecar) => sidecar,
        Err(error) => return Ok(stale(format!("it does not parse: {error}"))),
    };
    if !sidecar.matches(cadir_bytes) {
        return Ok(stale(format!(
            "it does not describe the bytes of {}",
            cadir_path.display()
        )));
    }
    Ok(SidecarLoad::Matching(Box::new(sidecar)))
}

/// Refuse to overwrite the input path, or an existing output without force.
fn check_output_path(input: &Path, output: &Path, force: bool) -> Result<()> {
    // A missing source is diagnosed by the loader. It cannot alias an
    // existing source file, so output preflight must not replace that
    // input error with a canonicalization failure.
    let input = input
        .exists()
        .then(|| {
            std::fs::canonicalize(input)
                .with_context(|| format!("canonicalizing {}", input.display()))
        })
        .transpose()?;
    let output_absolute = absolute_output_path(output)?;
    if input.as_ref() == Some(&output_absolute) {
        bail!("refusing to overwrite input {}", output_absolute.display());
    }
    if output.exists() && !force {
        bail!("{} exists; pass --force to overwrite", output.display());
    }
    Ok(())
}

/// Refuse two independently written outputs that resolve to one path.
pub(crate) fn check_distinct_output_paths(
    first: &Path,
    first_label: &str,
    second: &Path,
    second_label: &str,
) -> Result<()> {
    if absolute_output_path(first)? == absolute_output_path(second)? {
        bail!(
            "{first_label} and {second_label} resolve to the same path {}; choose distinct output paths",
            first.display()
        );
    }
    Ok(())
}

fn absolute_output_path(output: &Path) -> Result<PathBuf> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if output.exists() {
        Ok(std::fs::canonicalize(output)?)
    } else {
        Ok(std::fs::canonicalize(parent)?.join(
            output
                .file_name()
                .ok_or_else(|| anyhow!("output path has no filename"))?,
        ))
    }
}

/// The path of each output of `outputs`: an output the caller named is
/// written beside `destination` under that name, and the output the
/// destination names to `destination`.
pub(crate) fn output_paths(destination: &Path, outputs: &[PlannedOutput]) -> Vec<PathBuf> {
    outputs
        .iter()
        .map(|output| output_path(destination, output))
        .collect()
}

fn output_path(destination: &Path, output: &PlannedOutput) -> PathBuf {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    output
        .name
        .as_ref()
        .map_or_else(|| destination.to_owned(), |name| parent.join(name.as_str()))
}

/// Refuse the outputs of a plan before any of them is written: an output at
/// the input path, an existing output without `--force`, two outputs at one
/// path, or an output at the path of the command report.
pub(crate) fn check_plan_outputs(
    input: &Path,
    destination: &FileDestination,
    report: Option<&FileDestination>,
    paths: &[PathBuf],
) -> Result<()> {
    for (index, path) in paths.iter().enumerate() {
        if path != &destination.path {
            check_output_path(input, path, destination.overwrite)?;
        }
        for earlier in &paths[..index] {
            check_distinct_output_paths(earlier, "CAD output", path, "CAD output")?;
        }
        if let Some(report) = report {
            check_distinct_output_paths(path, "CAD output", &report.path, "command report")?;
        }
    }
    Ok(())
}

/// Stage bytes then atomically replace `output`.
fn write_bytes_atomic(output: &Path, bytes: &[u8]) -> Result<()> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
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

/// One file an export wrote: its path, the census of the output it holds,
/// and the SHA-256 of its bytes.
pub(crate) struct WrittenFile {
    pub(crate) path: PathBuf,
    pub(crate) entities: usize,
    pub(crate) sha256: Sha256Digest,
}

/// Stage each output of an export plan at its path ([`output_paths`]) and
/// return the report with each file written, in the plan's output order.
/// The files are not written as one transaction: where a write fails, the
/// error names the files written before it.
pub(super) fn write_plan_atomic(
    output: &Path,
    plan: ExportPlan,
) -> Result<(ExportReport, Vec<WrittenFile>)> {
    let entities: Vec<usize> = plan.censuses().iter().map(EntityCensus::total).collect();
    if output_paths(output, plan.outputs()) != [output] {
        let mut written = Vec::new();
        let result = plan.write_each(|planned, bytes| {
            let path = output_path(output, planned);
            write_bytes_atomic(&path, bytes).map_err(io::Error::other)?;
            written.push(WrittenFile {
                path,
                entities: entities.get(written.len()).copied().unwrap_or_default(),
                sha256: Sha256Digest::from_bytes(Sha256::digest(bytes).into()),
            });
            Ok(())
        });
        let report = result.with_context(|| {
            let paths: Vec<String> = written
                .iter()
                .map(|file| file.path.display().to_string())
                .collect();
            format!(
                "writing the outputs of {}; written before the failure: [{}]",
                output.display(),
                paths.join(", ")
            )
        })?;
        return Ok((report, written));
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temporary output in {}", parent.display()))?;
    let mut sink = TempFileWriter {
        file: &mut temporary,
        hasher: Sha256::new(),
    };
    let mut writer = BufWriter::new(&mut sink);
    let report = plan
        .write_to(&mut writer)
        .with_context(|| format!("writing temporary output for {}", output.display()))?;
    writer
        .flush()
        .with_context(|| format!("flushing temporary output for {}", output.display()))?;
    drop(writer);
    let sha256 = sink.finish();
    temporary
        .persist(output)
        .map_err(|error| error.error)
        .with_context(|| format!("persisting temporary output to {}", output.display()))?;
    let written = WrittenFile {
        path: output.to_owned(),
        entities: entities.first().copied().unwrap_or_default(),
        sha256,
    };
    Ok((report, vec![written]))
}

/// Persist or remove the decode-fidelity sidecar beside a CADIR file.
///
/// A decoded origin causes a second atomic rename after the CADIR write.
/// The pair is not transactional; see the module docs. A neutral origin
/// removes a stale sidecar.
/// The sidecar holds `annotations`, the plan's annotations of the written
/// document, which a CADIR plan states for every document with a decode
/// origin.
pub(super) fn persist_decode_sidecar(
    cadir_path: &Path,
    cadir_sha256: &Sha256Digest,
    origin: &LoadOrigin,
    annotations: Option<cadmpeg_ir::Annotations>,
) -> Result<SidecarPersistOutcome> {
    let Some(path) = decode_sidecar_path(cadir_path) else {
        return Err(anyhow!(
            "{} names no file, so it has no decode sidecar",
            cadir_path.display()
        ));
    };
    match origin {
        LoadOrigin::Decoded {
            report, fidelity, ..
        }
        | LoadOrigin::Restored { report, fidelity }
        | LoadOrigin::Read { report, fidelity } => {
            let Some(annotations) = annotations else {
                return Err(anyhow!(
                    "the CADIR plan for {} states no annotations of the written document",
                    cadir_path.display()
                ));
            };
            let mut fidelity = fidelity.as_ref().clone();
            fidelity.annotations = annotations;
            let sidecar =
                DecodeSidecar::bind_sha256(cadir_sha256.clone(), report.clone(), fidelity);
            let mut bytes = sidecar.to_canonical_json()?.into_bytes();
            bytes.push(b'\n');
            write_bytes_atomic(&path, &bytes)?;
            Ok(SidecarPersistOutcome::Wrote(path))
        }
        LoadOrigin::Neutral if path.exists() => {
            std::fs::remove_file(&path)
                .with_context(|| format!("removing stale decode sidecar {}", path.display()))?;
            Ok(SidecarPersistOutcome::RemovedStale(path))
        }
        LoadOrigin::Neutral => Ok(SidecarPersistOutcome::Absent),
    }
}

/// Result of attempting to keep a CADIR sidecar in sync with its document.
#[derive(Debug)]
pub(crate) enum SidecarPersistOutcome {
    /// Sidecar written beside the CADIR file.
    Wrote(PathBuf),
    /// Stale sidecar removed because the CADIR has no decode origin.
    RemovedStale(PathBuf),
    /// No sidecar present and none required.
    Absent,
}

struct TempFileWriter<'a> {
    file: &'a mut tempfile::NamedTempFile,
    hasher: Sha256,
}

impl TempFileWriter<'_> {
    fn finish(self) -> Sha256Digest {
        Sha256Digest::from_bytes(self.hasher.finalize().into())
    }
}

impl Write for TempFileWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.file.write(bytes)?;
        self.hasher.update(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
#[allow(clippy::default_trait_access)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
    use cadmpeg_core::serde_erase::json;
    use cadmpeg_core::CodecError;
    use std::fs::File;
    #[cfg(feature = "nx")]
    use std::io::Cursor;
    use std::path::Path;

    use super::{
        check_output_path, check_plan_outputs, load_matching_sidecar, read_bounded_text,
        FileDestination, OptionalFileDestination, OutputDestinations, SidecarLoad,
    };
    #[cfg(feature = "nx")]
    use cadmpeg_core::decode::InspectOptions;
    use cadmpeg_ir::report::Severity;
    use cadmpeg_ir::source_fidelity::sidecar_stale_kind;
    use cadmpeg_ir::{decode_sidecar_path, DecodeSidecar};
    use cadmpeg_ir::{report::decode::DecodeReport, CadIr, SourceFidelity};
    use cadmpeg_registry::DETECTION_PREFIX_LEN;
    #[cfg(feature = "nx")]
    use cadmpeg_registry::{
        resolve_and_inspect_with, InputCatalog, InspectError, Inspected, ResolveSourceError,
        Selection,
    };
    #[cfg(feature = "nx")]
    use cadmpeg_test_support::bytes::{put_u16, put_u32};

    fn read_detection_input(
        path: &Path,
        prefix_len: usize,
        max_bytes: u64,
    ) -> Result<Vec<u8>, CodecError> {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_input_bytes = max_bytes;
        let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
        let mut file = File::open(path)?;
        let result =
            cadmpeg_container::compound::read_detection_prefix(&ctx, &mut file, prefix_len);
        ctx.finish_session()?;
        result
    }

    /// A plan of `input.cadir.json` that writes `assembly.sldprt`, the
    /// destination, and the further outputs `others` beside it.
    fn plan_paths(dir: &Path, others: &[&str]) -> (std::path::PathBuf, Vec<std::path::PathBuf>) {
        let input = dir.join("input.cadir.json");
        std::fs::write(&input, b"{}").unwrap();
        let mut paths = vec![dir.join("assembly.sldprt")];
        paths.extend(others.iter().map(|name| dir.join(name)));
        (input, paths)
    }

    fn destination(dir: &Path, overwrite: bool) -> FileDestination {
        FileDestination {
            path: dir.join("assembly.sldprt"),
            overwrite,
        }
    }

    #[test]
    fn an_existing_further_output_is_refused_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let (input, paths) = plan_paths(dir.path(), &["part.sldprt"]);
        std::fs::write(&paths[1], b"kept").unwrap();
        let error =
            check_plan_outputs(&input, &destination(dir.path(), false), None, &paths).unwrap_err();
        assert!(error.to_string().contains("pass --force"), "{error}");
        check_plan_outputs(&input, &destination(dir.path(), true), None, &paths).unwrap();
    }

    #[test]
    fn a_further_output_at_the_input_path_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (input, paths) = plan_paths(dir.path(), &["input.cadir.json"]);
        let error =
            check_plan_outputs(&input, &destination(dir.path(), true), None, &paths).unwrap_err();
        assert!(
            error.to_string().contains("refusing to overwrite input"),
            "{error}"
        );
    }

    #[test]
    fn two_outputs_at_one_path_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (input, paths) = plan_paths(dir.path(), &["part.sldprt", "part.sldprt"]);
        let error =
            check_plan_outputs(&input, &destination(dir.path(), false), None, &paths).unwrap_err();
        assert!(
            error.to_string().contains("resolve to the same path"),
            "{error}"
        );
    }

    #[test]
    fn a_further_output_at_the_report_path_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (input, paths) = plan_paths(dir.path(), &["part.sldprt"]);
        let report = FileDestination {
            path: dir.path().join("part.sldprt"),
            overwrite: false,
        };
        let error = check_plan_outputs(
            &input,
            &destination(dir.path(), false),
            Some(&report),
            &paths,
        )
        .unwrap_err();
        assert!(error.to_string().contains("command report"), "{error}");
    }

    #[test]
    fn clap_pairs_each_output_with_the_shared_force_flag() {
        use clap::Parser;
        for command in ["inspect", "check", "diff"] {
            for flag in ["-o", "--output", "--report"] {
                let mut args = vec!["cadmpeg", command, "input.step"];
                if command == "diff" {
                    args.push("other.step");
                }
                args.extend([flag, "report.json", "--force"]);
                let report = match crate::Cli::try_parse_from(args).unwrap().command {
                    crate::Command::Inspect(crate::inspect::InspectArgs::Summary(args)) => {
                        args.report
                    }
                    crate::Command::Check { report, .. } | crate::Command::Diff { report, .. } => {
                        report
                    }
                    _ => panic!("expected a report command"),
                };
                assert_eq!(
                    report.0,
                    Some(FileDestination {
                        path: "report.json".into(),
                        overwrite: true
                    })
                );
            }
        }
        for command in ["dump", "convert"] {
            let destinations = match crate::Cli::try_parse_from([
                "cadmpeg",
                command,
                "input.step",
                "-o",
                "output.step",
                "--report",
                "report.json",
                "--force",
            ])
            .unwrap()
            .command
            {
                crate::Command::Dump { destinations, .. } => destinations,
                crate::Command::Convert { destinations, .. } => {
                    let crate::application::transcoder::DestinationPolicy::File(file) =
                        destinations.destination
                    else {
                        panic!("expected file output");
                    };
                    OutputDestinations {
                        output: OptionalFileDestination(Some(file)),
                        report: destinations.report,
                    }
                }
                _ => panic!("expected a writing command"),
            };
            assert_eq!(
                destinations.output.0,
                Some(FileDestination {
                    path: "output.step".into(),
                    overwrite: true
                })
            );
            assert_eq!(
                destinations.report,
                Some(FileDestination {
                    path: "report.json".into(),
                    overwrite: true
                })
            );
        }
    }

    #[test]
    fn output_preflight_leaves_a_missing_source_for_the_loader() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing.cadir.json");
        let output = directory.path().join("part.step");

        check_output_path(&missing, &output, false)
            .expect("a missing source cannot alias an existing input file");
    }

    #[test]
    fn output_preflight_still_rejects_the_existing_source_path() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("part.step");
        std::fs::write(&source, b"source").unwrap();

        let error = check_output_path(&source, &source, true).unwrap_err();
        assert!(error.to_string().contains("refusing to overwrite input"));
    }

    #[test]
    fn matching_sidecar_loads_and_a_mismatch_is_stale() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("part.cadir.json");
        let text = CadIr::empty().to_canonical_json().unwrap();
        std::fs::write(&path, &text).unwrap();
        let report: DecodeReport = json::from_value(cadmpeg_core::json!({
            "identity": {"kind": "unclassified", "format": "test"},
            "transfer": {"kind": "full", "geometry_transferred": false},
            "coverage": {},
            "losses": [],
            "notes": [],
            "transfer_ledger": {"entries": []},
        }))
        .unwrap();
        let sidecar = DecodeSidecar::bind_sha256(
            cadmpeg_ir::hash::digest::Sha256Digest::digest(text.as_bytes()),
            report,
            SourceFidelity::new(cadmpeg_ir::hash::digest::Sha256Digest::digest(b"source")),
        );
        std::fs::write(
            decode_sidecar_path(&path).unwrap(),
            sidecar.to_canonical_json().unwrap(),
        )
        .unwrap();

        let loaded = load_matching_sidecar(&path, text.as_bytes(), 1024 * 1024).unwrap();
        assert!(matches!(loaded, SidecarLoad::Matching(_)));

        std::fs::write(&path, format!("{text}\n")).unwrap();
        let loaded =
            load_matching_sidecar(&path, format!("{text}\n").as_bytes(), 1024 * 1024).unwrap();
        let SidecarLoad::Stale(loss) = loaded else {
            panic!("a sidecar of other bytes is stale, got {loaded:?}");
        };
        assert_eq!(loss.code, sidecar_stale_kind());
        assert_eq!(loss.severity, Severity::Warning);
        assert!(loss.message.contains("does not describe the bytes of"));
    }

    #[test]
    fn a_sidecar_that_does_not_parse_is_ignored_as_stale() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("part.cadir.json");
        let text = CadIr::empty().to_canonical_json().unwrap();
        std::fs::write(&path, &text).unwrap();
        std::fs::write(decode_sidecar_path(&path).unwrap(), "{\"edited\": true}").unwrap();
        let loaded = load_matching_sidecar(&path, text.as_bytes(), 1024 * 1024).unwrap();
        let SidecarLoad::Stale(loss) = loaded else {
            panic!("an unparsable sidecar is stale, got {loaded:?}");
        };
        assert_eq!(loss.code, sidecar_stale_kind());
        assert!(loss.message.contains("does not parse"));
    }

    #[test]
    fn text_reader_refuses_input_above_the_configured_limit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.json");
        std::fs::write(&path, "12345").unwrap();
        let error = read_bounded_text(&path, 4).unwrap_err();
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::InputBytes && limit.limit == 4)
        );
    }

    #[test]
    fn compound_detection_probes_beyond_a_short_prefix() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("part.prt");
        let mut bytes = vec![0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
        bytes.extend(std::iter::repeat_n(0x5a, 128));
        std::fs::write(&path, &bytes).unwrap();

        let detected = read_detection_input(&path, 8, 1024).unwrap();
        assert_eq!(detected, bytes);
    }

    #[test]
    fn non_compound_detection_under_a_small_limit_returns_a_prefix() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("part.igs");
        std::fs::write(&path, vec![b'x'; 1024]).unwrap();

        let detected = read_detection_input(&path, DETECTION_PREFIX_LEN, 16)
            .expect("a non-CFB detection prefix respects the limit without refusing the file");

        assert_eq!(detected, vec![b'x'; 16]);
    }

    #[cfg(feature = "nx")]
    #[test]
    fn identify_and_cli_detection_reach_remote_compound_directory_evidence() {
        let bytes = cfb_with_remote_ug_part_directory();
        assert!(bytes.len() > DETECTION_PREFIX_LEN);

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("remote-directory.prt");
        std::fs::write(&path, &bytes).unwrap();
        let cli_prefix = read_detection_input(
            &path,
            DETECTION_PREFIX_LEN,
            cadmpeg_core::decode::u64_from_index(bytes.len()),
        )
        .unwrap();
        assert_eq!(cli_prefix, bytes);

        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&cli_prefix, &arena, &DecodePolicy::default())
                .expect("root");
        let catalog = InputCatalog::with_builtins();
        let cli_candidates = catalog
            .candidates(&ctx, root)
            .expect("candidates")
            .into_iter()
            .map(|(codec, confidence)| (codec.id(), confidence))
            .collect::<Vec<_>>();
        let mut source = Cursor::new(bytes);
        let catalog = InputCatalog::with_builtins();
        let library_candidates =
            match resolve_and_inspect_with(&catalog, &mut source, None, &InspectOptions::default())
            {
                Ok(Inspected {
                    format, selection, ..
                })
                | Err(InspectError::Codec {
                    format, selection, ..
                }) => {
                    let Selection::Detected { confidence } = selection else {
                        panic!("unforced inspection must use content detection");
                    };
                    vec![(format, confidence)]
                }
                Err(InspectError::Unresolved(ResolveSourceError::Ambiguous(tie))) => tie
                    .candidates()
                    .iter()
                    .map(|format| (*format, tie.confidence()))
                    .collect(),
                Err(InspectError::Unrecognized | InspectError::Cadir) => Vec::new(),
                Err(error) => panic!("library detection failed: {error}"),
            };

        assert!(cli_candidates
            .iter()
            .any(|(format, _)| *format == cadmpeg_ir::codec::FormatId::new("nx")));
        assert_eq!(library_candidates, cli_candidates);
    }

    #[cfg(feature = "nx")]
    fn cfb_with_remote_ug_part_directory() -> Vec<u8> {
        const SECTOR: usize = 512;
        const DIRECTORY_SECTOR: usize = 256;
        const END: u32 = 0xffff_fffe;
        const FREE: u32 = 0xffff_ffff;
        const FAT: u32 = 0xffff_fffd;
        let mut file = vec![0; SECTOR * (DIRECTORY_SECTOR + 2)];
        file[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
        put_u16(&mut file, 24, 0x003e);
        put_u16(&mut file, 26, 3);
        put_u16(&mut file, 28, 0xfffe);
        put_u16(&mut file, 30, 9);
        put_u16(&mut file, 32, 6);
        put_u32(&mut file, 44, 3);
        put_u32(
            &mut file,
            48,
            u32::try_from(DIRECTORY_SECTOR).expect("test directory sector fits u32"),
        );
        put_u32(&mut file, 56, 4096);
        put_u32(&mut file, 60, END);
        put_u32(&mut file, 68, END);
        for index in 0..109 {
            put_u32(&mut file, 76 + index * 4, FREE);
        }
        for index in 0..3 {
            put_u32(
                &mut file,
                76 + index * 4,
                u32::try_from(index).expect("test FAT index fits u32"),
            );
            file[SECTOR * (index + 1)..SECTOR * (index + 2)].fill(0xff);
            put_u32(&mut file, SECTOR + index * 4, FAT);
        }
        put_u32(&mut file, SECTOR * 3 + (DIRECTORY_SECTOR - 256) * 4, END);

        let directory = SECTOR * (DIRECTORY_SECTOR + 1);
        directory_entry(
            &mut file[directory..directory + SECTOR],
            0,
            "Root Entry",
            5,
            FREE,
            1,
        );
        directory_entry(
            &mut file[directory..directory + SECTOR],
            1,
            "UG_PART",
            1,
            FREE,
            2,
        );
        directory_entry(
            &mut file[directory..directory + SECTOR],
            2,
            "UG_PART",
            2,
            END,
            FREE,
        );
        file
    }

    #[cfg(feature = "nx")]
    fn directory_entry(
        directory: &mut [u8],
        index: usize,
        name: &str,
        kind: u8,
        start: u32,
        child: u32,
    ) {
        const FREE: u32 = 0xffff_ffff;
        let entry = &mut directory[index * 128..(index + 1) * 128];
        let mut encoded = name.encode_utf16().collect::<Vec<_>>();
        encoded.push(0);
        for (offset, word) in encoded.iter().enumerate() {
            put_u16(entry, offset * 2, *word);
        }
        put_u16(
            entry,
            64,
            u16::try_from(encoded.len() * 2).expect("test name length fits u16"),
        );
        entry[66] = kind;
        entry[67] = 1;
        put_u32(entry, 68, FREE);
        put_u32(entry, 72, FREE);
        put_u32(entry, 76, child);
        put_u32(entry, 116, start);
    }
}
