# cadmpeg-step

`cadmpeg-step` writes `CadIr` documents as ISO 10303-21 STEP AP214 files. Use
it when you want to export a model without calling the cadmpeg CLI.

## Install

```sh
cargo add cadmpeg-step cadmpeg-ir
```

## Use

```rust
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;
use cadmpeg_step::{write_step, StepWriteOptions};

let ir = CadIr::empty(Units::default());
let mut output = Vec::new();
let report = write_step(&ir, &mut output, &StepWriteOptions::default())?;

assert_eq!(report.error_count(), 0);
assert!(output.starts_with(b"ISO-10303-21;"));

# Ok::<(), cadmpeg_step::StepError>(())
```

## Coverage

The writer handles solids, shells, faces, loops, edges, vertices, common
analytic geometry, and B-spline curves and surfaces. It also writes the product
and unit records expected by STEP readers.

The writer does not map every `CadIr` field to STEP AP214. `StepReport` lists
entities it skipped or reduced so callers can decide whether to keep the file.

## Documentation

- [API documentation][docs]
- [Format support][support]
- [Architecture and crate map][architecture]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[docs]: https://docs.rs/cadmpeg-step
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#step-ap214
