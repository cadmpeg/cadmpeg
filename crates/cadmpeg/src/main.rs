// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! The `cadmpeg` command-line interface.
//!
//! Convert native CAD files between formats, inspect their contents, and
//! compare two files. See the package README for workflows, format limits,
//! loss reporting, and exit-status semantics.

mod application;
mod commands;
mod input_format;
mod inspect;
#[cfg(test)]
mod integration_tests;
mod loader;
mod measure;
mod query;
mod registry_view;
mod reject_json;
mod selection;

use std::path::PathBuf;
use std::process::ExitCode;

use cadmpeg_registry::{ForcedInput, InputCatalog};
use clap::{Args, Parser, Subcommand, ValueEnum};
use registry_view::{print_dialects, print_formats};

use crate::application::artifact_store::{OptionalFileDestination, OutputDestinations};
use crate::application::transcoder::{
    ConversionDestinations, DestinationPolicy, LossFloor, LossPolicy,
};

#[derive(Debug, Parser)]
#[command(
    name = "cadmpeg",
    version,
    about = "Convert and inspect native CAD files.",
    long_about = "Convert and inspect native CAD files.\n\n\
                  Reads vendor CAD files and writes them in another format.",
    after_help = "Examples:\n  \
                  cadmpeg convert part.sldprt -o part.step\n  \
                  cadmpeg inspect part.sldprt\n  \
                  cadmpeg diff a.sldprt b.step\n  \
                  cadmpeg measure part.step\n\n\
                  Exit codes: 0 success, 1 negative verdict (failed check, refused write, models differ) or a semantic refusal of the input, 2 operational error."
)]
struct Cli {
    /// Operation to perform.
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Args)]
struct InputArgs {
    /// Treat the input as this format.
    #[arg(
        long,
        visible_alias = "from",
        value_parser = crate::input_format::input_format_parser()
    )]
    input_format: Option<ForcedInput>,
}

#[derive(Debug, Clone, Args)]
struct DecodeArgs {
    /// Read the container only; do not decode geometry. `convert` then writes
    /// CADIR only and refuses every other output format.
    #[arg(long)]
    container_only: bool,
    #[command(flatten)]
    policy: DecodePolicyArgs,
}

/// The decode policy half of [`DecodeArgs`], without the container-only
/// depth switch.
#[derive(Debug, Clone, Args)]
struct DecodePolicyArgs {
    /// Fail if required content cannot be decoded. Salvage is off.
    #[arg(long)]
    no_salvage: bool,
    /// Resource-limit profile: `desktop` (generous, the default) or `service`
    /// (tight ceilings for unattended use).
    #[arg(long, value_enum, default_value_t = LimitProfile::Desktop)]
    limits: LimitProfile,
    /// Override the profile's maximum number of physical input bytes.
    #[arg(long)]
    max_input_bytes: Option<u64>,
}

/// Which caller-owned resource-limit profile a decode runs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LimitProfile {
    /// Generous ceilings for interactive desktop use.
    Desktop,
    /// Tight ceilings for unattended service use.
    Service,
}

impl DecodeArgs {
    /// Options for a full decode, or a container-only one on request.
    fn options(&self) -> cadmpeg_ir::DecodeOptions {
        self.options_at(cadmpeg_core::decode::DecodeDepth::Full)
    }

    /// Options for a decode at `depth`, or a container-only one on request.
    fn options_at(&self, depth: cadmpeg_core::decode::DecodeDepth) -> cadmpeg_ir::DecodeOptions {
        let depth = if self.container_only {
            cadmpeg_core::decode::DecodeDepth::Container
        } else {
            depth
        };
        self.policy.options_at(depth)
    }
}

impl DecodePolicyArgs {
    /// Options for a decode at `depth`.
    fn options_at(&self, depth: cadmpeg_core::decode::DecodeDepth) -> cadmpeg_ir::DecodeOptions {
        let limits = self.limits.limits(self.max_input_bytes);
        let mode = if self.no_salvage {
            cadmpeg_core::decode::DecodeMode::Strict
        } else {
            cadmpeg_core::decode::DecodeMode::Salvage
        };
        cadmpeg_ir::DecodeOptions {
            depth,
            policy: cadmpeg_core::decode::DecodePolicy { mode, limits },
        }
    }
}

impl LimitProfile {
    /// The measurement work profile of this resource profile.
    const fn work_profile(self) -> cadmpeg_measure::WorkProfile {
        match self {
            LimitProfile::Desktop => cadmpeg_measure::WorkProfile::Desktop,
            LimitProfile::Service => cadmpeg_measure::WorkProfile::Service,
        }
    }

    const fn limits(self, max_input_bytes: Option<u64>) -> cadmpeg_core::decode::ResourceLimits {
        let mut limits = match self {
            LimitProfile::Desktop => cadmpeg_core::decode::ResourceLimits::desktop(),
            LimitProfile::Service => cadmpeg_core::decode::ResourceLimits::service(),
        };
        if let Some(max_input_bytes) = max_input_bytes {
            limits.max_input_bytes = max_input_bytes;
        }
        limits
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Convert a CAD file to another format.
    ///
    /// Reads a CAD file, checks it, and writes another format.
    /// The output path's extension selects the format. Pass `--to` when writing to stdout.
    ///
    /// `--to FORMAT:DIALECT` names the output dialect as well as the format.
    /// With no `--to`, a same-format conversion keeps the dialect the input
    /// already is, and a cross-format conversion writes the format's default.
    ///
    /// Convert reports check errors and writes the output. `--reject-errors` refuses when the output carries an error.
    #[command(
        display_order = 1,
        after_help = "Examples:\n  cadmpeg convert part.sldprt -o part.step\n  cadmpeg convert part.sldprt -o out.3dm --to rhino:archive-80\n  cadmpeg convert part.f3d -o out.igs --to 5.1\n  cadmpeg convert part.f3d --to step"
    )]
    Convert {
        #[command(flatten)]
        file: inspect::FileArg,
        #[command(flatten)]
        _reject_json: crate::reject_json::RejectJson,
        /// Output format and dialect: `FORMAT`, `FORMAT:DIALECT`, or a bare
        /// dialect of the format the output path implies. Inferred from the
        /// output extension when omitted.
        #[arg(short, long, visible_alias = "to", value_name = "FORMAT[:DIALECT]")]
        format: Option<String>,
        #[command(flatten)]
        destinations: ConversionDestinations,
        /// Refuse output if the check finds errors in what the output writes.
        #[arg(long)]
        reject_errors: bool,
        /// Refuse output if no geometry was decoded or exported.
        #[arg(long)]
        reject_empty: bool,
        /// Decode the whole source, design intent included, even when the
        /// output format needs only its geometry.
        #[arg(long, conflicts_with = "container_only")]
        full_decode: bool,
        /// Do not write if a loss was reported. `--reject-lossy=decode` refuses
        /// only on decode loss, `=export` only on export loss, and the bare
        /// flag on either. Every scope decodes the whole source, as
        /// `--full-decode` does, so a loss in its design intent counts.
        #[arg(
            long,
            value_enum,
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "any",
            value_name = "SCOPE"
        )]
        reject_lossy: Option<LossPolicy>,
        /// Minimum loss severity refused by --reject-lossy.
        #[arg(long, value_enum, default_value = "info", value_name = "SEVERITY")]
        reject_lossy_floor: LossFloor,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodeArgs,
    },
    /// List the formats this build reads and writes.
    #[command(display_order = 8)]
    Formats,
    /// List the dialects of each format, and what this build does with them.
    ///
    /// The identity registry crossed with the capability registry: which
    /// dialects exist, how well each is read, and which are write targets of
    /// this build's encoders.
    #[command(
        display_order = 9,
        after_help = "Examples:\n  cadmpeg dialects\n  cadmpeg dialects rhino"
    )]
    Dialects {
        /// Show only this format's rows.
        format: Option<String>,
    },
    /// Show what is inside a CAD file.
    ///
    /// Prints the format, container layout, and stored streams.
    ///
    /// Byte tools (hex, find, extract, ...) dump raw bytes. They work on any file.
    ///
    /// `query` prints tables from JSON that `convert` and `dump` write. `inspect` reads the CAD file.
    #[command(
        display_order = 2,
        args_conflicts_with_subcommands = true,
        subcommand_negates_reqs = true,
        subcommand_help_heading = "Byte tools",
        after_help = "Examples:\n  cadmpeg inspect part.sldprt"
    )]
    Inspect(inspect::InspectArgs),
    /// Write a CAD file as CADIR JSON.
    ///
    /// CADIR is cadmpeg's JSON form of a model. dump does not check.
    ///
    /// `convert` checks and writes another CAD format. Use dump when you want the JSON.
    #[command(
        display_order = 6,
        after_help = "Examples:\n  cadmpeg dump part.sldprt -o part.cadir.json"
    )]
    Dump {
        #[command(flatten)]
        file: inspect::FileArg,
        #[command(flatten)]
        _reject_json: crate::reject_json::RejectJson,
        #[command(flatten)]
        destinations: OutputDestinations,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodeArgs,
    },
    /// Print a table from a report or CADIR JSON.
    ///
    /// Reads JSON from `--report`, from dump, or from a decode sidecar.
    #[command(
        display_order = 7,
        subcommand_help_heading = "Views",
        after_help = "Examples:\n  cadmpeg query losses part.convert.json\n  cadmpeg query counts part.cadir.json\n  cadmpeg query graph part.cadir.json model.features ID\n  cadmpeg query join part.cadir.json model.features native.rhino.unknowns --left-key native_ref --right-key id"
    )]
    Query {
        /// Table to print.
        #[command(subcommand)]
        view: query::QueryView,
    },
    /// Check a CAD file for errors.
    ///
    /// Accepts a native CAD file or CADIR JSON. The check also measures the
    /// model and warns about face rings listed out of traversal order, faces
    /// whose sense disagrees with their loop winding, and closed bodies whose
    /// volume readings conflict. When its allowance runs out, an info
    /// finding states how many faces and bodies it did not check. These
    /// findings do not change the exit status.
    #[command(
        mut_arg("output", |arg| arg.long("report").visible_alias("output").help("Write a JSON report to this file")),
        display_order = 4,
        after_help = "Examples:\n  cadmpeg check part.sldprt"
    )]
    Check {
        #[command(flatten)]
        file: inspect::FileArg,
        /// Write JSON to standard output.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        report: OptionalFileDestination,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodeArgs,
    },
    /// Measure a CAD file: carrier parameters, areas, lengths and boxes.
    ///
    /// Accepts a native CAD file or CADIR JSON. Without a face, edge or
    /// surface selector, prints one row per body. Every quantity is measured
    /// with its method and error terms, or unavailable with a reason.
    #[command(
        mut_arg("output", |arg| arg.long("report").visible_alias("output").help("Write a JSON report to this file")),
        display_order = 5,
        after_help = "Examples:\n  cadmpeg measure part.step\n  cadmpeg measure part.step --faces --kind cylinder --tsv\n  cadmpeg measure part.step --face face#46 --json\n\n\
                      measure is not a verdict: exit status 0 means the measurement completed,\n\
                      and an unavailable quantity is data. Exit status 1 is a semantic refusal\n\
                      of the input. `cadmpeg diff A B --geometry` compares two models."
    )]
    Measure {
        #[command(flatten)]
        file: inspect::FileArg,
        #[command(flatten)]
        selection: measure::SelectionArgs,
        /// Write JSON to standard output.
        #[arg(long, conflicts_with = "tsv")]
        json: bool,
        /// Write one tab-separated table to standard output.
        #[arg(long)]
        tsv: bool,
        /// Material density in kg/m³, finite and positive. Body rows then
        /// report mass in kg and the inertia tensor about the centroid in
        /// kg·mm².
        #[arg(long, value_name = "VALUE", value_parser = parse_density)]
        density: Option<cadmpeg_measure::Density>,
        #[command(flatten)]
        report: OptionalFileDestination,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodePolicyArgs,
    },
    /// Compare two CAD files.
    ///
    /// Compares decoded geometry and topology of two files.
    /// Accepts native CAD files or CADIR JSON.
    #[command(
        mut_arg("output", |arg| arg.long("report").visible_alias("output").help("Write a JSON report to this file")),
        display_order = 3,
        after_help = "Examples:\n  cadmpeg diff a.sldprt b.step\n  cadmpeg diff before.f3d after.f3d\n  cadmpeg diff part.step part_rt.step --geometry\n\n\
                      Exit status 1 means the models differ. With --geometry it means the\n\
                      measured geometry does not agree, or the comparison cannot decide.\n\
                      `inspect cmp` compares raw bytes, not decoded models."
    )]
    Diff {
        /// First CAD file.
        a: PathBuf,
        /// Second CAD file.
        b: PathBuf,
        /// Treat the first file as this format.
        #[arg(
            long,
            value_parser = crate::input_format::input_format_parser()
        )]
        input_format_a: Option<ForcedInput>,
        /// Treat the second file as this format.
        #[arg(
            long,
            value_parser = crate::input_format::input_format_parser()
        )]
        input_format_b: Option<ForcedInput>,
        /// Write JSON to standard output.
        #[arg(long)]
        json: bool,
        /// Measure every body of both models in model space and compare
        /// them without entity IDs. Exit status follows the geometry verdict.
        #[arg(long, conflicts_with = "container_only")]
        geometry: bool,
        /// Relative floor of the geometry comparison tolerance: finite, at
        /// least 0 and less than 1.
        #[arg(long, requires = "geometry", value_name = "REL", value_parser = parse_relative_floor)]
        geometry_tolerance: Option<f64>,
        /// Write the geometry comparison table to standard output.
        #[arg(long, requires = "geometry", conflicts_with = "json")]
        tsv: bool,
        #[command(flatten)]
        report: OptionalFileDestination,
        #[command(flatten)]
        decode: DecodeArgs,
    },
}

/// Parse a material density in kilograms per cubic metre.
fn parse_density(text: &str) -> Result<cadmpeg_measure::Density, String> {
    let value: f64 = text
        .parse()
        .map_err(|_| format!("{text} is not a number"))?;
    cadmpeg_measure::Density::new(value)
        .ok_or_else(|| format!("{text} is not a finite positive density in kg/m³"))
}

/// Parse a relative comparison floor.
fn parse_relative_floor(text: &str) -> Result<f64, String> {
    let value: f64 = text
        .parse()
        .map_err(|_| format!("{text} is not a number"))?;
    cadmpeg_measure::compare::CompareOptions::new(value)
        .map(|options| options.relative_floor())
        .ok_or_else(|| format!("{text} is not finite, at least 0 and less than 1"))
}

/// Restore `SIG_DFL` so a closed stdout pipe delivers SIGPIPE instead of a
/// `print!` panic (`failed printing to stdout: Broken pipe`).
#[cfg(unix)]
fn reset_sigpipe() {
    const SIGPIPE: i32 = 13;
    const SIG_DFL: usize = 0;
    unsafe extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    // SAFETY: one call on the main thread, before any I/O or spawned threads.
    // POSIX `SIG_DFL` is a null handler; `signal` is in the C library already
    // linked into a Unix Rust binary.
    unsafe {
        signal(SIGPIPE, SIG_DFL);
    }
}

fn main() -> ExitCode {
    #[cfg(unix)]
    reset_sigpipe();
    let command = Cli::parse().command;
    let inputs = InputCatalog::with_builtins();
    let result: Result<ExitCode, application::refusal::ApplicationError> = match command {
        Command::Inspect(inspect::InspectArgs::Bytes(byte_command)) => {
            inspect::run(byte_command).map_err(application::refusal::ApplicationError::from)
        }
        Command::Inspect(inspect::InspectArgs::Summary(args)) => commands::inspect(
            &inputs,
            args.file.path(),
            args.input_format,
            args.json,
            args.report.0.as_ref(),
            args.limits.limits(args.max_input_bytes),
        )
        .map(|()| ExitCode::SUCCESS),
        Command::Dump {
            file,
            _reject_json: _,
            destinations,
            input_args,
            decode,
        } => commands::dump(
            &inputs,
            file.path(),
            &match destinations.output.0 {
                Some(file) => DestinationPolicy::File(file),
                None => DestinationPolicy::Stdout {
                    allow_binary: false,
                },
            },
            destinations.report.as_ref(),
            input_args.input_format,
            &decode,
        )
        .map(|()| ExitCode::SUCCESS),
        Command::Query { view } => query::run(&view)
            .map(|()| ExitCode::SUCCESS)
            .map_err(application::refusal::ApplicationError::from),
        Command::Check {
            file,
            json,
            report,
            input_args,
            decode,
        } => commands::check_cmd(
            &inputs,
            file.path(),
            input_args.input_format,
            &decode,
            json,
            report.0.as_ref(),
        )
        .map(|()| ExitCode::SUCCESS),
        Command::Diff {
            a,
            b,
            input_format_a,
            input_format_b,
            json,
            geometry,
            geometry_tolerance,
            tsv,
            report,
            decode,
        } => commands::diff(
            &inputs,
            commands::DiffInput {
                path: &a,
                forced: input_format_a,
            },
            commands::DiffInput {
                path: &b,
                forced: input_format_b,
            },
            &decode,
            commands::DiffOutput {
                json,
                geometry: geometry.then(|| measure::geometry_diff::GeometryArgs {
                    compare: geometry_tolerance
                        .and_then(cadmpeg_measure::compare::CompareOptions::new)
                        .unwrap_or_default(),
                    profile: decode.policy.limits.work_profile(),
                    tsv,
                }),
            },
            report.0.as_ref(),
        ),
        Command::Convert {
            file,
            _reject_json: _,
            format,
            destinations,
            reject_errors,
            reject_empty,
            full_decode,
            reject_lossy,
            reject_lossy_floor,
            input_args,
            decode,
        } => {
            let conversion_args = commands::ConversionArgs {
                losses: reject_lossy.unwrap_or_default(),
                loss_floor: reject_lossy_floor,
                reject_errors,
                reject_empty,
                full_decode,
                destination: destinations.destination,
                report: destinations.report,
                forced_input: input_args.input_format,
            };
            commands::convert(
                &inputs,
                file.path(),
                format.as_deref(),
                &conversion_args,
                &decode,
            )
        }
        .map(|()| ExitCode::SUCCESS),
        Command::Measure {
            file,
            selection,
            json,
            tsv,
            density,
            report,
            input_args,
            decode,
        } => measure::measure(
            &inputs,
            &measure::MeasureCommand {
                density,
                path: file.path(),
                selection: &selection,
                output: if json {
                    measure::Output::Json
                } else if tsv {
                    measure::Output::Tsv
                } else {
                    measure::Output::Text
                },
                report: report.0.as_ref(),
                forced: input_args.input_format,
                options: decode.options_at(cadmpeg_core::decode::DecodeDepth::Geometry),
                profile: decode.limits.work_profile(),
            },
        ),
        Command::Formats => print_formats(&inputs)
            .map(|()| ExitCode::SUCCESS)
            .map_err(application::refusal::ApplicationError::from),
        Command::Dialects { format } => print_dialects(format.as_deref())
            .map(|()| ExitCode::SUCCESS)
            .map_err(anyhow::Error::new)
            .map_err(application::refusal::ApplicationError::from),
    };
    result.unwrap_or_else(|err| {
        eprintln!("error: {err:#}");
        ExitCode::from(err.exit_code())
    })
}
