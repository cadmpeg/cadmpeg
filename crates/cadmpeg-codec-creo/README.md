# Creo `.prt` inspection and structural decode

`cadmpeg-codec-creo` reads PTC Creo Parametric and Pro/ENGINEER `.prt` files
with the `#UGC:2` PSB container signature. It identifies the container layout,
lists named sections, reports geometry namespace counts and JPEG preview
presence, and decodes standard datum planes into [`CadIr`].

The `.prt` extension is also used by Siemens NX. Format detection uses the
`#UGC:2` signature, not the extension.

## Installation

```sh
cargo add cadmpeg-codec-creo cadmpeg-ir
```

## Inspect a file

```rust,no_run
use cadmpeg_codec_creo::CreoCodec;
use cadmpeg_ir::Codec;
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.prt")?;
    let summary = CreoCodec.inspect(&mut input)?;

    println!("{} sections", summary.entries.len());
    for note in summary.notes {
        println!("{note}");
    }
    Ok(())
}
```

Call `CreoCodec.decode` when you need a `CadIr` document and a structured loss
report. Decode preserves recognized PSB geometry sections as unknown records
and transfers model-space standard datum planes as derived, unbounded plane
surfaces.

## Data model and limits

PSB files use an ASCII header and table of contents followed by named binary
sections. The crate recognizes the ND and DEPDB layout families and reads
surface and curve namespace rows, prototype parameters, native half-edge
links, active units, feature identifiers, and datum outlines.

Surface prototype parameters are family templates, not placed model
geometry. Per-instance coordinates, curve geometry, face bindings, and
feature evaluation remain incomplete, so decode does not emit a body B-rep.
The [`DecodeReport`] records these losses. Writing `.prt` files is not
supported.

## References

- [API reference][docs]
- [Format support][support]
- [Format specification][spec]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

[docs]: https://docs.rs/cadmpeg-codec-creo
[DecodeReport]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/report/struct.DecodeReport.html
[CadIr]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/document/struct.CadIr.html
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#creo-parametric-prt
