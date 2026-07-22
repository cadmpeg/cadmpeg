# cadmpeg-codec-nx

`cadmpeg-codec-nx` reads Siemens NX `.prt` files stored as SPLMSSTR containers
into [`CadIr`][ir]. It detects the container by its `SPLMSSTR` signature,
extracts zlib-compressed Parasolid neutral-binary streams from the canonical
part payload, and decodes supported geometry and topology. It does not read
Creo files, which also use the `.prt` extension.

Support level: [L4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder)
for single-body, `RMFastLoad`-selected, and terminal-lineage-resolved body images;
L2 for unresolved multi-partition history.

```sh
cargo add cadmpeg-codec-nx cadmpeg-ir
```

```rust,no_run
use cadmpeg_codec_nx::NxCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.prt")?;
    let result = NxCodec.decode(&mut input, &DecodeOptions::default())?;

    println!(
        "{} bodies, {} surfaces",
        result.ir.model.bodies.len(),
        result.ir.model.surfaces.len()
    );
    Ok(())
}
```

The result contains the decoded model in `result.ir` and a
[`DecodeReport`][report] in `result.report`. Check the report before treating
the model as complete. It records missing topology, unresolved edit overlays,
unsupported entity families, and NX metadata that was not transferred.

Call `NxCodec::inspect` when you need container metadata without entity decode.
It lists SPLMSSTR directory entries and classifies embedded streams as
partition, deltas, plain cached body, or preview data. Set
`DecodeOptions::container_only` to produce metadata IR without decoding
entities.

## Data model and coverage

NX stores part geometry in one or more Parasolid streams inside an SPLMSSTR
container. The decoder inflates each stream, converts Parasolid metre values to
the millimetre-based IR, and retains the inflated stream as an unknown record
for provenance and passthrough.

Supported typed geometry includes points; planes, cylinders, cones, spheres,
and tori; lines, circles, and ellipses; NURBS curves and surfaces; and selected
trimmed curves. The decoder builds body, region, shell, face, loop, coedge,
edge, and vertex topology when its fixed-record references resolve. Geometry
that cannot be attached remains available through derived free topology.

Partition and adjacent equal-schema deltas streams are scanned together.
Exactly keyed full records and tombstones use the last event for each key.
Unmatched tombstones remain unresolved. Segment body aliases, primary-body
writers, and Boolean tool operands select terminal partition images when the
complete body lineage is unambiguous. Assembly files can contain only external
child-part references and therefore produce no inline geometry.

Ordered feature-operation records, body dependencies, Boolean operations,
sketch record lanes, arrangements, part attributes, and numeric expressions
transfer. Complete design history, assembly occurrence placement, materials,
appearances, entity-owned attributes, tessellation, and native `.prt` writing
are not supported. See the [format support matrix][support] for current coverage
and the [format notes][spec] for byte semantics.

The crate also exposes lower-level container, stream, geometry, NURBS, and
topology modules for tools that need inspection or partial decoding. Most
applications should use `NxCodec`.

Requires Rust 1.88 or later. Licensed under Apache-2.0. See the
[API documentation][docs], [repository][repo], and [clean-room and legal
policy][legal].

Siemens NX and Parasolid and other product names are trademarks of their
respective owners. cadmpeg uses them only to identify the file formats this
codec targets and is not affiliated with, endorsed by, or sponsored by any CAD
vendor. See the [clean-room and legal policy][legal].

[docs]: https://docs.rs/cadmpeg-codec-nx
[ir]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/document/struct.CadIr.html
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[report]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/report/struct.DecodeReport.html
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/siemens_nx.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#siemens-nx-prt
