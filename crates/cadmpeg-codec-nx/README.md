# cadmpeg-codec-nx

**Siemens NX `.prt` decoding for the cadmpeg CAD pipeline.**

This crate inspects SPLMSSTR containers, extracts embedded Parasolid
neutral-binary streams, and decodes supported B-rep topology and geometry into
`cadmpeg-ir`.

> Support is partial. Unsupported source records and semantic domains are
> preserved or reported as explicit loss. This version does not write native NX
> `.prt` files.

## Install

```sh
cargo add cadmpeg-codec-nx cadmpeg-ir
```

cadmpeg requires Rust 1.88 or later.

## Use

```rust,no_run
use cadmpeg_codec_nx::NxCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.prt")?;
    let result = NxCodec.decode(&mut input, &DecodeOptions::default())?;

    println!("geometry transferred: {}", result.report.geometry_transferred);
    println!("loss notes: {}", result.report.losses.len());
    Ok(())
}
```

`NxCodec::inspect` enumerates named streams and classifies embedded Parasolid
partition, deltas, and cached-body streams without decoding model geometry.
`NxCodec::decode` transfers supported analytic and NURBS carriers, trimmed
curves, and topology when stream framing and references resolve.

## Current boundaries

- SPLMSSTR container and embedded-stream extraction are partial.
- Analytic geometry, NURBS, trimmed curves, and topology are partial.
- Active-face selection remains unavailable for layouts whose
  partition-to-deltas tombstones do not resolve.
- Tessellation, design intent, assembly placements, presentation, metadata, and
  native writing are not implemented.

See the [format support profile][support] for the current domain-by-domain
status.

## Project links

- [API documentation][docs]
- [Format support][support]
- [Siemens NX byte-format specification][spec]
- [Repository][repo]
- [Clean-room and legal policy][legal]

Code is licensed under the Apache License 2.0. NX and Parasolid are trademarks
of their respective owners; cadmpeg is independent of and is not endorsed by
Siemens.

[docs]: https://docs.rs/cadmpeg-codec-nx
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/siemens_nx.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#siemens-nx-prt
