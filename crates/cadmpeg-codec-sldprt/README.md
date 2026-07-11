# cadmpeg-codec-sldprt

`cadmpeg-codec-sldprt` reads SolidWorks part documents into
[`cadmpeg-ir`][ir] and writes supported IR changes back to `.sldprt`. It
transfers B-rep topology, analytic and NURBS carriers, display meshes,
appearances, selected document attributes, Keywords XML feature history, and
ResolvedFeatures sketch-entity records.

The crate handles part documents. It does not model SolidWorks assemblies or
assembly constraints.

Support level: [L3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder) on the cadmpeg support ladder.

## Decode a part

```sh
cargo add cadmpeg-codec-sldprt cadmpeg-ir
```

```rust,no_run
use std::fs::File;

use cadmpeg_codec_sldprt::SldprtCodec;
use cadmpeg_ir::{Codec, DecodeOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.sldprt")?;
    let decoded = SldprtCodec.decode(&mut input, &DecodeOptions::default())?;

    println!(
        "{} bodies, {} faces, {} diagnostics",
        decoded.ir.model.bodies.len(),
        decoded.ir.model.faces.len(),
        decoded.report.losses.len(),
    );
    Ok(())
}
```

Read `decoded.report` before consuming geometry. A successful call can return a
partial model with warnings. Unsupported surface and curve carriers retain
their topology as opaque geometry linked to preserved source bytes. If no
Parasolid body stream produces a graph, the result contains container metadata
and blocking geometry diagnostics.

Set `DecodeOptions::container_only` to skip geometry. `Codec::inspect` offers a
lighter inventory of compressed blocks, section-directory entries, cache
cells, payload families, and embedded Parasolid schemas.

## Data and format model

An `.sldprt` file contains an outer header, raw-DEFLATE blocks protected by
CRC-32, a cache-cell grid, and a tail section directory. Blocks can contain
Parasolid streams, XML, SW Objects records, previews, tessellation, or opaque
payloads.

The decoder selects related Parasolid `partition` and `deltas` body streams,
resolves their attribute-id references, and builds the `CadIr` body, region,
shell, face, loop, coedge, edge, vertex, point, surface, and curve arenas.
Parasolid model lengths use metres; `CadIr` geometry uses the document’s IR
units and decoded coordinates are expressed in millimetres. Provenance and
exactness annotations identify source streams, record offsets, and derived
entities such as reconstructed pcurves and periodic seams.

Supported decoded curves include lines, circles, ellipses, and NURBS. Supported
surfaces include planes, cylinders, cones, spheres, tori, and NURBS. The codec
derives pcurves for supported planar, cylindrical, spherical, and matching
NURBS-boundary cases. The decode report records opaque carriers, synthetic body
grouping, trim reconstruction limits, and appearance ambiguity.

## Write a part

`SldprtCodec` implements `Encoder`; `encode` and `write_preserved` use the same
writer. Unchanged decoded IR replays the retained source image byte for byte
after an integrity check. Geometry-only changes may retain or patch the native
Parasolid partition when the entity graph and provenance permit it. Other
supported changes regenerate the container and semantic records.

Semantic regeneration accepts solid and sheet bodies with one region per body
and one shell per region. It writes analytic and non-periodic NURBS geometry,
body and face base colors, selected document attributes, sequential triangle
strips, feature history, and retained feature-input payloads. Unsupported IR
shapes return `CodecError::NotImplemented`; malformed references and invalid
retained data return `CodecError::Malformed`. Body transforms must be
right-handed and rigid because the writer bakes them into model-space
geometry.

Consult the [format support matrix][support] for the current coverage boundary
and the [format specification][spec] for byte-level details.

## Links

- [API documentation][docs]
- [Repository][repo]
- [Clean-room and legal policy][legal]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

SolidWorks and Parasolid and other product names are trademarks of their
respective owners. cadmpeg uses them only to identify the file formats this
codec targets and is not affiliated with, endorsed by, or sponsored by any CAD
vendor. See the [clean-room and legal policy][legal].

[docs]: https://docs.rs/cadmpeg-codec-sldprt
[ir]: https://docs.rs/cadmpeg-ir
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#solidworks-sldprt
