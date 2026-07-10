# cadmpeg-ir

`cadmpeg-ir` is the common data model used by every cadmpeg codec. Use it to
work with decoded CAD data without coupling your code to one source format.

The crate includes the `CadIr` document type, codec traits, canonical JSON,
validation, structural diffing, and source annotations.

## Install

```sh
cargo add cadmpeg-ir
```

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

Format crates implement the object-safe `Codec` trait:

```rust
use cadmpeg_ir::{Codec, Confidence};

fn accepts(codec: &dyn Codec, prefix: &[u8]) -> bool {
    codec.detect(prefix) >= Confidence::Medium
}
```

## Scope

Version 1 covers units, tolerances, B-rep topology, analytic and spline
geometry, tessellation, appearance, and design records. Entity IDs connect flat
arenas, which keeps serialized documents stable and easy to diff.

Assembly structure is reserved. Feature history currently stores ordered
operations rather than a model that cadmpeg can replay. `UnknownRecord` keeps
source records that a codec cannot map yet.

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
