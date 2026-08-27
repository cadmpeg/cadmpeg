# cadmpeg-codec-freecad

`cadmpeg-codec-freecad` decodes FreeCAD `.FCStd` archives into `CadIr` and
encodes supported `CadIr` documents back to `.FCStd`.

<!-- generated: capability -->

Support: L5 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#freecad-fcstd)).

<!-- /generated: capability -->

Deterministic retained writes, checked edits, and source-less typed application
graphs are extras above the schema-4/file-1 envelope.

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
`DecodeOptions::container_only` for archive metadata without shape decoding.
`FcstdCodec::inspect` returns ZIP entry structure and document version facts.
Semantic decode accepts `SchemaVersion=2`, `3`, and `4`. Schema 2 uses the
`Features`/`FeatureData` object envelope. Schemas 3 and 4 use the
`Objects`/`ObjectData` envelope. An absent `FileVersion` has value zero.

## Encode

```rust,no_run
use cadmpeg_codec_freecad::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions, Encoder};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.FCStd")?;
    let result = FcstdCodec.decode(&mut input, &DecodeOptions::default())?;

    // Edit supported fields on the FCStd native document graph in result.ir().

    let mut output = File::create("part-edited.FCStd")?;
    FcstdCodec
        .plan(EncodeInput::new(&result.ir(), None))?
        .write_to(&mut output)?;
    Ok(())
}
```

Writing uses the FCStd native document graph on `CadIr`
(`ir.native["fcstd"]`), not `SourceFidelity`. `Encoder::plan` ignores
`fidelity` and returns `NotConsumed` when one is supplied. Retained
schema-4/file-1 documents regenerate while preserving unedited XML records and
named side entries. Checked leaf property edits and side-entry replacements
update the native graph in place. `FcstdDocumentBuilder` builds source-less
application graphs for the same write envelope. Unsupported schema or file
targets are refused.

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
