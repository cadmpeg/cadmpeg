# cadmpeg-codec-nx

`cadmpeg-codec-nx` reads Siemens NX `.prt` files stored as SPLMSSTR containers
into [`CadIr`][ir]. It detects the container by its `SPLMSSTR` signature,
extracts zlib-compressed Parasolid neutral-binary streams from the canonical
part payload, and decodes supported geometry and topology.

<!-- generated: capability -->

Support: L1 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#siemens-nx-prt)).

<!-- /generated: capability -->

Connected B-rep on selected or terminal-lineage-resolved body images shows as
extras. `RMFastLoad` retains every body whose complete
nonempty topology node-ID set is covered by the active object-ID set; when no
body has complete membership, selection declines and terminal lineage can
resolve the images.

## Install

```sh
cargo add cadmpeg-codec-nx cadmpeg-ir
```

## Decode

```rust,no_run
use cadmpeg_codec_nx::NxCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.prt")?;
    let result = NxCodec.decode(&mut input, &DecodeOptions::default())?;

    println!(
        "{} bodies, {} surfaces",
        result.ir().model.bodies.len(),
        result.ir().model.surfaces.len()
    );
    Ok(())
}
```

The result holds the decoded `CadIr` and a [`DecodeReport`][report]. Read
`report.losses` before trusting geometry. Call `NxCodec::inspect` for container
metadata without entity decode. It lists SPLMSSTR directory entries and
classifies embedded streams as partition, deltas, plain cached body, or preview
data. Set `DecodeOptions::container_only` for metadata IR without entity
decoding.

Run the capability profiler against a directory of `.prt` files for a
deterministic JSON census:

```sh
cargo run -p cadmpeg-codec-nx --bin nx_profile -- FIXTURES OUTPUT.json
```

## Data model

NX stores part geometry in one or more Parasolid streams inside an SPLMSSTR
container. The decoder inflates each stream, converts Parasolid metre values to
the millimetre-based IR, and retains the inflated stream as an unknown record
for provenance and passthrough.

Typed transfer covers analytic carriers, NURBS, selected trimmed curves, and
connected topology when fixed-record references resolve. Geometry that cannot
be attached remains available through derived free topology. Partition and
adjacent equal-schema deltas streams are scanned together. Exactly keyed full
records and tombstones use the last event for each key. Unmatched tombstones
remain unresolved. `RMFastLoad` intersects membership IDs with topology node
IDs and retains every image whose complete nonempty set is covered; otherwise
selection declines and complete primary-writer lineage falls back to terminal
partition images. Assembly files can contain only external child-part
references and produce no inline geometry.

Embedded JT shape-LOD segments transfer as display tessellation. Validated
embedded JPEG previews and TIFF material textures transfer as exact document
assets. Ordered feature-operation records, body dependencies, Boolean
operations, sketch record lanes, named arrangements and configurations, part
attributes, external dependency inspection, and numeric expressions transfer.
Coverage detail lives in the [format-support profile][support]. Byte semantics
live in the [format notes][spec].

## Documentation

- [API documentation][docs]
- [Format support][support]
- [Format notes][spec]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

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
