# cadmpeg-codec-pvz

`cadmpeg-codec-pvz` inspects PTC Creo View (`ProductView`) `.pvz` packages for
[`cadmpeg-ir`][ir]. A `.pvz` package is a ZIP archive with one `.pvs`
structure member and `.ol` geometry members.

<!-- generated: capability pvz -->

Support: L0 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#creo-view-pvz)).

<!-- /generated: capability pvz -->

The codec is read-only. It detects a package by its `.pvs` member, lists every
member with its role and leading signature, and classifies the package dialect
from the `.pvs` signature. Decode returns a classified document with an empty
model. A decode deeper than the container reports the blocking loss
`pvz/package.content-undecoded`: the structure and geometry members are
located, not decoded.

## Install

```sh
cargo add cadmpeg-codec-pvz cadmpeg-ir cadmpeg-core
```

## Inspect a package

```rust,no_run
use std::fs::File;

use cadmpeg_codec_pvz::PvzCodec;
use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::Codec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("assembly.pvz")?;
    let summary = PvzCodec.inspect(&mut input, &InspectOptions::default())?;
    for entry in &summary.entries {
        println!(
            "{} {} {}",
            entry.role,
            entry.name,
            entry.attributes.get("signature").map_or("-", String::as_str)
        );
    }
    Ok(())
}
```

## Detection

| Evidence | Confidence |
| --- | --- |
| The first ZIP member is a `.pvs` member that opens with a known structure signature | High |
| The ZIP names a `.pvs` member elsewhere | Medium |
| Any other ZIP | Low |

## Documentation

- [API documentation][docs]
- [Format support][support]
- [Architecture and crate map][architecture]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

PTC, Creo, Creo View, and other product names are trademarks of their
respective owners. cadmpeg uses them only to identify the file formats this
codec targets and is not affiliated with, endorsed by, or sponsored by any CAD
vendor. See the [clean-room and legal policy][legal].

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[docs]: https://docs.rs/cadmpeg-codec-pvz
[ir]: https://docs.rs/cadmpeg-ir
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md
