# cadmpeg-asm

`cadmpeg-asm` parses Autodesk Shape Manager and admitted Spatial ACIS binary
model streams. It reads ASM `BinaryFile4`/`BinaryFile8` and ACIS 217/218
headers, frames SAB token streams, locates the boundary between solved records
and construction history, and reports byte locations for payload fields.

## Install

```sh
cargo add cadmpeg-asm
```

## Parse kernel headers

`asm_header::has_asm_magic` recognizes the 15-byte `ASM BinaryFile4` and
`ASM BinaryFile8` prefixes. `asm_header::parse` returns a [`KernelHeader`][header]
with `width`, `save_format_version`, `record_count`, `entity_count`, `flags`,
product strings, and tolerance fields that the input provides. A missing magic
returns `None`; a recognized but incomplete header returns a `KernelHeader` with
unavailable fields set to `None`.

`acis_header::has_acis_magic` recognizes `ACIS BinaryFile`.
`acis_header::parse` admits its 32-bit header and returns the same
`KernelHeader` metadata. Every ACIS save format uses this parser. Majors 217
and 218 are the bands the record decoders are verified against, which `dialect`
states: a stream outside them is framed and decoded the same way, and the host
labels the result `Admission::AdmittedUnverified` and charges its
`source.dialect-unverified` loss.

`BinaryFile4` stores the save-format version, record count, entity count, and
flags as little-endian `u32` words at offsets 15, 19, 23, and 27. Its string
region begins at byte 31. `BinaryFile8` stores the save-format version at byte
15, the entity count as a little-endian `u64` at byte 31, and flags as a
little-endian `u64` at byte 39. Its string region begins at byte 47. Both
layouts carry three `0x07`-tagged strings followed by three `0x06`-tagged
little-endian `f64` values. The values populate `product_family`,
`product_version`, `save_date`, `scale`, `linear` (`resabs`), and `angular`
(`resnor`).

`KernelHeader::save_format_major` and `save_format_minor` split the encoded
save-format version, where `100 * major + minor` is the stored value.
`KernelHeader::has_history_partition` reads [`HISTORY_PARTITION_FLAG`][history]
from `flags`. [`FORMAT_REVISION_FLAGS`][revision] identifies bits 1 through 7;
`KernelHeader::format_revision` reads them. `KernelHeader::unassigned_flags` preserves
the remaining flag bits.

`asm_header::record_stream_start` returns the byte after the fixed words, the
three strings, and the three tolerance values. `asm_header::solved_record_limit`
attempts to locate the history partition's first record when the header
declares `HISTORY_PARTITION_FLAG`. It recognizes the
`Begin-of-ASM-History-Data` preamble and the earlier `delta_state` boundary.
`asm_header::stream_ref_width` returns the declared integer/reference width in
bytes and uses `8` when the header is unreadable.

## Frame SAB records

`sab::frame(bytes, start, limit, ref_width)` returns a `Vec<Record>` for the
requested byte range. `ref_width` is the stream's integer and reference width,
usually `4` or `8`. A `0x11` tag terminates a record at subtype depth zero.
`frame` stops at the supplied limit, at the end of `bytes`, or at the
`delta_state` history boundary. `sab::frame_history` accepts a final history
record that ends at the supplied limit without a `0x11` terminator.

An unknown tag or truncated payload returns [`FrameError`][frame-error], which
stores the byte offset and a reason string. Framing preserves recognized
payload tokens, record names, and record extents. A [`Record`][record] contains
the zero-based `index`, the hyphen-joined `name`, its leading `head`, the
retained `tokens` in an `Arc<[Token]>`, the starting `offset`, and the byte
`len` including the record terminator when present.

Record names use `0x0e` sub-identifiers followed by a `0x0d` identifier. Those
tags become `Token::SubIdent` and `Token::Ident` when they occur in a payload.
`Token::is_payload_ident` identifies those payload identifiers.
`Record::chunks`, `Record::chunk`, and `Record::chunk_len` index value tokens
and skip them. `Record::ref_at` returns a non-null `Token::Ref` value at a
chunk index; the `-1` null reference produces `None`.

The framer maps SAB tags to [`Token`][token] variants as follows:

| Tags | Variants |
| --- | --- |
| `0x02`, `0x03`, `0x04`, `0x05`, `0x06` | `Char(u8)`, `Short(i16)`, `Long(i64)`, `Float(f32)`, `Double(f64)` |
| `0x07`, `0x08`, `0x09`, `0x12` | `Str(String)` with one-byte, two-byte, or `ref_width` length prefixes |
| `0x0a`, `0x0b`, `0x0c` | `True`, `False`, `Ref(i64)` |
| `0x0f`, `0x10` | `SubtypeOpen`, `SubtypeClose` |
| `0x13`, `0x14`, `0x16` | `Position([f64; 3])`, `Vector3([f64; 3])`, `Vector2([f64; 2])` |
| `0x15`, `0x17` | `Enum(i64)`, `Int64(i64)` |
| payload `0x0d`, `0x0e` | `Ident(String)`, `SubIdent(String)` |

`Long`, `Ref`, and `Enum` use `ref_width`. `Int64` always uses eight bytes.
The `0x11` record terminator is a framing control tag and has no `Token`
variant.

## Locate payload bytes

`sab::payload_token_offset` returns the absolute byte offset for a value token
at a record's chunk index. `sab::payload_token_offsets` returns all absolute
offsets for a selected payload tag and reports a [`FrameError`][frame-error]
when the record cannot be lexed. Both helpers use the same value-token indexing
as `Record::chunk`.

`sab::payload_subtype_range` returns the absolute byte range inside the subtype
at a chunk index when its following identifier matches the requested name. The
range starts after that identifier and ends before the matching
`Token::SubtypeClose`; nested subtype scopes remain in the range.

Frame a synthetic SAB record:

```rust,no_run
use cadmpeg_asm::sab::{frame, Token};

fn main() {
    let mut bytes = vec![0x0d, 4];
    bytes.extend_from_slice(b"body");
    bytes.push(0x0c);
    bytes.extend_from_slice(&1_i64.to_le_bytes());
    bytes.push(0x11);

    let records = frame(&bytes, 0, bytes.len(), 8).expect("valid SAB record");
    let record = &records[0];

    assert_eq!(record.name, "body");
    assert!(matches!(record.chunk(0), Some(Token::Ref(1))));
    assert_eq!(record.ref_at(0), Some(1));
}
```

## Documentation

- [API documentation][docs]
- [Architecture and crate map][architecture]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[docs]: https://docs.rs/cadmpeg-asm
[frame-error]: https://docs.rs/cadmpeg-asm/latest/cadmpeg_asm/sab/struct.FrameError.html
[header]: https://docs.rs/cadmpeg-asm/latest/cadmpeg_asm/kernel_header/struct.KernelHeader.html
[history]: https://docs.rs/cadmpeg-asm/latest/cadmpeg_asm/kernel_header/constant.HISTORY_PARTITION_FLAG.html
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[record]: https://docs.rs/cadmpeg-asm/latest/cadmpeg_asm/sab/struct.Record.html
[revision]: https://docs.rs/cadmpeg-asm/latest/cadmpeg_asm/kernel_header/constant.FORMAT_REVISION_FLAGS.html
[repo]: https://github.com/cadmpeg/cadmpeg
[token]: https://docs.rs/cadmpeg-asm/latest/cadmpeg_asm/sab/enum.Token.html
