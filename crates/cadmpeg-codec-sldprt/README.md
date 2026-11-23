# cadmpeg-codec-sldprt

`cadmpeg-codec-sldprt` reads SolidWorks part documents, and the component
structure of assembly documents, into [`cadmpeg-ir`][ir] and writes supported
IR changes back to `.sldprt`. It
transfers B-rep topology, analytic and NURBS carriers, display meshes,
appearances, selected document attributes, Keywords XML feature history, and
ResolvedFeatures sketch-entity records.

<!-- generated: capability sldprt -->

Support: L3 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#solidworks-sldprt)).

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
        decoded.ir().geometry.bodies.len(),
        decoded.ir().geometry.faces.len(),
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

Set `DecodeOptions::depth` to `DecodeDepth::Container` to skip geometry.
`Codec::inspect` offers a
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
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
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
the entity graph and provenance permit it. Container regeneration requires a
retained envelope with validated local, central-directory, and end-of-directory
signature tokens. Source-less IR and unproved envelopes are refused before
output is written.

Retained writing can synchronize supported feature, sketch, parameter,
configuration, active-configuration XML, and PMI edits while rejecting
structural edits it cannot safely rewrite. The writer has no modeling kernel.
An edit of a feature, a parameter, a suppression state, or a configuration
state that shapes the solid is written as edited over the stored solid, and a
`writer.solid-stale-after-edit` loss names each edited feature and states that
the solid shows the state before the edit. The writer does not write the active
configuration snapshot that the decoder synthesizes from the model; an edit of
that snapshot which the model does not state gets a
`writer.configuration-snapshot-edit-unwritten` loss. An edit that the native
records cannot carry keeps the stored record or value: an edit of a
display-only relation parameter or a parameter value that the written
expression does not state gets a `writer.parameter-edit-unwritten` loss, and
an edited compact selection, a feature definition that the encoder cannot
restate, or a construction edit that the written part, decoded again, does not
read back as edited gets a `writer.feature-edit-unwritten` loss, and the
feature is written with its stored definition record. A feature that keeps its
stored definition record keeps its stored operands and source properties too,
and its loss names each edit of its definition, dependencies and source
properties that the written part does not read back as edited. A record field
that no edit moves keeps its stored state: an unedited feature or parameter is
written as stored, and a rename changes only the name and the references that
name it. The decoder fills some field groups of a record from the
feature-input lane as one unit while the record states none of the group's
gate (a revolution axis; a coordinate-system or reference-axis frame; a
reference-plane frame with the plane's reference; a move-face translation; an
extrusion's end conditions and depths; the two directions of a linear
pattern); an edit of one member writes every member the encoding states. An
extrusion depth is written under the dimension name the record holds. The
full decoder reads the written part again, and the read-back compares the whole
document with it: every record of every arena of the geometry, model, design
and process homes, the units, the tolerances and the source attributes. The
native namespaces hold the encoding, and the read-back leaves them out. A
write moves the block and lane offsets and the history indices that ids embed,
so records pair by content: a record pairs with the record whose wire form is
the same once each paired id is replaced by its pair. The edit accounts for
the records the caller changed, the expression, value and display of a
parameter of an edited feature, the design records and configuration entries
keyed by an edited record, the definition of a feature that owns an edited
parameter, and text that names a renamed feature. The source part written with
no edit and decoded again gives the differences that every write of the part
has, and those are accounted for as well. Any other difference (a sibling
default plane that changes kind, a sketch on an edited plane that moves) is
caused by a written edit: that feature is written with its stored definition
record, and its `writer.feature-edit-unwritten` loss names each differing
record and the place in it. With several written edits, each one in turn is
withheld alone, and the one whose withholding removes the difference is kept
as stored; when no single edit removes it, every written edit is. An edited
feature that reads back with a parameter added or removed or a source property
missing is written with its stored definition record in the same way. A
variant edit (a change of termination kind, of a list, or of a field that is
present or absent) writes the record in the shape of the new variant, and a
`writer.feature-record-reshaped` loss names each parameter that shape adds or
drops and each source property it drops. A difference that remains with every edit kept as stored, or that a
parameter or rename edit causes, is named in a `writer.feature-edit-unwritten`
loss.
The writer has no compound-file serializer: an edited compound-file part is
written as its stored part, and a `writer.compound-file-edit-unwritten` loss
(severity error) states that no edit is written. `cadmpeg convert
--reject-lossy=export` refuses such a write; `--reject-errors` does not, because
it judges the written file's check findings, not export losses.

With a validated retained envelope, semantic regeneration accepts solid bodies with at most five regions and at
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
