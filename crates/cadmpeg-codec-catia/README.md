# cadmpeg-codec-catia

`cadmpeg-codec-catia` opens `.CATPart` files and loads available model data
into `CadIr`. It recognizes the known `V5_CFV2` storage layouts and reads
analytic geometry, spline geometry, and topology where the file records provide
the needed links.

## Install

```sh
cargo add cadmpeg-codec-catia cadmpeg-ir
```

## Use

```rust,no_run
use cadmpeg_codec_catia::CatiaCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.CATPart")?;
    let result = CatiaCodec.decode(&mut input, &DecodeOptions::default())?;

    println!(
        "{} bodies, {} surfaces",
        result.ir.model.bodies.len(),
        result.ir.model.surfaces.len()
    );
    Ok(())
}
```

`CatiaCodec::inspect` identifies the storage layout and lists its logical
streams without decoding geometry.

## Coverage

Standard nested files have the broadest coverage, including connected B-rep
topology when trim and endpoint records resolve. Other layouts currently yield
smaller sets of analytic or spline geometry.

The crate does not yet read tessellation, design history, assemblies,
appearances, or persistent attributes, and it does not write `.CATPart` files.
See [format support][support] for the detailed matrix.

## Documentation

- [API documentation][docs]
- [Format support][support]
- [Format notes][spec]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

[docs]: https://docs.rs/cadmpeg-codec-catia
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#catia-v5-catpart
