# cadmpeg-codec-f3d

`cadmpeg-codec-f3d` decodes Autodesk Fusion `.f3d` archives into `CadIr` and
encodes supported `CadIr` documents back to `.f3d`. The codec covers ZIP
container metadata, ASM B-rep topology, analytic and cached NURBS geometry,
body transforms, design and sketch records, construction history, and
appearances.

Support level: [L4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder) on the cadmpeg support ladder.

## Install

```sh
cargo add cadmpeg-codec-f3d cadmpeg-ir
```

## Decode

```rust,no_run
use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.f3d")?;
    let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;

    for loss in &result.report.losses {
        eprintln!("{:?}: {}", loss.severity, loss.message);
    }
    println!("{} bodies", result.ir.model.bodies.len());
    Ok(())
}
```

The result contains the decoded `CadIr` and a `DecodeReport`. Check
`report.losses` before using geometry from files that may contain unsupported
record forms. Set `DecodeOptions::container_only` when you need archive
metadata without B-rep decoding. `F3dCodec::inspect` returns the classified ZIP
entries and B-rep header facts.

## Encode

```rust,no_run
use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::{Codec, DecodeOptions, Encoder};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.f3d")?;
    let mut result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;

    // Edit supported fields in result.ir.

    let mut output = File::create("part-edited.f3d")?;
    F3dCodec.encode(&result.ir, &mut output)?;
    Ok(())
}
```

Decode retains the source archive and records a semantic baseline. Encoding an
unchanged result replays the original bytes. After supported edits, encoding
patches the retained archive and preserves unmodified entries and records.
Encoding a `CadIr` without retained F3D source data creates a canonical archive
for the supported source-less profile.

## Data model and support

The decoder selects the `.smbh` history stream, or the first `.smb` when no
`.smbh` exists. The Design body map selects every B-rep blob contributing
bodies to the document model. The decoder frames their SAB record slices and
builds each topology chain from bodies through vertices and points.
ASM model-space lengths become millimetres in `CadIr`; directions, ratios,
angles, knots, weights, and UV parameters retain their native scale.

Analytic carriers include planes, cylinders, cones, spheres, tori, lines,
circles, and ellipses. The codec also reads cached NURBS surfaces, 3D curves,
and pcurves, selected procedural definitions, source attributes, design joins,
sketch entities, typed ASM history, and Protein appearances.

Records that prevent faithful transfer appear in `DecodeReport::losses`.
Referenced carrier bytes that remain useful for passthrough are stored as
`UnknownRecord` values. If SAB framing or geometry decoding fails, the result
contains container metadata and retained source data with blocking geometry
and topology losses.

Display meshes, complete component structure, assembly constraints, and
replayable feature history are outside current support. The
[format-support matrix][support] lists decode and encode coverage.

## Documentation

- [API documentation][docs]
- [Format support][support]
- [Format notes][spec]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

Autodesk and Autodesk Fusion and other product names are trademarks of their
respective owners. cadmpeg uses them only to identify the file formats this
codec targets and is not affiliated with, endorsed by, or sponsored by any CAD
vendor. See the [clean-room and legal policy][legal].

[docs]: https://docs.rs/cadmpeg-codec-f3d
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#fusion-360-f3d
