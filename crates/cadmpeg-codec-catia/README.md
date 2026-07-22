# cadmpeg-codec-catia

`cadmpeg-codec-catia` reads CATIA V5 `.CATPart` files into
[`CadIr`](https://docs.rs/cadmpeg-ir). It recognizes the `V5_CFV2` container
layouts used by CATPart files and decodes supported analytic surfaces, NURBS
surfaces, curves, vertices, and B-rep topology.

Support level: [L2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder) on the cadmpeg support ladder for the standard-nested layout; other layout bands remain L1 because their connected topology support is conditional rather than band-wide.

Add the codec and IR crates:

```sh
cargo add cadmpeg-codec-catia cadmpeg-ir
```

Decode a part with the shared codec interface:

```rust,no_run
use std::fs::File;

use cadmpeg_codec_catia::CatiaCodec;
use cadmpeg_ir::{Codec, DecodeOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.CATPart")?;
    let decoded = CatiaCodec.decode(&mut input, &DecodeOptions::default())?;

    println!(
        "{} bodies, {} surfaces",
        decoded.ir.model.bodies.len(),
        decoded.ir.model.surfaces.len()
    );
    for loss in &decoded.report.losses {
        eprintln!("{:?}: {}", loss.severity, loss.message);
    }
    Ok(())
}
```

The decode report is part of the result. Check it before assuming that every
native relationship or attribute has an IR representation.

## Storage and model coverage

A CATPart starts with an outer `V5_CFV2` container. Most files also contain a
nested `V5_CFV2` directory whose physical extents reconstruct logical streams
such as `MainDataStream` and `SurfacicReps`. The codec identifies the storage
variant before selecting a record decoder.

Standard nested parts have the broadest model coverage. The decoder emits
analytic carrier surfaces and vertices, binds faces when stored senses resolve,
and emits loops, coedges, edges, and endpoint assignments when the trim,
support, and vertex tables form a complete unambiguous graph. FBB-only,
zero-entity, E5, and object-stream layouts can yield analytic or NURBS carriers
and selected edge bindings. Unresolved native bytes remain attached to the IR
as unknown records, and the report describes missing geometry, topology, or
attributes.

Use `CatiaCodec::inspect` to identify the storage variant and list catalogued
logical streams without decoding entities. Set `DecodeOptions::container_only`
when only source metadata and container diagnostics are needed.

The crate reads parts only. It does not write `.CATPart` files or decode
assemblies, design history, tessellation, appearances, materials, persistent
object tags, or general document metadata beyond the embedded JPEG preview. The [format support matrix][support]
tracks coverage by model layer.

## Reference

- [API documentation][docs]
- [CATIA format model][spec]
- [Format support matrix][support]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

CATIA V5 and other product names are trademarks of their respective owners.
cadmpeg uses them only to identify the file formats this codec targets and is
not affiliated with, endorsed by, or sponsored by any CAD vendor. See the
[clean-room and legal policy][legal].

[docs]: https://docs.rs/cadmpeg-codec-catia
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#catia-v5-catpart
