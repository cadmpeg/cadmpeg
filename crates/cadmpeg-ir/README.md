# cadmpeg-ir

`cadmpeg-ir` defines the format-neutral document exchanged by cadmpeg codecs.
It provides the `CadIr` data model, codec interfaces, validation, structural
diffing, JSON serialization, and explicit representations of source fidelity
and decode loss.

## Install

```sh
cargo add cadmpeg-ir
```

## Model

A `CadIr` document contains:

- canonical units and document tolerances;
- flat, ID-referenced arenas for topology, geometry, construction features,
  tessellation, appearance, and source attributes;
- sparse provenance and exactness annotations;
- independently versioned source-native namespaces;
- retained records whose bytes could not be mapped to typed entities.

The topology graph follows
`body → region → shell → face → loop → coedge → edge → vertex`. Faces reference
surface carriers, edges reference curve carriers, coedges may reference
parameter-space curves, and vertices reference points. IDs are globally unique
within a document. Arena order is canonical after `CadIr::finalize`.

Coordinates and linear quantities use millimeters. Angular quantities use
radians. Constructors do not enforce document invariants; call `validate`
after construction or transformation.

## Construct and consume a document

Create and validate an empty current-version document:

```rust
use cadmpeg_ir::units::Units;
use cadmpeg_ir::{validate, CadIr};

let mut ir = CadIr::empty(Units::default());
// Populate ir.model arenas and use typed IDs to connect entities.
ir.finalize();
let report = validate(&ir, Vec::new());

assert!(report.is_ok());
assert_eq!(ir.ir_version, cadmpeg_ir::IR_VERSION);
```

`CadIr::to_canonical_json` emits pretty JSON after the caller establishes
canonical arena order. `CadIr::from_json` parses only the IR version supported
by the current crate. `diff` compares units, tolerances, annotations, and entity
arenas by stable identity.

Format crates implement the object-safe `Codec` trait. A consumer can select a
codec by detection confidence, inspect a container without decoding geometry,
then decode the selected source:

```rust
use cadmpeg_ir::{Codec, Confidence};

fn accepts(codec: &dyn Codec, prefix: &[u8]) -> bool {
    codec.detect(prefix) >= Confidence::Medium
}
```

`DecodeResult` contains the finalized document and a `DecodeReport`.
`CodecError` represents operation failure such as wrong format, malformed
container, unsupported capability, or I/O failure. Successful decoding can
still be incomplete: `LossNote` records transferred information that was
approximated or omitted, while `UnknownRecord` retains an uninterpreted source
record by location, digest, links, and optional bytes.

Entity and field fidelity belongs in `Annotations`. Missing exactness entries
mean byte-exact. Other entries distinguish deterministic derivation, inference,
and unknown origin. Provenance entries identify source streams and byte
offsets.

## Scope

IR version 1 covers B-rep topology, analytic and NURBS geometry, procedural
construction links, tessellation, appearance, attributes, and neutral feature
records. Native namespaces retain format-specific design and history records.
Assembly instancing, component trees, and joint constraints are reserved.

## Documentation

- [API documentation][docs]
- [CAD IR version 1][ir-spec]
- [Architecture and crate map][architecture]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[docs]: https://docs.rs/cadmpeg-ir
[ir-spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/cad-ir.md
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
