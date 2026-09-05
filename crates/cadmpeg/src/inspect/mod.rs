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
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use cadmpeg_core::decode::alloc_filled;
use clap::{Args, Subcommand};

use crate::LimitProfile;
use numeric::{parse_offset, EndianArgs, ScalarType};

/// Default number of bytes a bare `inspect hex` prints.
const DEFAULT_HEX_LEN: u64 = 256;

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

/// The file a single-input byte tool reads, under either spelling.
///
/// The positional form is canonical; `--input FILE` is a tolerated guessed
/// spelling. Exactly one of the pair must be present, and giving both is a
/// clap conflict error.
#[derive(Debug, Args)]
pub struct FileArg {
    /// File to read.
    #[arg(value_name = "FILE", required_unless_present = "input_flag")]
    pub file: Option<PathBuf>,
    /// Tolerated spelling of the positional file.
    #[arg(
        long = "input",
        value_name = "FILE",
        hide = true,
        conflicts_with = "file"
    )]
    pub input_flag: Option<PathBuf>,
}

impl FileArg {
    /// Returns the file under whichever spelling was given.
    pub fn path(&self) -> &Path {
        self.file
            .as_deref()
            .or(self.input_flag.as_deref())
            .expect("clap requires one file spelling")
    }
}

/// Rejects `--json` on inspect tools that have no JSON form.
///
/// The teaching text is pinned by `json_on_a_tool_without_a_json_form_teaches_where_json_lives`.
fn reject_inspect_json(_: &str) -> Result<bool, String> {
    Err(
        "this inspect tool has no JSON form; JSON lives on `inspect FILE --json` \
         (the container summary), `inspect container --json`, and `inspect \
         find --json`"
            .into(),
    )
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
    #[arg(long, default_value_t = 16)]
    pub width: usize,
    /// Rejected placeholder: this tool has no JSON form.
    #[arg(
        long,
        hide = true,
        num_args = 0,
        default_missing_value = "true",
        value_parser = reject_inspect_json
    )]
    pub json: bool,
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
    #[arg(long, alias = "step", value_parser = parse_offset)]
    pub stride: Option<u64>,
    #[command(flatten)]
    pub endian: EndianArgs,
    /// Rejected placeholder: this tool has no JSON form.
    #[arg(
        long,
        hide = true,
        num_args = 0,
        default_missing_value = "true",
        value_parser = reject_inspect_json
    )]
    pub json: bool,
}

/// Arguments for `cadmpeg inspect find`.
#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("needle").args(["hex", "ascii", "utf16le"])))]
pub struct FindArgs {
    /// File to search.
    #[arg(value_name = "FILE", required_unless_present = "input_flag")]
    pub file: Option<PathBuf>,
    /// Tolerated spelling of the positional file. Deliberately not a clap
    /// conflict with the positional: when both appear, the positional slot
    /// caught a misplaced search pattern, and the runner explains that.
    #[arg(long = "input", value_name = "FILE", hide = true)]
    pub input_flag: Option<PathBuf>,
    /// Rejected placeholder: the pattern belongs to `--hex`, `--ascii`, or
    /// `--utf16le`, because a bare word cannot say how to encode it.
    #[arg(hide = true)]
    pub misplaced_pattern: Option<String>,
    /// Hexadecimal byte pattern; `??` matches any byte.
    #[arg(long)]
    pub hex: Option<String>,
    /// ASCII string to search for.
    #[arg(long)]
    pub ascii: Option<String>,
    /// String to search for encoded as UTF-16LE.
    #[arg(long)]
    pub utf16le: Option<String>,
    /// Stop after this many hits; 0 reports every hit.
    #[arg(long, default_value_t = 100)]
    pub max: usize,
    /// Bytes of context dumped before and after each hit; 0 prints none.
    #[arg(long, default_value = "0", value_parser = parse_offset)]
    pub context: u64,
    /// Rejected placeholder: `find` names the encoding by flag, not by a
    /// `--type` value.
    #[arg(long = "type", hide = true)]
    pub misplaced_type: Option<String>,
    /// Print the hits as versioned JSON instead of the table.
    #[arg(long, conflicts_with = "context")]
    pub json: bool,
}

/// A search pattern with its selected encoding.
#[derive(Debug, Clone, Copy)]
enum Needle<'a> {
    Hex(&'a str),
    Ascii(&'a str),
    Utf16le(&'a str),
}

impl FindArgs {
    /// Resolves the flat clap fields into one search mode.
    fn mode(&self) -> Result<Needle<'_>> {
        let misplaced = match (&self.input_flag, &self.file) {
            (Some(_), Some(stray)) => Some(stray.display().to_string()),
            (Some(_), None) | (None, Some(_)) => self.misplaced_pattern.clone(),
            (None, None) => unreachable!("clap requires one file spelling"),
        };
        if let Some(stray) = misplaced {
            bail!(
                "`{stray}` is an extra positional argument; the search pattern is named by a flag \
                 because a bare word does not say how to encode it: pass `--hex {stray}` for a byte \
                 pattern, `--ascii {stray}` for text, or `--utf16le {stray}` for UTF-16LE text"
            );
        }
        if let Some(guessed) = &self.misplaced_type {
            let flag = match guessed.to_ascii_lowercase().as_str() {
                "hex" | "bytes" => "--hex PATTERN",
                text if text.starts_with("utf16") || text.starts_with("utf-16") => "--utf16le TEXT",
                _ => "--ascii TEXT",
            };
            bail!(
                "`--type {guessed}` does not select an encoding here; `find` names the pattern \
                 encoding by flag: pass `{flag}` (the choices are --hex, --ascii, and --utf16le)"
            );
        }
        match (&self.hex, &self.ascii, &self.utf16le) {
            (Some(text), None, None) => Ok(Needle::Hex(text)),
            (None, Some(text), None) => Ok(Needle::Ascii(text)),
            (None, None, Some(text)) => Ok(Needle::Utf16le(text)),
            (None, None, None) => bail!("pass one of --hex, --ascii, or --utf16le"),
            _ => unreachable!("clap rejects conflicting search encodings"),
        }
    }
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
        default_value_t = 4
    )]
    pub min: usize,
    /// Which encodings to scan for.
    #[arg(long, value_enum, default_value_t = search::StringScan::Ascii)]
    pub encoding: search::StringScan,
    /// Rejected placeholder: this tool has no JSON form.
    #[arg(
        long,
        hide = true,
        num_args = 0,
        default_missing_value = "true",
        value_parser = reject_inspect_json
    )]
    pub json: bool,
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
    /// Rejected placeholder: this tool has no JSON form.
    #[arg(
        long,
        hide = true,
        num_args = 0,
        default_missing_value = "true",
        value_parser = reject_inspect_json
    )]
    pub json: bool,
}

/// Arguments for `cadmpeg inspect container`.
#[derive(Debug, Args)]
pub struct ContainerArgs {
    #[command(flatten)]
    pub file: FileArg,
    /// Print the entries as versioned JSON instead of the table.
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
    /// Output file for the extracted bytes; omit it or pass `-` to write
    /// them to standard output.
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,
    /// Replace an existing output file.
    #[arg(long)]
    pub force: bool,
    /// Resource-limit profile applied while reading the archive.
    #[arg(long, value_enum, default_value_t = LimitProfile::Desktop)]
    pub limits: LimitProfile,
    /// Rejected placeholder: this tool has no JSON form.
    #[arg(
        long,
        hide = true,
        num_args = 0,
        default_missing_value = "true",
        value_parser = reject_inspect_json
    )]
    pub json: bool,
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
    #[arg(long, default_value_t = 32)]
    pub max_runs: usize,
    /// Bytes of context dumped on each side of the first difference.
    #[arg(long, default_value = "32", value_parser = parse_offset)]
    pub context: u64,
    /// Rejected placeholder: this tool has no JSON form.
    #[arg(
        long,
        hide = true,
        num_args = 0,
        default_missing_value = "true",
        value_parser = reject_inspect_json
    )]
    pub json: bool,
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
        ByteTool::Read(args) => {
            let mode = args.endian.mode();
            read(&args, mode).map(|()| ExitCode::SUCCESS)
        }
        ByteTool::Find(args) => {
            let mode = args.mode()?;
            find(&args, mode).map(|()| ExitCode::SUCCESS)
        }
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
    if args.width == 0 {
        bail!("--width must be at least 1");
    }
    let len = args.len.unwrap_or(DEFAULT_HEX_LEN);
    let bytes = read_window(args.file.path(), args.offset, len)?;
    if bytes.is_empty() {
        println!("(no bytes at 0x{:x})", args.offset);
        return Ok(());
    }
    print!("{}", hexdump::render(args.offset, &bytes, args.width));
    Ok(())
}

fn read(args: &ReadArgs, endian: numeric::Endian) -> Result<()> {
    let width = args.ty.width() as u64;
    let stride = args.stride.unwrap_or(width);
    if args.count == 0 {
        return Ok(());
    }
    if stride == 0 {
        bail!("--stride 0 would read the same bytes forever");
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

fn find(args: &FindArgs, needle: Needle<'_>) -> Result<()> {
    let file = match (&args.input_flag, &args.file) {
        (Some(input), _) => input,
        (None, Some(file)) => file,
        (None, None) => unreachable!("clap requires one file spelling"),
    };
    let (pattern, described) = match needle {
        Needle::Hex(text) => (search::parse_pattern(text), format!("hex {text}")),
        Needle::Ascii(text) => (search::ascii_pattern(text), format!("ascii {text:?}")),
        Needle::Utf16le(text) => (search::utf16le_pattern(text), format!("utf16le {text:?}")),
    };
    let pattern = pattern.map_err(|message| anyhow::anyhow!(message))?;
    let bytes = read_whole(file)?;
    let limit = (args.max > 0).then_some(args.max);
    let hits = search::find_all(&bytes, &pattern, limit);
    let truncated = limit.is_some_and(|max| hits.len() >= max);
    if args.json {
        let payload = serde_json::json!({
            "pattern": described,
            "pattern_bytes": pattern.len(),
            "truncated": truncated,
            "hits": hits,
        });
        println!(
            "{}",
            crate::commands::reporting::command_report_json("inspect find", &payload)?
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
            args.max
        );
    }
    Ok(())
}

fn strings(args: &StringsArgs) -> Result<()> {
    if args.min == 0 {
        bail!("--min must be at least 1");
    }
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
    let record_size = layout.size as u64;
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
    let name_width = layout
        .fields
        .iter()
        .map(|field| field.name.len())
        .max()
        .unwrap_or(1);
    for index in 0..args.count {
        let start = (index * record_size) as usize;
        let record = &bytes[start..start + layout.size];
        let base = args.offset + index * record_size;
        println!("record {index} @ 0x{base:08x} ({record_size} bytes)");
        for field in layout.decode(record) {
            let at = base + field.offset as u64;
            println!(
                "  0x{at:08x}  {:<name_width$}  {:<8}  {:<24}  {}",
                field.name, field.type_name, field.decimal, field.hex
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
        None => write_payload_to_stdout(&payload),
        Some(path) if path == Path::new("-") => write_payload_to_stdout(&payload),
        Some(path) => {
            if path.exists() && !args.force {
                bail!("{} exists; pass --force to replace it", path.display());
            }
            std::fs::write(path, &payload)
                .with_context(|| format!("writing {}", path.display()))?;
            Ok(())
        }
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
        summary.len_a,
        args.b.display(),
        summary.len_b
    );
    if summary.identical() {
        println!("identical");
        return Ok(ExitCode::SUCCESS);
    }
    if summary.len_a != summary.len_b {
        println!(
            "length differs by {} bytes; only the first {} bytes are compared",
            summary.len_a.abs_diff(summary.len_b),
            summary.compared
        );
    }
    let Some(first) = summary.first else {
        println!("the common prefix is identical");
        return Ok(ExitCode::from(1));
    };
    println!(
        "first difference: 0x{first:08x} ({first})\ndiffering bytes: {} of {}\nruns (gap {}): {}",
        summary.differing,
        summary.compared,
        args.gap,
        summary.runs.len()
    );
    let shown = if args.max_runs == 0 {
        summary.runs.len()
    } else {
        args.max_runs.min(summary.runs.len())
    };
    for run in &summary.runs[..shown] {
        println!(
            "  0x{:08x}..0x{:08x}  {} bytes",
            run.start,
            run.end(),
            run.len
        );
    }
    if shown < summary.runs.len() {
        println!(
            "  … {} more runs (raise --max-runs)",
            summary.runs.len() - shown
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
    hexdump::render(begin as u64, &bytes[begin..end], 16)
}
