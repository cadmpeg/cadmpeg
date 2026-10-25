# cadmpeg-codec-f3d

`cadmpeg-codec-f3d` decodes Autodesk Fusion `.f3d` archives into `CadIr` and
encodes supported `CadIr` documents back to `.f3d`. The codec covers ZIP
container metadata, ASM B-rep topology, analytic and cached NURBS geometry,
body transforms, design and sketch records, construction history, and
appearances. Multi-document `.f3z` archives decode into one merged document.

<!-- generated: capability f3d -->

Support: L5 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#fusion-360-f3d)).

<!-- /generated: capability f3d -->

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

    for loss in &result.report().losses {
        eprintln!("{:?}: {}", loss.severity, loss.message);
    }
    println!("{} bodies", result.ir().model.bodies.len());
    Ok(())
}
```

The result holds the decoded `CadIr` and a `DecodeReport`. Read
`report.losses` before trusting geometry. Set
`DecodeOptions::depth` to `DecodeDepth::Container` for archive metadata
without B-rep decoding, or to `DecodeDepth::Geometry` for geometry and
products without standalone design projection. Bodyless, image-bearing
and F3Z routes can also deliver design because the same phase supplies model
assets. The document declares its held classes in
`ir.scope`. A total design-intent failure has the same scope and document
content as a geometry-depth decode; the failure is recorded in the report.
`F3dCodec::inspect` returns classified ZIP entries and B-rep header facts.

## Encode

```rust,no_run
use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.f3d")?;
    let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;

    // Edit supported fields in result.ir().

    let mut output = File::create("part-edited.f3d")?;
    F3dCodec
        .plan(
            EncodeInput::new(result.ir(), Some(result.source_fidelity())),
            TargetRequest::Inherit,
        )?
        .write_to(&mut output)?;
    Ok(())
}
```

Encoding authors content from the neutral arenas and supported typed native
records. Neutral values take precedence over their native projections.
Retained source bytes select layout only: replay and patch require empty
`source_fidelity.unrepresented` and the same content that synthesis would
produce. An unavailable image or an unsupported patch selects synthesis;
`ExportReport` records `Synthesized` and the patch-decline cause.

Synthesis writes supported B-rep geometry, appearances, scalar document
parameters and configurations. Unsupported records are omitted locally and
reported as export losses. Procedural definitions use their solved carrier
when their native construction has no exact encoding. A document with no
exportable body writes an empty geometry carrier and reports the existing
no-exportable-solids loss. Read the export report before accepting a reduction.

## Data model

The Design body map selects every B-rep blob that contributes bodies.
Extension does not choose the model: `.smb` and `.smbh` are both ASM streams.
History streams are selected independently by the ASM header flag, and every
selected history graph is decoded. Without body-map bindings, a unique
history-bearing stream or a single BREP is the only fallback. The decoder
frames each selected SAB slice and builds each topology chain from bodies
through vertices and points. ASM model-space lengths become millimetres in
`CadIr`. Directions, ratios, angles, knots, weights, and UV parameters keep
their native scale.

Typed transfer covers analytic carriers, cached NURBS, selected procedural
definitions, design and sketch records, typed ASM history, source attributes,
and Protein appearances. Records that block faithful transfer land in
`DecodeReport::losses`. Opaque retained bytes stay outside the typed content model and do not
author export content. The sidecar declares their unrepresented arena classes.
Failed SAB framing or geometry decoding still returns
container metadata and retained source data, with blocking geometry and
topology losses.

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
