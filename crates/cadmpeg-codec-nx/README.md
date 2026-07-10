# cadmpeg-codec-nx

`cadmpeg-codec-nx` opens `.prt` files stored as SPLMSSTR containers and loads
their model data into `CadIr`. It extracts embedded Parasolid streams and reads
analytic geometry, spline geometry, trimmed curves, and connected topology.

## Install

```sh
cargo add cadmpeg-codec-nx cadmpeg-ir
```

## Use

```rust,no_run
use cadmpeg_codec_nx::NxCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.prt")?;
    let result = NxCodec.decode(&mut input, &DecodeOptions::default())?;

    println!(
        "{} bodies, {} surfaces",
        result.ir.model.bodies.len(),
        result.ir.model.surfaces.len()
    );
    Ok(())
}
```

`NxCodec::inspect` lists named streams and classifies embedded model data
without decoding geometry.

## Coverage

The decoder handles points, common analytic surfaces and curves, B-splines,
selected trimmed curves, and topology where record references resolve.

Some files still lack enough decoded tombstone data to choose the active face
set. Tessellation, design history, assembly placement, appearances, and native
writing are not implemented. See [format support][support] for the detailed
matrix.

## Documentation

- [API documentation][docs]
- [Format support][support]
- [Format notes][spec]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

[docs]: https://docs.rs/cadmpeg-codec-nx
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/siemens_nx.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#siemens-nx-prt
