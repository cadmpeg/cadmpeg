// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! The `cadmpeg` command-line interface.
//!
//! Convert native CAD files between formats, inspect their contents, and
//! compare two files. See the package README for workflows, format limits,
//! loss reporting, and exit-status semantics.

mod application;
mod commands;
mod inspect;
mod loader;
mod query;
mod registry_view;

use std::path::PathBuf;
use std::process::ExitCode;

use cadmpeg_registry::{ForcedInput, InputCatalog};
use clap::{Args, Parser, Subcommand, ValueEnum};
use registry_view::{print_dialects, print_formats};

use crate::application::NativeValidatorCatalog;
use crate::commands::AppCatalogs;

/// Which losses `--reject-lossy` refuses on.
///
/// One predicate at two stages. Decode loss is what the reader could not carry
/// into the neutral document; export loss is what the writer cannot put in the
/// output. `Any` is both, and is what the bare flag means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum LossScope {
    /// Refuse when the decode reported any loss.
    Decode,
    /// Refuse when export planning reported any loss.
    Export,
    /// Refuse on either.
    Any,
}

impl LossScope {
    /// Whether a decode loss refuses the conversion.
    const fn covers_decode(self) -> bool {
        matches!(self, Self::Decode | Self::Any)
    }

    /// Whether an export loss refuses the conversion.
    const fn covers_export(self) -> bool {
        matches!(self, Self::Export | Self::Any)
    }
}

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
                  cadmpeg diff a.sldprt b.step\n\n\
                  Exit codes: 0 success, 1 negative verdict (failed check, refused write, files differ), 2 operational error."
)]
struct Cli {
    /// Operation to perform.
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum InputFormat {
    /// `FreeCAD` `.FCStd`.
    #[cfg(feature = "fcstd")]
    Fcstd,
    /// Autodesk Fusion `.f3d`.
    #[cfg(feature = "f3d")]
    F3d,
    /// Autodesk Inventor `.ipt` or `.iam`.
    #[cfg(feature = "inventor")]
    #[value(alias = "ipt", alias = "iam")]
    Inventor,
    /// `SolidWorks` `.sldprt`.
    #[cfg(feature = "sldprt")]
    Sldprt,
    /// CATIA V5 `.CATPart`.
    #[cfg(feature = "catia")]
    #[value(alias = "catia")]
    Catpart,
    /// Siemens NX `.prt`.
    #[cfg(feature = "nx")]
    Nx,
    /// Creo Parametric `.prt`.
    #[cfg(feature = "creo")]
    Creo,
    /// Rhino `.3dm`.
    #[cfg(feature = "rhino")]
    #[value(alias = "3dm")]
    Rhino,
    /// IGES `.igs` or `.iges`.
    #[cfg(feature = "iges")]
    #[value(alias = "igs")]
    Iges,
    /// ISO 10303 STEP.
    #[cfg(feature = "step")]
    Step,
    /// Bare ASM `.sat`/`.smt`/`.smb`/`.sab` stream.
    #[cfg(feature = "sat")]
    #[value(alias = "smt", alias = "smb", alias = "sab")]
    Sat,
    /// CADIR JSON.
    Cadir,
}

impl InputFormat {
    fn resolution(self) -> ForcedInput {
        match self {
            #[cfg(feature = "fcstd")]
            Self::Fcstd => ForcedInput::Codec("fcstd"),
            #[cfg(feature = "f3d")]
            Self::F3d => ForcedInput::Codec("f3d"),
            #[cfg(feature = "inventor")]
            Self::Inventor => ForcedInput::Codec("inventor"),
            #[cfg(feature = "sldprt")]
            Self::Sldprt => ForcedInput::Codec("sldprt"),
            #[cfg(feature = "catia")]
            Self::Catpart => ForcedInput::Codec("catia"),
            #[cfg(feature = "nx")]
            Self::Nx => ForcedInput::Codec("nx"),
            #[cfg(feature = "creo")]
            Self::Creo => ForcedInput::Codec("creo"),
            #[cfg(feature = "rhino")]
            Self::Rhino => ForcedInput::Codec("rhino"),
            #[cfg(feature = "iges")]
            Self::Iges => ForcedInput::Codec("iges"),
            #[cfg(feature = "step")]
            Self::Step => ForcedInput::Codec("step"),
            #[cfg(feature = "sat")]
            Self::Sat => ForcedInput::Codec("sat"),
            Self::Cadir => ForcedInput::Cadir,
        }
    }
}

#[derive(Debug, Clone, Args)]
struct InputArgs {
    /// Treat the input as this format.
    #[arg(long, visible_alias = "from", value_enum)]
    input_format: Option<InputFormat>,
}

impl InputArgs {
    fn forced(&self) -> Option<ForcedInput> {
        self.input_format.map(InputFormat::resolution)
    }
}

#[derive(Debug, Clone, Args)]
struct DecodeArgs {
    /// Read the container only; do not decode geometry.
    #[arg(long)]
    container_only: bool,
    /// Fail if required content cannot be decoded. Salvage is off.
    #[arg(long)]
    no_salvage: bool,
    /// Resource-limit profile: `desktop` (generous, the default) or `service`
    /// (tight ceilings for unattended use).
    #[arg(long, value_enum, default_value_t = LimitProfile::Desktop)]
    limits: LimitProfile,
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
    fn options(&self) -> cadmpeg_ir::DecodeOptions {
        let limits = self.limits.limits();
        let mode = if self.no_salvage {
            cadmpeg_core::decode::DecodeMode::Strict
        } else {
            cadmpeg_core::decode::DecodeMode::Salvage
        };
        cadmpeg_ir::DecodeOptions {
            container_only: self.container_only,
            policy: cadmpeg_core::decode::DecodePolicy { mode, limits },
        }
    }
}

impl LimitProfile {
    const fn limits(self) -> cadmpeg_core::decode::ResourceLimits {
        match self {
            LimitProfile::Desktop => cadmpeg_core::decode::ResourceLimits::desktop(),
            LimitProfile::Service => cadmpeg_core::decode::ResourceLimits::service(),
        }
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
    /// `--allow-errors` writes the file even if the check finds errors.
    #[command(
        display_order = 1,
        after_help = "Examples:\n  cadmpeg convert part.sldprt -o part.step\n  cadmpeg convert part.sldprt -o out.3dm --to rhino:archive-80\n  cadmpeg convert part.f3d -o out.igs --to 5.1\n  cadmpeg convert part.f3d --to step"
    )]
    Convert {
        /// CAD file to convert.
        #[arg(required_unless_present = "input_flag")]
        input: Option<PathBuf>,
        /// Tolerated spelling of the positional input.
        #[arg(
            long = "input",
            value_name = "FILE",
            hide = true,
            conflicts_with = "input"
        )]
        input_flag: Option<PathBuf>,
        /// Rejected placeholder: the artifact format comes from --format/-o and
        /// the machine-readable report from --report.
        #[arg(long, hide = true)]
        json: bool,
        /// Stream a binary output format to standard output anyway.
        #[arg(long, hide = true)]
        binary_stdout: bool,
        /// Output format and dialect: `FORMAT`, `FORMAT:DIALECT`, or a bare
        /// dialect of the format the output path implies. Inferred from the
        /// output extension when omitted.
        #[arg(short, long, visible_alias = "to", value_name = "FORMAT[:DIALECT]")]
        format: Option<String>,
        /// Output file; omit to write to standard output.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Replace an existing output file.
        #[arg(long)]
        force: bool,
        /// Write a JSON report to this file.
        #[arg(long)]
        report: Option<PathBuf>,
        /// Write output even if the check finds errors. The check runs and prints findings. Skip the refusal.
        #[arg(long)]
        allow_errors: bool,
        /// Write output even if no geometry was decoded.
        #[arg(long)]
        allow_empty: bool,
        /// Do not write if a loss was reported. `--reject-lossy=decode` refuses
        /// only on decode loss, `=export` only on export loss, and the bare
        /// flag on either.
        #[arg(
            long,
            value_enum,
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "any",
            value_name = "SCOPE"
        )]
        reject_lossy: Option<LossScope>,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodeArgs,
    },
    /// List the formats this build reads and writes.
    #[command(display_order = 7)]
    Formats,
    /// List the dialects of each format, and what this build does with them.
    ///
    /// The identity registry crossed with the capability registry: which
    /// dialects exist, how well each is read, and which are write targets of
    /// this build's encoders.
    #[command(
        display_order = 8,
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
    Inspect {
        /// CAD file to inspect.
        #[arg(required_unless_present = "input_flag")]
        input: Option<PathBuf>,
        /// Tolerated spelling of the positional input.
        #[arg(
            long = "input",
            value_name = "FILE",
            hide = true,
            conflicts_with = "input"
        )]
        input_flag: Option<PathBuf>,
        /// Write JSON to standard output.
        #[arg(long)]
        json: bool,
        /// Write a JSON report to this file.
        #[arg(short = 'o', long, visible_alias = "output")]
        report: Option<PathBuf>,
        /// Replace an existing report file.
        #[arg(long)]
        force: bool,
        /// Resource-limit profile applied during inspection.
        #[arg(long, value_enum, default_value_t = LimitProfile::Desktop)]
        limits: LimitProfile,
        #[command(flatten)]
        input_args: InputArgs,
        /// Byte tool to run instead of showing the container.
        #[command(subcommand)]
        bytes: Option<inspect::ByteCommand>,
    },
    /// Write a CAD file as CADIR JSON.
    ///
    /// CADIR is cadmpeg's JSON form of a model. dump does not check.
    ///
    /// `convert` checks and writes another CAD format. Use dump when you want the JSON.
    #[command(
        display_order = 5,
        after_help = "Examples:\n  cadmpeg dump part.sldprt -o part.cadir.json"
    )]
    Dump {
        /// CAD file to dump.
        #[arg(required_unless_present = "input_flag")]
        input: Option<PathBuf>,
        /// Tolerated spelling of the positional input.
        #[arg(
            long = "input",
            value_name = "FILE",
            hide = true,
            conflicts_with = "input"
        )]
        input_flag: Option<PathBuf>,
        /// Rejected placeholder: dump's stdout is already CADIR JSON; dump report goes to --report.
        #[arg(long, hide = true)]
        json: bool,
        /// Output file; omit to write CADIR to standard output.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Replace an existing output file.
        #[arg(long)]
        force: bool,
        /// Write a JSON report to this file.
        #[arg(long)]
        report: Option<PathBuf>,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodeArgs,
    },
    /// Print a table from a report or CADIR JSON.
    ///
    /// Reads JSON from `--report`, from dump, or from a decode sidecar.
    #[command(
        display_order = 6,
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
    /// Accepts a native CAD file or CADIR JSON.
    #[command(
        display_order = 4,
        after_help = "Examples:\n  cadmpeg check part.sldprt"
    )]
    Check {
        /// CAD file to check.
        #[arg(required_unless_present = "input_flag")]
        input: Option<PathBuf>,
        /// Tolerated spelling of the positional input.
        #[arg(
            long = "input",
            value_name = "FILE",
            hide = true,
            conflicts_with = "input"
        )]
        input_flag: Option<PathBuf>,
        /// Write JSON to standard output.
        #[arg(long)]
        json: bool,
        /// Write a JSON report to this file.
        #[arg(short = 'o', long, visible_alias = "output")]
        report: Option<PathBuf>,
        /// Replace an existing report file.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodeArgs,
    },
    /// Compare two CAD files.
    ///
    /// Compares decoded geometry and topology of two files.
    /// Accepts native CAD files or CADIR JSON.
    #[command(
        display_order = 3,
        after_help = "Examples:\n  cadmpeg diff a.sldprt b.step\n  cadmpeg diff before.f3d after.f3d\n\n\
                      Exit status 1 means the models differ.\n\
                      `inspect cmp` compares raw bytes, not decoded models."
    )]
    Diff {
        /// First CAD file.
        a: PathBuf,
        /// Second CAD file.
        b: PathBuf,
        /// Treat the first file as this format.
        #[arg(long, value_enum)]
        input_format_a: Option<InputFormat>,
        /// Treat the second file as this format.
        #[arg(long, value_enum)]
        input_format_b: Option<InputFormat>,
        /// Write JSON to standard output.
        #[arg(long)]
        json: bool,
        /// Write a JSON report to this file.
        #[arg(short = 'o', long, visible_alias = "output")]
        report: Option<PathBuf>,
        /// Replace an existing report file.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        decode: DecodeArgs,
    },
}

/// Collapses the positional input and the tolerated `--input` spelling.
///
/// Clap guarantees exactly one of the pair is present: the positional is
/// required unless the flag is given, and the two conflict.
fn resolve_input(positional: Option<PathBuf>, flag: Option<PathBuf>) -> PathBuf {
    positional
        .or(flag)
        .expect("clap requires one input spelling")
}

/// Error for `--json` on a command whose output is an artifact.
fn misdirected_json(command: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "--json is not an output selector on {command}; the artifact format comes from \
         --format/-o, and the machine-readable report from --report FILE, projected \
         with `cadmpeg query`"
    )
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
    let catalogs = AppCatalogs {
        inputs: InputCatalog::with_builtins(),
        validators: NativeValidatorCatalog::with_builtins(),
    };
    let result = match command {
        Command::Inspect {
            input,
            input_flag,
            json,
            report,
            force,
            limits,
            input_args,
            bytes,
        } => match bytes {
            Some(byte_command) => inspect::run(byte_command),
            None => {
                let input = resolve_input(input, input_flag);
                commands::inspect(
                    &catalogs,
                    &input,
                    input_args.forced(),
                    json,
                    report.as_deref(),
                    force,
                    limits.limits(),
                )
                .map(|()| ExitCode::SUCCESS)
            }
        },
        Command::Dump {
            input,
            input_flag,
            json,
            output,
            force,
            report,
            input_args,
            decode,
        } => {
            if json {
                Err(anyhow::anyhow!(
                    "dump writes the CADIR JSON artifact itself; its standard output is \
                     already JSON when -o is omitted; the dump report goes to --report FILE, \
                     projected with `cadmpeg query`"
                ))
            } else {
                commands::dump(
                    &catalogs,
                    &resolve_input(input, input_flag),
                    output.as_deref(),
                    force,
                    report.as_deref(),
                    input_args.forced(),
                    &decode,
                )
            }
        }
        .map(|()| ExitCode::SUCCESS),
        Command::Query { view } => query::run(&view).map(|()| ExitCode::SUCCESS),
        Command::Check {
            input,
            input_flag,
            json,
            report,
            force,
            input_args,
            decode,
        } => commands::check_cmd(
            &catalogs,
            &resolve_input(input, input_flag),
            input_args.forced(),
            &decode,
            json,
            report.as_deref(),
            force,
        )
        .map(|()| ExitCode::SUCCESS),
        Command::Diff {
            a,
            b,
            input_format_a,
            input_format_b,
            json,
            report,
            force,
            decode,
        } => commands::diff(
            &catalogs,
            commands::DiffInput {
                path: &a,
                forced: input_format_a.map(InputFormat::resolution),
            },
            commands::DiffInput {
                path: &b,
                forced: input_format_b.map(InputFormat::resolution),
            },
            &decode,
            json,
            report.as_deref(),
            force,
        ),
        Command::Convert {
            input,
            input_flag,
            json,
            binary_stdout,
            format,
            output,
            force,
            report,
            allow_errors,
            allow_empty,
            reject_lossy,
            input_args,
            decode,
        } => {
            if json {
                Err(misdirected_json("convert"))
            } else {
                let conversion_args = commands::ConversionArgs {
                    policy: application::ConversionPolicy {
                        force,
                        binary_stdout,
                        allow_errors,
                        allow_empty,
                        reject_decode_losses: reject_lossy.is_some_and(LossScope::covers_decode),
                        reject_export_losses: reject_lossy.is_some_and(LossScope::covers_export),
                        destination: output.clone(),
                    },
                    report,
                    forced_input: input_args.forced(),
                };
                commands::convert(
                    &catalogs,
                    &resolve_input(input, input_flag),
                    format.as_deref(),
                    output.as_deref(),
                    &conversion_args,
                    &decode,
                )
            }
        }
        .map(|()| ExitCode::SUCCESS),
        Command::Formats => {
            print_formats(&catalogs.inputs);
            Ok(ExitCode::SUCCESS)
        }
        Command::Dialects { format } => print_dialects(format.as_deref())
            .map(|()| ExitCode::SUCCESS)
            .map_err(anyhow::Error::new),
    };
    result.unwrap_or_else(|err| {
        eprintln!("error: {err:#}");
        if let Some(refusal) = err.downcast_ref::<application::ConversionRefusal>() {
            ExitCode::from(refusal.exit_code())
        } else {
            ExitCode::from(2)
        }
    })
}
