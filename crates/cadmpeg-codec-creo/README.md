# cadmpeg-codec-creo

`cadmpeg-codec-creo` reads PTC Creo Parametric and Pro/ENGINEER `.prt` files
with the `#UGC:2` PSB container signature. It identifies the container layout,
lists named sections, reports geometry namespace counts and JPEG preview
presence, and decodes placed geometry, topology, sketches, and design
records into [`CadIr`].

The `.prt` extension is also used by Siemens NX. Format detection uses the
`#UGC:2` signature.

Support level: [L1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder).

## Install

```sh
cargo add cadmpeg-codec-creo cadmpeg-ir
```

## Inspect

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

Call `CreoCodec.decode` for a `CadIr` document and a structured loss report.
Decode keeps recognized PSB geometry sections as unknown records and transfers
carriers and design records that are complete in the source bytes.

## Data model

PSB files use an ASCII header and table of contents followed by named binary
sections. The crate recognizes the ND and DEPDB layout families and reads
surface and curve namespace rows, prototype parameters, native half-edge links,
active units, feature identifiers, and datum outlines.

Surface prototype parameters are family templates. Exact plane components and
selected cylinders transfer with connected topology. Complete named
triangle-strip position arrays transfer as display tessellation. Losses land in
[`DecodeReport`].

## References

- [API reference][docs]
- [Format support][support]
- [Format specification][spec]
- [Coverage contract][coverage]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

PTC Creo Parametric and Pro/ENGINEER and other product names are trademarks of
their respective owners. cadmpeg uses them only to identify the file formats
this codec targets and is not affiliated with, endorsed by, or sponsored by any
CAD vendor. See the [clean-room and legal policy][legal].

[docs]: https://docs.rs/cadmpeg-codec-creo
[DecodeReport]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/report/struct.DecodeReport.html
[CadIr]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/document/struct.CadIr.html
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md
[coverage]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt-coverage.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#creo-parametric-prt
