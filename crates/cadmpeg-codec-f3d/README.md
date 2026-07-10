# cadmpeg-codec-f3d

**Autodesk Fusion 360 `.f3d` decoding for the cadmpeg CAD pipeline.**

This crate inspects Fusion 360 ZIP containers and decodes supported ASM/SAB
B-rep topology, analytic geometry, cached NURBS, design records, transforms,
attributes, and appearance data into `cadmpeg-ir`.

> Support is partial. Unsupported source records and semantic domains are
> preserved or reported as explicit loss. This version does not write native
> `.f3d` files.

## Install

```sh
cargo add cadmpeg-codec-f3d cadmpeg-ir
```

cadmpeg requires Rust 1.88 or later.

## Use

```rust,no_run
use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.f3d")?;
    let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;

    println!("geometry transferred: {}", result.report.geometry_transferred);
    println!("loss notes: {}", result.report.losses.len());
    Ok(())
}
```

`F3dCodec::inspect` enumerates archive entries and reads B-rep stream headers
without decoding model geometry. `F3dCodec::decode` selects the active B-rep,
frames the SAB record stream, builds the supported topology and geometry graph,
and returns both `CadIr` and `DecodeReport`.

## Current boundaries

- B-rep topology, analytic carriers, cached spline carriers, selected
  procedural definitions, appearances, and design-side records are partial.
- Fusion display meshes are not transferred into the IR tessellation arena.
- Complete component structure, constraints, and replayable feature history are
  not implemented.
- Native write and round-trip support are not implemented.

See the [format support profile][support] for the current domain-by-domain
status.

## Project links

- [API documentation][docs]
- [Format support][support]
- [F3D byte-format specification][spec]
- [Repository][repo]
- [Clean-room and legal policy][legal]

Code is licensed under the Apache License 2.0. Autodesk and Fusion 360 are
trademarks of their respective owners; cadmpeg is independent of and is not
endorsed by Autodesk.

[docs]: https://docs.rs/cadmpeg-codec-f3d
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#fusion-360-f3d
