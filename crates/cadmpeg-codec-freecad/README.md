# cadmpeg-codec-freecad

`cadmpeg-codec-freecad` decodes FreeCAD `.FCStd` archives into `CadIr` and
encodes supported `CadIr` documents back to `.FCStd`.

<!-- generated: capability fcstd -->

Support: L5 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#freecad-fcstd)).

<!-- /generated: capability fcstd -->

Neutral-first synthesis and guarded retained writes support document schemas
2, 3, and 4; schema 4 is the default for authored and foreign documents.

## Install

```sh
cargo add cadmpeg-codec-freecad cadmpeg-ir
```

## Decode

```rust,no_run
use cadmpeg_codec_freecad::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.FCStd")?;
    let result = FcstdCodec.decode(&mut input, &DecodeOptions::default())?;

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
without shape decoding.
`FcstdCodec::inspect` returns ZIP entry structure and document version facts.
Semantic decode accepts `SchemaVersion=2`, `3`, and `4`. Schema 2 uses the
`Features`/`FeatureData` object envelope. Schemas 3 and 4 use the
`Objects`/`ObjectData` envelope. An absent `FileVersion` has value zero.

## Encode

```rust,no_run
use cadmpeg_codec_freecad::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.FCStd")?;
    let result = FcstdCodec.decode(&mut input, &DecodeOptions::default())?;

    // Edit neutral CADIR fields on a cloned document before planning an export.

    let mut output = File::create("part-edited.FCStd")?;
    FcstdCodec
        .plan(EncodeInput::new(result.ir(), None), TargetRequest::Inherit)?
        .write_to(&mut output)?;
    Ok(())
}
```

The writer synthesizes current neutral geometry, feature history, sketches,
constraints, parameters, spreadsheets, products, joints, and presentation.
Its typed `fcstd` native namespace supplies values with no neutral form.
Synthesis consumes no retained archive bytes. Unsupported content produces
export losses. Feature edits retain the current evaluated BRep because the writer
does not run a modeling kernel; `needs_recompute` writes the native object
`Touched` declaration attribute.

An unchanged, fully represented source can use the retained `Patched` path when
its source fidelity is supplied. `SourceFidelity::unrepresented()` derives the union of source-only classes from
retained source records and prevents patching opaque content. Edited documents, missing layout, or a declined
patch use synthesis. `FcstdDocumentBuilder` also constructs source-less typed
application graphs. See the format notes for arena dispositions and mappings.

## Data model

An `.FCStd` archive is a ZIP package whose `Document.xml` carries an
application object and property graph. Exact-shape payloads travel as text or
binary side entries. The decoder frames those entries and builds topology from
bodies through vertices. Coverage for the primary envelope lives in the
[format-support profile][support].

## Documentation

- [API documentation][docs]
- [Format support][support]
- [Format notes][spec]
- [Clean-room and legal policy][legal]
- [Repository][repo]
- FreeCAD interop check: `tools/validate_fcstd_interop.py`

Requires Rust 1.88 or later. Licensed under Apache-2.0.

FreeCAD and other product names are trademarks of their respective owners.
cadmpeg uses them only to identify the file formats this codec targets and is
not affiliated with, endorsed by, or sponsored by any CAD vendor. See the
[clean-room and legal policy][legal].

[docs]: https://docs.rs/cadmpeg-codec-freecad
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/freecad_fcstd.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#freecad-fcstd
