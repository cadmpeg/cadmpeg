# cadmpeg-codec-inventor

`cadmpeg-codec-inventor` reads Autodesk Inventor `.ipt` and `.iam` documents
into [`cadmpeg-ir`][ir]. It owns the Inventor compound-file, RSe, OLE property,
Protein, external-reference, presentation, and design-record layers. Supported
part kernel carriers transfer through [`cadmpeg-asm`][asm]. The codec is
read-only: it has no Inventor writer, replay path, or patch path.

<!-- generated: capability -->

Support: depth none, breadth n/a ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#autodesk-inventor-ipt-and-iam)).

<!-- /generated: capability -->

The primary structural envelope is CFB v3, RSe schema 31, Meta Stream 8. ACIS
217/218 part carriers show as extras. The finite support claims and extras are
maintained in the [format-support profile][support].

## Install

```sh
cargo add cadmpeg-codec-inventor cadmpeg-ir
```

## Decode

```rust,no_run
use std::fs::File;

use cadmpeg_codec_inventor::InventorCodec;
use cadmpeg_ir::{Codec, DecodeOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.ipt")?;
    let decoded = InventorCodec.decode(&mut input, &DecodeOptions::default())?;

    println!(
        "{} bodies, {} faces, {} losses",
        decoded.ir().model.bodies.len(),
        decoded.ir().model.faces.len(),
        decoded.report().losses.len(),
    );
    for loss in &decoded.report().losses {
        eprintln!("{:?}: {}", loss.severity, loss.message);
    }
    Ok(())
}
```

Read [`DecodeReport::losses`][decode-report] before trusting geometry. A
successful decode can be structurally useful while retaining an unsupported
carrier or semantic branch. Blocking losses are rejected by strict decode
policy. Set `DecodeOptions::container_only` when only the Inventor hierarchy,
metadata, and native records are required; container-only decoding does not
inflate bulk geometry streams.

`InventorCodec::inspect` returns the complete compound hierarchy and bounded
container facts without transferring geometry. The public
[`validate_native`][validate-native] function validates the typed
`inventor` native namespace; the CLI also runs shared IR validation on decoded
geometry.

## Container and RSe model

Inventor documents are Compound File Binary containers. Detection follows the
CFB directory structure and requires structurally reached Inventor evidence;
an arbitrary CFB file is not classified as Inventor. The codec uses the shared
lazy, budgeted compound reader for DIFAT, FAT, mini-FAT, directory trees,
regular streams, mini streams, and physical allocation facts.

The primary semantic envelope is:

- CFB major version 3 with 512-byte sectors;
- RSe database schema 31 and a dynamically selected `V<n>/RSeDb` storage;
- Meta Stream version 8;
- exact zlib framing for paired `M<token>` and `B<token>` streams;
- versioned RSe metadata tables and B-record trailers;
- part and assembly document kinds.

These are the grammars the codec implements, not an admission gate. A document
declaring another `RSeDb` schema, metadata marker, or metadata version is read
with them anyway; a stream they cannot frame degrades to an unavailable stream
with its own issue record, and the declaration makes the document
`inventor:unknown` with a `source.dialect-unverified` charge.

The parser enumerates database candidates, registry and revision records, exact
M/B token pairs, metadata type and block tables, and every typed bulk record.
Segment selection is based on registry and metadata identity, not fixed stream
offsets, segment count, display name, storage-band order, or kernel magic
searches. Unknown and optional segments remain named native records with
coverage and loss accounting.

CFB v4, other RSe schemas, other Meta Stream versions, and other record-trailer
variants are separate envelopes. The codec reports or retains those branches
only where the surrounding structure can be established; it does not silently
apply the primary grammar.

## Decoded content

The decode result combines neutral IR with the `inventor` native namespace.

| Layer        | Transfer                                                                                                                                                                                               |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Container    | CFB hierarchy, stream sizes, allocation facts, segment pairs, and physical ranges.                                                                                                                     |
| RSe          | Database identity, registry, revisions, Meta Stream tables, exact B-record frames, typed record identities, and bounded unknown records.                                                               |
| Properties   | OLE property sets, FMTIDs, code pages supported by the decoder, typed scalar values, thumbnails with recognized image signatures, and unmapped native properties.                                      |
| Protein      | Absent, empty, or packaged state; safe ZIP entry inventory; schema-driven asset records and rejected instance positions through `cadmpeg-protein`.                                                     |
| References   | `UFRxDoc` external document identities, persisted paths, model states, occurrence identifiers, and unresolved external prototypes. The codec never opens those paths.                                  |
| Presentation | Document-default appearance and supported body/face style joins, with explicit losses for unresolved assignments.                                                                                      |
| Design       | Parameters, planar sketch graphs, typed feature records, and closed extrude, hole, constant-radius fillet, and equal-distance chamfer branches when every typed operand and result-body join resolves. |

## Part geometry

The active part path selects one typed kernel-carrier record in the sole
`PmBRep` segment. It frames the record from RSe metadata, validates the
carrier-specific footer and kernel header, then transfers through the shared
ASM/ACIS decoder with segment- and carrier-qualified identities.

ACIS binary carriers use the 32-bit ACIS header and SAB grammar at every save
format. Majors 217 and 218 are the verified bands; a carrier outside them is
framed and decoded the same way, reports the non-primary `acis:` layer as
`Admission::AdmittedUnverified` naming the nearer verified band, and charges
`source.dialect-unverified`. The embedded carrier and the exact extracted carrier
use the same decoder and must produce equal normalized geometry and validation
findings. Other ACIS save-format bands remain retained carriers and produce a
blocking `geometry_not_transferred` loss.

ASM carriers are retained and transferred where their admitted kernel and
validation envelope permits it. A carrier signature validates an already typed
RSe record; it never locates a carrier. Empty, malformed, ambiguous, or
unsupported active carriers retain bounded source identity and report the
corresponding blocking loss.

## Assemblies and external references

The codec transfers the supported `UFRxDoc` and `Am*` occurrence branch without
filesystem access. An occurrence joins its external prototype through the
persisted file-reference identifier and joins its `AmDc` identity to an active
finite `AmGraphics` placement. Placement translations are converted from
centimetres to neutral millimetres. Suppressed occurrences transfer hidden
visibility; a suppressed occurrence without graphics placement may use the
identity transform.

External prototypes remain unresolved `PrototypeReference` values. Local
prototypes, nested parent links, exceptional transform branches, and project
resolution are outside the current semantic envelope. A future resolver must
be an explicit operation above the codec with user-supplied roots, identity
checks, cycle limits, and deterministic reference handling.

## Losses and unsupported content

The codec distinguishes structural refusal from semantic loss. It retains
typed native records, exact ranges, digests, and bounded unknown bytes when
they are needed for source identity. It reports machine-readable losses for
unsupported geometry, metadata, appearance, design-history, placement, and
external-component transfer.

The following are deliberately separate open investigations rather than
heuristic fallbacks:

- several coherent RSe databases and untyped metadata sections;
- multiple active or historical part carriers;
- additional OLE code pages and preview/property variants;
- local assembly prototypes, parent links, and exceptional placements;
- additional appearance channels and assignment precedence;
- suppression, dependency, and result-body joins for the remaining feature
  families;
- CFB v4 Inventor envelopes, non-v8 metadata, and IDW/IPN semantics.

See [Inventor open items][open-items] for the exact boundary and required
evidence for each branch.

## Documentation

- [API documentation][docs]
- [Format support][support]
- [Inventor format notes][spec]
- [Inventor open items][open-items]
- [Architecture and crate map][architecture]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

Autodesk, Inventor, and other product names are trademarks of their respective
owners. cadmpeg uses them only to identify the file formats this codec targets
and is not affiliated with, endorsed by, or sponsored by any CAD vendor. See
the [clean-room and legal policy][legal].

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[asm]: https://docs.rs/cadmpeg-asm
[decode-report]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/report/struct.DecodeReport.html
[docs]: https://docs.rs/cadmpeg-codec-inventor
[ir]: https://docs.rs/cadmpeg-ir
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[open-items]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor-open-items.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md
[validate-native]: https://docs.rs/cadmpeg-codec-inventor/latest/cadmpeg_codec_inventor/fn.validate_native.html
