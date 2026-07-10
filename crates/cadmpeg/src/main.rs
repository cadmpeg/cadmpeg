// SPDX-License-Identifier: Apache-2.0
//! Command-line front-end for cadmpeg.

mod commands;
mod loader;
mod registry;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::registry::Registry;

#[derive(Debug, Parser)]
#[command(
    name = "cadmpeg",
    version,
    about = "An open-source CAD transcoder",
    after_help = "Exit codes: 0 success, 1 semantic failure, 2 operational error."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    #[value(alias = "json")]
    Cadir,
    Step,
    Sldprt,
}

impl Format {
    fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "cadir" | "json" => Some(Self::Cadir),
            "step" | "stp" => Some(Self::Step),
            "sldprt" => Some(Self::Sldprt),
            _ => None,
        }
    }

    fn is_geometry_export(self) -> bool {
        matches!(self, Self::Step | Self::Sldprt)
    }

    fn from_path(path: Option<&std::path::Path>) -> Option<Self> {
        path.and_then(std::path::Path::extension)
            .and_then(|extension| extension.to_str())
            .and_then(Self::from_extension)
    }

    fn name(self) -> &'static str {
        match self {
            Self::Cadir => "cadir",
            Self::Step => "step",
            Self::Sldprt => "sldprt",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum InputFormat {
    F3d,
    Sldprt,
    #[value(alias = "catia")]
    Catpart,
    Nx,
    Creo,
    Cadir,
}

#[derive(Debug, Clone, Copy)]
enum ForcedInput {
    Codec(&'static str),
    Cadir,
}

impl InputFormat {
    fn resolution(self) -> ForcedInput {
        match self {
            Self::F3d => ForcedInput::Codec("f3d"),
            Self::Sldprt => ForcedInput::Codec("sldprt"),
            Self::Catpart => ForcedInput::Codec("catia"),
            Self::Nx => ForcedInput::Codec("nx"),
            Self::Creo => ForcedInput::Codec("creo"),
            Self::Cadir => ForcedInput::Cadir,
        }
    }
}

#[derive(Debug, Clone, Args)]
struct InputArgs {
    /// Bypass format detection and decode as this input format.
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
    /// Stop after the container layer.
    #[arg(long)]
    container_only: bool,
}

impl DecodeArgs {
    fn options(&self) -> cadmpeg_ir::DecodeOptions {
        cadmpeg_ir::DecodeOptions {
            container_only: self.container_only,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List a container's streams/segments without decoding geometry.
    Inspect {
        input: PathBuf,
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        input_args: InputArgs,
    },
    /// Decode a source file into a .cadir.json IR.
    Decode {
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        force: bool,
        /// Write a machine-readable command report.
        #[arg(long)]
        report: Option<PathBuf>,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodeArgs,
    },
    /// Validate an IR document or decoded source file.
    Validate {
        input: PathBuf,
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodeArgs,
    },
    /// Decode (if needed) and export without validation.
    Export {
        input: PathBuf,
        #[arg(short, long, value_enum)]
        format: Option<Format>,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        force: bool,
        /// Write a machine-readable command report.
        #[arg(long)]
        report: Option<PathBuf>,
        /// Write a geometry format even when decoding transferred no geometry.
        #[arg(long)]
        allow_empty: bool,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodeArgs,
    },
    /// Structurally diff two IR documents; exits 1 when they differ.
    Diff {
        a: PathBuf,
        b: PathBuf,
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        decode: DecodeArgs,
    },
    /// Decode, validate, then export.
    Convert {
        input: PathBuf,
        #[arg(short, long, value_enum)]
        format: Option<Format>,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        force: bool,
        /// Write a machine-readable command report.
        #[arg(long)]
        report: Option<PathBuf>,
        /// Export even when IR validation fails.
        #[arg(long)]
        allow_invalid: bool,
        /// Write a geometry format even when decoding transferred no geometry.
        #[arg(long)]
        allow_empty: bool,
        #[command(flatten)]
        input_args: InputArgs,
        #[command(flatten)]
        decode: DecodeArgs,
    },
}

fn main() -> ExitCode {
    let registry = Registry::with_builtins();
    let result = match Cli::parse().command {
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
            input_args,
            decode,
        } => commands::export(
            &registry,
            &input,
            format,
            output.as_deref(),
            commands::ExportSettings {
                force,
                report,
                allow_empty,
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
            input_args,
            decode,
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
