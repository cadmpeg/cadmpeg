// SPDX-License-Identifier: Apache-2.0
//! Format-agnostic byte tools under `cadmpeg inspect`.
//!
//! These subcommands read a file as bytes. They know nothing about CAD formats,
//! so they work on a container, on an entry extracted from one, and on a probe
//! variant that no codec accepts yet. `cadmpeg inspect FILE` without a
//! subcommand still runs the codec-aware container summary.
//!
//! Every offset and length argument accepts hexadecimal with an `0x` prefix or
//! decimal, with `_` allowed between digits.

pub mod container;
pub mod diff;
pub mod hexdump;
pub mod layout;
pub mod numeric;
pub mod search;

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use cadmpeg_core::decode::alloc_filled;
use clap::builder::TypedValueParser;
use clap::{Args, Subcommand, ValueEnum};

use crate::LimitProfile;
use numeric::{parse_offset, Endian, ScalarType};

/// Default number of bytes a bare `inspect hex` prints.
const DEFAULT_HEX_LEN: u64 = 256;

/// A container summary or a byte tool with its own input arguments.
#[derive(Debug)]
pub enum InspectArgs {
    Summary(SummaryArgs),
    Bytes(ByteCommand),
}

/// Arguments for a codec-aware container summary.
#[derive(Debug, Args)]
pub struct SummaryArgs {
    #[command(flatten)]
    pub file: FileArg,
    /// Write JSON to standard output.
    #[arg(long)]
    pub json: bool,
    /// Write a JSON report to this file.
    #[arg(short = 'o', long, visible_alias = "output")]
    pub report: Option<PathBuf>,
    /// Replace an existing report file.
    #[arg(long)]
    pub force: bool,
    /// Resource-limit profile applied during inspection.
    #[arg(long, value_enum, default_value_t = LimitProfile::Desktop)]
    pub limits: LimitProfile,
    /// Treat the input as this native format.
    #[arg(long, visible_alias = "from", value_parser = native_input_parser())]
    pub input_format: Option<&'static cadmpeg_registry::NativeDescriptor>,
}

fn native_input_parser(
) -> impl TypedValueParser<Value = &'static cadmpeg_registry::NativeDescriptor> {
    clap::builder::PossibleValuesParser::new(cadmpeg_registry::input_names().filter(|name| {
        matches!(
            cadmpeg_registry::forced_input(name),
            Some(cadmpeg_registry::ForcedInput::Codec(_))
        )
    }))
    .try_map(|name| match cadmpeg_registry::forced_input(&name) {
        Some(cadmpeg_registry::ForcedInput::Codec(native)) => Ok(native),
        _ => Err(format!("unsupported native input format: {name}")),
    })
}

impl clap::Args for InspectArgs {
    fn augment_args(command: clap::Command) -> clap::Command {
        ByteCommand::augment_subcommands(SummaryArgs::augment_args(command))
    }

    fn augment_args_for_update(command: clap::Command) -> clap::Command {
        Self::augment_args(command)
    }
}

impl clap::FromArgMatches for InspectArgs {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        if matches.subcommand_name().is_some() {
            ByteCommand::from_arg_matches(matches).map(Self::Bytes)
        } else {
            SummaryArgs::from_arg_matches(matches).map(Self::Summary)
        }
    }

    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

/// Byte-level subcommands of `cadmpeg inspect`.
///
/// The tools run directly, as `cadmpeg inspect hex FILE`. The hidden `bytes`
/// group accepts the same tools one level deeper, as `cadmpeg inspect bytes hex
/// FILE`, so the guessed spelling reaches the tool and its `--help` instead of
/// a subcommand-conflict error.
#[derive(Debug, Subcommand)]
pub enum ByteCommand {
    /// One byte tool named directly under `inspect`.
    #[command(flatten)]
    Tool(ByteTool),
    /// The same byte tools under an explicit `bytes` group.
    #[command(hide = true)]
    Bytes {
        /// Byte tool to run.
        #[command(subcommand)]
        tool: ByteTool,
    },
}

/// One format-agnostic byte tool.
#[derive(Debug, Subcommand)]
pub enum ByteTool {
    /// Dump bytes as hex.
    ///
    /// Prints a hexadecimal dump with absolute offsets and an ASCII gutter.
    Hex(HexArgs),
    /// Read numbers at an offset.
    ///
    /// Reads fixed-width scalars at an offset, optionally striding a record array.
    Read(ReadArgs),
    /// Search for a byte pattern or string.
    Find(FindArgs),
    /// List printable strings.
    Strings(StringsArgs),
    /// Decode records from a layout spec.
    Struct(StructArgs),
    /// List container members (ZIP or CFB).
    Container(ContainerArgs),
    /// Write one ZIP entry or CFB stream.
    Extract(ExtractArgs),
    /// Compare two files as raw bytes.
    ///
    /// Compares byte n of one file with byte n of the other.
    /// `cadmpeg diff` compares decoded models.
    /// Exit status 1 means the files differ.
    Cmp(CmpArgs),
}

/// One resolved input file.
#[derive(Debug)]
pub struct FileArg {
    path: PathBuf,
}

impl FileArg {
    /// Returns the resolved input path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl clap::Args for FileArg {
    fn augment_args(command: clap::Command) -> clap::Command {
        let input = clap::Arg::new("input_flag")
            .long("input")
            .value_name("FILE")
            .help("Tolerated spelling of the positional file")
            .hide(true)
            .value_parser(clap::value_parser!(PathBuf));
        let input = input.conflicts_with("file");
        let value_name = if command.get_name() == "inspect" {
            "INPUT"
        } else {
            "FILE"
        };
        command
            .arg(
                clap::Arg::new("file")
                    .value_name(value_name)
                    .help("File to read")
                    .required_unless_present("input_flag")
                    .value_parser(clap::value_parser!(PathBuf)),
            )
            .arg(input)
    }

    fn augment_args_for_update(command: clap::Command) -> clap::Command {
        Self::augment_args(command)
    }
}

impl clap::FromArgMatches for FileArg {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        matches
            .get_one::<PathBuf>("file")
            .or_else(|| matches.get_one::<PathBuf>("input_flag"))
            .cloned()
            .map(|path| Self { path })
            .ok_or_else(|| {
                clap::Error::raw(
                    clap::error::ErrorKind::MissingRequiredArgument,
                    "file is required",
                )
            })
    }

    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

/// Arguments for `cadmpeg inspect hex`.
#[derive(Debug, Args)]
pub struct HexArgs {
    #[command(flatten)]
    pub file: FileArg,
    /// First byte to print.
    #[arg(long, alias = "start", default_value = "0", value_parser = parse_offset)]
    pub offset: u64,
    /// Number of bytes to print; the dump stops early at end of file.
    #[arg(long, visible_alias = "length", value_parser = parse_offset)]
    pub len: Option<u64>,
    /// Bytes per output line.
    #[arg(long, default_value = "16")]
    pub width: NonZeroUsize,
    #[command(flatten)]
    _reject_json: crate::reject_json::RejectJson,
}

fn parse_stride(text: &str) -> Result<NonZeroU64, String> {
    NonZeroU64::new(parse_offset(text)?).ok_or_else(|| "stride must be at least 1".to_owned())
}

type CountLimit = Option<NonZeroUsize>;

fn parse_limit(text: &str) -> Result<CountLimit, std::num::ParseIntError> {
    text.parse::<usize>().map(NonZeroUsize::new)
}

/// Arguments for `cadmpeg inspect read`.
#[derive(Debug, Args)]
pub struct ReadArgs {
    #[command(flatten)]
    pub file: FileArg,
    /// Scalar type to decode.
    #[arg(long = "type", value_parser = numeric::ScalarTypeParser)]
    pub ty: ScalarType,
    /// Offset of the first value.
    #[arg(long, alias = "start", default_value = "0", value_parser = parse_offset)]
    pub offset: u64,
    /// How many values to read.
    #[arg(short = 'n', long, default_value_t = 1)]
    pub count: u64,
    /// Byte step between consecutive values; defaults to the scalar width.
    #[arg(long, alias = "step", value_parser = parse_stride)]
    pub stride: Option<NonZeroU64>,
    /// Byte order for scalar reads.
    #[arg(long, value_enum, default_value_t = Endian::Le)]
    pub endian: Endian,
    #[command(flatten)]
    _reject_json: crate::reject_json::RejectJson,
}

/// Arguments for `cadmpeg inspect find`.
#[derive(Debug, Args)]
pub struct FindArgs {
    #[command(flatten)]
    pub input: FindInput,
    /// Pattern encoding.
    #[arg(long, value_enum)]
    pub encoding: FindEncoding,
    /// Stop after this many hits; 0 reports every hit.
    #[arg(long, default_value = "100", value_parser = parse_limit)]
    pub max: CountLimit,
    /// Bytes of context dumped before and after each hit; 0 prints none.
    #[arg(long, default_value = "0", value_parser = parse_offset)]
    pub context: u64,
    /// Print the hits as JSON instead of the table.
    #[arg(long, conflicts_with = "context")]
    pub json: bool,
}

/// A resolved search file and pattern.
#[derive(Debug)]
pub struct FindInput {
    file: PathBuf,
    needle: String,
}

impl clap::Args for FindInput {
    fn augment_args(command: clap::Command) -> clap::Command {
        command
            .arg(clap::Arg::new("search_operands")
                .value_name("FILE NEEDLE")
                .help("File and pattern, or just the pattern with --input; hex accepts ?? wildcards")
                .num_args(1..=2)
                .action(clap::ArgAction::Append)
                .required(true)
                .value_parser(clap::value_parser!(std::ffi::OsString)))
            .arg(clap::Arg::new("input_flag")
                .long("input")
                .value_name("FILE")
                .hide(true)
                .value_parser(clap::value_parser!(PathBuf)))
    }

    fn augment_args_for_update(command: clap::Command) -> clap::Command {
        Self::augment_args(command)
    }
}

impl clap::FromArgMatches for FindInput {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        let operands: Vec<_> = matches
            .get_many::<std::ffi::OsString>("search_operands")
            .into_iter()
            .flatten()
            .collect();
        let (file, needle) = match (
            matches.get_one::<PathBuf>("input_flag"),
            operands.as_slice(),
        ) {
            (None, [file, needle]) => (PathBuf::from(file), *needle),
            (Some(file), [needle]) => (file.clone(), *needle),
            (Some(_), [_, _, ..]) => {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::ArgumentConflict,
                    "--input cannot be used with a positional file",
                ))
            }
            (None, [_, _, _, ..]) => {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::TooManyValues,
                    "expected only FILE and NEEDLE",
                ))
            }
            _ => {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::MissingRequiredArgument,
                    "required arguments: FILE and NEEDLE",
                ))
            }
        };
        let needle = needle
            .to_str()
            .ok_or_else(|| {
                clap::Error::raw(
                    clap::error::ErrorKind::InvalidUtf8,
                    "NEEDLE must be valid UTF-8",
                )
            })?
            .to_owned();
        Ok(Self { file, needle })
    }

    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

/// Encoding of a search pattern.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum FindEncoding {
    /// Hexadecimal bytes with optional `??` wildcards.
    Hex,
    /// ASCII text.
    Ascii,
    /// UTF-16LE text.
    Utf16le,
}

/// Arguments for `cadmpeg inspect strings`.
#[derive(Debug, Args)]
pub struct StringsArgs {
    #[command(flatten)]
    pub file: FileArg,
    /// Shortest run to report, in characters.
    #[arg(
        long,
        visible_alias = "min-len",
        alias = "min-length",
        default_value = "4"
    )]
    pub min: NonZeroUsize,
    /// Which encodings to scan for.
    #[arg(long, value_enum, default_value_t = search::StringScan::Ascii)]
    pub encoding: search::StringScan,
    #[command(flatten)]
    _reject_json: crate::reject_json::RejectJson,
}

/// Arguments for `cadmpeg inspect struct`.
#[derive(Debug, Args)]
pub struct StructArgs {
    #[command(flatten)]
    pub file: FileArg,
    /// Record layout, for example `u32le:count,pad4,f64le:x,f64le:y`.
    #[arg(long)]
    pub layout: String,
    /// Offset of the first record.
    #[arg(long, alias = "start", default_value = "0", value_parser = parse_offset)]
    pub offset: u64,
    /// How many consecutive records to decode.
    #[arg(short = 'n', long, default_value_t = 1)]
    pub count: u64,
    #[command(flatten)]
    _reject_json: crate::reject_json::RejectJson,
}

/// Arguments for `cadmpeg inspect container`.
#[derive(Debug, Args)]
pub struct ContainerArgs {
    #[command(flatten)]
    pub file: FileArg,
    /// Print the entries as JSON instead of the table.
    #[arg(long)]
    pub json: bool,
    /// Resource-limit profile applied while reading the central directory.
    #[arg(long, value_enum, default_value_t = LimitProfile::Desktop)]
    pub limits: LimitProfile,
}

/// Arguments for `cadmpeg inspect extract`.
#[derive(Debug, Args)]
pub struct ExtractArgs {
    /// ZIP or CFB file to read.
    pub file: PathBuf,
    /// Exact entry or stream path (quotes removed).
    pub member: String,
    /// Extracted-byte destination.
    #[command(flatten)]
    pub output: ExtractDestination,
    /// Resource-limit profile applied while reading the archive.
    #[arg(long, value_enum, default_value_t = LimitProfile::Desktop)]
    pub limits: LimitProfile,
    #[command(flatten)]
    _reject_json: crate::reject_json::RejectJson,
}

/// Extracted-byte destination and its file overwrite policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractDestination {
    /// Write bytes to standard output.
    Stdout,
    /// Write bytes with the file overwrite policy.
    File(crate::application::artifact_store::FileDestination),
}

fn parse_destination(value: std::ffi::OsString) -> ExtractDestination {
    if value == "-" {
        ExtractDestination::Stdout
    } else {
        ExtractDestination::File(crate::application::artifact_store::FileDestination {
            path: value.into(),
            overwrite: false,
        })
    }
}

impl clap::Args for ExtractDestination {
    fn augment_args(command: clap::Command) -> clap::Command {
        command
            .arg(
                clap::Arg::new("output")
                    .short('o')
                    .long("output")
                    .help("Output file; omit it or pass - for standard output")
                    .default_value("-")
                    .value_parser(clap::builder::OsStringValueParser::new().map(parse_destination)),
            )
            .arg(
                clap::Arg::new("force")
                    .long("force")
                    .help("Replace an existing output file")
                    .action(clap::ArgAction::SetTrue),
            )
    }

    fn augment_args_for_update(command: clap::Command) -> clap::Command {
        Self::augment_args(command)
    }
}

impl clap::FromArgMatches for ExtractDestination {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        let mut destination = matches
            .get_one::<Self>("output")
            .cloned()
            .unwrap_or(Self::Stdout);
        if let Self::File(file) = &mut destination {
            file.overwrite = matches.get_flag("force");
        }
        Ok(destination)
    }

    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

/// Arguments for `cadmpeg inspect cmp`.
#[derive(Debug, Args)]
pub struct CmpArgs {
    /// First file.
    pub a: PathBuf,
    /// Second file.
    pub b: PathBuf,
    /// Merge two differing spans separated by this many equal bytes or fewer.
    #[arg(long, default_value_t = 8)]
    pub gap: u64,
    /// Stop listing after this many runs; 0 lists every run.
    #[arg(long, default_value = "32", value_parser = parse_limit)]
    pub max_runs: CountLimit,
    /// Bytes of context dumped on each side of the first difference.
    #[arg(long, default_value = "32", value_parser = parse_offset)]
    pub context: u64,
    #[command(flatten)]
    _reject_json: crate::reject_json::RejectJson,
}

/// Runs one byte subcommand.
///
/// # Errors
///
/// Returns an operational error when a file cannot be read, an argument does
/// not parse, or a requested offset lies past end of file.
pub fn run(command: ByteCommand) -> Result<ExitCode> {
    let tool = match command {
        ByteCommand::Tool(tool) | ByteCommand::Bytes { tool } => tool,
    };
    match tool {
        ByteTool::Hex(args) => hex(&args).map(|()| ExitCode::SUCCESS),
        ByteTool::Read(args) => read(&args).map(|()| ExitCode::SUCCESS),
        ByteTool::Find(args) => find(&args).map(|()| ExitCode::SUCCESS),
        ByteTool::Strings(args) => strings(&args).map(|()| ExitCode::SUCCESS),
        ByteTool::Struct(args) => structure(&args).map(|()| ExitCode::SUCCESS),
        ByteTool::Container(args) => container_list(&args).map(|()| ExitCode::SUCCESS),
        ByteTool::Extract(args) => extract_entry(&args).map(|()| ExitCode::SUCCESS),
        ByteTool::Cmp(args) => cmp_files(&args),
    }
}

/// Returns the file length in bytes.
fn file_len(path: &Path) -> Result<u64> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("reading metadata for {}", path.display()))?;
    Ok(metadata.len())
}

/// Reads up to `len` bytes starting at `offset`, returning fewer at end of file.
///
/// Seeking past end of file is an error rather than an empty result, because an
/// offset outside the file is always a mistake worth reporting.
fn read_window(path: &Path, offset: u64, len: u64) -> Result<Vec<u8>> {
    let size = file_len(path)?;
    if offset > size {
        bail!(
            "offset 0x{offset:x} ({offset}) is past the end of {}, which is 0x{size:x} ({size}) bytes",
            path.display()
        );
    }
    let available = size - offset;
    let want = usize::try_from(len.min(available))
        .context("the requested length does not fit in memory on this target")?;
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    file.seek(SeekFrom::Start(offset))
        .with_context(|| format!("seeking to 0x{offset:x} in {}", path.display()))?;
    let mut buffer = alloc_filled(want, 0_u8, "cli inspect read window")?;
    file.read_exact(&mut buffer).with_context(|| {
        format!(
            "reading {want} bytes at 0x{offset:x} from {}",
            path.display()
        )
    })?;
    Ok(buffer)
}

/// Reads a whole file.
fn read_whole(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("reading {}", path.display()))
}

fn hex(args: &HexArgs) -> Result<()> {
    let len = args.len.unwrap_or(DEFAULT_HEX_LEN);
    let bytes = read_window(args.file.path(), args.offset, len)?;
    if bytes.is_empty() {
        println!("(no bytes at 0x{:x})", args.offset);
        return Ok(());
    }
    print!("{}", hexdump::render(args.offset, &bytes, args.width));
    Ok(())
}

fn read(args: &ReadArgs) -> Result<()> {
    let endian = args.endian;
    let width = args.ty.width() as u64;
    let stride = args.stride.map_or(width, NonZeroU64::get);
    if args.count == 0 {
        return Ok(());
    }
    let file_path = args.file.path();
    let size = file_len(file_path)?;
    let name = args.ty.display_name(endian);
    let mut file =
        File::open(file_path).with_context(|| format!("opening {}", file_path.display()))?;
    for index in 0..args.count {
        let offset = index
            .checked_mul(stride)
            .and_then(|step| args.offset.checked_add(step))
            .context("the strided offset overflows 64 bits")?;
        let end = offset
            .checked_add(width)
            .context("the read overflows 64 bits")?;
        if end > size {
            bail!(
                "value {index} needs bytes 0x{offset:x}..0x{end:x}, past the end of {} at 0x{size:x}",
                file_path.display()
            );
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut buffer = [0u8; 8];
        let slot = &mut buffer[..width as usize];
        file.read_exact(slot)?;
        let value = args.ty.read(slot, endian);
        println!(
            "0x{offset:08x}  {name:<6}  {:<24}  {}",
            value.decimal(),
            value.hex()
        );
    }
    Ok(())
}

fn find(args: &FindArgs) -> Result<()> {
    let file = &args.input.file;
    let text = &args.input.needle;
    let (pattern, described) = match args.encoding {
        FindEncoding::Hex => (search::parse_pattern(text), format!("hex {text}")),
        FindEncoding::Ascii => (search::ascii_pattern(text), format!("ascii {text:?}")),
        FindEncoding::Utf16le => (search::utf16le_pattern(text), format!("utf16le {text:?}")),
    };
    let pattern = pattern.map_err(|message| anyhow::anyhow!(message))?;
    let bytes = read_whole(file)?;
    let limit = args.max;
    let hits = search::find_all(&bytes, &pattern, limit);
    let truncated = limit.is_some_and(|max| hits.len() >= max.get());
    if args.json {
        let payload = serde_json::json!({
            "subcommand": "find",
            "pattern": described,
            "pattern_bytes": pattern.len(),
            "truncated": truncated,
            "hits": hits,
        });
        println!(
            "{}",
            crate::commands::reporting::command_report_json("inspect", &payload)?
        );
        return Ok(());
    }
    println!(
        "pattern: {described} ({} bytes)  hits: {}{}",
        pattern.len(),
        hits.len(),
        if truncated {
            " (truncated by --max)"
        } else {
            ""
        }
    );
    for offset in &hits {
        println!("0x{offset:08x}  {offset}");
        if args.context > 0 {
            let start = offset.saturating_sub(args.context);
            let len = args
                .context
                .saturating_mul(2)
                .saturating_add(pattern.len() as u64);
            print!("{}", window(&bytes, start, len));
        }
    }
    if truncated {
        println!(
            "note: output truncated at {} matches; pass --max 0 for all",
            hits.len()
        );
    }
    Ok(())
}

fn strings(args: &StringsArgs) -> Result<()> {
    let bytes = read_whole(args.file.path())?;
    for found in search::extract_strings(&bytes, args.min, args.encoding) {
        println!(
            "0x{:08x}  {:<8}  \"{}\"",
            found.offset,
            found.encoding.label(),
            search::escape(&found.text)
        );
    }
    Ok(())
}

fn structure(args: &StructArgs) -> Result<()> {
    let layout = layout::Layout::parse(&args.layout)?;
    if args.count == 0 {
        return Ok(());
    }
    let file_path = args.file.path();
    let size = file_len(file_path)?;
    let record_size = layout.size() as u64;
    let span = record_size
        .checked_mul(args.count)
        .and_then(|total| args.offset.checked_add(total))
        .context("the requested records overflow a 64-bit offset")?;
    if span > size {
        bail!(
            "{} records of {record_size} bytes at 0x{:x} need 0x{span:x} bytes, \
             but {} is 0x{size:x} bytes",
            args.count,
            args.offset,
            file_path.display()
        );
    }
    let bytes = read_window(file_path, args.offset, span - args.offset)?;
    let name_width = layout.names().map(str::len).max().unwrap_or(1);
    for index in 0..args.count {
        let start = (index * record_size) as usize;
        let record = &bytes[start..start + layout.size()];
        let base = args.offset + index * record_size;
        println!("record {index} @ 0x{base:08x} ({record_size} bytes)");
        for field in layout.decode(record) {
            let at = base + field.offset as u64;
            println!(
                "  0x{at:08x}  {:<name_width$}  {:<8}  {:<24}  {}",
                field.name,
                field.type_name,
                field.decimal.as_deref().unwrap_or(""),
                field.hex
            );
        }
    }
    Ok(())
}

fn container_list(args: &ContainerArgs) -> Result<()> {
    let file_path = args.file.path();
    let bytes = read_whole(file_path)?;
    let listing = container::list(&bytes, args.limits.limits()).with_context(|| {
        format!(
            "cannot list {} as a ZIP or CFB container; `cadmpeg inspect {}` reads \
             the other container families through their codec",
            file_path.display(),
            file_path.display()
        )
    })?;
    if args.json {
        print!("{}", container::render_json(&listing));
    } else {
        print!("{}", container::render(&listing));
    }
    Ok(())
}

fn extract_entry(args: &ExtractArgs) -> Result<()> {
    let bytes = read_whole(&args.file)?;
    let payload = container::extract(&bytes, args.limits.limits(), &args.member)
        .with_context(|| format!("extracting from {}", args.file.display()))?;
    match &args.output {
        ExtractDestination::Stdout => write_payload_to_stdout(&payload),
        ExtractDestination::File(destination) => destination.write(&args.file, &payload),
    }
}

/// Writes extracted bytes to standard output without any rendering.
fn write_payload_to_stdout(payload: &[u8]) -> Result<()> {
    use std::io::Write as _;
    std::io::stdout()
        .lock()
        .write_all(payload)
        .context("writing the entry to standard output")
}

fn cmp_files(args: &CmpArgs) -> Result<ExitCode> {
    let a = read_whole(&args.a)?;
    let b = read_whole(&args.b)?;
    let summary = diff::compare(&a, &b, args.gap);
    println!(
        "a: {} ({} bytes)\nb: {} ({} bytes)",
        args.a.display(),
        summary.len_a(),
        args.b.display(),
        summary.len_b()
    );
    if summary.identical() {
        println!("identical");
        return Ok(ExitCode::SUCCESS);
    }
    if summary.len_a() != summary.len_b() {
        println!(
            "length differs by {} bytes; only the first {} bytes are compared",
            summary.len_a().abs_diff(summary.len_b()),
            summary.compared()
        );
    }
    let Some(first) = summary.first() else {
        println!("the common prefix is identical");
        return Ok(ExitCode::from(1));
    };
    println!(
        "first difference: 0x{first:08x} ({first})\ndiffering bytes: {} of {}\nruns (gap {}): {}",
        summary.differing(),
        summary.compared(),
        args.gap,
        summary.runs().len()
    );
    let shown = args.max_runs.map_or(summary.runs().len(), |max| {
        max.get().min(summary.runs().len())
    });
    for run in &summary.runs()[..shown] {
        println!(
            "  0x{:08x}..0x{:08x}  {} bytes",
            run.start,
            run.end(),
            run.len
        );
    }
    if shown < summary.runs().len() {
        println!(
            "  … {} more runs (raise --max-runs)",
            summary.runs().len() - shown
        );
    }
    if args.context > 0 {
        let window_start = first.saturating_sub(args.context / 2);
        println!("\na @ 0x{window_start:x}:");
        print!("{}", window(&a, window_start, args.context));
        println!("b @ 0x{window_start:x}:");
        print!("{}", window(&b, window_start, args.context));
    }
    Ok(ExitCode::from(1))
}

/// Renders a bounded hexadecimal window of an in-memory buffer.
fn window(bytes: &[u8], start: u64, len: u64) -> String {
    let begin = usize::try_from(start)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    let end = usize::try_from(start.saturating_add(len))
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    hexdump::render(
        begin as u64,
        &bytes[begin..end],
        const { NonZeroUsize::new(16).unwrap() },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::FromArgMatches;

    #[test]
    fn inspect_rejects_cadir_at_argument_admission() {
        use clap::Parser;
        let error =
            crate::Cli::try_parse_from(["cadmpeg", "inspect", "missing", "--from", "cadir"])
                .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
    }

    #[test]
    fn extract_stdout_spellings_have_one_destination() {
        let parse = |args: Vec<&str>| {
            let matches = ExtractArgs::augment_args(clap::Command::new("extract"))
                .try_get_matches_from(args)
                .unwrap();
            ExtractArgs::from_arg_matches(&matches).unwrap()
        };
        let omitted = parse(vec!["extract", "input.zip", "entry"]);
        let explicit = parse(vec!["extract", "input.zip", "entry", "-o", "-", "--force"]);
        assert_eq!(omitted.output, explicit.output);
    }
}
