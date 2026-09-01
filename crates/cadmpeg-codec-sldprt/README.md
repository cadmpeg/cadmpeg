# cadmpeg-codec-sldprt

`cadmpeg-codec-sldprt` reads SolidWorks part documents into
[`cadmpeg-ir`][ir] and writes supported IR changes back to `.sldprt`. It
transfers B-rep topology, analytic and NURBS carriers, display meshes,
appearances, selected document attributes, Keywords XML feature history, and
ResolvedFeatures sketch-entity records.

<!-- generated: capability sldprt -->

Support: L1 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#solidworks-sldprt)).

<!-- /generated: capability sldprt -->

## Install

```sh
cargo add cadmpeg-codec-sldprt cadmpeg-ir
```

## Decode

```rust,no_run
use std::fs::File;

use cadmpeg_codec_sldprt::SldprtCodec;
use cadmpeg_ir::{Codec, DecodeOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.sldprt")?;
    let decoded = SldprtCodec.decode(&mut input, &DecodeOptions::default())?;

    println!(
        "{} bodies, {} faces, {} diagnostics",
        decoded.ir().model.bodies.len(),
        decoded.ir().model.faces.len(),
        decoded.report().losses.len(),
    );
    Ok(())
}
```

Read `decoded.report()` before trusting geometry. A successful call can return a
partial model with warnings. Unsupported surface and curve carriers retain
their topology as opaque geometry linked to preserved source bytes. If no
Parasolid body stream produces a graph, the result contains container metadata
and blocking geometry diagnostics.

Set `DecodeOptions::container_only` to skip geometry. `Codec::inspect` offers a
lighter inventory of compressed blocks, section-directory entries, cache
cells, payload families, and embedded Parasolid schemas.

## Data model

An `.sldprt` file contains an outer header, raw-DEFLATE blocks protected by
CRC-32, a cache-cell grid, and a tail section directory. Blocks can contain
Parasolid streams, XML, SW Objects records, previews, tessellation, or opaque
payloads.

The decoder groups related Parasolid `partition` and `deltas` body streams by
site, excluding ghost and ResolvedFeatures sections. It decodes each site,
selects the active configuration's source partition when identified, and
merges alternate sites as configuration-specific bodies. A sole non-ghost
partition is active when no configuration supplies a source index. Multiple
unselected partitions retain site-qualified model identities and leave active
geometry identity unresolved. Attribute-id references resolve into the
`CadIr` topology and geometry arenas. Parasolid model lengths use metres;
`CadIr` geometry uses the document’s IR units and decoded coordinates are
expressed in millimetres. Provenance and exactness annotations identify source
streams, record offsets, and derived entities such as reconstructed pcurves and
periodic seams.

Typed transfer covers analytic carriers, NURBS, swept and spun surfaces that
resolve to NURBS, recursive offset surfaces, constant-radius rolling-ball
blends, variable-radius blend result faces through solved NURBS carriers, and
validated surface-intersection curves. Other unsupported families remain
opaque. Procedural constructions retain typed and opaque support surfaces even
when no topological face owns the support. The decode report records opaque
carriers, synthetic body grouping, trim reconstruction limits, and appearance
ambiguity.

## Encode

```rust,no_run
use std::fs::File;

use cadmpeg_codec_sldprt::SldprtCodec;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::{Codec, DecodeOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.sldprt")?;
    let decoded = SldprtCodec.decode(&mut input, &DecodeOptions::default())?;

    // Edit supported fields in decoded.ir().

    let mut output = File::create("part-edited.sldprt")?;
    SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )?
        .write_to(&mut output)?;
    Ok(())
}
```

`SldprtCodec` implements `Encoder` through `plan` → `write_to`. Encoding with
the retained `source_fidelity` sidecar replays an unchanged source image byte
for byte after an integrity check, and patches supported edits in place.
Geometry-only changes may retain or patch the native Parasolid partition when
the entity graph and provenance permit it. Encoding without that sidecar, or
when the retained image cannot be replayed, regenerates the container and
semantic records for the supported source-less profile.

Retained writing can synchronize supported feature, sketch, parameter,
configuration, active-configuration XML, and PMI edits while rejecting
structural edits it cannot safely rewrite.

Semantic regeneration accepts solid bodies with at most five regions and at
most six shells per solid region. Sheet regions require exactly one shell. It
writes analytic and non-periodic NURBS geometry, body and face base colors,
selected document attributes, sequential triangle strips, feature history, and
retained feature-input payloads. Unsupported IR shapes return
`CodecError::NotImplemented`; malformed references and invalid retained data
return `CodecError::Malformed`. Body transforms must be right-handed and rigid
because the writer bakes them into model-space geometry.

## Links

- [API documentation][docs]
- [Format support][support]
- [Format notes][spec]
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
