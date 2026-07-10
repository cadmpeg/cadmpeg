# cadmpeg-codec-creo

**Creo Parametric `.prt` inspection and partial decoding for cadmpeg.**

This crate recognizes the `#UGC:2` Pro/E Session Binary container, enumerates
its sections, decodes supported PSB primitives and namespace structure, and
transfers supported datum-plane carriers into `cadmpeg-ir`.

> This is the lowest-fidelity cadmpeg native-format codec. Reliable container
> inspection is the primary capability. It does not currently transfer a placed
> model B-rep or write native `.prt` files.

## Install

```sh
cargo add cadmpeg-codec-creo cadmpeg-ir
```

cadmpeg requires Rust 1.88 or later.

## Use

```rust,no_run
use cadmpeg_codec_creo::CreoCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.prt")?;
    let result = CreoCodec.decode(&mut input, &DecodeOptions::default())?;

    println!("geometry transferred: {}", result.report.geometry_transferred);
    println!("loss notes: {}", result.report.losses.len());
    Ok(())
}
```

`CreoCodec::inspect` parses the PSB header and table of contents, identifies the
layout family, enumerates sections, and reports supported namespace counts.
`CreoCodec::decode` preserves geometry sections as opaque records, transfers
supported datum planes, and reports each blocked semantic layer.

## Current boundaries

- Container and PSB primitive decoding are the strongest capabilities.
- Surface and curve prototype namespaces do not represent placed model
  geometry and are not presented as if they do.
- Model B-rep, tessellation, design intent, product structure, presentation,
  metadata, and native writing are not implemented.

See the [format support profile][support] for the current domain-by-domain
status.

## Project links

- [API documentation][docs]
- [Format support][support]
- [Creo PRT byte-format specification][spec]
- [Repository][repo]
- [Clean-room and legal policy][legal]

Code is licensed under the Apache License 2.0. Creo and Pro/ENGINEER are
trademarks of their respective owners; cadmpeg is independent of and is not
endorsed by PTC.

[docs]: https://docs.rs/cadmpeg-codec-creo
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#creo-parametric-prt
