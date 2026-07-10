# cadmpeg-codec-creo

`cadmpeg-codec-creo` opens `.prt` files that use the `#UGC:2` container layout.
Use it to list sections, inspect layout and namespace data, find the preview,
and recover datum planes.

## Install

```sh
cargo add cadmpeg-codec-creo cadmpeg-ir
```

## Use

```rust,no_run
use cadmpeg_codec_creo::CreoCodec;
use cadmpeg_ir::Codec;
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.prt")?;
    let summary = CreoCodec.inspect(&mut input)?;

    println!("{} sections", summary.entries.len());
    Ok(())
}
```

## Coverage

The container reader handles the known ND and DEPDB layouts, PSB compact
numbers, section tables, and surface and curve namespace counts. The decoder
can add model-space datum planes to `CadIr`.

The geometry namespaces describe prototypes rather than placed model geometry,
so this crate does not build a body B-rep from them. It also does not write
`.prt` files. See [format support][support] for the detailed matrix.

## Documentation

- [API documentation][docs]
- [Format support][support]
- [Format notes][spec]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

[docs]: https://docs.rs/cadmpeg-codec-creo
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#creo-parametric-prt
