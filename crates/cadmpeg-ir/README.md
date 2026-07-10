# cadmpeg-ir

**The shared, provenance-aware model behind the cadmpeg CAD pipeline.**

`cadmpeg-ir` defines the versioned CAD intermediate representation, codec
traits, canonical JSON serialization, validation, structural diffing, source
annotations, and explicit loss reports used by every cadmpeg format crate.

> cadmpeg is early software. The IR is versioned and validated, but several
> semantic areas remain reserved or partial. Assembly structure is reserved,
> while feature history currently represents ordered source-provenanced
> operations.

## Install

```sh
cargo add cadmpeg-ir
```

cadmpeg requires Rust 1.88 or later.

## Use

Create and validate an empty current-version document:

```rust
use cadmpeg_ir::units::Units;
use cadmpeg_ir::{validate, CadIr};

let ir = CadIr::empty(Units::default());
let report = validate(&ir);

assert_eq!(report.error_count(), 0);
assert_eq!(ir.ir_version, cadmpeg_ir::IR_VERSION);
```

Format plugins implement the object-safe `Codec` trait:

```rust
use cadmpeg_ir::{Codec, Confidence};

fn accepts(codec: &dyn Codec, prefix: &[u8]) -> bool {
    codec.detect(prefix) >= Confidence::Medium
}
```

## Model

`CadIr` stores canonical units and tolerances, a flat ID-referenced B-rep graph,
geometry carriers, tessellation, appearance, design records, sparse provenance
annotations, source-native namespaces, and opaque records. Recognized content
that cannot be represented remains explicit in `DecodeReport` or
`UnknownRecord`; it is not silently discarded.

## Project links

- [API documentation][docs]
- [CAD IR version 1][ir-spec]
- [Architecture and crate map][architecture]
- [Repository][repo]
- [Clean-room and legal policy][legal]

Code is licensed under the Apache License 2.0.

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[docs]: https://docs.rs/cadmpeg-ir
[ir-spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/cad-ir.md
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
