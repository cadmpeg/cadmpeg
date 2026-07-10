# cadmpeg-codec-f3d

`cadmpeg-codec-f3d` opens `.f3d` archives and loads their model data into
`CadIr`. It reads the archive structure, B-rep topology, analytic and spline
geometry, design records, transforms, and appearances.

## Install

```sh
cargo add cadmpeg-codec-f3d cadmpeg-ir
```

## Use

```rust,no_run
use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.f3d")?;
    let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;

    println!(
        "{} bodies, {} surfaces",
        result.ir.model.bodies.len(),
        result.ir.model.surfaces.len()
    );
    Ok(())
}
```

`F3dCodec::inspect` returns the archive entries and B-rep headers without
decoding the model.

## Coverage

The decoder handles common analytic and NURBS geometry, connected B-rep
topology, body transforms, material data, and a growing set of design and
sketch records.

The writer can replay an unchanged archive byte for byte. It can also update
the B-rep points, curves, surfaces, sketch geometry, and sketch constraints it
understands while retaining the rest of the archive.

Display meshes, full component structure, assembly constraints, and replayable
feature history remain outside the current coverage. See
[format support][support] for the detailed matrix.

## Documentation

- [API documentation][docs]
- [Format support][support]
- [Format notes][spec]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

[docs]: https://docs.rs/cadmpeg-codec-f3d
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#fusion-360-f3d
