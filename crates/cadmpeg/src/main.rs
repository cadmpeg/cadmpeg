// SPDX-License-Identifier: Apache-2.0
//! The `cadmpeg` command-line interface.
//!
//! The CLI detects supported native CAD containers, decodes model data through
//! CADIR, validates and compares CADIR models, and writes CADIR, STEP AP214,
//! `.FCStd`, `.f3d`, or `.sldprt` output. See the package README for workflows, format
//! limits, loss reporting, and exit-status semantics.

mod commands;
mod format;
mod loader;
mod registry;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::format::{ForcedInput, Format, InputFormat};
use crate::registry::Registry;

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum StepTarget {
    Ap203e1,
    Ap203e2,
    #[default]
    Ap214,
    Ap242e1,
    Ap242e2,
    Ap242e3,
}

impl StepTarget {
    fn schema(self) -> cadmpeg_step::StepSchema {
        match self {
            Self::Ap203e1 => cadmpeg_step::StepSchema::Ap203Edition1,
            Self::Ap203e2 => cadmpeg_step::StepSchema::Ap203Edition2,
            Self::Ap214 => cadmpeg_step::StepSchema::Ap214,
            Self::Ap242e1 => cadmpeg_step::StepSchema::Ap242Edition1,
            Self::Ap242e2 => cadmpeg_step::StepSchema::Ap242Edition2,
            Self::Ap242e3 => cadmpeg_step::StepSchema::Ap242Edition3,
        }
    }
}

#[derive(Debug, Clone, Args)]
struct StepOutputArgs {
    /// STEP application protocol and edition for STEP output.
    #[arg(long, value_enum, default_value_t)]
    step_target: StepTarget,
    /// Reject STEP output before writing when any STEP loss note would be reported.
    #[arg(long)]
    reject_step_losses: bool,
}

impl StepOutputArgs {
    fn options(&self) -> cadmpeg_step::StepWriteOptions {
        cadmpeg_step::StepWriteOptions {
            schema: self.step_target.schema(),
            unsupported: if self.reject_step_losses {
                cadmpeg_step::StepUnsupportedPolicy::Reject
            } else {
                cadmpeg_step::StepUnsupportedPolicy::Report
            },
            ..Default::default()
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "cadmpeg",
    version,
    about = "Inspect, decode, validate, compare, and convert CAD models",
    after_help = "Exit codes: 0 success, 1 semantic failure, 2 operational error."
)]
struct Cli {
    /// Operation to perform.
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum RhinoVersion {
    /// Rhino 5 archive version 50.
    #[value(name = "50", alias = "5")]
    V5,
    /// Rhino 6 archive version 60.
    #[value(name = "60", alias = "6")]
    V6,
    /// Rhino 7 archive version 70.
    #[value(name = "70", alias = "7")]
    V7,
    /// Rhino 8 archive version 80.
    #[value(name = "80", alias = "8")]
    V8,
}

impl RhinoVersion {
    const fn codec(self) -> cadmpeg_codec_rhino::RhinoArchiveVersion {
        match self {
            Self::V5 => cadmpeg_codec_rhino::RhinoArchiveVersion::V5,
            Self::V6 => cadmpeg_codec_rhino::RhinoArchiveVersion::V6,
            Self::V7 => cadmpeg_codec_rhino::RhinoArchiveVersion::V7,
            Self::V8 => cadmpeg_codec_rhino::RhinoArchiveVersion::V8,
        }
    }
}

#[derive(Debug, Clone, Args)]
struct InputArgs {
    /// Bypass content detection and read the input as this format.
    #[arg(long, value_enum)]
    input_format: Option<InputFormat>,
}

impl InputArgs {
    fn forced(&self) -> Option<ForcedInput> {
        self.input_format.map(InputFormat::resolution)
    }
}

#[derive(Debug, Clone, Args)]
struct DecodeArgs {
    /// Stop after the native container layer without transferring geometry.
    #[arg(long)]
    container_only: bool,
    /// Reject a decode that reports a mandatory transfer loss.
    #[arg(long)]
    strict: bool,
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
        let limits = match self.limits {
            LimitProfile::Desktop => cadmpeg_ir::decode::ResourceLimits::desktop(),
            LimitProfile::Service => cadmpeg_ir::decode::ResourceLimits::service(),
        };
        let mode = if self.strict {
            cadmpeg_ir::decode::DecodeMode::Strict
        } else {
            cadmpeg_ir::decode::DecodeMode::Salvage
        };
        cadmpeg_ir::DecodeOptions {
            container_only: self.container_only,
            policy: cadmpeg_ir::decode::DecodePolicy { mode, limits },
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List a native container's entries without decoding its model.
    Inspect {
        /// Native CAD file to inspect.
        input: PathBuf,
        /// Write a versioned JSON summary to standard output.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        input_args: InputArgs,
    },
    /// Decode a native CAD file to canonical CADIR JSON.
    Decode {
        /// Native CAD file to decode.
        input: PathBuf,
        /// Output file; omit to write CADIR to standard output.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Replace an existing output file.
        #[arg(long)]
        force: bool,
        /// Write a versioned JSON command report to this file.
        #[arg(long)]
        report: Option<PathBuf>,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodeArgs,
    },
    /// Validate a CADIR document or a decoded native CAD file.
    Validate {
        /// CADIR or supported native CAD file to validate.
        input: PathBuf,
        /// Write a versioned JSON result to standard output.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodeArgs,
    },
    /// Decode if needed, then export without CADIR validation.
    Export {
        /// CADIR or supported native CAD file to export.
        input: PathBuf,
        /// Output format; inferred from the output extension when omitted.
        #[arg(short, long, value_enum)]
        format: Option<Format>,
        /// Output file; omit to write the artifact to standard output.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Replace an existing output file.
        #[arg(long)]
        force: bool,
        /// Write a versioned JSON command report to this file.
        #[arg(long)]
        report: Option<PathBuf>,
        /// Write geometry output even when decoding transferred no geometry.
        #[arg(long)]
        allow_empty: bool,
        /// Refuse to write output when decoding reported any loss (exit 1).
        #[arg(long)]
        reject_lossy: bool,
        /// Target Rhino archive version; valid only for Rhino output.
        #[arg(long, value_enum)]
        rhino_version: Option<RhinoVersion>,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodeArgs,
        #[command(flatten)]
        step: StepOutputArgs,
    },
    /// Structurally compare two CADIR or supported native CAD models.
    Diff {
        /// First model.
        a: PathBuf,
        /// Second model.
        b: PathBuf,
        /// Write a versioned JSON result to standard output.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        decode: DecodeArgs,
    },
    /// Decode if needed, validate CADIR, then export.
    Convert {
        /// CADIR or supported native CAD file to convert.
        input: PathBuf,
        /// Output format; inferred from the output extension when omitted.
        #[arg(short, long, value_enum)]
        format: Option<Format>,
        /// Output file; omit to write the artifact to standard output.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Replace an existing output file.
        #[arg(long)]
        force: bool,
        /// Write a versioned JSON command report to this file.
        #[arg(long)]
        report: Option<PathBuf>,
        /// Export even when CADIR validation finds errors.
        #[arg(long)]
        allow_invalid: bool,
        /// Write geometry output even when decoding transferred no geometry.
        #[arg(long)]
        allow_empty: bool,
        /// Refuse to write output when decoding reported any loss (exit 1).
        #[arg(long)]
        reject_lossy: bool,
        /// Target Rhino archive version; valid only for Rhino output.
        #[arg(long, value_enum)]
        rhino_version: Option<RhinoVersion>,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodeArgs,
        #[command(flatten)]
        step: StepOutputArgs,
    },
}

fn main() -> ExitCode {
    let command = Cli::parse().command;
    let mut registry = Registry::with_builtins();
    match &command {
        Command::Export { step, .. } | Command::Convert { step, .. } => {
            registry.set_step_options(step.options());
        }
        _ => {}
    }
    let result = match command {
        Command::Inspect {
            input,
            json,
            input_args,
        } => commands::inspect(&registry, &input, input_args.forced(), json)
            .map(|()| ExitCode::SUCCESS),
        Command::Decode {
            input,
            output,
            force,
            report,
            input_args,
            decode,
        } => commands::decode(
            &registry,
            &input,
            output.as_deref(),
            force,
            report.as_deref(),
            input_args.forced(),
            &decode,
        )
        .map(|()| ExitCode::SUCCESS),
        Command::Validate {
            input,
            json,
            input_args,
            decode,
        } => commands::validate_cmd(&registry, &input, input_args.forced(), &decode, json)
            .map(|()| ExitCode::SUCCESS),
        Command::Export {
            input,
            format,
            output,
            force,
            report,
            allow_empty,
            reject_lossy,
            rhino_version,
            input_args,
            decode,
            step: _,
        } => commands::export(
            &registry,
            &input,
            format,
            output.as_deref(),
            commands::ExportSettings {
                force,
                report,
                allow_empty,
                reject_lossy,
                rhino_version: rhino_version.map(RhinoVersion::codec),
                forced_input: input_args.forced(),
            },
            &decode,
        )
        .map(|()| ExitCode::SUCCESS),
        Command::Diff { a, b, json, decode } => commands::diff(&registry, &a, &b, &decode, json),
        Command::Convert {
            input,
            format,
            output,
            force,
            report,
            allow_invalid,
            allow_empty,
            reject_lossy,
            rhino_version,
            input_args,
            decode,
            step: _,
        } => commands::convert(
            &registry,
            &input,
            format,
            output.as_deref(),
            &commands::ConvertSettings {
                force,
                report,
                allow_invalid,
                allow_empty,
                reject_lossy,
                rhino_version: rhino_version.map(RhinoVersion::codec),
                forced_input: input_args.forced(),
            },
            &decode,
        )
        .map(|()| ExitCode::SUCCESS),
    };
    result.unwrap_or_else(|err| {
        eprintln!("error: {err:#}");
        if err.downcast_ref::<commands::SemanticFailure>().is_some() {
            ExitCode::from(1)
        } else {
            ExitCode::from(2)
        }
    })
}
