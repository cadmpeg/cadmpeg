# cadmpeg-codec-iges

`cadmpeg-codec-iges` inspects, decodes, and writes IGES 5.1, 5.2, and 5.3
Fixed ASCII files through `CadIr`.

Support level: [L8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#iges)
for the Fixed ASCII mechanical/document envelope.

## Install

```sh
cargo add cadmpeg-codec-iges cadmpeg-ir
```

## Decode

```rust,no_run
use cadmpeg_codec_iges::IgesCodec;
use cadmpeg_ir::{CodecEntry, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.igs")?;
    let result = IgesCodec.decode(&mut input, &DecodeOptions::default())?;

    for loss in &result.report.losses {
        eprintln!("{:?}: {}", loss.severity, loss.message);
    }
    println!("{} bodies", result.ir.model.bodies.len());
    Ok(())
}
```

The result holds the decoded `CadIr` and a `DecodeReport`. Read
`report.losses` before trusting geometry. Set
`DecodeOptions::container_only` for card and Global metadata without entity
decode. `IgesCodec::inspect` returns section structure, Directory census, and
reference findings. Compressed ASCII and Binary representations are detected
and inspected by name and refused for semantic decode.

## Write

`IgesCodec` replays an unchanged decoded source image byte for byte when its
retained source record and document baseline are intact. `IgesEncoder` accepts
an explicit target version. The semantic writer supports standalone points,
finite lines, analytic conic arcs, NURBS curves, planar and NURBS support
surfaces, one-face trimmed sheet bodies with NURBS parameter curves, and
bounded Type 186/502/504/508/510/514 manifold B-rep solids and multi-face
sheet bodies. Exact solved carriers for procedural surfaces and curves are
regenerated as neutral geometry and reported as `procedural_reduced`. It
validates topology ownership, edge spans, radial incidence, and supported
surface and pcurve geometry before output. Unsupported neutral or native
content is refused. Edited source documents may report
`passthrough_record_omitted` losses for native direction, display, or
occurrence-expansion records that are not regenerated.

## Data model

A Fixed ASCII IGES file is an 80-column card stream with Start, Global,
Directory Entry, Parameter Data, and Terminate sections. The decoder frames
those cards, resolves Directory and Parameter records, and transfers admitted
geometry, topology, product, presentation, annotation, drawing, associativity,
and property entities into `CadIr`. Lengths convert to millimetres from the
Global unit system. Directions, ratios, angles, knots, weights, and UV
parameters keep their native scale.

Records that block faithful transfer land in `DecodeReport::losses`.
`native.iges` retains physical cards and typed or generic entity data.
`SourceFidelity` retains the complete source image and its SHA-256 digest;
`transfer_ledger` is not populated. Coverage for each envelope lives in the
[format-support profile][support].

## Documentation

- [API documentation][docs]
- [Format support][support]
- [Format notes][spec]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

IGES and other product names are trademarks of their respective owners.
cadmpeg uses them only to identify the file formats this codec targets and is
not affiliated with, endorsed by, or sponsored by any CAD vendor. See the
[clean-room and legal policy][legal].

[docs]: https://docs.rs/cadmpeg-codec-iges
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/iges.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#iges
