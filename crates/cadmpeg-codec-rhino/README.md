# cadmpeg-codec-rhino

`cadmpeg-codec-rhino` decodes Rhino `.3dm` archives into `CadIr` and encodes
supported `CadIr` documents back to `.3dm`.

Support level: [L0](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder).
Archive 2/3/4/50/60/70/80/90 and V2–V4 open at L1 and show as extras. V1
and archive version 5 remain L0. Bounded source-less native writing is an
extra above that L1 subset.

## Install

```sh
cargo add cadmpeg-codec-rhino cadmpeg-ir
```

## Decode

```rust,no_run
use cadmpeg_codec_rhino::RhinoCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("model.3dm")?;
    let result = RhinoCodec.decode(&mut input, &DecodeOptions::default())?;

    for loss in &result.report().losses {
        eprintln!("{:?}: {}", loss.severity, loss.message);
    }
    println!("{} bodies", result.ir().model.bodies.len());
    Ok(())
}
```

The result holds the decoded `CadIr` and a `DecodeReport`. Read
`report.losses` before trusting geometry. Set
`DecodeOptions::container_only` for archive metadata without object decoding.
`RhinoCodec::inspect` returns chunk and table structure.

## Encode

```rust,no_run
use cadmpeg_codec_rhino::{RhinoArchiveVersion, RhinoEncoder};
use cadmpeg_ir::codec::{EncodeInput, Encoder};
use cadmpeg_ir::CadIr;
use std::fs::File;

fn write_3dm(ir: &CadIr, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut output = File::create(path)?;
    RhinoEncoder::new(RhinoArchiveVersion::V7)
        .plan(EncodeInput {
            ir,
            fidelity: None,
        })?
        .write_to(&mut output)?;
    Ok(())
}
```

`RhinoEncoder` selects the target archive version explicitly. Writing is
source-less semantic regeneration from a narrowly writable IR. `fidelity` is
ignored and resolves to `NotConsumed`. Writable families are points and point
clouds, circles, canonical NURBS curves and surfaces, planes, restricted
planar and NURBS sheet and solid Breps, and triangle meshes. Unsupported
arenas and retained non-writer namespaces are refused. Generated archives
declare millimetre units. The CLI accepts the same archive-version choice:

```sh
cadmpeg inspect model.3dm
cadmpeg dump model.3dm -o model.cadir.json
cadmpeg convert model.cadir.json -o model.3dm
cadmpeg convert model.cadir.json -o model-v6.3dm --rhino-target 60
```

## Data model

A `.3dm` archive is a versioned chunk and table stream. The decoder frames
those tables and transfers built-in object, presentation, product, and history
records into `CadIr`. Lengths and length-valued tolerances become millimetres.
Angles, unit vectors, knot values, UV values, relative tolerances, and hatch
pattern scale keep their native scale.

Records that block faithful transfer land in `DecodeReport::losses`. Named
opaque records retain identity and, within bounds, complete bytes or length
plus SHA-256. Coverage for each archive band lives in the
[format-support profile][support].

## Documentation

- [API documentation][docs]
- [Format support][support]
- [Format notes][spec]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

Rhino and other product names are trademarks of their respective owners.
cadmpeg uses them only to identify the file formats this codec targets and is
not affiliated with, endorsed by, or sponsored by any CAD vendor. See the
[clean-room and legal policy][legal].

[docs]: https://docs.rs/cadmpeg-codec-rhino
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/rhino_3dm.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#rhino-3dm
