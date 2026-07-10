# cadmpeg-codec-sldprt

`cadmpeg-codec-sldprt` opens `.sldprt` files and loads their model data into
`CadIr`. It reads B-rep topology, analytic and spline geometry, display meshes,
appearances, document metadata, and parts of the feature history.

## Install

```sh
cargo add cadmpeg-codec-sldprt cadmpeg-ir
```

## Use

```rust,no_run
use cadmpeg_codec_sldprt::SldprtCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.sldprt")?;
    let result = SldprtCodec.decode(&mut input, &DecodeOptions::default())?;

    println!(
        "{} bodies, {} faces",
        result.ir.model.bodies.len(),
        result.ir.model.faces.len()
    );
    Ok(())
}
```

`SldprtCodec::inspect` returns the container blocks, section directory, cache
cells, and embedded streams without decoding the model.

## Coverage

The writer can replay an unchanged file byte for byte. It can also rebuild the
B-rep geometry it understands and retain selected appearances, metadata,
display data, and feature records.

Assembly documents and constraints are outside this crate. Older layouts and
some geometry families still need broader coverage. See
[format support][support] for the detailed matrix.

## Documentation

- [API documentation][docs]
- [Format support][support]
- [Format notes][spec]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

[docs]: https://docs.rs/cadmpeg-codec-sldprt
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#solidworks-sldprt
