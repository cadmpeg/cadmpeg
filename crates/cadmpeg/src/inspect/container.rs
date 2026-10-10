// SPDX-License-Identifier: Apache-2.0
//! Container member listing (ZIP or CFB) and exact member extraction.

use anyhow::{Context, Result};
use cadmpeg_container::compound::{CompoundAllocation, CompoundEntry, CompoundSnapshot};
use cadmpeg_container::{ArchiveSnapshot, EntryRecord};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceLimits};
use cadmpeg_core::{CodecError, ReadSeek};
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};

const CFB_MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const SUGGESTION_COUNT: usize = 10;

enum ContainerKind {
    Cfb,
    Zip,
}

fn detect(bytes: &[u8]) -> ContainerKind {
    if bytes.starts_with(&CFB_MAGIC) {
        ContainerKind::Cfb
    } else {
        ContainerKind::Zip
    }
}

/// An empty stream owns no sectors, so it has no allocation.
fn allocation_label(allocation: Option<CompoundAllocation>) -> Option<&'static str> {
    allocation.map(|allocation| match allocation {
        CompoundAllocation::Regular => "fat",
        CompoundAllocation::Mini => "mini-fat",
    })
}

/// Container members listed from a ZIP archive or a CFB file.
enum Listing<'entries> {
    /// ZIP central-directory entries.
    Zip(&'entries [EntryRecord]),
    /// CFB directory rows (storages and streams).
    Cfb(&'entries [CompoundEntry]),
}

struct MemberNameSuggestions<'entries> {
    first: [&'entries str; SUGGESTION_COUNT],
    first_count: usize,
    close: [&'entries str; SUGGESTION_COUNT],
    close_count: usize,
}

/// Selects the first names and near names from one charged step per source row.
///
/// The iterator must yield one item for every inspected row. A row without a
/// searchable name is `None`; filtering it before this function would skip its
/// visit charge.
fn select_member_names<'entries, I, F>(
    ctx: &DecodeContext<'_>,
    entries: I,
    operation: &'static str,
    mut is_near: F,
) -> Result<MemberNameSuggestions<'entries>, CodecError>
where
    I: Iterator<Item = Option<&'entries str>>,
    F: FnMut(&str) -> Result<bool, CodecError>,
{
    let mut entries = entries;
    let mut first = [""; SUGGESTION_COUNT];
    let mut first_count = 0;
    let mut close = [""; SUGGESTION_COUNT];
    let mut close_count = 0;

    while let Some(name) = ctx.next_charged(&mut entries, operation)? {
        let Some(name) = name else {
            continue;
        };
        if first_count < SUGGESTION_COUNT {
            first[first_count] = name;
            first_count += 1;
        }
        if close_count < SUGGESTION_COUNT && is_near(name)? {
            close[close_count] = name;
            close_count += 1;
        }
        if close_count == SUGGESTION_COUNT {
            break;
        }
    }

    Ok(MemberNameSuggestions {
        first,
        first_count,
        close,
        close_count,
    })
}

/// Acquires and lists ZIP entries or CFB directory members from `reader`.
///
/// # Errors
///
/// Returns an error when input acquisition exceeds the resource-limit profile
/// or the ZIP central directory or CFB directory does not parse.
pub(super) fn list<R: ReadSeek + ?Sized>(
    reader: &mut R,
    limits: ResourceLimits,
    json: bool,
) -> Result<String> {
    with_root(reader, limits, |ctx, root| match detect(root.window()) {
        ContainerKind::Cfb => {
            let snapshot = CompoundSnapshot::new(ctx, root).context("reading the CFB directory")?;
            let listing = Listing::Cfb(snapshot.entries());
            if json {
                render_json(&listing)
            } else {
                render(&listing)
            }
        }
        ContainerKind::Zip => {
            let snapshot =
                ArchiveSnapshot::new(ctx, root).context("reading the ZIP central directory")?;
            let listing = Listing::Zip(snapshot.entries());
            if json {
                render_json(&listing)
            } else {
                render(&listing)
            }
        }
    })
}

/// Runs one inspection operation with a single context from bounded input
/// acquisition through parsing and session finalization.
fn with_root<R: ReadSeek + ?Sized, T>(
    reader: &mut R,
    limits: ResourceLimits,
    parse: impl for<'bytes, 'ctx> FnOnce(
        &'ctx DecodeContext<'bytes>,
        cadmpeg_core::decode::View<'bytes>,
    ) -> Result<T>,
) -> Result<T> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy {
        limits,
        ..DecodePolicy::default()
    };
    let (ctx, root) = DecodeContext::read_root(reader, &arena, &policy, false)
        .context("the file does not fit the resource-limit profile")?;
    let result = parse(&ctx, root);
    ctx.finish(result)
}

/// Extracts one ZIP entry or CFB stream.
///
/// The input arena and decode session stay alive through member opening and
/// the output payload copy. Listing returns rendered text and does not expose
/// a session that extraction could reuse.
///
/// # Errors
///
/// Returns an error when the bytes are not a supported container within the
/// limit profile, when no stream or entry has exactly `name`, or when opening
/// the member fails structural, size, or integrity checks.
pub(super) fn extract<R: ReadSeek + ?Sized>(
    reader: &mut R,
    limits: ResourceLimits,
    name: &str,
) -> Result<Vec<u8>> {
    with_root(reader, limits, |ctx, root| match detect(root.window()) {
        ContainerKind::Cfb => {
            let snapshot = CompoundSnapshot::new(ctx, root).context("reading the CFB directory")?;
            let Some(entry) = snapshot.stream(ctx, name)? else {
                anyhow::bail!(missing_compound_member_message(ctx, &snapshot, name)?);
            };
            let view = snapshot
                .open(ctx, entry)
                .with_context(|| format!("opening stream {}", shell_quote(name)))?;
            Ok(view.window().to_vec())
        }
        ContainerKind::Zip => {
            let snapshot =
                ArchiveSnapshot::new(ctx, root).context("reading the ZIP central directory")?;
            let Some(entry) = snapshot.entry(ctx, name)? else {
                anyhow::bail!(missing_member_message(ctx, &snapshot, name)?);
            };
            let view = snapshot
                .open(ctx, &entry.name)
                .with_context(|| format!("opening entry {}", shell_quote(name)))?;
            Ok(view.window().to_vec())
        }
    })
}

fn near_name(
    ctx: &DecodeContext<'_>,
    path: &str,
    name: &str,
    lowercase_name: &str,
) -> Result<bool, CodecError> {
    let (contains, storage) =
        ctx.with_scoped_storage("inspect suggestion path lowercase", || {
            let lowercase_path = ctx.to_lowercase(path, "inspect suggestion path lowercase")?;
            ctx.contains_text(
                &lowercase_path,
                lowercase_name,
                "inspect suggestion substring search",
            )
        })?;
    drop(storage);
    if contains {
        return Ok(true);
    }
    let leaf = ctx
        .rsplit_once(path, "/", "inspect suggestion path leaf")?
        .map_or(path, |(_, leaf)| leaf);
    ctx.equal(leaf, name, "inspect suggestion path leaf comparison")
}

fn missing_compound_member_message(
    ctx: &DecodeContext<'_>,
    snapshot: &CompoundSnapshot<'_>,
    name: &str,
) -> Result<String, CodecError> {
    let (lowercase_name, name_storage) = ctx
        .with_scoped_storage("inspect CFB suggestion query lowercase", || {
            ctx.to_lowercase(name, "inspect CFB suggestion query lowercase")
        })?;
    let query = (|| -> std::result::Result<_, CodecError> {
        let selected = select_member_names(
            ctx,
            snapshot.entries().iter().map(|entry| match entry {
                CompoundEntry::Storage(_) => None,
                CompoundEntry::Stream(stream) => Some(stream.path()),
            }),
            "inspect CFB suggestion entries",
            |path| near_name(ctx, path, name, &lowercase_name),
        )?;
        if selected.close_count == 0 {
            Ok(("streams include", selected.first, selected.first_count))
        } else {
            Ok(("close stream names", selected.close, selected.close_count))
        }
    })();
    drop(lowercase_name);
    drop(name_storage);
    let (label, names, count) = query?;

    let names = names[..count]
        .iter()
        .map(|name| shell_quote(name))
        .collect::<Vec<_>>();
    Ok(format!(
        "no stream is named exactly {}; {label}: {}; run `cadmpeg inspect FILE` for the full list",
        shell_quote(name),
        names.join(", ")
    ))
}

/// Builds the error text for a member name with no exact match.
///
/// Names that contain the request case-insensitively, or whose final path
/// component equals it, are suggested first; with no near-miss the first
/// entries are listed instead. Every name uses the same shell quoting as
/// the listing.
fn missing_member_message(
    ctx: &DecodeContext<'_>,
    snapshot: &ArchiveSnapshot<'_, '_>,
    name: &str,
) -> Result<String, CodecError> {
    let (lowercase_name, name_storage) = ctx
        .with_scoped_storage("inspect ZIP suggestion query lowercase", || {
            ctx.to_lowercase(name, "inspect ZIP suggestion query lowercase")
        })?;
    let query = (|| -> std::result::Result<_, CodecError> {
        let selected = select_member_names(
            ctx,
            snapshot
                .entries()
                .iter()
                .map(|entry| Some(entry.name.as_str())),
            "inspect ZIP suggestion entries",
            |path| near_name(ctx, path, name, &lowercase_name),
        )?;
        if selected.close_count == 0 {
            Ok(("entries include", selected.first, selected.first_count))
        } else {
            Ok(("close names", selected.close, selected.close_count))
        }
    })();
    drop(lowercase_name);
    drop(name_storage);
    let (label, names, count) = query?;

    let names = names[..count]
        .iter()
        .map(|name| shell_quote(name))
        .collect::<Vec<_>>();
    Ok(format!(
        "no entry is named exactly {}; {label}: {}; run `cadmpeg inspect container FILE` \
         for the full list",
        shell_quote(name),
        names.join(", ")
    ))
}

/// Quotes an entry name so it survives a POSIX shell verbatim.
///
/// Fusion `.f3d` entry names hold `[` and `]`, which a shell expands as a glob
/// character class. Single quotes suppress every expansion, and an embedded
/// single quote is closed, escaped, and reopened.
fn shell_quote(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    out.push('\'');
    for c in name.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Formats an entry listing as the JSON command-report envelope.
///
/// Names are raw strings here — shell quoting belongs to the table
/// rendering, not to JSON.
fn render_json(listing: &Listing<'_>) -> Result<String> {
    #[derive(Serialize)]
    struct Body<'listing, 'entries> {
        container_kind: &'static str,
        entries: &'listing Listing<'entries>,
        subcommand: &'static str,
    }
    impl crate::commands::reporting::ReportBody for Body<'_, '_> {}

    let container_kind = match listing {
        Listing::Zip(_) => "zip",
        Listing::Cfb(_) => "cfb",
    };
    let payload = Body {
        subcommand: "container",
        container_kind,
        entries: listing,
    };
    let mut rendered = crate::commands::reporting::command_report_json("inspect", payload)?;
    rendered.push('\n');
    Ok(rendered)
}

impl Serialize for Listing<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct ZipRow<'entry> {
            central_start: u64,
            compressed_size: u64,
            compression: &'static str,
            crc32: u32,
            data_start: u64,
            header_start: u64,
            name: &'entry str,
            uncompressed_size: u64,
        }
        #[derive(Serialize)]
        struct CfbRow<'entry> {
            allocation: Option<&'static str>,
            directory_id: u32,
            kind: &'static str,
            path: &'entry str,
            size: Option<u64>,
        }

        match self {
            Self::Zip(entries) => {
                let mut sequence = serializer.serialize_seq(Some(entries.len()))?;
                for entry in *entries {
                    sequence.serialize_element(&ZipRow {
                        name: &entry.name,
                        compression: entry.compression.label(),
                        crc32: entry.crc32,
                        compressed_size: entry.compressed_size,
                        uncompressed_size: entry.uncompressed_size,
                        header_start: entry.header_start,
                        data_start: entry.data_start,
                        central_start: entry.central_start,
                    })?;
                }
                sequence.end()
            }
            Self::Cfb(entries) => {
                let mut sequence = serializer.serialize_seq(Some(entries.len()))?;
                for entry in *entries {
                    let (kind, size, allocation) = match entry {
                        CompoundEntry::Storage(_) => ("storage", None, None),
                        CompoundEntry::Stream(stream) => (
                            "stream",
                            Some(stream.logical_size()),
                            allocation_label(stream.allocation()),
                        ),
                    };
                    sequence.serialize_element(&CfbRow {
                        kind,
                        path: entry.path(),
                        size,
                        allocation,
                        directory_id: entry.directory_id(),
                    })?;
                }
                sequence.end()
            }
        }
    }
}

/// Formats an entry listing as an aligned table.
fn render(listing: &Listing<'_>) -> Result<String> {
    use std::fmt::Write as _;

    let mut out = String::new();
    match listing {
        Listing::Zip(entries) => {
            writeln!(
                out,
                "{:>10}  {:>10}  {:>12}  {:>12}  {:>8}  {:>10}  name",
                "header", "data", "packed", "unpacked", "method", "crc32"
            )?;
            for entry in *entries {
                writeln!(
                    out,
                    "0x{:08x}  0x{:08x}  {:>12}  {:>12}  {:>8}  0x{:08x}  {}",
                    entry.header_start,
                    entry.data_start,
                    entry.compressed_size,
                    entry.uncompressed_size,
                    entry.compression.label(),
                    entry.crc32,
                    shell_quote(&entry.name)
                )?;
            }
        }
        Listing::Cfb(entries) => {
            writeln!(
                out,
                "{:>4}  {:>8}  {:>12}  {:>8}  path",
                "id", "kind", "size", "alloc"
            )?;
            for entry in *entries {
                let (kind, allocation) = match entry {
                    CompoundEntry::Storage(_) => ("storage", ""),
                    CompoundEntry::Stream(stream) => (
                        "stream",
                        allocation_label(stream.allocation()).unwrap_or(""),
                    ),
                };
                write!(out, "{:>4}  {:>8}  ", entry.directory_id(), kind)?;
                match entry {
                    CompoundEntry::Storage(_) => write!(out, "{:>12}", "")?,
                    CompoundEntry::Stream(stream) => {
                        write!(out, "{:>12}", stream.logical_size())?;
                    }
                }
                writeln!(out, "  {:>8}  {}", allocation, shell_quote(entry.path()))?;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{extract, list, near_name, select_member_names, shell_quote, with_root};
    use anyhow::anyhow;
    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimits,
    };
    use cadmpeg_core::CodecError;
    use cadmpeg_test_support::compound::compound_fixture;
    use std::io::Cursor;

    #[test]
    fn quotes_plain_names() {
        assert_eq!(shell_quote("Document.xml"), "'Document.xml'");
    }

    #[test]
    fn quotes_bracketed_and_spaced_names() {
        assert_eq!(
            shell_quote("FusionAssetName[Active]/Design.dat"),
            "'FusionAssetName[Active]/Design.dat'"
        );
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("$HOME`x`"), "'$HOME`x`'");
    }

    #[test]
    fn closes_reopens_around_an_embedded_single_quote() {
        // 'it'\''s' concatenates to the four characters it's in a POSIX shell.
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn near_name_preserves_typed_path_lowercase_refusal() {
        let arena = DecodeArena::new();
        let mut limits = ResourceLimits::desktop();
        limits.max_work_units = 0;
        let policy = DecodePolicy {
            limits,
            ..DecodePolicy::default()
        };
        let ctx = DecodeContext::new(&arena, &policy, false);

        let error = near_name(&ctx, "Design/Streams.dat", "Payload", "payload")
            .expect_err("path lowercase refuses before unbounded work");
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("path lowercase refusal stays typed");
        };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.limit, 0);
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.operation, "inspect suggestion path lowercase");

        let error = ctx
            .finish_session()
            .expect_err("ignored path refusal remains fused in the session");
        let CodecError::ResourceLimit(fused) = error else {
            panic!("session finalization preserves the typed refusal");
        };
        assert_eq!(fused, refusal);
    }

    #[test]
    fn suggestion_scan_stops_after_ten_matches_before_the_limit() {
        let arena = DecodeArena::new();
        let mut limits = ResourceLimits::desktop();
        limits.max_work_units = 10;
        let policy = DecodePolicy {
            limits,
            ..DecodePolicy::default()
        };
        let ctx = DecodeContext::new(&arena, &policy, false);
        let names = [
            "entry-00", "entry-01", "entry-02", "entry-03", "entry-04", "entry-05", "entry-06",
            "entry-07", "entry-08", "entry-09", "entry-10", "entry-11",
        ];

        let selected = select_member_names(
            &ctx,
            names.iter().copied().map(Some),
            "inspect ZIP suggestion entries",
            |_| Ok(true),
        )
        .expect("the tenth match ends the scan before the work limit");

        assert_eq!(selected.first_count, 10);
        assert_eq!(selected.close_count, 10);
        assert_eq!(
            selected.first,
            [
                "entry-00", "entry-01", "entry-02", "entry-03", "entry-04", "entry-05", "entry-06",
                "entry-07", "entry-08", "entry-09",
            ]
        );
        assert_eq!(selected.close, selected.first);
        ctx.finish_session()
            .expect("the two unvisited rows do not consume work");
    }

    #[test]
    fn suggestion_scan_charges_cfb_storage_rows() {
        let names = [
            "stream-00",
            "stream-01",
            "stream-02",
            "stream-03",
            "stream-04",
            "stream-05",
            "stream-06",
            "stream-07",
            "stream-08",
            "stream-09",
        ];

        let arena = DecodeArena::new();
        let mut limits = ResourceLimits::desktop();
        limits.max_work_units = 10;
        let policy = DecodePolicy {
            limits,
            ..DecodePolicy::default()
        };
        let ctx = DecodeContext::new(&arena, &policy, false);
        let Err(error) = select_member_names(
            &ctx,
            std::iter::once(None).chain(names.iter().copied().map(Some)),
            "inspect CFB suggestion entries",
            |_| Ok(true),
        ) else {
            panic!("the storage row consumes one step before the tenth stream");
        };
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("storage-row work refusal stays typed");
        };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.limit, 10);
        assert_eq!(refusal.used, 10);
        assert_eq!(refusal.additional, 1);
        assert_eq!(refusal.operation, "inspect CFB suggestion entries");
        let error = ctx
            .finish_session()
            .expect_err("the storage-row refusal remains fused");
        let CodecError::ResourceLimit(fused) = error else {
            panic!("session finalization preserves the storage-row refusal");
        };
        assert_eq!(fused, refusal);

        let arena = DecodeArena::new();
        let mut limits = ResourceLimits::desktop();
        limits.max_work_units = 11;
        let policy = DecodePolicy {
            limits,
            ..DecodePolicy::default()
        };
        let ctx = DecodeContext::new(&arena, &policy, false);
        let selected = select_member_names(
            &ctx,
            std::iter::once(None).chain(names.iter().copied().map(Some)),
            "inspect CFB suggestion entries",
            |_| Ok(true),
        )
        .expect("one storage and ten streams fit eleven source steps");
        assert_eq!(selected.first_count, 10);
        assert_eq!(selected.close_count, 10);
        assert_eq!(selected.first, names);
        assert_eq!(selected.close, names);
        ctx.finish_session()
            .expect("the tenth match ends the scan before an end probe");
    }

    #[test]
    fn extracts_a_compound_stream_by_exact_path() {
        let mut file = Cursor::new(compound_fixture());
        assert_eq!(
            extract(&mut file, ResourceLimits::desktop(), "Payload")
                .expect("synthetic CFB stream extracts"),
            vec![0x5a; 4096]
        );
    }

    #[test]
    fn lists_compound_storages_and_streams() {
        let mut file = Cursor::new(compound_fixture());
        let listing =
            list(&mut file, ResourceLimits::desktop(), true).expect("synthetic CFB lists");
        let value: serde_json::Value =
            serde_json::from_str(&listing).expect("synthetic CFB JSON parses");
        let entries = value["entries"]
            .as_array()
            .expect("CFB entries are an array");
        let payload = entries
            .iter()
            .find(|entry| entry["path"] == "Payload")
            .expect("Payload row");
        assert_eq!(payload["kind"], "stream");
        assert_eq!(payload["size"], 4096);
    }

    #[test]
    fn bounded_acquisition_refuses_past_input_limit_before_container_parse() {
        let mut limits = ResourceLimits::desktop();
        limits.max_input_bytes = 4;

        let mut listing_input = Cursor::new(b"abcde".as_slice());
        let error = list(&mut listing_input, limits, false)
            .expect_err("container listing must refuse before parsing");
        let Some(CodecError::ResourceLimit(limit)) = error.downcast_ref::<CodecError>() else {
            panic!("input acquisition returns a typed resource refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::InputBytes);
        assert_eq!(limit.limit, 4);
        assert_eq!(limit.used, 4);
        assert_eq!(limit.additional, 1);
        assert_eq!(limit.operation, "complete input");
        assert_eq!(listing_input.position(), 5);

        let mut extraction_input = Cursor::new(b"abcde".as_slice());
        let error = extract(&mut extraction_input, limits, "missing")
            .expect_err("entry extraction must refuse before parsing");
        let Some(CodecError::ResourceLimit(limit)) = error.downcast_ref::<CodecError>() else {
            panic!("input acquisition returns a typed resource refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::InputBytes);
        assert_eq!(limit.limit, 4);
        assert_eq!(limit.used, 4);
        assert_eq!(limit.additional, 1);
        assert_eq!(limit.operation, "complete input");
        assert_eq!(extraction_input.position(), 5);
    }

    #[test]
    fn container_session_surfaces_fused_refusal_over_parser_error() {
        let mut input = Cursor::new(b"root".as_slice());
        let error = with_root(
            &mut input,
            ResourceLimits::desktop(),
            |ctx: &DecodeContext<'_>, _| -> anyhow::Result<()> {
                assert!(matches!(
                    ctx.charge_work(u64::MAX, "synthetic ignored parser refusal"),
                    Err(CodecError::ResourceLimit(_))
                ));
                Err(anyhow!("parser error after an ignored refusal"))
            },
        )
        .expect_err("session finalization restores the original refusal");
        let Some(CodecError::ResourceLimit(limit)) = error.downcast_ref::<CodecError>() else {
            panic!("fused refusal remains typed after parser failure");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "synthetic ignored parser refusal");
    }
}
