# cadmpeg-codec-sldprt

**SolidWorks `.sldprt` reading and writing for the cadmpeg CAD pipeline.**

This crate inspects SolidWorks part containers, decodes supported embedded
Parasolid B-rep topology and geometry into `cadmpeg-ir`, preserves source
content, and writes unchanged or supported modified documents.

> Support is partial. Unsupported source records and semantic domains are
> preserved or reported as explicit loss. Generated `.sldprt` output is limited
> to the documented semantic-write subset.

## Install

```sh
cargo add cadmpeg-codec-sldprt cadmpeg-ir
```

cadmpeg requires Rust 1.88 or later.

## Use

```rust,no_run
use cadmpeg_codec_sldprt::SldprtCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.sldprt")?;
    let result = SldprtCodec.decode(&mut input, &DecodeOptions::default())?;

    println!("geometry transferred: {}", result.report.geometry_transferred);
    println!("loss notes: {}", result.report.losses.len());
    Ok(())
}
```

`SldprtCodec::inspect` enumerates compressed blocks, the section directory,
cache cells, and embedded Parasolid streams without decoding model geometry.
`SldprtCodec::decode` builds supported B-rep topology, analytic and NURBS
geometry, appearance, feature lanes, history, and tessellation.

## Current boundaries

- Container, geometry, topology, tessellation, design intent, presentation, and
  metadata support are partial.
- Product structure and assembly constraints are not implemented.
- Unchanged retained source files can round-trip byte for byte.
- Semantic writing supports a bounded subset and rejects unsupported inputs
  instead of fabricating output.

See the [format support profile][support] for the current domain-by-domain
status.

## Project links

- [API documentation][docs]
- [Format support][support]
- [SLDPRT byte-format specification][spec]
- [Repository][repo]
- [Clean-room and legal policy][legal]

Code is licensed under the Apache License 2.0. SolidWorks is a trademark of its
respective owner; cadmpeg is independent of and is not endorsed by Dassault
Systèmes.

[docs]: https://docs.rs/cadmpeg-codec-sldprt
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#solidworks-sldprt
