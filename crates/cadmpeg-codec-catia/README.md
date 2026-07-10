# cadmpeg-codec-catia

**CATIA V5 `.CATPart` decoding for the cadmpeg CAD pipeline.**

This crate inspects `V5_CFV2` containers, identifies the native geometry storage
family, and decodes supported analytic, freeform, and topology content into
`cadmpeg-ir`.

> Support is partial. Unsupported source records and unresolved
> carrier-to-topology bindings are preserved or reported as explicit loss. This
> version does not write native `.CATPart` files.

## Install

```sh
cargo add cadmpeg-codec-catia cadmpeg-ir
```

cadmpeg requires Rust 1.88 or later.

## Use

```rust,no_run
use cadmpeg_codec_catia::CatiaCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.CATPart")?;
    let result = CatiaCodec.decode(&mut input, &DecodeOptions::default())?;

    println!("geometry transferred: {}", result.report.geometry_transferred);
    println!("loss notes: {}", result.report.losses.len());
    Ok(())
}
```

`CatiaCodec::inspect` reconstructs named logical streams and identifies the
storage family without decoding model geometry. `CatiaCodec::decode` handles
supported standard-nested, zero-entity, E5, and freeform carrier families and
attaches topology where source references resolve.

## Current boundaries

- Container identification works across the known storage families.
- Analytic and freeform geometry support is partial.
- Standard-nested topology is conditional on available source senses and
  bindings.
- Tessellation, design intent, product structure, presentation, metadata, and
  native writing are not implemented.

See the [format support profile][support] for the current domain-by-domain
status.

## Project links

- [API documentation][docs]
- [Format support][support]
- [CATIA V5 byte-format specification][spec]
- [Repository][repo]
- [Clean-room and legal policy][legal]

Code is licensed under the Apache License 2.0. CATIA is a trademark of its
respective owner; cadmpeg is independent of and is not endorsed by Dassault
Systèmes.

[docs]: https://docs.rs/cadmpeg-codec-catia
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#catia-v5-catpart
